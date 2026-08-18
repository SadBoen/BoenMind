//! 进程内事件总线（观察者模式，同步分发）。
//!
//! 仅承载进程内事件（三层事件中的"进程内"层，不上 wire，内部自由）。
//! 观察者 panic 被吞掉并忽略，不影响主链路（bobleer 同款纪律）。

use std::sync::Arc;

use parking_lot::RwLock;

use crate::session::SessionRecord;

/// 注销句柄：drop 即注销。
#[must_use = "dropping the disposer unregisters the listener"]
pub struct Disposer(Arc<dyn Fn() + Send + Sync>);

impl Drop for Disposer {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// 事件观察者签名。
pub type EventListener = Arc<dyn Fn(&SessionRecord) + Send + Sync>;

/// 简单事件总线：只做 emit 分发，不做 waterfall 短路（waterfall 由 loop 内部实现）。
#[derive(Default)]
pub struct EventBus {
    slots: Arc<RwLock<Vec<(u64, EventListener)>>>,
    /// 监听器 id 计数器。克隆共享同一计数器（clone 只复制 slots 引用，
    /// 计数器独立会发出重复 id → Disposer 误删对方监听器）。
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册观察者，返回注销句柄。
    pub fn on_event<F>(&self, listener: F) -> Disposer
    where
        F: Fn(&SessionRecord) + Send + Sync + 'static,
    {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let listener: EventListener = Arc::new(listener);
        self.slots.write().push((id, listener));
        let slots = Arc::clone(&self.slots);
        Disposer(Arc::new(move || {
            slots.write().retain(|(sid, _)| *sid != id);
        }))
    }

    /// 分发一条事件记录到全部观察者。
    pub fn emit(&self, record: &SessionRecord) {
        let slots = self.slots.read().clone();
        for (_, listener) in &slots {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                listener(record)
            }));
        }
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            slots: Arc::clone(&self.slots),
            next_id: Arc::clone(&self.next_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_shares_listener_ids() {
        // 回归 BUG-003：克隆共享 next_id，两个 clone 注册的监听器 id 不重复，
        // drop 一个 Disposer 不误删对方监听器。
        let bus = EventBus::new();
        let bus2 = bus.clone();
        let fired = Arc::new(std::sync::Mutex::new(Vec::new()));
        let fired1 = Arc::clone(&fired);
        let d1 = bus.on_event(move |_| fired1.lock().unwrap().push("d1"));
        let d2 = bus2.on_event(|_| {});
        drop(d2);
        let header = crate::session::SessionHeader {
            id: crate::session::SessionId("s".into()),
            app: "t".into(),
            profile: "t".into(),
            workspace: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        bus.emit(&SessionRecord::new(
            1,
            header.id.clone(),
            crate::session::SessionEvent::SessionStarted { header },
        ));
        // d2 已注销；d1 仍在且被触发一次。
        assert_eq!(fired.lock().unwrap().as_slice(), &["d1"]);
        drop(d1);
    }
}
