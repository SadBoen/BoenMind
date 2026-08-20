//! plugin-code-runtime 插件清单声明（功能分类）。
//! 装配由 `crate::register_all` 执行（在 bm-assembly 注入注册表）。

use kernel_contracts::plugin::{PluginCategory, PluginManifestEntry};

/// 插件清单条目（功能分类，随契约层版本）。
pub fn manifest() -> PluginManifestEntry {
    PluginManifestEntry {
        id: "plugin-code-runtime".to_string(),
        category: PluginCategory::Feature,
        name: "Code Runtime".to_string(),
        description: "代码执行沙箱：workdir 作用域编译/脚本执行，输出钱包防上下文撑爆".to_string(),
        version: option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0").to_string(),
    }
}