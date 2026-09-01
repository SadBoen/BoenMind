//! GLM HTTP 适配器(feature = "glm",默认关;规格 §4.3/D1)。
//! 走智谱 chat/completions 端点,非流式。验收不依赖本模块;
//! 仅作为真实传输的存在性证明与联调工具。

use async_trait::async_trait;
use bm_contract::connector::{FinishReason, InvokeRequest, InvokeResponse, Role, Usage};
use bm_contract::error_codes::ErrorCode;
use bm_core::ports::ModelConnector;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// 进程级 Secret Store 桥:组装方启动时调用 [`set_secret_bridge`]。
/// (连接器自身构造时不持有 store,是为了让示例组装路径最短;M7 外置进程时
/// 由 Provider handshake 注入,调用方无感。)
static SECRET_BRIDGE: std::sync::OnceLock<std::sync::Arc<dyn bm_core::ports::SecretStore>> =
    std::sync::OnceLock::new();

pub fn set_secret_bridge(store: std::sync::Arc<dyn bm_core::ports::SecretStore>) {
    let _ = SECRET_BRIDGE.set(store);
}

fn resolve_secret(secret_ref: &str) -> Result<String, bm_core::ports::SecretError> {
    match SECRET_BRIDGE.get() {
        Some(store) => bm_core::ports::SecretStore::get(store.as_ref(), secret_ref),
        None => Err(bm_core::ports::SecretError::Backend(
            "GLM Secret 桥未设置".into(),
        )),
    }
}

pub struct GlmConnector {
    endpoint: String,
    http: reqwest::Client,
}

impl GlmConnector {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            http: reqwest::Client::new(),
        }
    }

    /// 智谱默认端点。
    pub fn zhipu() -> Self {
        Self::new("https://open.bigmodel.cn/api/paas/v4/chat/completions")
    }
}

#[derive(serde::Serialize)]
struct WireMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(serde::Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    messages: Vec<WireMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
}

#[derive(serde::Deserialize)]
struct WireResponse {
    choices: Vec<WireChoice>,
    usage: Option<WireUsage>,
}

#[derive(serde::Deserialize)]
struct WireChoice {
    finish_reason: Option<String>,
    message: WireMsg,
}

#[derive(serde::Deserialize)]
struct WireMsg {
    content: Option<String>,
}

#[derive(serde::Deserialize)]
struct WireUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

fn failed(code: ErrorCode, retryable: bool, attempt: u32) -> InvokeResponse {
    InvokeResponse::Failed {
        error_code: code,
        retryable,
        attempt,
        detail_ref: None,
    }
}

#[async_trait]
impl ModelConnector for GlmConnector {
    async fn invoke(&self, req: InvokeRequest, cancel: CancellationToken) -> InvokeResponse {
        let attempt = req.attempt;
        let model = req.model_id.clone();

        let api_key = match resolve_secret(&req.secret_ref) {
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
                    },
                    content: &m.content,
                })
                .collect(),
            temperature: req.params.temperature,
            max_tokens: req.params.max_tokens,
            stream: false,
        };

        // P1(第四轮评审):对齐 openai 连接器,预算超时防网络悬挂
        // (悬挂会让 stop() 排空永不返回)。
        let budget = bm_contract::timestamp::remaining_until(&req.deadline)
            .unwrap_or(Duration::from_secs(120));
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
            return failed(
                ErrorCode::Unavailable,
                resp.status().is_server_error(),
                attempt,
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
                    usage: Usage {
                        tokens_in: w.usage.as_ref().and_then(|u| u.prompt_tokens).unwrap_or(0),
                        tokens_out: w
                            .usage
                            .as_ref()
                            .and_then(|u| u.completion_tokens)
                            .unwrap_or(0),
                    },
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
