//! checkpoint 策略（dsh checkpoint-policy 语义）：
//!
//! - **请求边界 fsync**：连接级 `synchronous=FULL`（WAL 模式下每事务
//!   提交即 fsync），每个 append/append_batch 是一次请求边界；
//! - **崩溃 interrupted 恢复**：checkpoint_state 表记录最近确认的
//!   `last_seq` + 状态位。写前标记 `interrupted`，成功后置 `clean`；
//!   启动时若发现 `interrupted`，核对 event_log 实际头 seq —— SQLite
//!   事务原子性保证不会半写，头 seq 若大于 last_seq 说明事务已提交，
//!   直接恢复 clean；若小于/等于则无需截断（无半写数据）。

use bm_protocol::{ErrorCode, ProtocolError};
use tokio::sync::Mutex;
use turso::{Builder, Connection};

/// 崩溃恢复状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointState {
    /// 上次请求边界正常结束
    Clean,
    /// 上次请求边界未确认（启动需核对恢复）
    Interrupted,
}

const MIGRATE_CHECKPOINT: &str = r#"
CREATE TABLE IF NOT EXISTS checkpoint_state (
  id       INTEGER PRIMARY KEY CHECK (id = 1),
  last_seq INTEGER NOT NULL DEFAULT 0,
  state    TEXT NOT NULL DEFAULT 'clean'
);
"#;

/// 独立连接的 checkpoint 记账器（与事件日志同文件、不同连接，
/// WAL 模式允许多连接并发读写）。
pub struct CheckpointStore {
    conn: Mutex<Connection>,
}

impl CheckpointStore {
    /// 打开（建表）。返回 (store, 恢复前状态)。
    pub async fn open(path: &str) -> Result<(Self, CheckpointState), ProtocolError> {
        let db = Builder::new_local(path)
            .build()
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("open checkpoint {path}: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("connect checkpoint: {e}")))?;
        conn.pragma_update("journal_mode", "WAL")
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("journal_mode: {e}")))?;
        conn.pragma_update("synchronous", "FULL")
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("synchronous: {e}")))?;
        // 与 event_log 同库不同连接：写锁争用内部等待（多实例共享 db 场景）
        conn.pragma_update("busy_timeout", 5000)
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("busy_timeout: {e}")))?;
        conn.execute_batch(MIGRATE_CHECKPOINT)
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("migrate checkpoint: {e}")))?;
        // 种子行
        conn.execute(
            "INSERT OR IGNORE INTO checkpoint_state (id, last_seq, state) VALUES (1, 0, 'clean')",
            (),
        )
        .await
        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("seed checkpoint: {e}")))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        let state = store.read_state().await?;
        Ok((store, state))
    }

    /// 写前标记：interrupted（请求边界开始）。
    pub async fn mark_interrupted(&self) -> Result<(), ProtocolError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE checkpoint_state SET state = 'interrupted' WHERE id = 1",
            (),
        )
        .await
        .map(|_| ())
        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("mark interrupted: {e}")))
    }

    /// 请求成功后置 clean 并记录 last_seq。
    pub async fn mark_clean(&self, last_seq: u64) -> Result<(), ProtocolError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE checkpoint_state SET last_seq = ?1, state = 'clean' WHERE id = 1",
            (last_seq as i64,),
        )
        .await
        .map(|_| ())
        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("mark clean: {e}")))
    }

    pub async fn read_state(&self) -> Result<CheckpointState, ProtocolError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare("SELECT state FROM checkpoint_state WHERE id = 1")
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare state: {e}")))?;
        let mut rows = stmt
            .query(())
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query state: {e}")))?;
        let Some(row) = rows
            .next()
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next state: {e}")))?
        else {
            return Ok(CheckpointState::Clean);
        };
        let s: String = row
            .get(0)
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get state: {e}")))?;
        Ok(if s == "interrupted" {
            CheckpointState::Interrupted
        } else {
            CheckpointState::Clean
        })
    }

    /// 启动恢复：interrupted 时核对事件日志实际头 seq，归一 clean。
    /// 返回 (恢复前状态, 实际头 seq)。
    pub async fn recover(&self, actual_head: Option<u64>) -> Result<CheckpointState, ProtocolError> {
        let state = self.read_state().await?;
        if state == CheckpointState::Interrupted {
            // 事务原子性保证无半写；头 seq 若存在说明上次提交已落盘
            self.mark_clean(actual_head.unwrap_or(0)).await?;
        }
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn checkpoint_lifecycle() {
        let path = format!(
            "{}/bm_checkpoint_test_{}.db",
            std::env::temp_dir().display(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let (store, state) = CheckpointStore::open(&path).await.unwrap();
        assert_eq!(state, CheckpointState::Clean);
        store.mark_interrupted().await.unwrap();
        store.mark_clean(7).await.unwrap();
        assert_eq!(store.read_state().await.unwrap(), CheckpointState::Clean);
        // 恢复：clean 状态下 no-op
        let recovered = store.recover(Some(7)).await.unwrap();
        assert_eq!(recovered, CheckpointState::Clean);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn interrupted_recovers() {
        let path = format!(
            "{}/bm_checkpoint_test2_{}.db",
            std::env::temp_dir().display(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        let (store, _) = CheckpointStore::open(&path).await.unwrap();
        store.mark_interrupted().await.unwrap();
        assert_eq!(store.read_state().await.unwrap(), CheckpointState::Interrupted);
        // 模拟崩溃重启：实际头 seq=3（事务已提交），恢复后归 clean
        let before = store.recover(Some(3)).await.unwrap();
        assert_eq!(before, CheckpointState::Interrupted);
        assert_eq!(store.read_state().await.unwrap(), CheckpointState::Clean);
        let _ = std::fs::remove_file(&path);
    }
}
