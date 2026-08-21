//! # bm-ports —— BoenMind 产品级契约层
//!
//! 内核契约（`kernel-contracts`）承载纯内核端口（LlmPort / FsPort / AuthPort /
//! SessionPersistPort…）；**产品级策略端口**（核心插件需要、但内核不提供的正交
//! 能力）放本层——如上下文压缩 `Compactor`。
//!
//! 由来（2026-08-20 回头看）：`Compactor` 原定义在功能插件 plugin-compactor，
//! 而核心插件 plugin-loop 编译期依赖该 trait →「核心依赖功能插件」依赖倒置
//! 硬伤。修复：trait（含默认事务实现）上提到产品契约层，功能插件只留策略
//! 实现（`DefaultCompactor`），loop 只依赖本层端口。`kernel/` submodule 只读，
//! 不进内核；本层是 BoenMind 侧的内核契约扩展面。
//!
//! 依赖纪律：本 crate 只依赖 `kernel-contracts`（纯契约），不依赖任何插件 /
//! 组合根——所有上层（plugin-loop / plugin-compactor / bm-assembly）向本层
//! 输入依赖均合法、向插件层输出依赖均违规。

pub mod compactor;

pub use compactor::{
    build_dialogue, estimate_tokens, Compactor, DEFAULT_CONTEXT_WINDOW,
};

pub mod tools;

pub use tools::{ToolGatePort, ToolRegistryPort};

pub mod host;

pub use host::WorkdirPort;

/// 工具审批契约（loop 消费面）：危险工具调用执行前暂停、推审批弹窗、等用户裁定。
/// 可选装配：未装配 = 审批面禁用（既有自动执行语义不变）。
pub mod approval;

pub use approval::{ApprovalVerdict, ToolApprovalPort, APPROVAL_TIMEOUT};

/// 定时任务契约（工具消费面）：创建/列出/取消周期任务，驱动目标会话回合。
/// 调度器实现（web-server）注入；未装配 = schedule 工具不可用。
pub mod schedule;

pub use schedule::{SchedulePort, ScheduleSpec, ScheduleTrigger, ScheduleView};

/// 目标管理契约（工具消费面）：get/create/update 三动词，web-server 现有
/// goal RPC 状态机语义；goal-round-driver 同会话续跑在回合完成点。
pub mod goal;

pub use goal::{GoalAction, GoalPort, GoalView};