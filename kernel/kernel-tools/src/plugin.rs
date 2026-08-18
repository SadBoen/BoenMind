//! Tools 插件声明（核心分类）。
//!
//! 对应 dsh 官方 `@deepseek-ai/dsh-tools` 的工具注册表 + 守卫管线。
//! 组件（`ToolRegistry` / `ToolGate`）由装配方持有；本模块声明清单身份
//! 与内置工具装配点（当前内置集为空，host 工具由 web-server 层注册）。

use kernel_contracts::plugin::{PluginManifestEntry, PLUGIN_TOOLS};

use crate::ToolRegistry;

/// Tools 插件清单条目（核心组件）。
pub fn manifest() -> PluginManifestEntry {
    PluginManifestEntry::core(
        PLUGIN_TOOLS,
        "Tools",
        "工具注册表与门控：注册/校验/装卸工具，fail-closed 门控",
    )
}

/// 内置工具装配（当前内置集为空；留作插件装配点，后续官方工具组挂这里）。
pub fn register_builtin(_registry: &ToolRegistry) {}
