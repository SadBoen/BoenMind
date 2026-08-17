//! OpenAI 兼容 chat/completions 适配器（M3：真 provider 三通道的公共底座）。
//!
//! 覆盖两家真实通道 + 自定义通道的 OpenAI 兼容子集：
//! - minimax（`https://api.minimaxi.com/v1`）——OpenAI 兼容 + reasoning_content 推理增量；
//! - deepseek（`https://api.deepseek.com/v1`）——OpenAI 兼容；
//! - custom——用户自填 base_url 的 OpenAI 兼容端点。
//!
//! 实现要点：
//! - `stream()` 发 `POST {base}/chat/completions`（`stream:true`），逐行解析 SSE；
//! - delta.content → TextDelta；delta.reasoning_content → ReasoningDelta；
//!   delta.tool_calls → ToolCallDelta（按 index 累积，首块带 id），流末补 ToolCallDone；
//!   usage → Usage；finish_reason → Finish。
//! - torn 纪律：HTTP 非 2xx / SSE 流中断 / 缺 [DONE] 都以 `Err(LlmError)` 结束流，
//!   绝不静默截断（上层以 Finish 缺失判 torn）。
//! - 模型清单静态（配置提供）；`llm.discoverModels` 的真实探测走 `list_models_remote`。

use async_trait::async_trait;
use kernel_contracts::error::LlmError;
use kernel_contracts::llm::{
    ChunkStream, ContentBlock, FinishReason, GenerateOptions, LlmMessage, LlmModelInfo, LlmPort,
    Role, StreamChunk, TokenUsage,
};
use serde_json::{json, Value};

/// 模型列表端点形态（探测用）：所有通道均走 OpenAI 标准 `GET {base}/models`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelListEndpoint {
    /// `GET {base}/models`（OpenAI 标准 / deepseek / minimax 实测同构）。
    Standard,
}

/// OpenAI 兼容 provider 配置。
#[derive(Debug, Clone)]
pub struct OpenAiProviderConfig {
    /// provider id（wire 上 llm.providers.provider / llm.models.provider）。
    pub id: String,
    /// 显示名。
    pub display_name: String,
    /// 设置命名空间（llm.providers.settingsNs 契约）。
    pub settings_ns: String,
    /// 基址（含 /v1 段，chat/completions 在其下）。
    pub base_url: String,
    /// API key。
    pub api_key: String,
    /// 静态模型清单。
    pub models: Vec<LlmModelInfo>,
    /// 模型清单探测端点形态。
    pub list_endpoint: ModelListEndpoint,
}

/// OpenAI 兼容流式适配器。
pub struct OpenAICompatLlm {
    cfg: OpenAiProviderConfig,
    client: reqwest::Client,
}

impl OpenAICompatLlm {
    pub fn new(cfg: OpenAiProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("reqwest client build");
        Self { cfg, client }
    }

    pub fn provider_id(&self) -> &str {
        &self.cfg.id
    }

    pub fn display_name(&self) -> &str {
        &self.cfg.display_name
    }

    pub fn settings_ns(&self) -> &str {
        &self.cfg.settings_ns
    }

    pub fn base_url(&self) -> &str {
        &self.cfg.base_url
    }

    pub fn models(&self) -> &[LlmModelInfo] {
        &self.cfg.models
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'))
    }

    fn models_url(&self) -> String {
        format!("{}/models", self.cfg.base_url.trim_end_matches('/'))
    }

    /// 把内核消息序列翻译为 OpenAI chat/completions messages。
    ///
    /// 规则：content 拼为文本字符串（当前内核消息基本是纯文本/工具结果）；
    /// assistant 消息含 ToolCall → `content:null + tool_calls[]`；
    /// tool 消息 → `role:"tool" + tool_call_id + content`；推理块不进请求。
    fn translate_messages(msgs: &[LlmMessage]) -> Vec<Value> {
        let mut out = Vec::new();
        for m in msgs {
            let mut text_parts: Vec<String> = Vec::new();
            let mut tool_calls: Vec<Value> = Vec::new();
            let mut tool_call_id: Option<String> = None;
            for block in &m.content {
                match block {
                    ContentBlock::Text(t) => text_parts.push(t.clone()),
                    ContentBlock::Reasoning(_) => {} // 推理内容不出请求
                    ContentBlock::ToolCall(c) => tool_calls.push(json!({
                        "id": c.id,
                        "type": "function",
                        "function": {
                            "name": c.name,
                            "arguments": serde_json::to_string(&c.arguments).unwrap_or_else(|_| "{}".to_string()),
                        }
                    })),
                    ContentBlock::ToolResult(r) => {
                        tool_call_id = Some(r.call_id.clone());
                        text_parts.push(r.output.clone());
                    }
                }
            }
            let content = text_parts.join("\n");
            let mut msg = json!({ "role": m.role.as_str() });
            match m.role {
                Role::Assistant if !tool_calls.is_empty() => {
                    msg["content"] = Value::Null;
                    msg["tool_calls"] = Value::Array(tool_calls);
                }
                Role::Tool => {
                    if let Some(id) = tool_call_id {
                        msg["tool_call_id"] = json!(id);
                    }
                    msg["content"] = json!(content);
                }
                _ => msg["content"] = json!(content),
            }
            out.push(msg);
        }
        out
    }

    fn build_request(&self, request: &GenerateOptions) -> Value {
        let messages = Self::translate_messages(&request.messages);
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        if let Some(t) = request.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(m) = request.max_tokens {
            body["max_tokens"] = json!(m);
        }
        body
    }

    /// 真实模型清单探测：`GET {base}/models`（或 minimax 形态）。
    /// 失败（网络/非 2xx/形状不符）→ Err，调用方回退静态清单或报 model-discovery-failed。
    pub async fn list_models_remote(&self) -> Result<Vec<LlmModelInfo>, LlmError> {
        let resp = self
            .client
            .get(self.models_url())
            .header("authorization", format!("Bearer {}", self.cfg.api_key))
            .send()
            .await
            .map_err(|e| LlmError::retryable(format!("model discovery request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(LlmError::new(format!(
                "model discovery returned HTTP {}",
                resp.status()
            )));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| LlmError::new(format!("model discovery body parse failed: {e}")))?;
        // OpenAI 标准：{ data: [{id, ...}] }。
        let list = body
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| LlmError::new("model discovery response has no data array"))?;
        let mut models = Vec::new();
        for item in list {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                models.push(LlmModelInfo {
                    id: id.to_string(),
                    label: item
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    supports_tools: true,
                });
            }
        }
        if models.is_empty() {
            return Err(LlmError::new("model discovery returned empty model list"));
        }
        Ok(models)
    }

    /// 流式生成：逐行消费 SSE。
    fn stream_inner(&self, request: GenerateOptions) -> ChunkStream {
        let client = self.client.clone();
        let url = self.chat_url();
        let key = self.cfg.api_key.clone();
        let body = self.build_request(&request);
        let supports_reasoning = request.model.to_lowercase().contains("m3")
            || request.model.to_lowercase().contains("deepseek-reasoner");
        Box::pin(async_stream::stream! {
            let resp = match client
                .post(&url)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {key}"))
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    yield Err(LlmError::retryable(format!("chat request failed: {e}")));
                    return;
                }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                // 读 body 拿错误详情（非流式，体量小）。
                let text = resp.text().await.unwrap_or_default();
                let detail = extract_api_error(&text).unwrap_or_else(|| text.clone());
                yield Err(LlmError::new(format!(
                    "chat completions HTTP {status}: {detail}"
                )));
                return;
            }
            let mut stream = resp.bytes_stream();
            // 按行缓冲 SSE（data: 前缀行）。
            let mut line_buf: Vec<u8> = Vec::new();
            // tool_calls 累积：index -> (id, name, arguments 拼接)。
            let mut tool_calls: std::collections::BTreeMap<usize, (Option<String>, String, String)> =
                std::collections::BTreeMap::new();
            let mut finished: Option<FinishReason> = None;
            // [DONE] 哨兵：内层行循环遇之置位，外层检查后退出。
            let mut done_seen = false;

            use futures::StreamExt;
            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        yield Err(LlmError::retryable(format!("stream read failed: {e}")));
                        return;
                    }
                };
                line_buf.extend_from_slice(&bytes);
                while let Some(pos) = line_buf.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = line_buf.drain(..=pos).collect();
                    let line = String::from_utf8_lossy(&line);
                    let line = line.trim_end_matches(['\r', '\n']);
                    if !line.starts_with("data:") {
                        continue;
                    }
                    let data = line[5..].trim();
                    if data == "[DONE]" {
                        // 流正常结束；tool_calls 补齐 Done 由下方统一处理。
                        done_seen = true;
                        break;
                    }
                    let parsed: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(e) => {
                            yield Err(LlmError::new(format!("SSE data parse failed: {e}")));
                            return;
                        }
                    };
                    // usage（可能出现在最后一帧）。
                    if let Some(u) = parsed.get("usage") {
                        yield Ok(StreamChunk::Usage(TokenUsage {
                            input: u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
                            output: u.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0),
                        }));
                    }
                    let Some(choice) = parsed
                        .get("choices")
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                    else {
                        continue;
                    };
                    let delta = choice.get("delta").cloned().unwrap_or_default();
                    if let Some(fr) = choice.get("finish_reason").and_then(Value::as_str) {
                        finished = Some(match fr {
                            "length" => FinishReason::MaxTokens,
                            "tool_calls" => FinishReason::ToolCalls,
                            "stop" => FinishReason::Stop,
                            _ => FinishReason::Stop,
                        });
                    }
                    // 文本增量。
                    if let Some(t) = delta.get("content").and_then(Value::as_str) {
                        if !t.is_empty() {
                            yield Ok(StreamChunk::TextDelta { text: t.to_string() });
                        }
                    }
                    // 推理增量（deepseek-reasoner / minimax M3 系）。
                    if supports_reasoning {
                        if let Some(t) = delta.get("reasoning_content").and_then(Value::as_str) {
                            if !t.is_empty() {
                                yield Ok(StreamChunk::ReasoningDelta { text: t.to_string() });
                            }
                        }
                    }
                    // 工具调用增量。
                    if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                        for call in calls {
                            let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                            let id = call.get("id").and_then(Value::as_str).map(str::to_string);
                            let fn_name = call
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(Value::as_str)
                                .map(str::to_string);
                            let args_delta = call
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let entry = tool_calls
                                .entry(index)
                                .or_insert_with(|| (None, String::new(), String::new()));
                            if let Some(id) = id {
                                entry.0 = Some(id);
                            }
                            if let Some(n) = fn_name {
                                entry.1 = n;
                            }
                            entry.2.push_str(&args_delta);
                            yield Ok(StreamChunk::ToolCallDelta {
                                index,
                                name: entry.1.clone(),
                                arguments_delta: args_delta,
                            });
                        }
                    }
                }
                if done_seen {
                    break;
                }
            }

            // [DONE] 未出现但流已尽：若是 finish_reason 已到也算正常收尾；
            // 否则 torn。
            if finished.is_none() {
                yield Err(LlmError::new("stream ended without [DONE] or finish_reason (torn)"));
                return;
            }
            // 补齐 ToolCallDone：把累积的 delta 拼成完整 arguments 发出。
            for (index, (id, name, args)) in tool_calls {
                yield Ok(StreamChunk::ToolCallDone {
                    index,
                    name,
                    arguments: args,
                    // 扩展字段无法表达 id；kernel-loop 用 index 生成 call id。
                });
                let _ = id;
            }
            yield Ok(StreamChunk::Finish(finished.unwrap()));
        })
    }
}

/// 从错误响应体尽量提取 API 错误消息（OpenAI 风格 `{"error":{"message":...}}`）。
fn extract_api_error(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.get("error")
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[async_trait]
impl LlmPort for OpenAICompatLlm {
    async fn list_models(&self, provider: &str) -> Result<Vec<LlmModelInfo>, LlmError> {
        if provider == self.cfg.id {
            Ok(self.cfg.models.clone())
        } else {
            Ok(Vec::new())
        }
    }

    fn stream(&self, request: GenerateOptions) -> ChunkStream {
        self.stream_inner(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel_contracts::llm::{text_message, ToolCall, ToolCallResult};
    use kernel_contracts::tools::ToolSchema;

    #[test]
    fn translate_plain_messages() {
        let msgs = vec![
            text_message(Role::System, "you are helpful"),
            text_message(Role::User, "hi"),
        ];
        let out = OpenAICompatLlm::translate_messages(&msgs);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["role"], "system");
        assert_eq!(out[0]["content"], "you are helpful");
        assert_eq!(out[1]["role"], "user");
        assert_eq!(out[1]["content"], "hi");
    }

    #[test]
    fn translate_assistant_tool_call() {
        let msgs = vec![LlmMessage {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolCall(ToolCall {
                id: "call_1".into(),
                name: "echo".into(),
                arguments: json!({ "x": 1 }),
            })],
        }];
        let out = OpenAICompatLlm::translate_messages(&msgs);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], Value::Null);
        assert_eq!(out[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            out[0]["tool_calls"][0]["function"]["arguments"],
            r#"{"x":1}"#
        );
    }

    #[test]
    fn translate_tool_result() {
        let msgs = vec![LlmMessage {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult(ToolCallResult {
                call_id: "call_1".into(),
                output: "echo: 1".into(),
                is_error: false,
            })],
        }];
        let out = OpenAICompatLlm::translate_messages(&msgs);
        assert_eq!(out[0]["role"], "tool");
        assert_eq!(out[0]["tool_call_id"], "call_1");
        assert_eq!(out[0]["content"], "echo: 1");
    }

    #[test]
    fn build_request_shape() {
        let llm = OpenAICompatLlm::new(OpenAiProviderConfig {
            id: "deepseek".into(),
            display_name: "DeepSeek".into(),
            settings_ns: "llm.deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: "k".into(),
            models: vec![],
            list_endpoint: ModelListEndpoint::Standard,
        });
        let req = GenerateOptions {
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            messages: vec![text_message(Role::User, "hi")],
            tools: vec![ToolSchema {
                name: "echo".into(),
                description: "echo".into(),
                parameters: json!({ "type": "object" }),
            }],
            temperature: Some(0.5),
            max_tokens: Some(100),
            session_id: Some("s1".into()),
        };
        let body = llm.build_request(&req);
        assert_eq!(body["model"], "deepseek-chat");
        assert_eq!(body["stream"], true);
        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["max_tokens"], 100);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn extract_error_message() {
        assert_eq!(
            extract_api_error(r#"{"error":{"message":"invalid api key"}}"#).as_deref(),
            Some("invalid api key")
        );
        assert_eq!(extract_api_error("plain text"), None);
    }
}
