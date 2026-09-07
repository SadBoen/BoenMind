//! 自 turn.rs 机械移入(内容零改动)。
use super::*;

/// W10(ADR-0024):上限可配形态(事件轨迹快照截断;调用方读 limits)。
pub(crate) fn content_trunc_with(content: &str, limit: usize) -> String {
    if content.len() <= limit {
        content.to_string()
    } else {
        let mut end = limit;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        content[..end].to_string()
    }
}
pub(crate) fn emit_model_call_error_audit(w: &mut World, operation_id: &BmId, code: ErrorCode) {
    if let Some(a) = w.model_call_audit.remove(operation_id) {
        emit_capability_invoked_with(
            w,
            &a.call_id,
            operation_id,
            "model.invoke",
            &a.principal,
            Some(a.epoch),
            Some(&a.instance_id),
            "error",
            Some(code),
            None,
        );
    }
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_capability_invoked(
    w: &mut World,
    op_id: &BmId,
    capability: &str,
    principal: &str,
    epoch: Option<u64>,
    instance: Option<&str>,
    outcome: &str,
    error_code: Option<ErrorCode>,
    key_hash: Option<&str>,
) {
    let call_id = w.config.id_gen.next_id("call");
    emit_capability_invoked_with(
        w, &call_id, op_id, capability, principal, epoch, instance, outcome, error_code, key_hash,
    );
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_capability_invoked_with(
    w: &mut World,
    call_id: &BmId,
    op_id: &BmId,
    capability: &str,
    principal: &str,
    epoch: Option<u64>,
    instance: Option<&str>,
    outcome: &str,
    error_code: Option<ErrorCode>,
    key_hash: Option<&str>,
) {
    w.emit(
        EventType::CapabilityInvoked,
        None,
        None,
        Some(op_id.clone()),
        serde_json::json!({
            "call_id": call_id.as_str(),
            "operation_id": op_id.as_str(),
            "capability": capability,
            "principal": principal,
            "binding_epoch": epoch.unwrap_or(0),
            "provider_instance_id": instance.unwrap_or("n/a"),
            "outcome": outcome,
            "error_code": error_code.map(|c| c.as_str()),
            "idempotency_key_hash": key_hash,
        }),
    );
}
pub(crate) fn error_code_of(outcome: &CallOutcome) -> ErrorCode {
    match outcome {
        CallOutcome::InvalidArgs { .. } => ErrorCode::ValidationFailed,
        CallOutcome::StaleBinding { .. } => ErrorCode::Unavailable,
        CallOutcome::ProviderError { .. } | CallOutcome::InvalidOutput { .. } => {
            ErrorCode::Internal
        }
        CallOutcome::ProviderUnavailable { .. } => ErrorCode::Unavailable,
        CallOutcome::Rejected { .. } => ErrorCode::PermissionDenied,
        _ => ErrorCode::Internal,
    }
}
pub(crate) fn sha256_hex(s: &str) -> String {
    bm_contract::hash::sha256_hex(s.as_bytes())
}
