//! M7 熔断/租约/抽屉测试(自 broker.rs 机械移入)。

use crate::broker::*;
use crate::clock::MockClock;
use crate::registry::CapabilityRegistry;
use bm_contract::capability::{Grant, GrantResource, GrantScope};
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
        drawer_call(
            &reg,
            &mut grants,
            "agent:AGENTAGENTAGENTAGENTAG1",
            "memory.write",
            "memory:agent:AGENTAGENTAGENTAGENTAG1"
        ),
        Decision::Allowed { grant_id: None }
    ));
}

/// t131:agent 写 user 抽屉 → 升级审批(不静默拒绝)。
#[test]
fn t131_drawer_agent_user_write_escalates() {
    let reg = memory_registry();
    let mut grants = GrantLedger::new();
    assert!(matches!(
        drawer_call(
            &reg,
            &mut grants,
            "agent:AGENTAGENTAGENTAGENTAG1",
            "memory.write",
            "memory:user"
        ),
        Decision::RequireApproval { .. }
    ));
}

/// t133:跨 agent 抽屉 → 升级审批。
#[test]
fn t133_drawer_agent_cross_agent_escalates() {
    let reg = memory_registry();
    let mut grants = GrantLedger::new();
    assert!(matches!(
        drawer_call(
            &reg,
            &mut grants,
            "agent:AGENTAGENTAGENTAGENTAG1",
            "memory.write",
            "memory:agent:AGENTAGENTAGENTAGENTAG2"
        ),
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
            drawer_call(
                &reg,
                &mut grants,
                principal,
                "memory.write",
                "memory:task:t_01"
            ),
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
        drawer_call(
            &reg,
            &mut grants,
            "agent:AGENTAGENTAGENTAGENTAG1",
            "memory.search",
            "memory:user"
        ),
        Decision::Allowed { grant_id: None }
    ));
    assert!(matches!(
        drawer_call(
            &reg,
            &mut grants,
            "agent:AGENTAGENTAGENTAGENTAG1",
            "memory.search",
            "memory:agent:AGENTAGENTAGENTAGENTAG2"
        ),
        Decision::RequireApproval { .. }
    ));
}

/// t132 前半:显式 Grant(带 scope 谓词)命中优先于步 4.5——批准一次,
/// 之后同抽屉调用走 Grant 台账(Allowed 且携带 grant_id)。
#[test]
fn t132_drawer_explicit_grant_predicates_match_before_drawer_step() {
    let reg = memory_registry();
    let mut grants = GrantLedger::new();
    let mut g = crate::butler::model_grant_for(
        &SeqIdGen::new(),
        "AGENTAGENTAGENTAGENTAG1",
        MockClock::at_ms(BASE_MS).now(),
    );
    g.action = "memory.write".into();
    g.resource
        .args_predicates
        .insert("scope".into(), json!("memory:user"));
    grants.record(g.clone());
    assert!(matches!(
        drawer_call(
            &reg,
            &mut grants,
            "agent:AGENTAGENTAGENTAGENTAG1",
            "memory.write",
            "memory:user"
        ),
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
