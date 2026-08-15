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

/// 生效的压缩参数（组装层换算注入 bm-compactor 用）。
/// `effective()` 把 `[compaction]` + overrides 换算成策略插件的构造参数——
/// 策略实现（bm-compactor）不读配置，参数注入是组装层的活（§6.9 拆法）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectiveCompaction {
    /// 软水线（0.0 ~ 1.0，占用窗口比例）
    pub watermark: f64,
    /// 尾部保留比例（占窗口比例）
    pub keep_recent_ratio: f64,
    /// 尾部保留 token 下限
    pub keep_recent_floor: u32,
}

impl CompactionConfig {
    /// 按 provider/model 求生效参数：overrides 优先，否则全局默认。
    /// `enabled=false` → None（组装层语义：不挂压缩插件 = 裸跑，核心
    /// 以硬触发兜底——"缺插件优雅失败"）。
    pub fn effective(&self, provider: &str, model: &str) -> Option<EffectiveCompaction> {
        if !self.enabled {
            return None;
        }
        let ov = self.overrides.get(&format!("{provider}/{model}"));
        Some(EffectiveCompaction {
            watermark: ov.and_then(|o| o.watermark).unwrap_or(self.watermark),
            keep_recent_ratio: ov
                .and_then(|o| o.keep_recent_ratio)
                .unwrap_or(self.keep_recent_ratio),
            keep_recent_floor: ov
                .and_then(|o| o.keep_recent_floor)
                .unwrap_or(self.keep_recent_floor),
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_uses_global_defaults_without_overrides() {
        let c = CompactionConfig::default();
        let e = c.effective("deepseek", "deepseek-chat").unwrap();
        assert_eq!(e.watermark, DEFAULT_WATERMARK);
        assert_eq!(e.keep_recent_ratio, DEFAULT_KEEP_RECENT_RATIO);
        assert_eq!(e.keep_recent_floor, DEFAULT_KEEP_RECENT_FLOOR);
    }

    #[test]
    fn effective_applies_model_override() {
        let mut c = CompactionConfig::default();
        c.overrides.insert(
            "mini/m3".to_string(),
            CompactionOverride {
                watermark: Some(0.8),
                keep_recent_ratio: None,
                keep_recent_floor: Some(8_000),
                context_window: None,
            },
        );
        let e = c.effective("mini", "m3").unwrap();
        assert_eq!(e.watermark, 0.8, "override 生效");
        assert_eq!(e.keep_recent_ratio, DEFAULT_KEEP_RECENT_RATIO, "未覆盖回落全局");
        assert_eq!(e.keep_recent_floor, 8_000);

        // 其他模型不受影响
        let other = c.effective("deepseek", "deepseek-chat").unwrap();
        assert_eq!(other.watermark, DEFAULT_WATERMARK);
    }

    #[test]
    fn effective_none_when_disabled() {
        let mut c = CompactionConfig::default();
        c.enabled = false;
        assert!(c.effective("deepseek", "deepseek-chat").is_none());
    }
}
