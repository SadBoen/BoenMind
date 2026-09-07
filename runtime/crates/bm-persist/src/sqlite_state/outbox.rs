//! StateDb 域方法(自 sqlite_state.rs 机械移入;内容零改动)。
use super::StateDb;
use crate::error::{SqlResultExt, StoreResult};

impl StateDb {
    /// outbox 记录 upsert(T6 副作用对账底座;状态 pending→published→verified)。
    pub fn outbox_upsert(
        &self,
        operation_id: &str,
        kind: &str,
        state: &str,
        payload: &str,
        now: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO outbox(operation_id, kind, state, payload, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(operation_id, kind) DO UPDATE SET state = excluded.state,
                 payload = excluded.payload, updated_at = excluded.updated_at",
            rusqlite::params![operation_id, kind, state, payload, now],
        )
        .sql()?;
        Ok(())
    }

    /// 恢复面:指定状态的 outbox 行。
    pub fn list_outbox_by_state(&self, state: &str) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT operation_id, kind, state, payload FROM outbox WHERE state = ?1",
            rusqlite::params![state],
        )
    }

    // ---- v4:tasks / task_members / idempotency_receipts(M5-T1)--------------
    // 载荷列 = task/task.v0.1 合同形态 JSON;行级键列供索引与恢复。
}
