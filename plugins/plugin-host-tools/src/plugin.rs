//! plugin-host-tools 插件清单声明（核心分类）。
//! 装配由 `crate::register_all` 执行（在 bm-assembly 注入注册表）。

use kernel_contracts::plugin::{PluginCategory, PluginManifestEntry};

/// 插件清单条目（核心分类，随契约层版本）。
pub fn manifest() -> PluginManifestEntry {
    PluginManifestEntry {
        id: "plugin-host-tools".to_string(),
        category: PluginCategory::Core,
        name: "Host Tools".to_string(),
        description: "宿主机文件与命令工具：workdir 作用域读写/列目录/命令执行".to_string(),
        version: option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0").to_string(),
    }
}