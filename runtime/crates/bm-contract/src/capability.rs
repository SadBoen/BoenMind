//! Capability 合同镜像(capability/manifest.v0_1.schema.json,M4 增发)。
//!
//! manifest 是开放结构(additionalProperties: true):未知字段反序列化时
//! 被忽略、不失真(合同 README 消费方纪律)。风险五级与 safe/mutation
//! 分级是 Broker 裁决与 M5 协调动词过滤的输入(ADR-0002 条件 2)。
//!
//! **镜像 vs 策略**(ADR-0042 核实标注,供后续读者/评审区分):
//! - `RiskClass` 的**枚举值**(`read-only`…`high-risk-command`)是 manifest
//! 合同的镜像,由 `tests/sync.rs` 对比 schema 守护;
//! - `ORDER` / `escalated` / `requires_approval_at_untrusted` / `is_approval_bearing`
//! 是**策略**(来源 = 基线 §5.3 与 ADR-0002 条件 3,**不是** contract JSON 字段),
//! 故不在 sync 守护范围,由本模块单测守护其行为。它们留在本层是刻意的:
//! `ORDER` 是 `RiskClass` 的固有次序,三者皆为纯函数、无外部状态;迁出须改
//! 自由函数而失去方法语法(孤儿规则),得不偿失——评审勿再当"契约不纯"重提。

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

/// 自动重试策略(manifest #/definitions/retry_policy)。【诚实化
/// 预留字段,Broker 当前不消费——全仓零读取点,真实重试走回合层
/// limits.model_max_attempts 模型降级链(ADR-0028);兑现或移除待拍板
/// (GitHub issue)。设计意向仍是仅 read-only 与 low-risk-command 允许
/// 自动重试(基线 §5.2)。】
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub retry_on: Vec<RetryableError>,
}

/// ADR-0036:执行分道声明(合同 Minor)。`async` = 慢外部/沙箱执行体,
/// 由运行期异步执行器承载;`sync` = 进程内快能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    Sync,
    Async,
}

/// ADR-0038:单条主体系留规则(合同 Minor)。命中 `principal_prefix` 后,取
/// 其余段按 `drawer_prefix` 拼出主体自有抽屉标签。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawerRule {
    pub principal_prefix: String,
    pub drawer_prefix: String,
}

/// ADR-0038:记忆抽屉式授权声明(`authorization.drawer`)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawerAuthorization {
 /// 按序匹配(具体前缀在前);命中即得主体自有抽屉标签。
 #[serde(default)]
    pub self_drawers: Vec<DrawerRule>,
 /// read-only 能力可额外放行的 scope(读不产生内容污染)。
 #[serde(default)]
    pub read_allow_scopes: Vec<String>,
}

/// ADR-0038:per-capability 授权规则声明(Broker 只做解释,ADR-0006)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationRule {
 #[serde(default)]
    pub drawer: Option<DrawerAuthorization>,
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
 /// ADR-0022 增发(合同 Minor):面向模型的一句功能描述;对话工具清单
 /// 展示用,缺省 = turn 侧兜底,不影响审批语义。
 #[serde(default)]
    pub description: Option<String>,
 /// ADR-0036 增发(合同 Minor):执行分道声明,唯一真源;缺省 = 回退
 /// provider 命名约定(mcp./skill./*.async)兼容旧 manifest。
 #[serde(default)]
    pub execution_mode: Option<ExecutionMode>,
 /// ADR-0038 增发(合同 Minor):授权规则声明(Broker 只解释);缺省 =
 /// 该步不适用,走既有审批/直通流。
 #[serde(default)]
    pub authorization: Option<AuthorizationRule>,
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

/// 能力 manifest 合成规格(ADR-0051):**全部 provider 族单一构造路径**。
/// 构造器、各自重抄缺省值,已出现漂移(如 `skill_default_timeout_ms` 被实现
/// 忽略、settings 页空转)。字段缺省与必填集在此一处定义;各族只声明差异。
/// 用法:
/// ```
/// use bm_contract::capability::{ManifestSpec, RiskClass, ApprovalRequirement, ExecutionMode};
/// let m = ManifestSpec::new("demo.echo", "demo.wasm", RiskClass::ReadOnly)
/// .timeout_ms(5_000)
/// .approval(ApprovalRequirement::NotRequired)
/// .execution_mode(ExecutionMode::Async)
/// .scopes(vec!["domain:demo".into()])
/// .build()
/// .expect("合法");
/// assert_eq!(m.capability, "demo.echo");
/// ```
#[derive(Debug, Clone)]
pub struct ManifestSpec {
    capability: String,
    provider: String,
    effect: RiskClass,
    version: String,
    idempotent: bool,
    cancellable: bool,
    timeout_ms: u64,
    approval: ApprovalRequirement,
    input_schema: serde_json::Value,
    output_schema: serde_json::Value,
    scopes: Vec<String>,
    execution_mode: Option<ExecutionMode>,
    description: Option<String>,
 /// 开放结构叠加(ADR-0051):额外字段最后合并;未知字段由合同忽略。
    overlay: Option<serde_json::Value>,
}

/// 缺省字段集(与 manifest.v0_1 必填十项 + 常用可选一致):
/// version `0.1.0`、in/out schema `{"type":"object"}`、idempotent=false、
/// cancellable=true、timeout 10s、approval=not-required、scopes 空。
impl ManifestSpec {
    pub fn new(
        capability: impl Into<String>,
        provider: impl Into<String>,
        effect: RiskClass,
    ) -> Self {
        Self {
            capability: capability.into(),
            provider: provider.into(),
            effect,
            version: "0.1.0".into(),
            idempotent: false,
            cancellable: true,
            timeout_ms: 10_000,
            approval: ApprovalRequirement::NotRequired,
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: serde_json::json!({"type": "object"}),
            scopes: Vec::new(),
            execution_mode: None,
            description: None,
            overlay: None,
        }
    }

    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = v.into();
        self
    }
    pub fn idempotent(mut self, v: bool) -> Self {
        self.idempotent = v;
        self
    }
    pub fn cancellable(mut self, v: bool) -> Self {
        self.cancellable = v;
        self
    }
    pub fn timeout_ms(mut self, v: u64) -> Self {
        self.timeout_ms = v;
        self
    }
    pub fn approval(mut self, v: ApprovalRequirement) -> Self {
        self.approval = v;
        self
    }
    pub fn input_schema(mut self, v: serde_json::Value) -> Self {
        self.input_schema = v;
        self
    }
    pub fn output_schema(mut self, v: serde_json::Value) -> Self {
        self.output_schema = v;
        self
    }
    pub fn scopes(mut self, v: Vec<String>) -> Self {
        self.scopes = v;
        self
    }
    pub fn execution_mode(mut self, v: ExecutionMode) -> Self {
        self.execution_mode = Some(v);
        self
    }
    pub fn description(mut self, v: impl Into<String>) -> Self {
        self.description = Some(v.into());
        self
    }
 /// 开放结构叠加:额外字段(undo/verification/… )最后合并进 manifest。
    pub fn overlay(mut self, v: serde_json::Value) -> Self {
        self.overlay = Some(v);
        self
    }

 /// 合成 manifest。唯一失败源 = 叠加字段与合同冲突(类型不符)。
    pub fn build(self) -> Result<CapabilityManifest, String> {
        let mut v = serde_json::json!({
            "capability": self.capability,
            "provider": self.provider,
            "version": self.version,
            "input_schema": self.input_schema,
            "output_schema": self.output_schema,
            "effect": self.effect,
            "idempotent": self.idempotent,
            "cancellable": self.cancellable,
            "timeout_ms": self.timeout_ms,
            "approval": self.approval,
            "scopes": self.scopes,
        });
        if let Some(mode) = self.execution_mode {
            v["execution_mode"] = serde_json::json!(mode);
        }
        if let Some(d) = &self.description {
            v["description"] = serde_json::json!(d);
        }
 // 叠加最后应用(可覆盖上方任一字段,与旧 json!-merge 语义一致)。
        if let (Some(obj), Some(extra)) = (
            v.as_object_mut(),
            self.overlay.as_ref().and_then(|o| o.as_object()),
        ) {
            for (k, val) in extra {
                obj.insert(k.clone(), val.clone());
            }
        }
        serde_json::from_value(v).map_err(|e| format!("manifest 合成失败: {e}"))
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
 /// 裁决来源(ADR-0030,Minor 只增):user=人工裁决;mode_auto=会话 yolo
 /// 模式服务端自动放行;system=系统自裁(审批等待超时撤销)。等待中缺省。
 #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_source: Option<String>,
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

 // ADR-0042 核实补测:风险升级/审批策略**不是契约镜像**(无对应 JSON 字段),
 // 故由本模块单测守护其行为——防止"策略漂移"无门可拦。
 #[test]
    fn risk_escalation_and_approval_policy_is_pinned() {
 // 上提一级(低→高),封顶 high-risk。
        assert_eq!(RiskClass::ReadOnly.escalated(), RiskClass::LowRiskCommand);
        assert_eq!(
            RiskClass::HighRiskCommand.escalated(),
            RiskClass::HighRiskCommand,
            "封顶:high-risk 上提仍是自身"
        );
 // 审批承载级 = reversible 及以上(基线 §5.3 / ADR-0002 条件 3)。
        for r in [RiskClass::ReadOnly, RiskClass::LowRiskCommand] {
            assert!(!r.is_approval_bearing(), "{r:?} 不应审批");
            assert!(!r.requires_approval_at_untrusted());
        }
        for r in [
            RiskClass::ReversibleCommand,
            RiskClass::ExternalSideEffect,
            RiskClass::HighRiskCommand,
        ] {
            assert!(r.is_approval_bearing(), "{r:?} 必须审批");
            assert!(r.requires_approval_at_untrusted());
        }
 // ORDER 与枚举值序一致(低→高),escalated 即在其上右移一格。
        assert_eq!(RiskClass::ORDER.len(), 5);
        assert_eq!(RiskClass::ORDER[0], RiskClass::ReadOnly);
        assert_eq!(RiskClass::ORDER[4], RiskClass::HighRiskCommand);
    }
}
