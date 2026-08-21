//! plugin-goal 插件清单声明（功能分类）。
//! 装配由 `crate::register_all` 执行（在 bm-assembly 注入注册表）。

use kernel_contracts::plugin::{PluginCategory, PluginManifestEntry};

/// 插件清单条目（功能分类，随契约层版本）。
pub fn manifest() -> PluginManifestEntry {
    PluginManifestEntry {
        id: "plugin-goal".to_string(),
        category: PluginCategory::Feature,
        name: "Goal".to_string(),
        description: "目标管理：goal.get/create/update——模型侧目标控制，同会话续跑驱动（goal-round-driver）".to_string(),
        version: option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0").to_string(),
    }
}