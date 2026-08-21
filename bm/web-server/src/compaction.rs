//! 上下文压缩「设置后背」实现（web-server 装配）：Compactor 策略参数从
//! settings.compaction ns **现场锁读**——设置页改参数后下一回合即生效
//! （无需重启，对齐 SettingsWorkdir 现读 host.workdir 的既有模式）。
//!
//! 实现取舍：`maybe_compact` 事务（选区间/摘要/失败回落）复用 trait 默认实现
//! （bm-ports 契约层统一执行），本 impl 只覆盖三个策略方法 + summarize_request
//! 转发——loop 零改动，装配面与 approval/workdir 完全同构（守卫规则 3 兼容）。

use std::sync::Arc;

use bm_ports::Compactor;
use serde_json::Value;

use crate::api::AppState;

/// 设置字段名（settings.compaction ns 的 key，与 config.toml [compaction] 同词）。
pub(crate) const CFG_ENABLED: &str = "enabled";
pub(crate) const CFG_WATERMARK: &str = "watermark";
pub(crate) const CFG_KEEP_RECENT_RATIO: &str = "keepRecentRatio";
pub(crate) const CFG_KEEP_RECENT_FLOOR: &str = "keepRecentFloor";
pub(crate) const CFG_MIN_MIDDLE_TOKENS: &str = "minMiddleTokens";

/// 设置后背压缩器：策略参数每次现读 settings.compaction，缺省回落
/// [`DefaultCompactor`] 内置默认（0.5 / 10% / 4000 / 512）。
pub struct SettingsBackedCompactor {
    state: Arc<AppState>,
    /// 内置落地策略（用于缺省回落 + summarize_request 委托）。
    fallback: bm_assembly::DefaultCompactor,
}

impl std::fmt::Debug for SettingsBackedCompactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsBackedCompactor").finish_non_exhaustive()
    }
}

impl SettingsBackedCompactor {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            fallback: bm_assembly::DefaultCompactor::default(),
        }
    }

    /// 现场锁读当前生效策略（settings.compaction 优先，缺省回落内置默认）。
    fn live(&self) -> bm_assembly::DefaultCompactor {
        let ns = self.state.settings.lock().unwrap();
        let value = ns.get("compaction").cloned().unwrap_or_default();
        drop(ns);
        let def = self.fallback.clone();
        bm_assembly::DefaultCompactor {
            watermark: num(&value, CFG_WATERMARK)
                .and_then(|v| v.as_f64())
                .unwrap_or(def.watermark),
            keep_recent_ratio: num(&value, CFG_KEEP_RECENT_RATIO)
                .and_then(|v| v.as_f64())
                .unwrap_or(def.keep_recent_ratio),
            keep_recent_floor: num(&value, CFG_KEEP_RECENT_FLOOR)
                .and_then(|v| v.as_u64())
                .unwrap_or(def.keep_recent_floor),
            min_middle_tokens: num(&value, CFG_MIN_MIDDLE_TOKENS)
                .and_then(|v| v.as_u64())
                .unwrap_or(def.min_middle_tokens),
        }
    }
}

fn num<'a>(value: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| v.is_number())
}

/// 设置面 enabled 开关：显式 false = 关闭压缩（即使装配了也不压）。
fn live_enabled(state: &AppState) -> bool {
    let ns = state.settings.lock().unwrap();
    match ns.get("compaction").and_then(|v| v.get(CFG_ENABLED)) {
        Some(Value::Bool(b)) => *b,
        _ => true, // 缺省 = 开启（装配 `--compact` 即表示要压）
    }
}

impl Compactor for SettingsBackedCompactor {
    fn should_compact(&self, input_tokens: u64, context_window: u64) -> bool {
        if !live_enabled(&self.state) {
            return false;
        }
        self.live().should_compact(input_tokens, context_window)
    }

    fn keep_recent_tokens(&self, context_window: u64) -> u64 {
        self.live().keep_recent_tokens(context_window)
    }

    fn min_middle_tokens(&self) -> u64 {
        self.live().min_middle_tokens()
    }

    fn summarize_request(&self, provider: &str, model: &str, dialogue: &str) -> kernel_contracts::llm::GenerateOptions {
        // 摘要请求构造（prompt 策略自治）委托内置落地——摘要本身就应稳定。
        self.fallback.summarize_request(provider, model, dialogue)
    }
}