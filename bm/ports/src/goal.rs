//! 目标管理契约（产品契约层扩展）。
//!
//! BoenMind 对齐 DSH `dsh-goal` + `dsh-tool-goal`：一个会话至多一个当前目标
//! （objective）。万物皆插件②（2026-08-22）：目标状态机（goals map）、
//! CAS 语义与续跑驱动（goal-round-driver）整体下沉 plugin-goal；
//! [`GoalEnginePort`] 是完整领域面——工具消费面（get/create/update）与
//! wire 面（CAS edit/phase 直置/clear）共用一个引擎，宿主经端口委托。
//! 激活（activation）是进程级状态，不持久化——`GoalView.activation`
//! 是活观测，永远不是 replay 权威。
//!
//! 依赖纪律：本 crate 只依赖 kernel-contracts（纯契约）。

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

/// 目标引擎错误（类型化）：工具面映射为 ToolError 文案，wire 面由宿主映射
/// 回各自逐字错误码（goal-not-found / goal-conflict / bad-request …）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalError {
    /// 会话无目标。
    NotFound,
    /// ref（id/revision）不匹配当前目标（CAS 失败）。
    Conflict,
    /// objective 为空（≥1 字符约束）。
    EmptyObjective,
    /// maxGoalRounds 非正整数。
    InvalidMaxRounds,
    /// resume 时额度耗尽（rounds >= max；工具面语义）。
    ResumeCapExhausted,
}

/// 目标引擎端口（完整领域面；全部同步——内部无 IO，锁即界）。
/// - 工具面：`goal_get/goal_create/goal_update`（含 resume 额度检查的工具语义）。
/// - wire 面：`goal_cas_edit/goal_cas_phase/goal_clear`（CAS 守卫；phase 直置
///   无额度检查——与既有 RPC 语义逐字一致）。
/// - 续跑：`maybe_continue`（回合完成点；driver 未启用恒 false）。
pub trait GoalEnginePort: Send + Sync + std::fmt::Debug {
    /// 解析目标会话：空 → 当前活跃会话；非空 → 存在则该会话，否则 None。
    fn resolve_session(&self, session_id: &str) -> Option<String>;

    /// 读当前目标（无 → None）。
    fn goal_get(&self, session_id: &str) -> Result<Option<GoalView>, GoalError>;

    /// 创建目标（objective 非空；max_goal_rounds 缺省由调用方决定：
    /// 工具面 8、wire 面 1——既有差异显式化）。创建即替换旧目标。
    fn goal_create(
        &self,
        session_id: &str,
        objective: &str,
        max_goal_rounds: Option<u64>,
    ) -> Result<GoalView, GoalError>;

    /// 更新目标（工具面语义：CAS + GoalAction 转换 + resume 额度检查）。
    #[allow(clippy::too_many_arguments)] // update 承载 6 个语义参数（官方 update_goal 同款）
    fn goal_update(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
        action: GoalAction,
        objective: Option<&str>,
        max_goal_rounds: Option<u64>,
        blocked_reason: Option<&str>,
    ) -> Result<GoalView, GoalError>;

    /// wire 面 edit（CAS + 字段校验；无「至少一个字段」检查——留在宿主）。
    /// 返回新 revision。
    fn goal_cas_edit(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
        objective: Option<&str>,
        max_goal_rounds: Option<u64>,
    ) -> Result<u64, GoalError>;

    /// wire 面相位直置（pause/resume/complete；无额度检查）。返回新 revision。
    fn goal_cas_phase(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
        to_phase: &str,
    ) -> Result<u64, GoalError>;

    /// 清除目标（留墓碑：投影置 null 的广播由引擎承担）。
    fn goal_clear(&self, session_id: &str, goal_id: &str, revision: u64) -> Result<(), GoalError>;

    /// 回合完成点续跑判定：active + 有额度 → 注入 `<goal_round>` 续跑一轮。
    /// driver 未启用（--goal 未开）恒 false。
    fn maybe_continue(&self, session_id: &str) -> bool;

    /// 启用/停用续跑驱动（装配方调用；引擎本体随宿主常驻）。
    fn set_driver_enabled(&self, enabled: bool);
}