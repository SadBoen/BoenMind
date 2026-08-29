//! Capability 合同镜像(capability/manifest.v0_1.schema.json,M4 增发)。
//!
//! manifest 是开放结构(additionalProperties: true):未知字段反序列化时
//! 被忽略、不失真(合同 README 消费方纪律)。风险五级与 safe/mutation
//! 分级是 Broker 裁决与 M5 协调动词过滤的输入(ADR-0002 条件 2)。

use serde::{Deserialize, Serialize};

wire_str_enum!(RiskClass {
    ReadOnly => "read-only",
    LowRiskCommand => "low-risk-command",
    ReversibleCommand => "reversible-command",
    ExternalSideEffect => "external-side-effect",
    HighRiskCommand => "high-risk-command",
});

impl RiskClass {
    /// 风险序全量(低 → 高)。
    pub const ORDER: [RiskClass; 5] = [
        RiskClass::ReadOnly,
        RiskClass::LowRiskCommand,
        RiskClass::ReversibleCommand,
        RiskClass::ExternalSideEffect,
        RiskClass::HighRiskCommand,
    ];

    /// untrusted 来源按风险序上提一级(基线 §5.3/§4.5;封顶 high-risk)。
    pub fn escalated(self) -> RiskClass {
        let idx = Self::ORDER.iter().position(|r| *r == self).unwrap_or(0);
        Self::ORDER[(idx + 1).min(Self::ORDER.len() - 1)]
    }

    /// reversible-command 及以上:untrusted 门控下强制审批(ADR-0002 条件 3)。
    pub fn requires_approval_at_untrusted(self) -> bool {
        matches!(
            self,
            RiskClass::ReversibleCommand
                | RiskClass::ExternalSideEffect
                | RiskClass::HighRiskCommand
        )
    }
}

wire_str_enum!(MutationClass {
    Safe => "safe",
    Mutation => "mutation",
});

wire_str_enum!(ApprovalRequirement {
    NotRequired => "not-required",
    Required => "required",
});

wire_str_enum!(RetryableError {
    Timeout => "timeout",
    Unavailable => "unavailable",
});

/// 自动重试策略(manifest #/definitions/retry_policy):由 Broker 统一执行;
/// 仅 read-only 与 low-risk-command 允许自动重试(基线 §5.2)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub retry_on: Vec<RetryableError>,
}

/// Capability Manifest(基线 §5.2 十必填全量 + M4 增发 mutation_class)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub capability: String,
    pub provider: String,
    pub version: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub effect: RiskClass,
    pub idempotent: bool,
    pub cancellable: bool,
    pub timeout_ms: u64,
    pub approval: ApprovalRequirement,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub verification: Option<serde_json::Value>,
    #[serde(default)]
    pub undo: Option<serde_json::Value>,
    #[serde(default)]
    pub retry: Option<RetryPolicy>,
    #[serde(default)]
    pub deprecated_by: Option<String>,
    /// M4 增发:safe/mutation 分级;缺省由 effect 派生(合同 description)。
    #[serde(default)]
    pub mutation_class: Option<MutationClass>,
}

impl CapabilityManifest {
    /// 显式声明优先,否则按 effect 派生:read-only→safe,其余→mutation。
    pub fn mutation_class_or_derived(&self) -> MutationClass {
        self.mutation_class.unwrap_or(match self.effect {
            RiskClass::ReadOnly => MutationClass::Safe,
            _ => MutationClass::Mutation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> CapabilityManifest {
        serde_json::from_value(json!({
            "capability": "system.echo",
            "provider": "system.echo",
            "version": "0.1.0",
            "input_schema": {"type": "object"},
            "output_schema": {"type": "object"},
            "effect": "read-only",
            "idempotent": true,
            "cancellable": true,
            "timeout_ms": 1000,
            "approval": "not-required",
            "scopes": ["system.echo"],
            "retry": {"max_attempts": 1, "backoff_ms": 100, "retry_on": ["timeout"]}
        }))
        .unwrap()
    }

    #[test]
    fn manifest_deserializes_and_derives_mutation_class() {
        let m = sample();
        assert_eq!(m.capability, "system.echo");
        assert_eq!(m.effect, RiskClass::ReadOnly);
        // 未声明 mutation_class → 由 effect 派生 safe
        assert_eq!(m.mutation_class_or_derived(), MutationClass::Safe);

        let mut m2 = sample();
        m2.effect = RiskClass::ReversibleCommand;
        assert_eq!(m2.mutation_class_or_derived(), MutationClass::Mutation);
    }

    #[test]
    fn unknown_manifest_fields_are_ignored_open_structure() {
        // 开放结构:未知字段被忽略(合同 README:消费方必须忽略不认识的字段)
        let v = json!({
            "capability": "system.echo", "provider": "system.echo",
            "version": "0.1.0", "input_schema": {}, "output_schema": {},
            "effect": "read-only", "idempotent": true, "cancellable": true,
            "timeout_ms": 1, "approval": "not-required",
            "future_extension": {"anything": true}
        });
        let m: CapabilityManifest = serde_json::from_value(v).unwrap();
        assert_eq!(m.capability, "system.echo");
    }

    #[test]
    fn risk_escalation_and_untrusted_approval_matrix() {
        // 上提一级:read-only→low-risk;封顶 high-risk 不再上提
        assert_eq!(RiskClass::ReadOnly.escalated(), RiskClass::LowRiskCommand);
        assert_eq!(
            RiskClass::LowRiskCommand.escalated(),
            RiskClass::ReversibleCommand
        );
        assert_eq!(
            RiskClass::HighRiskCommand.escalated(),
            RiskClass::HighRiskCommand
        );
        // untrusted 门控:reversible 及以上 100% 升级(ADR-0002 条件 3)
        assert!(!RiskClass::ReadOnly.requires_approval_at_untrusted());
        assert!(!RiskClass::LowRiskCommand.requires_approval_at_untrusted());
        assert!(RiskClass::ReversibleCommand.requires_approval_at_untrusted());
        assert!(RiskClass::ExternalSideEffect.requires_approval_at_untrusted());
        assert!(RiskClass::HighRiskCommand.requires_approval_at_untrusted());
    }
}
