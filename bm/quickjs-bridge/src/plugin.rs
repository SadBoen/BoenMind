//! JS 插件 manifest 与装载（落地顺序 §5.3：manifest 驱动 + 最小权限授面）。
//!
//! 一个 JS 插件 = 一个文件夹（与 Rust 插件 `plugins/plugin-*` 一插件一子文件夹
//! 的形态对齐），内含 `plugin.json`（manifest）+ 入口 JS。
//!
//! ```json
//! {
//!   "id": "my-plugin",
//!   "name": "My Plugin",
//!   "version": "0.1.0",
//!   "entry": "main.js",
//!   "host": ["tools.list", "tools.invoke", "llm.complete"]
//! }
//! ```
//!
//! `host` 数组声明插件使用的宿主 API 面；组合根**按声明授面，默认最小权限**——
//! 未声明的面不注入 JS（`host.tools` / `host.llm` 等为 `undefined`），防止插件
//! 越权读配置/调工具/碰会话。面名单见 [`ALL_HOST_FACES`]。

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// 全部 host 面（授面粒度 = 单个方法，最小权限可精确到危险面如 `tools.invoke`）。
pub const ALL_HOST_FACES: &[&str] = &[
    "log",
    "config.get",
    "tools.list",
    "tools.invoke",
    "llm.complete",
    "session.append",
    "session.get",
    "session.poll",
];

/// JS 插件 manifest（`plugin.json`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsPluginManifest {
    /// 插件唯一 id。
    pub id: String,
    /// 展示名。
    pub name: String,
    #[serde(default)]
    pub version: String,
    /// JS 入口文件相对路径（相对插件目录）。
    pub entry: String,
    /// 声明的 host 面（`ALL_HOST_FACES` 的子集）；缺省 = 最小（空集）。
    #[serde(default)]
    pub host: Vec<String>,
}

impl JsPluginManifest {
    /// 从 `plugin.json` 文本解析。
    pub fn from_json(text: &str) -> Result<Self, String> {
        let m: JsPluginManifest =
            serde_json::from_str(text).map_err(|e| format!("parse plugin.json: {e}"))?;
        // 校验面名都在白名单内（防拼错面名静默失效）。
        for face in &m.host {
            if !ALL_HOST_FACES.contains(&face.as_str()) {
                return Err(format!("unknown host face: {face}"));
            }
        }
        Ok(m)
    }

    /// 去重后的面集合（manifest 可能重复声明）。
    pub fn face_set(&self) -> BTreeSet<String> {
        self.host.iter().cloned().collect()
    }
}

/// 插件目录 → manifest + 入口 JS 源码。
///
/// 目录结构：`<dir>/plugin.json` + `<dir>/<manifest.entry>`。
pub struct LoadedPlugin {
    pub manifest: JsPluginManifest,
    /// 入口 JS 源码（已按 manifest.entry 读盘）。
    pub entry_source: String,
}

impl LoadedPlugin {
    /// 从插件目录装载（读 manifest + 入口源码，不做 JS 执行/授面）。
    pub fn load(dir: &Path) -> Result<Self, String> {
        let manifest_path = dir.join("plugin.json");
        let manifest_text = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
        let manifest = JsPluginManifest::from_json(&manifest_text)?;
        let entry_path = dir.join(&manifest.entry);
        let entry_source = std::fs::read_to_string(&entry_path)
            .map_err(|e| format!("read {}: {e}", entry_path.display()))?;
        Ok(Self { manifest, entry_source })
    }
}
