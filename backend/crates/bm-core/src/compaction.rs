//! 按模型上下文压缩配置（需求 1/2/3：探测窗口、默认 50% 水线、按模型可配置）。
//!
//! 压缩引擎逻辑在 vendored pi_agent_rust 内（`compaction.rs`），本模块只负责
//! 把 BoenMind 的配置解析成 pi `SessionOptions.compaction_settings` 需要的
//! `ResolvedCompactionSettings`（窗口/水线/尾部预算），经 SDK 入口注入，
//! 不触碰 pi 核心逻辑。
//!
//! 语义（Hermes 启发）：
//! - 水线 watermark：占用 ≥ 窗口 × watermark 时触发压缩 → pi 的
//!   `reserve_tokens = 窗口 × (1 - watermark)`（压缩发生在窗口 - reserve 处）
//! - 尾部保护 keep_recent：`max(窗口 × keep_recent_ratio, keep_recent_floor)`
//!
//! 探测优先级：配置覆盖（`compaction.overrides` 的 context_window，即"实际
//! 窗口"）→ models.json 声明窗口（sync 时写入的 contextWindow）→ 128K 兜底
//! （与 pi 的 fallback 一致）。

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::pi_agent_dir;

/// 全局压缩默认值（需求 2：默认 50% 水线）。
pub const DEFAULT_WATERMARK: f64 = 0.50;
/// 尾部保留比例（占窗口比例，Hermes 的 threshold × target_ratio 语义）
pub const DEFAULT_KEEP_RECENT_RATIO: f64 = 0.10;
/// 尾部保留 token 下限（防止小窗口模型尾部预算过小）
pub const DEFAULT_KEEP_RECENT_FLOOR: u32 = 4_000;
/// 未探测到窗口时的兜底（与 pi `ResolvedCompactionSettings::default` 一致）
pub const FALLBACK_CONTEXT_WINDOW: u32 = 128_000;

/// `[compaction]` 配置段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// 总开关；`false` 时完全不注入（走 pi 现有全局行为，向后兼容）
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
    /// 按模型覆盖；键为 `"<pi_name>/<model_id>"`（如 `"deepseek/deepseek-chat"`）
    #[serde(default)]
    pub overrides: HashMap<String, CompactionOverride>,
}

/// 单个模型的覆盖配置（`[compaction.overrides."<pi>/<model>"]`）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionOverride {
    /// 模型上下文窗口（token）；覆盖探测值（"实际窗口"）
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

/// 解析后的压缩设置（注入 pi 的 `SessionOptions.compaction_settings`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedCompaction {
    pub enabled: bool,
    pub context_window: u32,
    /// pi `ResolvedCompactionSettings.reserve_tokens`：窗口 × (1 - watermark)
    pub reserve_tokens: u32,
    /// pi `ResolvedCompactionSettings.keep_recent_tokens`：max(窗口 × ratio, floor)
    pub keep_recent_tokens: u32,
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

impl ResolvedCompaction {
    /// 从解析参数构造；水线与比例按 (0, 1] 收敛（非法值取默认）。
    pub fn new(enabled: bool, window: u32, watermark: f64, ratio: f64, floor: u32) -> Self {
        let watermark = normalize_ratio(watermark, DEFAULT_WATERMARK);
        let ratio = normalize_ratio(ratio, DEFAULT_KEEP_RECENT_RATIO);
        let floor = if floor == 0 { DEFAULT_KEEP_RECENT_FLOOR } else { floor };
        let reserve = ratio_to_tokens(window, 1.0 - watermark);
        let keep_recent = ratio_to_tokens(window, ratio).max(floor);
        Self {
            enabled,
            context_window: window,
            reserve_tokens: reserve,
            keep_recent_tokens: keep_recent,
        }
    }
}

/// 把比例换算成 token 数（四舍五入，至少 1）。
fn ratio_to_tokens(window: u32, ratio: f64) -> u32 {
    let tokens = (f64::from(window) * ratio).round();
    tokens.clamp(1.0, f64::from(u32::MAX)) as u32
}

/// 比例归一化：NaN / ≤0 / >1 时取默认值。
fn normalize_ratio(value: f64, default: f64) -> f64 {
    if value.is_finite() && value > 0.0 && value <= 1.0 {
        value
    } else {
        default
    }
}

/// 探测模型的声明窗口：读 `~/.boenmind/pi/models.json` 中
/// `providers.<pi_name>.models[]` 里对应模型的 `contextWindow`。
pub fn probe_context_window(models_json: &Path, pi_name: &str, model_id: &str) -> Option<u32> {
    let text = std::fs::read_to_string(models_json).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    let providers = doc.get("providers")?;
    let provider = providers.get(pi_name)?;
    let models = provider.get("models")?.as_array()?;
    models
        .iter()
        .find(|m| m.get("id").and_then(serde_json::Value::as_str) == Some(model_id))
        .and_then(|m| m.get("contextWindow").and_then(serde_json::Value::as_u64))
        .and_then(|w| u32::try_from(w).ok())
}

/// 按模型解析压缩配置。
///
/// - `config.compaction.enabled == false` → `None`（不注入，pi 现有行为）
/// - 窗口 = 覆盖配置 context_window → 探测 models.json → 128K 兜底
/// - 水线/尾部比例 = 模型覆盖 → 全局默认
pub fn resolve_for_model(
    compaction: &CompactionConfig,
    pi_name: &str,
    model_id: &str,
    models_json: &Path,
) -> Option<ResolvedCompaction> {
    if !compaction.enabled {
        return None;
    }
    let key = format!("{pi_name}/{model_id}");
    let ov = compaction.overrides.get(&key);
    let window = ov
        .and_then(|o| o.context_window)
        .or_else(|| probe_context_window(models_json, pi_name, model_id))
        .unwrap_or(FALLBACK_CONTEXT_WINDOW);
    if window == 0 {
        // 显式声明 0 视为"不压缩"（与 pi 的语义一致：窗口 0 模型不参与）
        return None;
    }
    let watermark = ov.and_then(|o| o.watermark).unwrap_or(compaction.watermark);
    let ratio = ov
        .and_then(|o| o.keep_recent_ratio)
        .unwrap_or(compaction.keep_recent_ratio);
    let floor = ov
        .and_then(|o| o.keep_recent_floor)
        .unwrap_or(compaction.keep_recent_floor);
    Some(ResolvedCompaction::new(
        compaction.enabled,
        window,
        watermark,
        ratio,
        floor,
    ))
}

/// 便捷版：使用默认 models.json 路径探测。
pub fn resolve_for_model_with_default_path(
    compaction: &CompactionConfig,
    pi_name: &str,
    model_id: &str,
) -> Option<ResolvedCompaction> {
    resolve_for_model(compaction, pi_name, model_id, &pi_agent_dir().join("models.json"))
}

/// 模型覆盖中配置了 context_window 时，供 `sync_pi_models_json` 写入 models.json
/// （这样探测值与 pi 模型注册表一致，LLM 侧也能看到真实窗口）。
pub fn override_window_for_model(
    compaction: &CompactionConfig,
    pi_name: &str,
    model_id: &str,
) -> Option<u32> {
    let key = format!("{pi_name}/{model_id}");
    compaction.overrides.get(&key).and_then(|o| o.context_window)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(watermark: f64) -> CompactionConfig {
        CompactionConfig {
            enabled: true,
            watermark,
            ..Default::default()
        }
    }

    #[test]
    fn disabled_returns_none() {
        let c = CompactionConfig {
            enabled: false,
            ..Default::default()
        };
        assert_eq!(resolve_for_model(&c, "openai", "gpt-4o", Path::new("/nonexistent")), None);
    }

    #[test]
    fn window_falls_back_to_128k_when_undetected() {
        let r = resolve_for_model(&config(0.50), "openai", "gpt-4o", Path::new("/nonexistent"))
            .expect("resolved");
        assert_eq!(r.context_window, FALLBACK_CONTEXT_WINDOW);
        // 水线 50% → reserve = 窗口一半；尾部 = max(10% 窗口, 4000)
        assert_eq!(r.reserve_tokens, 64_000);
        assert_eq!(r.keep_recent_tokens, 12_800);
        assert!(r.enabled);
    }

    #[test]
    fn probe_reads_models_json_window() {
        let dir = std::env::temp_dir().join(format!("bm-compaction-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("models.json");
        std::fs::write(
            &path,
            r#"{"providers": {"deepseek": {"models": [
                {"id": "deepseek-chat", "contextWindow": 128000},
                {"id": "deepseek-reasoner"}
            ]}}}"#,
        )
        .unwrap();
        assert_eq!(
            probe_context_window(&path, "deepseek", "deepseek-chat"),
            Some(128_000)
        );
        // 未声明的模型探测不到 → 走兜底
        assert_eq!(probe_context_window(&path, "deepseek", "deepseek-reasoner"), None);
        let r = resolve_for_model(&config(0.50), "deepseek", "deepseek-chat", &path).unwrap();
        assert_eq!(r.context_window, 128_000);
        assert_eq!(r.reserve_tokens, 64_000);
        // 探测不到时兜底 128K（50% 水线同值；用非默认窗口验证更严谨的用例在下面）
        let _ = r;
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn override_beats_probe_and_global_defaults() {
        let dir = std::env::temp_dir().join(format!("bm-compaction-ovr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("models.json");
        std::fs::write(&path, r#"{"providers": {"m": {"models": [{"id": "x", "contextWindow": 32000}]}}}"#).unwrap();

        let mut c = config(0.50);
        c.overrides.insert(
            "m/x".to_string(),
            CompactionOverride {
                context_window: Some(1_000_000),
                watermark: Some(0.75),
                keep_recent_ratio: Some(0.05),
                keep_recent_floor: Some(2_000),
            },
        );
        let r = resolve_for_model(&c, "m", "x", &path).unwrap();
        // 覆盖窗口 1M > 探测 32K
        assert_eq!(r.context_window, 1_000_000);
        // 水线 75% → reserve = 25% = 250_000
        assert_eq!(r.reserve_tokens, 250_000);
        // 尾部 = max(5% × 1M = 50_000, 2_000)
        assert_eq!(r.keep_recent_tokens, 50_000);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn small_window_floor_kicks_in() {
        let dir = std::env::temp_dir().join(format!("bm-compaction-floor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("models.json");
        std::fs::write(&path, r#"{"providers": {"m": {"models": [{"id": "x", "contextWindow": 32000}]}}}"#).unwrap();
        let r = resolve_for_model(&config(0.50), "m", "x", &path).unwrap();
        // 10% × 32K = 3200 < 下限 4000 → 4000
        assert_eq!(r.keep_recent_tokens, 4_000);
        assert_eq!(r.reserve_tokens, 16_000);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn zero_window_declared_means_no_compaction() {
        let dir = std::env::temp_dir().join(format!("bm-compaction-zero-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("models.json");
        std::fs::write(&path, r#"{"providers": {"m": {"models": [{"id": "x", "contextWindow": 0}]}}}"#).unwrap();
        let mut c = config(0.50);
        c.overrides.insert(
            "m/x".to_string(),
            CompactionOverride { context_window: Some(0), ..Default::default() },
        );
        assert_eq!(resolve_for_model(&c, "m", "x", &path), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_ratios_fall_back_to_defaults() {
        let r = ResolvedCompaction::new(true, 128_000, f64::NAN, 0.0, 0);
        assert_eq!(r.reserve_tokens, 64_000); // NaN → 0.50
        assert_eq!(r.keep_recent_tokens, 12_800); // 0 → 0.10；floor 0 → 4000
    }

    #[test]
    fn override_window_written_to_models_json() {
        let mut c = config(0.50);
        c.overrides.insert(
            "d/chat".to_string(),
            CompactionOverride { context_window: Some(200_000), ..Default::default() },
        );
        assert_eq!(override_window_for_model(&c, "d", "chat"), Some(200_000));
        assert_eq!(override_window_for_model(&c, "d", "other"), None);
    }
}
