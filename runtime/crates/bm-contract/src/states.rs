//! 四台状态机镜像(state-machines/core-transitions.v0_1.json):
//! Operation / Session / Agent / Task。表外迁移一律非法(INV-2)。
//! 迁移的 guard 文本原样保留,供测试与审计对照。

wire_str_enum!(OperationState {
    NotStarted => "not_started",
    Running => "running",
    // M4 增发(2026-08-29,Minor:基线 §9.6 等待审批的 Operation;M4 规格 §4-8)
    WaitingApproval => "waiting_approval",
    Succeeded => "succeeded",
    Failed => "failed",
    Cancelled => "cancelled",
    Timeout => "timeout",
    OutcomeUnknown => "outcome_unknown",
    Interrupted => "interrupted",
});

wire_str_enum!(SessionState {
    Created => "created",
    Active => "active",
    Detached => "detached",
    Closed => "closed",
});

wire_str_enum!(AgentState {
    Created => "created",
    Starting => "starting",
    Running => "running",
    WaitingModel => "waiting_model",
    // M5 增发(2026-08-29,Minor:级联 task.pause/resume 与 agent.pause/resume)
    Paused => "paused",
    Stopping => "stopping",
    Stopped => "stopped",
    Failed => "failed",
    Cancelled => "cancelled",
    Interrupted => "interrupted",
    Resuming => "resuming",
});

// M5 增发(2026-08-29,Minor:task 状态机 7 态,基线 §2.2/§10.3,M5 规格 §5.1)
wire_str_enum!(TaskState {
    Created => "created",
    Running => "running",
    Paused => "paused",
    Blocked => "blocked",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
});

/// 一条合法迁移。guard 为迁移表中的前置条件表达式原文。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition<S> {
    pub from: S,
    pub to: S,
    pub guard: &'static str,
}

macro_rules! state_machine {
    ($states_ty:ident, $states_len:expr, $terminal:expr, $transitions:expr) => {
        impl $states_ty {
            /// 状态数(与迁移表 states 数组同步)。
            pub const ALL_LEN: usize = $states_len;

            /// 终态集合(迁移表 terminal 列;M1 内终态不可迁出)。
            pub fn is_terminal(self) -> bool {
                $terminal.contains(&self)
            }

            /// 合法迁移表(与 core-transitions.v0_1.json 逐条同步)。
            pub fn transitions() -> &'static [Transition<Self>] {
                &$transitions
            }

            /// from → to 是否为表中一条边。
            pub fn can_transition(from: Self, to: Self) -> bool {
                $transitions.iter().any(|t| t.from == from && t.to == to)
            }
        }
    };
}

pub const OPERATION_TERMINAL: [OperationState; 5] = [
    OperationState::Succeeded,
    OperationState::Failed,
    OperationState::Cancelled,
    OperationState::Timeout,
    OperationState::OutcomeUnknown,
];

pub const OPERATION_TRANSITIONS: [Transition<OperationState>; 15] = [
    t(
        OperationState::NotStarted,
        OperationState::Running,
        "dispatch_accepted",
    ),
    t(
        OperationState::NotStarted,
        OperationState::Cancelled,
        "explicit_cancel_before_dispatch",
    ),
    t(
        OperationState::Running,
        OperationState::Succeeded,
        "result_recorded",
    ),
    t(
        OperationState::Running,
        OperationState::Failed,
        "error_terminal_and_no_external_effect_possible",
    ),
    t(
        OperationState::Running,
        OperationState::Cancelled,
        "explicit_cancel_and_no_external_effect_possible",
    ),
    t(
        OperationState::Running,
        OperationState::Timeout,
        "deadline_exceeded AND effect_class IN [read-only, low-risk-command]",
    ),
    t(
        OperationState::Running,
        OperationState::OutcomeUnknown,
        "(deadline_exceeded OR crash OR cancel) AND effect_class IN [reversible-command, external-side-effect, high-risk-command]",
    ),
    t(
        OperationState::Running,
        OperationState::Interrupted,
        "runtime_crash_before_terminal",
    ),
    // M4 增发:审批等待三边(基线 §9.6;denied/expired 等价拒绝,无超时默认同意)
    t(
        OperationState::Running,
        OperationState::WaitingApproval,
        "approval_pending",
    ),
    t(
        OperationState::WaitingApproval,
        OperationState::Running,
        "approval_granted",
    ),
    t(
        OperationState::WaitingApproval,
        OperationState::Cancelled,
        "approval_denied_or_expired_or_withdrawn",
    ),
    t(
        OperationState::Interrupted,
        OperationState::Running,
        "recovery_replay_ok",
    ),
    t(
        OperationState::Interrupted,
        OperationState::Cancelled,
        "user_ruling",
    ),
    t(
        OperationState::OutcomeUnknown,
        OperationState::Succeeded,
        "external_verification OR user_ruling",
    ),
    t(
        OperationState::OutcomeUnknown,
        OperationState::Failed,
        "external_verification OR user_ruling",
    ),
];

pub const SESSION_TERMINAL: [SessionState; 1] = [SessionState::Closed];

pub const SESSION_TRANSITIONS: [Transition<SessionState>; 6] = [
    t(
        SessionState::Created,
        SessionState::Active,
        "surface_attached",
    ),
    t(
        SessionState::Active,
        SessionState::Detached,
        "surface_disconnected",
    ),
    t(
        SessionState::Detached,
        SessionState::Active,
        "session_resume",
    ),
    t(SessionState::Created, SessionState::Closed, "session_close"),
    t(SessionState::Active, SessionState::Closed, "session_close"),
    t(
        SessionState::Detached,
        SessionState::Closed,
        "session_close",
    ),
];

pub const AGENT_TERMINAL: [AgentState; 3] = [
    AgentState::Stopped,
    AgentState::Failed,
    AgentState::Cancelled,
];

pub const AGENT_TRANSITIONS: [Transition<AgentState>; 22] = [
    t(AgentState::Created, AgentState::Starting, "agent_start"),
    t(
        AgentState::Created,
        AgentState::Cancelled,
        "explicit_cancel",
    ),
    t(
        AgentState::Starting,
        AgentState::Running,
        "model_binding_ready",
    ),
    t(AgentState::Starting, AgentState::Failed, "init_error"),
    t(
        AgentState::Running,
        AgentState::WaitingModel,
        "model_invoke_issued",
    ),
    t(
        AgentState::WaitingModel,
        AgentState::Running,
        "model_response_ok",
    ),
    t(
        AgentState::WaitingModel,
        AgentState::Failed,
        "model_chain_exhausted OR budget_exceeded",
    ),
    t(
        AgentState::WaitingModel,
        AgentState::Stopping,
        "explicit_cancel",
    ),
    t(AgentState::Running, AgentState::Stopping, "explicit_cancel"),
    // M5 增发:paused 四边(用户 task.pause/resume 级联或 Coordinator 显式动词;
    // resume 为编排重启触发者之一,ADR-0004 条件 6)
    t(
        AgentState::Running,
        AgentState::Paused,
        "task_paused OR agent_paused",
    ),
    t(
        AgentState::Paused,
        AgentState::Running,
        "task_resumed OR agent_resumed",
    ),
    t(AgentState::Paused, AgentState::Stopping, "explicit_cancel"),
    t(AgentState::Paused, AgentState::Cancelled, "explicit_cancel"),
    t(
        AgentState::Running,
        AgentState::Failed,
        "unrecoverable_error",
    ),
    t(
        AgentState::Stopping,
        AgentState::Stopped,
        "turn_boundary_reached",
    ),
    t(
        AgentState::Stopping,
        AgentState::Interrupted,
        "runtime_crash_during_stop",
    ),
    t(
        AgentState::Starting,
        AgentState::Interrupted,
        "runtime_crash",
    ),
    t(
        AgentState::Running,
        AgentState::Interrupted,
        "runtime_crash",
    ),
    t(
        AgentState::WaitingModel,
        AgentState::Interrupted,
        "runtime_crash",
    ),
    t(
        AgentState::Interrupted,
        AgentState::Resuming,
        "restart_recovery",
    ),
    t(AgentState::Resuming, AgentState::Running, "replay_ok"),
    t(
        AgentState::Resuming,
        AgentState::Stopped,
        "replay_ok AND turn_was_stopping",
    ),
];

pub const TASK_TERMINAL: [TaskState; 3] = [
    TaskState::Completed,
    TaskState::Failed,
    TaskState::Cancelled,
];

// M5 增发:task 状态机 12 边(M5 规格 §5.1;completed 必须 verified_completion
// ——完成判定门禁;blocked 仅 user_resolved 可回 running,ADR-0004 条件 6)
pub const TASK_TRANSITIONS: [Transition<TaskState>; 12] = [
    t(TaskState::Created, TaskState::Running, "task_started"),
    t(TaskState::Running, TaskState::Paused, "task_paused"),
    t(TaskState::Paused, TaskState::Running, "task_resumed"),
    t(
        TaskState::Running,
        TaskState::Blocked,
        "budget_exhausted OR stall_hard_limit OR outcome_unknown_pending",
    ),
    t(TaskState::Blocked, TaskState::Running, "user_resolved"),
    t(
        TaskState::Running,
        TaskState::Completed,
        "verified_completion",
    ),
    t(
        TaskState::Paused,
        TaskState::Completed,
        "verified_completion",
    ),
    t(TaskState::Running, TaskState::Failed, "verified_failure"),
    t(TaskState::Paused, TaskState::Failed, "verified_failure"),
    t(TaskState::Running, TaskState::Cancelled, "task_cancelled"),
    t(TaskState::Paused, TaskState::Cancelled, "task_cancelled"),
    t(TaskState::Blocked, TaskState::Cancelled, "task_cancelled"),
];

const fn t<S>(from: S, to: S, guard: &'static str) -> Transition<S> {
    Transition { from, to, guard }
}

state_machine!(OperationState, 9, OPERATION_TERMINAL, OPERATION_TRANSITIONS);
state_machine!(SessionState, 4, SESSION_TERMINAL, SESSION_TRANSITIONS);
state_machine!(AgentState, 11, AGENT_TERMINAL, AGENT_TRANSITIONS);
state_machine!(TaskState, 7, TASK_TERMINAL, TASK_TRANSITIONS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_no_transition_out_of_terminal() {
        for terminal in OPERATION_TERMINAL {
            for state in [
                OperationState::NotStarted,
                OperationState::Running,
                OperationState::Interrupted,
            ] {
                assert!(
                    !OperationState::can_transition(terminal, state),
                    "{terminal:?} 不可迁出"
                );
            }
        }
    }
}
