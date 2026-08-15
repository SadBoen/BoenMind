//! BoenMind 契约层（bm-protocol）。
//!
//! 内核 / 插件 / 存储三方共享的纯类型定义。**铁律：零运行时依赖**
//! （无 tokio/turso/axum/redb），仅 serde + serde_json——契约 crate 的
//! 纯净性是内核"最小"的物理锁（实现方案 §5-1，Life Agent OS 验证过的姿势）。
//!
//! 分域：
//! - [`ids`]：typed 标识符（SessionId/BranchId/SeqNo/CallId）
//! - [`event`]：核心域事件（强类型 enum）+ 会话事件信封
//! - [`surface`]：消息面操作（Append/Replace，压缩遮蔽用）
//! - [`port`]：Port traits（内核依赖 Port 而非实现）
//! - [`policy`]：能力模式串与策略评估
//! - [`error`]：类型化错误码（能力矩阵/未知事件/seq 类）

pub mod error;
pub mod event;
pub mod ids;
pub mod policy;
pub mod port;
pub mod surface;

pub use error::{ErrorCode, ProtocolError};
pub use event::{
    AssistantMsg, CompactionSummaryMsg, CoreEvent, CustomEvent, EpochHeader, EventKind,
    FormatMigration, HeaderReason, SessionEvent, StreamChunk, TodoItem, TokenUsage,
    ToolResultMsg, TurnEndReason, UserMsg, UserMsgSource, core_type_name, FORMAT_MIGRATIONS,
    SESSION_FORMAT_VERSION,
};
pub use ids::{BranchId, CallId, GlobalSeq, SeqNo, SessionId};
pub use policy::{Capability, PolicyEvaluation};
pub use port::{
    BoxFuture, BranchHead, CredentialsPort, EventQuery, EventStorePort, LlmPort, MemoryPort,
    NotifyPort, SchedulerPort, SessionPort, SessionUsage, SettingsPort, SkillPort, StatsPort,
    ToolsPort,
};
pub use surface::{SurfaceOp, SurfaceOutcome};
