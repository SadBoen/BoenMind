//! INV-5 凭据脱敏共享原语。
//!
//! ContextLog / ExecutionLog / TurnDebugLog 三个落盘面共用同一套「注册扫描面 →
//! 整串扫描 → 命中替换」语义。
//! 容易漂移;收拢到本模块作为唯一实现。

use std::collections::BTreeSet;

/// 扫描命中后的替换占位符(三面同字面量)。
pub(crate) const PLACEHOLDER: &str = "[REDACTED]";

/// 注册凭据明文进扫描面。
/// 过短的值误报率高,不入面(6 字符阈值三面同口径);同时登记 JSON 转义形态——
/// 序列化后凭据里的 `"` / `\` 会转义,纯明文 `contains` 永不命中。
pub(crate) fn register(set: &mut BTreeSet<String>, value: &str) {
    if value.len() < 6 {
        return;
    }
    set.insert(value.to_string());
    if let Ok(esc) = serde_json::to_string(value) {
        let trimmed = esc.trim_matches('"').to_string();
        if trimmed != value {
            set.insert(trimmed);
        }
    }
}

/// 明文扫描:命中任一凭据即整体替换为 [`PLACEHOLDER`],返回脱敏后的串。
pub(crate) fn redact(set: &BTreeSet<String>, text: &str) -> String {
    let mut out = text.to_string();
    for secret in set {
        if out.contains(secret.as_str()) {
            out = out.replace(secret.as_str(), PLACEHOLDER);
        }
    }
    out
}

/// 是否仍含任一凭据明文(脱敏复扫 / fail-closed 判定用)。
pub(crate) fn contains_any(set: &BTreeSet<String>, text: &str) -> bool {
    set.iter().any(|s| text.contains(s.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

 #[test]
    fn register_skips_short_values_and_adds_escaped_form() {
        let mut set = BTreeSet::new();
        register(&mut set, "short");
        assert!(set.is_empty(), "短值不入扫描面");
        register(&mut set, "sk-t\"quote\\back-123456");
        assert!(set.contains("sk-t\"quote\\back-123456"));
        assert!(
            set.iter().any(|s| s != "sk-t\"quote\\back-123456"),
            "应登记 JSON 转义形态"
        );
    }

 #[test]
    fn redact_replaces_every_hit_and_contains_any_tracks_residual() {
        let mut set = BTreeSet::new();
        register(&mut set, "sk-aaaaaa");
        register(&mut set, "sk-bbbbbb");
        let raw = "k1=sk-aaaaaa k2=sk-bbbbbb";
        let out = redact(&set, raw);
        assert!(!contains_any(&set, &out), "脱敏后不得再命中");
        assert_eq!(out, "k1=[REDACTED] k2=[REDACTED]");
        assert!(contains_any(&set, raw), "原文命中");
    }
}
