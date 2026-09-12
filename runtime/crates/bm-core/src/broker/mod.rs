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
use bm_contract::capability::{ApprovalRequirement, CapabilityManifest, DataTrust, RiskClass};
use bm_contract::ids::IdGen;
use bm_contract::timestamp::{format_ts, parse_ts};
use predicate::resource_matches;

mod ledger;
mod predicate;
mod types;

pub use ledger::GrantLedger;
pub use predicate::{provider_fn, provider_fn_with_meta};
pub use types::{
    CallContext, CallCredential, CallOutcome, Decision, DenyReason, Lease, LeaseError,
    PreparedCall, TrustViolation,
};

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
        // 步 4.5(ADR-0038):per-capability 授权规则由 manifest 声明,Broker
        // 只做解释——不再硬编码能力名/主体前缀(ADR-0006)。当前唯一消费形态
        // 是记忆抽屉:主体对自己的抽屉常量放行,越界升级审批(不静默拒绝,
        // 产出可审批事实,批准即签发带 scope 谓词的 Grant)。未声明 = 本步不
        // 适用,走既有审批/直通流。
        if let Some(v) = Self::authorization_verdict(ctx, args, manifest, effective) {
            return v;
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
        // M7.6:App 主体(surface:app:<name>)不享内建直通——跨 provider 访问
        // 一律走显式 Grant(默认拒绝,基线 M7 通过条件第五句)。
        if ctx.trust == DataTrust::Trusted
            && !ctx.principal.starts_with("surface:app:")
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

    /// 步 4.5 的声明式授权裁决(None = 本步不适用,继续既有流)。
    ///
    /// ADR-0038:规则本体在 manifest.authorization(合同),此处只是解释器。
    /// 抽屉语义:按 `self_drawers` 依序匹配主体前缀,命中即得「主体自有抽屉
    /// 标签」;`scope` 与之相等 → 常量放行;`read_allow_scopes` 内的 scope 对
    /// read-only 能力额外放行;其余越界 → 升级审批。
    fn authorization_verdict(
        ctx: &CallContext,
        args: &serde_json::Value,
        manifest: &CapabilityManifest,
        effective: RiskClass,
    ) -> Option<Decision> {
        let drawer = manifest.authorization.as_ref()?.drawer.as_ref()?;
        let scope = args["scope"].as_str()?; // 缺 scope 由 Provider 形态校验拒
        // 主体自有抽屉:取首个命中前缀,其余段拼入 drawer_prefix。
        let own = drawer.self_drawers.iter().find_map(|r| {
            ctx.principal
                .strip_prefix(&r.principal_prefix)
                .map(|rest| format!("{}{rest}", r.drawer_prefix))
        })?; // 无命中(surface:user / 系统主体 / App):本步不适用
        if scope == own
            || (manifest.effect == RiskClass::ReadOnly
                && drawer.read_allow_scopes.iter().any(|s| s == scope))
        {
            return Some(Decision::Allowed { grant_id: None });
        }
        Some(Decision::RequireApproval {
            risk_class: manifest.effect,
            effective_risk: effective,
        })
    }

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

    /// 预备完成:凭证/manifest/Grant 引用/Provider 句柄就绪,可进入执行段。
    /// 副作用类(is_side_effect)在 prepare 与 execute 之间落 intent 事件
    /// ——副作用前门禁(规格 §5.5;ADR-0001 条件 5)。
    #[allow(clippy::result_large_err)] // CallOutcome 即合同错误形态,装箱无益
    pub fn prepare(
        &mut self,
        ctx: &CallContext,
        capability: &str,
        args: serde_json::Value,
    ) -> Result<PreparedCall, CallOutcome> {
        // ADR-0037 生命周期门:binding 非 Active(排空/不可用)一律拒绝新调用。
        // 与运行期健康门(World.provider_health,进程内按 provider 记)分工:
        // 此处是「注册-切换-下线」的生命周期真相,持久且带代际。
        if let Some(b) = self.registry.binding_of(capability)
            && b.status != BindingStatus::Active
        {
            return Err(CallOutcome::ProviderUnavailable {
                message: format!(
                    "能力 {capability} 处于 {:?} 生命周期态(排空/不可用),拒绝新调用",
                    b.status
                ),
            });
        }
        let decision = self.decide(ctx, capability, &args);
        let grant_id = match &decision {
            Decision::Allowed { grant_id } => grant_id.clone(),
            _ => return Err(CallOutcome::Rejected { decision }),
        };
        let Some(manifest) = self.registry.manifest_of(capability) else {
            return Err(CallOutcome::Rejected {
                decision: Decision::Denied {
                    reason: DenyReason::UnknownCapability,
                },
            });
        };
        // 步 5:参数校验(违者 validation_failed,审计由上层映射 capability.denied)。
        if let Err(e) = Self::validate_args(manifest, &args) {
            return Err(CallOutcome::InvalidArgs { message: e });
        }
        // Grant 预扣(见模块注释的语义留档)。
        if let Some(gid) = &grant_id
            && self.grants.consume(gid, self.clock.now()).is_err()
        {
            return Err(CallOutcome::Rejected {
                decision: Decision::Denied {
                    reason: DenyReason::NoGrant,
                },
            });
        }
        // 步 6:凭证签发 + 执行点重验(不匹配即拒绝)。
        let Ok(credential) = self.issue_credential(capability, &ctx.principal) else {
            return Err(CallOutcome::Rejected {
                decision: Decision::Denied {
                    reason: DenyReason::UnknownCapability,
                },
            });
        };
        if let Err((expected, current)) = self.verify_credential(&credential) {
            return Err(CallOutcome::StaleBinding {
                expected_epoch: expected,
                current_epoch: current,
            });
        }
        let Some(handle) = self.registry.handle_of(capability) else {
            return Err(CallOutcome::ProviderError {
                message: "Provider 句柄不可用(binding 在而缓存缺失)".into(),
            });
        };
        Ok(PreparedCall {
            manifest: manifest.clone(),
            credential,
            grant_id,
            is_side_effect: manifest.effect == RiskClass::ExternalSideEffect,
            handle,
        })
    }

    /// 步 7:执行(返回值过 output_schema 后才算完成)。
    pub fn execute(&self, prepared: &PreparedCall, args: serde_json::Value) -> CallOutcome {
        // 故障半径(T8;ADR-0001 条件 1 证伪③):Provider panic 被 execute
        // 收容为 ProviderError——决策路径与核心循环不被第三方实现击穿;
        // 兜底仍由 L0 重启承担,无特权降级通道。
        let invoke_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prepared.handle.invoke(args)
        }));
        match invoke_result {
            Ok(Ok(result)) => {
                if let Err(e) = bm_contract::schemas::validate(
                    &prepared.manifest.output_schema.to_string(),
                    &result,
                ) {
                    return CallOutcome::InvalidOutput { message: e };
                }
                CallOutcome::Completed {
                    call_id: prepared.credential.call_id.clone(),
                    grant_id: prepared.grant_id.clone(),
                    credential: prepared.credential.clone(),
                    result,
                }
            }
            Ok(Err(e)) => CallOutcome::ProviderError { message: e },
            Err(panic_payload) => {
                let detail = panic_payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic_payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "provider panicked".into());
                CallOutcome::ProviderError {
                    message: format!("Provider panic(已收容): {detail}"),
                }
            }
        }
    }

    /// 统一调用入口(步 1-7 组合;副作用前门禁由调用方在 prepare/execute
    /// 之间落 intent 事件——核心循环单写者)。
    pub fn call(
        &mut self,
        ctx: &CallContext,
        capability: &str,
        args: serde_json::Value,
    ) -> CallOutcome {
        match self.prepare(ctx, capability, args.clone()) {
            Ok(prepared) => self.execute(&prepared, args),
            Err(outcome) => outcome,
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

#[cfg(test)]
mod tests;
