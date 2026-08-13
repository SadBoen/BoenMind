//! 事件日志原语：内核只承诺语义，不承诺存储。
//!
//! - [`EventLog`]：append/replay/derive_messages/fork 语义层（组装信封、
//!   校验、投影入口），存储走 [`EventStorePort`]；
//! - [`InMemoryEventStore`]：Port 的内存实现（单写者 Mutex，测试/无
//!   持久化场景）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bm_protocol::{
    BranchHead, BranchId, EventQuery, EventStorePort, EventKind, ProtocolError, SeqNo, SessionEvent,
    SessionId,
};
use tokio::sync::Mutex;

use crate::projection::{Projection, SurfaceMessage, SurfaceProjection};
use crate::validation::EventValidator;

/// 事件的消息面意图：None = 不参与消息面；Append = 追加；
/// Replace = 压缩遮蔽区间 [start, end]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceIntent {
    None,
    Append,
    Replace { start: u64, end: u64 },
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 事件日志语义层。
pub struct EventLog {
    store: Arc<dyn EventStorePort>,
}

impl EventLog {
    pub fn new(store: Arc<dyn EventStorePort>) -> Self {
        Self { store }
    }

    /// 追加单条事件（原子；seq 由存储层分配并覆写信封）。
    pub async fn append(
        &self,
        session_id: SessionId,
        branch_id: BranchId,
        kind: EventKind,
        surface: SurfaceIntent,
    ) -> Result<SeqNo, ProtocolError> {
        self.append_with(session_id, branch_id, kind, surface, false, None)
            .await
    }

    /// 追加（完整参数：ignorable/source_seqs 由上层显式给）。
    pub async fn append_with(
        &self,
        session_id: SessionId,
        branch_id: BranchId,
        kind: EventKind,
        surface: SurfaceIntent,
        ignorable: bool,
        source_seqs: Option<Vec<u64>>,
    ) -> Result<SeqNo, ProtocolError> {
        let surface_op = match surface {
            SurfaceIntent::None => None,
            SurfaceIntent::Append => Some(bm_protocol::SurfaceOp::Append),
            SurfaceIntent::Replace { start, end } => {
                let max_seen = self
                    .store
                    .head_seq(&session_id, &branch_id)
                    .await?
                    .map(|s| s.as_u64())
                    .unwrap_or(0);
                EventValidator::check_replace_interval(start, end, max_seen)?;
                Some(bm_protocol::SurfaceOp::Replace { start, end })
            }
        };
        let ev = SessionEvent {
            seq: SeqNo::new(0), // 占位：存储层分配后覆写
            session_id,
            branch_id,
            time: now_ms(),
            kind,
            ignorable,
            surface_op,
            source_seqs,
        };
        self.store.append(ev).await
    }

    /// 原子批量追加（seq 连续分配，失败整体不落）。
    pub async fn append_batch(
        &self,
        session_id: SessionId,
        branch_id: BranchId,
        events: Vec<(EventKind, SurfaceIntent, bool, Option<Vec<u64>>)>,
    ) -> Result<Vec<SeqNo>, ProtocolError> {
        let mut evs = Vec::with_capacity(events.len());
        for (kind, surface, ignorable, source_seqs) in events {
            let surface_op = match surface {
                SurfaceIntent::None => None,
                SurfaceIntent::Append => Some(bm_protocol::SurfaceOp::Append),
                SurfaceIntent::Replace { start, end } => {
                    Some(bm_protocol::SurfaceOp::Replace { start, end })
                }
            };
            evs.push(SessionEvent {
                seq: SeqNo::new(0),
                session_id: session_id.clone(),
                branch_id: branch_id.clone(),
                time: now_ms(),
                kind,
                ignorable,
                surface_op,
                source_seqs,
            });
        }
        self.store.append_batch(evs).await
    }

    /// 重放：读取分支全部事件并做防御性 seq 校验。
    pub async fn replay(
        &self,
        session_id: &SessionId,
        branch_id: &BranchId,
    ) -> Result<Vec<SessionEvent>, ProtocolError> {
        let evs = self
            .store
            .read(EventQuery::new(session_id.clone(), branch_id.clone()))
            .await?;
        EventValidator::verify_replay(&evs)?;
        Ok(evs)
    }
    /// 重放并重建消息面（用户/助手/工具排序视图）。
    pub async fn derive_messages(
        &self,
        session_id: &SessionId,
        branch_id: &BranchId,
    ) -> Result<Vec<SurfaceMessage>, ProtocolError> {
        let evs = self.replay(session_id, branch_id).await?;
        let mut proj = SurfaceProjection::new();
        for ev in &evs {
            proj.on_event(ev)?;
        }
        Ok(proj.into_messages())
    }

    /// 分支当前头 seq（无事件为 None）。
    pub async fn head_seq(
        &self,
        session_id: &SessionId,
        branch_id: &BranchId,
    ) -> Result<Option<SeqNo>, ProtocolError> {
        self.store.head_seq(session_id, branch_id).await
    }

    /// fork 新分支（三维寻址）：`br_<hex>`，parent 记录在分支头。
    /// 新分支为空（seq 从 1 起），replay 读分支自身事件。
    pub async fn fork(&self, session_id: &SessionId, from: &BranchId) -> Result<BranchId, ProtocolError> {
        // 源分支必须存在（超头拒绝：不存在的分支不能 fork）
        self.store.head_seq(session_id, from).await?.ok_or_else(|| {
            ProtocolError::new(
                bm_protocol::ErrorCode::ForkConflict,
                format!("cannot fork from unknown branch `{from}`"),
            )
        })?;
        let new = BranchId::new(format!(
            "br_{:08x}",
            (now_ms() as u64) & 0xffff_ffff ^ 0x5a17_7b0e
        ));
        self.store.fork_branch(session_id, from, &new).await?;
        Ok(new)
    }

    /// 会话全部分支头。
    pub async fn branch_heads(&self, session_id: &SessionId) -> Result<Vec<BranchHead>, ProtocolError> {
        self.store.branch_heads(session_id).await
    }
}

// ---------------------------------------------------------------------------
// 内存存储（Port 实现）
// ---------------------------------------------------------------------------

struct Inner {
    /// 全局追加序（含所有分支；查询时按 (session, branch) 过滤）
    events: Vec<SessionEvent>,
    /// (session_id, branch_id) -> head seq
    heads: HashMap<(String, String), u64>,
    /// 分支头元数据（fork parent）
    heads_meta: HashMap<(String, String), Option<String>>,
}

/// EventStorePort 内存实现：单写者 Mutex，seq 连续分配（原子）。
pub struct InMemoryEventStore {
    inner: Mutex<Inner>,
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                events: Vec::new(),
                heads: HashMap::new(),
                heads_meta: HashMap::new(),
            }),
        }
    }
}

impl EventStorePort for InMemoryEventStore {
    fn append(&self, ev: SessionEvent) -> bm_protocol::BoxFuture<'_, Result<SeqNo, ProtocolError>> {
        Box::pin(async move {
            let mut inner = self.inner.lock().await;
            let key = (ev.session_id.to_string(), ev.branch_id.to_string());
            let head = inner.heads.get(&key).copied();
            let next = head.map(|h| h + 1).unwrap_or(1);
            EventValidator::check_next_seq(head, next)?;
            let mut ev = ev;
            ev.seq = SeqNo::new(next);
            EventValidator::verify_lossless(&ev)?;
            inner.events.push(ev);
            inner.heads.insert(key, next);
            Ok(SeqNo::new(next))
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
            let mut inner = self.inner.lock().await;
            let key = (evs[0].session_id.to_string(), evs[0].branch_id.to_string());
            // 批量要求同 (session, branch)
            for ev in &evs {
                if (ev.session_id.to_string(), ev.branch_id.to_string()) != key {
                    return Err(ProtocolError::new(
                        bm_protocol::ErrorCode::InvalidArgument,
                        "append_batch requires same (session_id, branch_id)",
                    ));
                }
            }
            let head = inner.heads.get(&key).copied();
            let mut next = head.map(|h| h + 1).unwrap_or(1);
            let mut assigned = Vec::with_capacity(evs.len());
            let mut stored = Vec::with_capacity(evs.len());
            for mut ev in evs {
                EventValidator::check_next_seq(Some(next - 1), next)?;
                ev.seq = SeqNo::new(next);
                EventValidator::verify_lossless(&ev)?;
                stored.push(ev);
                assigned.push(SeqNo::new(next));
                next += 1;
            }
            let last = next - 1;
            inner.events.append(&mut stored);
            inner.heads.insert(key, last);
            Ok(assigned)
        })
    }

    fn read(&self, q: EventQuery) -> bm_protocol::BoxFuture<'_, Result<Vec<SessionEvent>, ProtocolError>> {
        Box::pin(async move {
            let inner = self.inner.lock().await;
            let sid = q.session_id.to_string();
            let bid = q.branch_id.to_string();
            let mut out: Vec<SessionEvent> = inner
                .events
                .iter()
                .filter(|ev| ev.session_id.to_string() == sid && ev.branch_id.to_string() == bid)
                .filter(|ev| q.seq_gt.is_none_or(|lo| ev.seq.as_u64() > lo))
                .filter(|ev| q.seq_lte.is_none_or(|hi| ev.seq.as_u64() <= hi))
                .cloned()
                .collect();
            if let Some(limit) = q.limit {
                out.truncate(limit as usize);
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
            let inner = self.inner.lock().await;
            Ok(inner
                .heads
                .get(&(sid.to_string(), bid.to_string()))
                .map(|h| SeqNo::new(*h)))
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
            let mut inner = self.inner.lock().await;
            let key = (sid.to_string(), new.to_string());
            if inner.heads.contains_key(&key) {
                return Err(ProtocolError::new(
                    bm_protocol::ErrorCode::ForkConflict,
                    format!("branch `{new}` already exists"),
                ));
            }
            if !inner.heads.contains_key(&(sid.to_string(), from.to_string())) {
                return Err(ProtocolError::new(
                    bm_protocol::ErrorCode::ForkConflict,
                    format!("cannot fork from unknown branch `{from}`"),
                ));
            }
            inner.heads.insert(key.clone(), 0);
            inner
                .heads_meta
                .insert(key, Some(from.to_string()));
            Ok(())
        })
    }

    fn branch_heads(
        &self,
        sid: &SessionId,
    ) -> bm_protocol::BoxFuture<'_, Result<Vec<BranchHead>, ProtocolError>> {
        let sid = sid.clone();
        Box::pin(async move {
            let inner = self.inner.lock().await;
            let sid_str = sid.to_string();
            let mut out = Vec::new();
            for ((s, b), head) in &inner.heads {
                if s == &sid_str {
                    out.push(BranchHead {
                        session_id: SessionId::new(s.clone()),
                        branch_id: BranchId::new(b.clone()),
                        parent_branch: inner.heads_meta.get(&(s.clone(), b.clone())).cloned().flatten(),
                        head_seq: SeqNo::new(*head),
                    });
                }
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_protocol::{CoreEvent, ErrorCode};

    fn turn(turn: u32) -> EventKind {
        EventKind::Core(CoreEvent::TurnStart { turn })
    }

    fn sid() -> SessionId {
        SessionId::new("sess_test")
    }

    fn main_branch() -> BranchId {
        BranchId::new("main")
    }

    #[tokio::test]
    async fn append_assigns_consecutive_seq() {
        let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
        let s1 = log.append(sid(), main_branch(), turn(1), SurfaceIntent::None).await.unwrap();
        let s2 = log.append(sid(), main_branch(), turn(2), SurfaceIntent::None).await.unwrap();
        assert_eq!(s1.as_u64(), 1);
        assert_eq!(s2.as_u64(), 2);
    }

    #[tokio::test]
    async fn per_branch_seq_independent() {
        let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
        log.append(sid(), main_branch(), turn(1), SurfaceIntent::None).await.unwrap();
        let br = log.fork(&sid(), &main_branch()).await.unwrap();
        let s1 = log.append(sid(), br.clone(), turn(1), SurfaceIntent::None).await.unwrap();
        assert_eq!(s1.as_u64(), 1); // 新分支 seq 从 1 起
        // main 分支不受影响
        let s2 = log.append(sid(), main_branch(), turn(2), SurfaceIntent::None).await.unwrap();
        assert_eq!(s2.as_u64(), 2);
    }

    #[tokio::test]
    async fn append_batch_atomic_and_consecutive() {
        let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
        let evs = vec![
            (turn(1), SurfaceIntent::None, false, None),
            (turn(2), SurfaceIntent::None, false, None),
        ];
        let seqs = log.append_batch(sid(), main_branch(), evs).await.unwrap();
        assert_eq!(seqs, vec![SeqNo::new(1), SeqNo::new(2)]);
    }

    #[tokio::test]
    async fn replay_returns_events_in_order() {
        let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
        log.append(sid(), main_branch(), turn(1), SurfaceIntent::None).await.unwrap();
        log.append(sid(), main_branch(), turn(2), SurfaceIntent::None).await.unwrap();
        let evs = log.replay(&sid(), &main_branch()).await.unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].seq.as_u64(), 1);
        assert_eq!(evs[1].seq.as_u64(), 2);
    }

    #[tokio::test]
    async fn fork_from_unknown_branch_rejected() {
        let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
        let err = log.fork(&sid(), &BranchId::new("br_nope")).await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::ForkConflict);
    }

    #[tokio::test]
    async fn fork_duplicate_rejected() {
        let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
        log.append(sid(), main_branch(), turn(1), SurfaceIntent::None).await.unwrap();
        let br = log.fork(&sid(), &main_branch()).await.unwrap();
        let err = log
            .store
            .fork_branch(&sid(), &main_branch(), &br)
            .await
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::ForkConflict);
    }

    #[tokio::test]
    async fn branch_heads_lists_all() {
        let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
        log.append(sid(), main_branch(), turn(1), SurfaceIntent::None).await.unwrap();
        let br = log.fork(&sid(), &main_branch()).await.unwrap();
        log.append(sid(), br.clone(), turn(1), SurfaceIntent::None).await.unwrap();
        let heads = log.branch_heads(&sid()).await.unwrap();
        assert_eq!(heads.len(), 2);
        let main = heads.iter().find(|h| h.branch_id.to_string() == "main").unwrap();
        assert_eq!(main.head_seq.as_u64(), 1);
        assert_eq!(main.parent_branch, None);
        let brh = heads.iter().find(|h| h.branch_id == br).unwrap();
        assert_eq!(brh.parent_branch.as_deref(), Some("main"));
        assert_eq!(brh.head_seq.as_u64(), 1);
    }
}
