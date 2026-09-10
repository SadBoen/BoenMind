//! GLM HTTP 适配器(feature = "glm",默认关;规格 §4.3/D1)。
//! 走智谱 chat/completions 端点,非流式。验收不依赖本模块;
//! 仅作为真实传输的存在性证明与联调工具。
//! 线格式与 openai_http 同源复用(OpenAI 兼容协议,单源防漂移)。

use async_trait::async_trait;
use bm_contract::connector::{FinishReason, InvokeRequest, InvokeResponse, Role};
use bm_contract::error_codes::ErrorCode;
use bm_core::ports::{ModelConnector, SecretStore};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::openai_http::{WireMessage, WireRequest, WireResponse, WireUsage, failed};

pub struct GlmConnector {
    endpoint: String,
    store: Arc<dyn SecretStore>,
    http: reqwest::Client,
}

impl GlmConnector {
    /// F-03(审计台账)修复:凭据经构造注入(与 OpenAiConnector 同口径),
    /// 移除"进程级 SECRET_BRIDGE 静态桥"——该桥从未被组装方接线,属死代码
    /// 且使本连接器运行时必败(桥未设置即报错)。
    pub fn new(endpoint: impl Into<String>, store: Arc<dyn SecretStore>) -> Self {
        Self {
            endpoint: endpoint.into(),
            store,
            http: reqwest::Client::new(),
        }
    }

    /// 智谱默认端点。
    pub fn zhipu(store: Arc<dyn SecretStore>) -> Self {
        Self::new(
            "https://open.bigmodel.cn/api/paas/v4/chat/completions",
            store,
        )
    }
}

#[async_trait]
impl ModelConnector for GlmConnector {
    async fn invoke(&self, req: InvokeRequest, cancel: CancellationToken) -> InvokeResponse {
        let attempt = req.attempt;
        let model = req.model_id.clone();

        let api_key = match SecretStore::get(self.store.as_ref(), &req.secret_ref) {
            Ok(k) => k,
            Err(_) => return failed(ErrorCode::Unavailable, true, attempt),
        };

        let body = WireRequest {
            model: &model,
            messages: req
                .messages
                .iter()
                .map(|m| WireMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    },
                    content: Some(&m.content),
                    tool_call_id: None,
                    tool_calls: None,
                })
                .collect(),
            temperature: req.params.temperature,
            max_tokens: req.params.max_tokens,
            stream: false,
            tools: None,
            tool_choice: None,
        };

        // P1(第四轮评审):对齐 openai 连接器,预算超时防网络悬挂
        // (悬挂会让 stop() 排空永不返回)。
        let budget = bm_contract::timestamp::remaining_until(&req.deadline)
            .unwrap_or(Duration::from_secs(120));
        // issue #11:错误体透传需对密钥脱敏,先留副本(bearer_auth 会移动)
        let api_key_for_sanitize = api_key.clone();
        let fut = self
            .http
            .post(&self.endpoint)
            .bearer_auth(api_key)
            .json(&body)
            .timeout(budget)
            .send();

        let resp = tokio::select! {
            _ = cancel.cancelled() => return failed(ErrorCode::Cancelled, false, attempt),
            r = fut => r,
        };

        let resp = match resp {
            Ok(r) => r,
            Err(_) => return failed(ErrorCode::Unavailable, true, attempt),
        };
        if !resp.status().is_success() {
            // P1-22(2026-09-07 架构评审):与 openai_http 同一状态码口径——
            // 401/403 归 PermissionDenied、其余 4xx 归 ValidationFailed(均
            // 不可重试不烧熔断),429/5xx 才是可重试 Unavailable。
            // issue #11:网关原文随 detail 脱敏透传(ADR-0029 错误原文保真)。
            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| "[响应体不可读]".into());
            return crate::openai_http::map_status_body(
                status.as_u16(),
                attempt,
                &body,
                &api_key_for_sanitize,
            );
        }
        let parsed: Result<WireResponse, _> = resp.json().await;
        match parsed {
            Ok(w) => match w.choices.into_iter().next() {
                Some(c) => InvokeResponse::Completed {
                    tool_calls: Vec::new(),
                    content: c.message.content.unwrap_or_default(),
                    finish_reason: match c.finish_reason.as_deref() {
                        Some("length") => FinishReason::Length,
                        _ => FinishReason::Stop,
                    },
                    usage: w.usage.map(WireUsage::into_usage).unwrap_or_default(),
                    model_id: model,
                    latency_ms: 0,
                    stream_interrupted: false,
                },
                None => failed(ErrorCode::Internal, false, attempt),
            },
            Err(_) => failed(ErrorCode::Internal, false, attempt),
        }
    }

    fn provider(&self) -> &'static str {
        "glm-http"
    }
}
