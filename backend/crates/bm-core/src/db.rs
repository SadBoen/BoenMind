//! 持久化（Turso/limbo，SQLite 文件格式兼容）：会话（sessions）与消息（messages）。
//!
//! 数据库位于 `~/.boenmind/boenmind.db`。v2 使用单连接 + 互斥锁（tokio Mutex，
//! 异步方法持锁跨 await 需要），写操作频率低，足以满足个人使用场景。
//!
//! 2026-08-12 由 rusqlite 迁移至 turso 0.7.2（limbo Rust 绑定）：
//! 文件格式直接兼容，现有数据文件无需转换即可打开。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;
use turso::{Builder, Connection};

use crate::config::app_dir;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 一次工具调用（挂在 assistant 消息下，按 seq 排序回放）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    pub seq: i64,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub is_error: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
    /// 该消息关联的工具调用（仅 assistant 消息有）
    pub tool_calls: Vec<ToolCall>,
}

pub struct Db {
    conn: Mutex<Connection>,
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn db_path() -> PathBuf {
    app_dir().join("boenmind.db")
}

impl Db {
    /// 打开（必要时创建）数据库并初始化表结构。
    pub async fn open() -> Result<Self, turso::Error> {
        let path = db_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let db = Builder::new_local(path.to_str().unwrap_or("boenmind.db"))
            .build()
            .await?;
        let conn = db.connect()?;
        // 返回值的 pragma（journal_mode）需用 pragma_update 而非 execute
        conn.pragma_update("journal_mode", "WAL").await?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL DEFAULT '新对话',
                provider_id TEXT,
                model       TEXT,
                created_at  INTEGER NOT NULL,
                updated_at  INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS messages (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                role       TEXT NOT NULL,
                content    TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, id);
            CREATE TABLE IF NOT EXISTS tool_calls (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                seq        INTEGER NOT NULL,
                tool_name  TEXT NOT NULL,
                args       TEXT NOT NULL,
                is_error   INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_tool_calls_message ON tool_calls(message_id, seq);
            "#,
        )
        .await?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub async fn create_session(
        &self,
        id: &str,
        provider_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<Session, turso::Error> {
        let ts = now_ts();
        self.conn.lock().await.execute(
            "INSERT INTO sessions (id, title, provider_id, model, created_at, updated_at)
             VALUES (?1, '新对话', ?2, ?3, ?4, ?4)",
            (id, provider_id, model, ts),
        ).await?;
        Ok(Session {
            id: id.to_string(),
            title: "新对话".to_string(),
            provider_id: provider_id.map(str::to_string),
            model: model.map(str::to_string),
            created_at: ts,
            updated_at: ts,
        })
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>, turso::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, provider_id, model, created_at, updated_at
                 FROM sessions ORDER BY updated_at DESC",
            )
            .await?;
        let mut rows = stmt.query(()).await?;
        let mut sessions = Vec::new();
        while let Some(row) = rows.next().await? {
            sessions.push(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                provider_id: row.get(2)?,
                model: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            });
        }
        Ok(sessions)
    }

    pub async fn get_session(&self, id: &str) -> Result<Option<Session>, turso::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, provider_id, model, created_at, updated_at
                 FROM sessions WHERE id = ?1",
            )
            .await?;
        let mut rows = stmt.query([id]).await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        Ok(Some(Session {
            id: row.get(0)?,
            title: row.get(1)?,
            provider_id: row.get(2)?,
            model: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        }))
    }

    pub async fn rename_session(&self, id: &str, title: &str) -> Result<(), turso::Error> {
        let ts = now_ts();
        self.conn.lock().await.execute(
            "UPDATE sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
            (title, ts, id),
        ).await?;
        Ok(())
    }

    pub async fn touch_session(&self, id: &str) -> Result<(), turso::Error> {
        let ts = now_ts();
        self.conn.lock().await.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            (ts, id),
        ).await?;
        Ok(())
    }

    pub async fn delete_session(&self, id: &str) -> Result<usize, turso::Error> {
        let n = self
            .conn
            .lock()
            .await
            .execute("DELETE FROM sessions WHERE id = ?1", [id])
            .await?;
        Ok(n as usize)
    }

    pub async fn add_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> Result<Message, turso::Error> {
        let ts = now_ts();
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO messages (session_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
            (session_id, role, content, ts),
        )
        .await?;
        let id = conn.last_insert_rowid();
        Ok(Message {
            id,
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            created_at: ts,
            tool_calls: Vec::new(),
        })
    }

    /// 给指定消息追加工具调用记录（seq 从 0 起按序编号）。
    pub async fn add_tool_calls(
        &self,
        message_id: i64,
        calls: &[(String, serde_json::Value, bool)],
    ) -> Result<(), turso::Error> {
        let conn = self.conn.lock().await;
        for (seq, (name, args, is_error)) in calls.iter().enumerate() {
            conn.execute(
                "INSERT INTO tool_calls (message_id, seq, tool_name, args, is_error) VALUES (?1, ?2, ?3, ?4, ?5)",
                (message_id, seq as i64, name.as_str(), args.to_string(), *is_error as i64),
            )
            .await?;
        }
        Ok(())
    }

    /// 读取某条消息的工具调用（按 seq 排序）。
    pub async fn list_tool_calls(&self, message_id: i64) -> Result<Vec<ToolCall>, turso::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT seq, tool_name, args, is_error FROM tool_calls WHERE message_id = ?1 ORDER BY seq",
            )
            .await?;
        let mut rows = stmt.query([message_id]).await?;
        let mut calls = Vec::new();
        while let Some(row) = rows.next().await? {
            let args: String = row.get(2)?;
            calls.push(ToolCall {
                seq: row.get(0)?,
                tool_name: row.get(1)?,
                args: serde_json::from_str(&args).unwrap_or(serde_json::Value::Null),
                is_error: row.get::<i64>(3)? != 0,
            });
        }
        Ok(calls)
    }

    pub async fn list_messages(&self, session_id: &str) -> Result<Vec<Message>, turso::Error> {
        // 先查询消息（作用域结束即释放连接锁），再逐条读工具调用，避免重入死锁
        let messages: Vec<Message> = {
            let conn = self.conn.lock().await;
            let mut stmt = conn
                .prepare(
                    "SELECT id, session_id, role, content, created_at
                     FROM messages WHERE session_id = ?1 ORDER BY id",
                )
                .await?;
            let mut rows = stmt.query([session_id]).await?;
            let mut messages = Vec::new();
            while let Some(row) = rows.next().await? {
                messages.push(Message {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                    tool_calls: Vec::new(),
                });
            }
            messages
        };
        // 消息量小，逐条查询工具调用即可
        let mut messages = messages;
        for msg in &mut messages {
            msg.tool_calls = self.list_tool_calls(msg.id).await?;
        }
        Ok(messages)
    }
}
