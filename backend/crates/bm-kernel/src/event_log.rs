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

    /// 底层存储句柄（订阅/恢复等需要 Arc<dyn EventStorePort> 的场景）。
    pub fn store(&self) -> Arc<dyn EventStorePort> {
        Arc::clone(&self.store)
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
        source_seqs: Option<Vec<bm_protocol::SeqNo>>,
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
            version: bm_protocol::SESSION_FORMAT_VERSION,
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
    /// Replace 遮蔽区间随批内推进校验（允许遮蔽批内已追加的前序事件）。
    pub async fn append_batch(
        &self,
        session_id: SessionId,
        branch_id: BranchId,
        events: Vec<(EventKind, SurfaceIntent, bool, Option<Vec<bm_protocol::SeqNo>>)>,
    ) -> Result<Vec<SeqNo>, ProtocolError> {
        let mut max_seen = self
            .store
            .head_seq(&session_id, &branch_id)
            .await?
            .map(|s| s.as_u64())
            .unwrap_or(0);
        let mut evs = Vec::with_capacity(events.len());
        for (kind, surface, ignorable, source_seqs) in events {
            let surface_op = match surface {
                SurfaceIntent::None => None,
                SurfaceIntent::Append => Some(bm_protocol::SurfaceOp::Append),
                SurfaceIntent::Replace { start, end } => {
                    EventValidator::check_replace_interval(start, end, max_seen)?;
                    Some(bm_protocol::SurfaceOp::Replace { start, end })
                }
            };
            evs.push(SessionEvent {
                version: bm_protocol::SESSION_FORMAT_VERSION,
                seq: SeqNo::new(0),
                session_id: session_id.clone(),
                branch_id: branch_id.clone(),
                time: now_ms(),
                kind,
                ignorable,
                surface_op,
                source_seqs,
            });
            max_seen += 1; // 本事件占一个 seq，批内后续 Replace 可遮蔽到此处
        }
        self.store.append_batch(evs).await
    }

    /// 重放：读取分支全部事件并做防御性校验（格式版本 + seq 严格递增）。
    pub async fn replay(
        &self,
        session_id: &SessionId,
        branch_id: &BranchId,
    ) -> Result<Vec<SessionEvent>, ProtocolError> {
        let evs = self
            .store
            .read(EventQuery::new(session_id.clone(), branch_id.clone()))
            .await?;
        for ev in &evs {
            EventValidator::check_version(ev)?;
        }
        EventValidator::verify_replay(&evs)?;
        Ok(evs)
    }

    /// 按事件类型计数（type=None 计全量）。turn 计数等场景用，避免全量重放。
    pub async fn count(
        &self,
        session_id: &SessionId,
        branch_id: &BranchId,
        event_type: Option<&str>,
    ) -> Result<u64, ProtocolError> {
        self.store.count(session_id, branch_id, event_type).await
    }
    /// 重放并重建消息面（用户/助手/工具排序视图）。
    /// A3：沿 parent_branch 链折叠父前缀（各父链截至 fork 点快照）。
    /// **逐段折叠**：每个分支段用新投影，段间不归并——chunk→message 归并
    /// 按 (turn, step) 匹配是分支内语义，跨分支同 (turn, step) 必须隔离
    /// （否则子分支首条消息会覆盖父前缀末条消息，串味）。
    pub async fn derive_messages(
        &self,
        session_id: &SessionId,
        branch_id: &BranchId,
    ) -> Result<Vec<SurfaceMessage>, ProtocolError> {
        let segments = self.visible_segments(session_id, branch_id).await?;
        let mut out: Vec<SurfaceMessage> = Vec::new();
        for seg in segments {
            let mut proj = SurfaceProjection::new();
            for ev in &seg {
                EventValidator::check_version(ev)?;
                proj.on_event(ev)?;
            }
            out.extend(proj.into_messages());
        }
        Ok(out)
    }

    /// 分支可见事件分段（A3）：`[父链前缀段…, 自身段]`，最旧在前。
    /// 各段是"fork 时刻快照"（父分支 seq <= forked_at），父分支分叉后
    /// 新增的事件对子分支不可见；每段内 seq 为分支内连续编号。
    pub async fn visible_segments(
        &self,
        session_id: &SessionId,
        branch_id: &BranchId,
    ) -> Result<Vec<Vec<SessionEvent>>, ProtocolError> {
        self.segment_prefix(session_id, branch_id, None).await
    }

    /// 递归分段：父链前缀段（截至 upto）→ 自身段（截至 upto；None=全部）。
    async fn segment_prefix(
        &self,
        session_id: &SessionId,
        branch_id: &BranchId,
        upto: Option<u64>,
    ) -> Result<Vec<Vec<SessionEvent>>, ProtocolError> {
        let heads = self.store.branch_heads(session_id).await?;
        let head = heads.iter().find(|h| h.branch_id == *branch_id).ok_or_else(|| {
            ProtocolError::new(
                bm_protocol::ErrorCode::ForkConflict,
                format!("unknown branch `{branch_id}`"),
            )
        })?;
        let mut segs = match &head.parent_branch {
            Some(parent) => {
                let fork_at = head.forked_at.unwrap_or(0);
                Box::pin(self.segment_prefix(session_id, parent, Some(fork_at))).await?
            }
            None => Vec::new(),
        };
        let mut q = EventQuery::new(session_id.clone(), branch_id.clone());
        q.seq_lte = upto;
        segs.push(self.store.read(q).await?);
        Ok(segs)
    }

    /// 分支可见事件（A3，扁平拼接）：自身事件 + 沿 parent_branch 链折叠的父前缀。
    /// main（无父）即自身全部事件。逐条版本检查（折叠流跨分支 seq 各自从 1 起，
    /// 不做整流连续性校验；每段连续性由存储层单写者 + 分支内 append 原子性保证）。
    pub async fn visible_events(
        &self,
        session_id: &SessionId,
        branch_id: &BranchId,
    ) -> Result<Vec<SessionEvent>, ProtocolError> {
        let segments = self.visible_segments(session_id, branch_id).await?;
        let mut evs = Vec::new();
        for seg in segments {
            for ev in &seg {
                EventValidator::check_version(ev)?;
            }
            evs.extend(seg);
        }
        Ok(evs)
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
    /// 分支名 = 时间戳 + 进程内原子计数混合（同毫秒多次 fork 也唯一）。
    pub async fn fork(&self, session_id: &SessionId, from: &BranchId) -> Result<BranchId, ProtocolError> {
        // 源分支必须存在（超头拒绝：不存在的分支不能 fork）
        self.store.head_seq(session_id, from).await?.ok_or_else(|| {
            ProtocolError::new(
                bm_protocol::ErrorCode::ForkConflict,
                format!("cannot fork from unknown branch `{from}`"),
            )
        })?;
        static FORK_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = FORK_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let new = BranchId::new(format!(
            "br_{:08x}{:08x}",
            (now_ms() as u64) & 0xffff_ffff,
            n & 0xffff_ffff
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
// A5 订阅：replay-prefix + tail（SSE 事件流推送的前端投影引擎前置）
// ---------------------------------------------------------------------------

/// 订阅句柄：`stop()` 停轮询（Drop 同效）；`error()` 取后台 tail 阶段的错误
/// （replay-prefix 阶段的错误直接由 subscribe_events 返回）。
pub struct Subscription {
    stop: Arc<std::sync::atomic::AtomicBool>,
    error: Arc<Mutex<Option<ProtocolError>>>,
}

impl Subscription {
    /// 停止订阅（幂等）。正在推送中的一次回调不受影响，轮询循环在下个节拍退出。
    pub fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// tail 阶段的后台错误（只读快照；None = 正常）。
    pub async fn error(&self) -> Option<ProtocolError> {
        self.error.lock().await.clone()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A5 事件流订阅：先推送 `after` 之后的既有事件（replay-prefix），随后
/// 每 250ms 轮询推送新增事件（tail）。
///
/// - prefix 阶段在调用方 await 期间同步推送完（有界重放），随后 spawn tail 轮询；
/// - `on_event` 顺序保证：prefix 全部完成才开始 tail，tail 内按 seq 序；
/// - `stop` 为外部共享停止开关：回调里发送失败（客户端断开）时置位即可退出；
/// - 实现说明：阶段 1 用轮询（事件量小、简单可靠）；A6 自研 loop 落位后
///   换内核事件总线直推（无轮询延迟）。
pub async fn subscribe_events(
    store: Arc<dyn EventStorePort>,
    session_id: SessionId,
    branch_id: BranchId,
    after: Option<u64>,
    on_event: impl Fn(SessionEvent) + Send + 'static,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<Subscription, ProtocolError> {
    // replay-prefix：after 之后的既有事件（含版本校验，读序即 seq 序）
    let mut q = EventQuery::new(session_id.clone(), branch_id.clone());
    q.seq_gt = after;
    let mut last_seen = after.unwrap_or(0);
    for ev in store.read(q).await? {
        EventValidator::check_version(&ev)?;
        last_seen = ev.seq.as_u64().max(last_seen);
        on_event(ev);
    }
    if stop.load(std::sync::atomic::Ordering::Relaxed) {
        // prefix 阶段已被外部停止：不进入 tail
        return Ok(Subscription {
            stop,
            error: Arc::new(Mutex::new(None)),
        });
    }
    let error = Arc::new(Mutex::new(None));
    let error_task = error.clone();
    let stop_task = stop.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            if stop_task.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let mut q = EventQuery::new(session_id.clone(), branch_id.clone());
            q.seq_gt = Some(last_seen);
            match store.read(q).await {
                Ok(evs) => {
                    for ev in evs {
                        if stop_task.load(std::sync::atomic::Ordering::Relaxed) {
                            return;
                        }
                        last_seen = ev.seq.as_u64().max(last_seen);
                        on_event(ev);
                    }
                }
                Err(e) => {
                    *error_task.lock().await = Some(e);
                    return;
                }
            }
        }
    });
    Ok(Subscription { stop, error })
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
    heads_meta: HashMap<(String, String), Option<BranchId>>,
    /// 分支头元数据（fork 时父 head 快照；main 无）
    heads_fork: HashMap<(String, String), u64>,
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
                heads_fork: HashMap::new(),
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
            // fork 点快照：父分支当前 head（A3 父前缀折叠的分叉点）
            let fork_at = inner.heads.get(&(sid.to_string(), from.to_string())).copied();
            inner.heads.insert(key.clone(), 0);
            inner.heads_meta.insert(key.clone(), Some(from.clone()));
            inner
                .heads_fork
                .insert(key, fork_at.unwrap_or(0));
            Ok(())
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
            let inner = self.inner.lock().await;
            let sid_str = sid.to_string();
            let bid_str = bid.to_string();
            Ok(inner
                .events
                .iter()
                .filter(|ev| ev.session_id.to_string() == sid_str && ev.branch_id.to_string() == bid_str)
                .filter(|ev| event_type.as_deref().is_none_or(|t| ev.kind.name() == t))
                .count() as u64)
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
                        forked_at: inner.heads_fork.get(&(s.clone(), b.clone())).copied(),
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
        assert_eq!(brh.parent_branch.as_ref().map(|b| b.as_str()), Some("main"));
        assert_eq!(brh.head_seq.as_u64(), 1);
    }

    #[tokio::test]
    async fn derive_messages_folds_parent_prefix_at_fork_point() {
        // A3：fork 分支的消息面 = 父前缀（截至 fork 点快照）+ 自身事件；
        // 父分支分叉后新增的事件对子分支不可见
        use bm_protocol::{AssistantMsg, UserMsg, UserMsgSource};
        let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
        let s = sid();
        let main = main_branch();
        let user = |content: &str| {
            EventKind::Core(CoreEvent::UserMessage {
                msg: UserMsg { content: content.into() },
                source: UserMsgSource::Human,
            })
        };
        let assistant = |content: &str| {
            EventKind::Core(CoreEvent::AssistantMessage {
                turn: 1,
                step: 1,
                msg: AssistantMsg { content: content.into() },
                usage: None,
            })
        };
        log.append(s.clone(), main.clone(), user("u1"), SurfaceIntent::Append).await.unwrap();
        log.append(s.clone(), main.clone(), assistant("a1"), SurfaceIntent::Append).await.unwrap();
        let br = log.fork(&s, &main).await.unwrap();
        // 分叉后主分支继续走
        log.append(s.clone(), main.clone(), user("u2"), SurfaceIntent::Append).await.unwrap();
        log.append(s.clone(), main.clone(), assistant("a2"), SurfaceIntent::Append).await.unwrap();
        // 子分支写自己的事件
        log.append(s.clone(), br.clone(), user("b1"), SurfaceIntent::Append).await.unwrap();

        let main_msgs = log.derive_messages(&s, &main).await.unwrap();
        assert_eq!(main_msgs.len(), 4, "main 全量");

        let br_msgs = log.derive_messages(&s, &br).await.unwrap();
        assert_eq!(br_msgs.len(), 3, "子分支 = 父前缀 u1/a1 + 自身 b1，不含 u2/a2");
        assert_eq!(br_msgs[0].content, "u1");
        assert_eq!(br_msgs[1].content, "a1");
        assert_eq!(br_msgs[2].content, "b1");
    }

    #[tokio::test]
    async fn grandchild_branch_folds_two_levels() {
        // A3：孙分支沿 parent_branch 链折叠两级父前缀
        use bm_protocol::{AssistantMsg, UserMsg, UserMsgSource};
        let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
        let s = sid();
        let main = main_branch();
        let user = |content: &str| {
            EventKind::Core(CoreEvent::UserMessage {
                msg: UserMsg { content: content.into() },
                source: UserMsgSource::Human,
            })
        };
        let assistant = |content: &str| {
            EventKind::Core(CoreEvent::AssistantMessage {
                turn: 1,
                step: 1,
                msg: AssistantMsg { content: content.into() },
                usage: None,
            })
        };
        log.append(s.clone(), main.clone(), user("u1"), SurfaceIntent::Append).await.unwrap();
        let a = log.fork(&s, &main).await.unwrap();
        log.append(s.clone(), a.clone(), assistant("a1"), SurfaceIntent::Append).await.unwrap();
        let b = log.fork(&s, &a).await.unwrap();
        log.append(s.clone(), b.clone(), assistant("b1"), SurfaceIntent::Append).await.unwrap();
        // 中间分支分叉后继续走（对孙分支不可见）
        log.append(s.clone(), a.clone(), assistant("a2"), SurfaceIntent::Append).await.unwrap();

        let b_msgs = log.derive_messages(&s, &b).await.unwrap();
        assert_eq!(b_msgs.len(), 3, "孙分支 = main[u1] + A[a1] + 自身[b1]");
        assert_eq!(b_msgs[0].content, "u1");
        assert_eq!(b_msgs[1].content, "a1");
        assert_eq!(b_msgs[2].content, "b1");
    }

    #[tokio::test]
    async fn subscribe_replays_prefix_then_tails_new_events() {
        // A5：replay-prefix（after 之后既有事件）+ tail（后续 append 推送）
        use std::sync::atomic::AtomicBool;
        let store: Arc<dyn EventStorePort> = Arc::new(InMemoryEventStore::new());
        let log = EventLog::new(store.clone());
        let s = sid();
        let b = main_branch();
        log.append(s.clone(), b.clone(), turn(1), SurfaceIntent::None).await.unwrap();
        log.append(s.clone(), b.clone(), turn(2), SurfaceIntent::None).await.unwrap();

        let got = Arc::new(std::sync::Mutex::new(Vec::new()));
        let got_cb = got.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let sub = subscribe_events(
            store,
            s.clone(),
            b.clone(),
            Some(1),
            move |ev| got_cb.lock().unwrap().push(ev.seq.as_u64()),
            stop.clone(),
        )
        .await
        .unwrap();
        // prefix：seq 2 已推送；seq 1 被 after=1 过滤
        assert_eq!(*got.lock().unwrap(), vec![2]);

        // tail：新 append 的 seq 3 轮询后推送
        log.append(s.clone(), b.clone(), turn(3), SurfaceIntent::None).await.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let cur = got.lock().unwrap().clone();
            if cur.contains(&3) {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "tail 未在 3s 内推送 seq 3");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(*got.lock().unwrap(), vec![2, 3], "顺序 = prefix 后 tail，无重复");
        sub.stop();
    }

    #[tokio::test]
    async fn subscribe_after_none_delivers_everything_in_order() {
        use std::sync::atomic::AtomicBool;
        let store: Arc<dyn EventStorePort> = Arc::new(InMemoryEventStore::new());
        let log = EventLog::new(store.clone());
        let s = sid();
        let b = main_branch();
        for i in 1..=5u32 {
            log.append(s.clone(), b.clone(), turn(i), SurfaceIntent::None).await.unwrap();
        }
        let got = Arc::new(std::sync::Mutex::new(Vec::new()));
        let got_cb = got.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let sub = subscribe_events(
            store,
            s.clone(),
            b.clone(),
            None,
            move |ev| got_cb.lock().unwrap().push(ev.seq.as_u64()),
            stop.clone(),
        )
        .await
        .unwrap();
        assert_eq!(*got.lock().unwrap(), vec![1, 2, 3, 4, 5], "after=None 全量 prefix 按序");
        assert!(sub.error().await.is_none());
        sub.stop();
    }
}
