//! W6:按 model_id 分发的路由连接器。
//!
//! 背景:OpenAiConnector 本身只绑定 base_url,model_id/secret_ref 是每次
//! invoke 的入参——多 provider/多模型路由因此不需要动内核回合循环,只需把
//! RuntimeConfig.connector 的单插槽装上本路由器:
//! - 表内命中(model_id → 网关连接器)→ 委派该连接器;
//! - 未命中 → 回落默认连接器(启动装配的服务器默认模型,现状语义);
//! - `invoke_stream` 必须透传委派(trait 默认实现会退化成非流式)。
//!
//! 凭据不变:回合侧照常传 `secret_ref = secret:model.<model_id>`,各模型
//! 的密钥由服务器启动/配置变更时播种进加密密钥库(见 boenmind-server)。

use bm_contract::connector::{InvokeRequest, InvokeResponse};
use bm_core::ports::ModelConnector;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct RoutingConnector {
    inner: std::sync::RwLock<Routes>,
}

struct Routes {
    /// model_id → 该 provider 网关的连接器。
    table: HashMap<String, Arc<dyn ModelConnector>>,
    /// 未命中回落(服务器默认模型)。
    default: Arc<dyn ModelConnector>,
}

impl RoutingConnector {
    pub fn new(default: Arc<dyn ModelConnector>) -> Self {
        Self {
            inner: std::sync::RwLock::new(Routes {
                table: HashMap::new(),
                default,
            }),
        }
    }

    /// 原子换表(默认连接器保留)。provider 配置写后由管理面调用,免重启。
    pub fn replace_table(&self, table: HashMap<String, Arc<dyn ModelConnector>>) {
        let mut g = self.inner.write().expect("路由表锁未中毒");
        g.table = table;
    }

    /// 已路由的模型 id 清单(/v1 校验与观测面用)。
    pub fn known_models(&self) -> Vec<String> {
        let g = self.inner.read().expect("路由表锁未中毒");
        let mut v: Vec<String> = g.table.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn contains(&self, model_id: &str) -> bool {
        self.inner
            .read()
            .expect("路由表锁未中毒")
            .table
            .contains_key(model_id)
    }

    fn route(&self, model_id: &str) -> Arc<dyn ModelConnector> {
        let g = self.inner.read().expect("路由表锁未中毒");
        g.table
            .get(model_id)
            .cloned()
            .unwrap_or_else(|| g.default.clone())
    }
}

#[async_trait::async_trait]
impl ModelConnector for RoutingConnector {
    fn provider(&self) -> &'static str {
        "openai-routing"
    }

    async fn invoke(&self, req: InvokeRequest, cancel: CancellationToken) -> InvokeResponse {
        self.route(&req.model_id).invoke(req, cancel).await
    }

    async fn invoke_stream(
        &self,
        req: InvokeRequest,
        cancel: CancellationToken,
        on_delta: Box<dyn for<'a> FnMut(&'a str) + Send + 'static>,
    ) -> InvokeResponse {
        // 必须覆写:委派目标自己的 invoke_stream(保真流式);trait 默认
        // 实现会退化成非流式整段回调。
        self.route(&req.model_id)
            .invoke_stream(req, cancel, on_delta)
            .await
    }
}

// ---- 测试 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bm_contract::connector::{InvokeParams, InvokeResponse, Usage};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn req(model: &str) -> InvokeRequest {
        InvokeRequest {
            model_id: model.to_string(),
            messages: vec![],
            tools: vec![],
            params: InvokeParams::default(),
            secret_ref: format!("secret:model.{model}"),
            budget_ctx: bm_contract::connector::BudgetCtx {
                operation_id: bm_contract::ids::BmId::generate("op"),
                agent_id: bm_contract::ids::BmId::generate("agt"),
                remaining_tokens: 1000,
            },
            deadline: "2030-01-01T00:00:00+00:00".to_string(),
            attempt: 1,
        }
    }

    /// 记录被调模型的假连接器;invoke_stream 可独立计数。
    #[derive(Default)]
    struct Fake {
        kind: &'static str,
        invocations: AtomicUsize,
        stream_invocations: AtomicUsize,
    }
    impl Fake {
        fn new(kind: &'static str) -> Arc<Self> {
            Arc::new(Self {
                kind,
                invocations: AtomicUsize::new(0),
                stream_invocations: AtomicUsize::new(0),
            })
        }
    }
    #[async_trait::async_trait]
    impl ModelConnector for Fake {
        fn provider(&self) -> &'static str {
            self.kind
        }
        async fn invoke(&self, req: InvokeRequest, _c: CancellationToken) -> InvokeResponse {
            self.invocations.fetch_add(1, Ordering::SeqCst);
            InvokeResponse::Completed {
                content: req.model_id.clone(),
                finish_reason: bm_contract::connector::FinishReason::Stop,
                usage: Usage {
                    tokens_in: 0,
                    tokens_out: 0,
                },
                model_id: req.model_id,
                latency_ms: 0,
                stream_interrupted: false,
                tool_calls: vec![],
            }
        }
        async fn invoke_stream(
            &self,
            req: InvokeRequest,
            _c: CancellationToken,
            mut on_delta: Box<dyn for<'a> FnMut(&'a str) + Send + 'static>,
        ) -> InvokeResponse {
            self.stream_invocations.fetch_add(1, Ordering::SeqCst);
            on_delta(&req.model_id);
            InvokeResponse::Completed {
                content: req.model_id.clone(),
                finish_reason: bm_contract::connector::FinishReason::Stop,
                usage: Usage {
                    tokens_in: 0,
                    tokens_out: 0,
                },
                model_id: req.model_id,
                latency_ms: 0,
                stream_interrupted: false,
                tool_calls: vec![],
            }
        }
    }

    #[tokio::test]
    async fn t_w6_route_by_model_id_and_fallback() {
        let a = Fake::new("a");
        let b = Fake::new("b");
        let default = Fake::new("default");
        let router = RoutingConnector::new(default.clone());
        let mut table = HashMap::new();
        table.insert("model-a".to_string(), a.clone() as Arc<dyn ModelConnector>);
        table.insert("model-b".to_string(), b.clone() as Arc<dyn ModelConnector>);
        router.replace_table(table);

        assert!(router.contains("model-a"));
        assert!(!router.contains("model-x"));
        assert_eq!(router.known_models(), vec!["model-a", "model-b"]);

        let cancel = CancellationToken::new();
        let r = router.invoke(req("model-b"), cancel.clone()).await;
        assert_eq!(
            r,
            InvokeResponse::Completed {
                content: "model-b".into(),
                finish_reason: bm_contract::connector::FinishReason::Stop,
                usage: Usage {
                    tokens_in: 0,
                    tokens_out: 0
                },
                model_id: "model-b".into(),
                latency_ms: 0,
                stream_interrupted: false,
                tool_calls: vec![]
            }
        );
        assert_eq!(b.invocations.load(Ordering::SeqCst), 1);
        assert_eq!(a.invocations.load(Ordering::SeqCst), 0);

        // 未知名 → 回落默认连接器
        let _ = router.invoke(req("model-unknown"), cancel.clone()).await;
        assert_eq!(default.invocations.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn t_w6_stream_delegates_to_routed_target() {
        let a = Fake::new("a");
        let default = Fake::new("default");
        let router = RoutingConnector::new(default.clone());
        let mut table = HashMap::new();
        table.insert("model-a".to_string(), a.clone() as Arc<dyn ModelConnector>);
        router.replace_table(table);

        let cancel = CancellationToken::new();
        let got = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let sink = got.clone();
        let r = router
            .invoke_stream(
                InvokeRequest {
                    deadline: "2030-01-01T00:00:00+00:00".to_string(),
                    ..req("model-a")
                },
                cancel,
                Box::new(move |s: &str| sink.lock().expect("锁未中毒").push_str(s)),
            )
            .await;
        // 流式必须走到目标连接器的 invoke_stream(而非退化),内容即增量
        assert_eq!(
            r,
            InvokeResponse::Completed {
                content: "model-a".into(),
                finish_reason: bm_contract::connector::FinishReason::Stop,
                usage: Usage {
                    tokens_in: 0,
                    tokens_out: 0
                },
                model_id: "model-a".into(),
                latency_ms: 0,
                stream_interrupted: false,
                tool_calls: vec![]
            }
        );
        assert_eq!(*got.lock().expect("锁未中毒"), "model-a");
        assert_eq!(a.stream_invocations.load(Ordering::SeqCst), 1);
        assert_eq!(default.stream_invocations.load(Ordering::SeqCst), 0);
    }
}
