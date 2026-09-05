//! MockConnector:脚本化确定性模型连接器。GT-01 两场景、降级链、INV-4 全部
//! 可回放;延迟是脚本声明的数值,不真实睡眠(性能定标口径,m0/perf-baseline)。

use async_trait::async_trait;
use bm_contract::connector::{FinishReason, InvokeRequest, InvokeResponse, Usage};
use bm_contract::error_codes::ErrorCode;
use bm_core::ports::ModelConnector;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// 单次调用的脚本行为。
#[derive(Debug, Clone)]
pub enum Step {
    /// 成功:返回内容与用量;latency_ms 是声明值,不睡眠。
    Ok {
        content: String,
        tokens_in: u64,
        tokens_out: u64,
        latency_ms: u64,
    },
    /// 失败:可声明是否可重试(如 timeout → true)。
    Fail {
        error_code: ErrorCode,
        retryable: bool,
    },
    /// 真实睡眠 delay_ms 后成功:给 close-during-flight / cancel 竞态类测试
    /// 制造可观测的进行中窗口。
    OkAfter {
        content: String,
        tokens_in: u64,
        tokens_out: u64,
        delay_ms: u64,
    },
}

impl Step {
    pub fn ok(content: impl Into<String>, tokens_in: u64, tokens_out: u64) -> Self {
        Step::Ok {
            content: content.into(),
            tokens_in,
            tokens_out,
            latency_ms: 1873,
        }
    }

    pub fn timeout() -> Self {
        Step::Fail {
            error_code: ErrorCode::Timeout,
            retryable: true,
        }
    }

    /// 真实延迟 success(测试专用制造进行中窗口)。
    pub fn ok_after(content: impl Into<String>, delay_ms: u64) -> Self {
        Step::OkAfter {
            content: content.into(),
            tokens_in: 412,
            tokens_out: 58,
            delay_ms,
        }
    }
}

#[derive(Default)]
struct MockInner {
    script: Vec<Step>,
    cursor: usize,
}

#[derive(Default)]
pub struct MockConnector {
    inner: Mutex<MockInner>,
    /// 脚本耗尽后的默认行为(false = 报 internal 不可重试)。
    loop_last: bool,
}

impl MockConnector {
    /// 取下一步(同步,锁不跨 await)。
    fn next_step(&self) -> Step {
        let mut inner = self.inner.lock().expect("锁未中毒");
        let step: Option<Step> = match inner.script.get(inner.cursor) {
            Some(s) => Some(s.clone()),
            None if self.loop_last => inner.script.last().cloned(),
            None => None,
        };
        inner.cursor += 1;
        step.unwrap_or(Step::Fail {
            error_code: ErrorCode::Internal,
            retryable: false,
        })
    }
    /// 按顺序消费脚本;耗尽后:loop_last=true 则重复最后一步,否则报 internal。
    pub fn new(script: Vec<Step>) -> Self {
        Self {
            inner: Mutex::new(MockInner { script, cursor: 0 }),
            loop_last: false,
        }
    }

    pub fn repeating(step: Step) -> Self {
        Self::new(vec![step]).with_loop_last()
    }

    pub fn with_loop_last(mut self) -> Self {
        self.loop_last = true;
        self
    }
}

#[async_trait]
impl ModelConnector for MockConnector {
    async fn invoke(&self, _req: InvokeRequest, _cancel: CancellationToken) -> InvokeResponse {
        let step = self.next_step();
        match step {
            Step::Ok {
                content,
                tokens_in,
                tokens_out,
                latency_ms,
            } => InvokeResponse::Completed {
                content,
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: Usage {
                    tokens_in,
                    tokens_out,
                    ..Default::default()
                },
                model_id: _req.model_id,
                latency_ms,
                stream_interrupted: false,
            },
            Step::OkAfter {
                content,
                tokens_in,
                tokens_out,
                delay_ms,
            } => {
                let model_id = _req.model_id;
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                InvokeResponse::Completed {
                    content,
                    tool_calls: Vec::new(),
                    finish_reason: FinishReason::Stop,
                    usage: Usage {
                        tokens_in,
                        tokens_out,
                        ..Default::default()
                    },
                    model_id,
                    latency_ms: delay_ms,
                    stream_interrupted: false,
                }
            }
            Step::Fail {
                error_code,
                retryable,
            } => InvokeResponse::Failed {
                error_code,
                retryable,
                attempt: _req.attempt,
                detail_ref: None,
            },
        }
    }

    fn provider(&self) -> &'static str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_contract::connector::{BudgetCtx, InvokeParams};
    use bm_contract::ids::{IdGen, SeqIdGen};

    fn req(attempt: u32) -> InvokeRequest {
        let ids = SeqIdGen::new();
        InvokeRequest {
            model_id: "zhipu.glm-4-flash".into(),
            messages: vec![bm_contract::connector::Message {
                role: bm_contract::connector::Role::User,
                content: "hi".into(),
                tool_call_id: None,
                tool_calls: None,
            }],
            tools: vec![],
            params: InvokeParams::default(),
            secret_ref: "secret:model.zhipu.glm-4-flash".into(),
            budget_ctx: BudgetCtx {
                operation_id: ids.next_id("op"),
                agent_id: ids.next_id("agent"),
                remaining_tokens: 1000,
            },
            deadline: "2026-08-29T09:30:35.000Z".into(),
            attempt,
        }
    }

    #[tokio::test]
    async fn scripted_sequence() {
        let m = MockConnector::new(vec![Step::timeout(), Step::ok("answer", 10, 5)]);
        let cancel = CancellationToken::new();
        assert!(matches!(
            m.invoke(req(1), cancel.clone()).await,
            InvokeResponse::Failed { .. }
        ));
        let ok = m.invoke(req(2), cancel.clone()).await;
        assert!(matches!(ok, InvokeResponse::Completed { .. }));
        // 耗尽后:internal
        assert!(matches!(
            m.invoke(req(3), cancel).await,
            InvokeResponse::Failed { .. }
        ));
    }

    #[tokio::test]
    async fn repeating_ok() {
        let m = MockConnector::repeating(Step::ok("x", 1, 1));
        let cancel = CancellationToken::new();
        for i in 1..=3 {
            assert!(matches!(
                m.invoke(req(i), cancel.clone()).await,
                InvokeResponse::Completed { .. }
            ));
        }
    }
}
