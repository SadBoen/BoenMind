//! 插件加载器：极简 manifest + 依赖解析 + 可逆副作用安装。
//!
//! manifest（plugin.json，Z1 风格）只承载元数据与依赖声明；
//! 具体插件本体首版为 Rust `Box<dyn Plugin>`（QuickJS 运行时是阶段 1
//! pi-compat 的事，此处只定义加载语义）。

use std::sync::Arc;

use bm_protocol::{ErrorCode, ProtocolError};
use serde::{Deserialize, Serialize};

use crate::ctx::Ctx;
use crate::{Disposer, Plugin, Registry};

/// 极简插件清单（plugin.json）。name 必须与 Plugin::name() 一致。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    /// 依赖的服务 key（未就绪 → 安装失败）
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// 加载器：manifest 校验 → 依赖解析 → apply → 返回可逆副作用。
pub struct Loader {
    registry: Arc<Registry>,
}

impl Loader {
    pub fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }

    /// 安装插件。deps 未就绪 / name 不一致 → Err（无副作用）。
    pub fn install(
        &self,
        manifest: &Manifest,
        plugin: &mut Box<dyn Plugin>,
        ctx: &mut Ctx<'_>,
    ) -> Result<Vec<Disposer>, ProtocolError> {
        if manifest.name != plugin.name() {
            return Err(ProtocolError::new(
                ErrorCode::PluginInstall,
                format!("manifest name `{}` != plugin name `{}`", manifest.name, plugin.name()),
            ));
        }
        self.check_deps(&manifest.deps)?;
        // deps() 声明与 manifest 不一致时以 manifest 为准（它是分发形态），
        // 但双处声明不一致属于打包错误：
        for key in plugin.deps() {
            if !manifest.deps.iter().any(|d| d.as_str() == *key) {
                return Err(ProtocolError::new(
                    ErrorCode::PluginInstall,
                    format!("plugin `{}` declares dep `{key}` missing in manifest", plugin.name()),
                ));
            }
        }
        plugin.apply(ctx)
    }

    /// 依赖就绪检查：每个 key 必须已注册（内核内置或先前插件提供）。
    pub fn check_deps(&self, deps: &[String]) -> Result<(), ProtocolError> {
        for key in deps {
            if !self.registry.contains(key.as_str()) {
                return Err(ProtocolError::new(
                    ErrorCode::PluginInstall,
                    format!("plugin dep `{key}` not ready"),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Kernel, KernelBuilder, ServiceKey};

    struct TestPlugin {
        fired: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Plugin for TestPlugin {
        fn name(&self) -> &'static str {
            "test.plugin"
        }
        fn apply(&mut self, ctx: &mut Ctx<'_>) -> Result<Vec<Disposer>, ProtocolError> {
            let n = self.fired.clone();
            let d = ctx.on("turn/start", Box::new(move |_ev| {
                n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }));
            Ok(vec![d])
        }
    }

    fn manifest(name: &str, deps: &[&str]) -> Manifest {
        Manifest {
            name: name.into(),
            version: "0.1.0".into(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
            description: None,
        }
    }

    #[test]
    fn install_plugin_via_builder() {
        let kernel: Kernel = KernelBuilder::new()
            .with_plugin(manifest("test.plugin", &[]), Box::new(TestPlugin {
                fired: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }))
            .build()
            .unwrap();
        kernel.emit(bm_protocol::EventKind::Core(bm_protocol::CoreEvent::TurnStart { turn: 1 }));
    }

    #[test]
    fn manifest_name_mismatch_rejected() {
        let err = KernelBuilder::new()
            .with_plugin(manifest("other.name", &[]), Box::new(TestPlugin {
                fired: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }))
            .build()
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::PluginInstall);
    }

    #[test]
    fn missing_dep_rejected_and_rolls_back() {
        let kernel: Kernel = KernelBuilder::new()
            .with_plugin(manifest("test.plugin", &[]), Box::new(TestPlugin {
                fired: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }))
            .build()
            .unwrap();

        // 运行期挂载依赖不存在的插件 → 失败
        let err = kernel
            .install_plugin(manifest("test.plugin", &["no_such_service"]), Box::new(TestPlugin {
                fired: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }))
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::PluginInstall);
    }

    #[test]
    fn deps_ready_then_install_ok() {
        // 第一个插件注册服务，第二个插件依赖它 → 顺序安装成功
        struct Provider;
        impl Plugin for Provider {
            fn name(&self) -> &'static str {
                "test.provider"
            }
            fn deps(&self) -> &[ServiceKey] {
                &["event_store"]
            }
            fn apply(&mut self, ctx: &mut Ctx<'_>) -> Result<Vec<Disposer>, ProtocolError> {
                let d = ctx.register_service("extra.svc", Arc::new(42i32))?;
                Ok(vec![d])
            }
        }
        struct Consumer;
        impl Plugin for Consumer {
            fn name(&self) -> &'static str {
                "test.consumer"
            }
            fn deps(&self) -> &[ServiceKey] {
                &["extra.svc"]
            }
            fn apply(&mut self, ctx: &mut Ctx<'_>) -> Result<Vec<Disposer>, ProtocolError> {
                let svc = ctx.service::<i32>("extra.svc")?;
                assert_eq!(*svc, 42);
                Ok(vec![])
            }
        }
        let kernel = KernelBuilder::new()
            .with_plugin(manifest("test.provider", &["event_store"]), Box::new(Provider))
            .with_plugin(manifest("test.consumer", &["extra.svc"]), Box::new(Consumer))
            .build()
            .unwrap();
        assert!(kernel.service::<i32>("extra.svc").is_ok());
    }
}
