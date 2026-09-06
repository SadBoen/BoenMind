//! StateDb 域方法(自 sqlite_state.rs 机械移入;内容零改动)。
use super::StateDb;
use super::rows::ApprovalRow;
use crate::error::StoreResult;

impl StateDb {
    // ---- v3:approvals / grants / capabilities / outbox(M4)------------------
    // 载荷列 = 合同形态 JSON 文本(capability/*.schema.json);行级键列供索引。

    /// 写入/更新审批对象(upsert)。
    pub fn save_approval(&self, row: ApprovalRow<'_>) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO approvals(id, operation_id, capability, principal, state, payload,
                                   created_at, resolved_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state,
                 payload = excluded.payload, resolved_at = excluded.resolved_at",
            rusqlite::params![
                row.id,
                row.operation_id,
                row.capability,
                row.principal,
                row.state,
                row.payload,
                row.created_at,
                row.resolved_at
            ],
        )?;
        Ok(())
    }

    pub fn approval_payload(&self, id: &str) -> StoreResult<Option<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn.prepare("SELECT payload FROM approvals WHERE id = ?1")?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_approvals_by_state(&self, state: &str) -> StoreResult<Vec<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt =
            conn.prepare("SELECT payload FROM approvals WHERE state = ?1 ORDER BY created_at")?;
        let mut rows = stmt.query([state])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row.get(0)?);
        }
        Ok(out)
    }

    /// 恢复面:全部审批行(id, operation_id, state, payload)。
    pub fn list_approvals(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT id, operation_id, state, payload FROM approvals ORDER BY created_at",
            &[],
        )
    }
}
