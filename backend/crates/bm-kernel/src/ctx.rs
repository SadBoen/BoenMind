//! Ctx：插件的唯一视角（订阅/发事件/注册服务/取服务/中间件）。
//!
//! Ctx 持内核共享引用（内部锁可变），可跨线程使用；插件挂载期
//! （`Plugin::apply`）与运行期（`Kernel::ctx`）是同一套 API。

use std::sync::Arc;

use bm_protocol::{EventKind, ProtocolError};
use serde_json::Value as JsonValue;

use crate::bus::{AsyncHandler, WaterfallOutcome};
use crate::{Disposer, Kernel, ServiceKey};

/// 插件唯一视角：内核的最小可操作面。
pub struct Ctx<'k> {
    pub(crate) kernel: &'k Kernel,
}

impl<'k> Ctx<'k> {
    pub(crate) fn new(kernel: &'k Kernel) -> Self {
        Self { kernel }
    }

    /// 按 key 取服务（类型不符 → InvalidArgument）。
    pub fn service<T: Send + Sync + 'static>(&self, key: ServiceKey) -> Result<Arc<T>, ProtocolError> {
        self.kernel.service(key)
    }

    /// 事件存储端口真身。
    pub fn event_store(&self) -> Arc<dyn bm_protocol::EventStorePort> {
        self.kernel.event_store()
    }

    /// 注册服务（重复 key → AlreadyRegistered；Disposer 撤销注册）。
    pub fn register_service<T: Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        svc: Arc<T>,
    ) -> Result<Disposer, ProtocolError> {
        let reg = &self.kernel.registry;
        reg.register(key, svc)?;
        Ok(Disposer::new({
            let reg = reg.clone();
            move || {
                reg.remove(key);
            }
        }))
    }

    /// 订阅事件（drop 退订）。
    pub fn on(&self, name: &'static str, h: crate::bus::SyncHandler) -> Disposer {
        self.kernel.on(name, h)
    }

    /// 环绕中间件（waterfall 短路语义）。
    pub fn around(
        &self,
        name: &'static str,
        h: impl Fn(&EventKind, &mut JsonValue) -> WaterfallOutcome + Send + Sync + 'static,
    ) -> Disposer {
        self.kernel.bus.around(name, h)
    }

    /// 异步处理器注册。
    pub fn on_async(&self, name: &'static str, h: AsyncHandler) -> Disposer {
        self.kernel.bus.on_async(name, h)
    }

    /// 发事件（不 await，按注册序观察）。
    pub fn emit(&self, ev: impl Into<EventKind>) {
        self.kernel.emit(ev);
    }

    /// 环绕中间件链（见 [`crate::EventBus::waterfall`]）。
    pub fn waterfall<F>(&self, name: &'static str, ev: impl Into<EventKind>, args: JsonValue, default: F) -> JsonValue
    where
        F: FnOnce() -> JsonValue,
    {
        let kind = ev.into();
        self.kernel.bus.waterfall(name, &kind, args, default)
    }

    /// 并发扇出（见 [`crate::EventBus::parallel`]）。
    pub fn parallel(&self, name: &'static str, ev: impl Into<EventKind>, args: JsonValue) -> impl Future<Output = Vec<JsonValue>> {
        self.kernel.bus.parallel(name, ev.into(), args)
    }

    /// 按序执行（见 [`crate::EventBus::serial`]）。
    pub fn serial(&self, name: &'static str, ev: impl Into<EventKind>, args: JsonValue) -> impl Future<Output = Vec<JsonValue>> {
        self.kernel.bus.serial(name, ev.into(), args)
    }
}
