//! 会话事件词汇与事件信封。
//!
//! 存储模型（v2.1 拍板）：append-only 事件日志 = 唯一事实源，
//! sessions/messages/tool_calls 均为其投影。本模块定义事件词汇最小集
//! （进程内事件层，M1 内部自由）；M2 对齐 wire 层时在序列化层映射到
//! dsh 的 46 种 SessionEvent 逐字形状。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::llm::{ContentBlock, Role, ToolCall, ToolCallResult};

/// 会话唯一标识。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SessionId {
    fn from(v: String) -> Self {
        SessionId(v)
    }
}

impl From<&str> for SessionId {
    fn from(v: &str) -> Self {
        SessionId(v.to_string())
    }
}

/// 会话头：会话的静态元信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionHeader {
    pub id: SessionId,
    /// 所属应用/场景（home / coding / wiki…）。
    pub app: String,
    /// 启动 profile（headless / web / test…）。
    pub profile: String,
    /// 工作区路径（可空）。
    pub workspace: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 回合级事件负载。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TurnEvent {
    Started { turn: u64 },
    Ended { turn: u64 },
}

/// 步骤级事件负载。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepEvent {
    pub turn: u64,
    pub step: u64,
}

/// 会话事件（append-only 日志的最小词汇集）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionEvent {
    /// 会话创建。
    SessionStarted { header: SessionHeader },
    /// 用户消息进入。
    UserMessage { text: String },
    /// 回合开始/结束。
    Turn(TurnEvent),
    /// 步骤开始/结束（一次模型调用 + 工具执行为一个 step）。
    Step { turn: u64, step: u64, phase: StepPhase },
    /// 模型流式文本增量（assistant/chunk 语义：model-visible-means-logged）。
    AssistantChunk { text: String },
    /// 模型完整消息（含工具调用块）。
    AssistantMessage { content: Vec<ContentBlock> },
    /// 工具调用（模型侧发出）。
    ToolCall { call: ToolCall },
    /// 工具执行结果。
    ToolResult { result: ToolCallResult },
    /// 会话结束。
    SessionEnded { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepPhase {
    Started,
    Ended,
}

/// 带单调序号的事件信封（seq 是日志内排序键，也是崩溃恢复的对账锚点）。
/// session_id 是归属会话（进程内总线不按会话过滤，监听方据此路由）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    pub session_id: SessionId,
    pub event: SessionEvent,
}

impl SessionRecord {
    pub fn new(seq: u64, session_id: impl Into<SessionId>, event: SessionEvent) -> Self {
        Self {
            seq,
            timestamp: Utc::now(),
            session_id: session_id.into(),
            event,
        }
    }
}

/// 便捷构造：把一条消息会话内容转成给模型的 LlmMessage。
pub fn user_message_block(text: &str) -> crate::llm::LlmMessage {
    crate::llm::LlmMessage {
        role: Role::User,
        content: vec![ContentBlock::Text(text.to_string())],
    }
}
