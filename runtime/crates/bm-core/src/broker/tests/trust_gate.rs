//! 信任门测试(自 broker.rs 机械移入)。

use crate::broker::*;
use crate::clock::MockClock;
use bm_contract::capability::CapabilityManifest;
use bm_contract::capability::Grant;
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
