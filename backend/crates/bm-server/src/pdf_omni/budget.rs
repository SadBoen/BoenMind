//! LlamaParse 多 key 串行预算账本。
//!
//! 策略（2026-08 用户决策，与 Hermes pdf-omni 一致）：
//! - 每把 key 有安全预算 `budget_per_key`（默认 9500 credits = 1 万额度的 95%，
//!   纯插件场景最大化利用；8000 已弃用）
//! - **串行使用**：按配置顺序先用第一把，达预算线后主动切第二把，全部达预算
//!   才报错——单个账号连续消耗自己的额度，行为接近正常用户；交替轮换会被
//!   风控识别为多账号共享（已弃用）
//! - **任务前精确检查**：选 key 时要求 用量 + 本次任务估算(页数×费率) ≤ 预算，
//!   大任务自动切下一把或报错，不会单任务越线撞 402
//! - 402 仅作意外兜底（如其他端并发消耗）：触发后该 key 标记到预算线
//!
//! 账本持久化 `~/.boenmind/pdf-omni/budget.json`（key 用 FNV 哈希标识，不落明文）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 每把 key 的安全预算（credits）
pub const DEFAULT_BUDGET_PER_KEY: u64 = 9500;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetFile {
    /// key 哈希 → 已用 credits
    pub usage: HashMap<String, u64>,
}

pub struct BudgetLedger {
    budget_per_key: u64,
    usage: HashMap<String, u64>,
    path: PathBuf,
}

impl BudgetLedger {
    /// 从 `~/.boenmind/pdf-omni/budget.json` 加载（不存在 → 空账本）。
    pub fn load(app_dir: &Path, budget_per_key: u64) -> Self {
        let path = app_dir.join("pdf-omni").join("budget.json");
        let usage = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<BudgetFile>(&s).ok())
            .map(|f| f.usage)
            .unwrap_or_default();
        BudgetLedger {
            budget_per_key,
            usage,
            path,
        }
    }

    /// 串行选 key：按配置顺序取第一个放得下本次任务的 key（不轮换）。
    /// 返回 (key 下标, key 明文)；全部放不下返回 None。
    pub fn pick_key(&self, tokens: &[String], task_credits: u64) -> Option<(usize, String)> {
        for (i, t) in tokens.iter().enumerate() {
            if t.trim().is_empty() {
                continue;
            }
            if self.usage_of(t) + task_credits <= self.budget_per_key {
                return Some((i, t.clone()));
            }
        }
        None
    }

    /// 用量累加（本地估算，用于主动轮换）并落盘。
    pub fn record_usage(&mut self, token: &str, credits: u64) -> u64 {
        let key = key_id(token);
        let used = self.usage.get(&key).copied().unwrap_or(0) + credits;
        self.usage.insert(key, used);
        self.save();
        used
    }

    /// 意外 402：该 key 标记到预算线，后续不再选。
    pub fn mark_exhausted(&mut self, token: &str) {
        self.usage.insert(key_id(token), self.budget_per_key);
        self.save();
    }

    pub fn usage_of(&self, token: &str) -> u64 {
        self.usage.get(&key_id(token)).copied().unwrap_or(0)
    }

    pub fn budget_per_key(&self) -> u64 {
        self.budget_per_key
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent()
            && std::fs::create_dir_all(parent).is_ok()
        {
            let file = BudgetFile {
                usage: self.usage.clone(),
            };
            if let Ok(text) = serde_json::to_string_pretty(&file) {
                let _ = std::fs::write(&self.path, text);
            }
        }
    }
}

/// FNV-1a 64 位哈希（key 标识，不落明文；无外部依赖）。
fn key_id(token: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in token.trim().as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger(dir: &Path) -> BudgetLedger {
        BudgetLedger::load(dir, 9500)
    }

    #[test]
    fn pick_serial_prefers_first_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = ledger(dir.path());
        let tokens = vec!["k1".to_string(), "k2".to_string()];
        assert_eq!(l.pick_key(&tokens, 100).unwrap().0, 0);
        l.record_usage("k1", 9450);
        // k1 只剩 50，放不下 100 → 切 k2
        assert_eq!(l.pick_key(&tokens, 100).unwrap().0, 1);
    }

    #[test]
    fn pick_rejects_oversized_task() {
        let dir = tempfile::tempdir().unwrap();
        let l = ledger(dir.path());
        let tokens = vec!["k1".to_string()];
        assert!(l.pick_key(&tokens, 9600).is_none());
    }

    #[test]
    fn pick_skips_empty_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let l = ledger(dir.path());
        let tokens = vec!["".to_string(), "k2".to_string()];
        assert_eq!(l.pick_key(&tokens, 10).unwrap().0, 1);
    }

    #[test]
    fn exhausted_key_not_picked_until_other_runs_out() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = ledger(dir.path());
        l.mark_exhausted("k1");
        let tokens = vec!["k1".to_string(), "k2".to_string()];
        assert_eq!(l.pick_key(&tokens, 10).unwrap().0, 1);
        l.record_usage("k2", 9495);
        assert!(l.pick_key(&tokens, 10).is_none());
    }

    #[test]
    fn ledger_persists_across_reload() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut l = ledger(dir.path());
            l.record_usage("k1", 100);
        }
        let l = ledger(dir.path());
        assert_eq!(l.usage_of("k1"), 100);
        assert_eq!(l.usage_of("k2"), 0);
    }

    #[test]
    fn key_id_stable_and_salted() {
        let a = key_id("llx-abc");
        let b = key_id("llx-abc");
        let c = key_id("llx-abc ");
        assert_eq!(a, b);
        assert_eq!(a, c); // trim
        assert_ne!(a, key_id("llx-other"));
    }
}
