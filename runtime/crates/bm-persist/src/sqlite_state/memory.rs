//! StateDb 域方法(自 sqlite_state.rs 机械移入;内容零改动)。
use super::StateDb;
use crate::error::{StoreError, StoreResult};

impl StateDb {
    /// Observation Log 条目落表(log_seq 自 MAX+1 单调分配),返回 seq。
    pub fn save_observation(
        &self,
        task_id: &str,
        verdict: &str,
        guard_state: &str,
        payload: &str,
        observed_at: &str,
    ) -> StoreResult<u64> {
        let conn = self.conn.lock().expect("锁未中毒");
        let next: i64 = conn.query_row(
            "SELECT COALESCE(MAX(log_seq), 0) + 1 FROM observations",
            [],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO observations(log_seq, task_id, verdict, guard_state, payload,
                                      observed_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![next, task_id, verdict, guard_state, payload, observed_at],
        )?;
        Ok(next as u64)
    }

    /// 记忆写入(墓碑语义见 delete),返回 entry_id(id 由调用方给定)。
    /// P0-4(2026-09-07 架构评审):写入/纠正墓碑/FTS 索引并入单事务——
    /// 此前各自独立提交,崩溃窗口内 DER 与 LIKE 兜底两面数据不一致;
    /// 吞错改结构化日志(此前 `let _` 静默,检索质量降级无人知晓)。
    #[allow(clippy::too_many_arguments)]
    pub fn memory_put(
        &self,
        entry_id: &str,
        scope: &str,
        _content_ref: &str,
        content_preview: Option<&str>,
        _source_trust: &str,
        source_ref: Option<&str>,
        correction_of: Option<&str>,
        payload: &str,
        created_at: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO memories(id, scope, tombstoned, content_preview, source_ref,
                                  correction_of, payload, created_at)
             VALUES(?1, ?2, 0, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            rusqlite::params![
                entry_id,
                scope,
                content_preview,
                source_ref,
                correction_of,
                payload,
                created_at
            ],
        )?;
        // 用户纠正:被纠正条目立即墓碑化(覆盖而非追加,基线 §4.1)
        if let Some(target) = correction_of {
            tx.execute(
                "UPDATE memories SET tombstoned = 1 WHERE id = ?1",
                rusqlite::params![target],
            )
            .map_err(StoreError::Sql)?;
        }
        // FTS5 索引(失败不阻断写入:LIKE 兜底,但必须可观测)
        if let Some(preview) = content_preview
            && let Err(e) = tx.execute(
                "INSERT INTO memories_fts(rowid, content)
                 VALUES((SELECT rowid FROM memories WHERE id = ?1), ?2)",
                rusqlite::params![entry_id, preview],
            )
        {
            tracing::warn!(entry = %entry_id, error = %e, "memory FTS 索引写入失败(检索退化为 LIKE 兜底)");
        }
        tx.commit()?;
        Ok(())
    }

    /// 记忆检索:scope 内非墓碑条目(FTS5 MATCH 优先,LIKE 兜底)。
    pub fn memory_search(&self, scope: &str, query: &str) -> StoreResult<Vec<serde_json::Value>> {
        let rows = self.query_rows(
            "SELECT m.id, m.scope, m.content_preview, m.source_ref, m.payload
             FROM memories m JOIN memories_fts f ON m.rowid = f.rowid
             WHERE m.scope = ?1 AND m.tombstoned = 0 AND memories_fts MATCH ?2",
            rusqlite::params![scope, format!("\"{query}\"")],
        );
        match rows {
            Ok(r) => Ok(r),
            Err(_) => self.query_rows(
                "SELECT id, scope, content_preview, source_ref, payload FROM memories
                 WHERE scope = ?1 AND tombstoned = 0
                   AND content_preview LIKE ('%' || ?2 || '%')",
                rusqlite::params![scope, query],
            ),
        }
    }

    /// 记忆删除:墓碑 + 来源级联失效。返回级联数。
    pub fn memory_delete(&self, entry_id: &str) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "UPDATE memories SET tombstoned = 1 WHERE id = ?1",
            [entry_id],
        )?;
        let cascaded = conn.execute(
            "UPDATE memories SET tombstoned = 1
             WHERE source_ref = ?1 AND tombstoned = 0",
            [entry_id],
        )?;
        Ok(cascaded)
    }
}
