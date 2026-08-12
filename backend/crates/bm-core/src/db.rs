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

/// 代理提交的改进建议（refine-suggest 插件 + bm-server 截获入库）。
/// status: pending（待审批）| approved（已批准生效）| rejected（已拒绝）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RefinementSuggestion {
    pub id: String,
    pub session_id: Option<String>,
    /// "skill:<id>" 或 "system_prompt"
    pub target: String,
    /// 目标描述中需修改的原文片段
    pub quote: String,
    /// 建议的替换/追加文本
    pub suggested: String,
    pub reason: String,
    pub status: String,
    pub created_at: i64,
    /// 批准生效时间（approve 时写入；用于展示"已生效"）
    pub applied_at: Option<i64>,
    /// 批准时产生的备份路径（skill 类型生效；rollback 用）
    pub backup_path: Option<String>,
}

/// 一次 prompt 回合的任务记录（断线续跑 + 心跳进度的持久化实体）。
/// status: running | completed | failed | cancelled。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub id: String,
    pub session_id: String,
    pub status: String,
    /// 心跳进度文本（最近的工具调用摘要/输出尾部）
    pub progress: String,
    pub started_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
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
            CREATE TABLE IF NOT EXISTS refinement_suggestions (
                id         TEXT PRIMARY KEY,
                session_id TEXT,
                target     TEXT NOT NULL,
                quote      TEXT NOT NULL,
                suggested  TEXT NOT NULL,
                reason     TEXT NOT NULL,
                status     TEXT NOT NULL DEFAULT 'pending',
                created_at INTEGER NOT NULL,
                applied_at INTEGER,
                backup_path TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_refine_status ON refinement_suggestions(status);
            CREATE TABLE IF NOT EXISTS tasks (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                status      TEXT NOT NULL DEFAULT 'running',
                progress    TEXT NOT NULL DEFAULT '',
                started_at  INTEGER NOT NULL,
                updated_at  INTEGER NOT NULL,
                finished_at INTEGER,
                error       TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_session ON tasks(session_id, started_at DESC);
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

    /// 更新会话的提供商与模型（聊天中切换模型时持久化，保证后续消息沿用新组合）
    pub async fn set_session_model(
        &self,
        id: &str,
        provider_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<(), turso::Error> {
        let ts = now_ts();
        self.conn.lock().await.execute(
            "UPDATE sessions SET provider_id = COALESCE(?1, provider_id),
                                   model = COALESCE(?2, model),
                                   updated_at = ?3 WHERE id = ?4",
            (provider_id, model, ts, id),
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
        // 外键默认关闭（turso/limbo 同 SQLite），ON DELETE CASCADE 不生效：
        // 手动级联删除 tool_calls → messages → sessions，避免孤儿数据
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM tool_calls WHERE message_id IN (SELECT id FROM messages WHERE session_id = ?1)",
            [id],
        )
        .await?;
        conn.execute("DELETE FROM messages WHERE session_id = ?1", [id]).await?;
        let n = conn.execute("DELETE FROM sessions WHERE id = ?1", [id]).await?;
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

    // ── 代理改进建议（refine-suggest 插件 + bm-server 截获入库）─────────

    pub async fn insert_refinement_suggestion(
        &self,
        id: &str,
        session_id: Option<&str>,
        target: &str,
        quote: &str,
        suggested: &str,
        reason: &str,
    ) -> Result<(), turso::Error> {
        let ts = now_ts();
        self.conn.lock().await.execute(
            "INSERT INTO refinement_suggestions
             (id, session_id, target, quote, suggested, reason, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
            (id, session_id, target, quote, suggested, reason, ts),
        ).await?;
        Ok(())
    }

    pub async fn list_refinement_suggestions(
        &self,
        status_filter: Option<&str>,
    ) -> Result<Vec<RefinementSuggestion>, turso::Error> {
        let conn = self.conn.lock().await;
        let mut out = Vec::new();
        if let Some(status) = status_filter {
            let mut stmt = conn
                .prepare(
                    "SELECT id, session_id, target, quote, suggested, reason, status, created_at, applied_at, backup_path
                     FROM refinement_suggestions WHERE status = ?1 ORDER BY created_at DESC",
                )
                .await?;
            let mut rows = stmt.query([status]).await?;
            while let Some(row) = rows.next().await? {
                out.push(row_to_suggestion(&row)?);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT id, session_id, target, quote, suggested, reason, status, created_at, applied_at, backup_path
                     FROM refinement_suggestions ORDER BY created_at DESC",
                )
                .await?;
            let mut rows = stmt.query(()).await?;
            while let Some(row) = rows.next().await? {
                out.push(row_to_suggestion(&row)?);
            }
        }
        Ok(out)
    }

    /// 更新建议状态（pending → approved/rejected）；approved 时写入 applied_at。
    pub async fn set_refinement_suggestion_status(
        &self,
        id: &str,
        status: &str,
    ) -> Result<(), turso::Error> {
        let ts = now_ts();
        self.conn.lock().await.execute(
            "UPDATE refinement_suggestions SET status = ?1,
             applied_at = CASE WHEN ?1 = 'approved' THEN ?2 ELSE applied_at END
             WHERE id = ?3",
            (status, ts, id),
        ).await?;
        Ok(())
    }

    /// 记录批准产生的备份路径（rollback 用）。
    pub async fn set_refinement_suggestion_backup(
        &self,
        id: &str,
        backup_path: &str,
    ) -> Result<(), turso::Error> {
        self.conn.lock().await.execute(
            "UPDATE refinement_suggestions SET backup_path = ?1 WHERE id = ?2",
            (backup_path, id),
        ).await?;
        Ok(())
    }

    /// 回滚后重置：状态回到 pending（可重新审批），清空生效信息。
    pub async fn reset_refinement_suggestion(&self, id: &str) -> Result<(), turso::Error> {
        self.conn.lock().await.execute(
            "UPDATE refinement_suggestions
             SET status = 'pending', applied_at = NULL, backup_path = NULL
             WHERE id = ?1",
            (id,),
        ).await?;
        Ok(())
    }

    pub async fn get_refinement_suggestion(
        &self,
        id: &str,
    ) -> Result<Option<RefinementSuggestion>, turso::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, target, quote, suggested, reason, status, created_at, applied_at, backup_path
                 FROM refinement_suggestions WHERE id = ?1",
            )
            .await?;
        let mut rows = stmt.query([id]).await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        Ok(Some(row_to_suggestion(&row)?))
    }

    // ── 任务（断线续跑 + 心跳进度；每 prompt 回合一条）───────────────

    pub async fn create_task(&self, id: &str, session_id: &str) -> Result<(), turso::Error> {
        let ts = now_ts();
        self.conn.lock().await.execute(
            "INSERT INTO tasks (id, session_id, status, progress, started_at, updated_at)
             VALUES (?1, ?2, 'running', '', ?3, ?3)",
            (id, session_id, ts),
        ).await?;
        Ok(())
    }

    /// 更新心跳进度（调用方控制频率；此处每次更新 updated_at）。
    pub async fn update_task_progress(&self, id: &str, progress: &str) -> Result<(), turso::Error> {
        let ts = now_ts();
        self.conn.lock().await.execute(
            "UPDATE tasks SET progress = ?1, updated_at = ?2 WHERE id = ?3",
            (progress, ts, id),
        ).await?;
        Ok(())
    }

    /// 结束任务（completed / failed / cancelled）。
    pub async fn finish_task(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), turso::Error> {
        let ts = now_ts();
        self.conn.lock().await.execute(
            "UPDATE tasks SET status = ?1, error = ?2, updated_at = ?3, finished_at = ?3
             WHERE id = ?4",
            (status, error, ts, id),
        ).await?;
        Ok(())
    }

    pub async fn list_tasks(&self, session_id: &str) -> Result<Vec<Task>, turso::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, status, progress, started_at, updated_at, finished_at, error
                 FROM tasks WHERE session_id = ?1 ORDER BY started_at DESC",
            )
            .await?;
        let mut rows = stmt.query([session_id]).await?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next().await? {
            tasks.push(Task {
                id: row.get(0)?,
                session_id: row.get(1)?,
                status: row.get(2)?,
                progress: row.get(3)?,
                started_at: row.get(4)?,
                updated_at: row.get(5)?,
                finished_at: row.get(6)?,
                error: row.get(7)?,
            });
        }
        Ok(tasks)
    }

    /// 是否存在运行中的任务（自更新升级前检查：进程重启会丢失内存中的
    /// agent 任务，有运行中任务时拒绝升级）
    pub async fn has_running_tasks(&self) -> Result<bool, turso::Error> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM tasks WHERE status = 'running'").await?;
        let mut rows = stmt.query(()).await?;
        let count: i64 = match rows.next().await? {
            Some(row) => row.get(0)?,
            None => 0,
        };
        Ok(count > 0)
    }
}

fn row_to_suggestion(row: &turso::Row) -> Result<RefinementSuggestion, turso::Error> {
    Ok(RefinementSuggestion {
        id: row.get(0)?,
        session_id: row.get(1)?,
        target: row.get(2)?,
        quote: row.get(3)?,
        suggested: row.get(4)?,
        reason: row.get(5)?,
        status: row.get(6)?,
        created_at: row.get(7)?,
        applied_at: row.get(8)?,
        backup_path: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // 与 config 测试串行：本测试修改全局 BOENMIND_HOME（数据库路径隔离到临时目录）
    use crate::config::TEST_ENV_LOCK;

    #[tokio::test]
    async fn session_message_roundtrip() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let original = std::env::var_os("BOENMIND_HOME");
        let dir = std::env::temp_dir().join(format!("bm-db-{}", std::process::id()));
        unsafe { std::env::set_var("BOENMIND_HOME", &dir) };
        let db = Db::open().await.unwrap();

        // 会话 CRUD
        let s = db.create_session("s1", None, None).await.unwrap();
        assert_eq!(s.title, "新对话");
        assert!(db.get_session("s1").await.unwrap().is_some());
        db.rename_session("s1", "测试标题").await.unwrap();
        assert_eq!(db.get_session("s1").await.unwrap().unwrap().title, "测试标题");
        db.touch_session("s1").await.unwrap();

        // 消息 + 工具调用回放
        db.add_message("s1", "user", "你好").await.unwrap();
        let a = db.add_message("s1", "assistant", "回答").await.unwrap();
        db.add_tool_calls(
            a.id,
            &[
                ("web_search".into(), serde_json::json!({"q": "x"}), false),
                ("bash".into(), serde_json::json!({"cmd": "ls"}), true),
            ],
        )
        .await
        .unwrap();
        let msgs = db.list_messages("s1").await.unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].tool_calls.len(), 2);
        assert_eq!(msgs[1].tool_calls[0].tool_name, "web_search");
        assert_eq!(msgs[1].tool_calls[0].seq, 0);
        assert!(msgs[1].tool_calls[1].is_error);

        // 删除会话级联删消息与工具调用
        db.delete_session("s1").await.unwrap();
        assert!(db.list_messages("s1").await.unwrap().is_empty());
        assert!(db.list_sessions().await.unwrap().is_empty());

        match original {
            Some(v) => unsafe { std::env::set_var("BOENMIND_HOME", v) },
            None => unsafe { std::env::remove_var("BOENMIND_HOME") },
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn has_running_tasks_detects_active_and_finished() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let original = std::env::var_os("BOENMIND_HOME");
        let dir = std::env::temp_dir().join(format!("bm-db-tasks-{}", std::process::id()));
        unsafe { std::env::set_var("BOENMIND_HOME", &dir) };
        let db = Db::open().await.unwrap();

        // 无任务 → false
        assert!(!db.has_running_tasks().await.unwrap());

        // running 任务 → true（自更新升级前检查：有运行中任务拒绝升级）
        db.create_task("t1", "s1").await.unwrap();
        assert!(db.has_running_tasks().await.unwrap());

        // 任务结束后（completed）→ false
        db.finish_task("t1", "completed", None).await.unwrap();
        assert!(!db.has_running_tasks().await.unwrap());

        match original {
            Some(v) => unsafe { std::env::set_var("BOENMIND_HOME", v) },
            None => unsafe { std::env::remove_var("BOENMIND_HOME") },
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
