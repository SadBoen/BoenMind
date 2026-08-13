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
pub use event_log::{EventLog, InMemoryEventStore, Subscription, SurfaceIntent, subscribe_events};
pub use loader::{Loader, Manifest};
pub use plugin::{Disposer, Plugin, ServiceKey};
pub use projection::{Projection, SurfaceMessage, SurfaceProjection, SurfaceToolCall};
pub use registry::{PortBox, Registry};
pub use validation::{EventValidator, ValidationOutcome};

/// 内核实例：四件套组装完毕的运行态。
///
/// 插件副作用按插件名分组（[`Kernel::install_plugin`] / [`Kernel::uninstall_plugin`]），
/// 卸载 = 该插件 Disposer 逆序执行；Kernel drop 时全部插件逆序卸载。
pub struct Kernel {
    registry: Arc<Registry>,
    bus: Arc<EventBus>,
    event_log: EventLog,
    /// 事件存储真身（同时经 PortBox 注册在 registry，插件可 `ctx.port` 取用）
    event_store: Arc<dyn EventStorePort>,
    /// 插件副作用分组（安装序，组内逆序撤销）
    plugin_disposers: std::sync::Mutex<Vec<(String, Vec<Disposer>)>>,
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

    /// 预装插件（build 时按依赖拓扑安装，声明顺序无关；deps 永不就绪即失败并整体回滚）。
    pub fn with_plugin(mut self, manifest: Manifest, plugin: Box<dyn Plugin>) -> Self {
        self.plugins.push((manifest, plugin));
        self
    }

    /// 组装内核。任一插件安装失败 → 已安装插件的副作用逆序回滚 → Err。
    ///
    /// 安装顺序由**依赖表达**（deferred 拓扑）：deps 未就绪的插件挂起，
    /// 就绪后再装；一轮无进展 = 依赖永远无法满足 → 失败（dsh inject
    /// 语义的同步版，启动期拓扑排序、运行期 fail-fast）。
    pub fn build(self) -> Result<Kernel, ProtocolError> {
        let store: Arc<dyn EventStorePort> = self
            .store
            .unwrap_or_else(|| Arc::new(InMemoryEventStore::new()));
        let registry = Arc::new(Registry::new());
        let bus = Arc::new(EventBus::new());

        // 内核内置服务：事件存储（阶段 0 唯一必需）。
        // PortBox 包装进 Any 注册表——插件按 trait 取用（ctx.port）。
        registry
            .register("event_store", Arc::new(PortBox(store.clone())) as Arc<dyn std::any::Any + Send + Sync>)
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
            plugin_disposers: std::sync::Mutex::new(Vec::new()),
        };

        // 预校验全部插件（name / deps 声明一致性——非就绪性错误即刻失败）
        for (manifest, plugin) in &self.plugins {
            loader.validate(manifest, plugin.as_ref())?;
        }

        // deferred 拓扑：循环取 deps 就绪的插件安装，未就绪的挂起等下一轮
        let mut pending = self.plugins;
        let mut groups: Vec<(String, Vec<Disposer>)> = Vec::new();
        while !pending.is_empty() {
            let (ready, waiting): (Vec<_>, Vec<_>) = pending
                .into_iter()
                .partition(|(m, _)| loader.deps_ready(&m.deps));
            if ready.is_empty() {
                rollback(&mut groups);
                let names: Vec<String> = waiting.into_iter().map(|(m, _)| m.name).collect();
                return Err(ProtocolError::new(
                    ErrorCode::PluginInstall,
                    format!("plugins waiting on unavailable deps: {}", names.join(", ")),
                ));
            }
            for (manifest, mut plugin) in ready {
                let mut ctx = Ctx::new(&kernel);
                match loader.install(&manifest, &mut plugin, &mut ctx) {
                    Ok(ds) => groups.push((manifest.name.clone(), ds)),
                    Err(e) => {
                        rollback(&mut groups);
                        return Err(e);
                    }
                }
            }
            pending = waiting;
        }
        *kernel
            .plugin_disposers
            .lock()
            .expect("build: uncontended") = groups;
        Ok(kernel)
    }
}

/// 逆序执行全部已装插件副作用（同插件组内逆序）。
fn rollback(groups: &mut [(String, Vec<Disposer>)]) {
    for (_, ds) in groups.iter_mut().rev() {
        for d in ds.iter_mut().rev() {
            d.fire();
        }
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

    /// 按 key 取 Port（trait object 服务，如 `Arc<dyn EventStorePort>`）。
    pub fn port<P: ?Sized + Send + Sync + 'static>(&self, key: ServiceKey) -> Result<Arc<P>, ProtocolError> {
        self.registry.get_port(key)
    }

    /// 事件存储端口真身（registry 里也有 PortBox 副本，此处为内核内部捷径）。
    pub fn event_store(&self) -> Arc<dyn EventStorePort> {
        self.event_store.clone()
    }

    /// 运行期挂载新插件（deps 就绪才启动，fail-fast；失败无副作用）。
    pub fn install_plugin(&self, manifest: Manifest, plugin: Box<dyn Plugin>) -> Result<(), ProtocolError> {
        let loader = Loader::new(self.registry.clone());
        let mut ctx = Ctx::new(self);
        let mut plugin = plugin;
        let ds = loader.install(&manifest, &mut plugin, &mut ctx)?;
        self.plugin_disposers
            .lock()
            .expect("plugin_disposers poisoned")
            .push((manifest.name.clone(), ds));
        Ok(())
    }

    /// 卸载指定插件：其 Disposer 逆序执行（撤销它注册的一切）。
    /// 未安装 → NotFound。
    pub fn uninstall_plugin(&self, name: &str) -> Result<(), ProtocolError> {
        let mut groups = self.plugin_disposers.lock().expect("plugin_disposers poisoned");
        let idx = groups
            .iter()
            .position(|(n, _)| n == name)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::NotFound,
                    format!("plugin `{name}` not installed"),
                )
            })?;
        let (_, mut ds) = groups.remove(idx);
        for d in ds.iter_mut().rev() {
            d.fire();
        }
        Ok(())
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        // 卸载 = 逆序执行全部插件副作用（后装先卸、组内逆序）
        let mut groups = std::mem::take(
            &mut *self.plugin_disposers.lock().expect("plugin_disposers poisoned"),
        );
        rollback(&mut groups);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_protocol::{CoreEvent, ErrorCode, EventKind, ProtocolError, SessionId};

    #[tokio::test]
    async fn kernel_build_with_default_store() {
        let kernel = KernelBuilder::new().build().unwrap();
        // 事件存储以 PortBox 注册，按 trait 取用（A2 Port 集合形态）
        assert!(kernel.port::<dyn EventStorePort>("event_store").is_ok());
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

    #[tokio::test]
    async fn uninstall_plugin_reverts_its_registrations() {
        // per-plugin disposer 分组：卸载单个插件 = 撤销它注册的一切
        struct Provider;
        impl Plugin for Provider {
            fn name(&self) -> &'static str {
                "u.provider"
            }
            fn apply(&mut self, ctx: &mut Ctx<'_>) -> Result<Vec<Disposer>, ProtocolError> {
                let d = ctx.register_service("u.svc", Arc::new(11i32))?;
                Ok(vec![d])
            }
        }
        let kernel = KernelBuilder::new()
            .with_plugin(
                Manifest {
                    name: "u.provider".into(),
                    version: "0.1.0".into(),
                    deps: vec![],
                    description: None,
                },
                Box::new(Provider),
            )
            .build()
            .unwrap();
        assert!(kernel.service::<i32>("u.svc").is_ok());
        kernel.uninstall_plugin("u.provider").unwrap();
        assert!(kernel.service::<i32>("u.svc").is_err());
        // 未安装的插件 → NotFound
        let err = kernel.uninstall_plugin("u.ghost").unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
    }
}
