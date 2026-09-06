//! 自 task_ops.rs 机械移入(内容零改动)。
use super::*;

pub(crate) fn handle_task_list(
    w: &World,
    params: wire::TaskListParams,
) -> CoreResult<wire::TaskListResult> {
    let mut tasks: Vec<&crate::task::Task> = w
        .tasks
        .values()
        .filter(|t| match &params.state_filter {
            Some(f) => t.state.as_str() == f,
            None => true,
        })
        .collect();
    tasks.sort_by(|a, b| (&a.created_at, a.id.as_str()).cmp(&(&b.created_at, b.id.as_str())));
    Ok(wire::TaskListResult {
        tasks: tasks
            .iter()
            .map(|t| serde_json::from_str(&task_contract_json(t)).unwrap_or_default())
            .collect(),
    })
}
pub(crate) fn handle_task_get(
    w: &World,
    params: wire::TaskGetParams,
) -> CoreResult<wire::TaskGetResult> {
    let Some(task) = w.tasks.get(&params.task_id) else {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 不存在: {}", params.task_id.as_str()),
        ));
    };
    Ok(wire::TaskGetResult {
        task: serde_json::from_str(&task_contract_json(task)).unwrap_or_default(),
        guard_states: None,
    })
}
pub(crate) fn task_error_to_core(task_id: &BmId, e: crate::task::TaskError) -> CoreError {
    let msg = match e {
        crate::task::TaskError::IllegalTransition { from, to } => format!(
            "Task {} 表外迁移: {} -> {}",
            task_id.as_str(),
            from.as_str(),
            to.as_str()
        ),
        crate::task::TaskError::UnverifiedCompletion => {
            format!("Task {} 无核验结论不得终局(完成判定门禁)", task_id.as_str())
        }
        crate::task::TaskError::StaleEpoch { current, presented } => format!(
            "Task {} 命令携带过期 epoch({presented},当前 {current}),Stale 拒绝",
            task_id.as_str()
        ),
    };
    CoreError::Semantic(ErrorCode::ValidationFailed, msg)
}
