//! 插件声明与清单模型（核心/功能分类）。
//!
//! 最小化运行 = 核心三插件（llm / loop / tools）装配即可跑完整回合。
//! 分类标签语义（用户定调）：核心组件归 `Core`，插件管理员可按类
//! 隐藏/折叠，用户日常不看到它们；功能插件归 `Feature`。

use serde::{Deserialize, Serialize};

/// 插件分类：核心组件 vs 功能插件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginCategory {
    /// 内核核心组件（llm / loop / tools）——最小基座本身，用户日常不可见。
    Core,
    /// 功能插件（记忆/搜索/浏览器等可选加装）。
    Feature,
}

/// 插件清单条目（供插件管理员/前端分组展示）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifestEntry {
    /// 插件唯一 id（如 "llm" / "loop" / "tools"）。
    pub id: String,
    /// 分类标签（Core=核心组件；Feature=功能插件）。
    pub category: PluginCategory,
    /// 展示名。
    pub name: String,
    /// 一句话作用说明。
    pub description: String,
    /// 插件版本。
    pub version: String,
}

impl PluginManifestEntry {
    /// 构造核心组件清单条目（分类恒 Core；版本随契约层）。
    pub fn core(id: &str, name: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            category: PluginCategory::Core,
            name: name.to_string(),
            description: description.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// 核心三插件常量 id（最小基座 = provider + loop + tools）。
pub const PLUGIN_LLM: &str = "llm";
pub const PLUGIN_LOOP: &str = "loop";
pub const PLUGIN_TOOLS: &str = "tools";
