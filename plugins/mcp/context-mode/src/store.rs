use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{path::Path, sync::Mutex};

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; CREATE TABLE IF NOT EXISTS documents(path TEXT PRIMARY KEY, content TEXT NOT NULL, bytes INTEGER NOT NULL, modified_ms INTEGER NOT NULL, indexed_at INTEGER NOT NULL); CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(path UNINDEXED, content); CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS session_events(id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL, seq INTEGER NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, created_at INTEGER NOT NULL, UNIQUE(session_id, seq)); CREATE TABLE IF NOT EXISTS snapshots(session_id TEXT PRIMARY KEY, upto_seq INTEGER NOT NULL, messages_json TEXT NOT NULL, created_at INTEGER NOT NULL);").map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_document(
        &self,
        path: &str,
        content: &str,
        bytes: usize,
        modified_ms: i64,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁失败".to_string())?;
        conn.execute("INSERT INTO documents(path,content,bytes,modified_ms,indexed_at) VALUES(?1,?2,?3,?4,strftime('%s','now')) ON CONFLICT(path) DO UPDATE SET content=excluded.content,bytes=excluded.bytes,modified_ms=excluded.modified_ms,indexed_at=excluded.indexed_at", params![path, content, bytes as i64, modified_ms]).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM documents_fts WHERE path=?1", params![path])
            .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO documents_fts(path,content) VALUES(?1,?2)",
            params![path, content],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁失败".to_string())?;
        let mut stmt = conn.prepare("SELECT path, snippet(documents_fts, 1, '[', ']', '…', 24), bm25(documents_fts) FROM documents_fts WHERE documents_fts MATCH ?1 ORDER BY bm25(documents_fts), path LIMIT ?2").map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![query, limit as i64], |row| Ok(json!({"path":row.get::<_,String>(0)?,"snippet":row.get::<_,String>(1)?,"score":row.get::<_,f64>(2)?,"trust":"untrusted"}))).map_err(|e| e.to_string())?;
        rows.map(|r| r.map_err(|e| e.to_string())).collect()
    }

    pub fn append_event(&self, session: &str, role: &str, content: &str) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁失败".to_string())?;
        let now = now();
        conn.execute("INSERT INTO sessions(id,created_at,updated_at) VALUES(?1,?2,?2) ON CONFLICT(id) DO UPDATE SET updated_at=excluded.updated_at", params![session, now]).map_err(|e| e.to_string())?;
        let seq: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(seq),0)+1 FROM session_events WHERE session_id=?1",
                params![session],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO session_events(session_id,seq,role,content,created_at) VALUES(?1,?2,?3,?4,?5)", params![session,seq,role,content,now]).map_err(|e| e.to_string())?;
        Ok(seq)
    }

    pub fn snapshot(&self, session: &str, max: usize) -> Result<Value, String> {
        let messages = self.events(session)?;
        let start = messages.len().saturating_sub(max);
        let kept: Vec<Value> = messages[start..].to_vec();
        let upto = kept.last().and_then(|v| v["seq"].as_i64()).unwrap_or(0);
        let conn = self.conn.lock().map_err(|_| "数据库锁失败".to_string())?;
        conn.execute("INSERT INTO snapshots(session_id,upto_seq,messages_json,created_at) VALUES(?1,?2,?3,?4) ON CONFLICT(session_id) DO UPDATE SET upto_seq=excluded.upto_seq,messages_json=excluded.messages_json,created_at=excluded.created_at", params![session,upto,serde_json::to_string(&kept).map_err(|e| e.to_string())?,now()]).map_err(|e| e.to_string())?;
        Ok(json!({"upto_seq":upto,"messages":kept}))
    }

    pub fn restore(&self, session: &str) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁失败".to_string())?;
        let snapshot: Option<(i64, String)> = conn
            .query_row(
                "SELECT upto_seq,messages_json FROM snapshots WHERE session_id=?1",
                params![session],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        drop(conn);
        let mut result: Vec<Value> = snapshot
            .as_ref()
            .map(|(_, raw)| serde_json::from_str(raw).unwrap_or_default())
            .unwrap_or_default();
        let after = snapshot.map(|(seq, _)| seq).unwrap_or(0);
        result.extend(self.events_after(session, after)?);
        Ok(result)
    }

    fn events(&self, session: &str) -> Result<Vec<Value>, String> {
        self.events_after(session, 0)
    }
    fn events_after(&self, session: &str, after: i64) -> Result<Vec<Value>, String> {
        let conn = self.conn.lock().map_err(|_| "数据库锁失败".to_string())?;
        let mut stmt = conn.prepare("SELECT seq,role,content,created_at FROM session_events WHERE session_id=?1 AND seq>?2 ORDER BY seq").map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![session,after], |row| Ok(json!({"seq":row.get::<_,i64>(0)?,"role":row.get::<_,String>(1)?,"content":row.get::<_,String>(2)?,"created_at":row.get::<_,i64>(3)?}))).map_err(|e| e.to_string())?;
        rows.map(|r| r.map_err(|e| e.to_string())).collect()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
