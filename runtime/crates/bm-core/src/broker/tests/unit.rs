//! Broker 单元测试(自 broker.rs 机械移入)。

use crate::broker::*;
use crate::clock::MockClock;
use crate::registry::CapabilityRegistry;
use bm_contract::capability::{Grant, GrantScope};
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

fn grant_of(audience: &str, action: &str, scope: GrantScope, preds: serde_json::Value) -> Grant {
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
    let ctx = CallContext::content_chain("agent:bot", DataTrust::Untrusted).expect("内容链构造");
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
    let ctx = CallContext::content_chain("agent:bot", DataTrust::AgentDerived).expect("内容链构造");
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
    let ctx = CallContext::content_chain("agent:bot", DataTrust::AgentDerived).expect("内容链构造");
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

    let ctx = CallContext::content_chain("agent:bot", DataTrust::AgentDerived).expect("内容链构造");
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
