//! EventStorePort 的 turso 实现：单写者 Mutex + 显式 seq 分配。
//!
//! **seq 分配**（实现方案 Schema 的实现期修正）：不用 AUTOINCREMENT——
//! 全局计数与"分支内 seq 连续"矛盾（跨分支事件会打洞），且事务回滚
//! 后 AUTOINCREMENT 不回用号码。改为：读分支 head → head+1 → 显式
//! INSERT，`UNIQUE (session_id, branch_id, seq)` 兜底；全部在单写者锁
//! 内完成，保证原子。
//!
//! **未知事件**（D2）：`data` 列存完整信封 JSON；read 时反序列化失败
//! → 按 `ignorable` 列跳过或拒绝重建（`unknown_required_event`）。

use std::sync::Arc;

use bm_protocol::{
    BranchHead, BranchId, EventQuery, EventStorePort, ErrorCode, ProtocolError, SeqNo,
    SessionEvent, SessionId,
};
use tokio::sync::Mutex;
use turso::{Builder, Connection};

/// 事件日志表结构（boenmind.db 新增，双写过渡核心表）。
pub const MIGRATE_EVENT_LOG: &str = r#"
CREATE TABLE IF NOT EXISTS event_log (
  seq          INTEGER NOT NULL,
  session_id   TEXT NOT NULL,
  branch_id    TEXT NOT NULL DEFAULT 'main',
  time         INTEGER NOT NULL,
  type         TEXT NOT NULL,
  data         TEXT NOT NULL,
  ignorable    INTEGER NOT NULL DEFAULT 0,
  surface_op   TEXT,
  source_seqs  TEXT,
  UNIQUE (session_id, branch_id, seq)
);
CREATE INDEX IF NOT EXISTS idx_event_log_lookup
  ON event_log (session_id, branch_id, seq DESC);
CREATE TABLE IF NOT EXISTS branch_heads (
  session_id   TEXT NOT NULL,
  branch_id    TEXT NOT NULL,
  parent_branch TEXT,
  head_seq     INTEGER NOT NULL,
  PRIMARY KEY (session_id, branch_id)
);
"#;

pub struct TursoEventStore {
    conn: Mutex<Connection>,
}

impl TursoEventStore {
    /// 打开（必要时建表）。与 bm-core 同款：新 local DB + 单连接。
    pub async fn open(path: &str) -> Result<Self, ProtocolError> {        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let db = Builder::new_local(path)
            .build()
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("open {path}: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("connect {path}: {e}")))?;
        // WAL + 每事务提交即 fsync（checkpoint 策略的"请求边界 fsync"）
        conn.pragma_update("journal_mode", "WAL")
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("journal_mode: {e}")))?;
        conn.pragma_update("synchronous", "FULL")
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("synchronous: {e}")))?;
        conn.execute_batch(MIGRATE_EVENT_LOG)
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("migrate: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 由已打开的连接构造（bm-server 双写场景可复用同一连接）。
    pub fn from_connection(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    fn insert_sql() -> &'static str {
        "INSERT INTO event_log (seq, session_id, branch_id, time, type, data, ignorable, surface_op, source_seqs)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
    }
}

/// 行参数形态（row_params 返回类型）。
type RowParams<'a> = (
    i64,
    &'a str,
    &'a str,
    i64,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
);

/// 信封 → 行参数。data = 完整信封 JSON（lossless）。
fn row_params(ev: &SessionEvent) -> RowParams<'_> {
    let data = serde_json::to_string(ev).expect("envelope serializable");
    let surface_op = match &ev.surface_op {
        None => None,
        Some(bm_protocol::SurfaceOp::Append) => Some("append".to_string()),
        Some(bm_protocol::SurfaceOp::Replace { start, end }) => Some(format!("replace:{start}:{end}")),
    };
    let source_seqs = ev
        .source_seqs
        .as_ref()
        .map(|v| serde_json::to_string(v).expect("seqs serializable"));
    (
        ev.seq.as_u64() as i64,
        ev.session_id.as_str(),
        ev.branch_id.as_str(),
        ev.time,
        ev.kind.name(),
            data,
            ev.ignorable as i64,
            surface_op,
            source_seqs,
        )
    }

    /// 行 → 信封。`data` 解析失败 = 未知事件 → 按 ignorable 守卫处置。
    async fn row_to_event(conn: &Connection, seq: i64, session_id: String, branch_id: String, ignorable: bool)
        -> Result<Option<SessionEvent>, ProtocolError>
    {
        let mut stmt = conn
            .prepare("SELECT data FROM event_log WHERE session_id = ?1 AND branch_id = ?2 AND seq = ?3")
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("read data: {e}")))?;
        let mut rows = stmt
            .query((session_id.as_str(), branch_id.as_str(), seq))
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query data: {e}")))?;
        let Some(row) = rows.next().await.map_err(|e| {
            ProtocolError::new(ErrorCode::StoreUnavailable, format!("next data: {e}"))
        })?
        else {
            return Ok(None);
        };
        let data: String = row
            .get(0)
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get data: {e}")))?;
        match serde_json::from_str::<SessionEvent>(&data) {
            Ok(ev) => Ok(Some(ev)),
            Err(_) => {
                // 未知事件守卫（D2）：ignorable → 跳过；必需 → 拒绝重建
                if ignorable {
                    Ok(None)
                } else {
                    Err(ProtocolError::new(
                        ErrorCode::UnknownRequiredEvent,
                        format!("event seq {seq} type unknown and ignorable=false"),
                    ))
                }
            }
        }
    }

    /// append 后同步分支头（main 首插，fork 分支保留 parent）。
    async fn upsert_head(
        conn: &Connection,
        session_id: &str,
        branch_id: &str,
        head: i64,
    ) -> Result<(), ProtocolError> {
        conn.execute(
            "INSERT INTO branch_heads (session_id, branch_id, parent_branch, head_seq)
             VALUES (?1, ?2, NULL, ?3)
             ON CONFLICT(session_id, branch_id) DO UPDATE SET head_seq = excluded.head_seq",
            (session_id, branch_id, head),
        )
        .await
        .map(|_| ())
        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("upsert head: {e}")))
}

impl EventStorePort for TursoEventStore {
    fn append(&self, ev: SessionEvent) -> bm_protocol::BoxFuture<'_, Result<SeqNo, ProtocolError>> {
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let sid = ev.session_id.clone();
            let bid = ev.branch_id.clone();
            // 分支头（锁内读 → 分配 → 插入，原子）
            let mut stmt = conn
                .prepare("SELECT head_seq FROM branch_heads WHERE session_id = ?1 AND branch_id = ?2")
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare head: {e}")))?;
            let mut rows = stmt
                .query((sid.as_str(), bid.as_str()))
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query head: {e}")))?;
            let head: Option<i64> = if let Some(row) = rows
                .next()
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next head: {e}")))?
            {
                Some(row.get(0).map_err(|e| {
                    ProtocolError::new(ErrorCode::StoreUnavailable, format!("get head: {e}"))
                })?)
            } else {
                None
            };
            let next = head.map(|h| h + 1).unwrap_or(1);
            if head == Some(next) {
                return Err(ProtocolError::new(ErrorCode::SeqDuplicate, format!("seq {next} already exists")));
            }
            let mut ev = ev;
            ev.seq = SeqNo::new(next as u64);
            let params = row_params(&ev);
            conn.execute(Self::insert_sql(), params)
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("insert: {e}")))?;
            upsert_head(&conn, sid.as_str(), bid.as_str(), next).await?;
            Ok(SeqNo::new(next as u64))
        })
    }

    fn append_batch(
        &self,
        evs: Vec<SessionEvent>,
    ) -> bm_protocol::BoxFuture<'_, Result<Vec<SeqNo>, ProtocolError>> {
        Box::pin(async move {
            if evs.is_empty() {
                return Ok(Vec::new());
            }
            let conn = self.conn.lock().await;
            // 批量同 (session, branch)
            let sid = evs[0].session_id.clone();
            let bid = evs[0].branch_id.clone();
            for ev in &evs {
                if ev.session_id != sid || ev.branch_id != bid {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidArgument,
                        "append_batch requires same (session_id, branch_id)",
                    ));
                }
            }
            // 分支头
            let mut stmt = conn
                .prepare("SELECT head_seq FROM branch_heads WHERE session_id = ?1 AND branch_id = ?2")
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare head: {e}")))?;
            let mut rows = stmt
                .query((sid.as_str(), bid.as_str()))
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query head: {e}")))?;
            let head: Option<i64> = if let Some(row) = rows
                .next()
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next head: {e}")))?
            {
                Some(row.get(0).map_err(|e| {
                    ProtocolError::new(ErrorCode::StoreUnavailable, format!("get head: {e}"))
                })?)
            } else {
                None
            };
            drop(stmt);
            drop(rows);

            conn.execute("BEGIN", ())
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("begin: {e}")))?;
            let mut next = head.map(|h| h + 1).unwrap_or(1);
            let mut assigned = Vec::with_capacity(evs.len());
            let result = async {
                for mut ev in evs {
                    ev.seq = SeqNo::new(next as u64);
                    let params = row_params(&ev);
                    conn.execute(Self::insert_sql(), params).await.map_err(|e| {
                        ProtocolError::new(ErrorCode::StoreUnavailable, format!("insert: {e}"))
                    })?;
                    assigned.push(SeqNo::new(next as u64));
                    next += 1;
                }
                Ok::<(), ProtocolError>(())
            }
            .await;
            match result {
                Ok(()) => {
                    let last = next - 1;
                    conn.execute("COMMIT", ())
                        .await
                        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("commit: {e}")))?;
                    upsert_head(&conn, sid.as_str(), bid.as_str(), last).await?;
                    Ok(assigned)
                }
                Err(e) => {
                    let _ = conn.execute("ROLLBACK", ()).await;
                    Err(e)
                }
            }
        })
    }

    fn read(&self, q: EventQuery) -> bm_protocol::BoxFuture<'_, Result<Vec<SessionEvent>, ProtocolError>> {
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let sid = q.session_id.to_string();
            let bid = q.branch_id.to_string();
            let sql = match (q.seq_gt, q.seq_lte, q.limit) {
                (Some(_), Some(_), Some(_)) => {
                    "SELECT seq, session_id, branch_id, ignorable FROM event_log
                     WHERE session_id = ?1 AND branch_id = ?2 AND seq > ?3 AND seq <= ?4
                     ORDER BY seq LIMIT ?5"
                }
                (Some(_), Some(_), None) => {
                    "SELECT seq, session_id, branch_id, ignorable FROM event_log
                     WHERE session_id = ?1 AND branch_id = ?2 AND seq > ?3 AND seq <= ?4
                     ORDER BY seq"
                }
                (Some(_), None, Some(_)) => {
                    "SELECT seq, session_id, branch_id, ignorable FROM event_log
                     WHERE session_id = ?1 AND branch_id = ?2 AND seq > ?3
                     ORDER BY seq LIMIT ?4"
                }
                (Some(_), None, None) => {
                    "SELECT seq, session_id, branch_id, ignorable FROM event_log
                     WHERE session_id = ?1 AND branch_id = ?2 AND seq > ?3 ORDER BY seq"
                }
                (None, Some(_), Some(_)) => {
                    "SELECT seq, session_id, branch_id, ignorable FROM event_log
                     WHERE session_id = ?1 AND branch_id = ?2 AND seq <= ?3
                     ORDER BY seq LIMIT ?4"
                }
                (None, Some(_), None) => {
                    "SELECT seq, session_id, branch_id, ignorable FROM event_log
                     WHERE session_id = ?1 AND branch_id = ?2 AND seq <= ?3 ORDER BY seq"
                }
                (None, None, Some(_)) => {
                    "SELECT seq, session_id, branch_id, ignorable FROM event_log
                     WHERE session_id = ?1 AND branch_id = ?2 ORDER BY seq LIMIT ?3"
                }
                (None, None, None) => {
                    "SELECT seq, session_id, branch_id, ignorable FROM event_log
                     WHERE session_id = ?1 AND branch_id = ?2 ORDER BY seq"
                }
            };
            let mut stmt = conn
                .prepare(sql)
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare read: {e}")))?;
            // turso 参数绑定不支持混用 Option 长度，按 sql 形态选择具体绑定
            let mut rows = match (q.seq_gt, q.seq_lte, q.limit) {
                (Some(lo), Some(hi), Some(lim)) => {
                    stmt.query((sid.as_str(), bid.as_str(), lo as i64, hi as i64, lim as i64)).await
                }
                (Some(lo), Some(hi), None) => {
                    stmt.query((sid.as_str(), bid.as_str(), lo as i64, hi as i64)).await
                }
                (Some(lo), None, Some(lim)) => {
                    stmt.query((sid.as_str(), bid.as_str(), lo as i64, lim as i64)).await
                }
                (Some(lo), None, None) => stmt.query((sid.as_str(), bid.as_str(), lo as i64)).await,
                (None, Some(hi), Some(lim)) => {
                    stmt.query((sid.as_str(), bid.as_str(), hi as i64, lim as i64)).await
                }
                (None, Some(hi), None) => stmt.query((sid.as_str(), bid.as_str(), hi as i64)).await,
                (None, None, Some(lim)) => stmt.query((sid.as_str(), bid.as_str(), lim as i64)).await,
                (None, None, None) => stmt.query((sid.as_str(), bid.as_str())).await,
            }
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query read: {e}")))?;

            let mut out = Vec::new();
            while let Some(row) = rows
                .next()
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next read: {e}")))?
            {
                let seq: i64 = row
                    .get(0)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get seq: {e}")))?;
                let session_id: String = row
                    .get(1)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get sid: {e}")))?;
                let branch_id: String = row
                    .get(2)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get bid: {e}")))?;
                let ignorable: i64 = row
                    .get(3)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get ignorable: {e}")))?;
                // 逐行重读 data（小表 + 低频，直接按主键取；保持查询语句简单）
                let ev = row_to_event(&conn, seq, session_id, branch_id, ignorable != 0).await?;
                if let Some(ev) = ev {
                    out.push(ev);
                }
            }
            Ok(out)
        })
    }

    fn head_seq(
        &self,
        sid: &SessionId,
        bid: &BranchId,
    ) -> bm_protocol::BoxFuture<'_, Result<Option<SeqNo>, ProtocolError>> {
        let sid = sid.clone();
        let bid = bid.clone();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut stmt = conn
                .prepare("SELECT head_seq FROM branch_heads WHERE session_id = ?1 AND branch_id = ?2")
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare head: {e}")))?;
            let mut rows = stmt
                .query((sid.as_str(), bid.as_str()))
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query head: {e}")))?;
            if let Some(row) = rows
                .next()
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next head: {e}")))?
            {
                let h: i64 = row
                    .get(0)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get head: {e}")))?;
                Ok(Some(SeqNo::new(h as u64)))
            } else {
                Ok(None)
            }
        })
    }

    fn fork_branch(
        &self,
        sid: &SessionId,
        from: &BranchId,
        new: &BranchId,
    ) -> bm_protocol::BoxFuture<'_, Result<(), ProtocolError>> {
        let sid = sid.clone();
        let from = from.clone();
        let new = new.clone();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            // 重复分支拒绝
            let mut stmt = conn
                .prepare("SELECT 1 FROM branch_heads WHERE session_id = ?1 AND branch_id = ?2")
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare dup: {e}")))?;
            let mut rows = stmt
                .query((sid.as_str(), new.as_str()))
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query dup: {e}")))?;
            if rows
                .next()
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next dup: {e}")))?
                .is_some()
            {
                return Err(ProtocolError::new(
                    ErrorCode::ForkConflict,
                    format!("branch `{new}` already exists"),
                ));
            }
            // 源分支必须存在（超头拒绝）
            let mut stmt = conn
                .prepare("SELECT 1 FROM branch_heads WHERE session_id = ?1 AND branch_id = ?2")
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare from: {e}")))?;
            let mut rows = stmt
                .query((sid.as_str(), from.as_str()))
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query from: {e}")))?;
            if rows
                .next()
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next from: {e}")))?
                .is_none()
            {
                return Err(ProtocolError::new(
                    ErrorCode::ForkConflict,
                    format!("cannot fork from unknown branch `{from}`"),
                ));
            }
            conn.execute(
                "INSERT INTO branch_heads (session_id, branch_id, parent_branch, head_seq)
                 VALUES (?1, ?2, ?3, 0)",
                (sid.as_str(), new.as_str(), from.as_str()),
            )
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("insert branch: {e}")))?;
            Ok(())
        })
    }

    fn branch_heads(
        &self,
        sid: &SessionId,
    ) -> bm_protocol::BoxFuture<'_, Result<Vec<BranchHead>, ProtocolError>> {
        let sid = sid.clone();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut stmt = conn
                .prepare("SELECT session_id, branch_id, parent_branch, head_seq FROM branch_heads WHERE session_id = ?1")
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare heads: {e}")))?;
            let mut rows = stmt
                .query([sid.as_str()])
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query heads: {e}")))?;
            let mut out = Vec::new();
            while let Some(row) = rows
                .next()
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next heads: {e}")))?
            {
                let session_id: String = row
                    .get(0)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get: {e}")))?;
                let branch_id: String = row
                    .get(1)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get: {e}")))?;
                let parent_branch: Option<String> = row
                    .get(2)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get: {e}")))?;
                let head_seq: i64 = row
                    .get(3)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get: {e}")))?;
                out.push(BranchHead {
                    session_id: SessionId::new(session_id),
                    branch_id: BranchId::new(branch_id),
                    parent_branch,
                    head_seq: SeqNo::new(head_seq as u64),
                });
            }
            Ok(out)
        })
    }
}

/// 便捷构造：以 Arc<dyn EventStorePort> 形态打开 turso 存储。
pub async fn open_event_store(path: &str) -> Result<Arc<dyn EventStorePort>, ProtocolError> {
    Ok(Arc::new(TursoEventStore::open(path).await?))
}
