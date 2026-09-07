//! StateDb 域方法(自 sqlite_state.rs 机械移入;内容零改动)。
use super::StateDb;
use crate::error::{SqlResultExt, StoreResult};

impl StateDb {
    /// 幂等收据落表(T6c):key_hash → 原收据;恢复后抑制判定不依赖内存。
    pub fn save_idem_receipt(
        &self,
        key_hash: &str,
        payload: &str,
        created_at: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO idempotency_receipts(key_hash, payload, created_at)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(key_hash) DO NOTHING",
            rusqlite::params![key_hash, payload, created_at],
        )
        .sql()?;
        Ok(())
    }

    /// 读幂等收据(T6c:恢复期判定「外部是否已执行」)。
    pub fn idem_receipt(&self, key_hash: &str) -> StoreResult<Option<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn
            .prepare("SELECT payload FROM idempotency_receipts WHERE key_hash = ?1")
            .sql()?;
        let mut rows = stmt.query([key_hash]).sql()?;
        if let Some(row) = rows.next().sql()? {
            Ok(Some(row.get(0).sql()?))
        } else {
            Ok(None)
        }
    }

    /// 恢复面:全部幂等收据行。
    pub fn list_idem_receipts(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT key_hash, payload, created_at FROM idempotency_receipts ORDER BY created_at",
            &[],
        )
    }
}
