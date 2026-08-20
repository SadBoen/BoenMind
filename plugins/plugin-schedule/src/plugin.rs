//! plugin-schedule 插件清单声明（功能分类）。
//! 装配由 `crate::register_all` 执行（在 bm-assembly 注入注册表）。

use kernel_contracts::plugin::{PluginCategory, PluginManifestEntry};

/// 插件清单条目（功能分类，随契约层版本）。
pub fn manifest() -> PluginManifestEntry {
    PluginManifestEntry {
        id: "plugin-schedule".to_string(),
        category: PluginCategory::Feature,
        name: "Schedule".to_string(),
        description: "定时任务：schedule.create/list/cancel 周期驱动目标会话回合".to_string(),
        version: option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0").to_string(),
    }
}