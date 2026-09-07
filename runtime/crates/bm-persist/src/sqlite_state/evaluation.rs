//! StateDb 域方法(自 sqlite_state.rs 机械移入;内容零改动)。
use super::StateDb;
use crate::error::{SqlResultExt, StoreResult};

impl StateDb {
    /// M8.7:评估报告写入(同 report_id 覆盖;报告为派生工件)。
    pub fn save_evaluation_report(
        &self,
        report_id: &str,
        from_seq: u64,
        to_seq: u64,
        payload: &str,
        created_at: &str,
    ) -> StoreResult<()> {
        self.conn
            .lock()
            .expect("锁未中毒")
            .execute(
                "INSERT OR REPLACE INTO evaluation_reports                  (report_id, from_seq, to_seq, payload, created_at)                  VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    report_id,
                    from_seq as i64,
                    to_seq as i64,
                    payload,
                    created_at
                ],
            )
            .sql()?;
        Ok(())
    }

    /// M8.7:评估报告列表(按创建时间)。
    pub fn list_evaluation_reports(&self) -> StoreResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn
            .prepare(
                "SELECT report_id, payload, created_at FROM evaluation_reports ORDER BY created_at",
            )
            .sql()?;
        let rows = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "report_id": r.get::<_, String>(0)?,
                    "payload": r.get::<_, String>(1)?,
                    "created_at": r.get::<_, String>(2)?,
                }))
            })
            .sql()?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.sql()?);
        }
        Ok(out)
    }
}
