//! StateDb 域方法(自 sqlite_state.rs 机械移入;内容零改动)。
use super::StateDb;
use crate::error::{SqlResultExt, StoreError, StoreResult};

impl StateDb {
    /// 保存回合输入原文(受保护存储;A4:原文不进事件/日志)。
    pub fn save_op_input(&self, operation_id: &str, content: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "UPDATE operations SET input_content = ?2 WHERE id = ?1",
            rusqlite::params![operation_id, content],
        )
        .sql()?;
        Ok(())
    }

    /// 读回合输入原文(claim 续跑用)。
    pub fn op_input(&self, operation_id: &str) -> StoreResult<Option<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn
            .prepare("SELECT input_content FROM operations WHERE id = ?1")
            .sql()?;
        let mut rows = stmt.query([operation_id]).sql()?;
        if let Some(row) = rows.next().sql()? {
            Ok(row.get(0).sql()?)
        } else {
            Ok(None)
        }
    }

    /// 持久化取消意图标记(2026-09-05 回看修复:显式取消必须可跨崩溃存续)。
    pub fn mark_op_cancelled(&self, operation_id: &str, marked_at: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT OR REPLACE INTO op_cancel_marks (op_id, marked_at) VALUES (?1, ?2)",
            rusqlite::params![operation_id, marked_at],
        )
        .sql()?;
        Ok(())
    }

    /// 会话绑定工作目录持久化(2026-09-06 重启续聊配套)。
    pub fn save_session_workspace(
        &self,
        session_id: &str,
        workspace_id: Option<&str>,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "UPDATE sessions SET workspace_id = ?2 WHERE id = ?1",
            rusqlite::params![session_id, workspace_id],
        )
        .sql()?;
        Ok(())
    }

    /// 会话删除侧效(2026-09-06 A+B):单事务内 ①墓碑(tombstones 防事件
    /// 重放复活)②该会话全部 operations.input_content 置空(用户消息原文
    /// 擦除;操作元数据行保留供审计)。
    pub fn erase_session_contents(&self, session_id: &str, at: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute_batch("BEGIN").sql()?;
        let r: Result<(), rusqlite::Error> = (|| {
            conn.execute(
                "INSERT OR REPLACE INTO tombstones (kind, id, at) VALUES ('session', ?1, ?2)",
                rusqlite::params![session_id, at],
            )?;
            conn.execute(
                "UPDATE operations SET input_content = NULL WHERE session_id = ?1",
                rusqlite::params![session_id],
            )?;
            Ok(())
        })();
        match r {
            Ok(()) => conn.execute_batch("COMMIT").sql()?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(StoreError::Sql(e.to_string()));
            }
        }
        Ok(())
    }

    /// 会话行与其 agent 行删除(墓碑已在,防复活靠 erase_session_contents;
    /// 事件重放侧由 recovery::load_rows 跳过墓碑会话兜底)。
    pub fn delete_session_rows(&self, session_id: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute_batch("BEGIN").sql()?;
        let r: Result<(), rusqlite::Error> = (|| {
            conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
            conn.execute("DELETE FROM agents WHERE session_id = ?1", [session_id])?;
            Ok(())
        })();
        match r {
            Ok(()) => conn.execute_batch("COMMIT").sql()?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(StoreError::Sql(e.to_string()));
            }
        }
        Ok(())
    }

    /// 查询取消意图标记(恢复端判定 turn_was_stopping)。
    pub fn op_cancel_requested(&self, operation_id: &str) -> StoreResult<bool> {
        let conn = self.conn.lock().expect("锁未中毒");
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM op_cancel_marks WHERE op_id = ?1",
                [operation_id],
                |r| r.get(0),
            )
            .sql()?;
        Ok(n > 0)
    }

    /// 通用行查询(恢复与测试读取用;返回按列名的 JSON 对象数组)。
    pub fn query_rows(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> StoreResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn.prepare(sql).sql()?;
        let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
        let mut rows = stmt.query(params).sql()?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().sql()? {
            let mut obj = serde_json::Map::new();
            for (i, name) in names.iter().enumerate() {
                let v: serde_json::Value = match row.get_ref(i).sql()? {
                    rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => n.into(),
                    rusqlite::types::ValueRef::Real(f) => f.into(),
                    rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into(),
                    rusqlite::types::ValueRef::Blob(_) => serde_json::Value::Null,
                };
                obj.insert(name.clone(), v);
            }
            out.push(serde_json::Value::Object(obj));
        }
        Ok(out)
    }
}
