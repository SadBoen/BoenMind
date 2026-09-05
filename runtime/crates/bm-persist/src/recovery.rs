//! 启动恢复(M2 规格 §5.2/任务 T3):
//! ① 修复窗口——重放日志中「状态位点之后」的尾部(崩溃落在 ①日志/②物化 之间);
//! ② 行装配——把 SQLite 规范状态读回,供建仓内存视图;
//! ③ 中断清点——找出崩溃时未到终态的 operation,交由核心循环走
//!    running→interrupted(事务崩溃 guard)与 interrupted→resuming→running
//!    的恢复迁移并留审计事件。
//!
//! 恢复语义边界(ADR-0004 条件 5,基线 L1023):只承诺规范状态/存在性恢复与
//! 幂等续跑,不承诺编排决策过程重放。

use crate::error::StoreResult;
use crate::sqlite_state::StateDb;
use crate::store::EventStore;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryReport {
    /// 恢复完成后的状态位点。
    pub last_applied_seq: u64,
    /// 修复窗口内重放(补物化)的事件数。
    pub replayed: usize,
    /// 被标记 interrupted 的未终态 operation 数。
    pub interrupted_recovered: usize,
}

/// 规范状态行(装配内存视图的载体)。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionRow {
    pub id: String,
    pub state: String,
    pub agent_id: String,
    pub created_at: String,
    /// 重启续聊配套(2026-09-06):会话绑定工作目录(未绑定 = None)。
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentRow {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub model_chain: String,
    pub state: String,
    pub budget_max_tokens: Option<i64>,
    pub budget_max_turns: Option<i64>,
    pub budget_used_tokens: i64,
    pub budget_turns_used: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OperationRow {
    pub id: String,
    pub session_id: String,
    pub agent_id: String,
    pub request_id: Option<String>,
    pub state: String,
    pub turn_index: i64,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub action_summary: Option<String>,
    pub result_reference: Option<String>,
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub input_content: Option<String>,
}

/// Task 规范状态行(M5-T1;payload = task/task.v0.1 合同 JSON)。
#[derive(Debug, Clone, Deserialize)]
pub struct TaskStateRow {
    pub id: String,
    pub title: String,
    pub state: String,
    pub created_by: String,
    pub task_epoch: i64,
    pub payload: String,
    pub created_at: String,
    pub updated_at: String,
    pub parent_task_id: Option<String>,
    pub delegation_depth: i64,
}

#[derive(Debug, Clone, Default)]
pub struct WorldRows {
    pub sessions: Vec<SessionRow>,
    pub agents: Vec<AgentRow>,
    pub operations: Vec<OperationRow>,
    pub tasks: Vec<TaskStateRow>,
}

/// ① 修复窗口:重放位点之后的日志尾部并补物化。返回补放条数。
pub fn repair_tail(store: &dyn EventStore) -> StoreResult<usize> {
    let applied = store.last_applied_seq()?;
    let tail = store.replay_since(applied)?;
    let n = tail.len();
    for event in tail {
        store.materialize_event(&event)?;
        store.mark_applied(event.event_seq)?;
    }
    Ok(n)
}

/// ③ 中断清点:未终态 operation 的 (id, agent_id, state)。
pub fn pending_operations(state: &StateDb) -> StoreResult<Vec<(String, String, String)>> {
    let rows = state.query_rows(
        "SELECT id, agent_id, state FROM operations
         WHERE state IN ('not_started', 'running', 'interrupted')",
        &[],
    )?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap_or_default().to_string(),
                r["agent_id"].as_str().unwrap_or_default().to_string(),
                r["state"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect())
}

/// 投影重建(ADR-0004 条件 1 / 混沌③):把 seq ≤ upto 的事件经同一 reducer
/// 物化进一个全新 StateDb。两次重建结果必须逐字段一致(确定性由「同一
/// materialize 函数」结构保证,本函数即混沌③的被测对象)。
pub fn rebuild_projection(
    store: &dyn EventStore,
    upto_seq: u64,
    dest: &StateDb,
) -> StoreResult<u64> {
    let events = store.replay_since(0)?;
    let mut last = 0u64;
    for event in events {
        if event.event_seq > upto_seq {
            break;
        }
        dest.materialize(&event)?;
        last = event.event_seq;
    }
    if last > 0 {
        let expect = dest.meta_get(crate::store::META_LAST_APPLIED)?;
        if expect.is_none() {
            dest.meta_compare_and_set(crate::store::META_LAST_APPLIED, None, &last.to_string())?;
        }
    }
    Ok(last)
}

/// 全表导出(确定性比对用):规范 JSON。
pub fn dump_all(state: &StateDb) -> StoreResult<serde_json::Value> {
    use serde_json::json;
    Ok(json!({
        "meta": state.query_rows("SELECT key, value FROM meta ORDER BY key", &[])?,
        "sessions": state.query_rows("SELECT * FROM sessions ORDER BY id", &[])?,
        "agents": state.query_rows("SELECT * FROM agents ORDER BY id", &[])?,
        "operations": state.query_rows("SELECT * FROM operations ORDER BY id", &[])?,
        "tasks": state.query_rows("SELECT * FROM tasks ORDER BY id", &[])?,
        "task_members": state.query_rows("SELECT * FROM task_members ORDER BY task_id, agent_id", &[])?,
        "grants": state.query_rows("SELECT * FROM grants ORDER BY id", &[])?,
    }))
}

/// ID 计数提示:扫描全部持久化发号表(会话/代理/操作/任务/授权/审批/记忆)
/// 取最大值(P0 第四轮评审修复:此前漏 task/grant/approval/memory,重启
/// 回退会撞号覆写权力记录)。
/// 重启后的 ID 生成必须从 hint+1 起,否则 INSERT OR REPLACE 会覆盖历史行
/// (M3 server 的单写者租约前置,任务 T2)。
pub fn id_counter_hint(state: &StateDb) -> StoreResult<u64> {
    let rows = state.query_rows(
        "SELECT MAX(n) AS m FROM (
            SELECT CAST(substr(id, 6) AS INTEGER) AS n FROM sessions
            UNION ALL SELECT CAST(substr(id, 7) AS INTEGER) FROM agents
            UNION ALL SELECT CAST(substr(id, 4) AS INTEGER) FROM operations
            UNION ALL SELECT CAST(substr(id, 6) AS INTEGER) FROM tasks
            UNION ALL SELECT CAST(substr(id, 7) AS INTEGER) FROM grants
            UNION ALL SELECT CAST(substr(id, 6) AS INTEGER) FROM approvals
            UNION ALL SELECT CAST(substr(id, 5) AS INTEGER) FROM memories
         )",
        &[],
    )?;
    Ok(rows
        .first()
        .and_then(|r| r["m"].as_i64())
        .unwrap_or(0)
        .max(0) as u64)
}

/// ② 行装配。
pub fn load_rows(state: &StateDb) -> StoreResult<WorldRows> {
    let sessions = state
        .query_rows(
            "SELECT id, state, agent_id, created_at, workspace_id FROM sessions",
            &[],
        )?
        .into_iter()
        .map(|v| serde_json::from_value(v).expect("行结构与 SessionRow 一致"))
        .collect();
    let agents = state
        .query_rows(
            "SELECT id, session_id, name, model_chain, state, budget_max_tokens,
                    budget_max_turns, budget_used_tokens, budget_turns_used
             FROM agents",
            &[],
        )?
        .into_iter()
        .map(|v| serde_json::from_value(v).expect("行结构与 AgentRow 一致"))
        .collect();
    let operations = state
        .query_rows(
            "SELECT id, session_id, agent_id, request_id, state, turn_index, created_at,
                    completed_at, action_summary, result_ref, error_code, error_message,
                    input_content
             FROM operations",
            &[],
        )?
        .into_iter()
        .map(|v| serde_json::from_value(v).expect("行结构与 OperationRow 一致"))
        .collect();
    let tasks = state
        .query_rows(
            "SELECT id, title, state, created_by, task_epoch, payload, created_at,
                    updated_at, parent_task_id, delegation_depth
             FROM tasks",
            &[],
        )?
        .into_iter()
        .map(|v| serde_json::from_value(v).expect("行结构与 TaskStateRow 一致"))
        .collect();
    Ok(WorldRows {
        sessions,
        agents,
        operations,
        tasks,
    })
}
