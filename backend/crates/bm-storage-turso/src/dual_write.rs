//! 阶段 0 双写过渡：现有落库（sessions/messages/tool_calls）与
//! 事件日志（event_log）并行运行。
//!
//! 验收目标（实现方案 §0）：bm-server 聊天与事件日志双写运行；
//! 30 轮对话事件流重放两次字节一致。
//!
//! 容错策略（阶段 0）：事件日志写失败**不阻断**主链路——记数后
//! 由上层决定告警；主链路数据始终是权威。事件日志是渐进式吸收的
//! 新家，不是闸门。

use bm_kernel::EventLog;
use bm_protocol::{EventKind, ProtocolError, SessionId};

/// 双写器：对事件日志的追加 + 成败计数（验证双写运行用）。
pub struct DualWriter {
    log: EventLog,
    /// C1 超期清除需要 turso 直连（sessions 表子查询）；内存实现无此维护面
    turso: Option<std::sync::Arc<crate::TursoEventStore>>,
    ok: std::sync::atomic::AtomicU64,
    failed: std::sync::atomic::AtomicU64,
}

impl DualWriter {
    pub fn new(log: EventLog) -> Self {
        Self {
            log,
            turso: None,
            ok: std::sync::atomic::AtomicU64::new(0),
            failed: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// turso 存储形态（bm-server 双写路径）：挂上具体存储以便 C1 超期清除。
    pub fn with_turso(
        log: EventLog,
        turso: std::sync::Arc<crate::TursoEventStore>,
    ) -> Self {
        let mut w = Self::new(log);
        w.turso = Some(turso);
        w
    }

    /// 底层日志句柄（turn 计数 / 重放校验用）。
    pub fn event_log(&self) -> &EventLog {
        &self.log
    }

    /// 底层存储句柄（A5 事件流订阅用）。
    pub fn event_store(&self) -> std::sync::Arc<dyn bm_protocol::EventStorePort> {
        self.log.store()
    }

    /// 清空会话事件日志（回收站 C2 用户主动清除）。返回删除的事件行数；
    /// messages 表不动，事件日志从 seq 1 重新记录。
    pub async fn clear_session(
        &self,
        session_id: SessionId,
    ) -> Result<u64, ProtocolError> {
        self.log.clear_session(&session_id).await
    }

    /// 回收站 C1 超期自动清除：孤儿会话（sessions 表已删）超期事件物理删除。
    /// 仅 turso 形态支持（内存实现无 sessions 表，恒 Ok(0)）。
    pub async fn purge_orphaned_events(&self, before_ms: i64) -> Result<u64, ProtocolError> {
        match &self.turso {
            Some(s) => s.purge_orphaned_events(before_ms).await,
            None => Ok(0),
        }
    }

    pub fn ok_count(&self) -> u64 {
        self.ok.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn failed_count(&self) -> u64 {
        self.failed.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 双写一条事件；失败记数并返回 Err（调用方决定是否告警，不抛）。
    pub async fn append(
        &self,
        session_id: SessionId,
        kind: EventKind,
        surface: bm_kernel::SurfaceIntent,
    ) -> Result<bm_protocol::SeqNo, ProtocolError> {
        match self
            .log
            .append(session_id, bm_protocol::BranchId::new("main"), kind, surface)
            .await
        {
            Ok(seq) => {
                self.ok.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(seq)
            }
            Err(e) => {
                self.failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Err(e)
            }
        }
    }

    /// 原子批量双写（seq 连续分配；失败整体不落并记数）。
    pub async fn append_batch(
        &self,
        session_id: SessionId,
        events: Vec<(EventKind, bm_kernel::SurfaceIntent, bool, Option<Vec<bm_protocol::SeqNo>>)>,
    ) -> Result<Vec<bm_protocol::SeqNo>, ProtocolError> {
        match self
            .log
            .append_batch(session_id, bm_protocol::BranchId::new("main"), events)
            .await
        {
            Ok(seqs) => {
                self.ok
                    .fetch_add(seqs.len() as u64, std::sync::atomic::Ordering::Relaxed);
                Ok(seqs)
            }
            Err(e) => {
                self.failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Err(e)
            }
        }
    }

    /// 以"不阻断主链路"语义双写（失败仅记数）。
    pub async fn append_best_effort(
        &self,
        session_id: SessionId,
        kind: EventKind,
        surface: bm_kernel::SurfaceIntent,
    ) {
        let _ = self.append(session_id, kind, surface).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_protocol::BranchId;

    #[tokio::test]
    async fn dual_writer_counts_success_and_failure() {
        let log = EventLog::new(std::sync::Arc::new(bm_kernel::InMemoryEventStore::new()));
        let w = DualWriter::new(log);
        let sid = SessionId::new("sess_dw");
        w.append_best_effort(
            sid.clone(),
            EventKind::Core(bm_protocol::CoreEvent::TurnStart { turn: 1 }),
            bm_kernel::SurfaceIntent::None,
        )
        .await;
        // 失败场景：从不存在的主机写？内存实现不会失败——直接验证计数正确性
        assert_eq!(w.ok_count(), 1);
        assert_eq!(w.failed_count(), 0);

        // 分支维度验证（双写默认 main 分支）
        let head = w
            .log
            .head_seq(&sid, &BranchId::new("main"))
            .await
            .unwrap();
        assert_eq!(head.unwrap().as_u64(), 1);
    }
}
