//! StateDb 域方法(自 sqlite_state.rs 机械移入;内容零改动)。
use super::StateDb;
use super::rows::GrantRow;
use crate::error::StoreResult;

impl StateDb {
    /// 写入/更新 Grant(revoked 标志/版本/消费计数随推进;T6c 起消费余量持久)。
    pub fn save_grant(&self, row: GrantRow<'_>) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO grants(id, audience, action, revocation_version, revoked, used_count,
                                payload, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET revocation_version = excluded.revocation_version,
                 revoked = excluded.revoked, used_count = excluded.used_count,
                 payload = excluded.payload",
            rusqlite::params![
                row.id,
                row.audience,
                row.action,
                row.revocation_version as i64,
                row.revoked as i64,
                row.used_count as i64,
                row.payload,
                row.created_at
            ],
        )?;
        Ok(())
    }

    /// 恢复面:全部 Grant 行(id, audience, action, revocation_version, revoked,
    /// used_count, payload)。
    pub fn list_grants(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT id, audience, action, revocation_version, revoked, used_count, payload
             FROM grants ORDER BY created_at",
            &[],
        )
    }
}
