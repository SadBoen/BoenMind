//! Context Compactor 插件声明（功能分类）。
//!
//! 对应 legacy 的 `bm-compactor`（默认压缩策略）+ `bm-loop compact.rs`
//! （事务协议）合并为一个插件面：策略与事务都在本插件内，loop 只装配
//! 调用（`LoopRuntime.compactor`）。功能插件 = 用户可加装/可关闭的可选面，
//! 与 auth 同形态（`Runtime::install_compactor` 装配）。

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
