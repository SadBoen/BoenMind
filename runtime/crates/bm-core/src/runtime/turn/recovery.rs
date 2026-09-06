//! 自 turn.rs 机械移入(内容零改动)。
use super::*;

pub(crate) fn handle_recovery_settle(
    w: &mut World,
    operation_id: BmId,
    verdict: RecoveryVerdict,
) -> CoreResult<Receipt> {
    if w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "持久层故障,Runtime 拒写".into(),
        ));
    }
    let from = {
        let op = w
            .operations
            .get(&operation_id)
            .ok_or_else(|| CoreError::validation("operation 不存在"))?;
        op.state
    };
    // INV-10/11:只允许恢复态被裁定,且只走迁移表合法边
    let target = match (from, verdict) {
        (OperationState::OutcomeUnknown, RecoveryVerdict::Succeeded) => {
            Some(OperationState::Succeeded)
        }
        (OperationState::OutcomeUnknown, RecoveryVerdict::Failed) => Some(OperationState::Failed),
        (OperationState::Interrupted, RecoveryVerdict::ClaimRun) => Some(OperationState::Running),
        (OperationState::Interrupted, RecoveryVerdict::Cancelled) => {
            Some(OperationState::Cancelled)
        }
        (OperationState::OutcomeUnknown, RecoveryVerdict::Cancelled) => {
            return Err(CoreError::validation(
                "outcome_unknown 无 →cancelled 边:只能经核验落 succeeded/failed(INV-11)",
            ));
        }
        (OperationState::Interrupted, RecoveryVerdict::Succeeded)
        | (OperationState::Interrupted, RecoveryVerdict::Failed) => {
            return Err(CoreError::validation(
                "interrupted 无直达 succeeded/failed 的边:claim 续跑或裁定取消",
            ));
        }
        _ => {
            return Err(CoreError::validation(
                "仅恢复态(outcome_unknown/interrupted)可裁定",
            ));
        }
    };
    let target = target.expect("上表已穷尽");

    // claim 续跑:需要受保护存储中的输入原文
    if target == OperationState::Running {
        let content = w
            .store
            .as_ref()
            .and_then(|s| s.op_input(operation_id.as_str()).ok())
            .flatten()
            .ok_or_else(|| CoreError::validation("无输入上下文,不可续跑(裁定取消或核验结论)"))?;
        w.settle_operation(&operation_id, OperationState::Running, None);
        let agent = w
            .agents
            .get(&w.operations[&operation_id].agent_id)
            .cloned()
            .expect("存在");
        {
            let agent_id = agent.id.clone();
            let a = w.agents.get_mut(&agent_id).expect("存在");
            a.transition(AgentState::WaitingModel);
        }
        spawn_turn(w, &agent, &operation_id, content, None);
        Ok(w.receipt_of(&w.operations[&operation_id]))
    } else {
        let error = match target {
            OperationState::Failed => {
                let mut e =
                    WireError::new(ErrorCode::OutcomeUnknown, "恢复裁定:按失败收口".to_string());
                e.retryable = false;
                Some(e)
            }
            OperationState::Cancelled => Some(WireError::new(
                ErrorCode::Cancelled,
                "恢复裁定:用户裁定取消".to_string(),
            )),
            _ => None,
        };
        w.settle_operation(&operation_id, target, error);
        Ok(w.receipt_of(&w.operations[&operation_id]))
    }
}
