//! plugin-web-tools 插件清单声明（功能分类）。
//! 装配由 `crate::register_all` 执行（在 bm-assembly 注入注册表）。

use kernel_contracts::plugin::{PluginCategory, PluginManifestEntry};

/// 插件清单条目（功能分类，随契约层版本）。
pub fn manifest() -> PluginManifestEntry {
    PluginManifestEntry {
        id: "plugin-web-tools".to_string(),
        category: PluginCategory::Feature,
        name: "Web Tools".to_string(),
        description: "Web 取数工具：web.fetch 抓取 URL + web.search DuckDuckGo 搜索，SSRF 防线 + 输出钱包".to_string(),
        version: option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0").to_string(),
    }
}