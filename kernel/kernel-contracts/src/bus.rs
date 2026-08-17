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
    next_id: std::sync::atomic::AtomicU64,
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
            next_id: std::sync::atomic::AtomicU64::new(
                self.next_id.load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}
