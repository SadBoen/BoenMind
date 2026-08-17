//! # kernel-contracts
//!
//! BoenMind 微内核契约层：跨层共享的端口 trait、事件词汇与 DTO。
//! 本 crate 不包含任何业务实现，只定义形状（bobleer 同款分层理念）。
//!
//! 分层纪律：所有上层 crate 只允许依赖本 crate（依赖只许向下）。

pub mod bus;
pub mod error;
pub mod llm;
pub mod ports;
pub mod session;
pub mod tools;

pub use bus::{Disposer, EventBus, EventListener};
pub use error::{LlmError, PortError, PortErrorKind, PortResult, ToolError};
pub use llm::{
    block_to_wire, text_message, ChunkStream, ContentBlock, FinishReason, GenerateOptions,
    LlmMessage, LlmModelInfo, LlmPort, LlmResolvedModelInfo, ModelReasoning, ReasoningEffort, Role,
    StreamChunk, TokenUsage, ToolCall, ToolCallResult,
};
pub use ports::{
    FsPort, PluginRuntimeAvailability, PluginRuntimePort, SessionPersistPort, ShellPort,
    ShellRequest, ShellResult, UnavailablePluginRuntime,
};
pub use session::{
    SessionEvent, SessionHeader, SessionId, SessionRecord, StepPhase, TurnEndReason, TurnEvent,
};
pub use tools::{ExecutionMode, ToolExecutionInput, ToolExecutionResult, ToolHandler, ToolSchema};
