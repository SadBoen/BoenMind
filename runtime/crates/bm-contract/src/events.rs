//! 运行时事件注册表镜像(registry/runtime-events.v0_1.json,43 类,封闭集合:
//! M1 20 + M2 增发 2 + M4 增发 10 + M5 增发 8 + M6 增发 1 + M7 增发 2)。
//!
//! 注册表是「允许发射」的封闭集,不是「必须发射」集;哪些流程发射哪些事件
//! 以黄金轨迹与迁移表为准(规格 §8.6)。`payload_keys` 是注册表声明的 payload
//! 字段清单,同步测试保证与 JSON 一致,发射侧用它做完备性断言。

use crate::BmTimestamp;
use crate::ids::BmId;
use serde::{Deserialize, Serialize};

wire_str_enum!(EventType {
    RuntimeStarted => "runtime.started",
    RuntimeStopping => "runtime.stopping",
    RuntimeStopped => "runtime.stopped",
    SessionCreated => "session.created",
    SessionResumed => "session.resumed",
    SessionClosed => "session.closed",
    AgentCreated => "agent.created",
    AgentStarted => "agent.started",
    AgentTurnStarted => "agent.turn.started",
    AgentWaitingModel => "agent.waiting_model",
    AgentCompleted => "agent.completed",
    AgentFailed => "agent.failed",
    AgentCancelled => "agent.cancelled",
    AgentInterrupted => "agent.interrupted",
    AgentResumed => "agent.resumed",
    OperationStateChanged => "operation.state.changed",
    ModelInvocationCompleted => "model.invocation.completed",
    ModelInvocationFailed => "model.invocation.failed",
    BudgetWarning => "budget.warning",
    BudgetExceeded => "budget.exceeded",
    // M2 增发(2026-08-29,Minor:纯追加,规格 §5.5)
    RuntimeRecovered => "runtime.recovered",
    StoreWriteRejected => "store.write.rejected",
    // M4 增发(2026-08-29,Minor:纯追加,M4 规格 §4-7)
    ApprovalRequested => "approval.requested",
    ApprovalResolved => "approval.resolved",
    ApprovalExpired => "approval.expired",
    GrantCreated => "grant.created",
    GrantRevoked => "grant.revoked",
    CapabilityInvoked => "capability.invoked",
    CapabilityDenied => "capability.denied",
    ProviderBindingChanged => "provider.binding.changed",
    BusDegraded => "bus.degraded",
    BusResumed => "bus.resumed",
    // M5 增发(2026-08-29,Minor:纯追加,M5 规格 §4-2;发射侧随 T1–T8)
    TaskCreated => "task.created",
    TaskStateChanged => "task.state.changed",
    TaskMemberAdded => "task.member.added",
    TaskBudgetIncreased => "task.budget.increased",
    TaskStalled => "task.stalled",
    TaskRepeating => "task.repeating",
    WatchdogReorchestrationTriggered => "watchdog.reorchestration.triggered",
    ObservationRecorded => "observation.recorded",
    // M6 增发(2026-08-30,Minor:纯追加,M6 规格 §4-2)
    TaskMemberRemoved => "task.member.removed",
    // M7 增发(2026-08-30,Minor:纯追加,M7 规格 §2-S4/S5)
    CapabilityProgress => "capability.progress",
    ProviderHealthChanged => "provider.health.changed",
});

impl EventType {
    /// 注册表声明的 payload 必备键(与 runtime-events.v0_1.json 逐条同步)。
    pub fn payload_keys(self) -> &'static [&'static str] {
        match self {
            EventType::RuntimeStarted => &["pid", "version", "started_at"],
            EventType::RuntimeStopping => &["reason"],
            EventType::RuntimeStopped => &["uptime_ms"],
            EventType::SessionCreated => &["session_id", "agent_id"],
            EventType::SessionResumed => &["session_id", "since_seq", "replayed"],
            EventType::SessionClosed => &["session_id", "reason"],
            EventType::AgentCreated => &["agent_id", "session_id", "model_chain", "budget"],
            EventType::AgentStarted => &["agent_id"],
            EventType::AgentTurnStarted => &["agent_id", "operation_id", "turn_index"],
            EventType::AgentWaitingModel => &["agent_id", "operation_id", "model_id"],
            EventType::AgentCompleted => &["agent_id", "operation_id", "turn_index", "content"],
            EventType::AgentFailed => &["agent_id", "operation_id", "error_code"],
            EventType::AgentCancelled => &["agent_id", "operation_id"],
            EventType::AgentInterrupted => &["agent_id", "operation_id", "reason"],
            EventType::AgentResumed => &["agent_id", "operation_id"],
            EventType::OperationStateChanged => &["operation_id", "from", "to", "reason_code"],
            EventType::ModelInvocationCompleted => &[
                "operation_id",
                "agent_id",
                "model_id",
                "attempt",
                "usage_in",
                "usage_out",
                "latency_ms",
                "stream_interrupted",
            ],
            EventType::ModelInvocationFailed => &[
                "operation_id",
                "agent_id",
                "model_id",
                "attempt",
                "error_code",
            ],
            EventType::BudgetWarning => {
                &["agent_id", "scope", "used_tokens", "limit_tokens", "ratio"]
            }
            EventType::BudgetExceeded => &["agent_id", "scope", "used_tokens", "limit_tokens"],
            EventType::RuntimeRecovered => {
                &["last_applied_seq", "replayed", "interrupted_recovered"]
            }
            EventType::StoreWriteRejected => &["key", "reason"],
            EventType::ApprovalRequested => &[
                "approval_id",
                "operation_id",
                "capability",
                "principal",
                "risk_class",
                "effective_risk",
                "input_trust",
                "expires_at",
            ],
            EventType::ApprovalResolved => &[
                "approval_id",
                "operation_id",
                "outcome",
                "scope",
                "grant_id",
            ],
            EventType::ApprovalExpired => &["approval_id", "operation_id", "expired_at"],
            EventType::GrantCreated => &[
                "grant_id",
                "approval_id",
                "audience",
                "action",
                "scope",
                "delegation_depth",
                "expires_at",
                "parent_hash",
            ],
            EventType::GrantRevoked => &["grant_id", "revocation_version", "reason"],
            EventType::CapabilityInvoked => &[
                "call_id",
                "operation_id",
                "capability",
                "principal",
                "binding_epoch",
                "provider_instance_id",
                "outcome",
                "error_code",
                "idempotency_key_hash",
            ],
            EventType::CapabilityDenied => &[
                "call_id",
                "capability",
                "principal",
                "input_trust",
                "reason_code",
            ],
            EventType::ProviderBindingChanged => &[
                "capability",
                "provider_instance_id",
                "old_epoch",
                "new_epoch",
                "reason",
            ],
            EventType::BusDegraded => &["reason", "component"],
            EventType::BusResumed => &["component", "degraded_ms"],
            EventType::TaskCreated => &["task_id", "title", "created_by", "parent_task_id"],
            EventType::TaskStateChanged => &["task_id", "from", "to", "reason_code", "task_epoch"],
            EventType::TaskMemberAdded => &["task_id", "agent_id", "role", "grant_id"],
            EventType::TaskBudgetIncreased => {
                &["task_id", "key", "old_limit", "new_limit", "approval_id"]
            }
            EventType::TaskStalled => &["task_id", "stalled_ms", "last_progress_seq"],
            EventType::TaskRepeating => &["task_id", "agent_id", "capability", "repeat_count"],
            EventType::WatchdogReorchestrationTriggered => &["task_id", "trigger", "reason"],
            EventType::TaskMemberRemoved => &["task_id", "agent_id", "reason"],
            EventType::CapabilityProgress => &[
                "call_id",
                "operation_id",
                "capability",
                "progress",
                "total",
                "message",
            ],
            EventType::ProviderHealthChanged => &["provider", "from", "to", "reason"],
            EventType::ObservationRecorded => &["task_id", "log_seq", "verdict", "guard_state"],
        }
    }
}

/// 事件信封(wire/envelope.v0_1.schema.json #/event_envelope)。
/// correlation 字段按发生对象携带;payload 键集由注册表约束。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_seq: u64,
    #[serde(rename = "type")]
    pub event_type: EventType,
    pub occurred_at: BmTimestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<BmId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<BmId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<BmId>,
    pub payload: serde_json::Value,
}

impl EventEnvelope {
    /// 构造并断言 payload 键集与注册表一致(缺键 = bug)。
    pub fn new(
        event_seq: u64,
        event_type: EventType,
        occurred_at: BmTimestamp,
        session_id: Option<BmId>,
        agent_id: Option<BmId>,
        operation_id: Option<BmId>,
        payload: serde_json::Value,
    ) -> Self {
        debug_assert!(payload.is_object(), "payload 必须是对象: {event_type}");
        let keys: Vec<&str> = payload
            .as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let mut expected: Vec<&str> = event_type.payload_keys().to_vec();
        let mut actual = keys;
        expected.sort_unstable();
        actual.sort_unstable();
        debug_assert_eq!(
            expected, actual,
            "事件 {event_type} 的 payload 键集偏离注册表"
        );
        Self {
            event_seq,
            event_type,
            occurred_at,
            session_id,
            agent_id,
            operation_id,
            payload,
        }
    }

    /// 免注册表断言的构造:仅供总线/回放等传输层测试使用,
    /// 发射侧一律走 [`EventEnvelope::new`]。
    #[doc(hidden)]
    pub fn new_unchecked(
        event_seq: u64,
        event_type: EventType,
        occurred_at: BmTimestamp,
        session_id: Option<BmId>,
        agent_id: Option<BmId>,
        operation_id: Option<BmId>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            event_seq,
            event_type,
            occurred_at,
            session_id,
            agent_id,
            operation_id,
            payload,
        }
    }
}
