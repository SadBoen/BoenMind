//! SQLite 持久化：会话（sessions）与消息（messages）。
//!
//! 数据库位于 `~/.boenmind/boenmind.db`。v1 使用单连接 + 互斥锁，
//! 写操作频率低，足以满足个人使用场景。

use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
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
    pub fn open() -> Result<Self, rusqlite::Error> {
        let path = db_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
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
            "#,
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn create_session(&self, id: &str, provider_id: Option<&str>, model: Option<&str>) -> Result<Session, rusqlite::Error> {
        let ts = now_ts();
        self.conn.lock().unwrap().execute(
            "INSERT INTO sessions (id, title, provider_id, model, created_at, updated_at)
             VALUES (?1, '新对话', ?2, ?3, ?4, ?4)",
            params![id, provider_id, model, ts],
        )?;
        Ok(Session {
            id: id.to_string(),
            title: "新对话".to_string(),
            provider_id: provider_id.map(str::to_string),
            model: model.map(str::to_string),
            created_at: ts,
            updated_at: ts,
        })
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, provider_id, model, created_at, updated_at
             FROM sessions ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                provider_id: row.get(2)?,
                model: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_session(&self, id: &str) -> Result<Option<Session>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, provider_id, model, created_at, updated_at
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                provider_id: row.get(2)?,
                model: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        rows.next().transpose()
    }

    pub fn rename_session(&self, id: &str, title: &str) -> Result<(), rusqlite::Error> {
        let ts = now_ts();
        self.conn.lock().unwrap().execute(
            "UPDATE sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![title, ts, id],
        )?;
        Ok(())
    }

    pub fn touch_session(&self, id: &str) -> Result<(), rusqlite::Error> {
        let ts = now_ts();
        self.conn.lock().unwrap().execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, id: &str) -> Result<(), rusqlite::Error> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn add_message(&self, session_id: &str, role: &str, content: &str) -> Result<Message, rusqlite::Error> {
        let ts = now_ts();
        self.conn.lock().unwrap().execute(
            "INSERT INTO messages (session_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, role, content, ts],
        )?;
        let id = self.conn.lock().unwrap().last_insert_rowid();
        Ok(Message {
            id,
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            created_at: ts,
        })
    }

    pub fn list_messages(&self, session_id: &str) -> Result<Vec<Message>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, created_at
             FROM messages WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(Message {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect()
    }
}
