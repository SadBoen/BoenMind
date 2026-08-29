//! 运行时事件注册表镜像(registry/runtime-events.v0_1.json,20 类,封闭集合)。
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
            EventType::AgentCreated => &["agent_id", "session_id", "model_chain"],
            EventType::AgentStarted => &["agent_id"],
            EventType::AgentTurnStarted => &["agent_id", "operation_id", "turn_index"],
            EventType::AgentWaitingModel => &["agent_id", "operation_id", "model_id"],
            EventType::AgentCompleted => &["agent_id", "operation_id", "turn_index"],
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
