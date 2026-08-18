//! # kernel-storage
//!
//! SQLite 持久化后端（BoenMind 微内核 M1 的 `SessionPersistPort` 实现）。
//!
//! ## 存储模型（v2.1 拍板）
//!
//! - append-only `events` 事件日志 = **唯一事实源**；`sessions` 表只是 header 索引。
//! - **原子性 = 无 torn-tail 的持久层保证**：每次 `append_events` / `create_session`
//!   都是单个 SQLite 事务——一个批次内的所有事件要么全落盘要么全不落。
//!   kill -9 发生在事务提交前 → 批次整体丢失 → 日志永远没有半条事件；
//!   重启加载后尾部必然完整。
//! - **fsync 语义**：`PRAGMA journal_mode=WAL` + `synchronous=FULL`（每个提交事务都
//!   fsync，WAL 保证崩溃后日志尾部不撕裂）。
//!
//! `rusqlite::Connection` 是 `Send` 但非 `Sync`，因此必须用 `std::sync::Mutex` 包裹
//! （M1 下单连接串行化写操作，同步阻塞可接受）。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use kernel_contracts::error::{PortError, PortResult};
use kernel_contracts::ports::SessionPersistPort;
use kernel_contracts::session::{SessionEvent, SessionHeader, SessionId, SessionRecord};
use rusqlite::{params, Connection};
use thiserror::Error;

/// 存储层错误（`SqlitePersist::open` 使用；端口方法统一映射为 `PortError::Backend`）。
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// 把任意可 Display 的底层错误映射为 `PortError::Backend`（fail-loud：底层细节不外泄）。
fn to_port_error(e: impl std::fmt::Display) -> PortError {
    PortError::backend(e.to_string())
}

/// SQLite 持久化后端。
pub struct SqlitePersist {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl SqlitePersist {
    /// 打开（或创建）数据库：建父目录 → `PRAGMA journal_mode=WAL` →
    /// `PRAGMA synchronous=FULL` → 初始化 schema。
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        // 顺序要求：先 WAL 再 FULL（FULL 对 WAL 连接才生效）。
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             PRAGMA synchronous=FULL; \
             PRAGMA foreign_keys=ON; \
             PRAGMA busy_timeout=5000;",
        )?;
        Self::init_schema(&conn)?;
        Ok(Self {
            path: path.to_path_buf(),
            conn: Mutex::new(conn),
        })
    }

    /// 数据库文件路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn init_schema(conn: &Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                header_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                session_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                event_json TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                PRIMARY KEY (session_id, seq),
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            );",
        )?;
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, PortError> {
        self.conn
            .lock()
            .map_err(|_| PortError::backend("storage mutex poisoned"))
    }
}

#[async_trait]
impl SessionPersistPort for SqlitePersist {
    /// 单事务：INSERT `sessions(header_json)` + 首条 `SessionStarted` 事件（seq=1）。
    /// 存储时把 header 的 `updated_at` 归一化为 `created_at`（创建即"最后活跃"）。
    async fn create_session(&self, header: &SessionHeader) -> PortResult<()> {
        let mut header = header.clone();
        header.updated_at = header.created_at;

        let session_id = header.id.as_str().to_string();
        let created_at = header.created_at.to_rfc3339();
        let header_json = serde_json::to_string(&header).map_err(to_port_error)?;
        let event = SessionEvent::SessionStarted { header };
        let event_json = serde_json::to_string(&event).map_err(to_port_error)?;

        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(to_port_error)?;

        match tx.query_row("SELECT 1 FROM sessions WHERE id=?1", params![session_id], |_| Ok(())) {
            Ok(()) => {
                return Err(PortError::invalid_request(&format!(
                    "session {session_id} already exists"
                )));
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(to_port_error(e)),
        }

        tx.execute(
            "INSERT INTO sessions (id, header_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, header_json, created_at, created_at],
        )
        .map_err(to_port_error)?;
        tx.execute(
            "INSERT INTO events (session_id, seq, event_json, timestamp) VALUES (?1, 1, ?2, ?3)",
            params![session_id, event_json, created_at],
        )
        .map_err(to_port_error)?;
        tx.commit().map_err(to_port_error)?;
        Ok(())
    }

    /// 单事务批量 INSERT；seq 从该会话已存在最大 seq 续算（+1 起，与 events 顺序对应）。
    /// 会话不存在 → `NotFound`。空批次直接返回 `Ok`。
    async fn append_events(&self, session_id: &str, events: &[SessionEvent]) -> PortResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(to_port_error)?;

        match tx.query_row("SELECT 1 FROM sessions WHERE id=?1", params![session_id], |_| Ok(())) {
            Ok(()) => {}
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Err(PortError::not_found(&format!("session {session_id} not found")));
            }
            Err(e) => return Err(to_port_error(e)),
        }

        let max_seq: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM events WHERE session_id=?1",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(to_port_error)?;

        let now = Utc::now();
        let timestamp = now.to_rfc3339();
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO events (session_id, seq, event_json, timestamp) \
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(to_port_error)?;
            for (i, event) in events.iter().enumerate() {
                let seq = max_seq + 1 + i as i64;
                let event_json = serde_json::to_string(event).map_err(to_port_error)?;
                stmt.execute(params![session_id, seq, event_json, timestamp])
                    .map_err(to_port_error)?;
            }
        }

        tx.execute(
            "UPDATE sessions SET updated_at=?1 WHERE id=?2",
            params![now.to_rfc3339(), session_id],
        )
        .map_err(to_port_error)?;
        tx.commit().map_err(to_port_error)?;
        Ok(())
    }

    /// 按 seq 升序返回完整事件记录（含磁盘 seq/timestamp——时间线保真，
    /// 恢复时直接沿用落盘时间，不重造）；会话不存在 → `Ok(None)`。
    async fn load_events(
        &self,
        session_id: &str,
    ) -> PortResult<Option<Vec<SessionRecord>>> {
        let conn = self.lock()?;
        match conn.query_row("SELECT 1 FROM sessions WHERE id=?1", params![session_id], |_| Ok(())) {
            Ok(()) => {}
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(to_port_error(e)),
        }

        let mut stmt = conn
            .prepare("SELECT seq, event_json, timestamp FROM events WHERE session_id=?1 ORDER BY seq ASC")
            .map_err(to_port_error)?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                let seq: i64 = row.get(0)?;
                let event_json: String = row.get(1)?;
                let timestamp: String = row.get(2)?;
                Ok((seq, event_json, timestamp))
            })
            .map_err(to_port_error)?;
        let mut records = Vec::new();
        for row in rows {
            let (seq, event_json, timestamp) = row.map_err(to_port_error)?;
            let event: SessionEvent = serde_json::from_str(&event_json).map_err(to_port_error)?;
            records.push(SessionRecord {
                seq: seq as u64,
                timestamp: timestamp
                    .parse::<chrono::DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                session_id: SessionId(session_id.to_string()),
                event,
            });
        }
        Ok(Some(records))
    }

    /// 按最近活跃（updated_at）降序列出全部会话 id。
    async fn list_sessions(&self) -> PortResult<Vec<String>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id FROM sessions ORDER BY updated_at DESC")
            .map_err(to_port_error)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(to_port_error)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(to_port_error)?);
        }
        Ok(out)
    }

    /// 事务内手动删除两个表（不依赖 CASCADE，更稳）。幂等：会话不存在也返回 `Ok`。
    async fn delete_session(&self, session_id: &str) -> PortResult<()> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(to_port_error)?;
        tx.execute("DELETE FROM events WHERE session_id=?1", params![session_id])
            .map_err(to_port_error)?;
        tx.execute("DELETE FROM sessions WHERE id=?1", params![session_id])
            .map_err(to_port_error)?;
        tx.commit().map_err(to_port_error)?;
        Ok(())
    }

    /// 全量重写会话事件日志（事务内 DELETE + INSERT）。
    /// interrupted-turn 修复落盘用：恢复时把修剪后的完整日志写回，
    /// 保证磁盘与内存一致（无 torn-tail 是磁盘层的不变量）。
    /// 保留原始时间戳：事件日志=唯一事实源，修复不重盖时间线。
    async fn rewrite_events(
        &self,
        session_id: &str,
        events: &[SessionEvent],
    ) -> PortResult<()> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(to_port_error)?;
        // 先取现有时间戳（按 seq 映射），DELETE 后 INSERT 沿用——修复不重盖时间线。
        let mut timestamps: std::collections::HashMap<i64, String> =
            std::collections::HashMap::new();
        {
            let mut stmt = tx
                .prepare("SELECT seq, timestamp FROM events WHERE session_id=?1")
                .map_err(to_port_error)?;
            let rows = stmt
                .query_map(params![session_id], |row| {
                    let seq: i64 = row.get(0)?;
                    let ts: String = row.get(1)?;
                    Ok((seq, ts))
                })
                .map_err(to_port_error)?;
            for row in rows {
                let (seq, ts) = row.map_err(to_port_error)?;
                timestamps.insert(seq, ts);
            }
        }
        tx.execute("DELETE FROM events WHERE session_id=?1", params![session_id])
            .map_err(to_port_error)?;
        {
            let mut stmt = tx
                .prepare("INSERT INTO events (session_id, seq, event_json, timestamp) VALUES (?1, ?2, ?3, ?4)")
                .map_err(to_port_error)?;
            for (i, event) in events.iter().enumerate() {
                let seq = (i + 1) as i64;
                let event_json = serde_json::to_string(event).map_err(to_port_error)?;
                let ts = timestamps
                    .get(&seq)
                    .cloned()
                    .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
                stmt.execute(params![session_id, seq, event_json, ts])
                    .map_err(to_port_error)?;
            }
        }
        tx.execute(
            "UPDATE sessions SET updated_at=?1 WHERE id=?2",
            params![chrono::Utc::now().to_rfc3339(), session_id],
        )
        .map_err(to_port_error)?;
        tx.commit().map_err(to_port_error)?;
        Ok(())
    }
}
