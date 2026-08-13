//! BoenMind 内核（bm-kernel）：插件加载器 / 服务注册表 / 事件总线 /
//! 会话事件日志原语（阶段 0 最小内核四件套）。
//!
//! 组装入口：[`KernelBuilder`]；插件唯一视角：[`Ctx`]。
//!
//! 铁律（实现方案 §5）：吸收不进核心——任何"顺手就能做"的功能放
//! 插件/存储/应用层；内核只承诺日志语义，不承诺存储实现（Port 化）。

pub mod bus;
pub mod ctx;
pub mod event_log;
pub mod loader;
pub mod plugin;
pub mod projection;
pub mod registry;
pub mod validation;

use std::sync::Arc;

use bm_protocol::{ErrorCode, EventKind, EventStorePort, ProtocolError};

pub use bus::EventBus;
pub use ctx::Ctx;
pub use event_log::{EventLog, InMemoryEventStore, SurfaceIntent};
pub use loader::{Loader, Manifest};
pub use plugin::{Disposer, Plugin, ServiceKey};
pub use projection::{Projection, SurfaceMessage, SurfaceProjection, SurfaceToolCall};
pub use registry::Registry;
pub use validation::{EventValidator, ValidationOutcome};

/// 内核实例：四件套组装完毕的运行态。
///
/// 插件副作用（Disposer）在 Kernel drop 时**逆序**执行（卸载 = 撤销
/// 一切注册）。运行期挂载新插件用 [`Kernel::install_plugin`]。
pub struct Kernel {
    registry: Arc<Registry>,
    bus: Arc<EventBus>,
    event_log: EventLog,
    /// 事件存储真身（registry 里的 key 只是就绪标记——trait object
    /// 无法进 Any 注册表，取用走这里）
    event_store: Arc<dyn EventStorePort>,
    plugin_disposers: tokio::sync::Mutex<Vec<Disposer>>,
}

/// 组装入口：先挂服务与插件，再 build。
pub struct KernelBuilder {
    store: Option<Arc<dyn EventStorePort>>,
    plugins: Vec<(Manifest, Box<dyn Plugin>)>,
}

impl Default for KernelBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelBuilder {
    pub fn new() -> Self {
        Self {
            store: None,
            plugins: Vec::new(),
        }
    }

    /// 指定事件存储 Port（缺省 = 内存实现，测试/无持久化场景）。
    pub fn with_event_store(mut self, store: Arc<dyn EventStorePort>) -> Self {
        self.store = Some(store);
        self
    }

    /// 预装插件（build 时按顺序安装；deps 未就绪即失败并整体回滚）。
    pub fn with_plugin(mut self, manifest: Manifest, plugin: Box<dyn Plugin>) -> Self {
        self.plugins.push((manifest, plugin));
        self
    }

    /// 组装内核。任一插件安装失败 → 已安装插件的副作用逆序回滚 → Err。
    pub fn build(self) -> Result<Kernel, ProtocolError> {
        let store: Arc<dyn EventStorePort> = self
            .store
            .unwrap_or_else(|| Arc::new(InMemoryEventStore::new()));
        let registry = Arc::new(Registry::new());
        let bus = Arc::new(EventBus::new());

        // 内核内置服务：事件存储（阶段 0 唯一必需）。
        // 注册表放就绪标记（deps 检查用）；真身经 Kernel::event_store 取。
        registry
            .register("event_store", Arc::new(()) as Arc<dyn std::any::Any + Send + Sync>)
            .map_err(|e| {
                ProtocolError::new(ErrorCode::PluginInstall, format!("register event_store: {e}"))
            })?;

        let event_log = EventLog::new(store.clone());
        let loader = Loader::new(registry.clone());
        let kernel = Kernel {
            registry,
            bus,
            event_log,
            event_store: store,
            plugin_disposers: tokio::sync::Mutex::new(Vec::new()),
        };

        // 逐插件安装；失败则回滚已安装的（disposers 逆序 drop）
        let mut installed: Vec<Disposer> = Vec::new();
        for (manifest, mut plugin) in self.plugins {
            let mut ctx = Ctx::new(&kernel);
            match loader.install(&manifest, &mut plugin, &mut ctx) {
                Ok(mut ds) => installed.append(&mut ds),
                Err(e) => {
                    for d in installed.iter_mut().rev() {
                        d.fire();
                    }
                    return Err(e);
                }
            }
        }
        *kernel.plugin_disposers.try_lock().expect("build: uncontended") = installed;
        Ok(kernel)
    }
}

impl std::fmt::Debug for Kernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kernel").finish_non_exhaustive()
    }
}

impl Kernel {
    /// 事件日志原语。
    pub fn event_log(&self) -> &EventLog {
        &self.event_log
    }

    /// 插件唯一视角（每次调用返回轻量视图）。
    pub fn ctx(&self) -> Ctx<'_> {
        Ctx::new(self)
    }

    /// 发事件（不 await，按注册序观察）。
    pub fn emit(&self, ev: impl Into<EventKind>) {
        self.bus.emit(&ev.into());
    }

    /// 订阅事件（返回 Disposer，drop 即退订）。
    pub fn on(&self, name: &'static str, h: bus::SyncHandler) -> Disposer {
        self.bus.on(name, h)
    }

    /// 按 key 取服务（类型不符 → InvalidArgument）。
    pub fn service<T: Send + Sync + 'static>(&self, key: ServiceKey) -> Result<Arc<T>, ProtocolError> {
        self.registry.get(key)
    }

    /// 事件存储端口真身（trait object 不进 Any 注册表，从这里取）。
    pub fn event_store(&self) -> Arc<dyn EventStorePort> {
        self.event_store.clone()
    }

    /// 运行期挂载新插件（deps 就绪才启动；失败无副作用）。
    pub fn install_plugin(&self, manifest: Manifest, plugin: Box<dyn Plugin>) -> Result<(), ProtocolError> {
        let loader = Loader::new(self.registry.clone());
        let mut ctx = Ctx::new(self);
        let mut plugin = plugin;
        let ds = loader.install(&manifest, &mut plugin, &mut ctx)?;
        self.plugin_disposers.try_lock().expect("install_plugin").extend(ds);
        Ok(())
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        // 卸载 = 逆序执行全部插件副作用（撤销注册/退订）
        let mut ds = std::mem::take(&mut *self.plugin_disposers.get_mut());
        for d in ds.iter_mut().rev() {
            d.fire();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_protocol::{CoreEvent, EventKind, SessionId};

    #[tokio::test]
    async fn kernel_build_with_default_store() {
        let kernel = KernelBuilder::new().build().unwrap();
        assert!(kernel.service::<()>("event_store").is_ok());
        // 事件存储真身可用（默认内存实现，可写可读）
        let sid = SessionId::new("sess_build");
        let bid = bm_protocol::BranchId::new("main");
        let log = kernel.event_log();
        log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnStart { turn: 1 }), crate::SurfaceIntent::None)
            .await
            .unwrap();
        let evs = log.replay(&sid, &bid).await.unwrap();
        assert_eq!(evs.len(), 1);
    }

    #[tokio::test]
    async fn kernel_emit_and_subscribe() {
        let kernel = KernelBuilder::new().build().unwrap();
        let seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen2 = seen.clone();
        let _d = kernel.on("turn/start", Box::new(move |_ev| {
            seen2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        kernel.emit(EventKind::Core(CoreEvent::TurnStart { turn: 1 }));
        assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn kernel_event_log_append_replay() {
        let kernel = KernelBuilder::new().build().unwrap();
        let sid = SessionId::new("sess_abc");
        let log = kernel.event_log();
        log.append(
            sid.clone(),
            bm_protocol::BranchId::new("main"),
            EventKind::Core(CoreEvent::TurnStart { turn: 1 }),
            SurfaceIntent::None,
        )
        .await
        .unwrap();
        let evs = log.replay(&sid, &bm_protocol::BranchId::new("main")).await.unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].seq.as_u64(), 1);
    }
}
