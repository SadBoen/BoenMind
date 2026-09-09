//! Broker 决策与执行的公共类型(自 broker.rs 机械移入;条目与行序原样)。
use crate::registry::CapabilityProvider;
use bm_contract::capability::{CapabilityManifest, DataTrust, RiskClass};
use bm_contract::ids::BmId;
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
    /// 发起调用的会话(ADR-0030):回合层模型工具调用携带,供裁决点读取
    /// 会话权限模式;None = 无会话上下文(wire 直调/worker 路径,恒按 ask)。
    /// 仅路由信息,不参与信任归因——提升权限的决定永远不在调用方。
    pub session_id: Option<BmId>,
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
            session_id: None,
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
                session_id: None,
            }),
        }
    }

    /// 附幂等键(副作用操作必备,基线 §9.5)。
    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// 附会话归属(ADR-0030):回合层模型工具调用标注来源会话,裁决点
    /// 据此读取该会话的权限模式。仅路由信息,不改信任。
    pub fn with_session(mut self, session_id: BmId) -> Self {
        self.session_id = Some(session_id);
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
    pub(super) handle: Arc<dyn CapabilityProvider>,
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
