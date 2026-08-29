//! Approval 状态机与 Grant 物化(M4.5/M4.7,基线 §9.6;ADR-0002 条件 1)。
//!
//! 审批是持久合同对象而非一次性弹窗:requested → waiting_user → approved |
//! denied;waiting_user 超时 → expired(**等价 denied**,任何配置不允许超时
//! 默认同意);调用方取消 → withdrawn。等待审批的 Operation 处于
//! waiting_approval(M4 增发三边,core-transitions)。
//!
//! 批准即物化 Grant(ADR-0002 条件 1 下限字段集):parent_grant_hash =
//! 裁决前 Approval 对象内容 SHA-256(授权链可重建);delegation_depth 恒 0;
//! M4 issued_by 恒为用户 Surface 身份(Wire approval.respond 即用户本人),
//! Coordinator 签发链随 M5。审计事件(approval.requested/resolved/expired)
//! 由核心循环单写者落盘(T3b 接线),本模块只做状态与记账。

use crate::broker::GrantLedger;
use crate::clock::Clock;
use bm_contract::capability::{
    Approval, ApprovalState, DataTrust, Grant, GrantResource, GrantScope, RiskClass,
};
use bm_contract::ids::IdGen;
use bm_contract::timestamp::format_ts;
use sha2::{Digest, Sha256};

/// 审批裁决(用户经 Wire approval.respond;GUI/CLI 同源)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespondDecision {
    Approve,
    Deny,
    Withdraw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalError {
    /// 对象已终态(approved/denied/expired/withdrawn),不可再裁决。
    AlreadyResolved,
    /// approve 未带 scope,或 scope 不在该对象的 scope_choices。
    ScopeNotAllowed,
    /// M4 曾拒 task: 前缀 scope(M5 起启用;变体保留供恢复面兼容)。
    TaskScopeUnavailable,
    /// 裁决窗口已过(expired 等价 denied):不得再批准。
    Expired,
}

/// open 的入参(args_summary 由调用方(Broker 路径)生成脱敏摘要)。
pub struct OpenApproval<'a> {
    pub capability: &'a str,
    pub principal: &'a str,
    pub risk_class: RiskClass,
    pub effective_risk: RiskClass,
    pub input_trust: DataTrust,
    pub args: &'a serde_json::Value,
    pub args_summary: &'a str,
    pub scope_choices: Vec<GrantScope>,
    /// 裁决窗口时长(毫秒);到期即 expired。
    pub ttl_ms: u64,
}

pub struct ApprovalManager<'a> {
    ledger: &'a mut GrantLedger,
    clock: &'a dyn Clock,
    ids: &'a dyn IdGen,
}

impl<'a> ApprovalManager<'a> {
    pub fn new(ledger: &'a mut GrantLedger, clock: &'a dyn Clock, ids: &'a dyn IdGen) -> Self {
        Self { ledger, clock, ids }
    }

    /// 创建审批对象(state=waiting_user:requested 即刻进入等待,同步路径)。
    /// 返回对象由调用方持久化(approvals 表)并发 approval.requested 事件。
    pub fn open(&mut self, p: OpenApproval<'_>) -> Approval {
        let now = self.clock.now();
        Approval {
            approval_id: self.ids.next_id("appr").to_string(),
            capability: p.capability.to_string(),
            args_digest: sha256_hex(&serde_json::to_string(p.args).unwrap_or_default()),
            args_summary: p.args_summary.to_string(),
            principal: p.principal.to_string(),
            risk_class: p.risk_class,
            effective_risk: p.effective_risk,
            input_trust: p.input_trust,
            state: ApprovalState::WaitingUser,
            scope_choices: p.scope_choices,
            requested_at: format_ts(now),
            expires_at: format_ts(now + chrono::Duration::milliseconds(p.ttl_ms as i64)),
            resolved_at: None,
            grant_id: None,
        }
    }

    /// 裁决:批准 = 物化 Grant 并入台账;拒绝/取消 = 终态落定。
    /// `resource` 为物化 Grant 的资源谓词(M4 调用方传能力名 + 空谓词 =
    /// 全参授权,以 scope 次数约束;M5 审批 UI 收窄)。
    /// `issued_by` 为签发者身份(M4 = 用户 Surface 身份)。
    /// 成功后对象已更新(state/resolved_at/grant_id),调用方重新持久化。
    pub fn respond(
        &mut self,
        approval: &mut Approval,
        decision: RespondDecision,
        scope: Option<GrantScope>,
        resource: GrantResource,
        issued_by: &str,
    ) -> Result<Option<Grant>, ApprovalError> {
        if approval.state != ApprovalState::WaitingUser {
            return Err(ApprovalError::AlreadyResolved);
        }
        let now = self.clock.now();
        if Self::is_expired(approval, now) {
            approval.state = ApprovalState::Expired;
            approval.resolved_at = Some(format_ts(now));
            return Err(ApprovalError::Expired);
        }
        match decision {
            RespondDecision::Approve => {
                let Some(scope) = scope else {
                    return Err(ApprovalError::ScopeNotAllowed);
                };
                // M5 起启用 task:<id> scope(解读条款 4 兑现):引用存在性
                // 由调用方(运行时任务表)校验;choices 约束仅约束固定枚举面
                if !matches!(scope, GrantScope::Task(_)) && !approval.scope_choices.contains(&scope)
                {
                    return Err(ApprovalError::ScopeNotAllowed);
                }
                // 父授权哈希 = 裁决前(waiting_user 形态)对象内容 SHA-256
                let parent_hash = sha256_hex(&serde_json::to_string(approval).unwrap_or_default());
                approval.state = ApprovalState::Approved;
                approval.resolved_at = Some(format_ts(now));
                let grant = Grant {
                    grant_id: self.ids.next_id("grant").to_string(),
                    audience: approval.principal.clone(),
                    action: approval.capability.clone(),
                    resource,
                    expires_at: match scope {
                        GrantScope::Ttl(ms) => {
                            Some(format_ts(now + chrono::Duration::milliseconds(ms as i64)))
                        }
                        _ => None,
                    },
                    scope,
                    delegation_depth: 0, // 不可再转授(基线 §11.2)
                    revocation_version: 0,
                    parent_grant_hash: parent_hash,
                    issued_by: issued_by.to_string(),
                    created_at: format_ts(now),
                };
                approval.grant_id = Some(grant.grant_id.clone());
                self.ledger.record(grant.clone());
                Ok(Some(grant))
            }
            RespondDecision::Deny => {
                approval.state = ApprovalState::Denied;
                approval.resolved_at = Some(format_ts(now));
                Ok(None)
            }
            RespondDecision::Withdraw => {
                approval.state = ApprovalState::Withdrawn;
                approval.resolved_at = Some(format_ts(now));
                Ok(None)
            }
        }
    }

    /// 到期检查(waiting_user 对象;调用方在扫描/响应路径上触发)。
    /// 返回是否刚刚转为 expired。
    pub fn expire_if_due(&self, approval: &mut Approval) -> bool {
        if approval.state != ApprovalState::WaitingUser {
            return false;
        }
        if Self::is_expired(approval, self.clock.now()) {
            approval.state = ApprovalState::Expired;
            approval.resolved_at = Some(format_ts(self.clock.now()));
            return true;
        }
        false
    }

    fn is_expired(approval: &Approval, now: chrono::DateTime<chrono::Utc>) -> bool {
        match bm_contract::timestamp::parse_ts(&approval.expires_at) {
            Some(t) => t <= now,
            None => true, // 无法解析的截止时间按已过期(安全侧)
        }
    }
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
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
    use crate::clock::MockClock;
    use bm_contract::ids::SeqIdGen;
    use serde_json::json;

    const BASE_MS: u128 = 1_788_000_000_000; // 2026-08-29T10:40:00.000Z

    fn open_params<'a>(args: &'a serde_json::Value, summary: &'a str) -> OpenApproval<'a> {
        OpenApproval {
            capability: "system.notes.write",
            principal: "agent:note_bot",
            risk_class: RiskClass::ReversibleCommand,
            effective_risk: RiskClass::ExternalSideEffect,
            input_trust: DataTrust::Untrusted,
            args,
            args_summary: summary,
            scope_choices: vec![
                GrantScope::Once,
                GrantScope::Count(3),
                GrantScope::Ttl(60_000),
            ],
            ttl_ms: 300_000,
        }
    }

    #[test]
    fn open_creates_waiting_user_with_digest_and_deadline() {
        let mut ledger = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let mut mgr = ApprovalManager::new(&mut ledger, &clock, &ids);
        let args = json!({"path": "notes/inbox.md"});
        let a = mgr.open(open_params(&args, "写笔记 inbox.md"));
        assert_eq!(a.state, ApprovalState::WaitingUser);
        assert_eq!(
            a.args_digest,
            sha256_hex(&serde_json::to_string(&args).unwrap()),
            "args 摘要 = 规范化 JSON 的 SHA-256"
        );
        assert_eq!(a.expires_at, "2026-08-29T10:45:00.000Z");
        assert!(a.grant_id.is_none());
    }

    #[test]
    fn approve_materializes_grant_with_parent_hash() {
        let mut ledger = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let mut mgr = ApprovalManager::new(&mut ledger, &clock, &ids);
        let mut a = mgr.open(open_params(&json!({}), "写笔记"));
        let parent_hash = sha256_hex(&serde_json::to_string(&a).unwrap());
        let resource = GrantResource {
            capability: "system.notes.write".into(),
            args_predicates: Default::default(),
        };
        let grant = mgr
            .respond(
                &mut a,
                RespondDecision::Approve,
                Some(GrantScope::Count(3)),
                resource.clone(),
                "surface:user",
            )
            .unwrap()
            .expect("approve 应物化 Grant");

        assert_eq!(a.state, ApprovalState::Approved);
        assert_eq!(a.grant_id.as_deref(), Some(grant.grant_id.as_str()));
        assert_eq!(
            grant.parent_grant_hash, parent_hash,
            "父哈希 = 裁决前对象内容"
        );
        assert_eq!(grant.audience, "agent:note_bot");
        assert_eq!(grant.action, "system.notes.write");
        assert_eq!(grant.scope, GrantScope::Count(3));
        assert_eq!(grant.delegation_depth, 0);
        assert_eq!(grant.issued_by, "surface:user");
        assert_eq!(grant.expires_at, None, "Count 授权无时限");
        // 终态后再裁决 → AlreadyResolved
        assert_eq!(
            mgr.respond(
                &mut a,
                RespondDecision::Deny,
                None,
                resource,
                "surface:user"
            ),
            Err(ApprovalError::AlreadyResolved)
        );
        // 台账可查(audience×action 索引;mgr 最后使用后 // 台账可查(audience×action 索引)mut 借用即结束)
        assert_eq!(
            ledger
                .active_for("agent:note_bot", "system.notes.write", clock.now())
                .len(),
            1
        );
    }

    #[test]
    fn approve_requires_choice_scope_and_rejects_task_scope() {
        let mut ledger = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let mut mgr = ApprovalManager::new(&mut ledger, &clock, &ids);
        let resource = GrantResource {
            capability: "system.notes.write".into(),
            args_predicates: Default::default(),
        };
        // approve 不带 scope
        let mut a = mgr.open(open_params(&json!({}), "写笔记"));
        assert_eq!(
            mgr.respond(
                &mut a,
                RespondDecision::Approve,
                None,
                resource.clone(),
                "surface:user"
            ),
            Err(ApprovalError::ScopeNotAllowed)
        );
        // scope 不在 choices
        let mut a = mgr.open(open_params(&json!({}), "写笔记"));
        assert_eq!(
            mgr.respond(
                &mut a,
                RespondDecision::Approve,
                Some(GrantScope::Forever),
                resource.clone(),
                "surface:user"
            ),
            Err(ApprovalError::ScopeNotAllowed)
        );
        // M5 起 task:<id> scope 启用(解读条款 4 兑现):管理器接受并物化,
        // 引用存在性由运行时任务表校验(此处无 Task 语境,直接验证物化形态)
        let mut a = mgr.open(open_params(&json!({}), "写笔记"));
        let grant = mgr
            .respond(
                &mut a,
                RespondDecision::Approve,
                Some(GrantScope::Task("task_01JAAAAAAAAAAAAAAAAAAAAAB2".into())),
                resource,
                "surface:user",
            )
            .unwrap()
            .expect("task scope 批准应物化 Grant");
        assert!(matches!(grant.scope, GrantScope::Task(_)));
        assert_eq!(grant.delegation_depth, 0);
    }

    #[test]
    fn deny_and_withdraw_transitions() {
        let mut ledger = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let mut mgr = ApprovalManager::new(&mut ledger, &clock, &ids);
        let resource = GrantResource {
            capability: "system.notes.write".into(),
            args_predicates: Default::default(),
        };
        let mut a = mgr.open(open_params(&json!({}), "写笔记"));
        assert_eq!(
            mgr.respond(
                &mut a,
                RespondDecision::Deny,
                None,
                resource.clone(),
                "surface:user"
            ),
            Ok(None)
        );
        assert_eq!(a.state, ApprovalState::Denied);
        assert!(a.resolved_at.is_some());

        let mut b = mgr.open(open_params(&json!({}), "写笔记"));
        assert_eq!(
            mgr.respond(
                &mut b,
                RespondDecision::Withdraw,
                None,
                resource,
                "surface:user"
            ),
            Ok(None)
        );
        assert_eq!(b.state, ApprovalState::Withdrawn);
    }

    #[test]
    fn expiry_blocks_approval() {
        let mut ledger = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let mut mgr = ApprovalManager::new(&mut ledger, &clock, &ids);
        let resource = GrantResource {
            capability: "system.notes.write".into(),
            args_predicates: Default::default(),
        };
        // 场景一:窗口已过、经 expire_if_due 扫描 → Expired 终态
        let mut a = mgr.open(open_params(&json!({}), "写笔记"));
        assert!(!mgr.expire_if_due(&mut a));
        clock.advance_ms(301_000);
        assert!(mgr.expire_if_due(&mut a));
        assert_eq!(a.state, ApprovalState::Expired);
        // 终态不得再裁决(Expired 是终态,守卫先于一切)
        assert_eq!(
            mgr.respond(
                &mut a,
                RespondDecision::Approve,
                Some(GrantScope::Once),
                resource.clone(),
                "surface:user"
            ),
            Err(ApprovalError::AlreadyResolved)
        );
        // 场景二:窗口已过但对象未被扫描 → respond 内联过期检查兜底,
        // expired 等价 denied,不得批准(无超时默认同意)
        let mut b = mgr.open(open_params(&json!({}), "写笔记"));
        clock.advance_ms(301_000);
        assert_eq!(
            mgr.respond(
                &mut b,
                RespondDecision::Approve,
                Some(GrantScope::Once),
                resource,
                "surface:user"
            ),
            Err(ApprovalError::Expired)
        );
        assert_eq!(b.state, ApprovalState::Expired);
    }

    #[test]
    fn ttl_scope_grant_carries_deadline() {
        let mut ledger = GrantLedger::new();
        let clock = MockClock::at_ms(BASE_MS);
        let ids = SeqIdGen::new();
        let mut mgr = ApprovalManager::new(&mut ledger, &clock, &ids);
        let mut a = mgr.open(open_params(&json!({}), "写笔记"));
        let resource = GrantResource {
            capability: "system.notes.write".into(),
            args_predicates: Default::default(),
        };
        let grant = mgr
            .respond(
                &mut a,
                RespondDecision::Approve,
                Some(GrantScope::Ttl(60_000)),
                resource,
                "surface:user",
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            grant.expires_at.as_deref(),
            Some("2026-08-29T10:41:00.000Z")
        );
    }
}
