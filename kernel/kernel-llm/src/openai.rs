//! OpenAI 兼容 chat/completions 适配器（M3：真 provider 三通道的公共底座）。
//!
//! 覆盖两家真实通道 + 自定义通道的 OpenAI 兼容子集：
//! - minimax（`https://api.minimaxi.com/v1`）——OpenAI 兼容 + reasoning_content 推理增量；
//! - deepseek（`https://api.deepseek.com/v1`）——OpenAI 兼容；
//! - custom——用户自填 base_url 的 OpenAI 兼容端点。
//!
//! SSE 翻译逐字对齐 DSH `packages/llm/llm-deepseek/src/translate.ts`（官方测试
//! `translate.spec.ts` 逐字节验证该协议行为）：
//! - 每个 content/reasoning/tool-call 索引一个状态化 harness 块；空字符串首增量不开块；
//! - 推理优先：thinking 模式把推理增量交错在文本之前；
//! - finish_reason 与最新 usage 推迟到 `[DONE]`（覆盖 finish 附带与尾部 usage-only
//!   两种形状，保证 finish 后无任何块）；
//! - `[DONE]` 缺失 → `STREAM_CLOSED` 错误；畸形 JSON → `MALFORMED_RESPONSE`；
//! - usage 缓存剔除：`prompt_tokens` 含缓存命中（deepseek 语义），按
//!   `prompt_tokens_details.cached_tokens` 或 `prompt_cache_hit_tokens` 减去；
//! - finish stop 但未开任何块 → EMPTY_RESPONSE 错误 finish。

use async_trait::async_trait;
use kernel_contracts::error::LlmError;
use kernel_contracts::llm::{
    ChunkStream, ContentBlock, FinishReason, GenerateOptions, LlmMessage, LlmModelInfo, LlmPort,
    LlmResolvedModelInfo, Role, StreamChunk, TokenUsage,
};
use serde_json::{json, Value};

/// 翻译状态机中的一个 open block（对齐 DSH `translate.ts` OpenBlock）。
struct OpenBlock {
    index: usize,
    kind: String, // 'text' | 'reasoning' | 'tool-call'
    text: String,
    call_id: Option<String>,
    name: Option<String>,
}

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
    /// 静态模型清单（含能力元数据）。
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
                            "arguments": c.arguments,
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
                    // 探测响应不携带能力元数据；已装配静态清单有的话补上。
                    context_window: self
                        .cfg
                        .models
                        .iter()
                        .find(|m| m.id == id)
                        .and_then(|m| m.context_window),
                    max_tokens: self
                        .cfg
                        .models
                        .iter()
                        .find(|m| m.id == id)
                        .and_then(|m| m.max_tokens),
                    reasoning: self
                        .cfg
                        .models
                        .iter()
                        .find(|m| m.id == id)
                        .and_then(|m| m.reasoning.clone()),
                });
            }
        }
        if models.is_empty() {
            return Err(LlmError::new("model discovery returned empty model list"));
        }
        Ok(models)
    }

    /// 流式生成：逐行消费 SSE，按 DSH `translate.ts` 状态机产出 harness 块。
    fn stream_inner(&self, request: GenerateOptions) -> ChunkStream {
        let client = self.client.clone();
        let url = self.chat_url();
        let key = self.cfg.api_key.clone();
        let body = self.build_request(&request);
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
            let mut line_buf: Vec<u8> = Vec::new();
            // ---- translate.ts 状态机 ----
            // 每个 content/reasoning/tool-call 索引一个 open block；索引按开块序递增。
            // 文本块/推理块各至多一个；工具块按 wire index 映射。
            let mut next_index: usize = 0;
            let mut text_block: Option<OpenBlock> = None;
            let mut reasoning_block: Option<OpenBlock> = None;
            let mut tool_blocks: std::collections::BTreeMap<usize, OpenBlock> =
                std::collections::BTreeMap::new();
            // 开块顺序（block-end 按此顺序在 [DONE] 统一发出）。
            let mut order: Vec<usize> = Vec::new();
            let mut pending_finish: Option<FinishReason> = None;
            let mut pending_usage: Option<TokenUsage> = None;
            let mut done_seen = false;

            // 开块并登记顺序。
            fn open(
                kind: String,
                next_index: &mut usize,
                order: &mut Vec<usize>,
                call_id: Option<String>,
                name: Option<String>,
            ) -> OpenBlock {
                let block = OpenBlock {
                    index: *next_index,
                    kind,
                    text: String::new(),
                    call_id,
                    name,
                };
                *next_index += 1;
                order.push(block.index);
                block
            }

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
                'lines: while let Some(pos) = line_buf.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = line_buf.drain(..=pos).collect();
                    let line = String::from_utf8_lossy(&line);
                    let line = line.trim_end_matches(['\r', '\n']);
                    if !line.starts_with("data:") {
                        continue;
                    }
                    let data = line[5..].trim();
                    if data == "[DONE]" {
                        // 翻译器只在 [DONE] 时收尾：先发全部 block-end（开块序），
                        // 再发 usage，最后 finish——保证 finish 后无任何块。
                        done_seen = true;
                        for idx in &order {
                            let b = if let Some(blk) = text_block.as_ref()
                                .filter(|b| &b.index == idx) {
                                Some(close_block(blk))
                            } else if let Some(blk) = reasoning_block.as_ref()
                                .filter(|b| &b.index == idx) {
                                Some(close_block(blk))
                            } else {
                                tool_blocks.get(idx).map(close_block)
                            };
                            if let Some(block) = b {
                                yield Ok(StreamChunk::BlockEnd {
                                    index: *idx,
                                    block,
                                });
                            }
                        }
                        if let Some(u) = pending_usage.take() {
                            yield Ok(StreamChunk::Usage(u));
                        }
                        let reason = pending_finish.take().unwrap_or(FinishReason::Stop);
                        // finish stop 但未开任何块 = 空响应错误（DSH EMPTY_RESPONSE）。
                        let final_reason = if reason == FinishReason::Stop && order.is_empty() {
                            FinishReason::Error {
                                message: "model returned a completed response with no content"
                                    .to_string(),
                                code: "EMPTY_RESPONSE".to_string(),
                            }
                        } else {
                            reason
                        };
                        yield Ok(StreamChunk::Finish(final_reason));
                        break 'lines;
                    }
                    let parsed: Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(e) => {
                            yield Err(LlmError::new(format!(
                                "malformed SSE payload: {e}"
                            )));
                            return;
                        }
                    };
                    // usage 可附在 finish 帧或尾部 usage-only 帧——保留最新。
                    if let Some(u) = parsed.get("usage") {
                        pending_usage = Some(map_usage(u));
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
                        pending_finish = Some(map_finish_reason(fr));
                    }
                    // 推理优先：thinking 模式把它交错在文本之前。
                    // 空字符串首增量不开块（DSH 明确测试该行为）。
                    if let Some(t) = delta.get("reasoning_content").and_then(Value::as_str) {
                        if !t.is_empty() {
                            if reasoning_block.is_none() {
                                let b = open(
                                    "reasoning".to_string(),
                                    &mut next_index,
                                    &mut order,
                                    None,
                                    None,
                                );
                                let idx = b.index;
                                reasoning_block = Some(b);
                                yield Ok(StreamChunk::BlockStart {
                                    index: idx,
                                    block_type: "reasoning".to_string(),
                                });
                            }
                            let idx = reasoning_block.as_ref().unwrap().index;
                            reasoning_block.as_mut().unwrap().text.push_str(t);
                            yield Ok(StreamChunk::ReasoningDelta {
                                index: idx,
                                text: t.to_string(),
                            });
                        }
                    }
                    if let Some(t) = delta.get("content").and_then(Value::as_str) {
                        if !t.is_empty() {
                            if text_block.is_none() {
                                let b = open(
                                    "text".to_string(),
                                    &mut next_index,
                                    &mut order,
                                    None,
                                    None,
                                );
                                let idx = b.index;
                                text_block = Some(b);
                                yield Ok(StreamChunk::BlockStart {
                                    index: idx,
                                    block_type: "text".to_string(),
                                });
                            }
                            let idx = text_block.as_ref().unwrap().index;
                            text_block.as_mut().unwrap().text.push_str(t);
                            yield Ok(StreamChunk::TextDelta {
                                index: idx,
                                text: t.to_string(),
                            });
                        }
                    }
                    if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                        for call in calls {
                            let wire_index =
                                call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                            let was_new = !tool_blocks.contains_key(&wire_index);
                            let block = tool_blocks
                                .entry(wire_index)
                                .or_insert_with(|| {
                                    let id = call
                                        .get("id")
                                        .and_then(Value::as_str)
                                        .map(str::to_string)
                                        .unwrap_or_default();
                                    let name = call
                                        .get("function")
                                        .and_then(|f| f.get("name"))
                                        .and_then(Value::as_str)
                                        .map(str::to_string);
                                    open(
                                        "tool-call".to_string(),
                                        &mut next_index,
                                        &mut order,
                                        Some(id),
                                        name,
                                    )
                                });
                            if was_new {
                                yield Ok(StreamChunk::BlockStart {
                                    index: block.index,
                                    block_type: "tool-call".to_string(),
                                });
                            }
                            if let Some(id) = call.get("id").and_then(Value::as_str) {
                                block.call_id = Some(id.to_string());
                            }
                            if let Some(n) = call
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(Value::as_str)
                            {
                                block.name = Some(n.to_string());
                            }
                            let fragment = call
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            block.text.push_str(&fragment);
                            yield Ok(StreamChunk::ToolCallDelta {
                                index: block.index,
                                id: block.call_id.clone().unwrap_or_default(),
                                name: block.name.clone(),
                                arguments_delta: fragment,
                            });
                        }
                    }
                }
                if done_seen {
                    break;
                }
            }

            // [DONE] 未出现但流已尽：parseSse 保证哨兵（或抛错）——违反契约。
            if !done_seen {
                yield Err(LlmError::new(
                    "SSE payload stream ended without [DONE]",
                ));
                return;
            }
        })
    }
}

/// 组装一个 open block 的最终 ContentBlock（对齐 DSH `closeBlock`）。
fn close_block(block: &OpenBlock) -> ContentBlock {
    match block.kind.as_str() {
        "text" => ContentBlock::Text(block.text.clone()),
        "reasoning" => ContentBlock::Reasoning(block.text.clone()),
        "tool-call" => ContentBlock::ToolCall(kernel_contracts::ToolCall {
            id: block.call_id.clone().unwrap_or_default(),
            name: block.name.clone().unwrap_or_default(),
            arguments: block.text.clone(),
        }),
        _ => ContentBlock::Text(block.text.clone()),
    }
}

/// 映射 wire finish_reason 词汇（对齐 DSH `mapFinishReason`：
/// stop/tool_calls/length；未识别值 → error kind + 大写码）。
fn map_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::Stop,
        "tool_calls" => FinishReason::ToolCalls,
        "length" => FinishReason::MaxTokens,
        // content_filter、insufficient_system_resource、未来新增。
        _ => FinishReason::Error {
            message: format!("model stopped: {reason}"),
            code: reason.to_uppercase(),
        },
    }
}

/// 映射 wire usage（对齐 DSH `mapUsage`）。deepseek 的 `prompt_tokens` 含缓存命中
/// （`prompt_tokens = prompt_cache_hit_tokens + prompt_cache_miss_tokens`），
/// 按 disjoint 约定把缓存读从 input 中剔除；cache/reasoning 字段仅在 wire 报时携带。
fn map_usage(usage: &Value) -> TokenUsage {
    let prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
    let completion_tokens = usage
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_read = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(Value::as_u64)
        .or_else(|| usage.get("prompt_cache_hit_tokens").and_then(Value::as_u64));
    let reasoning = usage
        .get("completion_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(Value::as_u64);
    TokenUsage {
        input: prompt_tokens.saturating_sub(cache_read.unwrap_or(0)),
        output: completion_tokens,
        cache_read,
        cache_write: None,
        reasoning,
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

    async fn resolve_model(&self, provider: &str, model: &str) -> LlmResolvedModelInfo {
        let found = self.cfg.models.iter().find(|m| m.id == model);
        LlmResolvedModelInfo {
            provider: provider.to_string(),
            id: model.to_string(),
            name: found
                .and_then(|m| m.label.clone())
                .unwrap_or_else(|| model.to_string()),
            context_window: found.and_then(|m| m.context_window),
            default_max_tokens: found.and_then(|m| m.max_tokens),
            reasoning: found.and_then(|m| m.reasoning.clone()),
            // chat-completions 路由仅文本；未入目录的模型声明同样的负能力。
            input_modalities: vec!["text".to_string()],
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
                arguments: r#"{"x":1}"#.into(),
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

    #[test]
    fn map_finish_reason_vocabulary() {
        assert_eq!(map_finish_reason("stop"), FinishReason::Stop);
        assert_eq!(map_finish_reason("tool_calls"), FinishReason::ToolCalls);
        assert_eq!(map_finish_reason("length"), FinishReason::MaxTokens);
        // 未识别值 → error kind + 大写码（DSH mapFinishReason 默认分支）。
        match map_finish_reason("content_filter") {
            FinishReason::Error { message, code } => {
                assert_eq!(message, "model stopped: content_filter");
                assert_eq!(code, "CONTENT_FILTER");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn map_usage_disjoint_cache_counts() {
        // DSH mapUsage 全形：prompt_tokens 含缓存命中，input 剔除缓存读。
        let u = map_usage(&json!({
            "prompt_tokens": 283,
            "completion_tokens": 69,
            "prompt_cache_hit_tokens": 256,
            "prompt_cache_miss_tokens": 27,
            "prompt_tokens_details": { "cached_tokens": 256 },
            "completion_tokens_details": { "reasoning_tokens": 24 },
        }));
        assert_eq!(u.input, 27);
        assert_eq!(u.output, 69);
        assert_eq!(u.cache_read, Some(256));
        assert_eq!(u.reasoning, Some(24));

        // 无 details → 回退 prompt_cache_hit_tokens。
        let u2 = map_usage(&json!({ "prompt_tokens": 10, "completion_tokens": 2, "prompt_cache_hit_tokens": 8 }));
        assert_eq!(u2.input, 2);
        assert_eq!(u2.cache_read, Some(8));

        // 无缓存字段 → 原样透传，可选字段省略。
        let u3 = map_usage(&json!({ "prompt_tokens": 10, "completion_tokens": 2 }));
        assert_eq!(u3.input, 10);
        assert_eq!(u3.cache_read, None);
        assert_eq!(u3.reasoning, None);
    }
}
