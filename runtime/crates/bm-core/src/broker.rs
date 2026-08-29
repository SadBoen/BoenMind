//! Capability Broker(M4.2,基线 §7;ADR-0001 条件 1/2/4、ADR-0002 条件 3)。
//!
//! 所有跨域调用的统一裁决入口。策略以「调用方×目标 Capability」O(1) 查表
//! 命中(GrantLedger 的 (audience, action) 索引 = 编译产物,签发/撤销时增量
//! 重编译并递增 policy_version),读取路径只做查表 + 常量级字段校验(过期/
//! 计数/撤回版本/谓词),禁止逐条策略求值(规格 §5.1)。
//!
//! 七步管线(身份→权限→scope→参数校验→绑定→执行→审计)实现为 [`Broker::call`]
//! 内的私有分段函数,非运行时串行管线。
//!
//! 本模块是纯决策/执行组件,**不发事件**:审计事件(capability.invoked/denied)
//! 由 Runtime 核心循环单写者落盘(bm-core 契约;T3/T5 接线)。数据面 lease
//! 准入四测试的①②两项在此层断言,③④随 T6/T8。
//!
//! Grant 消费语义(留档,回看复核):Once/Count 在执行**前**预扣——执行失败
//! 不退还授权次数(授权=一次执行机会,保守面);真实副作用场景的幂等键与
//! outbox 对账随 T6,不依赖消费退还。

use crate::clock::Clock;
use crate::registry::{BindingStatus, CapabilityRegistry, RegistryError};
use bm_contract::capability::{
    ApprovalRequirement, CapabilityManifest, DataTrust, Grant, GrantResource, GrantScope, RiskClass,
};
use bm_contract::ids::IdGen;
use bm_contract::timestamp::{format_ts, parse_ts};
use std::collections::HashMap;
use std::sync::Arc;

/// 调用上下文:身份与信任级别随内容来源链传递(M4 规格 §5.4;
/// Wire Surface 直调恒 trusted,客户端无 trust 参数面)。
#[derive(Debug, Clone)]
pub struct CallContext {
    /// kind:local-id(如 surface:user / agent:note_bot)。
    pub principal: String,
    pub trust: DataTrust,
    pub idempotency_key: Option<String>,
}

impl CallContext {
    pub fn new(principal: &str, trust: DataTrust) -> Self {
        Self {
            principal: principal.to_string(),
            trust,
            idempotency_key: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    /// capability 未注册(或 binding 已不存在):默认拒绝,无审批出口。
    UnknownCapability,
    /// 无 Grant 且不满足直通:默认拒绝(ADR-0006)。
    NoGrant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// grant_id = None 表示内建直通(trusted × not-required × read-only/low-risk)。
    Allowed {
        grant_id: Option<String>,
    },
    RequireApproval {
        risk_class: RiskClass,
        effective_risk: RiskClass,
    },
    Denied {
        reason: DenyReason,
    },
}

/// 授权决策点固化的调用凭证(ADR-0001 条件 2):Provider 侧执行前校验,
/// binding 切换后旧凭证失效,在途归属仍由凭证中的 epoch 保全。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CallCredential {
    pub call_id: String,
    pub capability: String,
    pub binding_epoch: u64,
    pub provider_instance_id: String,
    pub principal: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallOutcome {
    Completed {
        call_id: String,
        grant_id: Option<String>,
        credential: CallCredential,
        result: serde_json::Value,
    },
    Rejected {
        decision: Decision,
    },
    InvalidArgs {
        message: String,
    },
    InvalidOutput {
        message: String,
    },
    StaleBinding {
        expected_epoch: u64,
        current_epoch: u64,
    },
    ProviderError {
        message: String,
    },
}

/// 数据面通道凭证(ADR-0001 条件 4;capability/lease 合同)。瞬态结构,
/// 不落盘、不占 L2 单写者。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Lease {
    pub lease_id: String,
    pub binding_epoch: u64,
    pub policy_version: u64,
    pub operation_id: String,
    pub provider_instance_id: String,
    pub deadline: bm_contract::BmTimestamp,
    pub byte_budget: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseError {
    Expired,
    EpochMismatch { expected: u64, current: u64 },
    PolicyVersionMismatch { expected: u64, current: u64 },
    ByteBudgetExceeded { budget: u64, used: u64 },
    UnknownCapability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    UnknownGrant,
    GrantExhausted,
}

struct LedgerEntry {
    grant: Grant,
    used_count: u64,
    revoked: bool,
}

impl std::fmt::Debug for LedgerEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LedgerEntry")
            .field("grant_id", &self.grant.grant_id)
            .field("used_count", &self.used_count)
            .field("revoked", &self.revoked)
            .finish()
    }
}

/// Grant 台账(M4 内存版;T3 由 SQLite grants 表承载同一索引语义)。
/// `index`(audience × action → grant_ids)即「调用方×目标 Capability」
/// 查表:签发/撤销时增量重编译并递增 policy_version。
#[derive(Debug, Default)]
pub struct GrantLedger {
    entries: HashMap<String, LedgerEntry>,
    index: HashMap<(String, String), Vec<String>>,
    policy_version: u64,
}

impl GrantLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// 策略表版本:lease 准入比对项(签发后策略变更 → 旧 lease 失效)。
    pub fn policy_version(&self) -> u64 {
        self.policy_version
    }

    /// 物化/签发一条 Grant(审批 approved 的下游;T3 接线)。
    pub fn record(&mut self, grant: Grant) {
        let key = (grant.audience.clone(), grant.action.clone());
        let id = grant.grant_id.clone();
        self.entries.insert(
            id.clone(),
            LedgerEntry {
                grant,
                used_count: 0,
                revoked: false,
            },
        );
        self.index.entry(key).or_default().push(id);
        self.policy_version += 1;
    }

    /// 撤销:revocation_version 单调 +1,旧副本即刻失效(可撤销,基线 §11.3)。
    pub fn revoke(&mut self, grant_id: &str) -> Result<u64, LedgerError> {
        let entry = self
            .entries
            .get_mut(grant_id)
            .ok_or(LedgerError::UnknownGrant)?;
        entry.revoked = true;
        entry.grant.revocation_version += 1;
        self.policy_version += 1;
        Ok(entry.grant.revocation_version)
    }

    /// O(1) 索引命中 + 常量级有效性校验(撤销/过期/计数)。返回克隆快照,
    /// 避免借用跨越决策。
    pub fn active_for(
        &self,
        audience: &str,
        action: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Grant> {
        let Some(ids) = self.index.get(&(audience.to_string(), action.to_string())) else {
            return Vec::new();
        };
        ids.iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|e| Self::entry_is_active(e, now))
            .map(|e| e.grant.clone())
            .collect()
    }

    /// 执行前预扣一次授权(Once/Count;Ttl/Forever/Task 不计数)。
    pub fn consume(
        &mut self,
        grant_id: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), LedgerError> {
        let now_clamped = now;
        let (active, scope) = {
            let entry = self
                .entries
                .get(grant_id)
                .ok_or(LedgerError::UnknownGrant)?;
            (
                Self::entry_is_active(entry, now_clamped),
                entry.grant.scope.clone(),
            )
        };
        if !active {
            return Err(LedgerError::GrantExhausted);
        }
        let entry = self
            .entries
            .get_mut(grant_id)
            .ok_or(LedgerError::UnknownGrant)?;
        match scope {
            GrantScope::Once | GrantScope::Count(_) => {
                entry.used_count += 1;
                if matches!(scope, GrantScope::Once) {
                    entry.revoked = true; // Once:首次消费即失效
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 有效性判定(纯函数,不借台账:撤销/过期/计数常量校验)。
    fn entry_is_active(entry: &LedgerEntry, now: chrono::DateTime<chrono::Utc>) -> bool {
        if entry.revoked {
            return false;
        }
        if let Some(expires_at) = &entry.grant.expires_at {
            match parse_ts(expires_at) {
                Some(t) if t > now => {}
                _ => return false,
            }
        }
        match entry.grant.scope {
            GrantScope::Once => entry.used_count == 0,
            GrantScope::Count(n) => entry.used_count < n,
            _ => true,
        }
    }

    pub fn get(&self, grant_id: &str) -> Option<&Grant> {
        self.entries.get(grant_id).map(|e| &e.grant)
    }

    /// 持久化视图:条目的 (used_count, revoked),供恢复/落库同步。
    pub fn entry_state(&self, grant_id: &str) -> Option<(u64, bool)> {
        self.entries
            .get(grant_id)
            .map(|e| (e.used_count, e.revoked))
    }

    /// 恢复:按持久行重建条目(used/revoked 原样装载)。
    pub fn restore(&mut self, grant: Grant, used_count: u64, revoked: bool) {
        let key = (grant.audience.clone(), grant.action.clone());
        let id = grant.grant_id.clone();
        self.entries.insert(
            id.clone(),
            LedgerEntry {
                grant,
                used_count,
                revoked,
            },
        );
        self.index.entry(key).or_default().push(id);
        self.policy_version += 1;
    }
}

/// Broker:持有 Registry(「谁提供什么」)的只读引用与 Grant 台账
/// (「谁被授了什么」)的**可变**引用——预扣/撤销是台账的记账行为,
/// 与 T3 起核心循环单写者顺序执行的形态一致;时钟/ID 端口只读。
pub struct Broker<'a> {
    registry: &'a CapabilityRegistry,
    grants: &'a mut GrantLedger,
    clock: &'a dyn Clock,
    ids: &'a dyn IdGen,
}

impl<'a> Broker<'a> {
    pub fn new(
        registry: &'a CapabilityRegistry,
        grants: &'a mut GrantLedger,
        clock: &'a dyn Clock,
        ids: &'a dyn IdGen,
    ) -> Self {
        Self {
            registry,
            grants,
            clock,
            ids,
        }
    }

    // ---- 步 1-4:身份 / 权限 / scope 查表(O(1))----------------------------

    /// 授权决策:查表 + 常量规则,无 IO。
    pub fn decide(
        &self,
        ctx: &CallContext,
        capability: &str,
        args: &serde_json::Value,
    ) -> Decision {
        // 步 1-2:身份随 ctx 携带;Registry 回答 capability 是否存在。
        let Some(manifest) = self.registry.manifest_of(capability) else {
            return Decision::Denied {
                reason: DenyReason::UnknownCapability,
            };
        };
        // 步 3:信任修正——untrusted 上提一级(基线 §4.5/§5.3)。
        let effective = if ctx.trust == DataTrust::Untrusted {
            manifest.effect.escalated()
        } else {
            manifest.effect
        };
        // 步 4(scope/授权):Grant 查表 O(1) + 常量校验 + 资源谓词。
        // Grant 命中优先于审批判定——审批的产物就是 Grant,已授权调用不得
        // 再撞审批弹窗(否则 Grant 失去意义);高危亦然(ADR-0002 裁决 4
        // 的「task:<id> 批量预授权」语义)。
        let now = self.clock.now();
        for g in self.grants.active_for(&ctx.principal, capability, now) {
            if resource_matches(&g.resource, args) {
                return Decision::Allowed {
                    grant_id: Some(g.grant_id),
                };
            }
        }
        // 步 5:审批判定——high-risk 恒审批(双保险,无视声明);
        // manifest 声明 required;effective_risk reversible 及以上(含
        // trusted 直调——直通只豁免 read-only/low-risk,规格 §5.4)。
        if manifest.effect == RiskClass::HighRiskCommand
            || manifest.approval == ApprovalRequirement::Required
            || effective.is_approval_bearing()
        {
            return Decision::RequireApproval {
                risk_class: manifest.effect,
                effective_risk: effective,
            };
        }
        // 步 6:内建直通(仅 trusted × not-required × read-only/low-risk)。
        if ctx.trust == DataTrust::Trusted
            && manifest.approval == ApprovalRequirement::NotRequired
            && matches!(
                manifest.effect,
                RiskClass::ReadOnly | RiskClass::LowRiskCommand
            )
        {
            return Decision::Allowed { grant_id: None };
        }
        // 步 7:默认拒绝(ADR-0006:未列入合同的权力视为不存在)。
        Decision::Denied {
            reason: DenyReason::NoGrant,
        }
    }

    // ---- 步 5:参数校验(M4.3)---------------------------------------------

    fn validate_args(
        manifest: &CapabilityManifest,
        args: &serde_json::Value,
    ) -> Result<(), String> {
        bm_contract::schemas::validate(&manifest.input_schema.to_string(), args)
    }

    // ---- 步 6:绑定与凭证签发/校验(ADR-0001 条件 2)------------------------

    pub fn issue_credential(
        &self,
        capability: &str,
        principal: &str,
    ) -> Result<CallCredential, RegistryError> {
        let binding = self
            .registry
            .binding_of(capability)
            .ok_or(RegistryError::UnknownCapability)?;
        Ok(CallCredential {
            call_id: self.ids.next_id("call").to_string(),
            capability: capability.to_string(),
            binding_epoch: binding.epoch,
            provider_instance_id: binding.provider_instance_id.clone(),
            principal: principal.to_string(),
        })
    }

    /// Provider 侧执行前校验:凭证与当前 binding 不匹配即拒绝(重试/拒绝)。
    pub fn verify_credential(&self, cred: &CallCredential) -> Result<(), (u64, u64)> {
        let binding = self
            .registry
            .binding_of(&cred.capability)
            .ok_or((cred.binding_epoch, 0))?;
        if binding.epoch != cred.binding_epoch
            || binding.provider_instance_id != cred.provider_instance_id
        {
            return Err((cred.binding_epoch, binding.epoch));
        }
        Ok(())
    }

    // ---- 步 6-7:执行 + 结果校验 -------------------------------------------

    /// 统一调用入口(七步管线的编排面;各步为上方分段函数)。
    pub fn call(
        &mut self,
        ctx: &CallContext,
        capability: &str,
        args: serde_json::Value,
    ) -> CallOutcome {
        let decision = self.decide(ctx, capability, &args);
        let grant_id = match &decision {
            Decision::Allowed { grant_id } => grant_id.clone(),
            _ => return CallOutcome::Rejected { decision },
        };
        let Some(manifest) = self.registry.manifest_of(capability) else {
            return CallOutcome::Rejected {
                decision: Decision::Denied {
                    reason: DenyReason::UnknownCapability,
                },
            };
        };
        // 步 5:参数校验(违者 validation_failed,审计由上层映射 capability.denied)。
        if let Err(e) = Self::validate_args(manifest, &args) {
            return CallOutcome::InvalidArgs { message: e };
        }
        // Grant 预扣(见模块注释的语义留档)。
        if let Some(gid) = &grant_id
            && self.grants.consume(gid, self.clock.now()).is_err()
        {
            return CallOutcome::Rejected {
                decision: Decision::Denied {
                    reason: DenyReason::NoGrant,
                },
            };
        }
        // 步 6:凭证签发 + 执行点重验(不匹配即拒绝)。
        let Ok(credential) = self.issue_credential(capability, &ctx.principal) else {
            return CallOutcome::Rejected {
                decision: Decision::Denied {
                    reason: DenyReason::UnknownCapability,
                },
            };
        };
        if let Err((expected, current)) = self.verify_credential(&credential) {
            return CallOutcome::StaleBinding {
                expected_epoch: expected,
                current_epoch: current,
            };
        }
        let Some(handle) = self.registry.handle_of(capability) else {
            return CallOutcome::ProviderError {
                message: "Provider 句柄不可用(binding 在而缓存缺失)".into(),
            };
        };
        // 步 7:执行(返回值过 output_schema 后才算完成)。
        match handle.invoke(args) {
            Ok(result) => {
                if let Err(e) =
                    bm_contract::schemas::validate(&manifest.output_schema.to_string(), &result)
                {
                    return CallOutcome::InvalidOutput { message: e };
                }
                CallOutcome::Completed {
                    call_id: credential.call_id.clone(),
                    grant_id,
                    credential,
                    result,
                }
            }
            Err(e) => CallOutcome::ProviderError { message: e },
        }
    }

    // ---- 数据面 lease(ADR-0001 条件 4)------------------------------------

    /// 决策 allow 后签发数据面通道凭证(准入测试①的签发半边)。
    pub fn issue_lease(
        &self,
        capability: &str,
        operation_id: &str,
        byte_budget: u64,
        ttl_ms: u64,
    ) -> Option<Lease> {
        let binding = self.registry.binding_of(capability)?;
        if binding.status != BindingStatus::Active {
            return None;
        }
        let deadline = self.clock.now() + chrono::Duration::milliseconds(ttl_ms as i64);
        Some(Lease {
            lease_id: self.ids.next_id("lease").to_string(),
            binding_epoch: binding.epoch,
            policy_version: self.grants.policy_version(),
            operation_id: operation_id.to_string(),
            provider_instance_id: binding.provider_instance_id.clone(),
            deadline: format_ts(deadline),
            byte_budget,
        })
    }

    /// 通道准入:capability 的当前 binding epoch / policy_version / deadline /
    /// byte_budget 常量校验。epoch 切换不改变已授权通道的审计归属(凭证保全),
    /// 但新数据准入按签发时 epoch 校验,旧 epoch 通道须重签(准入测试②)。
    pub fn admit_lease(
        &self,
        capability: &str,
        lease: &Lease,
        used_bytes: u64,
    ) -> Result<(), LeaseError> {
        let binding = self
            .registry
            .binding_of(capability)
            .ok_or(LeaseError::UnknownCapability)?;
        if binding.epoch != lease.binding_epoch {
            return Err(LeaseError::EpochMismatch {
                expected: lease.binding_epoch,
                current: binding.epoch,
            });
        }
        let pv = self.grants.policy_version();
        if pv != lease.policy_version {
            return Err(LeaseError::PolicyVersionMismatch {
                expected: lease.policy_version,
                current: pv,
            });
        }
        let now = self.clock.now();
        match parse_ts(&lease.deadline) {
            Some(t) if t > now => {}
            _ => return Err(LeaseError::Expired),
        }
        if used_bytes > lease.byte_budget {
            return Err(LeaseError::ByteBudgetExceeded {
                budget: lease.byte_budget,
                used: used_bytes,
            });
        }
        Ok(())
    }
}

/// 资源谓词(下限实现:args 等值字典;数字统一 f64 比较避免 1 vs 1.0 形态差)。
fn resource_matches(resource: &GrantResource, args: &serde_json::Value) -> bool {
    for (k, want) in &resource.args_predicates {
        match args.get(k) {
            Some(have) if json_scalar_eq(have, want) => {}
            _ => return false,
        }
    }
    true
}

fn json_scalar_eq(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a, b) {
        (serde_json::Value::Number(x), serde_json::Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// 内置 Provider 测试/装配辅助:把闭包包装成 CapabilityProvider。
/// (正式内置能力集随 T5;此处供 Broker 测试与早期装配。)
pub fn provider_fn(
    f: impl Fn(serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync + 'static,
) -> Arc<dyn crate::registry::CapabilityProvider> {
    struct F(Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>);
    impl crate::registry::CapabilityProvider for F {
        fn invoke(&self, args: serde_json::Value) -> Result<serde_json::Value, String> {
            (self.0)(args)
        }
    }
    Arc::new(F(Box::new(f)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::MockClock;
    use crate::registry::CapabilityRegistry;
    use bm_contract::ids::SeqIdGen;
    use serde_json::json;

    const BASE_MS: u128 = 1_788_000_000_000;

    /// 五风险能力集(manifest approval 均 not-required;high-risk 的恒审批
    /// 由 Broker 双保险兜住,不依赖注册方声明)。
    fn register_five(reg: &mut CapabilityRegistry) {
        for (name, effect) in [
            ("system.ro", "read-only"),
            ("system.low", "low-risk-command"),
            ("system.rev", "reversible-command"),
            ("system.ext", "external-side-effect"),
            ("system.high", "high-risk-command"),
        ] {
            let m: CapabilityManifest = serde_json::from_value(json!({
                "capability": name, "provider": name, "version": "0.1.0",
                "input_schema": {"type": "object"},
                "output_schema": {"type": "object"},
                "effect": effect, "idempotent": true, "cancellable": true,
                "timeout_ms": 1000, "approval": "not-required"
            }))
            .unwrap();
            reg.register(m, &format!("{name}@0.1.0"), provider_fn(Ok))
                .unwrap();
        }
    }

    fn harness() -> (CapabilityRegistry, GrantLedger, MockClock, SeqIdGen) {
        let mut reg = CapabilityRegistry::new();
        register_five(&mut reg);
        (
            reg,
            GrantLedger::new(),
            MockClock::at_ms(BASE_MS),
            SeqIdGen::new(),
        )
    }

    fn grant_of(
        audience: &str,
        action: &str,
        scope: GrantScope,
        preds: serde_json::Value,
    ) -> Grant {
        serde_json::from_value(json!({
            "grant_id": "grant_01JAAAAAAAAAAAAAAAAAAAAA0C",
            "audience": audience, "action": action,
            "resource": {"capability": action, "args_predicates": preds},
            "scope": scope, "delegation_depth": 0,
            "expires_at": null, "revocation_version": 0,
            "parent_grant_hash": "9b1dec3f2a6c47d5b8e0f1a2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a",
            "issued_by": "surface:user", "created_at": "2026-08-29T10:00:00.000Z"
        }))
        .unwrap()
    }

    #[test]
    fn trusted_direct_call_matrix() {
        let (reg, mut grants, clock, ids) = harness();
        let mut broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::new("surface:user", DataTrust::Trusted);
        // read-only / low-risk → 直通(grant_id=None)
        for cap in ["system.ro", "system.low"] {
            assert_eq!(
                broker.decide(&ctx, cap, &json!({})),
                Decision::Allowed { grant_id: None },
                "{cap} trusted 应直通"
            );
        }
        // reversible / external / high → RequireApproval(trusted 亦然)
        for cap in ["system.rev", "system.ext", "system.high"] {
            let d = broker.decide(&ctx, cap, &json!({}));
            assert!(
                matches!(d, Decision::RequireApproval { .. }),
                "{cap}: {d:?}"
            );
        }
        // 完成 system.ro 全链路(执行/收据形态)
        let outcome = broker.call(&ctx, "system.ro", json!({"x": 1}));
        assert!(
            matches!(outcome, CallOutcome::Completed { .. }),
            "{outcome:?}"
        );
    }

    #[test]
    fn untrusted_escalation_matrix_100_percent() {
        let (reg, mut grants, clock, ids) = harness();
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::new("agent:bot", DataTrust::Untrusted);
        // 上提:read-only→low-risk(不审批,但无直通无 Grant → 默认拒绝)
        assert_eq!(
            broker.decide(&ctx, "system.ro", &json!({})),
            Decision::Denied {
                reason: DenyReason::NoGrant
            }
        );
        // low-risk→reversible、reversible→external、external→high、high 封顶:
        // reversible 及以上 100% 升级(ADR-0002 条件 3 的量化门槛,矩阵全断言)
        for cap in ["system.low", "system.rev", "system.ext", "system.high"] {
            let d = broker.decide(&ctx, cap, &json!({}));
            let Decision::RequireApproval { effective_risk, .. } = &d else {
                panic!("{cap} 应 100% 升级审批,实际 {d:?}");
            };
            assert!(effective_risk.is_approval_bearing(), "{cap}: {d:?}");
        }
    }

    #[test]
    fn agent_derived_requires_grant_and_predicate_must_match() {
        let (reg, mut grants, clock, ids) = harness();
        let g = grant_of(
            "agent:bot",
            "system.low",
            GrantScope::Count(10),
            json!({"path": "notes/inbox.md"}),
        );
        let gid = g.grant_id.clone();
        grants.record(g);
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::new("agent:bot", DataTrust::AgentDerived);
        // 谓词命中 → Allowed{grant_id}
        assert_eq!(
            broker.decide(
                &ctx,
                "system.low",
                &json!({"path": "notes/inbox.md", "n": 1})
            ),
            Decision::Allowed {
                grant_id: Some(gid.clone())
            }
        );
        // 谓词不中 → 默认拒绝
        assert_eq!(
            broker.decide(&ctx, "system.low", &json!({"path": "notes/other.md"})),
            Decision::Denied {
                reason: DenyReason::NoGrant
            }
        );
        // 未授权 principal → 默认拒绝(越权 100% 拒绝矩阵的查表半边)
        let stranger = CallContext::new("agent:other", DataTrust::AgentDerived);
        assert_eq!(
            broker.decide(&stranger, "system.low", &json!({"path": "notes/inbox.md"})),
            Decision::Denied {
                reason: DenyReason::NoGrant
            }
        );
    }

    #[test]
    fn count_grant_exhausts_after_n_consumptions() {
        let (reg, mut grants, clock, ids) = harness();
        let g = grant_of("agent:bot", "system.low", GrantScope::Count(2), json!({}));
        let gid = g.grant_id.clone();
        grants.record(g);
        let mut broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::new("agent:bot", DataTrust::AgentDerived);
        for i in 1..=2 {
            let out = broker.call(&ctx, "system.low", json!({}));
            assert!(
                matches!(out, CallOutcome::Completed { .. }),
                "第{i}次应执行"
            );
        }
        // 第三次:Grant 已尽 → Rejected(默认拒绝)
        assert_eq!(
            broker.decide(&ctx, "system.low", &json!({})),
            Decision::Denied {
                reason: DenyReason::NoGrant
            }
        );
        assert!(matches!(
            broker.call(&ctx, "system.low", json!({})),
            CallOutcome::Rejected { .. }
        ));
        assert_eq!(grants.get(&gid).unwrap().scope, GrantScope::Count(2));
    }

    #[test]
    fn revoked_or_expired_grant_denies() {
        let (reg, mut grants, clock, ids) = harness();
        let g = grant_of("agent:bot", "system.low", GrantScope::Forever, json!({}));
        let gid = g.grant_id.clone();
        // 带过期时间的第二条(forever + expires_at 双字段并存时以 expires_at 为准)
        let mut g2 = grant_of("agent:bot", "system.ro", GrantScope::Forever, json!({}));
        g2.grant_id = "grant_01JAAAAAAAAAAAAAAAAAAAAA0D".into();
        g2.expires_at = Some("2026-08-29T10:41:00.000Z".into()); // BASE+60s 过期(BASE=10:40)
        grants.record(g);
        grants.record(g2);

        let ctx = CallContext::new("agent:bot", DataTrust::AgentDerived);
        // 撤销:version +1,立即失效
        assert_eq!(grants.revoke(&gid).unwrap(), 1);
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        assert_eq!(
            broker.decide(&ctx, "system.low", &json!({})),
            Decision::Denied {
                reason: DenyReason::NoGrant
            }
        );
        // 过期:clock 推进 61s 后 system.ro 的 Grant 失效
        assert_eq!(
            broker.decide(&ctx, "system.ro", &json!({})),
            Decision::Allowed {
                grant_id: Some("grant_01JAAAAAAAAAAAAAAAAAAAAA0D".into())
            }
        );
        clock.advance_ms(61_000);
        assert_eq!(
            broker.decide(&ctx, "system.ro", &json!({})),
            Decision::Denied {
                reason: DenyReason::NoGrant
            }
        );
    }

    #[test]
    fn credential_and_lease_catch_binding_switch() {
        let (mut reg, mut grants, clock, ids) = harness();
        // 切换前:签发凭证(epoch=1)与 lease(epoch=1),各自验证通过
        let (cred, lease) = {
            let broker = Broker::new(&reg, &mut grants, &clock, &ids);
            let cred = broker
                .issue_credential("system.ro", "surface:user")
                .unwrap();
            let lease = broker
                .issue_lease("system.ro", "op_00000000000000000000000001", 1024, 60_000)
                .unwrap();
            assert_eq!(cred.binding_epoch, 1);
            assert_eq!(lease.binding_epoch, 1);
            assert!(broker.verify_credential(&cred).is_ok());
            assert_eq!(broker.admit_lease("system.ro", &lease, 0), Ok(()));
            (cred, lease)
        };
        // 热替换:epoch 1→2 → 旧凭证执行点校验失败、旧 lease 拒绝准入
        // (授权-执行-审计三方一致,ADR-0001 条件 2;在途归属由凭证旧 epoch 保全)
        reg.switch_binding("system.ro", "system.ro@0.2.0", provider_fn(Ok))
            .unwrap();
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        assert_eq!(
            broker.verify_credential(&cred),
            Err((1, 2)),
            "旧 epoch 凭证必须被拒"
        );
        assert_eq!(
            broker.admit_lease("system.ro", &lease, 0),
            Err(LeaseError::EpochMismatch {
                expected: 1,
                current: 2
            })
        );
        // 新 epoch 重签后恢复准入
        let lease2 = broker
            .issue_lease("system.ro", "op_00000000000000000000000002", 1024, 60_000)
            .unwrap();
        assert_eq!(lease2.binding_epoch, 2);
        assert_eq!(broker.admit_lease("system.ro", &lease2, 0), Ok(()));
    }

    #[test]
    fn invalid_args_and_output_are_rejected() {
        // 独立装配:input/output schema 收紧的 read-only 能力
        let mut reg = CapabilityRegistry::new();
        let m: CapabilityManifest = serde_json::from_value(json!({
            "capability": "system.ro", "provider": "system.ro", "version": "0.1.0",
            "input_schema": {"type": "object", "required": ["msg"],
                             "properties": {"msg": {"type": "string"}}},
            "output_schema": {"type": "object", "required": ["echo"],
                              "properties": {"echo": {"type": "string"}}},
            "effect": "read-only", "idempotent": true, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required"
        }))
        .unwrap();
        reg.register(
            m,
            "system.ro@0.1.0",
            provider_fn(|_| Ok(json!({"wrong": true}))),
        )
        .unwrap();
        let mut grants = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let mut broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::new("surface:user", DataTrust::Trusted);
        // 入参违 schema → InvalidArgs(M4.3)
        assert!(matches!(
            broker.call(&ctx, "system.ro", json!({"msg": 42})),
            CallOutcome::InvalidArgs { .. }
        ));
        // 出参违 schema → InvalidOutput
        assert!(matches!(
            broker.call(&ctx, "system.ro", json!({"msg": "hi"})),
            CallOutcome::InvalidOutput { .. }
        ));
    }

    #[test]
    fn lease_lifecycle_admission_gates() {
        let (reg, mut grants, clock, ids) = harness();
        // ① 有效 lease 准入 + 字节预算门
        let lease = {
            let broker = Broker::new(&reg, &mut grants, &clock, &ids);
            let lease = broker
                .issue_lease("system.ro", "op_00000000000000000000000001", 1024, 5_000)
                .expect("active binding 应可签发 lease");
            assert_eq!(broker.admit_lease("system.ro", &lease, 0), Ok(()));
            assert_eq!(
                broker.admit_lease("system.ro", &lease, 2048),
                Err(LeaseError::ByteBudgetExceeded {
                    budget: 1024,
                    used: 2048
                })
            );
            lease
        };
        // ② 策略版本变更(Grant 签发)→ 旧 lease 不再准入
        grants.record(grant_of(
            "agent:bot",
            "system.low",
            GrantScope::Once,
            json!({}),
        ));
        {
            let broker = Broker::new(&reg, &mut grants, &clock, &ids);
            assert!(matches!(
                broker.admit_lease("system.ro", &lease, 0),
                Err(LeaseError::PolicyVersionMismatch { .. })
            ));
            // ④ 过期:重签(policy_version 已匹配)后推进时钟,deadline 到期失效
            let fresh = broker
                .issue_lease("system.ro", "op_00000000000000000000000002", 1024, 1_000)
                .unwrap();
            clock.advance_ms(2_000);
            assert_eq!(
                broker.admit_lease("system.ro", &fresh, 0),
                Err(LeaseError::Expired)
            );
            // 未知 capability 不可签发
            assert!(
                broker
                    .issue_lease("system.nope", "op_00000000000000000000000003", 1, 1_000)
                    .is_none()
            );
        }
    }

    #[test]
    fn unknown_capability_is_denied_without_approval_exit() {
        let (reg, mut grants, clock, ids) = harness();
        let mut broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::new("surface:user", DataTrust::Trusted);
        // 未注册能力:默认拒绝,审批不能补授权(ADR-0006)
        assert_eq!(
            broker.decide(&ctx, "system.ghost", &json!({})),
            Decision::Denied {
                reason: DenyReason::UnknownCapability
            }
        );
        assert!(matches!(
            broker.call(&ctx, "system.ghost", json!({})),
            CallOutcome::Rejected { .. }
        ));
    }
}
