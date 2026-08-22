//! plugin-approval 插件清单声明（功能分类）。
//! 装配由 bm-assembly `install_approval_center` 执行（审批中心 + loop 消费面）。

use kernel_contracts::plugin::{PluginCategory, PluginManifestEntry};

/// 插件清单条目（功能分类，随契约层版本）。
pub fn manifest() -> PluginManifestEntry {
    PluginManifestEntry {
        id: "plugin-approval".to_string(),
        category: PluginCategory::Feature,
        name: "Approval".to_string(),
        description: "工具审批：危险工具执行前弹窗裁定 + pending 表 + /api/respond 路由".to_string(),
        version: option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0").to_string(),
    }
}
