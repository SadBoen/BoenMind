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

    pub fn delete_session(&self, id: &str) -> Result<usize, rusqlite::Error> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])
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
            tool_calls: Vec::new(),
        })
    }

    /// 给指定消息追加工具调用记录（seq 从 0 起按序编号）。
    pub fn add_tool_calls(
        &self,
        message_id: i64,
        calls: &[(String, serde_json::Value, bool)],
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        for (seq, (name, args, is_error)) in calls.iter().enumerate() {
            conn.execute(
                "INSERT INTO tool_calls (message_id, seq, tool_name, args, is_error) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![message_id, seq as i64, name, args.to_string(), *is_error as i64],
            )?;
        }
        Ok(())
    }

    /// 读取某条消息的工具调用（按 seq 排序）。
    pub fn list_tool_calls(&self, message_id: i64) -> Result<Vec<ToolCall>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT seq, tool_name, args, is_error FROM tool_calls WHERE message_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![message_id], |row| {
            let args: String = row.get(2)?;
            Ok(ToolCall {
                seq: row.get(0)?,
                tool_name: row.get(1)?,
                args: serde_json::from_str(&args).unwrap_or(serde_json::Value::Null),
                is_error: row.get::<_, i64>(3)? != 0,
            })
        })?;
        rows.collect()
    }

    pub fn list_messages(&self, session_id: &str) -> Result<Vec<Message>, rusqlite::Error> {
        // 先查询消息（作用域结束即释放连接锁），再逐条读工具调用，避免重入死锁
        let messages: Vec<Message> = {
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
                    tool_calls: Vec::new(),
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        // 消息量小，逐条查询工具调用即可
        let mut messages = messages;
        for msg in &mut messages {
            msg.tool_calls = self.list_tool_calls(msg.id)?;
        }
        Ok(messages)
    }
}
