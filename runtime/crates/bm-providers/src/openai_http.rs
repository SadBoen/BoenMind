//! OpenAI 兼容 HTTP 模型连接器(M7.1,ADR-0010)。
//! 面向 OpenAI 兼容网关(chat/completions,非流式),真实第三方中转网关
//! 与官方直连同走本实现——连接器可替换性由端口保证(基线 5.4)。
//!
//! 脱敏纪律(INV-5):错误分支只携带合同错误码与 retryable,绝不携带
//! 响应体、请求内容或凭据明文;detail_ref 恒为 None。

use async_trait::async_trait;
use bm_contract::connector::{
    FinishReason, InvokeRequest, InvokeResponse, Role, ToolCallPayload, Usage,
};
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
 /// OpenCode Go 网关要求的稳定会话标识(请求头 `x-opencode-session`;
 /// )。语义 =
 /// 每个对话一个稳定 id,供网关做路由优化与 prompt 缓存亲和;本实现给
 /// 连接器实例级稳定 id(env BOEN_OPENCODE_SESSION_ID 可固定,缺省进程
 /// 内随机),网关侧仅缓存亲和损失,无功能影响。
    session_tag: String,
}

impl OpenAiConnector {
    pub fn new(base_url: impl Into<String>, store: Arc<dyn SecretStore>) -> Self {
 // UA 必带:部分网关(opencode zen 等)套 Cloudflare,无 UA 请求
 // 403/1010 拒收;自报客户端身份即放行
        let http = reqwest::Client::builder()
            .user_agent(concat!("boenmind-server/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest Client 构造失败");
        let session_tag = std::env::var("BOEN_OPENCODE_SESSION_ID").unwrap_or_else(|_| {
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).expect("系统熵源不可用");
            format!("boenmind-{}", bm_contract::hash::hex(&bytes))
        });
        Self {
            base_url: base_url.into(),
            store,
            http,
            session_tag,
        }
    }

    fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}/chat/completions")
    }
}

// ---- OpenAI 兼容线格式(单源:真实连接器与 mock 共用同一形状,防协议漂移)--------------

#[derive(serde::Serialize)]
pub(crate) struct WireMessage<'a> {
    pub(crate) role: &'a str,
 // ADR-0022:assistant 携带 tool_calls 时 content 允许为 null(OpenAI
 // 形态);其余角色恒 Some。
 #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content: Option<&'a str>,
 /// role="tool" 时本结果对应的 tool_call id(因果链对齐)。
 #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_call_id: Option<&'a str>,
 /// assistant 消息原样透传模型发起的工具调用。
 #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_calls: Option<Vec<WireToolCallOut<'a>>>,
}

#[derive(serde::Serialize)]
pub(crate) struct WireToolCallOut<'a> {
    id: &'a str,
 #[serde(rename = "type")]
    kind: &'a str,
    function: WireToolFnOut<'a>,
}

#[derive(serde::Serialize)]
struct WireToolFnOut<'a> {
    name: &'a str,
    arguments: &'a str,
}

/// 合同 Message → OpenAI 兼容 wire 消息。
/// ADR-0022 协议还原:工具结果出原生 `role:"tool"` + `tool_call_id`;
/// assistant 回喂携带 tool_calls。仅当 tool 结果缺 tool_call_id(修复前
/// 的历史消息,正常路径不产生)才回落 user 伪装,保老网关兼容。
fn to_wire_messages(messages: &[bm_contract::connector::Message]) -> Vec<WireMessage<'_>> {
    messages
        .iter()
        .map(|m| match m.role {
            Role::System => WireMessage {
                role: "system",
                content: Some(&m.content),
                tool_call_id: None,
                tool_calls: None,
            },
            Role::User => WireMessage {
                role: "user",
                content: Some(&m.content),
                tool_call_id: None,
                tool_calls: None,
            },
            Role::Assistant => WireMessage {
                role: "assistant",
                content: match m.tool_calls.as_ref() {
 // 纯工具调用无文本 → content 置 null(OpenAI 形态)
                    Some(_) if m.content.is_empty() => None,
                    _ => Some(&m.content),
                },
                tool_call_id: None,
                tool_calls: m.tool_calls.as_ref().map(|tcs| {
                    tcs.iter()
                        .map(|tc| WireToolCallOut {
                            id: &tc.id,
                            kind: "function",
                            function: WireToolFnOut {
                                name: &tc.name,
                                arguments: &tc.arguments,
                            },
                        })
                        .collect()
                }),
            },
            Role::Tool => match m.tool_call_id.as_deref() {
                Some(id) => WireMessage {
                    role: "tool",
                    content: Some(&m.content),
                    tool_call_id: Some(id),
                    tool_calls: None,
                },
                None => WireMessage {
                    role: "user",
                    content: Some(&m.content),
                    tool_call_id: None,
                    tool_calls: None,
                },
            },
        })
        .collect()
}

#[derive(serde::Serialize)]
pub(crate) struct WireRequest<'a> {
    pub(crate) model: &'a str,
    pub(crate) messages: Vec<WireMessage<'a>>,
 #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f64>,
 #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    pub(crate) stream: bool,
 /// W4 对话工具闭环:直通工具(OpenAI function 格式)透传。
 #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<serde_json::Value>,
 #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_choice: Option<&'a str>,
}

#[derive(serde::Deserialize)]
pub(crate) struct WireResponse {
    pub(crate) choices: Vec<WireChoice>,
    pub(crate) usage: Option<WireUsage>,
}

#[derive(serde::Deserialize)]
pub(crate) struct WireChoice {
    pub(crate) finish_reason: Option<String>,
    pub(crate) message: WireMsg,
}

#[derive(serde::Deserialize)]
pub(crate) struct WireMsg {
    pub(crate) content: Option<String>,
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
pub(crate) struct WireUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
 /// OpenAI 兼容细分:提示词缓存命中(各家网关实现不一,缺省不报)。
 #[serde(default)]
    prompt_tokens_details: Option<WirePromptTokensDetails>,
 /// 推理思考分账(推理模型;缺省不报)。
 #[serde(default)]
    completion_tokens_details: Option<WireCompletionTokensDetails>,
}

#[derive(serde::Deserialize)]
struct WirePromptTokensDetails {
    cached_tokens: Option<u64>,
}

#[derive(serde::Deserialize)]
struct WireCompletionTokensDetails {
    reasoning_tokens: Option<u64>,
}

impl WireUsage {
 /// 网报 usage → 合同 Usage(细分字段缺省如实为 None,不估算冒充)。
    pub(crate) fn into_usage(self) -> Usage {
        Usage {
            tokens_in: self.prompt_tokens.unwrap_or(0),
            tokens_out: self.completion_tokens.unwrap_or(0),
            tokens_reasoning: self
                .completion_tokens_details
                .and_then(|d| d.reasoning_tokens),
            tokens_cached: self.prompt_tokens_details.and_then(|d| d.cached_tokens),
        }
    }
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
        usage: usage.map(WireUsage::into_usage).unwrap_or_default(),
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

pub(crate) fn failed(code: ErrorCode, retryable: bool, attempt: u32) -> InvokeResponse {
    InvokeResponse::Failed {
        error_code: code,
        retryable,
        attempt,
        detail_ref: None,
        detail: None,
    }
}

/// ADR-0029(用户裁决「错误原文保真」):HTTP 错误响应体经凭据脱敏与
/// 2000 字符截断后随 Failed.detail 透传——模型与用户终于能看到网关的
/// 真实死因。INV-5 纪律不变:凭据明文在构造点替换为 [REDACTED]。
fn sanitize_detail(body: &str, secret: &str) -> String {
    let mut s = if secret.is_empty() {
        body.trim().to_string()
    } else {
        body.trim().replace(secret, "[REDACTED]")
    };
    if s.chars().count() > 2000 {
        s = s.chars().take(2000).collect();
    }
    s
}

fn with_detail(mut resp: InvokeResponse, detail: String) -> InvokeResponse {
    if let InvokeResponse::Failed { detail: d, .. } = &mut resp {
        *d = Some(detail);
    }
    resp
}

pub(crate) fn map_status_body(
    status: u16,
    attempt: u32,
    body: &str,
    secret: &str,
) -> InvokeResponse {
    with_detail(map_status(status, attempt), sanitize_detail(body, secret))
}

/// send 阶段错误:HTTP 错误状态携带响应体(供脱敏透传),其余复用 reqwest 语义。
pub(crate) enum OpenAiErr {
    Status { status: u16, body: String },
    Http(reqwest::Error),
}

impl From<reqwest::Error> for OpenAiErr {
    fn from(e: reqwest::Error) -> Self {
        OpenAiErr::Http(e)
    }
}

/// HTTP 状态 → 合同错误码。429/5xx/传输故障可重试;4xx(鉴权/参数)不可重试。
pub(crate) fn map_status(status: u16, attempt: u32) -> InvokeResponse {
    match status {
        429 | 500..=599 => failed(ErrorCode::Unavailable, true, attempt),
 // P1():4xx(鉴权/参数错)归非故障类——不再计入 provider
 // 熔断(401 反复失败不该把通道熔断,掩盖配置错误)。
        401 | 403 => failed(ErrorCode::PermissionDenied, false, attempt),
        400..=499 => failed(ErrorCode::ValidationFailed, false, attempt),
        _ => failed(ErrorCode::Internal, false, attempt),
    }
}

/// wire 请求体组装(工具透传三元),invoke 与 invoke_stream 共用。
fn build_body<'a>(req: &'a InvokeRequest, model: &'a str, stream: bool) -> WireRequest<'a> {
    let has_tools = !req.tools.is_empty();
    WireRequest {
        model,
        messages: to_wire_messages(&req.messages),
        temperature: req.params.temperature,
        max_tokens: req.params.max_tokens,
        stream,
        tools: if has_tools {
            Some(serde_json::Value::Array(req.tools.clone()))
        } else {
            None
        },
        tool_choice: if has_tools { Some("auto") } else { None },
    }
}

impl OpenAiConnector {
 /// POST 链(预算超时防悬挂;会话标签头),两个入口共用。
    fn build_request(
        &self,
        body: &WireRequest<'_>,
        api_key: &str,
        budget: Duration,
    ) -> reqwest::RequestBuilder {
        self.http
            .post(self.endpoint())
            .bearer_auth(api_key)
            .header("x-opencode-session", &self.session_tag)
            .json(body)
            .timeout(budget)
    }
}

/// send + HTTP 状态预检:4xx/5xx → Status 错误携响应体(供脱敏透传)。
async fn send_and_check(request: reqwest::RequestBuilder) -> Result<reqwest::Response, OpenAiErr> {
    let resp = request.send().await?;
    let status = resp.status();
    if status.is_client_error() || status.is_server_error() {
        let body = resp
            .text()
            .await
            .unwrap_or_else(|_| "[响应体不可读]".into());
        return Err(OpenAiErr::Status {
            status: status.as_u16(),
            body,
        });
    }
    Ok(resp)
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

        let body = build_body(&req, &model, false);
        let budget = timestamp::remaining_until(&req.deadline).unwrap_or(Duration::from_secs(120));
        let request = self.build_request(&body, &api_key, budget);

        let respond = async {
            let wire: WireResponse = send_and_check(request).await?.json().await?;
            Ok::<WireResponse, OpenAiErr>(wire)
        };

        let wire = tokio::select! {
            _ = cancel.cancelled() => return failed(ErrorCode::Cancelled, false, attempt),
            r = respond => r,
        };

        let wire = match wire {
            Ok(w) => w,
            Err(e) => {
                if let OpenAiErr::Status { status, body } = e {
                    return map_status_body(status, attempt, &body, &api_key);
                }
                let e = match e {
                    OpenAiErr::Http(e) => e,
                    OpenAiErr::Status { .. } => unreachable!("上方已拦截"),
                };
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
        let usage = wire.usage.map(WireUsage::into_usage);

        InvokeResponse::Completed {
            content,
            tool_calls,
            finish_reason,
            usage: usage.unwrap_or_default(),
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
        let body = build_body(&req, &model, true);
        let budget = timestamp::remaining_until(&req.deadline).unwrap_or(Duration::from_secs(120));
        let request = self.build_request(&body, &api_key, budget);
        let open = send_and_check(request);
        let mut resp = tokio::select! {
            _ = cancel.cancelled() => return failed(ErrorCode::Cancelled, false, attempt),
            r = open => match r {
                Ok(resp) => resp,
                Err(e) => return match e {
                    OpenAiErr::Status { status, body } => {
                        map_status_body(status, attempt, &body, &api_key)
                    }
                    OpenAiErr::Http(e) => transport_failed(&e, attempt),
                },
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
 // P1-20():缺 index 时按
 // id 归槽——同块多个缺 index 的 tool_calls 不再
 // 全部挤进 0 号槽互相拼接成畸形调用;id 亦缺
 // 则退回 0(单工具调用的常见网关形态)。
                                let idx = match tc.index {
                                    Some(i) => i,
                                    None => {
                                        let id = tc.id.as_deref().unwrap_or_default();
                                        let matched = tc_parts
                                            .iter()
                                            .find(|(_, (sid, _, _))| !sid.is_empty() && sid == id)
                                            .map(|(k, _)| *k);
                                        matched.unwrap_or(0)
                                    }
                                };
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
                name: _n.clone(),
                arguments: ar.clone(),
            })
            .collect();
        completed_stream(content, &finish, usage.take(), false, &model, tcs)
    }
}

#[cfg(test)]
mod stream_decode_tests {
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
        let done = completed_stream(
            "你好".into(),
            "length",
            chunk2.usage,
            false,
            "m1",
            Vec::new(),
        );
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

 /// usage 细分字段透传:推理思考与缓存命中如实进合同 Usage;
 /// 网关不报 → None(前端据此显示「未上报」,绝不估算冒充)。
 #[test]
    fn t_usage_details_reasoning_and_cached() {
        let wire: WireUsage = serde_json::from_str(
            r#"{"prompt_tokens":100,"completion_tokens":40,
               "prompt_tokens_details":{"cached_tokens":60},
               "completion_tokens_details":{"reasoning_tokens":25}}"#,
        )
        .expect("细分 usage 解析");
        let u = wire.into_usage();
        assert_eq!(u.tokens_in, 100);
        assert_eq!(u.tokens_reasoning, Some(25));
        assert_eq!(u.tokens_cached, Some(60));

 // 网关口径欠缺(只有总量)→ 细分如实 None
        let bare: WireUsage =
            serde_json::from_str(r#"{"prompt_tokens":7,"completion_tokens":3}"#).unwrap();
        let b = bare.into_usage();
        assert_eq!(b.tokens_reasoning, None);
        assert_eq!(b.tokens_cached, None);
    }
}

#[cfg(test)]
mod m9_review_status_tests {
    use super::*;

 /// P1()验收:401/403 归 PermissionDenied(非故障类),
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

 // ---- ADR-0022 协议还原:wire 形态验收 ------------------------------

    use bm_contract::connector::{Message, ToolCallPayload};

 #[test]
    fn wire_tool_result_uses_native_role_with_call_id() {
        let msgs = vec![
            Message {
                role: Role::User,
                content: "查一下".into(),
                tool_call_id: None,
                tool_calls: None,
            },
            Message {
                role: Role::Assistant,
                content: String::new(),
                tool_call_id: None,
                tool_calls: Some(vec![ToolCallPayload {
                    id: "call_1".into(),
                    name: "fs_read".into(),
                    arguments: "{\"path\":\"a.txt\"}".into(),
                }]),
            },
            Message {
                role: Role::Tool,
                content: "文件内容".into(),
                tool_call_id: Some("call_1".into()),
                tool_calls: None,
            },
        ];
        let wire = to_wire_messages(&msgs);
        let json = serde_json::to_string(&wire).expect("序列化");
 // 原生 tool 角色 + id 对齐
        assert!(json.contains("\"role\":\"tool\""), "{json}");
        assert!(json.contains("\"tool_call_id\":\"call_1\""), "{json}");
 // assistant 透传 tool_calls;纯调用无文本 → content 缺省(null)
        assert!(
            json.contains("\"tool_calls\":[{\"id\":\"call_1\",\"type\":\"function\""),
            "{json}"
        );
        let assistant = &wire[1];
        assert_eq!(assistant.role, "assistant");
        assert!(
            assistant.content.is_none(),
            "纯工具调用消息 content 应为 null"
        );
    }

 #[test]
    fn wire_legacy_tool_without_id_falls_back_to_user() {
        let msgs = vec![Message {
            role: Role::Tool,
            content: "旧消息".into(),
            tool_call_id: None,
            tool_calls: None,
        }];
        let wire = to_wire_messages(&msgs);
        assert_eq!(
            wire[0].role, "user",
            "缺 id 的历史 tool 消息回落 user 保兼容"
        );
    }

 #[test]
    fn wire_assistant_text_with_tool_calls_keeps_content() {
        let msgs = vec![Message {
            role: Role::Assistant,
            content: "我先读文件".into(),
            tool_call_id: None,
            tool_calls: Some(vec![ToolCallPayload {
                id: "call_9".into(),
                name: "fs_read".into(),
                arguments: "{}".into(),
            }]),
        }];
        let wire = to_wire_messages(&msgs);
        assert_eq!(wire[0].content, Some("我先读文件"));
    }
}

// ADR-0029 / INV-13:错误原文保真的脱敏与边界纪律。
#[cfg(test)]
mod inv13_error_detail_tests {
    use super::sanitize_detail;

 #[test]
    fn inv13_detail_redacts_credential_and_bounded() {
        let secret = "sk-abcdef1234567890XYZ";
        let body = format!(
            "{{\"error\":{{\"message\":\"invalid api key {secret} provided\",\"type\":\"invalid_request_error\"}}}}"
        );
        let d = sanitize_detail(&body, secret);
        assert!(!d.contains(secret), "凭据明文必须被替换(INV-5)");
        assert!(d.contains("[REDACTED]"), "脱敏标记必须存在");
        assert!(
            d.contains("invalid api key"),
            "事实文本必须保留(错误原文保真)"
        );
    }

 #[test]
    fn inv13_detail_truncated_to_2000_chars() {
        let body = "x".repeat(5000);
        let d = sanitize_detail(&body, "no-secret");
        assert_eq!(d.chars().count(), 2000, "detail 必须截断到 2000 字符");
    }
}
