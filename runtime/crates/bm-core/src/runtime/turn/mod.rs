//! 回合执行与能力异步引擎(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

use super::*;

mod audit;
mod capability;
mod events;
mod history;
mod recovery;
mod share;
mod spawn;

pub(super) use audit::{
    content_trunc_with, emit_capability_invoked, emit_capability_invoked_with,
    emit_model_call_error_audit, error_code_of, sha256_hex,
};
pub(super) use capability::{
    capability_call_inner, dispatch_capability, fail_capability_call, handle_capability_call,
    handle_capability_cancel, handle_provider_call, handle_provider_progress, persist_grant,
};
pub(super) use events::handle_turn_event;
pub(super) use history::{rebuild_session_chats, remember_turn, session_title_from};
pub(super) use recovery::handle_recovery_settle;
pub(super) use share::dispatch_share;
pub(super) use spawn::spawn_turn;

pub(crate) struct ModelCallAudit {
    pub(crate) call_id: BmId,
    pub(crate) epoch: u64,
    pub(crate) instance_id: String,
    pub(crate) principal: String,
}
pub(crate) struct AsyncCallMeta {
    pub(crate) capability: String,
    pub(crate) principal: String,
    pub(crate) call_id: BmId,
    pub(crate) epoch: u64,
    pub(crate) instance_id: String,
    pub(crate) key_hash: Option<String>,
    pub(crate) is_side_effect: bool,
    pub(crate) output_schema: String,
    pub(crate) grant_id: Option<String>,
}
pub(crate) struct PendingCapabilityCall {
    pub(crate) op_id: BmId,
    pub(crate) capability: String,
    pub(crate) args: serde_json::Value,
    pub(crate) idempotency_key: Option<String>,
    /// 调用方身份(M5 双路径:surface / worker;审批重放归因一致)
    pub(crate) principal: String,
    pub(crate) trust: DataTrust,
}
pub(crate) const CAPABILITY_CALLER: &str = "surface:user";

/// 审批对象持久化(payload = 包装 JSON:approval 合同形态;未决时附重放执行
/// 载荷 call,裁决后剥离)。写失败仅告警不阻断:审批对象当次仍在内存可裁决,
/// 重启丢失窗口留 T6 事务性 outbox 统一收紧。
pub(crate) fn persist_approval(
    w: &World,
    approval: &Approval,
    op_id: &BmId,
    pending: Option<(&str, &serde_json::Value, Option<&str>, &str, DataTrust)>,
) {
    if let Some(store) = &w.store {
        let mut wrap = serde_json::json!({ "approval": approval });
        if let Some((capability, args, idempotency_key, principal, trust)) = pending {
            wrap["call"] = serde_json::json!({
                "capability": capability, "args": args,
                "idempotency_key": idempotency_key,
                "principal": principal, "trust": trust.as_str()
            });
        }
        if let Err(e) = store.save_approval(crate::ports::persist::ApprovalRow {
            id: approval.approval_id.as_str(),
            operation_id: op_id.as_str(),
            capability: approval.capability.as_str(),
            principal: approval.principal.as_str(),
            state: approval.state.as_str(),
            payload: &wrap.to_string(),
            created_at: approval.requested_at.as_str(),
            resolved_at: approval.resolved_at.as_deref(),
        }) {
            // T6 outbox 收紧前维持「告警不阻断」口径,但不再静默
            tracing::error!(error = %e, approval = %approval.approval_id.as_str(),
                "审批行落库失败(当次内存可裁决;重启有丢失窗口,待 T6 outbox 收紧)");
        }
    }
}
pub(crate) fn evicted_turns(accounted: u64, alive: u64) -> u64 {
    accounted.saturating_sub(alive)
}
