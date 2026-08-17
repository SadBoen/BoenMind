//! LLM 端口：provider trait 与流式类型。
//!
//! 端口形状借鉴 bobleer `LlmPort`：`stream()` 返回异步流，逐块产出
//! `StreamChunk`，结束由 `Finish` 块标记。M1 只接 mock 实现，
//! 真实 provider（OpenAI 兼容 SSE）在 M2+ 按同一端口接入。

use std::pin::Pin;

use futures::Stream;
use serde::{Deserialize, Serialize};

use crate::error::LlmError;

/// 流式块流。
pub type ChunkStream = Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send>>;

/// 模型结束原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    MaxTokens,
    /// 工具调用序列结束（模型请求执行工具）。
    ToolCalls,
    /// 错误中断。
    Error,
    /// 被取消。
    Cancelled,
}

/// token 用量。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
}

/// 流式增量块（对齐 dsh harness 的 chunk 语义：文本/推理/工具调用逐块增量）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamChunk {
    /// 文本增量。
    TextDelta { text: String },
    /// 推理内容增量（reasoning_content）。
    ReasoningDelta { text: String },
    /// 工具调用增量：arguments 是逐块拼贴的字符串。
    ToolCallDelta {
        index: usize,
        name: String,
        arguments_delta: String,
    },
    /// 工具调用完成（该 index 的 arguments 拼贴完整）。
    ToolCallDone {
        index: usize,
        name: String,
        arguments: String,
    },
    /// token 用量（流中可出现多次，后到覆盖先到）。
    Usage(TokenUsage),
    /// 流结束。
    Finish(FinishReason),
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
    /// JSON 序列化的参数（透传给工具执行）。
    pub arguments: serde_json::Value,
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
}

/// LLM 提供者端口。
///
/// 实现必须：流中逐块产出增量；结束时产出 `Finish`；错误以 `Err` 形式
/// 结束流（torn 流不允许静默中断——调用方以 `Finish` 缺失判定 torn）。
#[async_trait::async_trait]
pub trait LlmPort: Send + Sync {
    /// 返回 provider 可用的模型清单。
    async fn list_models(&self, provider: &str) -> Result<Vec<LlmModelInfo>, LlmError>;

    /// 发起一次流式生成。
    fn stream(&self, request: GenerateOptions) -> ChunkStream;
}

/// provider 模型信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmModelInfo {
    pub id: String,
    pub label: Option<String>,
    /// 是否支持工具调用。
    pub supports_tools: bool,
}

/// 从消息构造块序列的便捷函数：纯文本消息。
pub fn text_message(role: Role, text: impl Into<String>) -> LlmMessage {
    LlmMessage {
        role,
        content: vec![ContentBlock::Text(text.into())],
    }
}
