//! StateDb 域方法(自 sqlite_state.rs 机械移入;内容零改动)。
use super::StateDb;
use super::rows::CapabilityRow;
use crate::error::{SqlResultExt, StoreResult};

impl StateDb {
    /// 写入/更新 capability binding(epoch 单调由调用方保证,恢复时取 max)。
    pub fn save_capability_binding(&self, row: CapabilityRow<'_>) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO capabilities(capability, provider_instance_id, epoch, status,
                                      manifest, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(capability) DO UPDATE SET
                 provider_instance_id = excluded.provider_instance_id,
                 epoch = excluded.epoch, status = excluded.status,
                 manifest = excluded.manifest, updated_at = excluded.updated_at",
            rusqlite::params![
                row.capability,
                row.provider_instance_id,
                row.epoch as i64,
                row.status,
                row.manifest,
                row.updated_at
            ],
        )
        .sql()?;
        Ok(())
    }

    /// 删除 capability binding(热拔/重载)。
    pub fn delete_capability_binding(&self, capability: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "DELETE FROM capabilities WHERE capability = ?1",
            rusqlite::params![capability],
        )
        .sql()?;
        Ok(())
    }

    /// 恢复面:全部 binding 行。
    pub fn list_capability_bindings(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT capability, provider_instance_id, epoch, status, manifest
             FROM capabilities ORDER BY capability",
            &[],
        )
    }
}
