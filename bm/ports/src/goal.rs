//! 目标管理契约（产品契约层扩展）。
//!
//! BoenMind 对齐 DSH `dsh-goal` + `dsh-tool-goal`：一个会话至多一个当前目标
//! （objective），持久化状态在 web-server（`GoalRecord` 内存态 + projection 广播），
//! 循环驱动（goal-round-driver 同会话续跑）在 web-server 回合完成点。
//!
//! 本层契约只留**工具消费面**（model-facing）：get/create/update 三动词，
//! 经 web-server 现有 goal RPC 状态机的语义实现。激活（activation）是进程级
//! 状态，不持久化——`GoalView.activation` 是活观测，永远不是 replay 权威。
//!
//! 依赖纪律：本 crate 只依赖 kernel-contracts（纯契约）。

use kernel_contracts::ToolError;

/// 目标操作（update 动词）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalAction {
    Edit,
    Pause,
    Resume,
    Complete,
    Blocked,
}

/// 目标视图（get 返回；紧凑 JSON 同款形状）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalView {
    pub id: String,
    pub revision: u64,
    pub objective: String,
    /// durable phase：active | paused | blocked | complete。
    pub phase: String,
    pub rounds_started: u64,
    pub max_goal_rounds: u64,
    pub blocked_reason: Option<String>,
    /// 进程级激活（活观测，不持久化）。
    pub activation: bool,
}

/// 目标端口（模型侧工具消费面）：get/create/update。
#[async_trait::async_trait]
#[allow(clippy::too_many_arguments)] // update 承载 6 个语义参数（官方 update_goal 同款）
pub trait GoalPort: Send + Sync {
    /// 读当前目标（无 → None）。
    async fn goal_get(&self, session_id: &str) -> Result<Option<GoalView>, ToolError>;
    /// 创建目标（objective 非空；max_goal_rounds 缺省 = 内部默认）。
    async fn goal_create(
        &self,
        session_id: &str,
        objective: &str,
        max_goal_rounds: Option<u64>,
    ) -> Result<GoalView, ToolError>;
    /// 更新目标（CAS revision 守卫）。返回新视图。
    async fn goal_update(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
        action: GoalAction,
        objective: Option<&str>,
        max_goal_rounds: Option<u64>,
        blocked_reason: Option<&str>,
    ) -> Result<GoalView, ToolError>;
}