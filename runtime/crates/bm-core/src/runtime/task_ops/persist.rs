//! 自 task_ops.rs 机械移入(内容零改动)。
use super::*;

pub struct SpawnMemberParams {
    pub task_id: BmId,
}
pub struct SpawnSubtaskParams {
    pub parent_task_id: BmId,
    pub title: String,
    pub goal: String,
    pub authorization: Vec<wire::TaskAuthorizationEntry>,
    pub budget: Option<bm_contract::budget::Budget>,
}
pub struct RemoveMemberParams {
    pub task_id: BmId,
    pub agent_id: BmId,
    pub reason: String,
}
pub struct WorkerCallParams {
    pub task_id: BmId,
    pub capability: String,
    pub args: serde_json::Value,
    pub idempotency_key: Option<String>,
    pub deadline_ms: Option<u64>,
}
pub(crate) fn persist_task(w: &mut World, task: &crate::task::Task) {
    let Some(store) = w.store.clone() else {
        return;
    };
    let payload = task_contract_json(task);
    if let Err(e) = store.save_task(bm_persist::sqlite_state::TaskRow {
        id: task.id.as_str(),
        title: &task.title,
        state: task.state.as_str(),
        created_by: &task.created_by,
        task_epoch: task.task_epoch,
        payload: &payload,
        created_at: task.created_at.as_str(),
        updated_at: task.updated_at.as_str(),
        parent_task_id: task.parent_task_id.as_ref().map(|p| p.as_str()),
        delegation_depth: task.delegation_depth,
    }) {
        tracing::error!(error = %e, task = %task.id.as_str(), "Task 行落库失败,进入拒写态");
        w.persist_poisoned = true;
    }
}
pub(crate) fn task_contract_json(task: &crate::task::Task) -> String {
    const DEFAULT_MAX_TOKENS: i64 = 1_000_000;
    const DEFAULT_MAX_TURNS: i64 = 1_000;
    let budget_json = match &task.budget {
        Some(b) => serde_json::to_value(b).unwrap_or(serde_json::json!({})),
        None => serde_json::json!({
            "max_tokens": DEFAULT_MAX_TOKENS, "max_turns": DEFAULT_MAX_TURNS
        }),
    };
    serde_json::json!({
        "task_id": task.id.as_str(),
        "title": task.title,
        "goal": task.goal,
        "state": task.state.as_str(),
        "created_by": task.created_by,
        "task_epoch": task.task_epoch,
        "authorization": task.authorization,
        "budget": budget_json,
        "deadline": task.deadline.as_deref(),
        "members": [],
        "parent_task_id": task.parent_task_id.as_ref().map(|p| p.as_str()),
        "delegation_depth": task.delegation_depth,
        "created_at": task.created_at.as_str(),
        "updated_at": task.updated_at.as_str(),
    })
    .to_string()
}
