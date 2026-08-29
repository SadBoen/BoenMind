//! Event Bus(内存版):持久事实源在 M2 才落盘,本版只维护进程内事件日志与
//! 分发。event_seq 由唯一写者(核心循环)分配,从 1 起严格递增、无空洞(INV-3)。

use bm_contract::events::EventEnvelope;
use tokio::sync::broadcast;

/// 事件通道容量:测试/单机场景足够;积压即背压。
const BROADCAST_CAPACITY: usize = 4096;

#[derive(Debug)]
pub struct EventBus {
    log: Vec<EventEnvelope>,
    tx: broadcast::Sender<EventEnvelope>,
    /// seq 分配器。无持久层时与 log.len()+1 等价;有持久层时启动恢复后
    /// resync_to(日志末尾+1),保证跨重启 seq 连续(INV-3)。
    next_seq_counter: u64,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            log: Vec::new(),
            tx,
            next_seq_counter: 1,
        }
    }

    /// 恢复路径:把 seq 分配器重同步到持久日志末尾之后(仅启动阶段调用)。
    pub fn resync_to(&mut self, next_seq: u64) {
        self.next_seq_counter = self.next_seq_counter.max(next_seq);
    }

    /// 追加一条事件(seq 已由调用方分配)。返回全局序号。
    pub fn append(&mut self, event: EventEnvelope) -> u64 {
        let seq = event.event_seq;
        self.next_seq_counter = self.next_seq_counter.max(seq + 1);
        self.log.push(event.clone());
        let _ = self.tx.send(event);
        seq
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq_counter
    }

    pub fn last_seq(&self) -> u64 {
        self.next_seq_counter.saturating_sub(1)
    }

    pub fn events(&self) -> &[EventEnvelope] {
        &self.log
    }

    /// 某会话相关、seq > since 的事件(至多 limit 条),及是否还有更多。
    pub fn poll(
        &self,
        session_id: &bm_contract::ids::BmId,
        since: u64,
        limit: u32,
    ) -> (Vec<EventEnvelope>, u64, bool) {
        let mut events: Vec<EventEnvelope> = self
            .log
            .iter()
            .filter(|e| e.event_seq > since && e.session_id.as_ref() == Some(session_id))
            .cloned()
            .collect();
        let last_seq = self.last_seq();
        let has_more = events.len() > limit as usize;
        events.truncate(limit as usize);
        (events, last_seq, has_more)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.tx.subscribe()
    }
}

/// 乱序/重复投递不改变按 seq 排序后的投影(INV-3 的投影半句)。
/// 投影形态:type 多重计数表。M2 的事件回放复用同一约定。
pub fn project_by_seq(events: &[EventEnvelope]) -> Vec<(u64, bm_contract::events::EventType)> {
    let mut seen = std::collections::BTreeMap::new();
    for e in events {
        seen.insert(e.event_seq, e.event_type); // 同 seq 重复投递幂等
    }
    seen.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_contract::events::EventType;
    use bm_contract::ids::{BmId, IdGen, SeqIdGen};
    use serde_json::json;

    fn ev(seq: u64, ty: EventType) -> EventEnvelope {
        EventEnvelope::new_unchecked(
            seq,
            ty,
            bm_contract::timestamp::now(),
            None,
            None,
            None,
            json!({}),
        )
    }

    #[test]
    fn seq_allocation_is_contiguous() {
        let mut bus = EventBus::new();
        for i in 1..=5 {
            let seq = bus.append(ev(i, EventType::RuntimeStarted));
            assert_eq!(seq, i);
        }
        assert_eq!(bus.next_seq(), 6);
    }

    #[test]
    fn duplicate_and_out_of_order_delivery_do_not_change_projection() {
        let e1 = ev(1, EventType::RuntimeStarted);
        let e2 = ev(2, EventType::SessionCreated);
        let e3 = ev(3, EventType::AgentCreated);

        let ordered = project_by_seq(&[e1.clone(), e2.clone(), e3.clone()]);
        let scrambled = project_by_seq(&[e3.clone(), e1.clone(), e2.clone(), e2, e1]);
        assert_eq!(ordered, scrambled);
    }

    #[test]
    fn poll_filters_by_session_and_since() {
        let mut bus = EventBus::new();
        let ids = SeqIdGen::new();
        let sess: BmId = ids.next_id("sess");
        bus.append(ev(1, EventType::RuntimeStarted));
        bus.append(EventEnvelope::new_unchecked(
            2,
            EventType::SessionCreated,
            bm_contract::timestamp::now(),
            Some(sess.clone()),
            None,
            None,
            json!({"session_id": sess.as_str(), "agent_id": "agent_00000000000000000000000004"}),
        ));
        bus.append(EventEnvelope::new_unchecked(
            3,
            EventType::SessionClosed,
            bm_contract::timestamp::now(),
            Some(sess.clone()),
            None,
            None,
            json!({"session_id": sess.as_str(), "reason": "user_request"}),
        ));

        let (events, last, more) = bus.poll(&sess, 1, 1);
        assert_eq!(events.len(), 1);
        assert!(more);
        assert_eq!(last, 3);
    }
}

#[cfg(test)]
mod t7_dispatch_tests {
    use super::*;

    /// 降级 A(T7 规格 §5.7):分发层暂停(订阅者全部离开)不阻塞核心循环
    /// 的追加;状态提交(event_seq 分配与 log)照常,补发由 resume cursor 承担。
    #[test]
    fn append_succeeds_without_any_subscriber() {
        let mut bus = EventBus::new();
        let rx = bus.subscribe();
        drop(rx); // 订阅者离开(慢消费者极端形态)
        let e = EventEnvelope::new_unchecked(
            1,
            bm_contract::events::EventType::RuntimeStarted,
            bm_contract::timestamp::now(),
            None,
            None,
            None,
            serde_json::json!({}),
        );
        let seq = bus.append(e);
        assert_eq!(seq, 1, "核心追加不受分发层影响");
        assert_eq!(bus.next_seq(), 2);
    }
}
