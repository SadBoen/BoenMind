//! LLM 端口：provider trait 与流式类型。
//!
//! 端口形状对齐 dsh harness 的流协议（`packages/llm/llm/src/types.ts` 的
//! `StreamChunk`）：`stream()` 返回异步流，逐块产出 `StreamChunk`，结束由
//! `Finish` 块标记。块索引（`index`）关联交错的增量，`block-end` 携带组装完
//! 整的块；usage 在 finish 之前发出，finish 之后无任何块。torn 纪律：流以
//! `Err` 结束即中断，调用方以 `Finish` 缺失判定 torn。

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::LlmError;

/// 请求级取消信号（对齐 DSH `AbortController/AbortSignal` 语义：单向置位、
/// 异步等待）。最小自研实现：`AtomicBool`（同步查询）+ `tokio::sync::watch`
/// （异步等待无竞态——subscribe 后 `borrow_and_update` 立即反映已置位状态）。
/// `abort()` 幂等。
#[derive(Clone)]
pub struct AbortSignal {
    inner: Arc<AbortInner>,
}

struct AbortInner {
    aborted: AtomicBool,
    tx: tokio::sync::watch::Sender<bool>,
    /// 永久 receiver：tokio watch 的 `send` 在无活跃 receiver 时返回 Err 且
    /// **不更新值**——保留它保证任意时序的 `abort()`（含订阅前的预中止）
    /// 都让后续 `wait_aborted` 立即读到 true。
    _keep_alive: tokio::sync::watch::Receiver<bool>,
}

impl Default for AbortSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AbortSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AbortSignal")
            .field("aborted", &self.is_aborted())
            .finish()
    }
}

impl AbortSignal {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::watch::channel(false);
        Self {
            inner: Arc::new(AbortInner {
                aborted: AtomicBool::new(false),
                tx,
                _keep_alive: rx,
            }),
        }
    }

    pub fn is_aborted(&self) -> bool {
        self.inner.aborted.load(Ordering::Acquire)
    }

    /// 触发取消：置位并唤醒所有等待者（幂等，仅首次生效）。
    pub fn abort(&self) {
        if self.inner.aborted.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.inner.tx.send(true);
    }

    /// 异步等待取消发生（已置位时立即返回）。用于中断挂起的传输读——
    /// 对齐 DSH "abort penetrates resolveModel and fetch"。
    pub async fn wait_aborted(&self) {
        let mut rx = self.inner.tx.subscribe();
        loop {
            if *rx.borrow_and_update() {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }
}

/// 流式块流。
pub type ChunkStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>;

/// 模型结束原因（对齐 DSH `FinishReasonMap`：kind 词汇 stop/tool-calls/max-tokens/aborted/error）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    MaxTokens,
    /// 工具调用序列结束（模型请求执行工具）。
    ToolCalls,
    /// 错误中断（failure 携带 message/code；`extra` = 结构化 LlmFailure 可选事实
    /// status/providerRetryAfterMs/requestId，None 时 wire 只出 message/code）。
    Error {
        message: String,
        code: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        extra: Option<crate::error::FailureInfo>,
    },
    /// 被取消（wire 上映射 aborted）。
    Cancelled,
}

/// token 用量（对齐 DSH `TokenUsage`：计数 DISJOINT——`input` 是不含缓存命中的
/// 净输入；缓存读/写与推理 token 单独报）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    /// 缓存读取 token（未计入 input）。
    pub cache_read: Option<u64>,
    /// 缓存写入 token（未计入 input）。
    pub cache_write: Option<u64>,
    /// 推理 token（含在 output 内）。
    pub reasoning: Option<u64>,
}

/// 流式增量块（对齐 dsh harness 的 chunk 语义；`packages/llm/llm-deepseek/src/translate.ts` 产出形态）。
///
/// wire 形状（经 `to_wire`）：`{type, index, ...}`——block-start / text-delta /
/// reasoning-delta / tool-call-delta / block-end / usage / finish。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamChunk {
    /// 块开始：`{type:'block-start', index, blockType}`。
    BlockStart { index: usize, block_type: String },
    /// 文本增量：`{type:'text-delta', index, text}`。
    TextDelta { index: usize, text: String },
    /// 推理内容增量：`{type:'reasoning-delta', index, text}`。
    ReasoningDelta { index: usize, text: String },
    /// 工具调用增量：`{type:'tool-call-delta', index, id, name?, argumentsDelta}`。
    ToolCallDelta {
        index: usize,
        id: String,
        name: Option<String>,
        arguments_delta: String,
    },
    /// 块结束：`{type:'block-end', index, block}`——携带组装完整的块（文本/推理/工具调用）。
    BlockEnd { index: usize, block: ContentBlock },
    /// token 用量（流中可出现多次，后到覆盖先到；finish 之前最后一条）。
    Usage(TokenUsage),
    /// 流结束（之后无任何块）。
    Finish(FinishReason),
}

impl StreamChunk {
    /// 转 dsh wire 形状（assistant/chunk 事件的 data.chunk 逐字形态）。
    pub fn to_wire(&self) -> Value {
        match self {
            StreamChunk::BlockStart { index, block_type } => json!({
                "type": "block-start",
                "index": index,
                "blockType": block_type,
            }),
            StreamChunk::TextDelta { index, text } => {
                json!({ "type": "text-delta", "index": index, "text": text })
            }
            StreamChunk::ReasoningDelta { index, text } => {
                json!({ "type": "reasoning-delta", "index": index, "text": text })
            }
            StreamChunk::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                let mut v = json!({
                    "type": "tool-call-delta",
                    "index": index,
                    "id": id,
                    "argumentsDelta": arguments_delta,
                });
                if let Some(n) = name {
                    v["name"] = json!(n);
                }
                v
            }
            StreamChunk::BlockEnd { index, block } => json!({
                "type": "block-end",
                "index": index,
                "block": block_to_wire(block),
            }),
            StreamChunk::Usage(u) => json!({ "type": "usage", "usage": u.to_wire() }),
            StreamChunk::Finish(reason) => json!({ "type": "finish", "reason": reason.to_wire() }),
        }
    }
}

impl FinishReason {
    /// dsh wire 形状：`{kind, failure?}`。
    pub fn to_wire(&self) -> Value {
        match self {
            FinishReason::Stop => json!({ "kind": "stop" }),
            FinishReason::MaxTokens => json!({ "kind": "max-tokens" }),
            FinishReason::ToolCalls => json!({ "kind": "tool-calls" }),
            FinishReason::Error { message, code, extra } => {
                let mut failure = json!({ "message": message, "code": code });
                // 结构化事实透传（对齐 adapter.spec：status/providerRetryAfterMs/requestId
                // 仅在携带时出现——toEqual 精确形状断言）。
                if let Some(e) = extra {
                    if let Some(s) = e.status {
                        failure["status"] = json!(s);
                    }
                    if let Some(ra) = e.provider_retry_after_ms {
                        failure["providerRetryAfterMs"] = json!(ra);
                    }
                    if let Some(rid) = &e.request_id {
                        failure["requestId"] = json!(rid);
                    }
                }
                json!({ "kind": "error", "failure": failure })
            }
            FinishReason::Cancelled => json!({
                "kind": "aborted",
                "failure": { "message": "cancelled", "code": "ABORTED" },
            }),
        }
    }
}

impl TokenUsage {
    /// dsh wire 形状：`{inputTokens, outputTokens, cacheReadTokens?, cacheWriteTokens?, reasoningTokens?}`。
    pub fn to_wire(&self) -> Value {
        let mut v = json!({ "inputTokens": self.input, "outputTokens": self.output });
        if let Some(c) = self.cache_read {
            v["cacheReadTokens"] = json!(c);
        }
        if let Some(c) = self.cache_write {
            v["cacheWriteTokens"] = json!(c);
        }
        if let Some(r) = self.reasoning {
            v["reasoningTokens"] = json!(r);
        }
        v
    }
}

/// 内容块转 wire 形状（assistant/message 与 block-end 共用）。
pub fn block_to_wire(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text(t) => json!({ "type": "text", "text": t }),
        ContentBlock::Reasoning(t) => json!({ "type": "reasoning", "text": t }),
        ContentBlock::ToolCall(c) => json!({
            "type": "tool-call",
            "id": c.id,
            "name": c.name,
            "arguments": c.arguments,
        }),
        ContentBlock::ToolResult(r) => json!({
            "type": "tool-result",
            "callId": r.call_id,
            "output": r.output,
            "isError": r.is_error,
        }),
    }
}

/// 消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
        }
    }
}

/// 内容块：消息的原子成分。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContentBlock {
    Text(String),
    Reasoning(String),
    ToolCall(ToolCall),
    ToolResult(ToolCallResult),
}

/// 工具调用（模型侧发出）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// 模型原始 JSON 文本（未解析——wire 透传保真，对齐 DSH tool/call 契约）。
    pub arguments: String,
}

/// 工具执行结果（回填给模型）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub call_id: String,
    pub output: String,
    pub is_error: bool,
}

/// 发给模型的一条消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

/// 一次流式生成的请求配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateOptions {
    pub provider: String,
    pub model: String,
    pub messages: Vec<LlmMessage>,
    pub tools: Vec<crate::tools::ToolSchema>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u64>,
    /// 会话 id（用于 provider 侧上下文关联与日志）。
    pub session_id: Option<String>,
    /// 请求级取消信号（对齐 DSH `options.signal`：abort → 终态
    /// finish{kind:'aborted', code:'ABORTED'}）。仅进程内传递，不上 wire/不落盘。
    #[serde(skip)]
    pub signal: Option<AbortSignal>,
    /// 请求级推理档位（'off'/'low'/'high'/'max'；None = 用 adapter 默认）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// 请求级 thinking 开关（'enabled'/'disabled'；None = adapter 默认）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// 请求用途（'session-title'/'compaction' 强制 thinking disabled；
    /// None = 普通会话调用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

/// LLM 提供者端口。
///
/// 实现必须：流中逐块产出增量；结束时产出 `Finish`；错误以 `Err` 形式
/// 结束流（torn 流不允许静默中断——调用方以 `Finish` 缺失判定 torn）。
#[async_trait::async_trait]
pub trait LlmPort: Send + Sync {
    /// 返回 provider 可用的模型清单。
    async fn list_models(&self, provider: &str) -> Result<Vec<LlmModelInfo>, LlmError>;

    /// 解析一个精确 provider/model 路由的全部已知元数据（对齐 DSH
    /// `LlmAdapter.resolveModel`：context / defaultMaxTokens / reasoning）。
    /// 默认实现从 `list_models` 目录查找；adapter 可覆写（如按配置声明）。
    async fn resolve_model(&self, provider: &str, model: &str) -> LlmResolvedModelInfo {
        let models = self.list_models(provider).await.unwrap_or_default();
        let found = models.iter().find(|m| m.id == model);
        LlmResolvedModelInfo {
            provider: provider.to_string(),
            id: model.to_string(),
            name: found
                .and_then(|m| m.label.clone())
                .unwrap_or_else(|| model.to_string()),
            context_window: found.and_then(|m| m.context_window),
            default_max_tokens: found.and_then(|m| m.max_tokens),
            reasoning: found.and_then(|m| m.reasoning.clone()),
            input_modalities: vec!["text".to_string()],
        }
    }

    /// 发起一次流式生成。
    fn stream(&self, request: GenerateOptions) -> ChunkStream;
}

/// 模型的可选推理档位元数据（对齐 DSH `ModelReasoning`：efforts + defaultEffort）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelReasoning {
    /// 支持的档位（adapter 偏好的显示顺序）。
    pub efforts: Vec<ReasoningEffort>,
    /// 默认档位（省略时保留 provider 自身默认）。
    pub default_effort: Option<String>,
}

/// 一个可选的推理档位（对齐 DSH `LlmReasoningEffortInfo`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningEffort {
    /// 稳定不透明值（GenerateOptions.reasoningEffort 可接受）。
    pub id: String,
    /// 选择器/诊断用可读名。
    pub name: String,
    pub description: Option<String>,
}

/// provider 模型信息（对齐 DSH `LlmModelInfo` + resolveModel 能力元数据）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmModelInfo {
    pub id: String,
    pub label: Option<String>,
    /// 是否支持工具调用。
    pub supports_tools: bool,
    /// provider 声明的上下文窗口（token）。
    pub context_window: Option<u64>,
    /// provider 声明的单请求输出上限（省略时 materialize 进请求）。
    pub max_tokens: Option<u64>,
    /// 可选的推理档位元数据。
    pub reasoning: Option<ModelReasoning>,
}

/// 精确 provider/model 路由的已解析元数据（对齐 DSH `LlmResolvedModelInfo`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmResolvedModelInfo {
    pub provider: String,
    pub id: String,
    pub name: String,
    /// provider 上下文容量。
    pub context_window: Option<u64>,
    /// adapter 配置的默认输出上限。
    pub default_max_tokens: Option<u64>,
    /// 可选推理档位。
    pub reasoning: Option<ModelReasoning>,
    /// 接受的请求模态（缺省 = 未知；显式负能力省略）。
    pub input_modalities: Vec<String>,
}

/// 从消息构造块序列的便捷函数：纯文本消息。
pub fn text_message(role: Role, text: impl Into<String>) -> LlmMessage {
    LlmMessage {
        role,
        content: vec![ContentBlock::Text(text.into())],
    }
}
