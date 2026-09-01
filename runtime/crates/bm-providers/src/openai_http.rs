//! OpenAI 兼容 HTTP 模型连接器(M7.1,ADR-0010)。
//! 面向 OpenAI 兼容网关(chat/completions,非流式),真实第三方中转网关
//! 与官方直连同走本实现——连接器可替换性由端口保证(基线 5.4)。
//!
//! 脱敏纪律(INV-5):错误分支只携带合同错误码与 retryable,绝不携带
//! 响应体、请求内容或凭据明文;detail_ref 恒为 None。

use async_trait::async_trait;
use bm_contract::connector::{FinishReason, InvokeRequest, InvokeResponse, Role, ToolCallPayload, Usage};
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
        // UA 必带:部分网关(opencode zen 等)套 Cloudflare,无 UA 请求
        // 403/1010 拒收;自报客户端身份即放行
        let http = reqwest::Client::builder()
            .user_agent(concat!("boenmind-server/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest Client 构造失败");
        Self {
            base_url: base_url.into(),
            store,
            http,
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
    /// W4 对话工具闭环:直通工具(OpenAI function 格式)透传。
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
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
    #[serde(default)]
    tool_calls: Option<Vec<WireToolCall>>,
}

#[derive(serde::Deserialize)]
struct WireToolCall {
    #[serde(default)]
    id: Option<String>,
    function: Option<WireToolFn>,
}

#[derive(serde::Deserialize)]
struct WireToolFn {
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(serde::Deserialize)]
struct WireUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

// ---- M9-S2 流式(SSE)线格式 --------------------------------------------

#[derive(serde::Deserialize)]
struct WireStreamChunk {
    choices: Vec<WireStreamChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(serde::Deserialize)]
struct WireStreamChoice {
    #[serde(default)]
    delta: Option<WireStreamDelta>,
    finish_reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct WireStreamDelta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<WireStreamToolCall>>,
}

#[derive(serde::Deserialize)]
struct WireStreamToolCall {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    function: Option<WireStreamToolFn>,
}

#[derive(serde::Deserialize)]
struct WireStreamToolFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// 聚合流式结果(占用流式路径共用;latency 口径与非流式一致,0 占位)。
fn completed_stream(
    content: String,
    finish_raw: &str,
    usage: Option<WireUsage>,
    interrupted: bool,
    model: &str,
    tool_calls: Vec<ToolCallPayload>,
) -> InvokeResponse {
    // finish_reason 按合同三值收敛;tool_calls 随 Completed 携带(W4)。
    let finish_reason = match finish_raw {
        "length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        _ => FinishReason::Stop,
    };
    InvokeResponse::Completed {
        content,
        tool_calls,
        finish_reason,
        usage: usage
            .map(|u| Usage {
                tokens_in: u.prompt_tokens.unwrap_or(0),
                tokens_out: u.completion_tokens.unwrap_or(0),
            })
            .unwrap_or(Usage {
                tokens_in: 0,
                tokens_out: 0,
            }),
        model_id: model.to_string(),
        latency_ms: 0,
        stream_interrupted: interrupted,
    }
}

/// 传输/解码故障 → 合同错误(与非流式 invoke 同纪律:零响应体零凭据)。
fn transport_failed(e: &reqwest::Error, attempt: u32) -> InvokeResponse {
    if e.is_decode() {
        return failed(ErrorCode::Internal, false, attempt);
    }
    if e.is_timeout() {
        return failed(ErrorCode::Unavailable, true, attempt);
    }
    if e.is_status() {
        return map_status(e.status().map(|s| s.as_u16()).unwrap_or(500), attempt);
    }
    failed(ErrorCode::Unavailable, true, attempt)
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
        // P1(第四轮评审):4xx(鉴权/参数错)归非故障类——不再计入 provider
        // 熔断(401 反复失败不该把通道熔断,掩盖配置错误)。
        401 | 403 => failed(ErrorCode::PermissionDenied, false, attempt),
        400..=499 => failed(ErrorCode::ValidationFailed, false, attempt),
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

        let has_tools = !req.tools.is_empty();
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
                        Role::Tool => "user",
                    },
                    content: &m.content,
                })
                .collect(),
            temperature: req.params.temperature,
            max_tokens: req.params.max_tokens,
            stream: false,
            tools: if has_tools {
                Some(serde_json::Value::Array(req.tools.clone()))
            } else {
                None
            },
            tool_choice: if has_tools { Some("auto") } else { None },
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

        // finish_reason 三值收敛;tool_calls 响应回喂对话循环(W4)。
        let finish = wire
            .choices
            .first()
            .and_then(|c| c.finish_reason.as_deref())
            .unwrap_or("stop");
        let finish_reason = match finish {
            "length" => FinishReason::Length,
            "tool_calls" => FinishReason::ToolCalls,
            _ => FinishReason::Stop,
        };
        let tool_calls: Vec<ToolCallPayload> = wire
            .choices
            .first()
            .and_then(|c| c.message.tool_calls.as_ref())
            .map(|tcs| {
                tcs.iter()
                    .enumerate()
                    .map(|(i, tc)| ToolCallPayload {
                        id: tc.id.clone().unwrap_or_else(|| format!("call_{}", i)),
                        name: tc
                            .function
                            .as_ref()
                            .and_then(|f| f.name.clone())
                            .unwrap_or_default(),
                        arguments: tc
                            .function
                            .as_ref()
                            .and_then(|f| f.arguments.clone())
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
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
            tool_calls,
            finish_reason,
            usage: usage.unwrap_or(Usage {
                tokens_in: 0,
                tokens_out: 0,
            }),
            model_id: model,
            // latency 由调用方(turn 循环)按真实钟测量;此处给 0 占位,
            // 与 MockConnector 的「声明值」口径一致(基线 9.7)。
            latency_ms: 0,
            stream_interrupted: false,
        }
    }

    /// 真 SSE 流式(stream=true):逐块回调增量;按字节缓冲整行再解码
    /// (防多字节字符被块边界劈开)。损坏块跳过不致命;[DONE] 或流自然
    /// 结束即聚合返回。中途传输故障:已收内容按 stream_interrupted=true
    /// 返回(可用即用),零内容则按可重试不可用上抛。
    async fn invoke_stream(
        &self,
        req: InvokeRequest,
        cancel: CancellationToken,
        mut on_delta: Box<dyn for<'a> FnMut(&'a str) + Send + 'static>,
    ) -> InvokeResponse {
        let attempt = req.attempt;
        let model = req.model_id.clone();
        let api_key = match SecretStore::get(self.store.as_ref(), &req.secret_ref) {
            Ok(k) => k,
            Err(_) => return failed(ErrorCode::Unavailable, true, attempt),
        };
        let has_tools = !req.tools.is_empty();
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
                        Role::Tool => "user",
                    },
                    content: &m.content,
                })
                .collect(),
            temperature: req.params.temperature,
            max_tokens: req.params.max_tokens,
            stream: true,
            tools: if has_tools {
                Some(serde_json::Value::Array(req.tools.clone()))
            } else {
                None
            },
            tool_choice: if has_tools { Some("auto") } else { None },
        };
        let budget = timestamp::remaining_until(&req.deadline).unwrap_or(Duration::from_secs(120));
        let request = self
            .http
            .post(self.endpoint())
            .bearer_auth(api_key)
            .json(&body)
            .timeout(budget);
        let open = async {
            let resp = request.send().await?.error_for_status()?;
            Ok::<reqwest::Response, reqwest::Error>(resp)
        };
        let mut resp = tokio::select! {
            _ = cancel.cancelled() => return failed(ErrorCode::Cancelled, false, attempt),
            r = open => match r {
                Ok(resp) => resp,
                Err(e) => return transport_failed(&e, attempt),
            },
        };
        let mut buf: Vec<u8> = Vec::new();
        let mut content = String::new();
        let mut finish = "stop".to_string();
        let mut usage: Option<WireUsage> = None;
        // W4:流式 tool_calls 分片聚合(按 index 拼 id/name/arguments)。
        let mut tc_parts: std::collections::BTreeMap<usize, (String, String, String)> =
            std::collections::BTreeMap::new();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => {
                    if content.is_empty() && tc_parts.is_empty() {
                        return failed(ErrorCode::Cancelled, false, attempt);
                    }
                    let tcs = tc_parts
                        .values()
                        .map(|(id, _n, ar)| ToolCallPayload {
                            id: id.clone(),
                            name: String::new(),
                            arguments: ar.clone(),
                        })
                        .collect();
                    return completed_stream(content, &finish, usage.take(), true, &model, tcs);
                }
                c = resp.chunk() => match c {
                    Ok(c) => c,
                    Err(e) => {
                        // 中途传输故障:已收内容可用即用(如实标记中断)。
                        if content.is_empty() && tc_parts.is_empty() {
                            return transport_failed(&e, attempt);
                        }
                        let tcs = tc_parts
                            .values()
                            .map(|(id, _n, ar)| ToolCallPayload {
                                id: id.clone(),
                                name: String::new(),
                                arguments: ar.clone(),
                            })
                            .collect();
                        return completed_stream(content, &finish, usage.take(), true, &model, tcs);
                    }
                },
            };
            let Some(bytes) = chunk else { break };
            buf.extend_from_slice(&bytes);
            while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                let line = String::from_utf8_lossy(&buf[..pos]).trim().to_string();
                buf.drain(..=pos);
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data == "[DONE]" {
                    let tcs: Vec<ToolCallPayload> = tc_parts
                        .values()
                        .map(|(id, _n, ar)| ToolCallPayload {
                            id: id.clone(),
                            name: _n.clone(),
                            arguments: ar.clone(),
                        })
                        .collect();
                    return completed_stream(content, &finish, usage.take(), false, &model, tcs);
                }
                // 损坏块跳过(网关行为差异容错,不致命)。
                let Ok(chunk_json) = serde_json::from_str::<WireStreamChunk>(data) else {
                    continue;
                };
                if let Some(u) = chunk_json.usage {
                    usage = Some(u);
                }
                if let Some(c) = chunk_json.choices.first() {
                    if let Some(f) = &c.finish_reason {
                        finish = f.clone();
                    }
                    if let Some(d) = &c.delta {
                        if let Some(t) = &d.content
                            && !t.is_empty()
                        {
                            content.push_str(t);
                            (on_delta)(t);
                        }
                        if let Some(tcs) = &d.tool_calls {
                            for tc in tcs {
                                let idx = tc.index.unwrap_or(0);
                                let slot = tc_parts.entry(idx).or_insert_with(|| {
                                    (
                                        tc.id.clone().unwrap_or_default(),
                                        String::new(),
                                        String::new(),
                                    )
                                });
                                if let Some(id) = &tc.id {
                                    slot.0 = id.clone();
                                }
                                if let Some(f) = &tc.function {
                                    if let Some(nm) = &f.name {
                                        slot.1.push_str(nm);
                                    }
                                    if let Some(ar) = &f.arguments {
                                        slot.2.push_str(ar);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let tcs: Vec<ToolCallPayload> = tc_parts
            .values()
            .map(|(id, _n, ar)| ToolCallPayload {
                id: id.clone(),
                name: String::new(),
                arguments: ar.clone(),
            })
            .collect();
        completed_stream(content, &finish, usage.take(), false, &model, tcs)
    }
}

#[cfg(test)]
mod m9_stream_tests {
    use super::*;

    /// t143:流式线格式——多块解析、finish_reason 收敛、损坏行容错(跳过不致命)。
    #[test]
    fn t143_sse_chunk_decode_and_completed_aggregation() {
        // 数据块:增量 + finish_reason + usage 各自独立到达(OpenAI 线格式)
        let chunk1: WireStreamChunk =
            serde_json::from_str(r#"{"choices":[{"delta":{"content":"你"}}]}"#).expect("块1");
        assert_eq!(
            chunk1.choices[0].delta.as_ref().unwrap().content.as_deref(),
            Some("你")
        );
        assert!(chunk1.usage.is_none());

        let chunk2: WireStreamChunk = serde_json::from_str(
            r#"{"choices":[{"delta":{"content":"好"},"finish_reason":null}],"usage":{"prompt_tokens":7,"completion_tokens":3}}"#,
        )
        .expect("块2");
        assert_eq!(chunk2.usage.as_ref().unwrap().completion_tokens, Some(3));

        // 损坏行 → None(调用方跳过,不致命)
        assert!(serde_json::from_str::<WireStreamChunk>("{not json").is_err());

        // finish_reason 收敛:length → Length,其余 → Stop
        let done = completed_stream("你好".into(), "length", chunk2.usage, false, "m1", Vec::new());
        match done {
            InvokeResponse::Completed {
                content,
                finish_reason,
                usage,
                model_id,
                stream_interrupted,
                ..
            } => {
                assert_eq!(content, "你好");
                assert_eq!(finish_reason, FinishReason::Length);
                assert_eq!(usage.tokens_in, 7);
                assert_eq!(usage.tokens_out, 3);
                assert_eq!(model_id, "m1");
                assert!(!stream_interrupted);
            }
            _ => panic!("应为 Completed"),
        }
    }
}

#[cfg(test)]
mod m9_review_status_tests {
    use super::*;

    /// P1(第四轮评审)验收:401/403 归 PermissionDenied(非故障类),
    /// 不再计入 provider 熔断;429 仍为可重试 Unavailable。
    #[test]
    fn auth_errors_are_not_provider_faults() {
        for status in [401u16, 403] {
            match map_status(status, 1) {
                InvokeResponse::Failed {
                    error_code,
                    retryable,
                    ..
                } => {
                    assert_eq!(error_code, ErrorCode::PermissionDenied);
                    assert!(!retryable);
                }
                _ => panic!("应为 Failed"),
            }
        }
        match map_status(429, 1) {
            InvokeResponse::Failed {
                error_code,
                retryable,
                ..
            } => {
                assert_eq!(error_code, ErrorCode::Unavailable);
                assert!(retryable);
            }
            _ => panic!("应为 Failed"),
        }
    }
}
