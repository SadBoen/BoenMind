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

    /// 审批承载级(reversible 及以上,与上一集合相同):Broker 裁决中,
    /// effective_risk 落在此集合即 RequireApproval——直通仅限
    /// read-only/low-risk(M4 规格 §5.4;trusted 直调 reversible+ 亦审批)。
    pub fn is_approval_bearing(self) -> bool {
        self.requires_approval_at_untrusted()
    }
}

// 数据信任分级(基线 §4.5;capability/approval 合同 input_trust 字段)。
// 声明面:随内容来源链传递,调用方不可自报降级(M4 规格 §5.4)。
wire_str_enum!(DataTrust {
    Trusted => "trusted",
    AgentDerived => "agent-derived",
    Untrusted => "untrusted",
});

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

/// 授权范围(基线 §9.6;grant 合同 scope pattern)。线上形态 = pattern 字符串,
/// 解析后承载语义值:Ttl 以毫秒存储(ms/s/m/h 归一),序列化统一 `ttl:<n>ms`。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GrantScope {
    Once,
    Forever,
    Count(u64),
    Ttl(u64),
    Task(String),
}

impl GrantScope {
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "once" => return Some(Self::Once),
            "forever" => return Some(Self::Forever),
            _ => {}
        }
        let (kind, val) = s.split_once(':')?;
        match kind {
            "count" => val.parse().ok().map(Self::Count),
            "task" => (!val.is_empty()).then(|| Self::Task(val.to_string())),
            "ttl" => {
                // 形态 ttl:<数字><ms|s|m|h>:找首个非数字字符切分数字与单位
                let digits = val.find(|c: char| !c.is_ascii_digit()).unwrap_or(val.len());
                let (num, unit) = val.split_at(digits);
                let n: u64 = num.parse().ok()?;
                match unit {
                    "ms" => Some(Self::Ttl(n)),
                    "s" => Some(Self::Ttl(n.saturating_mul(1_000))),
                    "m" => Some(Self::Ttl(n.saturating_mul(60_000))),
                    "h" => Some(Self::Ttl(n.saturating_mul(3_600_000))),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub fn to_wire(&self) -> String {
        match self {
            GrantScope::Once => "once".into(),
            GrantScope::Forever => "forever".into(),
            GrantScope::Count(n) => format!("count:{n}"),
            GrantScope::Ttl(ms) => format!("ttl:{ms}ms"),
            GrantScope::Task(id) => format!("task:{id}"),
        }
    }
}

impl Serialize for GrantScope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_wire())
    }
}

impl<'de> Deserialize<'de> for GrantScope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        GrantScope::from_wire(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("非法 scope: {s:?}")))
    }
}

/// 资源谓词(ADR-0002 条件 1 的下限实现:参数等值字典;缺省 = 全参授权)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantResource {
    pub capability: String,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub args_predicates: serde_json::Map<String, serde_json::Value>,
}

/// Capability Grant(Broker 记账载体;capability/grant.v0_1.schema.json 下限
/// 字段集,ADR-0002 条件 1)。M4 单路径期:由用户批准的 Approval 物化,
/// parent_grant_hash = Approval 对象 SHA-256;delegation_depth 恒 0。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub grant_id: String,
    pub audience: String,
    pub action: String,
    pub resource: GrantResource,
    pub scope: GrantScope,
    pub delegation_depth: u32,
    pub expires_at: Option<crate::BmTimestamp>,
    pub revocation_version: u64,
    pub parent_grant_hash: String,
    pub issued_by: String,
    pub created_at: crate::BmTimestamp,
}

// 审批状态机(capability/approval.v0_1.schema.json;基线 §9.6):
// requested → waiting_user → approved | denied;超时 → expired(等价 denied,
// 无超时默认同意);调用方取消 → withdrawn。
wire_str_enum!(ApprovalState {
    Requested => "requested",
    WaitingUser => "waiting_user",
    Approved => "approved",
    Denied => "denied",
    Expired => "expired",
    Withdrawn => "withdrawn",
});

/// Capability Approval(用户裁决载体,持久合同对象;基线 §9.6)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub approval_id: String,
    pub capability: String,
    /// args 规范化 JSON 的 SHA-256(A4:原文不进普通日志)。
    pub args_digest: String,
    /// Broker 生成的结构化脱敏摘要(审批卡片主体)。
    pub args_summary: String,
    pub principal: String,
    pub risk_class: RiskClass,
    pub effective_risk: RiskClass,
    pub input_trust: DataTrust,
    pub state: ApprovalState,
    /// 批准时用户可选择的授权范围(Broker 按 effective_risk 生成)。
    pub scope_choices: Vec<GrantScope>,
    pub requested_at: crate::BmTimestamp,
    /// 等待用户裁决的截止;到期 → expired(等价 denied,无超时默认同意)。
    pub expires_at: crate::BmTimestamp,
    #[serde(default)]
    pub resolved_at: Option<crate::BmTimestamp>,
    /// 批准后物化的 Grant 回填;其余状态为 null。
    #[serde(default)]
    pub grant_id: Option<String>,
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

    #[test]
    fn grant_scope_wire_roundtrip() {
        for (wire, scope) in [
            ("once", GrantScope::Once),
            ("forever", GrantScope::Forever),
            ("count:5", GrantScope::Count(5)),
            ("ttl:90s", GrantScope::Ttl(90_000)),
            ("ttl:5m", GrantScope::Ttl(300_000)),
            ("ttl:200ms", GrantScope::Ttl(200)),
            ("task:t1", GrantScope::Task("t1".into())),
        ] {
            assert_eq!(GrantScope::from_wire(wire).as_ref(), Some(&scope));
            assert_eq!(scope.to_wire(), {
                // 归一化形态:ttl 统一 ms;其余原样
                match &scope {
                    GrantScope::Ttl(ms) => format!("ttl:{ms}ms"),
                    _ => wire.to_string(),
                }
            });
            let back = GrantScope::from_wire(&scope.to_wire()).unwrap();
            assert_eq!(back, scope, "归一化形态必须稳定可解析");
        }
        for bad in ["count:", "ttl:5x", "task:", "whenever", "count:5x"] {
            assert!(GrantScope::from_wire(bad).is_none(), "{bad} 应被拒绝");
        }
    }

    #[test]
    fn approval_roundtrip_keeps_state_and_choices() {
        let a: Approval = serde_json::from_value(json!({
            "approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAA04",
            "capability": "system.danger.purge",
            "args_digest": "9b1dec3f2a6c47d5b8e0f1a2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a",
            "args_summary": "清除 notes 域全部内容(target=notes)",
            "principal": "surface:user",
            "risk_class": "high-risk-command",
            "effective_risk": "high-risk-command",
            "input_trust": "trusted",
            "state": "waiting_user",
            "scope_choices": ["once", "count:5", "ttl:1h"],
            "requested_at": "2026-08-29T10:00:00.220Z",
            "expires_at": "2026-08-29T10:05:00.220Z",
            "resolved_at": null,
            "grant_id": null
        }))
        .unwrap();
        assert_eq!(a.state, ApprovalState::WaitingUser);
        assert_eq!(
            a.scope_choices,
            vec![
                GrantScope::Once,
                GrantScope::Count(5),
                GrantScope::Ttl(3_600_000)
            ]
        );
        let ser = serde_json::to_value(&a).unwrap();
        assert_eq!(ser["state"], json!("waiting_user"));
        assert_eq!(ser["scope_choices"][2], json!("ttl:3600000ms"));
        let back: Approval = serde_json::from_value(ser).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn grant_serialization_matches_contract_shape() {
        let g: Grant = serde_json::from_value(json!({
            "grant_id": "grant_01JAAAAAAAAAAAAAAAAAAAAA0C",
            "audience": "agent:note_bot",
            "action": "system.notes.write",
            "resource": {"capability": "system.notes.write",
                         "args_predicates": {"path": "notes/inbox.md"}},
            "scope": "once",
            "delegation_depth": 0,
            "expires_at": "2026-08-29T10:30:00.000Z",
            "revocation_version": 0,
            "parent_grant_hash": "9b1dec3f2a6c47d5b8e0f1a2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a",
            "issued_by": "surface:user",
            "created_at": "2026-08-29T10:02:09.500Z"
        }))
        .unwrap();
        assert_eq!(g.scope, GrantScope::Once);
        // 空 args_predicates 不序列化(schema additionalProperties=false 下合法;
        // 且缺省即全参授权)
        let mut bare = g.clone();
        bare.resource.args_predicates.clear();
        let ser = serde_json::to_value(&bare).unwrap();
        assert!(ser["resource"].get("args_predicates").is_none());
        // delegation_depth 序列化在场(合同必填)
        assert_eq!(ser["delegation_depth"], json!(0));
    }
}
