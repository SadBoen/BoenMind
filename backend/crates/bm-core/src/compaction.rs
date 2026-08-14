//! 按模型上下文压缩配置（`[compaction]` 配置段解析与默认值）。
//!
//! 压缩引擎本体 = bm-compactor 插件（策略参数插件自治，可换可关，架构 §6.9）；
//! 本模块只保留配置结构的解析（config.toml 向后兼容）。
//! pi 路径的窗口探测/模型解析（resolve_for_model/probe_context_window）已于
//! 2026-08-15 pi 废除轮删除——bm 引擎不读 pi models.json。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 全局压缩默认值（需求 2：默认 50% 水线，与 pi 对齐——双开复测定稿）。
pub const DEFAULT_WATERMARK: f64 = 0.50;
/// 尾部保留比例（占窗口比例，Hermes 的 threshold × target_ratio 语义）
pub const DEFAULT_KEEP_RECENT_RATIO: f64 = 0.10;
/// 尾部保留 token 下限（防止小窗口模型尾部预算过小）
pub const DEFAULT_KEEP_RECENT_FLOOR: u32 = 4_000;

/// `[compaction]` 配置段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// 总开关；`false` 时完全不注入
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 全局默认压缩水线（0.0 ~ 1.0，占用窗口比例）
    #[serde(default = "default_watermark")]
    pub watermark: f64,
    /// 全局默认尾部保留比例（0.0 ~ 1.0，占窗口比例）
    #[serde(default = "default_keep_recent_ratio")]
    pub keep_recent_ratio: f64,
    /// 全局默认尾部保留 token 下限
    #[serde(default = "default_keep_recent_floor")]
    pub keep_recent_floor: u32,
    /// 按模型覆盖；键为 `"<provider>/<model>"`（如 `"deepseek/deepseek-chat"`）
    #[serde(default)]
    pub overrides: HashMap<String, CompactionOverride>,
}

/// 单个模型的覆盖配置（`[compaction.overrides."<provider>/<model>"]`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionOverride {
    /// 模型上下文窗口（token）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    /// 覆盖全局水线
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watermark: Option<f64>,
    /// 覆盖全局尾部保留比例
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_recent_ratio: Option<f64>,
    /// 覆盖全局尾部保留下限
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_recent_floor: Option<u32>,
}

fn default_enabled() -> bool {
    true
}

fn default_watermark() -> f64 {
    DEFAULT_WATERMARK
}

fn default_keep_recent_ratio() -> f64 {
    DEFAULT_KEEP_RECENT_RATIO
}

fn default_keep_recent_floor() -> u32 {
    DEFAULT_KEEP_RECENT_FLOOR
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            watermark: default_watermark(),
            keep_recent_ratio: default_keep_recent_ratio(),
            keep_recent_floor: default_keep_recent_floor(),
            overrides: HashMap::new(),
        }
    }
}
