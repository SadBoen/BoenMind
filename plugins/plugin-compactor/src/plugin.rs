//! Context Compactor 插件声明（功能分类）。
//!
//! 对应 legacy 的 `bm-compactor`（默认压缩策略）；事务协议在契约层
//! `bm-ports::Compactor::maybe_compact`（2026-08-20 上提收口，loop 已不
//! 依赖本插件）。本插件只提供默认策略实现 + 清单身份，功能插件 = 用户
//! 可加装/可关闭的可选面，与 auth 同形态（`Runtime::install_compactor` 装配）。

use kernel_contracts::plugin::{PluginCategory, PluginManifestEntry};

/// Compactor 插件清单条目（功能插件）。
pub fn manifest() -> PluginManifestEntry {
    PluginManifestEntry {
        id: "compactor".to_string(),
        category: PluginCategory::Feature,
        name: "Context Compactor".to_string(),
        description: "上下文压缩：长会话按水线自动摘要压缩（运行态视图变换，前端无感）".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}
