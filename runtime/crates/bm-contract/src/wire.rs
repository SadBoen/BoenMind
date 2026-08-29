//! Wire API 信封与方法参数/结果镜像
//! (wire/envelope + wire/session + wire/agent + wire/capability,v0.1)。
//!
//! M1 方法集合(7 个):session.create / session.resume / session.close /
//! events.poll / agent.send_input / agent.cancel / operations.get。
//! M4 增发(3 个):capability.call / approval.list / approval.respond
//! (服务端实现随 M4-T5)。

use crate::BmTimestamp;
use crate::budget::Budget;
use crate::error_codes::WireErrorCode;
use crate::events::EventEnvelope;
use crate::ids::BmId;
use crate::states::{AgentState, OperationState, SessionState};
use serde::{Deserialize, Serialize};

pub const WIRE_VERSION: &str = "0.1";

wire_str_enum!(Method {
    SessionCreate => "session.create",
    SessionResume => "session.resume",
    SessionClose => "session.close",
    EventsPoll => "events.poll",
    AgentSendInput => "agent.send_input",
    AgentCancel => "agent.cancel",
    OperationsGet => "operations.get",
    // M4 增发(2026-08-29,Minor:envelope method 枚举同步;params/result 见
    // wire/capability.v0_1;服务端行为 M4-T5 实现,当前 unavailable)
    CapabilityCall => "capability.call",
    ApprovalList => "approval.list",
    ApprovalRespond => "approval.respond",
});

wire_str_enum!(InputTrust {
    // M1 只接受用户直接输入;untrusted 分级门控随 Capability 引入(基线 4.5)。
    Trusted => "trusted",
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub v: String,
    pub method: Method,
    pub request_id: BmId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    pub params: serde_json::Value,
}

impl RequestEnvelope {
    pub fn new(method: Method, request_id: BmId, params: serde_json::Value) -> Self {
        Self {
            v: WIRE_VERSION.to_string(),
            method,
            request_id,
            idempotency_key: None,
            params,
        }
    }
}

/// 统一错误信封(envelope #/definitions/error)。message 脱敏,禁止凭据与隐私原文。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireError {
    pub code: WireErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default)]
    pub retry_after_ms: Option<u64>,
    #[serde(default)]
    pub detail_ref: Option<BmId>,
}

impl WireError {
    pub fn new(code: crate::error_codes::ErrorCode, message: impl Into<String>) -> Self {
        let retryable = code.default_retryable();
        Self {
            code: WireErrorCode::new(code).expect("M1 错误码"),
            message: message.into(),
            retryable,
            retry_after_ms: None,
            detail_ref: None,
        }
    }
}

/// 响应信封:ok=true 带 result / ok=false 带 error(envelope #/response oneOf)。
#[derive(Debug, Clone, PartialEq)]
pub enum ResponseEnvelope {
    Success {
        request_id: BmId,
        result: serde_json::Value,
    },
    Failure {
        request_id: BmId,
        error: WireError,
    },
}

impl Serialize for ResponseEnvelope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        match self {
            ResponseEnvelope::Success { request_id, result } => {
                map.serialize_entry("v", WIRE_VERSION)?;
                map.serialize_entry("request_id", request_id)?;
                map.serialize_entry("ok", &true)?;
                map.serialize_entry("result", result)?;
            }
            ResponseEnvelope::Failure { request_id, error } => {
                map.serialize_entry("v", WIRE_VERSION)?;
                map.serialize_entry("request_id", request_id)?;
                map.serialize_entry("ok", &false)?;
                map.serialize_entry("error", error)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ResponseEnvelope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            v: String,
            request_id: BmId,
            ok: bool,
            #[serde(default)]
            result: Option<serde_json::Value>,
            #[serde(default)]
            error: Option<WireError>,
        }
        let raw = Raw::deserialize(deserializer)?;
        if raw.v != WIRE_VERSION {
            return Err(serde::de::Error::custom(format!(
                "未知 Wire 版本: {}",
                raw.v
            )));
        }
        if raw.ok {
            Ok(ResponseEnvelope::Success {
                request_id: raw.request_id,
                result: raw
                    .result
                    .ok_or_else(|| serde::de::Error::missing_field("result"))?,
            })
        } else {
            Ok(ResponseEnvelope::Failure {
                request_id: raw.request_id,
                error: raw
                    .error
                    .ok_or_else(|| serde::de::Error::missing_field("error"))?,
            })
        }
    }
}

// ---- session.create ------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub name: String,
    /// 降级链,按序尝试;1..=4 项(合同 minItems/maxItems)。
    pub model_chain: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<Budget>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCreateParams {
    pub agent: AgentSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    pub event_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCreateResult {
    pub session_id: BmId,
    pub agent_id: BmId,
    pub created_at: BmTimestamp,
    pub resume_cursor: Cursor,
}

// ---- session.resume ------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionResumeParams {
    pub session_id: BmId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_seq: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionResumeResult {
    pub session_state: SessionState,
    pub agent_state: AgentState,
    pub last_event_seq: u64,
    pub events: Vec<EventEnvelope>,
}

// ---- session.close -------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCloseParams {
    pub session_id: BmId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCloseResult {
    pub closed_at: BmTimestamp,
    /// 进行中的回合不被取消,仅脱离(INV-6)。
    pub agent_final_state: String,
}

// ---- events.poll ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventsPollParams {
    pub session_id: BmId,
    pub since_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventsPollResult {
    pub events: Vec<EventEnvelope>,
    pub last_seq: u64,
    pub has_more: bool,
}

// ---- agent.send_input ----------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendInputParams {
    pub session_id: BmId,
    pub agent_id: BmId,
    pub content: String,
    pub input_trust: InputTrust,
}

/// 执行收据(agent #/definitions/receipt,基线 9.5)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    pub operation_id: BmId,
    pub request_id: BmId,
    /// M1 仅本地用户身份;M4 起扩展 principal 体系(基线 4.4)。
    pub principal: Principal,
    pub task_type: TaskType,
    pub state: OperationState,
    pub created_at: BmTimestamp,
    /// 终态前为 null。
    #[serde(default)]
    pub completed_at: Option<BmTimestamp>,
    pub action_summary: String,
    #[serde(default)]
    pub result_reference: Option<ResultReference>,
    /// 非 failed/outcome_unknown 终态时为 null。
    #[serde(default)]
    pub error: Option<WireError>,
}

wire_str_enum!(Principal {
    User => "user",
});

wire_str_enum!(TaskType {
    AgentTurn => "agent.turn",
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultReference {
    pub kind: ResultRefKind,
    pub r#ref: String,
}

wire_str_enum!(ResultRefKind {
    ExecutionLog => "execution_log",
});

// ---- agent.cancel --------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CancelParams {
    pub session_id: BmId,
    pub agent_id: BmId,
    pub operation_id: BmId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CancelResult {
    pub accepted: bool,
    pub operation_id: BmId,
}

// ---- operations.get ------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetOperationParams {
    pub operation_id: BmId,
}
