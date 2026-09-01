//! Wire API 信封与方法参数/结果镜像
//! (wire/envelope + wire/session + wire/agent + wire/capability + wire/task,v0.1)。
//!
//! M1 方法集合(7 个):session.create / session.resume / session.close /
//! events.poll / agent.send_input / agent.cancel / operations.get。
//! M4 增发(3 个):capability.call / approval.list / approval.respond
//! (服务端实现随 M4-T5)。
//! M5 增发(6 个):task.create / task.list / task.get / task.pause /
//! task.resume / task.stop(服务端实现随 M5-T2)。

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
    // M5 增发(2026-08-29,Minor:envelope method 枚举同步;params/result 见
    // wire/task.v0_1;服务端行为 M5-T2 实现,当前 unavailable)
    TaskCreate => "task.create",
    TaskList => "task.list",
    TaskGet => "task.get",
    TaskPause => "task.pause",
    TaskResume => "task.resume",
    TaskStop => "task.stop",
    // M8 增发(2026-08-30,Minor:envelope method 枚举同步;M7.5 语义取消)
    CapabilityCancel => "capability.cancel",
    // M9 增发(2026-08-30,Minor:envelope method 枚举同步;worker 自主环 v0)
    TaskAutorun => "task.autorun",
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
    /// W4 角色:会话级 system prompt(设置页「角色」定义;None = 无)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
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
    /// M5 增发:按 Task 过滤事件流(null = 不过滤;wire/session events.poll)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<BmId>,
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

// ---- capability.call / approval.list / approval.respond(M4;wire/capability)--

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityCallParams {
    pub capability: String,
    pub args: serde_json::Value,
    /// external-side-effect 类必备(基线 §9.5/ADR-0004);M4 暂记调用面。
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub deadline_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalListParams {
    /// 缺省 = 全部;waiting_user 在前由实现侧排序。
    #[serde(default)]
    pub state_filter: Option<String>,
}

/// decision:approve | deny | withdraw(实现侧校验;approve 必带 scope,
/// 须 ∈ 该 Approval 的 scope_choices —— wire/capability 合同 description)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRespondParams {
    pub approval_id: BmId,
    pub decision: String,
    #[serde(default)]
    pub scope: Option<String>,
}

/// M9-S3:worker 自主环 v0(task.autorun)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskAutorunParams {
    pub task_id: BmId,
    /// 至多推进的模型回合数(缺省 6;预算硬限独立生效)。
    #[serde(default)]
    pub max_turns: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskAutorunResult {
    pub session_id: BmId,
    pub agent_id: BmId,
    pub accepted: bool,
}

/// M8.3:能力调用语义取消(在途异步调用;迟到完成丢弃)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityCancelParams {
    pub operation_id: BmId,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityCancelResult {
    pub operation_id: BmId,
    pub state: String,
}

// ---- task.create / task.list / task.get / task.pause / task.resume /
//      task.stop(M5;wire/task)------------------------------------------

/// Task 授权声明条目(task/task.v0.1 authorization_entry):三方交集的
/// Task 分量载体;mutation 动词必须显式列入(ADR-0002 §11.2)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskAuthorizationEntry {
    pub verb: String,
    /// safe | mutation(须与动词默认分级一致)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub klass: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskCreateParams {
    pub title: String,
    pub goal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization: Option<Vec<TaskAuthorizationEntry>>,
    /// Task 预算包络(基线 §9.7;开放键值,缺省用运行时默认包络)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<crate::budget::Budget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<BmTimestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskCreateResult {
    pub task_id: BmId,
    pub state: crate::states::TaskState,
    pub created_at: BmTimestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskGetParams {
    pub task_id: BmId,
}

/// task.list result(tasks = task/task.v0.1 对象数组;顺序确定性由实现侧保证)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskListResult {
    pub tasks: Vec<serde_json::Value>,
}

/// task.get result(guard_states = 监护态投影,基线 §20 八态;T7 起填充)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskGetResult {
    pub task: serde_json::Value,
    pub guard_states: Option<serde_json::Value>,
}

/// Task 生命周期命令共用 result(pause/resume/stop)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskStateResult {
    pub task_id: BmId,
    pub state: crate::states::TaskState,
}

/// task.pause / task.resume / task.stop 共用 params(各方法按合同只填
/// 自己声明的可选字段:pause/stop 用 reason,resume 用 note)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskLifecycleParams {
    pub task_id: BmId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}
