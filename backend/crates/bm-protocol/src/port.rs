//! Port traits：内核依赖 Port 而非实现（A2）。
//!
//! 首版只定义 [`EventStorePort`]（阶段 0 必需）；其余 Port
//! （ModelProviderPort/FileSystemPort/…）留待对应阶段按 S9
//! "只注册正在使用的类型"逐个添加——**不建空 trait 占位**
//! （诚实标注 partial，避免 kernel.chat 的宣称与交付脱节）。
//!
//! 签名用 `BoxFuture`（手写）而非 async-trait：保持契约 crate
//! 零额外依赖。

use std::future::Future;
use std::pin::Pin;

use crate::error::ProtocolError;
use crate::event::SessionEvent;
use crate::ids::{BranchId, SeqNo, SessionId};

/// 手写 async fn 签名（等价 async-trait 展开，零依赖）。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 事件读取查询（按 (session, branch, seq 范围)）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventQuery {
    pub session_id: SessionId,
    pub branch_id: BranchId,
    /// 只返回 seq > seq_gt 的事件
    pub seq_gt: Option<u64>,
    /// 只返回 seq <= seq_lte 的事件
    pub seq_lte: Option<u64>,
    /// 返回条数上限（默认不限）
    pub limit: Option<u64>,
}

impl EventQuery {
    pub fn new(session_id: SessionId, branch_id: BranchId) -> Self {
        Self {
            session_id,
            branch_id,
            seq_gt: None,
            seq_lte: None,
            limit: None,
        }
    }
}

/// 分支头（fork/merge 语义，branch_heads 表行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchHead {
    pub session_id: SessionId,
    pub branch_id: BranchId,
    /// fork 来源分支（main 为 None）
    pub parent_branch: Option<BranchId>,
    pub head_seq: SeqNo,
    /// fork 时父分支的 head 快照（A3 父前缀折叠的分叉点；main 为 None）。
    /// 父分支 seq <= forked_at 的事件对子分支可见，分叉后父分支新增不可见。
    pub forked_at: Option<u64>,
}

/// 事件存储端口。实现：内存（bm-kernel InMemoryEventStore）与
/// turso（bm-storage-turso）。**单写者约定**：跨进程不直写日志
/// （走 RPC 代理，首版不承诺多进程写，实现方案 §5-4）。
///
/// 能力矩阵（shipped/partial 诚实标注）：
/// - append / append_batch / read / head_seq：shipped
/// - 事件流订阅：kernel 级 `subscribe_events`（replay-prefix + tail 轮询，
///   A5 已落地；SSE 路由 /api/sessions/{id}/events 消费）——非本端口方法
pub trait EventStorePort: Send + Sync {
    /// 原子 append 单条事件，返回分配的 seq（存储层覆写信封 seq）。
    fn append(&self, ev: SessionEvent) -> BoxFuture<'_, Result<SeqNo, ProtocolError>>;

    /// 原子批量 append（seq 连续分配，失败整体不落）。
    fn append_batch(&self, evs: Vec<SessionEvent>) -> BoxFuture<'_, Result<Vec<SeqNo>, ProtocolError>>;

    /// 按查询读取事件（seq 升序）。
    fn read(&self, q: EventQuery) -> BoxFuture<'_, Result<Vec<SessionEvent>, ProtocolError>>;

    /// 分支当前头 seq（无事件为 None）。
    fn head_seq(&self, sid: &SessionId, bid: &BranchId) -> BoxFuture<'_, Result<Option<SeqNo>, ProtocolError>>;

    /// 按事件类型计数（event_type=None 计全量）。
    /// turn 计数等场景用，避免全量重放 O(n) 读。
    fn count(
        &self,
        sid: &SessionId,
        bid: &BranchId,
        event_type: Option<&str>,
    ) -> BoxFuture<'_, Result<u64, ProtocolError>>;

    /// fork 新分支（记录 parent，超头/重复拒绝）。`new` 由上层生成。
    fn fork_branch(
        &self,
        sid: &SessionId,
        from: &BranchId,
        new: &BranchId,
    ) -> BoxFuture<'_, Result<(), ProtocolError>>;

    /// 列出会话全部分支头。
    fn branch_heads(&self, sid: &SessionId) -> BoxFuture<'_, Result<Vec<BranchHead>, ProtocolError>>;

    /// 清空会话全部事件与分支头（回收站 C2 用户主动清除）。
    /// 返回删除的事件行数；分支头随之重置（下次 append 从 seq 1 重新起）。
    fn clear_session(&self, sid: &SessionId) -> BoxFuture<'_, Result<u64, ProtocolError>>;
}
