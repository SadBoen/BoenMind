//! SQLite 规范状态库:schema 版本化迁移(PRAGMA user_version,expand-contract)
//! + meta 表(含 CAS 写入门禁底座,ADR-0004 条件 3)。
//!
//! 行级 materialize(事件 → 行变更)自 T2 接入;本文件只负责打开/迁移/meta。

use crate::error::{StoreError, StoreResult};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub const SCHEMA_VERSION: i64 = 1;

pub struct StateDb {
    conn: Mutex<Connection>,
}

impl StateDb {
    /// 打开并迁移到最新 schema。新建库直接建表;旧库按 user_version 逐级迁移。
    pub fn open(path: &Path) -> StoreResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        match version {
            0 => Self::migrate_v0_to_v1(&conn)?,
            SCHEMA_VERSION => {}
            other => {
                return Err(StoreError::Corrupt {
                    seq: 0,
                    reason: format!("未知 schema 版本 {other}(库来自更新的实现?)"),
                });
            }
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn migrate_v0_to_v1(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            CREATE TABLE meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE sessions (
                id         TEXT PRIMARY KEY,
                state      TEXT NOT NULL,
                agent_id   TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE agents (
                id                  TEXT PRIMARY KEY,
                session_id          TEXT NOT NULL,
                name                TEXT NOT NULL,
                model_chain         TEXT NOT NULL,
                state               TEXT NOT NULL,
                budget_max_tokens   INTEGER,
                budget_max_turns    INTEGER,
                budget_used_tokens  INTEGER NOT NULL DEFAULT 0,
                budget_turns_used   INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE operations (
                id             TEXT PRIMARY KEY,
                session_id     TEXT NOT NULL,
                agent_id       TEXT NOT NULL,
                request_id     TEXT NOT NULL,
                state          TEXT NOT NULL,
                turn_index     INTEGER NOT NULL,
                created_at     TEXT NOT NULL,
                completed_at   TEXT,
                action_summary TEXT,
                result_ref     TEXT,
                error_code     TEXT,
                error_message  TEXT
            );
            CREATE TABLE tombstones (
                kind TEXT NOT NULL,
                id   TEXT NOT NULL,
                at   TEXT NOT NULL,
                PRIMARY KEY (kind, id)
            );
            COMMIT;
            "#,
        )?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    /// meta 读。
    pub fn meta_get(&self, key: &str) -> StoreResult<Option<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// meta 无条件写。
    pub fn meta_set(&self, key: &str, value: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// meta CAS:仅当现值等于 expect 时写入。返回是否成功;
    /// 不匹配返回 CasMismatch(调用方据此产生 store.write.rejected 审计事件)。
    pub fn meta_compare_and_set(
        &self,
        key: &str,
        expect: Option<&str>,
        new: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        let current: Option<String> = {
            let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
            let mut rows = stmt.query([key])?;
            rows.next()?.map(|r| r.get(0)).transpose()?
        };
        if current.as_deref() != expect {
            return Err(StoreError::CasMismatch {
                key: key.to_string(),
                expect: expect.unwrap_or("<absent>").to_string(),
            });
        }
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, new],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_migrates_and_sets_version() {
        let dir = tempfile::tempdir().expect("临时目录");
        let db = StateDb::open(&dir.path().join("state.db")).expect("打开");
        assert_eq!(
            db.meta_get("last_applied_seq").expect("读"),
            None,
            "新库 meta 为空"
        );
    }

    #[test]
    fn meta_set_and_cas() {
        let dir = tempfile::tempdir().expect("临时目录");
        let db = StateDb::open(&dir.path().join("state.db")).expect("打开");

        db.meta_set("last_applied_seq", "5").expect("写");
        assert_eq!(
            db.meta_get("last_applied_seq").expect("读"),
            Some("5".into())
        );

        // CAS 成功:expect 匹配
        db.meta_compare_and_set("last_applied_seq", Some("5"), "9")
            .expect("CAS 成功");
        assert_eq!(
            db.meta_get("last_applied_seq").expect("读"),
            Some("9".into())
        );

        // CAS 失败:expect 过期 → CasMismatch,值不变
        let err = db
            .meta_compare_and_set("last_applied_seq", Some("5"), "99")
            .expect_err("过期 expect 必须被拒");
        assert!(matches!(err, StoreError::CasMismatch { .. }));
        assert_eq!(
            db.meta_get("last_applied_seq").expect("读"),
            Some("9".into())
        );

        // CAS 对不存在的键:expect = None
        db.meta_compare_and_set("fresh", None, "1")
            .expect("absent 分支");
        assert_eq!(db.meta_get("fresh").expect("读"), Some("1".into()));
    }
}
