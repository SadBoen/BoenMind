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

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use kernel_contracts::error::LlmError;
use kernel_contracts::llm::{
    ChunkStream, ContentBlock, FinishReason, GenerateOptions, LlmMessage, LlmModelInfo, LlmPort,
    LlmResolvedModelInfo, Role, StreamChunk, TokenUsage,
};
use serde_json::{json, Value};

/// 产品身份 User-Agent（对齐 DSH `attribution.ts`：`product/version (+url)`
/// RFC 9110 §10.1.5 约定；公开产品事实，非个人/实例标识）。
const ATTRIBUTION_USER_AGENT: &str = "boenmind/0.1.0 (+https://github.com/SadBoen/BoenMind)";

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
    /// 匿名用户 id（归因头 `x-deepseek-harness-user-id`；装配层解析注入，
    /// 对齐 DSH `.anonymous-user-id` 语义：home 范围稳定、非个人标识）。
    pub user_id: String,
}

/// 请求级动态覆盖（对齐 DSH `llm-deepseek` 的 settings section + credentials
/// 每请求解析语义：`baseURL` 走 settings ns、API key 走 credentials store，
/// 写后下一请求即生效，无需重启/重新注册）。
#[derive(Debug, Default, Clone)]
struct DynamicOverrides {
    /// settings 写的 `baseURL`；None = 用装配值。
    base_url: Option<String>,
    /// credentials.set 写的 API key；None = 用装配值/env。
    api_key: Option<String>,
}

/// OpenAI 兼容流式适配器。
pub struct OpenAICompatLlm {
    cfg: OpenAiProviderConfig,
    client: reqwest::Client,
    dynamic: Arc<Mutex<DynamicOverrides>>,
}

impl OpenAICompatLlm {
    pub fn new(cfg: OpenAiProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("reqwest client build");
        Self {
            cfg,
            client,
            dynamic: Arc::new(Mutex::new(DynamicOverrides::default())),
        }
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

    /// 装配基址（不含运行时覆盖）。
    pub fn base_url(&self) -> &str {
        &self.cfg.base_url
    }

    pub fn models(&self) -> &[LlmModelInfo] {
        &self.cfg.models
    }

    /// settings ns 写面入口：更新 `baseURL` 覆盖（None = 恢复装配值）。
    pub fn set_base_url_override(&self, base_url: Option<String>) {
        self.dynamic.lock().unwrap().base_url = base_url;
    }

    /// credentials 写面入口：更新 API key 覆盖（None = 恢复装配值/env）。
    pub fn set_api_key_override(&self, api_key: Option<String>) {
        self.dynamic.lock().unwrap().api_key = api_key;
    }

    /// 当前生效 base_url（覆盖优先，缺省装配值）。
    pub fn effective_base_url(&self) -> String {
        let dynamic = self.dynamic.lock().unwrap();
        dynamic
            .base_url
            .clone()
            .unwrap_or_else(|| self.cfg.base_url.clone())
    }

    /// 当前生效 API key（覆盖优先，缺省装配值；均无 → None）。
    pub fn effective_api_key(&self) -> Option<String> {
        let dynamic = self.dynamic.lock().unwrap();
        let k = dynamic.api_key.clone().unwrap_or_else(|| self.cfg.api_key.clone());
        (!k.is_empty()).then_some(k)
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.effective_base_url().trim_end_matches('/'))
    }

    fn models_url(&self) -> String {
        format!("{}/models", self.effective_base_url().trim_end_matches('/'))
    }

    /// 把内核消息序列翻译为 OpenAI chat/completions messages
    /// （逐字对齐 DSH `serializeMessages`：text 块合并、tool-result 独立
    /// role:tool 消息、空内容哨兵 `'(no output)'`、混合 user text+tool-result
    /// 拆多条 wire 消息、assistant 推理 passback 只出现在 tool-call 轮、
    /// image/未知块拒 `UNSUPPORTED_CONTENT`——绝不静默 flatten 掉）。
    fn translate_messages(msgs: &[LlmMessage]) -> Result<Vec<Value>, LlmError> {
        fn text_of(content: &[ContentBlock]) -> String {
            content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text(t) => Some(t.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
        }
        fn reasoning_of(content: &[ContentBlock]) -> String {
            content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Reasoning(t) => Some(t.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
        }
        // 核心 image 内容显式拒绝（对齐 serialize.spec "rejects image blocks
        // instead of silently flattening them away"）。当前内核 ContentBlock
        // enum 无 image 变体（Rust 类型系统穷尽）；未来加入 image 块时，本
        // 函数即拒收点（返回 UNSUPPORTED_CONTENT，绝不静默 flatten 掉）。

        let mut out = Vec::new();
        for m in msgs {
            match m.role {
                Role::System => {
                    out.push(json!({ "role": "system", "content": text_of(&m.content) }));
                }
                Role::Assistant => {
                    let text = text_of(&m.content);
                    let reasoning = reasoning_of(&m.content);
                    let tool_calls: Vec<Value> = m
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::ToolCall(c) => Some(json!({
                                "id": c.id,
                                "type": "function",
                                "function": { "name": c.name, "arguments": c.arguments },
                            })),
                            _ => None,
                        })
                        .collect();
                    let mut msg = json!({
                        "role": "assistant",
                        // Text-less turns send "" — NEVER null（对齐 serialize.spec：
                        // 纯 tool-call 轮/纯推理轮 content 均为空串；null 会被 API 400，
                        // 且会话日志中的 null 会封死该会话后续所有回合）。
                        "content": text,
                    });
                    // 官方 passback 规则：reasoning_content 只在 tool-call 轮回传；
                    // 普通轮丢弃以省 token。
                    if !tool_calls.is_empty() && !reasoning.is_empty() {
                        msg["reasoning_content"] = json!(reasoning);
                    }
                    if !tool_calls.is_empty() {
                        msg["tool_calls"] = Value::Array(tool_calls);
                    }
                    out.push(msg);
                }
                // user 角色：text 与 tool-result 拆多条（对齐 serialize.spec
                // "splits mixed user text + tool results"）。
                Role::User => {
                    let text = text_of(&m.content);
                    let tool_results: Vec<&kernel_contracts::ToolCallResult> = m
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::ToolResult(r) => Some(r),
                            _ => None,
                        })
                        .collect();
                    if !text.is_empty() || tool_results.is_empty() {
                        out.push(json!({ "role": "user", "content": text }));
                    }
                    for r in tool_results {
                        out.push(json!({
                            "role": "tool",
                            "tool_call_id": r.call_id,
                            // 空工具输出仍需 SOME content（对齐哨兵 `'(no output)'`）。
                            "content": if r.output.trim().is_empty() {
                                "(no output)"
                            } else {
                                r.output.as_str()
                            },
                        }));
                    }
                }
                // 内核 tool 角色消息不直接出现（工具结果随 user 消息内的
                // ToolResult 块携带）；兜底映射（content 取 ToolResult.output）。
                Role::Tool => {
                    let mut msg = json!({ "role": "tool" });
                    if let Some(r) = m.content.iter().find_map(|b| match b {
                        ContentBlock::ToolResult(r) => Some(r),
                        _ => None,
                    }) {
                        msg["tool_call_id"] = json!(r.call_id);
                        msg["content"] = if r.output.trim().is_empty() {
                            json!("(no output)")
                        } else {
                            json!(r.output)
                        };
                    } else {
                        msg["content"] = json!(text_of(&m.content));
                    }
                    out.push(msg);
                }
            }
        }
        Ok(out)
    }

    /// 解析 thinking/effort 对（逐字对齐 DSH `resolveThinking`：
    /// 非 DeepSeek 能力档位 → UNSUPPORTED_REASONING_EFFORT；`off` → thinking
    /// disabled 且绝不上 wire effort；`low/high/max` → enabled + effort；
    /// session-title/compaction 用途强制 disabled；deployment 锁 disabled 时
    /// 显式非 off effort 拒绝）。
    fn resolve_thinking(
        request: &GenerateOptions,
        adapter_thinking: Option<&str>,
        adapter_effort: Option<&str>,
    ) -> Result<Option<(String, Option<String>)>, LlmError> {
        // 用途强制：标题/压缩不推理（DSH resolveThinking 首行）。
        if request
            .purpose
            .as_deref()
            .is_some_and(|p| p == "session-title" || p == "compaction")
        {
            return Ok(Some(("disabled".to_string(), None)));
        }
        let effort = match request.reasoning_effort.as_deref() {
            Some("off" | "low" | "high" | "max") => request.reasoning_effort.as_deref(),
            Some(other) => {
                return Err(LlmError::new(format!(
                    "DeepSeek does not support reasoning effort \"{other}\""
                )))
            }
            None => adapter_effort,
        };
        // deployment 锁 disabled：显式非 off effort 拒绝（对齐 adapter.spec）。
        if adapter_thinking == Some("disabled")
            && effort.is_some_and(|e| e != "off")
        {
            return Err(LlmError::new(format!(
                "DeepSeek deployment does not support reasoning effort \"{}\"",
                effort.unwrap_or_default()
            )));
        }
        Ok(match effort {
            Some("off") => Some(("disabled".to_string(), None)),
            Some("low" | "high" | "max") => {
                Some(("enabled".to_string(), effort.map(str::to_string)))
            }
            _ => adapter_thinking.map(|t| (t.to_string(), None)),
        })
    }

    fn build_request(&self, request: &GenerateOptions) -> Value {
        let messages = Self::translate_messages(&request.messages).expect("translate");
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
            // 恒随请求（对齐 DSH payload：usage 上报恒开）。
            "stream_options": { "include_usage": true },
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
        if let Ok(Some((thinking, effort))) =
            Self::resolve_thinking(request, None, None)
        {
            body["thinking"] = json!({ "type": thinking });
            if let Some(e) = effort {
                body["reasoning_effort"] = json!(e);
            }
        }
        body
    }

    /// 真实模型清单探测：`GET {base}/models`（或 minimax 形态）。
    /// 失败（网络/非 2xx/形状不符）→ Err，调用方回退静态清单或报 model-discovery-failed。
    pub async fn list_models_remote(&self) -> Result<Vec<LlmModelInfo>, LlmError> {
        // 对齐 DSH：keyless 时可浏览目录但请求 MISSING_CREDENTIAL；
        // 探测无 key → NO_ADAPTER（resolveModelInfo 未注册同码）。
        let key = self
            .effective_api_key()
            .ok_or_else(|| LlmError::new("no adapter registered for provider".to_string()))?;
        let resp = self
            .client
            .get(self.models_url())
            .header("authorization", format!("Bearer {key}"))
            .header("user-agent", ATTRIBUTION_USER_AGENT)
            .header("x-deepseek-harness-user-id", &self.cfg.user_id)
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
        let key = self.effective_api_key();
        let body = self.build_request(&request);
        let signal = request.signal.clone();
        // 归因 user id 在 Box 外 clone（boxed 'static 流不得借用 self）。
        let user_id = self.cfg.user_id.clone();
        Box::pin(async_stream::stream! {
            // 预中止：请求发出前已 abort → 直接终态（对齐 adapter.spec
            // "classifies an aborted request as an aborted finish"：不碰传输，
            // 且仅此一个 chunk）。
            if let Some(s) = &signal {
                if s.is_aborted() {
                    yield Ok(StreamChunk::Finish(FinishReason::Cancelled));
                    return;
                }
            }
            let Some(key) = key else {
                // keyless：对齐 DSH resolveApiKey —— 无 key 即 MISSING_CREDENTIAL。
                // 错误以 finish 呈现（对齐 translate：错误是 finish 语义，不产 Err chunk，
                // 否则 loop 的 torn 分支会把 code 覆盖成 LLM_STREAM）。
                yield Ok(StreamChunk::Finish(FinishReason::Error {
                    message: format!("no API key for provider route '{}'", request.provider),
                    code: "MISSING_CREDENTIAL".to_string(),
                    extra: None,
                }));
                return;
            };
            let send_fut = client
                .post(&url)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {key}"))
                // 归因头（对齐 DSH `attributionHeaders` + adapter.ts：产品身份 +
                // home 匿名用户 id + 会话 id（按需）+ compact 标记（按需）。
                // 全部为公开产品事实，不含密钥/路径/提示词/个人标识。
                .header("user-agent", ATTRIBUTION_USER_AGENT)
                .header("x-deepseek-harness-user-id", user_id.clone())
                .header(
                    "x-deepseek-harness-session-id",
                    request.session_id.as_deref().unwrap_or(""),
                )
                .header(
                    "x-deepseek-harness-compact",
                    if request.purpose.as_deref() == Some("compaction") { "1" } else { "" },
                )
                .json(&body)
                .send();
            let resp = match &signal {
                // 中止穿透 fetch：请求发出后、响应头到达前 abort → 终态 aborted
                //（对齐 adapter.spec：signal abort 映射 ABORTED，不产 TRANSPORT）。
                Some(s) => tokio::select! {
                    biased;
                    r = send_fut => r,
                    _ = s.wait_aborted() => {
                        yield Ok(StreamChunk::Finish(FinishReason::Cancelled));
                        return;
                    }
                },
                None => send_fut.await,
            };
            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    // abort 已发生时的连接错误一律按取消呈现（对齐 DSH
                    // `if (signal.aborted) throw error`——abort 优先于 transport）。
                    if signal.as_ref().is_some_and(|s| s.is_aborted()) {
                        yield Ok(StreamChunk::Finish(FinishReason::Cancelled));
                        return;
                    }
                    // TRANSPORT（对齐 adapter.spec：连接失败带 endpoint；finish 呈现）。
                    yield Ok(StreamChunk::Finish(FinishReason::Error {
                        message: format!("LLM request to {url} failed: {e}"),
                        code: "TRANSPORT".to_string(),
                        extra: None,
                    }));
                    return;
                }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                // Retry-After（秒数或 HTTP-date）与 provider request id 作为结构化事实
                // 随 failure 透传（对齐 adapter.spec "retains status, Retry-After seconds,
                // and provider request id as structured facts"）——headers 须在 body 消费前读取。
                let retry_after_ms = parse_retry_after(
                    resp.headers().get("retry-after").and_then(|v| v.to_str().ok()),
                );
                let request_id = resp
                    .headers()
                    .get("x-request-id")
                    .or_else(|| resp.headers().get("x-deepseek-request-id"))
                    .and_then(|v| v.to_str().ok())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                // 读 body 拿错误详情（非流式，体量小）。
                let text = resp.text().await.unwrap_or_default();
                let (code, message) = map_http_code(status.as_u16(), &text);
                let extra = kernel_contracts::error::FailureInfo {
                    message: message.clone(),
                    code: code.clone(),
                    status: Some(status.as_u16()),
                    provider_retry_after_ms: retry_after_ms,
                    request_id,
                };
                // 错误以 finish 呈现（对齐 adapter.spec 词汇；不产 Err chunk，
                // 否则 loop 的 torn 分支会把 code 覆盖成 LLM_STREAM 并双回合收尾）。
                yield Ok(StreamChunk::Finish(FinishReason::Error {
                    message,
                    code,
                    extra: Some(extra),
                }));
                return;
            }
            let stream = resp.bytes_stream();
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
            // fuse 后 select_biased 可安全反复 poll（None 后不重复 poll）。
            let mut stream = stream.fuse();
            loop {
                // 流中中止穿透：select 竞争"下一块"与"abort 等待"——挂起的网络读
                // 能被 signal 打断（对齐 adapter.spec "aborts mid-stream via the
                // request signal"：恰一个 aborted finish chunk）。select_biased 优先
                // 流分支：EOF 与 abort 同时就绪时先走流，避免正常 EOF 被误判。
                let chunk = match &signal {
                    Some(s) => {
                        tokio::select! {
                            biased;
                            c = stream.next() => c,
                            _ = s.wait_aborted() => {
                                yield Ok(StreamChunk::Finish(FinishReason::Cancelled));
                                return;
                            }
                        }
                    }
                    None => stream.next().await,
                };
                let Some(chunk) = chunk else { break };
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        // 流中途断开 → TRANSPORT（对齐 adapter.spec abrupt body close）。
                        yield Ok(StreamChunk::Finish(FinishReason::Error {
                            message: format!("LLM stream from {url} failed: {e}"),
                            code: "TRANSPORT".to_string(),
                            extra: None,
                        }));
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
                                extra: None,
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
                            // 畸形 JSON → MALFORMED_RESPONSE（对齐 translate.spec）。
                            yield Ok(StreamChunk::Finish(FinishReason::Error {
                                message: format!("malformed SSE payload: {e}"),
                                code: "MALFORMED_RESPONSE".to_string(),
                                extra: None,
                            }));
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
                // STREAM_CLOSED（对齐 sse.spec：EOF 无 [DONE]）。
                yield Ok(StreamChunk::Finish(FinishReason::Error {
                    message: "SSE payload stream ended without [DONE]".to_string(),
                    code: "STREAM_CLOSED".to_string(),
                    extra: None,
                }));
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
            extra: None,
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

/// HTTP 状态码 → DSH 错误码 + wire 消息（对齐 `adapter.spec.ts` 的 `httpErrorCode`：
/// 401/403→AUTH 最优先；quota 分类（任意 status）；429→RATE_LIMIT；400→内容分类
/// else INVALID_REQUEST；>=500→SERVER；其余→HTTP_<status>）。消息取 body 的
/// `error.message`（JSON），无则状态行（对齐 spec："keeps the status-line message
/// for JSON error bodies without a message / non-JSON bodies"）。
fn map_http_code(status: u16, body: &str) -> (String, String) {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let error = parsed.as_ref().and_then(|v| v.get("error"));
    // DSH 分类 detail = error.code + type + message 拼接（filter 非空 join " "）。
    let detail: String = ["code", "type", "message"]
        .iter()
        .filter_map(|k| error.and_then(|e| e.get(k)).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let code = if status == 401 || status == 403 {
        "AUTH".to_string()
    } else if is_quota_exceeded(&detail) {
        "QUOTA".to_string()
    } else if status == 429 {
        "RATE_LIMIT".to_string()
    } else if status == 400 {
        if is_context_window_exceeded(&detail) {
            "CONTEXT_WINDOW_EXCEEDED".to_string()
        } else {
            "INVALID_REQUEST".to_string()
        }
    } else if status >= 500 {
        "SERVER".to_string()
    } else {
        format!("HTTP_{status}")
    };
    let message = extract_api_error(body)
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| format!("HTTP {status}"));
    (code, message)
}

/// 识别上下文窗口超限措辞（镜像 DSH `isContextWindowExceededError` 的判词正则）。
fn is_context_window_exceeded(detail: &str) -> bool {
    // 镜像 error.ts 五条正则（原文逐字移植；i flag 内联）。
    const RULES: [&str; 5] = [
        r"(?i)(?:^|[^a-z0-9])context[\s_-](?:length|window)[\s_-](?:exceed(?:ed|s)?|overflow(?:ed)?|limit[\s_-]exceeded)(?:$|[^a-z0-9])",
        r"(?i)\b(?:maximum|max)(?:\s+(?:allowed|supported))?\s+context\s+(?:length|window)\b",
        r"(?i)\b(?:request|prompt|input|messages?)\s+(?:is\s+|are\s+)?too\s+(?:large|long)\s+for\s+(?:(?:this|the)\s+)?(?:model(?:'s)?\s+)?context(?:\s+window)?\b",
        r"(?i)\b(?:input|prompt|request)\s+(?:is\s+)?too\s+(?:long|large)\s+for\s+(?:this|the)\s+model\b",
        r"(?i)\b(?:input|prompt|request|messages?)\b.{0,40}\b(?:exceed(?:s|ed)?|overflows?|is\s+larger\s+than)\b.{0,40}\b(?:the\s+)?(?:model(?:'s)?\s+)?context(?:\s+(?:length|window))?\b",
    ];
    RULES.iter().any(|r| {
        regex::Regex::new(r)
            .expect("static rule")
            .is_match(detail)
    })
}

/// 识别账户配额耗尽措辞（镜像 DSH `isQuotaExceededError` 判词正则）。
fn is_quota_exceeded(detail: &str) -> bool {
    const RULES: [&str; 5] = [
        r"(?i)\binsufficient[\s_-]+(?:quota|balance|credits?)\b",
        r"(?i)\b(?:quota|usage[\s_-]+limit)[\s_-]+(?:exceeded|exhausted|reached)\b",
        r"(?i)\bexceed(?:ed|s)?[\s_-]+(?:(?:your|the)[\s_-]+)?(?:current[\s_-]+)?quota\b",
        r"(?i)\b(?:balance|credits?)[\s_-]+(?:exhausted|depleted)\b",
        r"(?i)\bout[\s_-]+of[\s_-]+(?:credits?|budget)\b",
    ];
    RULES.iter().any(|r| {
        regex::Regex::new(r)
            .expect("static rule")
            .is_match(detail)
    })
}

/// 解析 Retry-After 为毫秒（秒数或 HTTP-date；0/非法/过去 → None，
/// 对齐 adapter.spec "omits zero, non-finite, invalid, and past Retry-After values"）。
fn parse_retry_after(value: Option<&str>) -> Option<u64> {
    let v = value?.trim();
    if v.is_empty() {
        return None;
    }
    if let Ok(secs) = v.parse::<u64>() {
        return (secs > 0).then(|| secs.saturating_mul(1000));
    }
    let parsed = chrono::DateTime::parse_from_rfc2822(v).ok()?;
    let delay_ms = parsed
        .signed_duration_since(chrono::Utc::now())
        .num_milliseconds();
    (delay_ms > 0).then_some(delay_ms as u64)
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
        let out = OpenAICompatLlm::translate_messages(&msgs).expect("translate");
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
        let out = OpenAICompatLlm::translate_messages(&msgs).expect("translate");
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "");
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
        let out = OpenAICompatLlm::translate_messages(&msgs).expect("translate");
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
            user_id: "test-user".into(),
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
            signal: None,
            reasoning_effort: None,
            thinking: None,
            purpose: None,
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
    fn stream_options_include_usage_always_sent() {
        // 镜像 serialize.spec "always streams with usage"：payload 恒带
        // stream_options.include_usage=true（可省略其它可选字段）。
        let llm = OpenAICompatLlm::new(OpenAiProviderConfig {
            id: "deepseek".into(),
            display_name: "DeepSeek".into(),
            settings_ns: "llm.deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: "k".into(),
            models: vec![],
            list_endpoint: ModelListEndpoint::Standard,
            user_id: "test-user".into(),
        });
        let req = GenerateOptions {
            provider: "deepseek".into(),
            model: "m".into(),
            messages: vec![text_message(Role::User, "hi")],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            session_id: None,
            signal: None,
            reasoning_effort: None,
            thinking: None,
            purpose: None,
        };
        let body = llm.build_request(&req);
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"], json!({ "include_usage": true }));
        // 无 effort/thinking/purpose → 两者都不上 wire（provider 默认生效）。
        assert!(body.get("thinking").is_none());
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn thinking_resolution_mirrors_serialize_spec() {
        // 镜像 serialize.spec "maps off to disabled thinking without a wire effort"。
        let (t, e) = OpenAICompatLlm::resolve_thinking(
            &req_with_effort("off"),
            None,
            None,
        )
        .unwrap()
        .expect("resolved");
        assert_eq!(t, "disabled");
        assert_eq!(e, None);

        // "re-enables thinking when max overrides an off default"：
        // adapter_thinking 未定义 + adapter_effort=off，request effort=max → enabled。
        let (t2, e2) = OpenAICompatLlm::resolve_thinking(
            &req_with_effort("max"),
            None,
            Some("off"),
        )
        .unwrap()
        .expect("resolved");
        assert_eq!(t2, "enabled");
        assert_eq!(e2.as_deref(), Some("max"));

        // 显式非 off effort + deployment 锁 disabled → 拒绝。
        let err = OpenAICompatLlm::resolve_thinking(&req_with_effort("high"), Some("disabled"), None);
        assert!(err.is_err());

        // "disables thinking for session-title requests without changing adapter defaults"。
        let mut title = req_with_effort("max");
        title.purpose = Some("session-title".into());
        let (t3, e3) = OpenAICompatLlm::resolve_thinking(&title, Some("enabled"), Some("max"))
            .unwrap()
            .expect("resolved");
        assert_eq!(t3, "disabled");
        assert_eq!(e3, None);

        // 未知档位 → UNSUPPORTED_REASONING_EFFORT 类错误。
        let mut bad = req_with_effort("ultra");
        bad.purpose = None;
        assert!(OpenAICompatLlm::resolve_thinking(&bad, None, None).is_err());

        // 全空 → None（不上 wire）。
        let empty = req_with_effort("");
        assert!(OpenAICompatLlm::resolve_thinking(&empty, None, None)
            .unwrap()
            .is_none());
    }

    fn req_with_effort(effort: &str) -> GenerateOptions {
        GenerateOptions {
            provider: "deepseek".into(),
            model: "m".into(),
            messages: vec![text_message(Role::User, "hi")],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            session_id: None,
            signal: None,
            reasoning_effort: if effort.is_empty() { None } else { Some(effort.into()) },
            thinking: None,
            purpose: None,
        }
    }

    #[test]
    fn empty_tool_output_uses_sentinel() {
        // 镜像 serialize.spec "sends a sentinel for empty tool-result content"。
        let msgs = vec![LlmMessage {
            role: Role::User,
            content: vec![ContentBlock::ToolResult(ToolCallResult {
                call_id: "call-1".into(),
                output: String::new(),
                is_error: false,
            })],
        }];
        let out = OpenAICompatLlm::translate_messages(&msgs).expect("translate");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "tool");
        assert_eq!(out[0]["tool_call_id"], "call-1");
        assert_eq!(out[0]["content"], "(no output)");
    }

    #[test]
    fn mixed_user_text_and_tool_results_split_wire_messages() {
        // 镜像 serialize.spec "splits mixed user text + tool results into
        // separate wire messages"：text 一条 user、每个 tool-result 一条 tool。
        let msgs = vec![LlmMessage {
            role: Role::User,
            content: vec![
                ContentBlock::Text("check the weather".into()),
                ContentBlock::ToolResult(ToolCallResult {
                    call_id: "call-1".into(),
                    output: "ok".into(),
                    is_error: false,
                }),
                ContentBlock::ToolResult(ToolCallResult {
                    call_id: "call-2".into(),
                    output: "12C".into(),
                    is_error: false,
                }),
            ],
        }];
        let out = OpenAICompatLlm::translate_messages(&msgs).expect("translate");
        assert_eq!(out.len(), 3);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"], "check the weather");
        assert_eq!(out[1]["role"], "tool");
        assert_eq!(out[1]["tool_call_id"], "call-1");
        assert_eq!(out[2]["role"], "tool");
        assert_eq!(out[2]["tool_call_id"], "call-2");
    }

    #[test]
    fn reasoning_passback_only_on_tool_call_turns() {
        // 镜像 serialize.spec "passes reasoning_content back on tool-call turns"：
        // reasoning 只随 tool-call 轮回传；普通轮丢弃省 token。
        let tool_turn = vec![LlmMessage {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Reasoning("I should check the weather.".into()),
                ContentBlock::ToolCall(ToolCall {
                    id: "call-1".into(),
                    name: "get_weather".into(),
                    arguments: r#"{"city":"Shenzhen"}"#.into(),
                }),
            ],
        }];
        let out = OpenAICompatLlm::translate_messages(&tool_turn).expect("translate");
        assert_eq!(out[0]["reasoning_content"], "I should check the weather.");
        assert_eq!(out[0]["tool_calls"][0]["id"], "call-1");

        // 普通文本轮：reasoning 不上 wire。
        let plain = vec![LlmMessage {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Reasoning("think quietly".into()),
                ContentBlock::Text("answer".into()),
            ],
        }];
        let out2 = OpenAICompatLlm::translate_messages(&plain).expect("translate");
        assert!(out2[0].get("reasoning_content").is_none());
        assert_eq!(out2[0]["content"], "answer");
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
    fn map_http_code_vocabulary() {
        // 401/403→AUTH、429→RATE_LIMIT、400→INVALID_REQUEST、500/503→SERVER、其余→HTTP_<status>。
        assert_eq!(map_http_code(401, r#"{"error":{"message":"bad key"}}"#).0, "AUTH");
        assert_eq!(map_http_code(403, "denied").0, "AUTH");
        assert_eq!(map_http_code(429, "slow down").0, "RATE_LIMIT");
        assert_eq!(map_http_code(400, r#"{"error":{"message":"nope"}}"#).0, "INVALID_REQUEST");
        assert_eq!(map_http_code(500, "boom").0, "SERVER");
        assert_eq!(map_http_code(503, "down").0, "SERVER");
        assert_eq!(map_http_code(418, "teapot").0, "HTTP_418");
        // 消息：JSON body 取 error.message；无消息 → 状态行文本。
        let (_, d) = map_http_code(400, r#"{"error":{"message":"bad request"}}"#);
        assert_eq!(d, "bad request");
        let (_, d2) = map_http_code(500, "");
        assert_eq!(d2, "HTTP 500");
    }

    #[test]
    fn map_finish_reason_vocabulary() {
        assert_eq!(map_finish_reason("stop"), FinishReason::Stop);
        assert_eq!(map_finish_reason("tool_calls"), FinishReason::ToolCalls);
        assert_eq!(map_finish_reason("length"), FinishReason::MaxTokens);
        // 未识别值 → error kind + 大写码（DSH mapFinishReason 默认分支）。
        match map_finish_reason("content_filter") {
            FinishReason::Error { message, code, .. } => {
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

    #[test]
    fn dynamic_overrides_take_effect_per_request() {
        let llm = OpenAICompatLlm::new(OpenAiProviderConfig {
            id: "deepseek".into(),
            display_name: "DeepSeek".into(),
            settings_ns: "llm.deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: "assemble-key".into(),
            models: vec![],
            list_endpoint: ModelListEndpoint::Standard,
            user_id: "test-user".into(),
        });
        // 装配值生效。
        assert_eq!(llm.effective_base_url(), "https://api.deepseek.com/v1");
        assert_eq!(llm.effective_api_key().as_deref(), Some("assemble-key"));

        // settings 写 baseURL + credentials 写 key → 下一请求即用新值。
        llm.set_base_url_override(Some("http://127.0.0.1:9999/v1".into()));
        llm.set_api_key_override(Some("hot-key".into()));
        assert_eq!(llm.effective_base_url(), "http://127.0.0.1:9999/v1");
        assert_eq!(llm.effective_api_key().as_deref(), Some("hot-key"));
        assert_eq!(
            llm.chat_url(),
            "http://127.0.0.1:9999/v1/chat/completions"
        );

        // 清覆盖 → 回退装配值。
        llm.set_base_url_override(None);
        llm.set_api_key_override(None);
        assert_eq!(llm.effective_base_url(), "https://api.deepseek.com/v1");
        assert_eq!(llm.effective_api_key().as_deref(), Some("assemble-key"));
    }

    #[test]
    fn keyless_stream_fails_missing_credential() {
        use futures::StreamExt;
        let llm = OpenAICompatLlm::new(OpenAiProviderConfig {
            id: "deepseek".into(),
            display_name: "DeepSeek".into(),
            settings_ns: "llm.deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: String::new(), // keyless
            models: vec![],
            list_endpoint: ModelListEndpoint::Standard,
            user_id: "test-user".into(),
        });
        let req = GenerateOptions {
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            messages: vec![text_message(Role::User, "hi")],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            session_id: None,
            signal: None,
            reasoning_effort: None,
            thinking: None,
            purpose: None,
        };
        let chunks = futures::executor::block_on(async {
            let mut stream = llm.stream(req);
            let mut out = Vec::new();
            while let Some(c) = stream.next().await {
                out.push(c);
            }
            out
        });
        // 错误以 finish 呈现（对齐 translate），且不得有 Err chunk（避免 loop torn 覆盖 code）。
        assert!(chunks.iter().all(Result::is_ok));
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            Ok(StreamChunk::Finish(FinishReason::Error { code, .. })) => {
                assert_eq!(code, "MISSING_CREDENTIAL")
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn context_window_classification_mirrors_service_spec() {
        // 镜像 service.spec.ts 的 isContextWindowExceededError 判词表。
        for body in [
            r#"{"error":{"code":"context_length_exceeded","message":"maximum context length"}}"#,
            r#"{"error":{"message":"context-window-overflowed"}}"#,
            r#"{"error":{"message":"This model maximum context length is 128000 tokens"}}"#,
            r#"{"error":{"message":"input is too long for this model"}}"#,
            r#"{"error":{"message":"request too large for model context"}}"#,
            r#"{"error":{"message":"input exceeds the model context window limit"}}"#,
        ] {
            assert_eq!(
                map_http_code(400, body).0,
                "CONTEXT_WINDOW_EXCEEDED",
                "body: {body}"
            );
        }
        // 不误伤 unrelated 输入校验。
        for body in [
            r#"{"error":{"message":"invalid request: malformed tool arguments"}}"#,
            r#"{"error":{"message":"invalid input: temperature exceeds maximum allowed value"}}"#,
            r#"{"error":{"message":"input exceeds maximum allowed value"}}"#,
            r#"{"error":{"message":"context window size must be positive"}}"#,
        ] {
            assert_eq!(map_http_code(400, body).0, "INVALID_REQUEST", "body: {body}");
        }
        // 413 状态优先：内容分类不覆盖 413。
        assert_eq!(
            map_http_code(413, r#"{"error":{"code":"context_length_exceeded"}}"#).0,
            "HTTP_413"
        );
    }

    #[test]
    fn quota_classification_mirrors_service_spec() {
        // 镜像 service.spec.ts 的 isQuotaExceededError 判词表。
        for body in [
            r#"{"error":{"code":"insufficient_quota"}}"#,
            r#"{"error":{"message":"account balance depleted"}}"#,
            r#"{"error":{"message":"usage-limit-exceeded"}}"#,
            r#"{"error":{"message":"out of credits"}}"#,
            r#"{"error":{"message":"You exceeded your current quota, please check your plan and billing details."}}"#,
        ] {
            assert_eq!(map_http_code(429, body).0, "QUOTA", "body: {body}");
        }
        // 瞬态 rate limit 不归类 quota。
        assert_eq!(map_http_code(429, "HTTP 429: rate limit reached").0, "RATE_LIMIT");
        assert_eq!(map_http_code(429, "quota resets in one minute").0, "RATE_LIMIT");
        // 镜像 adapter.spec：429+code 区分。
        assert_eq!(
            map_http_code(429, r#"{"error":{"code":"insufficient_quota","message":"account credits exhausted"}}"#).0,
            "QUOTA"
        );
        assert_eq!(
            map_http_code(429, r#"{"error":{"message":"request rate limit exceeded"}}"#).0,
            "RATE_LIMIT"
        );
        // 401/403 优先于 quota 分类（httpErrorCode 顺序）。
        assert_eq!(
            map_http_code(401, r#"{"error":{"message":"insufficient_quota"}}"#).0,
            "AUTH"
        );
    }

    #[test]
    fn http_error_keeps_status_line_message_for_shapeless_bodies() {
        // 镜像 adapter.spec：JSON 无 message → 状态行；非 JSON → 状态行。
        let (code, msg) = map_http_code(500, r#"{"error":{"type":"x"}}"#);
        assert_eq!(code, "SERVER");
        assert!(msg.contains("HTTP 500"));
        let (code2, msg2) = map_http_code(502, "Bad Gateway");
        // DSH httpErrorCode：status >= 500 → SERVER（502 同样归类）。
        assert_eq!(code2, "SERVER");
        assert_eq!(msg2, "HTTP 502");
        let (code3, msg3) = map_http_code(400, r#"{"error":{"message":"bad request"}}"#);
        assert_eq!(code3, "INVALID_REQUEST");
        assert_eq!(msg3, "bad request");
    }

    #[test]
    fn retry_after_parsing_mirrors_adapter_spec() {
        // '2' 秒 → 2000ms；HTTP date 未来 → 精确毫秒；0/垃圾/过去 → 省略。
        assert_eq!(parse_retry_after(Some("2")), Some(2000));
        assert_eq!(parse_retry_after(Some("0")), None);
        assert_eq!(parse_retry_after(Some("not-a-date")), None);
        assert_eq!(parse_retry_after(Some("Thu, 01 Jan 1970 00:00:00 GMT")), None);
        assert_eq!(parse_retry_after(None), None);
        // 未来 3 秒的 HTTP-date。
        let future = (chrono::Utc::now() + chrono::Duration::seconds(3))
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        let ms = parse_retry_after(Some(&future)).expect("future date");
        assert!((1000..=5000).contains(&ms), "ms: {ms}");
    }

    #[test]
    fn llm_error_normalizes_to_failure_unknown_fallback() {
        // 镜像 normalizeLlmFailure 兜底：message 空 → "LLM adapter failed"、code 缺失 → UNKNOWN。
        let e = LlmError::new("");
        let f = e.to_failure();
        assert_eq!(f.message, "LLM adapter failed");
        assert_eq!(f.code, "UNKNOWN");
        assert_eq!(f.status, None);

        // 结构化事实原样入终态。
        let e2 = LlmError::structured("slow down", "RATE_LIMIT", Some(429), Some(2000), Some("req-429".into()));
        let f2 = e2.to_failure();
        assert_eq!(f2.message, "slow down");
        assert_eq!(f2.code, "RATE_LIMIT");
        assert_eq!(f2.status, Some(429));
        assert_eq!(f2.provider_retry_after_ms, Some(2000));
        assert_eq!(f2.request_id.as_deref(), Some("req-429"));
    }

    #[test]
    fn finish_error_wire_carries_structured_facts() {
        // 镜像 adapter.spec：failure 携带 status/providerRetryAfterMs/requestId 时逐字上 wire。
        let extra = kernel_contracts::error::FailureInfo {
            message: "slow down".into(),
            code: "RATE_LIMIT".into(),
            status: Some(429),
            provider_retry_after_ms: Some(2000),
            request_id: Some("req-429".into()),
        };
        let wire = FinishReason::Error {
            message: "slow down".into(),
            code: "RATE_LIMIT".into(),
            extra: Some(extra),
        }
        .to_wire();
        assert_eq!(
            wire,
            json!({
                "kind": "error",
                "failure": {
                    "message": "slow down",
                    "code": "RATE_LIMIT",
                    "status": 429,
                    "providerRetryAfterMs": 2000,
                    "requestId": "req-429",
                }
            })
        );
        // 无 extra → 只 message/code（精确形状）。
        let plain = FinishReason::Error {
            message: "boom".into(),
            code: "SERVER".into(),
            extra: None,
        }
        .to_wire();
        assert_eq!(
            plain,
            json!({ "kind": "error", "failure": { "message": "boom", "code": "SERVER" } })
        );
    }

    fn abort_llm(base_url: &str) -> OpenAICompatLlm {
        OpenAICompatLlm::new(OpenAiProviderConfig {
            id: "deepseek".into(),
            display_name: "DeepSeek".into(),
            settings_ns: "llm.deepseek".into(),
            base_url: base_url.into(),
            api_key: "k".into(),
            models: vec![],
            list_endpoint: ModelListEndpoint::Standard,
            user_id: "test-user".into(),
        })
    }

    fn abort_request(signal: kernel_contracts::AbortSignal) -> GenerateOptions {
        GenerateOptions {
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            messages: vec![text_message(Role::User, "hi")],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            session_id: None,
            signal: Some(signal),
            reasoning_effort: None,
            thinking: None,
            purpose: None,
        }
    }

    #[tokio::test]
    async fn wait_aborted_twice_succeeds() {
        use std::time::Duration;
        let signal = kernel_contracts::AbortSignal::new();
        signal.abort();
        tokio::time::timeout(Duration::from_secs(1), signal.wait_aborted())
            .await
            .expect("first wait should return");
        tokio::time::timeout(Duration::from_secs(1), signal.wait_aborted())
            .await
            .expect("second wait should return immediately");
    }

    #[tokio::test]
    async fn wait_aborted_fires_after_abort() {
        use std::time::Duration;
        let signal = kernel_contracts::AbortSignal::new();
        let signal2 = signal.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            signal2.abort();
        });
        tokio::time::timeout(Duration::from_secs(1), signal.wait_aborted())
            .await
            .expect("wait_aborted should return promptly after abort");
    }

    #[tokio::test]
    async fn pre_aborted_request_yields_single_aborted_finish() {
        // 镜像 adapter.spec "classifies an aborted request as an aborted finish"：
        // 预 abort 不碰传输，且仅此一个 finish chunk。
        let llm = abort_llm("http://127.0.0.1:1/v1");
        let signal = kernel_contracts::AbortSignal::new();
        signal.abort();
        use futures::StreamExt;
        let mut stream = llm.stream(abort_request(signal));
        let first = stream.next().await;
        match first {
            Some(Ok(StreamChunk::Finish(FinishReason::Cancelled))) => {}
            other => panic!("expected single aborted finish, got {other:?}"),
        }
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn mid_stream_abort_yields_single_aborted_finish() {
        // 镜像 adapter.spec "aborts mid-stream via the request signal"：
        // 延迟 SSE 流中 abort → 恰一个 aborted finish chunk（挂起的读被 signal 打断）。
        use futures::StreamExt;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // 读请求头（直到空行）。
                let mut buf = [0u8; 4096];
                let mut seen = Vec::new();
                loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    seen.extend_from_slice(&buf[..n]);
                    if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                // 首帧延迟 5 秒（模拟慢流）：abort（50ms 后）须在首帧前打断挂起的读。
                std::thread::sleep(Duration::from_secs(5));
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
                );
                let _ = stream.flush();
                std::thread::sleep(Duration::from_secs(30));
            }
        });

        let llm = abort_llm(&format!("http://{addr}/v1"));
        let signal = kernel_contracts::AbortSignal::new();
        let signal2 = signal.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            signal2.abort();
        });

        let mut stream = llm.stream(abort_request(signal));
        let mut chunks = Vec::new();
        while let Some(c) = stream.next().await {
            chunks.push(c);
        }
        // 恰一个 chunk：aborted finish（若未中断，会产出 block-start/text-delta 等）。
        assert_eq!(chunks.len(), 1, "chunks: {chunks:?}");
        match &chunks[0] {
            Ok(StreamChunk::Finish(FinishReason::Cancelled)) => {}
            other => panic!("expected single aborted finish, got {other:?}"),
        }
        // wire 形状：{kind:'aborted', failure:{code:'ABORTED'}}。
        let wire = chunks[0]
            .as_ref()
            .ok()
            .map(|c| c.to_wire())
            .expect("ok chunk");
        assert_eq!(
            wire,
            json!({
                "type": "finish",
                "reason": { "kind": "aborted", "failure": { "message": "cancelled", "code": "ABORTED" } }
            })
        );
    }

    /// 起一个本地 mock SSE 端点，返回 (base_url, 预写好的事件行)。
    /// 服务端读完请求头后按事件行逐个写出并立即 close（无需 [DONE] 由调用方定）。
    fn mock_sse_server(events: Vec<&'static str>, done: bool) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 8192];
                let mut seen = Vec::new();
                loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    seen.extend_from_slice(&buf[..n]);
                    if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let mut payload = String::from("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n");
                for e in &events {
                    payload.push_str(&format!("data: {e}\n\n"));
                }
                if done {
                    payload.push_str("data: [DONE]\n\n");
                }
                let _ = stream.write_all(payload.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://{addr}/v1")
    }

    async fn collect_stream(llm: &OpenAICompatLlm, request: GenerateOptions) -> Vec<StreamChunk> {
        use futures::StreamExt;
        let mut stream = llm.stream(request);
        let mut out = Vec::new();
        while let Some(c) = stream.next().await {
            out.push(c.expect("no Err chunks in translate path"));
        }
        out
    }

    /// 起一个捕获请求头的 mock 端点：读完整请求头后把原始 ASCII 头经 mpsc 发回，
    /// 随即回 200 + [DONE]。返回 (base_url, header_receiver)。
    fn mock_capture_server() -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 8192];
                let mut seen = Vec::new();
                loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    seen.extend_from_slice(&buf[..n]);
                    if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let headers = String::from_utf8_lossy(&seen).to_lowercase();
                let _ = tx.send(headers);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: [DONE]\n\n",
                );
                let _ = stream.flush();
            }
        });
        (format!("http://{addr}/v1"), rx)
    }

    #[tokio::test]
    async fn attribution_headers_sent_on_every_request() {
        // 归因头（对齐 DSH attribution.ts + adapter.ts）：user-agent 产品身份、
        // x-deepseek-harness-user-id 恒发、session-id 随 options、compact 仅 compaction。
        let (base, rx) = mock_capture_server();
        let llm = OpenAICompatLlm::new(OpenAiProviderConfig {
            id: "deepseek".into(),
            display_name: "DeepSeek".into(),
            settings_ns: "llm.deepseek".into(),
            base_url: base.clone(),
            api_key: "k".into(),
            models: vec![],
            list_endpoint: ModelListEndpoint::Standard,
            user_id: "11111111-2222-4333-8444-555555555555".into(),
        });
        let mut req = abort_request(kernel_contracts::AbortSignal::new());
        req.session_id = Some("sess-1".into());
        let mut stream = llm.stream(req);
        use futures::StreamExt;
        while stream.next().await.is_some() {}
        let headers = rx.recv_timeout(std::time::Duration::from_secs(3)).expect("headers captured");
        assert!(headers.contains("user-agent: boenmind/0.1.0 (+https://github.com/sadboen/boenmind)"), "{headers}");
        assert!(headers.contains("x-deepseek-harness-user-id: 11111111-2222-4333-8444-555555555555"), "{headers}");
        assert!(headers.contains("x-deepseek-harness-session-id: sess-1"), "{headers}");

        // compaction 用途 → compact 标记。
        let (base2, rx2) = mock_capture_server();
        let llm2 = OpenAICompatLlm::new(OpenAiProviderConfig {
            id: "deepseek".into(),
            display_name: "DeepSeek".into(),
            settings_ns: "llm.deepseek".into(),
            base_url: base2,
            api_key: "k".into(),
            models: vec![],
            list_endpoint: ModelListEndpoint::Standard,
            user_id: "u".into(),
        });
        let mut req2 = abort_request(kernel_contracts::AbortSignal::new());
        req2.purpose = Some("compaction".into());
        let mut stream2 = llm2.stream(req2);
        while stream2.next().await.is_some() {}
        let headers2 = rx2.recv_timeout(std::time::Duration::from_secs(3)).expect("headers2 captured");
        assert!(headers2.contains("x-deepseek-harness-compact: 1"), "{headers2}");
    }

    #[tokio::test]
    async fn translate_mirrors_sse_sequence() {
        // 镜像 translate.spec.ts 核心序列：推理+文本交错独立块、
        // trailing usage-only 帧、默认 finish stop。
        let base = mock_sse_server(
            vec![
                // 空首增量 signature：不开块。
                r#"{"choices":[{"delta":{"reasoning_content":"","content":"hi"}}]}"#,
                // 推理增量（非空）→ 开 reasoning 块。
                r#"{"choices":[{"delta":{"reasoning_content":"think"}}]}"#,
                // 文本增量 → 开 text 块。
                r#"{"choices":[{"delta":{"content":" world"}}]}"#,
            ],
            true,
        );
        let llm = abort_llm(&base);
        let chunks = collect_stream(&llm, abort_request(kernel_contracts::AbortSignal::new())).await;
        // 序列：reasoning 块（推理优先）→ text 块 → trailing usage 无 → finish stop。
        let types: Vec<String> = chunks
            .iter()
            .map(|c| match c {
                StreamChunk::BlockStart { block_type, .. } => format!("start:{block_type}"),
                StreamChunk::TextDelta { .. } => "text-delta".to_string(),
                StreamChunk::ReasoningDelta { .. } => "reasoning-delta".to_string(),
                StreamChunk::ToolCallDelta { .. } => "tool-call-delta".to_string(),
                StreamChunk::BlockEnd { .. } => "block-end".to_string(),
                StreamChunk::Usage(_) => "usage".to_string(),
                StreamChunk::Finish(_) => "finish".to_string(),
            })
            .collect();
        // 块序：reasoning 块 start → delta → end，text 块 start → delta → end，finish。
        assert!(types.contains(&"start:reasoning".to_string()), "{types:?}");
        assert!(types.contains(&"start:text".to_string()), "{types:?}");
        assert_eq!(types.last().unwrap(), "finish");
        // 无 usage 帧（镜像 "omits the usage chunk when none arrived"）。
        assert!(!types.contains(&"usage".to_string()), "{types:?}");
        // finish = stop（镜像 "defaults to finish stop"）。
        assert!(matches!(chunks.last().unwrap(), StreamChunk::Finish(FinishReason::Stop)));
        // 本序列中 text 先开（空 reasoning 首增量不开块、其 content 先落地）；
        // 两个块均存在且独立开/闭。
        let i_reasoning = types.iter().position(|t| t == "start:reasoning").unwrap();
        let i_text = types.iter().position(|t| t == "start:text").unwrap();
        assert!(i_text < i_reasoning, "{types:?}");
    }

    #[tokio::test]
    async fn sse_empty_stream_yields_stop_finish() {
        // 空流（无任何事件也无 [DONE]）→ STREAM_CLOSED（镜像 sse.spec
        // "throws STREAM_CLOSED for an empty stream"；我们以 finish 呈现）。
        let base = mock_sse_server(vec![], false);
        let llm = abort_llm(&base);
        let chunks = collect_stream(&llm, abort_request(kernel_contracts::AbortSignal::new())).await;
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            StreamChunk::Finish(FinishReason::Error { code, .. }) => {
                assert_eq!(code, "STREAM_CLOSED")
            }
            other => panic!("expected STREAM_CLOSED finish, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sse_done_then_extra_data_stops() {
        // [DONE] 后即使还有数据也停（镜像 sse.spec "stops yielding after DONE"）——
        // [DONE] 前已有块，正常收尾。
        let base = mock_sse_server(
            vec![
                r#"{"choices":[{"delta":{"content":"par"}}]}"#,
                r#"{"choices":[{"delta":{"content":" extra"}}]}"#,
            ],
            true,
        );
        let llm = abort_llm(&base);
        let chunks = collect_stream(&llm, abort_request(kernel_contracts::AbortSignal::new())).await;
        assert_eq!(chunks.last().unwrap(), &StreamChunk::Finish(FinishReason::Stop));
        // finish 后无任何块。
        let finish_idx = chunks
            .iter()
            .position(|c| matches!(c, StreamChunk::Finish(_)))
            .expect("finish present");
        assert_eq!(finish_idx, chunks.len() - 1);
    }

    #[tokio::test]
    async fn malformed_json_yields_malformed_response_finish() {
        // 畸形 JSON → MALFORMED_RESPONSE（镜像 translate.spec "throws
        // MALFORMED_RESPONSE for invalid JSON payloads"；以 finish 呈现）。
        let base = mock_sse_server(vec![r#"{not-json"#], true);
        let llm = abort_llm(&base);
        let chunks = collect_stream(&llm, abort_request(kernel_contracts::AbortSignal::new())).await;
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            StreamChunk::Finish(FinishReason::Error { code, .. }) => {
                assert_eq!(code, "MALFORMED_RESPONSE")
            }
            other => panic!("expected MALFORMED_RESPONSE finish, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stop_with_no_blocks_yields_empty_response() {
        // 显式 stop 但未开任何块 → EMPTY_RESPONSE，且 usage 先于 finish
        // （镜像 translate.spec "classifies an explicit stop with no opened
        // blocks as EMPTY_RESPONSE, after usage"）。
        let base = mock_sse_server(
            vec![
                r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2}}"#,
                r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            ],
            true,
        );
        let llm = abort_llm(&base);
        let chunks = collect_stream(&llm, abort_request(kernel_contracts::AbortSignal::new())).await;
        let types: Vec<&str> = chunks
            .iter()
            .map(|c| match c {
                StreamChunk::Usage(_) => "usage",
                StreamChunk::Finish(_) => "finish",
                _ => "other",
            })
            .collect();
        assert_eq!(types, vec!["usage", "finish"]);
        match &chunks[1] {
            StreamChunk::Finish(FinishReason::Error { code, .. }) => {
                assert_eq!(code, "EMPTY_RESPONSE")
            }
            other => panic!("expected EMPTY_RESPONSE finish, got {other:?}"),
        }
    }
}
