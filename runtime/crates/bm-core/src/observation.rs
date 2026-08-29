//! Observation 与完成判定(M5.6,基线 §8.4/§20)。
//!
//! 「Agent 声称完成」与「系统实际观察到」的对照落在此处:Worker 声称任务
//! 完成 → 消费其执行能力的 manifest verification 钩子(query/expect)做
//! 确定性核验——外部系统查询、收据与确定性断言优先于模型自述。verdict:
//! - verified:证据支持声称 → Task completed(verified_completion);
//! - unverified:无可核验结果 → Task blocked(outcome_unknown_pending)
//!   等用户裁定,禁止自动标成功(完成判定门禁,基线 M5 通过条件第 4 条)。
//!
//! 独立 Judge(M8.7 起)接口预留不实现;M5 判定全确定性。

use sha2::{Digest, Sha256};

/// expect 语义的确定性判定:
/// - "exists"(或空串):查询结果非空、无 error 字段;
/// - 其它:结果 JSON 的字符串形态包含 expect 子串。
///
/// 查询执行失败(能力错误/超时)不等于 expect 不满足——返回 None(证据
/// 不可得 = unverified,而非 failed)。
pub fn expect_satisfied(result: &serde_json::Value, expect: &str) -> Option<bool> {
    // 收据形态:error 显式非 null 才是错误(null = 无错误)
    let has_error = result.get("error").map(|e| !e.is_null()).unwrap_or(false);
    if has_error || result.get("ok") == Some(&serde_json::json!(false)) {
        return Some(false);
    }
    let nonempty = match result {
        serde_json::Value::Null => false,
        serde_json::Value::Object(o) => !o.is_empty(),
        serde_json::Value::Array(a) => !a.is_empty(),
        serde_json::Value::String(s) => !s.is_empty(),
        _ => true,
    };
    let expect = expect.trim();
    if expect.is_empty() || expect == "exists" {
        return Some(nonempty);
    }
    Some(
        nonempty
            && serde_json::to_string(result)
                .unwrap_or_default()
                .contains(expect),
    )
}

/// Observation Log 条目(logs/observation-log-entry.v0.1 合同形态;
/// 四类记录合同的第二块,与 execution-log-entry 同构落盘)。
#[derive(Debug, Clone)]
pub struct ObservationEntry {
    pub log_seq: u64,
    pub task_id: String,
    pub agent_id: Option<String>,
    pub operation_id: Option<String>,
    pub claim_summary: String,
    /// (kind, ref)
    pub evidence: Vec<(String, String)>,
    pub verdict: &'static str,
    pub guard_state: &'static str,
    pub observed_at: String,
}

impl ObservationEntry {
    pub fn to_contract_json(&self) -> String {
        serde_json::json!({
            "log_seq": self.log_seq,
            "task_id": self.task_id,
            "agent_id": self.agent_id,
            "operation_id": self.operation_id,
            "claim_summary": self.claim_summary,
            "evidence": self
                .evidence
                .iter()
                .map(|(kind, r)| serde_json::json!({"kind": kind, "ref": r}))
                .collect::<Vec<_>>(),
            "verdict": self.verdict,
            "guard_state": self.guard_state,
            "verification_hook": null,
            "observed_at": self.observed_at,
        })
        .to_string()
    }
}

/// 声称摘要哈希(证据链辅助;claim 原文入受保护引用)。
pub fn claim_digest(claim: &str) -> String {
    let mut h = Sha256::new();
    h.update(claim.as_bytes());
    let out = h.finalize();
    let mut hex = String::with_capacity(64);
    for b in out {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expect_exists_requires_nonempty_result() {
        assert_eq!(
            expect_satisfied(&json!({"written": true}), "exists"),
            Some(true)
        );
        assert_eq!(expect_satisfied(&json!({}), "exists"), Some(false));
        assert_eq!(
            expect_satisfied(&serde_json::Value::Null, "exists"),
            Some(false)
        );
        // 错误形态 = 不满足(确定性失败,非证据不可得)
        assert_eq!(
            expect_satisfied(&json!({"error": {"code": "timeout"}}), "exists"),
            Some(false)
        );
    }

    #[test]
    fn expect_substring_is_containment_check() {
        assert_eq!(
            expect_satisfied(&json!({"content": "归档摘要:关于幂等性"}), "幂等性"),
            Some(true)
        );
        assert_eq!(
            expect_satisfied(&json!({"content": "无关内容"}), "幂等性"),
            Some(false)
        );
    }

    #[test]
    fn claim_digest_is_stable_hex() {
        let a = claim_digest("声称完成");
        let b = claim_digest("声称完成");
        let c = claim_digest("声称未完成");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }
}
