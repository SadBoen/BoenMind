//! 定时任务契约（产品契约层扩展）。
//!
//! schedule 插件（plugin-schedule）的工具消费面：创建/列出/取消定时任务。
//! 调度器实现（web-server Scheduler）持有 tokio runtime + sessions 访问权，
//! 后台 spawn 驱动目标会话回合（复用 session.prompt 的 run_turn 语义）。
//!
//! 契约只留**消费面**（工具侧）：create/list/cancel 三个方法；装配面
//! （后台循环、到期判定、会话驱动）由 web-server 具体实现承载，经
//! [`SchedulePort`] 注入——与 WorkdirPort/Compactor 同构。
//!
//! 依赖纪律：本 crate 只依赖 kernel-contracts（纯契约）。

use kernel_contracts::ToolError;

/// 一个定时任务描述（create 入参 → 登记项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleSpec {
    /// 触发方式：interval（秒级间隔，循环）或 cron（5 段 cron 表达式）。
    pub trigger: ScheduleTrigger,
    /// 到期时发给目标会话的提示文本（user message）。
    pub prompt: String,
    /// 目标会话 id（为空 = 由调度器取当前活跃会话，单会话场景直接驱动）。
    pub session_id: Option<String>,
}

/// 触发方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleTrigger {
    /// 固定间隔（秒，≥1），循环执行直到 cancel。
    Interval { secs: u64 },
    /// cron 表达式（5 段：分 时 日 月 周）。简化实现：仅支持固定分钟/小时级
    /// 间隔匹配（cron 全语法解析属另一工程，见 HANDOFF 待办）。
    Cron { expr: String },
}

/// 定时任务查询视图（list 返回）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleView {
    pub id: String,
    pub trigger: String,
    pub prompt: String,
    pub session_id: Option<String>,
    /// 下次触发时间（unix 毫秒；None = 已失效）。
    pub next_at_ms: Option<i64>,
}

/// 定时任务端口（工具消费面）：create/list/cancel 三操作。
#[async_trait::async_trait]
pub trait SchedulePort: Send + Sync {
    /// 创建定时任务，返回任务 id。
    async fn schedule_create(&self, spec: ScheduleSpec) -> Result<String, ToolError>;
    /// 列出全部活动任务。
    async fn schedule_list(&self) -> Result<Vec<ScheduleView>, ToolError>;
    /// 取消任务（id 不存在 → Err）。
    async fn schedule_cancel(&self, id: &str) -> Result<(), ToolError>;
}