//! JS 插件运行时（§6 接业务）：把目录插件注册表收敛进 `PluginRuntimePort`——
//! 扫描 → 逐插件按 manifest 最小权限授面建引擎 → 探针变 Ready。
//!
//! 职责边界（组合根纪律）：本模块只做**装配**（扫描 + 建引擎 + 保活）与
//! **插件入口执行**（`exec_entry`/`call_main`）；具体插件（llm/loop/tools）
//! 只能在 bm-assembly 装配，web-server/headless 不直接依赖 quickjs-bridge
//! 宿主实现。

use kernel_contracts::plugin::{PluginCategory, PluginManifestEntry};
use kernel_contracts::ports::{PluginRuntimeAvailability, PluginRuntimePort};
use quickjs_bridge::{JsBridge, JsPluginManifest};

/// 一个已装配的 JS 插件（manifest 元数据 + 引擎保活 + 入口已装载）。
///
/// `engine` 持有即保活：每插件一 `JsBridge` = 独立 AsyncRuntime + QuickJS
/// 上下文，插件间天然隔离（全局变量/异常互不干扰）；drop 时销毁引擎线程。
/// 入口源码在装配时已 `exec`（定义插件全局函数），`call_main` 执行主函数。
pub struct JsPluginEntry {
    pub manifest: JsPluginManifest,
    engine: JsBridge,
}

impl JsPluginEntry {
    pub fn new(manifest: JsPluginManifest, entry_source: &str, engine: JsBridge) -> Result<Self, String> {
        engine.exec(entry_source)?;
        Ok(Self { manifest, engine })
    }

    /// 执行插件入口已定义的主函数（`__main`）并返回 JSON 结果。
    pub fn call_main(&self) -> Result<serde_json::Value, String> {
        self.engine.call_async("__main", &[])
    }
}

/// 目录插件运行时：实现 [`PluginRuntimePort`] 探针。
///
/// - 空清单 → `Unavailable`（诚实失败：没装配就是没装配，不假 Ready）；
/// - 非空 → `Ready`（至少一个 JS 插件引擎已装配）。
#[derive(Default)]
pub struct JsPluginRuntime {
    entries: Vec<JsPluginEntry>,
}

impl JsPluginRuntime {
    pub fn new(entries: Vec<JsPluginEntry>) -> Self {
        Self { entries }
    }

    /// 已装配插件只读视图（id 字典序，与 `scan_plugins` 一致）。
    pub fn entries(&self) -> &[JsPluginEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 执行指定插件的主函数（`__main`；插件须在 manifest `entry` 里定义）。
    pub fn call(&self, plugin_id: &str) -> Result<serde_json::Value, String> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.manifest.id == plugin_id)
            .ok_or_else(|| format!("js plugin '{plugin_id}' not loaded"))?;
        entry.call_main()
    }

    /// 插件清单条目（category=Feature）：供 `plugin.core.list` 合并展示——
    /// 核心三插件（Core）不变，JS 插件以 Feature 分类追加。
    pub fn manifest_entries(&self) -> Vec<PluginManifestEntry> {
        self.entries
            .iter()
            .map(|e| PluginManifestEntry {
                id: e.manifest.id.clone(),
                category: PluginCategory::Feature,
                name: e.manifest.name.clone(),
                description: format!(
                    "JS plugin ({} host face(s))",
                    e.manifest.face_set().len()
                ),
                version: e.manifest.version.clone(),
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl PluginRuntimePort for JsPluginRuntime {
    fn availability(&self) -> PluginRuntimeAvailability {
        if self.entries.is_empty() {
            PluginRuntimeAvailability::Unavailable {
                reason: "no JS plugins loaded (plugins-dir empty or not provided)".into(),
            }
        } else {
            PluginRuntimeAvailability::Ready
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
