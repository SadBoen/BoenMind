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
use crate::registry::{BindingStatus, CapabilityProvider, CapabilityRegistry, RegistryError};
use bm_contract::capability::{
    ApprovalRequirement, CapabilityManifest, DataTrust, Grant, GrantResource, GrantScope, RiskClass,
};
use bm_contract::ids::IdGen;
use bm_contract::timestamp::{format_ts, parse_ts};
use std::collections::HashMap;
use std::sync::Arc;

/// 调用上下文:身份与信任级别随内容来源链传递(M4 规格 §5.4;
/// Wire Surface 直调恒 trusted,客户端无 trust 参数面)。
///
/// 构造面即安全边界:`trusted` 只能经 [`CallContext::surface`](用户显式操作)
/// 产生;内部内容链经 [`CallContext::content_chain`](agent 推理/外部内容驱动)
/// 构造,声称 trusted 在构造层即被拒——「untrusted 内容标注为 trusted 视为
/// 编程错误」(基线 §4.5 来源链;提升权限的决定永远不在调用方)。
#[derive(Debug, Clone)]
pub struct CallContext {
    /// kind:local-id(如 surface:user / agent:note_bot)。
    pub principal: String,
    pub trust: DataTrust,
    pub idempotency_key: Option<String>,
}

/// 内容链构造声称 trusted 的编程错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustViolation;

impl std::fmt::Display for TrustViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "内容链不得声称 trusted(trusted 仅来自用户直接输入)")
    }
}

impl CallContext {
    /// Wire Surface 直调(用户显式操作):唯一合法的 trusted 来源
    /// (PI-01:用户输入本身即 trusted)。CLI/GUI/Web 均经此入口。
    pub fn surface(principal: &str) -> Self {
        Self {
            principal: principal.to_string(),
            trust: DataTrust::Trusted,
            idempotency_key: None,
        }
    }

    /// 内部内容链:trust 由上游内容标注携带;声称 trusted 被构造层拒绝。
    pub fn content_chain(principal: &str, trust: DataTrust) -> Result<Self, TrustViolation> {
        match trust {
            DataTrust::Trusted => Err(TrustViolation),
            t => Ok(Self {
                principal: principal.to_string(),
                trust: t,
                idempotency_key: None,
            }),
        }
    }

    /// 附幂等键(副作用操作必备,基线 §9.5)。
    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
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
    /// 幂等抑制(ADR-0002 条件 6):等价请求返回原收据,不重复执行;
    /// 上层必须落 outcome=suppressed 审计事件以资证明。
    Suppressed {
        original_result: serde_json::Value,
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
    /// M7 S5:Provider 熔断/重连超限(unavailable 语义,区别于内部错误)。
    ProviderUnavailable {
        message: String,
    },
    /// M7 S4:已派发异步执行(收据 running;完成经 Cmd::ProviderCall 落定)。
    DispatchedAsync,
}

/// 预备完成的调用:进入执行段的一切就绪(副作用门禁插在 prepare 与 execute
/// 之间——intent 事件落盘后方允许 invoke)。
pub struct PreparedCall {
    pub manifest: CapabilityManifest,
    pub credential: CallCredential,
    pub grant_id: Option<String>,
    /// manifest.effect == external-side-effect(前门禁触发面)。
    pub is_side_effect: bool,
    handle: Arc<dyn CapabilityProvider>,
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

    /// 按作用域查 Grant(M5:task:<id> 作用域的「Task 结束即失效」撤销面)。
    /// 返回该作用域的全部 Grant(含已撤销;调用方按需过滤)。
    pub fn grants_scoped_to(&self, task_id: &str) -> Vec<Grant> {
        self.entries
            .values()
            .map(|e| &e.grant)
            .filter(|g| matches!(&g.scope, GrantScope::Task(t) if t == task_id))
            .cloned()
            .collect()
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
        // 步 4.5(M9 S1):记忆抽屉在主体维度的权限边界——「作用域即权限
        // 边界」(基线 §4.1)落到「谁可写哪个抽屉」。agent/task 族主体对
        // 自己的抽屉常量放行(agent 本体 ↔ memory:agent:<id>;coord/worker
        // ↔ memory:task:<id>);search 对 memory:user 放行(读不产生内容
        // 污染);越界抽屉一律升级审批——不静默拒绝,产出可审批事实,
        // 批准即签发带 scope 谓词的 Grant(资源谓词捕获见 handlers)。
        // App 主体不享抽屉直通(M7.6 延续:跨域一律显式 Grant);user
        // Surface 与系统主体按既有流。memory.delete 按条目 ID 定位、args
        // 不含 scope,主体维度执行面随条目所有权列(留档演进)。
        if let Some(v) = Self::memory_drawer_verdict(ctx, capability, args, manifest, effective) {
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

    /// 步 4.5 的记忆抽屉裁决(None = 本步不适用,继续既有流)。
    fn memory_drawer_verdict(
        ctx: &CallContext,
        capability: &str,
        args: &serde_json::Value,
        manifest: &CapabilityManifest,
        effective: RiskClass,
    ) -> Option<Decision> {
        if capability != "memory.write" && capability != "memory.search" {
            return None;
        }
        let scope = args["scope"].as_str()?; // 缺 scope 由 Provider 形态校验拒
        let own = ctx.principal.strip_prefix("agent:").map(|rest| {
            // 任务族成员(coord:/worker: 前缀)的抽屉按 task 维度;
            // 其余即 agent 本体(M6 per-task principal 命名空间)。
            rest.strip_prefix("coord:")
                .or_else(|| rest.strip_prefix("worker:"))
                .map(|tid| format!("memory:task:{tid}"))
                .unwrap_or_else(|| format!("memory:agent:{rest}"))
        });
        let own = own?; // surface:user / 系统主体 / App:本步不适用
        if scope == own || (capability == "memory.search" && scope == "memory:user") {
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
        let ctx = CallContext::surface("surface:user");
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
        let ctx =
            CallContext::content_chain("agent:bot", DataTrust::Untrusted).expect("内容链构造");
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
        let ctx =
            CallContext::content_chain("agent:bot", DataTrust::AgentDerived).expect("内容链构造");
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
        let stranger =
            CallContext::content_chain("agent:other", DataTrust::AgentDerived).expect("内容链构造");
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
        let ctx =
            CallContext::content_chain("agent:bot", DataTrust::AgentDerived).expect("内容链构造");
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

        let ctx =
            CallContext::content_chain("agent:bot", DataTrust::AgentDerived).expect("内容链构造");
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
        let ctx = CallContext::surface("surface:user");
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
        let ctx = CallContext::surface("surface:user");
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

// ---- M4-T4:量化 CI 门槛(硬约束 9)与 PI 决策层断言 ------------------------

#[cfg(test)]
mod trust_gate_tests {
    use super::*;
    use crate::clock::MockClock;
    use bm_contract::capability::CapabilityManifest;
    use bm_contract::ids::SeqIdGen;
    use serde_json::json;

    const BASE_MS: u128 = 1_788_000_000_000;

    /// 注册五风险能力集(manifest approval 均 not-required:审批要求只来自
    /// Broker 规则,不依赖注册方声明——PI-12 的裁决面)。
    fn five_capability_registry() -> CapabilityRegistry {
        let mut reg = CapabilityRegistry::new();
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
        reg
    }

    fn grant_id_for(cap_name: &str) -> String {
        let names = [
            "system.ro",
            "system.low",
            "system.rev",
            "system.ext",
            "system.high",
        ];
        let idx = names.iter().position(|c| *c == cap_name).unwrap();
        format!("grant_01JAAAAAAAAAAAAAAAAAAAAA{idx:02}")
    }

    /// 30 格全断言:5 风险 × 3 trust × {无 Grant, 有 Grant}。
    /// 量化门槛(硬断言,非报告指标,ADR-0002 条件 3):
    /// - untrusted → reversible+ 100% 升级审批(升级率恰好 100%)
    /// - 越权(无 Grant 且非直通)100% 默认拒绝
    /// - Grant 命中即放行(审批产物免再审,任意 trust/风险)
    #[test]
    fn decision_matrix_5risk_x_3trust_x_grant() {
        let reg = five_capability_registry();
        let mut grants = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();

        let caps = [
            ("system.ro", RiskClass::ReadOnly),
            ("system.low", RiskClass::LowRiskCommand),
            ("system.rev", RiskClass::ReversibleCommand),
            ("system.ext", RiskClass::ExternalSideEffect),
            ("system.high", RiskClass::HighRiskCommand),
        ];
        let mut untrusted_approval_cases = 0u32;
        let mut untrusted_reversible_plus_cases = 0u32;
        for (cap_name, risk) in caps {
            for (trust, principal) in [
                (DataTrust::Trusted, "surface:user"),
                (DataTrust::AgentDerived, "agent:bot"),
                (DataTrust::Untrusted, "agent:bot"),
            ] {
                let ctx = match trust {
                    DataTrust::Trusted => CallContext::surface(principal),
                    t => CallContext::content_chain(principal, t)
                        .expect("内容链构造(AgentDerived/Untrusted)"),
                };
                // ---- 无 Grant ----
                let broker = Broker::new(&reg, &mut grants, &clock, &ids);
                let d = broker.decide(&ctx, cap_name, &json!({}));
                let expected: Decision = match (trust, risk) {
                    (DataTrust::Trusted, RiskClass::ReadOnly)
                    | (DataTrust::Trusted, RiskClass::LowRiskCommand) => {
                        Decision::Allowed { grant_id: None }
                    }
                    _ if risk.is_approval_bearing()
                        || (trust == DataTrust::Untrusted
                            && risk.escalated().is_approval_bearing()) =>
                    {
                        Decision::RequireApproval {
                            risk_class: risk,
                            effective_risk: if trust == DataTrust::Untrusted {
                                risk.escalated()
                            } else {
                                risk
                            },
                        }
                    }
                    _ => Decision::Denied {
                        reason: DenyReason::NoGrant,
                    },
                };
                assert_eq!(d, expected, "{cap_name} × {trust:?}(无 Grant)");
                if trust == DataTrust::Untrusted && risk.is_approval_bearing() {
                    untrusted_reversible_plus_cases += 1;
                    if matches!(d, Decision::RequireApproval { .. }) {
                        untrusted_approval_cases += 1;
                    }
                }

                // ---- 有 Grant(全参、active)----
                let gid = grant_id_for(cap_name);
                let g: Grant = serde_json::from_value(json!({
                    "grant_id": gid,
                    "audience": "agent:bot", "action": cap_name,
                    "resource": {"capability": cap_name},
                    "scope": "forever", "delegation_depth": 0,
                    "expires_at": null, "revocation_version": 0,
                    "parent_grant_hash": "9b1dec3f2a6c47d5b8e0f1a2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a",
                    "issued_by": "surface:user", "created_at": "2026-08-29T10:00:00.000Z"
                }))
                .unwrap();
                grants.restore(g, 0, false);
                let broker = Broker::new(&reg, &mut grants, &clock, &ids);
                let d = broker.decide(&ctx, cap_name, &json!({}));
                let expected_with_grant: Decision = if trust == DataTrust::Trusted {
                    match risk {
                        RiskClass::ReadOnly | RiskClass::LowRiskCommand => {
                            Decision::Allowed { grant_id: None }
                        }
                        _ => Decision::RequireApproval {
                            risk_class: risk,
                            effective_risk: risk,
                        },
                    }
                } else {
                    Decision::Allowed {
                        grant_id: Some(grant_id_for(cap_name)),
                    }
                };
                assert_eq!(d, expected_with_grant, "{cap_name} × {trust:?}(有 Grant)");
                grants.revoke(&grant_id_for(cap_name)).unwrap();
            }
        }
        // 量化门槛硬断言(ADR-0002 条件 3:100%)
        assert_eq!(
            untrusted_approval_cases, untrusted_reversible_plus_cases,
            "untrusted→reversible+ 升级率必须 100%"
        );
        assert_eq!(
            untrusted_reversible_plus_cases, 3,
            "reversible+ 三级全在矩阵内"
        );
    }

    /// 构造层守护:内容链声称 trusted 被拒(「untrusted 标注为 trusted 视为
    /// 编程错误」,规格 §5.4 守护测试);surface 是唯一 trusted 来源。
    #[test]
    fn content_chain_cannot_claim_trusted() {
        assert!(CallContext::content_chain("agent:bot", DataTrust::Trusted).is_err());
        assert!(CallContext::content_chain("agent:bot", DataTrust::AgentDerived).is_ok());
        assert!(CallContext::content_chain("agent:bot", DataTrust::Untrusted).is_ok());
        let ctx = CallContext::surface("surface:user");
        assert_eq!(ctx.trust, DataTrust::Trusted);
    }

    /// PI-02(间接注入-邮件正文):untrusted 内容夹带的转发指令不执行为指令;
    /// 作为内容链请求时 reversible+ 一律升级审批(A2/A3)。
    #[test]
    fn test_pi_02_indirect_injection_escalates() {
        let reg = five_capability_registry();
        let mut grants = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::content_chain("agent:mail", DataTrust::Untrusted).unwrap();
        let d = broker.decide(&ctx, "system.ext", &json!({"to": "attacker@x"}));
        assert!(
            matches!(d, Decision::RequireApproval { .. }),
            "PI-02: {d:?}"
        );
    }

    /// PI-03(工具结果注入"管理员授权免审批"):权限只能来自注册合同;
    /// 内容里的授权声明不改变 Broker 裁决(untrusted 无 Grant → 升级审批)。
    #[test]
    fn test_pi_03_admin_claim_does_not_bypass() {
        let reg = five_capability_registry();
        let mut grants = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::content_chain("agent:web", DataTrust::Untrusted).unwrap();
        let d = broker.decide(&ctx, "system.rev", &json!({"granted_by": "admin"}));
        assert!(
            matches!(d, Decision::RequireApproval { .. }),
            "PI-03: {d:?}"
        );
    }

    /// PI-06(跨域诱惑):无目标域授权的 agent 请求高危操作 → 强制审批
    /// (用户把关即拒绝语义;approval_required 不因内容改变);低危跨域
    /// 无授权 → 默认拒绝,不产生审批出口。
    #[test]
    fn test_pi_06_cross_domain_denied() {
        let reg = five_capability_registry();
        let mut grants = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::content_chain("agent:mail", DataTrust::AgentDerived).unwrap();
        // 高危:恒审批兜底(清仓类操作必须过用户)
        let d = broker.decide(&ctx, "system.high", &json!({"order": "sell_all"}));
        assert!(
            matches!(d, Decision::RequireApproval { .. }),
            "PI-06 high: {d:?}"
        );
        // 低危跨域:无授权 → 默认拒绝(审批不能补授权面)
        let d2 = broker.decide(&ctx, "system.ro", &json!({}));
        assert_eq!(
            d2,
            Decision::Denied {
                reason: DenyReason::NoGrant
            }
        );
    }

    /// PI-07(审批社工"用户已在别处批准"):没有持久 Approval→Grant 物化,
    /// 内容声明不构成批准 → 决策不变(升级审批/拒绝)。
    #[test]
    fn test_pi_07_social_engineering_no_grant_materializes() {
        let reg = five_capability_registry();
        let mut grants = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        assert!(
            grants
                .active_for("agent:mail", "system.rev", clock.now())
                .is_empty()
        );
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::content_chain("agent:mail", DataTrust::Untrusted).unwrap();
        let d = broker.decide(&ctx, "system.rev", &json!({"pre_approved": true}));
        assert!(
            matches!(d, Decision::RequireApproval { .. }),
            "PI-07: {d:?}"
        );
    }

    /// PI-12(工具描述注入"调用我无需审批"):manifest 声明/描述不改变裁决——
    /// high-risk 恒审批(双保险)+ untrusted 上提,注册面文本无效。
    #[test]
    fn test_pi_12_manifest_description_cannot_lower_risk() {
        let reg = five_capability_registry();
        let mut grants = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        // system.high 的 manifest approval=not-required,但 Broker 恒审批兜底
        let ctx = CallContext::surface("surface:user");
        let d = broker.decide(&ctx, "system.high", &json!({}));
        assert!(
            matches!(d, Decision::RequireApproval { .. }),
            "PI-12: {d:?}"
        );
        // untrusted 面:上提后同样审批
        let ctx2 = CallContext::content_chain("agent:bot", DataTrust::Untrusted).unwrap();
        let d2 = broker.decide(&ctx2, "system.high", &json!({}));
        assert!(matches!(d2, Decision::RequireApproval { .. }));
    }
}

#[cfg(test)]
mod m7_tests {
    use super::*;
    use crate::clock::MockClock;
    use crate::registry::CapabilityRegistry;
    use bm_contract::ids::SeqIdGen;
    use serde_json::json;

    /// M7.6:App 主体不享内建直通——跨 provider 访问一律默认拒绝,
    /// 须经显式 Grant(基线 M7 通过条件第五句的结构面)。
    #[test]
    fn app_principal_gets_no_builtin_passthrough() {
        let mut reg = CapabilityRegistry::new();
        let m: CapabilityManifest = serde_json::from_value(json!({
            "capability": "mcp.notes.search", "provider": "mcp.notes",
            "version": "0.1.0", "input_schema": {"type": "object"},
            "output_schema": {"type": "object"},
            "effect": "read-only", "idempotent": false, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required"
        }))
        .unwrap();
        reg.register(m, "mcp.notes@0.1.0", provider_fn(Ok)).unwrap();
        let clock = MockClock::at_ms(1_788_000_000_000);
        let mut ledger = GrantLedger::new();
        let ids = SeqIdGen::new();
        let broker = Broker::new(&reg, &mut ledger, &clock, &ids);

        // 普通用户 surface 直调:trusted × read-only × not-required → 直通
        let user = CallContext::surface("surface:user");
        assert!(matches!(
            broker.decide(&user, "mcp.notes.search", &json!({})),
            Decision::Allowed { .. }
        ));

        // App 主体同参直调:默认拒绝(无直通、无 Grant)
        let app = CallContext::surface("surface:app:wiki");
        assert!(matches!(
            broker.decide(&app, "mcp.notes.search", &json!({})),
            Decision::Denied { .. }
        ));

        // 显式 Grant 后放行(App 经批准获得跨 provider 访问)
        let grant = Grant {
            grant_id: ids.next_id("grant").to_string(),
            audience: "surface:app:wiki".into(),
            action: "mcp.notes.search".into(),
            resource: GrantResource {
                capability: "mcp.notes.search".into(),
                args_predicates: Default::default(),
            },
            scope: GrantScope::Forever,
            delegation_depth: 0,
            expires_at: None,
            revocation_version: 0,
            parent_grant_hash: "seed".into(),
            issued_by: "user_grant".into(),
            created_at: "2026-08-30T00:00:00.000Z".into(),
        };
        broker.grants.record(grant);
        assert!(matches!(
            broker.decide(&app, "mcp.notes.search", &json!({})),
            Decision::Allowed { .. }
        ));
    }

    // ---- M9 S1:记忆抽屉主体边界(步 4.5)--------------------------------

    const BASE_MS: u128 = 1_788_000_000_000;

    fn memory_registry() -> CapabilityRegistry {
        let mut reg = CapabilityRegistry::new();
        for (name, effect) in [
            ("memory.write", "low-risk-command"),
            ("memory.search", "read-only"),
        ] {
            let m: CapabilityManifest = serde_json::from_value(json!({
                "capability": name, "provider": "memory", "version": "0.1.0",
                "input_schema": {"type": "object"},
                "output_schema": {"type": "object"},
                "effect": effect, "idempotent": true, "cancellable": true,
                "timeout_ms": 1000, "approval": "not-required"
            }))
            .unwrap();
            reg.register(m, &format!("{name}@0.1.0"), provider_fn(|_| Ok(json!({}))))
                .unwrap();
        }
        reg
    }

    fn drawer_call(
        reg: &CapabilityRegistry,
        grants: &mut GrantLedger,
        principal: &str,
        capability: &str,
        scope: &str,
    ) -> Decision {
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let broker = Broker::new(reg, grants, &clock, &ids);
        let ctx = CallContext::content_chain(principal, DataTrust::Untrusted).unwrap();
        broker.decide(&ctx, capability, &json!({"scope": scope}))
    }

    /// t130:agent 写自己的抽屉 → 常量放行(无需 Grant)。
    #[test]
    fn t130_drawer_agent_own_write_allowed() {
        let reg = memory_registry();
        let mut grants = GrantLedger::new();
        assert!(matches!(
            drawer_call(&reg, &mut grants, "agent:AGENTAGENTAGENTAGENTAG1", "memory.write",
                "memory:agent:AGENTAGENTAGENTAGENTAG1"),
            Decision::Allowed { grant_id: None }
        ));
    }

    /// t131:agent 写 user 抽屉 → 升级审批(不静默拒绝)。
    #[test]
    fn t131_drawer_agent_user_write_escalates() {
        let reg = memory_registry();
        let mut grants = GrantLedger::new();
        assert!(matches!(
            drawer_call(&reg, &mut grants, "agent:AGENTAGENTAGENTAGENTAG1", "memory.write",
                "memory:user"),
            Decision::RequireApproval { .. }
        ));
    }

    /// t133:跨 agent 抽屉 → 升级审批。
    #[test]
    fn t133_drawer_agent_cross_agent_escalates() {
        let reg = memory_registry();
        let mut grants = GrantLedger::new();
        assert!(matches!(
            drawer_call(&reg, &mut grants, "agent:AGENTAGENTAGENTAGENTAG1", "memory.write",
                "memory:agent:AGENTAGENTAGENTAGENTAG2"),
            Decision::RequireApproval { .. }
        ));
    }

    /// t134:task 族成员(coord/worker)只可写本任务抽屉。
    #[test]
    fn t134_drawer_task_members_scoped_to_own_task() {
        let reg = memory_registry();
        let mut grants = GrantLedger::new();
        for principal in ["agent:worker:t_01", "agent:coord:t_01"] {
            assert!(matches!(
                drawer_call(&reg, &mut grants, principal, "memory.write", "memory:task:t_01"),
                Decision::Allowed { grant_id: None }
            ));
            assert!(matches!(
                drawer_call(&reg, &mut grants, principal, "memory.write", "memory:user"),
                Decision::RequireApproval { .. }
            ));
        }
    }

    /// t135:search 放宽——user 抽屉可检索(读不污染),他人抽屉仍升级。
    #[test]
    fn t135_drawer_search_user_allowed_cross_agent_escalates() {
        let reg = memory_registry();
        let mut grants = GrantLedger::new();
        assert!(matches!(
            drawer_call(&reg, &mut grants, "agent:AGENTAGENTAGENTAGENTAG1", "memory.search",
                "memory:user"),
            Decision::Allowed { grant_id: None }
        ));
        assert!(matches!(
            drawer_call(&reg, &mut grants, "agent:AGENTAGENTAGENTAGENTAG1", "memory.search",
                "memory:agent:AGENTAGENTAGENTAGENTAG2"),
            Decision::RequireApproval { .. }
        ));
    }

    /// t132 前半:显式 Grant(带 scope 谓词)命中优先于步 4.5——批准一次,
    /// 之后同抽屉调用走 Grant 台账(Allowed 且携带 grant_id)。
    #[test]
    fn t132_drawer_explicit_grant_predicates_match_before_drawer_step() {
        let reg = memory_registry();
        let mut grants = GrantLedger::new();
        let mut g = crate::butler::model_grant_for(&SeqIdGen::new(), "AGENTAGENTAGENTAGENTAG1",
            MockClock::at_ms(BASE_MS).now());
        g.action = "memory.write".into();
        g.resource.args_predicates.insert(
            "scope".into(),
            json!("memory:user"),
        );
        grants.record(g.clone());
        assert!(matches!(
            drawer_call(&reg, &mut grants, "agent:AGENTAGENTAGENTAGENTAG1", "memory.write",
                "memory:user"),
            Decision::Allowed { grant_id: Some(_) }
        ));
    }

    /// user Surface 直写不受步 4.5 影响(既有直通/审批流原样)。
    #[test]
    fn drawer_surface_user_unchanged() {
        let reg = memory_registry();
        let mut grants = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        let ctx = CallContext::surface("surface:user");
        assert!(matches!(
            broker.decide(&ctx, "memory.search", &json!({"scope": "memory:user"})),
            Decision::Allowed { .. }
        ));
    }
}
