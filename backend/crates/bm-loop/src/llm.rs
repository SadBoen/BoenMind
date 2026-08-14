//! OpenAI 兼容流式 LLM client（A6 主体第一件）。
//!
//! - [`LlmEvent`]：loop 自有流事件形态（与 OpenAI 方言解耦——工具调用
//!   增量在 client 内累积，流结束时以 [`LlmEvent::ToolCallEnd`]（完整参数）
//!   与 [`LlmEvent::MessageEnd`]（权威内容 + usage）收口）；
//! - [`OpenAiClient`]：POST `{base}/chat/completions`（stream:true）→ SSE
//!   解析 → [`LlmEvent`] 流。非流式兜底（个别提供商忽略 stream 参数）时
//!   解析整包 JSON 转单条 MessageEnd；
//! - 错误 4xx/5xx 读响应体入 [`LlmError`]（`retryable` = 429/5xx，loop 重试策略用）；
//! - 取消语义 = drop 流（reqwest 断连），loop 层用 `tokio::select!` 丢流即可。
//!
//! 提供商配置复用 bm-core providers：**本 crate 不依赖 bm-core**（铁律 3，
//! 见 tests/architecture.rs）——集成方（bm-server）从 bm-core 配置解析出
//! base_url/api_key/model 后构造 [`LlmConfig`] 传入。

use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;

/// LLM 流事件（loop 自有形态）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LlmEvent {
    /// 正文增量（思考文本由上游并入正文后下发，与 pi 路径语义一致）
    TextDelta { text: String },
    /// 工具调用开始（id + 名；参数随后以 Args 增量到达）
    ToolCallStart { id: String, name: String },
    /// 工具调用参数增量（多帧拼接；拼接权在 client）
    ToolCallArgs { id: String, args_delta: String },
    /// 工具调用收口（arguments = 完整 JSON 参数串）
    ToolCallEnd { id: String, arguments: String },
    /// 流结束：权威内容 + 本步全部工具调用 + token 用量（可能为 None）
    MessageEnd {
        content: String,
        tool_calls: Vec<LlmToolCall>,
        usage: Option<LlmUsage>,
    },
}

/// 完整工具调用（ToolCallEnd / MessageEnd 携带）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmToolCall {
    pub id: String,
    pub name: String,
    /// 完整 JSON 参数串
    pub arguments: String,
}

/// token 用量（MessageEnd 携带；非流式兜底无 usage 时为 None）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// LLM 错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmError {
    pub message: String,
    /// 可重试（429/5xx/网络中断）；不可重试（4xx 参数/鉴权错误重试无意义）
    pub retryable: bool,
}

impl LlmError {
    pub fn new(message: impl Into<String>, retryable: bool) -> Self {
        Self {
            message: message.into(),
            retryable,
        }
    }
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LlmError {}

/// 一次模型请求（payload 已含 model/messages/tools；client 注入 stream:true）。
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub payload: serde_json::Value,
}

/// LLM 端口：loop 依赖本 trait 而非具体 client（测试用脚本 mock）。
/// 流被 drop 即断连（取消语义）。
pub trait Llm: Send + Sync {
    fn stream_chat(&self, req: LlmRequest) -> impl tokio_stream::Stream<Item = Result<LlmEvent, LlmError>> + Send;
}

// ============================================================================
// OpenAI 兼容 client
// ============================================================================

/// 提供商端点配置（由集成方从 bm-core providers 配置解析注入）。
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// 如 `https://api.deepseek.com/v1`（不含尾部斜杠；chat/completions 由 client 拼接）
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// 提供商标识（request/header 审计用，纯透传）
    pub provider: Option<String>,
}

/// OpenAI 兼容流式 client。
pub struct OpenAiClient {
    cfg: LlmConfig,
    http: reqwest::Client,
}

impl OpenAiClient {
    pub fn new(cfg: LlmConfig) -> Self {
        Self {
            cfg,
            http: reqwest::Client::new(),
        }
    }

    /// 自定义 http client（测试注入超时/代理等）。
    pub fn with_http(cfg: LlmConfig, http: reqwest::Client) -> Self {
        Self { cfg, http }
    }

    pub fn config(&self) -> &LlmConfig {
        &self.cfg
    }
}

impl Llm for OpenAiClient {
    fn stream_chat(&self, req: LlmRequest) -> impl tokio_stream::Stream<Item = Result<LlmEvent, LlmError>> + Send {
        let cfg = self.cfg.clone();
        let http = self.http.clone();
        async_stream::stream! {
            let mut payload = req.payload;
            let obj = payload
                .as_object_mut()
                .ok_or_else(|| LlmError::new("请求 payload 必须是对象", false))?;
            obj.insert("stream".into(), serde_json::Value::Bool(true));
            if obj.get("model").is_none() {
                obj.insert("model".into(), serde_json::Value::String(cfg.model.clone()));
            }
            // 排障观测：请求面的 tools 数量（工具未上 payload 是"模型不调工具"的头号嫌疑）。
            // 注意：不打 payload 全文——含用户消息，日志是审计之家但不该存敏感正文。
            tracing::debug!(
                event = "bm.loop_request",
                model = %cfg.model,
                tools = obj.get("tools").and_then(|t| t.as_array()).map_or(0, |a| a.len()),
            );

            let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
            let resp = match http
                .post(&url)
                .bearer_auth(&cfg.api_key)
                .json(&payload)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    // 连接类错误：可重试
                    yield Err(LlmError::new(format!("请求失败: {e}"), true));
                    return;
                }
            };

            let status = resp.status();
            if !status.is_success() {
                let code = status.as_u16();
                let body = resp.text().await.unwrap_or_default();
                yield Err(LlmError::new(
                    format!("上游返回 {code}: {}", snippet(&body)),
                    code == 429 || code >= 500,
                ));
                return;
            }

            // 非流式兜底：个别提供商忽略 stream 参数返回整包 JSON
            let is_sse = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|ct| ct.contains("text/event-stream"));
            if !is_sse {
                match resp.json::<serde_json::Value>().await {
                    Ok(v) => match parse_completion(&v) {
                        Ok(Some(ev)) => yield Ok(ev),
                        Ok(None) => {}
                        Err(e) => yield Err(LlmError::new(format!("非流式响应解析失败: {e}"), false)),
                    },
                    Err(e) => yield Err(LlmError::new(format!("读取响应失败: {e}"), false)),
                }
                return;
            }

            // SSE 消费：行缓冲器（chunk 边界可能切断行/帧）
            let mut buf = SseFrameBuf::new();
            let mut parser = CompletionParser::new();
            let mut stream = resp.bytes_stream();
            loop {
                match stream.next().await {
                    Some(Ok(bytes)) => {
                        for frame in buf.feed(&bytes) {
                            for ev in parser.feed_frame(&frame) {
                                yield Ok(ev);
                            }
                        }
                    }
                    Some(Err(e)) => {
                        // 流中断：可重试（已产出的事件不重复，loop 重试整步）
                        yield Err(LlmError::new(format!("流中断: {e}"), true));
                        return;
                    }
                    None => break,
                }
            }
            for frame in buf.finish() {
                for ev in parser.feed_frame(&frame) {
                    yield Ok(ev);
                }
            }
            // 收口：ToolCallEnd（完整参数）+ MessageEnd（权威内容/用量）
            for ev in parser.finish() {
                yield Ok(ev);
            }
        }
    }
}

fn snippet(body: &str) -> String {
    let body = body.trim();
    if body.len() <= 300 {
        body.to_string()
    } else {
        let mut s: String = body.chars().take(300).collect();
        s.push('…');
        s
    }
}

// ============================================================================
// SSE 帧缓冲（bytes_stream chunk → 完整 data: 帧）
// ============================================================================

/// SSE `data:` 帧累积器：chunk 可能切断行，行内拼接、按空行出帧。
#[derive(Debug, Default)]
pub struct SseFrameBuf {
    buf: String,
}

impl SseFrameBuf {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入字节，返回切分出的完整帧（多行 data: 已合并、[DONE] 标记保留）。
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        self.drain()
    }

    /// 流结束时排空余留（无尾空行的最后一帧）。
    pub fn finish(mut self) -> Vec<String> {
        if !self.buf.trim().is_empty() {
            self.buf.push('\n');
        }
        self.drain()
    }

    fn drain(&mut self) -> Vec<String> {
        let mut frames = Vec::new();
        while let Some(pos) = self.buf.find("\n\n") {
            let raw = self.buf[..pos].to_string();
            self.buf.drain(..pos + 2);
            let data = join_data_lines(&raw);
            if !data.is_empty() {
                frames.push(data);
            }
        }
        frames
    }
}

/// 单帧内多条 `data:` 行合并（OpenAI 事件单行即单帧，规范要求多行拼接）。
fn join_data_lines(raw: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(data) = line.strip_prefix("data:") {
            parts.push(data.trim_start().to_string());
        }
        // 其余行（event:/id:/注释）忽略——client 只消费 data 负载
    }
    parts.join("\n")
}

// ============================================================================
// chat.completion chunk 解析（纯函数，可单测）
// ============================================================================

/// 流式 chunk → LlmEvent 解析器（状态：工具调用按 index 累积）。
#[derive(Debug, Default)]
struct CompletionParser {
    content: String,
    /// 按 index 累积的工具调用（id/name/args 增量拼接）
    tool_calls: Vec<(String, String, String)>, // (id, name, arguments)
    usage: Option<LlmUsage>,
}

impl CompletionParser {
    fn new() -> Self {
        Self::default()
    }

    /// 喂一个 SSE 帧（data 内容），返回本帧产生的事件。
    fn feed_frame(&mut self, frame: &str) -> Vec<LlmEvent> {
        if frame.trim() == "[DONE]" {
            return Vec::new();
        }
        let v: serde_json::Value = match serde_json::from_str(frame) {
            Ok(v) => v,
            Err(_) => return Vec::new(), // 心跳/非 JSON 帧忽略
        };
        let mut out = Vec::new();
        if let Some(usage) = v.get("usage") {
            self.usage = Some(LlmUsage {
                input_tokens: usage.get("prompt_tokens").and_then(|u| u.as_u64()).unwrap_or(0),
                output_tokens: usage.get("completion_tokens").and_then(|u| u.as_u64()).unwrap_or(0),
            });
        }
        let Some(choices) = v.get("choices").and_then(|c| c.as_array()) else {
            return out;
        };
        for choice in choices {
            let Some(delta) = choice.get("delta") else { continue };
            if let Some(text) = delta.get("content").and_then(|c| c.as_str())
                && !text.is_empty()
            {
                self.content.push_str(text);
                out.push(LlmEvent::TextDelta { text: text.to_string() });
            }
            let Some(tc_deltas) = delta.get("tool_calls").and_then(|t| t.as_array()) else {
                continue;
            };
            for tc in tc_deltas {
                let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                while self.tool_calls.len() <= index {
                    self.tool_calls.push((String::new(), String::new(), String::new()));
                }
                let entry = &mut self.tool_calls[index];
                if let Some(id) = tc.get("id").and_then(|i| i.as_str())
                    && !id.is_empty()
                {
                    entry.0 = id.to_string();
                }
                if let Some(f) = tc.get("function") {
                    if let Some(name) = f.get("name").and_then(|n| n.as_str())
                        && !name.is_empty()
                    {
                        entry.1 = name.to_string();
                        out.push(LlmEvent::ToolCallStart {
                            id: entry.0.clone(),
                            name: entry.1.clone(),
                        });
                    }
                    if let Some(args) = f.get("arguments").and_then(|a| a.as_str())
                        && !args.is_empty()
                    {
                        entry.2.push_str(args);
                        out.push(LlmEvent::ToolCallArgs {
                            id: entry.0.clone(),
                            args_delta: args.to_string(),
                        });
                    }
                }
            }
        }
        out
    }

    /// 流结束收口：全部工具调用的 ToolCallEnd + MessageEnd。
    fn finish(mut self) -> Vec<LlmEvent> {
        let mut out = Vec::new();
        let tool_calls: Vec<LlmToolCall> = std::mem::take(&mut self.tool_calls)
            .into_iter()
            .filter(|(id, name, _)| !id.is_empty() || !name.is_empty())
            .map(|(id, name, arguments)| {
                out.push(LlmEvent::ToolCallEnd {
                    id: id.clone(),
                    arguments: arguments.clone(),
                });
                LlmToolCall { id, name, arguments }
            })
            .collect();
        out.push(LlmEvent::MessageEnd {
            content: std::mem::take(&mut self.content),
            tool_calls,
            usage: self.usage,
        });
        out
    }
}

/// 非流式兜底：整包 completion JSON → 单条 MessageEnd。
fn parse_completion(v: &serde_json::Value) -> Result<Option<LlmEvent>, String> {
    let Some(choices) = v.get("choices").and_then(|c| c.as_array()) else {
        return Err("缺少 choices 数组".into());
    };
    let Some(first) = choices.first() else {
        return Ok(None);
    };
    let message = first.get("message").cloned().unwrap_or_else(|| serde_json::json!({}));
    let content = message
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let tool_calls: Vec<LlmToolCall> = message
        .get("tool_calls")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    let f = tc.get("function")?;
                    Some(LlmToolCall {
                        id: tc.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string(),
                        name: f.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                        arguments: f
                            .get("arguments")
                            .and_then(|a| a.as_str())
                            .unwrap_or("{}")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let usage = v.get("usage").map(|u| LlmUsage {
        input_tokens: u.get("prompt_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
        output_tokens: u.get("completion_tokens").and_then(|x| x.as_u64()).unwrap_or(0),
    });
    Ok(Some(LlmEvent::MessageEnd {
        content,
        tool_calls,
        usage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(delta: serde_json::Value) -> String {
        serde_json::to_string(&serde_json::json!({"choices": [{"delta": delta}]})).unwrap()
    }

    #[test]
    fn sse_frame_buf_splits_and_joins() {
        let mut buf = SseFrameBuf::new();
        // 跨 chunk 切断的行
        assert!(buf.feed(b"data: {\"choices\":[{\"de").is_empty());
        assert!(buf.feed(b"lta\":{\"content\":\"a\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\n").len() >= 2);
        // 多行 data 合并
        let mut b2 = SseFrameBuf::new();
        let frames = b2.feed(b"data: line1\ndata: line2\n\n");
        assert_eq!(frames, vec!["line1\nline2"]);
    }

    #[test]
    fn sse_frame_buf_finish_drains_tail() {
        let mut buf = SseFrameBuf::new();
        buf.feed(b"data: {\"a\":1}\n");
        let frames = buf.finish();
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn parser_accumulates_delta_and_tool_calls() {
        let mut p = CompletionParser::new();
        let evs = p.feed_frame(&chunk(serde_json::json!({"content": "hi"})));
        assert_eq!(evs, vec![LlmEvent::TextDelta { text: "hi".into() }]);

        let evs = p.feed_frame(&chunk(serde_json::json!({"tool_calls": [
            {"index": 0, "id": "c1", "function": {"name": "web_search", "arguments": "{\"q\":"}}
        ]})));
        assert_eq!(
            evs,
            vec![
                LlmEvent::ToolCallStart { id: "c1".into(), name: "web_search".into() },
                LlmEvent::ToolCallArgs { id: "c1".into(), args_delta: "{\"q\":".into() },
            ],
            "同帧携带 name+arguments 时两事件都产出"
        );

        let evs = p.feed_frame(&chunk(serde_json::json!({"tool_calls": [
            {"index": 0, "function": {"arguments": "\"rust\"}"}}
        ]})));
        assert_eq!(
            evs,
            vec![LlmEvent::ToolCallArgs { id: "c1".into(), args_delta: "\"rust\"}".into() }]
        );

        // 收口：ToolCallEnd（完整参数）→ MessageEnd
        let evs = p.finish();
        assert_eq!(evs.len(), 2);
        assert_eq!(
            evs[0],
            LlmEvent::ToolCallEnd { id: "c1".into(), arguments: "{\"q\":\"rust\"}".into() }
        );
        match &evs[1] {
            LlmEvent::MessageEnd { content, tool_calls, .. } => {
                assert_eq!(content, "hi");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].name, "web_search");
                assert_eq!(tool_calls[0].arguments, "{\"q\":\"rust\"}");
            }
            other => panic!("应为 MessageEnd，得到 {other:?}"),
        }
    }

    #[test]
    fn parser_ignores_done_and_garbage_frames() {
        let mut p = CompletionParser::new();
        assert!(p.feed_frame("[DONE]").is_empty());
        assert!(p.feed_frame(":keep-alive").is_empty());
        assert!(p.feed_frame("not json").is_empty());
        assert!(p.feed_frame("{\"unrelated\": true}").is_empty());
    }

    #[test]
    fn parser_captures_usage_from_last_chunk() {
        let mut p = CompletionParser::new();
        let frame = r#"{"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#;
        p.feed_frame(frame);
        let evs = p.finish();
        match evs.last().unwrap() {
            LlmEvent::MessageEnd { usage, .. } => {
                assert_eq!(usage.unwrap(), LlmUsage { input_tokens: 10, output_tokens: 5 });
            }
            other => panic!("应为 MessageEnd，得到 {other:?}"),
        }
    }

    #[test]
    fn non_stream_fallback_parses_full_json() {
        let v = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "done",
                    "tool_calls": [{"id": "t1", "function": {"name": "exec", "arguments": "{}"}}]
                }
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 2}
        });
        match parse_completion(&v).unwrap().unwrap() {
            LlmEvent::MessageEnd { content, tool_calls, usage } => {
                assert_eq!(content, "done");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(usage.unwrap().output_tokens, 2);
            }
            other => panic!("应为 MessageEnd，得到 {other:?}"),
        }
    }

    #[test]
    fn snippet_truncates_long_bodies() {
        let long = "x".repeat(1000);
        assert_eq!(snippet(&long).chars().count(), 301); // 300 + …
    }
}
