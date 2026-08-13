//! 事件总线：四种分发模式。
//!
//! - `on` / `emit`：同步观察（emit 不 await，按注册序调用）；
//! - `waterfall`：环绕中间件（短路 = 不调 next，直接返回结果）；
//! - `parallel`：异步处理器并发扇出（JoinSet）；
//! - `serial`：异步处理器按序执行（结果收集，不传递）。
//!
//! 匹配键 = 事件类型名（[`EventKind::name`]：Core → "turn/start"，
//! Custom → "app.wiki.indexed"）。emit 先取快照再调用，避免持锁回调。

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use bm_protocol::{BoxFuture, EventKind};
use serde_json::Value as JsonValue;

use crate::Disposer;

/// 同步观察者。
pub type SyncHandler = Box<dyn Fn(&bm_protocol::EventKind) + Send + Sync>;

/// 环绕中间件返回值：Continue 继续链，ShortCircuit 短路（结果定案）。
pub enum WaterfallOutcome {
    Continue,
    ShortCircuit(JsonValue),
}

/// 异步处理器 trait（parallel/serial 用；BoxFuture 手写签名）。
pub trait AsyncHandlerTrait: Send + Sync {
    fn call(&self, ev: Arc<bm_protocol::EventKind>, args: JsonValue) -> BoxFuture<'static, JsonValue>;
}

pub type AsyncHandler = Arc<dyn AsyncHandlerTrait>;

struct SyncEntry {
    id: usize,
    name: &'static str,
    handler: Arc<SyncHandler>,
}

/// 环绕中间件函数（waterfall 链）：返回 Continue 或短路。
pub type WaterfallFn = dyn Fn(&EventKind, &mut JsonValue) -> WaterfallOutcome + Send + Sync;

struct WaterfallEntry {
    id: usize,
    name: &'static str,
    handler: Arc<WaterfallFn>,
}

struct AsyncEntry {
    id: usize,
    name: &'static str,
    handler: AsyncHandler,
}

#[derive(Default)]
struct Inner {
    sync: RwLock<Vec<SyncEntry>>,
    waterfall: RwLock<Vec<WaterfallEntry>>,
    async_: RwLock<Vec<AsyncEntry>>,
    next_id: AtomicUsize,
}

#[derive(Clone, Default)]
pub struct EventBus {
    inner: Arc<Inner>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// 同步订阅（emit 时按注册序观察）。返回 Disposer，drop 即退订。
    pub fn on(&self, name: &'static str, h: SyncHandler) -> Disposer {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .sync
            .write()
            .expect("bus poisoned")
            .push(SyncEntry { id, name, handler: Arc::new(h) });
        let inner = self.inner.clone();
        Disposer::new(move || {
            inner.sync.write().expect("bus poisoned").retain(|e| e.id != id);
        })
    }

    /// 环绕中间件（waterfall 链）。短路 = 返回 ShortCircuit，不再调后续。
    pub fn around(
        &self,
        name: &'static str,
        h: impl Fn(&bm_protocol::EventKind, &mut JsonValue) -> WaterfallOutcome + Send + Sync + 'static,
    ) -> Disposer {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .waterfall
            .write()
            .expect("bus poisoned")
            .push(WaterfallEntry { id, name, handler: Arc::new(h) });
        let inner = self.inner.clone();
        Disposer::new(move || {
            inner
                .waterfall
                .write()
                .expect("bus poisoned")
                .retain(|e| e.id != id);
        })
    }

    /// 异步处理器注册（parallel/serial 用）。
    pub fn on_async(&self, name: &'static str, h: AsyncHandler) -> Disposer {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .async_
            .write()
            .expect("bus poisoned")
            .push(AsyncEntry { id, name, handler: h });
        let inner = self.inner.clone();
        Disposer::new(move || {
            inner
                .async_
                .write()
                .expect("bus poisoned")
                .retain(|e| e.id != id);
        })
    }

    /// 发事件：不 await，按注册序同步观察（快照后调用，防持锁回调）。
    pub fn emit(&self, ev: &bm_protocol::EventKind) {
        let name = ev.name();
        let snapshot: Vec<Arc<SyncHandler>> = self
            .inner
            .sync
            .read()
            .expect("bus poisoned")
            .iter()
            .filter(|e| e.name == name)
            .map(|e| e.handler.clone())
            .collect();
        for h in snapshot {
            h(ev);
        }
    }

    /// 环绕中间件链：依次调用匹配的 handler；全部 Continue 且无 handler
    /// 时返回 `default()`；ShortCircuit 即短路返回。
    pub fn waterfall<F>(
        &self,
        name: &'static str,
        ev: &bm_protocol::EventKind,
        mut args: JsonValue,
        default: F,
    ) -> JsonValue
    where
        F: FnOnce() -> JsonValue,
    {
        let snapshot: Vec<Arc<WaterfallFn>> = self
            .inner
            .waterfall
            .read()
            .expect("bus poisoned")
            .iter()
            .filter(|e| e.name == name)
            .map(|e| e.handler.clone())
            .collect();
        for h in snapshot {
            match h(ev, &mut args) {
                WaterfallOutcome::Continue => {}
                WaterfallOutcome::ShortCircuit(v) => return v,
            }
        }
        default()
    }

    /// 并发扇出：匹配的异步处理器 spawn 并行执行，收集结果（Vec 与
    /// 注册序对应；失败的 task 记 Err 由调用方定夺）。
    pub fn parallel(&self, name: &'static str, ev: EventKind, args: JsonValue) -> impl Future<Output = Vec<JsonValue>> {
        let snapshot: Vec<AsyncHandler> = self
            .inner
            .async_
            .read()
            .expect("bus poisoned")
            .iter()
            .filter(|e| e.name == name)
            .map(|e| e.handler.clone())
            .collect();
        let ev = Arc::new(ev);
        async move {
            if snapshot.is_empty() {
                return Vec::new();
            }
            let mut set = tokio::task::JoinSet::new();
            for h in snapshot {
                let ev = ev.clone();
                let args = args.clone();
                set.spawn(async move { h.call(ev, args).await });
            }
            let mut out = Vec::with_capacity(set.len());
            while let Some(res) = set.join_next().await {
                match res {
                    Ok(v) => out.push(v),
                    Err(e) => out.push(JsonValue::String(format!("task_panic: {e}"))),
                }
            }
            out
        }
    }

    /// 按序执行：匹配的异步处理器依次 await（结果收集，不传递）。
    pub fn serial(&self, name: &'static str, ev: EventKind, args: JsonValue) -> impl Future<Output = Vec<JsonValue>> {
        let snapshot: Vec<AsyncHandler> = self
            .inner
            .async_
            .read()
            .expect("bus poisoned")
            .iter()
            .filter(|e| e.name == name)
            .map(|e| e.handler.clone())
            .collect();
        let ev = Arc::new(ev);
        async move {
            let mut out = Vec::with_capacity(snapshot.len());
            for h in snapshot {
                out.push(h.call(ev.clone(), args.clone()).await);
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_protocol::{CoreEvent, EventKind};

    fn turn_start() -> EventKind {
        EventKind::Core(CoreEvent::TurnStart { turn: 1 })
    }

    #[test]
    fn emit_in_registration_order() {
        let bus = EventBus::new();
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let o1 = order.clone();
        let _d1 = bus.on("turn/start", Box::new(move |_| o1.lock().unwrap().push(1)));
        let o2 = order.clone();
        let _d2 = bus.on("turn/start", Box::new(move |_| o2.lock().unwrap().push(2)));
        bus.emit(&turn_start());
        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    }

    #[test]
    fn disposer_unsubscribes() {
        let bus = EventBus::new();
        let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n2 = n.clone();
        let d = bus.on("turn/start", Box::new(move |_| {
            n2.fetch_add(1, Ordering::SeqCst);
        }));
        bus.emit(&turn_start());
        drop(d);
        bus.emit(&turn_start());
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn name_filtering_isolates_events() {
        let bus = EventBus::new();
        let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n2 = n.clone();
        let _d = bus.on("turn/end", Box::new(move |_| {
            n2.fetch_add(1, Ordering::SeqCst);
        }));
        bus.emit(&turn_start());
        assert_eq!(n.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn waterfall_short_circuits() {
        let bus = EventBus::new();
        let _d1 = bus.around("middle", |_ev, args| {
            *args = serde_json::json!({"stage": "first"});
            WaterfallOutcome::Continue
        });
        let _d2 = bus.around("middle", |_ev, args| {
            *args = serde_json::json!({"stage": "short", "origin": args["stage"]});
            WaterfallOutcome::ShortCircuit(serde_json::json!({"blocked": true}))
        });
        let _d3 = bus.around("middle", |_ev, _args| WaterfallOutcome::Continue);
        let out = bus.waterfall("middle", &turn_start(), JsonValue::Null, || JsonValue::Null);
        assert_eq!(out, serde_json::json!({"blocked": true}));
    }

    #[test]
    fn waterfall_default_when_no_handler() {
        let bus = EventBus::new();
        let out = bus.waterfall("nobody", &turn_start(), JsonValue::Null, || serde_json::json!(42));
        assert_eq!(out, serde_json::json!(42));
    }

    struct Doubler;
    impl AsyncHandlerTrait for Doubler {
        fn call(&self, _ev: Arc<EventKind>, args: JsonValue) -> BoxFuture<'static, JsonValue> {
            let v = args["n"].as_u64().unwrap_or(0);
            Box::pin(async move { serde_json::json!(v * 2) })
        }
    }

    #[tokio::test]
    async fn parallel_fanout_collects_all() {
        let bus = EventBus::new();
        let _d = bus.on_async("calc", Arc::new(Doubler));
        let _d2 = bus.on_async("calc", Arc::new(Doubler));
        let out = bus.parallel("calc", turn_start(), serde_json::json!({"n": 21})).await;
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|v| v == &serde_json::json!(42)));
    }

    #[tokio::test]
    async fn serial_runs_in_order() {
        let bus = EventBus::new();
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        struct Seq(usize, std::sync::Arc<std::sync::Mutex<Vec<usize>>>);
        impl AsyncHandlerTrait for Seq {
            fn call(&self, _ev: Arc<EventKind>, _args: JsonValue) -> BoxFuture<'static, JsonValue> {
                let i = self.0;
                let order = self.1.clone();
                Box::pin(async move {
                    order.lock().unwrap().push(i);
                    serde_json::json!(i)
                })
            }
        }
        let _d = bus.on_async("seq", Arc::new(Seq(1, order.clone())));
        let _d2 = bus.on_async("seq", Arc::new(Seq(2, order.clone())));
        let out = bus.serial("seq", turn_start(), JsonValue::Null).await;
        assert_eq!(out, vec![serde_json::json!(1), serde_json::json!(2)]);
        assert_eq!(*order.lock().unwrap(), vec![1, 2]);
    }
}
