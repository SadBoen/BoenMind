//! StateDb 域方法(自 sqlite_state.rs 机械移入;内容零改动)。
use super::StateDb;
use super::rows::TaskRow;
use crate::error::{SqlResultExt, StoreResult};

impl StateDb {
    /// 写入/更新 Task(upsert;task_epoch 单调由调用方保证,恢复时取 max)。
    pub fn save_task(&self, row: TaskRow<'_>) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO tasks(id, title, state, created_by, task_epoch, payload,
                               created_at, updated_at, parent_task_id, delegation_depth)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title,
                 state = excluded.state, task_epoch = excluded.task_epoch,
                 payload = excluded.payload, updated_at = excluded.updated_at,
                 parent_task_id = excluded.parent_task_id,
                 delegation_depth = excluded.delegation_depth",
            rusqlite::params![
                row.id,
                row.title,
                row.state,
                row.created_by,
                row.task_epoch as i64,
                row.payload,
                row.created_at,
                row.updated_at,
                row.parent_task_id,
                row.delegation_depth as i64,
            ],
        )
        .sql()?;
        Ok(())
    }

    /// 恢复面:全部 Task 行。
    pub fn list_tasks(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT id, title, state, created_by, task_epoch, payload, created_at,
                    updated_at, parent_task_id, delegation_depth
             FROM tasks ORDER BY created_at",
            &[],
        )
    }

    /// Task 预算账本行 upsert(agent_id = "" 为 Task 级聚合行)。
    pub fn save_task_budget(
        &self,
        task_id: &str,
        agent_id: &str,
        used_tool_calls: u64,
        used_tokens: u64,
        now: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO task_budget_ledger(task_id, agent_id, used_tokens, used_tool_calls,
                                          updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(task_id, agent_id) DO UPDATE SET
                 used_tokens = excluded.used_tokens,
                 used_tool_calls = excluded.used_tool_calls,
                 updated_at = excluded.updated_at",
            rusqlite::params![
                task_id,
                agent_id,
                used_tokens as i64,
                used_tool_calls as i64,
                now
            ],
        )
        .sql()?;
        Ok(())
    }

    /// 恢复面:全部预算账本行。
    pub fn list_task_budget(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT task_id, agent_id, used_tokens, used_tool_calls, updated_at
             FROM task_budget_ledger",
            &[],
        )
    }
}
