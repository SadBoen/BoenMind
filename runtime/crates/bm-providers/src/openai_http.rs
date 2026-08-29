//! OpenAI 兼容 HTTP 模型连接器(M7.1,ADR-0010)。
//! 面向 OpenAI 兼容网关(chat/completions,非流式),真实第三方中转网关
//! 与官方直连同走本实现——连接器可替换性由端口保证(基线 5.4)。
//!
//! 脱敏纪律(INV-5):错误分支只携带合同错误码与 retryable,绝不携带
//! 响应体、请求内容或凭据明文;detail_ref 恒为 None。

use async_trait::async_trait;
use bm_contract::connector::{FinishReason, InvokeRequest, InvokeResponse, Role, Usage};
use bm_contract::error_codes::ErrorCode;
use bm_contract::timestamp;
use bm_core::ports::{ModelConnector, SecretStore};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub struct OpenAiConnector {
    /// 形如 `https://host/v1`;请求发往 `{base_url}/chat/completions`。
    base_url: String,
    store: Arc<dyn SecretStore>,
    http: reqwest::Client,
}

impl OpenAiConnector {
    pub fn new(base_url: impl Into<String>, store: Arc<dyn SecretStore>) -> Self {
        Self {
            base_url: base_url.into(),
            store,
            http: reqwest::Client::new(),
        }
    }

    fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/chat/completions")
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

/// HTTP 状态 → 合同错误码。429/5xx/传输故障可重试;4xx(鉴权/参数)不可重试。
fn map_status(status: u16, attempt: u32) -> InvokeResponse {
    match status {
        429 | 500..=599 => failed(ErrorCode::Unavailable, true, attempt),
        400..=499 => failed(ErrorCode::Unavailable, false, attempt),
        _ => failed(ErrorCode::Internal, false, attempt),
    }
}

#[async_trait]
impl ModelConnector for OpenAiConnector {
    fn provider(&self) -> &'static str {
        "openai-http"
    }

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
                    },
                    content: &m.content,
                })
                .collect(),
            temperature: req.params.temperature,
            max_tokens: req.params.max_tokens,
            stream: false,
        };

        let budget = timestamp::remaining_until(&req.deadline).unwrap_or(Duration::from_secs(120));
        let request = self
            .http
            .post(self.endpoint())
            .bearer_auth(api_key)
            .json(&body)
            .timeout(budget);

        let respond = async {
            let resp = request.send().await?.error_for_status()?;
            let wire: WireResponse = resp.json().await?;
            Ok::<WireResponse, reqwest::Error>(wire)
        };

        let wire = tokio::select! {
            _ = cancel.cancelled() => return failed(ErrorCode::Cancelled, false, attempt),
            r = respond => r,
        };

        let wire = match wire {
            Ok(w) => w,
            Err(e) => {
                // 解码失败 = 网关响应不兼容(内部问题,不盲重试);
                // 超时/传输故障 = 可重试不可用;HTTP 状态另行映射。
                if e.is_decode() {
                    return failed(ErrorCode::Internal, false, attempt);
                }
                let code = if e.is_timeout() {
                    ErrorCode::Unavailable
                } else if e.is_status() {
                    return map_status(e.status().map(|s| s.as_u16()).unwrap_or(500), attempt);
                } else {
                    ErrorCode::Unavailable
                };
                return failed(code, true, attempt);
            }
        };

        // finish_reason 三值以上(如 tool_calls)按合同二值收敛:M7 非流式、
        // tools 恒空,内容照常返回(合同枚举不破,流式留 M8)。
        let finish = wire
            .choices
            .first()
            .and_then(|c| c.finish_reason.as_deref())
            .unwrap_or("stop");
        let finish_reason = match finish {
            "length" => FinishReason::Length,
            _ => FinishReason::Stop,
        };
        let content = wire
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();
        let usage = wire.usage.map(|u| Usage {
            tokens_in: u.prompt_tokens.unwrap_or(0),
            tokens_out: u.completion_tokens.unwrap_or(0),
        });

        InvokeResponse::Completed {
            content,
            finish_reason,
            usage: usage.unwrap_or(Usage { tokens_in: 0, tokens_out: 0 }),
            model_id: model,
            // latency 由调用方(turn 循环)按真实钟测量;此处给 0 占位,
            // 与 MockConnector 的「声明值」口径一致(基线 9.7)。
            latency_ms: 0,
            stream_interrupted: false,
        }
    }
}
