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
  forked_at    INTEGER,
  PRIMARY KEY (session_id, branch_id)
);
"#;

/// A3 增量迁移：老库补 branch_heads.forked_at 列（fork 时父 head 快照）。
/// sqlite 无 ADD COLUMN IF NOT EXISTS，用 pragma_table_info 探测后补列。
pub const MIGRATE_FORKED_AT: &str = r#"
ALTER TABLE branch_heads ADD COLUMN forked_at INTEGER;
"#;

pub struct TursoEventStore {
    conn: Mutex<Connection>,
}

impl TursoEventStore {
    /// 打开（必要时建表 + 自愈分支头）。与 bm-core 同款：新 local DB + 单连接。
    pub async fn open(path: &str) -> Result<Self, ProtocolError> {
        if let Some(dir) = std::path::Path::new(path).parent() {
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
        // 多实例共享同一 db 文件时（桌面壳 + standalone 同时跑，2026-08-14
        // 真实验收实测）：写锁争用由 SQLite 内部等待，避免 append 直接
        // "database is locked" 丢事件（审计链断裂）。
        conn.pragma_update("busy_timeout", 5000)
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("busy_timeout: {e}")))?;
        conn.execute_batch(MIGRATE_EVENT_LOG)
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("migrate: {e}")))?;
        // A3 增量迁移：老库补 branch_heads.forked_at 列
        if !has_column(&conn, "branch_heads", "forked_at").await? {
            conn.execute_batch(MIGRATE_FORKED_AT)
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("migrate forked_at: {e}")))?;
        }
        let store = Self {
            conn: Mutex::new(conn),
        };
        // 启动自愈：head 与 max(seq) 重新对齐（防 append 两步间崩溃留下的落后 head）
        store.repair_heads().await?;
        Ok(store)
    }

    /// 由已打开的连接构造（bm-server 双写场景可复用同一连接）。
    pub fn from_connection(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// 自愈分支头：head_seq = 各 (session, branch) 的 max(seq)（无事件为 0）；
    /// 补齐有事件但缺头行的分支。崩溃窗口（INSERT 与 upsert_head 之间）
    /// 造成的落后 head 由此修复。
    pub async fn repair_heads(&self) -> Result<(), ProtocolError> {
        let conn = self.conn.lock().await;
        conn.execute_batch(
            "INSERT OR IGNORE INTO branch_heads (session_id, branch_id, parent_branch, head_seq)
             SELECT session_id, branch_id, NULL, MAX(seq) FROM event_log GROUP BY session_id, branch_id;
             UPDATE branch_heads SET head_seq = COALESCE(
               (SELECT MAX(seq) FROM event_log e
                 WHERE e.session_id = branch_heads.session_id AND e.branch_id = branch_heads.branch_id), 0);",
        )
        .await
        .map(|_| ())
        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("repair heads: {e}")))
    }

    /// 未闭合回合查询（A4）：每个 (session, branch) 里，最后一条 turn/start
    /// 之后没有 turn/end 的回合（崩溃时 TurnEnd 尾事件未落的场景）。
    /// 返回 (session, branch, turn)——turn 从信封 JSON 顶层解析（kind 已 flatten）。
    pub async fn unclosed_turns(&self) -> Result<Vec<(SessionId, BranchId, u32)>, ProtocolError> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT e.session_id, e.branch_id, e.data FROM event_log e
                 WHERE e.type = 'turn/start'
                   AND e.seq = (SELECT MAX(seq) FROM event_log x
                                WHERE x.session_id = e.session_id
                                  AND x.branch_id = e.branch_id
                                  AND x.type = 'turn/start')
                   AND NOT EXISTS (SELECT 1 FROM event_log y
                                   WHERE y.session_id = e.session_id
                                     AND y.branch_id = e.branch_id
                                     AND y.type = 'turn/end'
                                     AND y.seq > e.seq)",
            )
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare unclosed: {e}")))?;
        let mut rows = stmt
            .query(())
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query unclosed: {e}")))?;
        let mut out = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next unclosed: {e}")))?
        {
            let sid: String = row
                .get(0)
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get sid: {e}")))?;
            let bid: String = row
                .get(1)
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get bid: {e}")))?;
            let data: String = row
                .get(2)
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get data: {e}")))?;
            match serde_json::from_str::<serde_json::Value>(&data)
                .ok()
                .and_then(|v| v.get("turn").and_then(serde_json::Value::as_u64))
            {
                Some(t) => out.push((SessionId::new(sid), BranchId::new(bid), t as u32)),
                None => tracing::warn!(event = "bm.unclosed_turn_parse_failed", session = %sid),
            }
        }
        Ok(out)
    }

    fn insert_sql() -> &'static str {
        "INSERT INTO event_log (seq, session_id, branch_id, time, type, data, ignorable, surface_op, source_seqs)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
    }
}

/// 探测表是否存在某列（A3 增量迁移用；sqlite 无 ADD COLUMN IF NOT EXISTS）。
async fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, ProtocolError> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn
        .prepare(&sql)
        .await
        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare table_info: {e}")))?;
    let mut rows = stmt
        .query(())
        .await
        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query table_info: {e}")))?;
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next table_info: {e}")))?
    {
        let name: String = row
            .get(1)
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get column name: {e}")))?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A4 启动恢复：给"有 TurnStart 无 TurnEnd"的回合补写
/// `TurnEnd { reason: Interrupted }`（dsh 语义——崩溃遗留的回合显式闭合）。
/// 幂等（已闭合的回合不再命中 unclosed_turns）。返回补写条数。
pub async fn recover_interrupted_turns(
    store: &TursoEventStore,
    log: &bm_kernel::EventLog,
) -> Result<u64, ProtocolError> {
    let unclosed = store.unclosed_turns().await?;
    let mut n = 0u64;
    for (sid, bid, turn) in unclosed {
        log.append(
            sid,
            bid,
            bm_protocol::EventKind::Core(bm_protocol::CoreEvent::TurnEnd {
                turn,
                reason: bm_protocol::TurnEndReason::Interrupted,
            }),
            bm_kernel::SurfaceIntent::None,
        )
        .await?;
        n += 1;
    }
    Ok(n)
}

/// 行参数形态（row_params 返回类型）。
type RowParams<'a> = (    i64,
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
    /// （data 由主查询直接选出，避免逐行 N+1 重查）
    fn parse_data(seq: i64, data: &str, ignorable: bool) -> Result<Option<SessionEvent>, ProtocolError> {
        match serde_json::from_str::<SessionEvent>(data) {
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
            // 分支头（锁内读 → 分配 → 事务内插入+更新头，原子）
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
            let next = head.map(|h| h + 1).unwrap_or(1);
            let mut ev = ev;
            ev.seq = SeqNo::new(next as u64);
            let params = row_params(&ev);
            // 单条 append 与 batch 同语义：INSERT + 头更新同事务
            // （两步间崩溃会留落后 head，启动 repair_heads 自愈；
            // 事务内原子则根本不产生该窗口）
            conn.execute("BEGIN", ())
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("begin: {e}")))?;
            let res: Result<(), ProtocolError> = async {
                conn.execute(Self::insert_sql(), params)
                    .await
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("insert: {e}")))?;
                upsert_head(&conn, sid.as_str(), bid.as_str(), next).await
            }
            .await;
            match res {
                Ok(()) => {
                    conn.execute("COMMIT", ())
                        .await
                        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("commit: {e}")))?;
                    Ok(SeqNo::new(next as u64))
                }
                Err(e) => {
                    let _ = conn.execute("ROLLBACK", ()).await;
                    Err(e)
                }
            }
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
                let last = next - 1;
                // 头更新在事务内（与 INSERT 原子，崩溃不留落后 head）
                upsert_head(&conn, sid.as_str(), bid.as_str(), last).await
            }
            .await;
            match result {
                Ok(()) => {
                    conn.execute("COMMIT", ())
                        .await
                        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("commit: {e}")))?;
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
            // 动态 WHERE（seq 范围/事件类型按需拼接；type 列过滤让长会话
            // 投影只读某类事件，替代全量重放）。参数一律占位符绑定，防注入。
            let mut sql = String::from(
                "SELECT seq, session_id, branch_id, ignorable, data FROM event_log
                 WHERE session_id = ?1 AND branch_id = ?2",
            );
            let mut params: Vec<turso::Value> =
                vec![turso::Value::Text(sid.clone()), turso::Value::Text(bid.clone())];
            let mut ph = 2;
            if let Some(lo) = q.seq_gt {
                ph += 1;
                sql.push_str(&format!(" AND seq > ?{ph}"));
                params.push(turso::Value::Integer(lo as i64));
            }
            if let Some(hi) = q.seq_lte {
                ph += 1;
                sql.push_str(&format!(" AND seq <= ?{ph}"));
                params.push(turso::Value::Integer(hi as i64));
            }
            if let Some(ty) = &q.event_type {
                ph += 1;
                sql.push_str(&format!(" AND type = ?{ph}"));
                params.push(turso::Value::Text(ty.clone()));
            }
            sql.push_str(if q.limit.is_some() { " ORDER BY seq LIMIT ?" } else { " ORDER BY seq" });
            if let Some(lim) = q.limit {
                params.push(turso::Value::Integer(lim as i64));
            }
            let mut stmt = conn
                .prepare(&sql)
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare read: {e}")))?;
            let mut rows = stmt
                .query(turso::params_from_iter(params))
                .await
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
                let ignorable: i64 = row
                    .get(3)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get ignorable: {e}")))?;
                let data: String = row
                    .get(4)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get data: {e}")))?;
                if let Some(ev) = parse_data(seq, &data, ignorable != 0)? {
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

    fn count(
        &self,
        sid: &SessionId,
        bid: &BranchId,
        event_type: Option<&str>,
    ) -> bm_protocol::BoxFuture<'_, Result<u64, ProtocolError>> {
        let sid = sid.clone();
        let bid = bid.clone();
        let event_type = event_type.map(str::to_string);
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let (sql, has_type) = match &event_type {
                Some(_) => (
                    "SELECT COUNT(*) FROM event_log
                     WHERE session_id = ?1 AND branch_id = ?2 AND type = ?3",
                    true,
                ),
                None => (
                    "SELECT COUNT(*) FROM event_log WHERE session_id = ?1 AND branch_id = ?2",
                    false,
                ),
            };
            let mut stmt = conn
                .prepare(sql)
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare count: {e}")))?;
            let mut rows = if has_type {
                stmt.query((sid.as_str(), bid.as_str(), event_type.as_deref().unwrap_or("")))
                    .await
            } else {
                stmt.query((sid.as_str(), bid.as_str())).await
            }
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query count: {e}")))?;
            let total: i64 = if let Some(row) = rows
                .next()
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next count: {e}")))?
            {
                row.get(0).map_err(|e| {
                    ProtocolError::new(ErrorCode::StoreUnavailable, format!("get count: {e}"))
                })?
            } else {
                0
            };
            Ok(total as u64)
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
            // 源分支必须存在（超头拒绝），并取 fork 点快照（A3）
            let mut stmt = conn
                .prepare("SELECT head_seq FROM branch_heads WHERE session_id = ?1 AND branch_id = ?2")
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("prepare from: {e}")))?;
            let mut rows = stmt
                .query((sid.as_str(), from.as_str()))
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("query from: {e}")))?;
            let Some(from_row) = rows
                .next()
                .await
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("next from: {e}")))?
            else {
                return Err(ProtocolError::new(
                    ErrorCode::ForkConflict,
                    format!("cannot fork from unknown branch `{from}`"),
                ));
            };
            let fork_at: i64 = from_row
                .get(0)
                .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get fork head: {e}")))?;
            conn.execute(
                "INSERT INTO branch_heads (session_id, branch_id, parent_branch, head_seq, forked_at)
                 VALUES (?1, ?2, ?3, 0, ?4)",
                (sid.as_str(), new.as_str(), from.as_str(), fork_at),
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
                .prepare("SELECT session_id, branch_id, parent_branch, head_seq, forked_at FROM branch_heads WHERE session_id = ?1")
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
                let forked_at: Option<i64> = row
                    .get(4)
                    .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("get: {e}")))?;
                out.push(BranchHead {
                    session_id: SessionId::new(session_id),
                    branch_id: BranchId::new(branch_id),
                    parent_branch: parent_branch.map(BranchId::new),
                    head_seq: SeqNo::new(head_seq as u64),
                    forked_at: forked_at.map(|v| v as u64),
                });
            }
            Ok(out)
        })
    }

    fn clear_session(
        &self,
        sid: &SessionId,
    ) -> bm_protocol::BoxFuture<'_, Result<u64, ProtocolError>> {
        let sid = sid.clone();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            // 事件与分支头同一事务清空（回收站 C2：用户主动清除）——两条
            // DELETE 各自 autocommit 会留孤儿头窗口（回看 P1）
            conn.execute("BEGIN", ()).await.map_err(|e| {
                ProtocolError::new(ErrorCode::StoreUnavailable, format!("clear begin: {e}"))
            })?;
            let removed = match conn
                .execute(
                    "DELETE FROM event_log WHERE session_id = ?1",
                    [sid.as_str()],
                )
                .await
            {
                Ok(n) => n,
                Err(e) => {
                    // 出错回滚：必须 await 真正执行（此前 let _ 直接丢弃 future，
                    // ROLLBACK 从未发出——clippy let_underscore_future 实爆暴露）
                    let _ = conn.execute("ROLLBACK", ()).await;
                    return Err(ProtocolError::new(
                        ErrorCode::StoreUnavailable,
                        format!("clear events: {e}"),
                    ));
                }
            };
            if let Err(e) = conn
                .execute(
                    "DELETE FROM branch_heads WHERE session_id = ?1",
                    [sid.as_str()],
                )
                .await
            {
                let _ = conn.execute("ROLLBACK", ()).await;
                return Err(ProtocolError::new(
                    ErrorCode::StoreUnavailable,
                    format!("clear heads: {e}"),
                ));
            }
            if let Err(e) = conn.execute("COMMIT", ()).await {
                let _ = conn.execute("ROLLBACK", ()).await;
                return Err(ProtocolError::new(
                    ErrorCode::StoreUnavailable,
                    format!("clear commit: {e}"),
                ));
            }
            Ok(removed as u64)
        })
    }
}

impl TursoEventStore {
    /// C1 回收站超期清除：删除「孤儿会话」的超期事件——
    /// sessions 表已无此行（用户已删会话）且事件 time < before_ms。
    /// 同库约定：sessions 表为 bm-core 所有，这里只读引用其 id 列。
    /// 顺带清理事件已空的孤儿分支头。返回删除的事件行数。
    pub async fn purge_orphaned_events(&self, before_ms: i64) -> Result<u64, ProtocolError> {
        let conn = self.conn.lock().await;
        let removed = conn
            .execute(
                "DELETE FROM event_log
                 WHERE time < ?1
                   AND session_id NOT IN (SELECT id FROM sessions)",
                [before_ms],
            )
            .await
            .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("purge orphans: {e}")))?;
        conn.execute(
            "DELETE FROM branch_heads
             WHERE session_id NOT IN (SELECT id FROM sessions)
               AND session_id NOT IN (SELECT DISTINCT session_id FROM event_log)",
            (),
        )
        .await
        .map_err(|e| ProtocolError::new(ErrorCode::StoreUnavailable, format!("purge orphan heads: {e}")))?;
        Ok(removed as u64)
    }
}

/// 便捷构造：以 Arc<dyn EventStorePort> 形态打开 turso 存储。
pub async fn open_event_store(path: &str) -> Result<Arc<dyn EventStorePort>, ProtocolError> {
    Ok(Arc::new(TursoEventStore::open(path).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_kernel::{EventLog, SurfaceIntent};
    use bm_protocol::{CoreEvent, EventKind};

    fn turn(t: u32) -> EventKind {
        EventKind::Core(CoreEvent::TurnStart { turn: t })
    }

    fn todo_write() -> EventKind {
        EventKind::Core(CoreEvent::TodoWrite { todos: vec![] })
    }

    #[tokio::test]
    async fn read_filters_by_event_type() {
        let store = TursoEventStore::open(":memory:").await.unwrap();
        let log = EventLog::new(Arc::new(store));
        let sid = SessionId::new("s1");
        let bid = BranchId::new("main");
        // 混合事件流：turn 标记与 todo 快照交错（长会话真实形态）
        log.append_batch(
            sid.clone(),
            bid.clone(),
            vec![
                (turn(1), SurfaceIntent::None, false, None),
                (todo_write(), SurfaceIntent::None, false, None),
                (turn(2), SurfaceIntent::None, false, None),
                (todo_write(), SurfaceIntent::None, false, None),
            ],
        )
        .await
        .unwrap();
        // 类型过滤：只回 todo/write（SQL 层过滤，不读全量）
        let only = log
            .read_where(EventQuery::of_type(sid.clone(), bid.clone(), "todo/write"))
            .await
            .unwrap();
        assert_eq!(only.len(), 2);
        assert!(only.iter().all(|e| e.kind.name() == "todo/write"));
        // 不过滤 = 全量（seq 升序）
        let all = log
            .read_where(EventQuery::new(sid, bid))
            .await
            .unwrap();
        assert_eq!(all.len(), 4);
    }
}
