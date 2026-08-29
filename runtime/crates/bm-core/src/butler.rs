//! Butler 内置 App(M5.1,基线 §10;ADR-0002:仅持系统协调权)。
//!
//! Butler 是真实 App 但不是超级 Agent:默认不拥有任何领域操作权。其协调权
//! 在系统引导期物化为 bootstrap Grant 集(基线 §10.1 的 12 个协调动词,
//! audience=butler:system,issued_by=runtime_bootstrap,scope=forever,审计
//! 可溯、可撤销)。撤销后 Butler 仅剩只读查询(safe 动词由查询面结构承载,
//! 不依赖 Grant);重授走审批(交互形态随 M8 审批 UI,规格 §9)。
//!
//! 权限上界:Task 授权声明的动词必须 ⊆ 本清单(合同 enum 已约束,运行期
//! 在 task.create 入口二次校验)——Butler 不得签发超出自身上界的授权。

use crate::broker::GrantLedger;
use crate::clock::Clock;
use bm_contract::capability::{Grant, GrantResource, GrantScope};
use bm_contract::ids::IdGen;
use bm_contract::timestamp::format_ts;
use sha2::{Digest, Sha256};

/// Butler 的系统身份(principal)。
pub const BUTLER_PRINCIPAL: &str = "butler:system";

/// bootstrap Grant 的签发者标识(非用户、非任何 Agent;审计区分引导权)。
pub const BOOTSTRAP_ISSUER: &str = "runtime_bootstrap";

/// 协调动词二分(基线 §10.1 清单 × ADR-0002 §11.2 safe/mutation 分级)。
/// safe = 只读查询、状态查询、结果收集(可默认继承);
/// mutation = 生命周期控制与团队组建(须在 Task 授权中显式列出)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinationClass {
    Safe,
    Mutation,
}

/// 协调动词全集与默认分级(与 task/task.v0.1 合同 enum 一一对应)。
pub const COORDINATION_VERBS: [(&str, CoordinationClass); 12] = [
    ("task.create", CoordinationClass::Mutation),
    ("task.cancel", CoordinationClass::Mutation),
    ("agent.spawn", CoordinationClass::Mutation),
    ("agent.pause", CoordinationClass::Mutation),
    ("agent.resume", CoordinationClass::Mutation),
    ("agent.stop", CoordinationClass::Mutation),
    ("agent.watch", CoordinationClass::Safe),
    ("team.create", CoordinationClass::Mutation),
    ("capability.discover", CoordinationClass::Safe),
    ("event.subscribe", CoordinationClass::Safe),
    ("task.collect", CoordinationClass::Safe),
    ("capability.call", CoordinationClass::Mutation),
];

/// 动词默认分级;非协调动词(领域动词如 mail.read)返回 None = 不可授权。
pub fn verb_class(verb: &str) -> Option<CoordinationClass> {
    COORDINATION_VERBS
        .iter()
        .find(|(v, _)| *v == verb)
        .map(|(_, c)| *c)
}

/// bootstrap 父授权哈希:固定引导标记的 SHA-256(无 Approval 父对象)。
pub fn bootstrap_parent_hash() -> String {
    let mut h = Sha256::new();
    h.update(BOOTSTRAP_ISSUER.as_bytes());
    let out = h.finalize();
    let mut hex = String::with_capacity(64);
    for b in out {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// 构造一个 bootstrap 协调权 Grant(scope=forever,delegation_depth=0)。
pub fn bootstrap_grant(ids: &dyn IdGen, verb: &str, now: chrono::DateTime<chrono::Utc>) -> Grant {
    Grant {
        grant_id: ids.next_id("grant").to_string(),
        audience: BUTLER_PRINCIPAL.to_string(),
        action: verb.to_string(),
        resource: GrantResource {
            capability: verb.to_string(),
            args_predicates: Default::default(),
        },
        scope: GrantScope::Forever,
        delegation_depth: 0, // 不可再转授(基线 §11.2)
        expires_at: None,
        revocation_version: 0,
        parent_grant_hash: bootstrap_parent_hash(),
        issued_by: BOOTSTRAP_ISSUER.to_string(),
        created_at: format_ts(now),
    }
}

/// 引导期物化:对清单中每个动词,若台账/持久行中尚无该 (audience, action)
/// 的 Grant(含已撤销),签发 bootstrap Grant 并返回之(调用方落库 + 发
/// grant.created 事件)。已撤销的不再复活——撤销是持久事实。
pub fn materialize_missing(
    ledger: &mut GrantLedger,
    existing_pairs: &[(String, String)],
    ids: &dyn IdGen,
    clock: &dyn Clock,
) -> Vec<Grant> {
    let mut issued = Vec::new();
    for (verb, _) in COORDINATION_VERBS {
        let pair = (BUTLER_PRINCIPAL.to_string(), verb.to_string());
        if existing_pairs.contains(&pair)
            || !ledger.active_for(&pair.0, &pair.1, clock.now()).is_empty()
        {
            continue;
        }
        let grant = bootstrap_grant(ids, verb, clock.now());
        ledger.record(grant.clone());
        issued.push(grant);
    }
    issued
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::provider_fn;
    use crate::clock::MockClock;
    use crate::registry::CapabilityRegistry;
    use bm_contract::capability::CapabilityManifest;
    use bm_contract::ids::SeqIdGen;
    use serde_json::json;

    const BASE_MS: u128 = 1_788_000_000_000;

    fn manifest(name: &str, effect: &str) -> CapabilityManifest {
        serde_json::from_value(json!({
            "capability": name, "provider": name, "version": "0.1.0",
            "input_schema": {"type": "object"},
            "output_schema": {"type": "object"},
            "effect": effect, "idempotent": true, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required"
        }))
        .unwrap()
    }

    #[test]
    fn verb_classification_matches_contract_and_rejects_domain_verbs() {
        assert_eq!(verb_class("task.collect"), Some(CoordinationClass::Safe));
        assert_eq!(verb_class("agent.spawn"), Some(CoordinationClass::Mutation));
        assert_eq!(verb_class("task.create"), Some(CoordinationClass::Mutation));
        // 领域动词不在协调清单 = 不可授权(Butler 上界)
        assert_eq!(verb_class("mail.read"), None);
        assert_eq!(verb_class("mail.send"), None);
        assert_eq!(verb_class("stock.place_order"), None);
        assert_eq!(COORDINATION_VERBS.len(), 12, "基线 §10.1 全集");
    }

    #[test]
    fn bootstrap_grants_are_forever_and_non_transferable() {
        let ids = SeqIdGen::new();
        let clock = MockClock::at_ms(BASE_MS);
        let g = bootstrap_grant(&ids, "task.create", clock.now());
        assert_eq!(g.audience, "butler:system");
        assert_eq!(g.issued_by, "runtime_bootstrap");
        assert_eq!(g.scope, GrantScope::Forever);
        assert_eq!(g.delegation_depth, 0);
        assert_eq!(g.parent_grant_hash, bootstrap_parent_hash());
        assert_eq!(g.action, "task.create");
    }

    #[test]
    fn materialize_is_idempotent_and_respects_revocation() {
        let mut ledger = GrantLedger::new();
        let ids = SeqIdGen::new();
        let clock = MockClock::at_ms(BASE_MS);
        let existing: Vec<(String, String)> = Vec::new();
        let first = materialize_missing(&mut ledger, &existing, &ids, &clock);
        assert_eq!(first.len(), 12, "首次引导签发全集");
        // 二次引导(空持久行):台账已有 active → 不重复签发
        let second = materialize_missing(&mut ledger, &existing, &ids, &clock);
        assert!(second.is_empty(), "幂等:已有 Grant 不重发");
        // 撤销一个后再引导:该动词不复活(撤销是持久事实——调用方须把
        // 持久行对(含已撤销行)传入 existing_pairs,启动路径即如此)
        let gid = first.iter().find(|g| g.action == "task.cancel").unwrap();
        ledger.revoke(&gid.grant_id).unwrap();
        let with_revoked = vec![("butler:system".to_string(), "task.cancel".to_string())];
        let third = materialize_missing(&mut ledger, &with_revoked, &ids, &clock);
        assert!(
            third.iter().all(|g| g.action != "task.cancel"),
            "已撤销的 bootstrap Grant 不复活"
        );
        assert!(third.is_empty(), "其余动词仍由台账 active 承载");
    }

    #[test]
    fn butler_agent_context_gets_no_blanket_domain_power() {
        // 权限矩阵(基线 M5 通过条件第 1 条):Butler 以 agent-derived 上下文
        // 调用领域/变更能力 → 无 Grant 一律升级审批,不存在免审通道;
        // 其全部特权 = bootstrap Grant 集(协调动词),与 Broker 同构无旁路。
        let mut registry = CapabilityRegistry::new();
        let mut ledger = GrantLedger::new();
        let ids = SeqIdGen::new();
        let clock = MockClock::at_ms(BASE_MS);
        registry
            .register(
                manifest("system.mail.mock_send", "external-side-effect"),
                "system.mail.mock_send@0.1.0",
                provider_fn(|_| Ok(json!({"sent": true}))),
            )
            .unwrap();
        registry
            .register(
                manifest("system.notes.write", "reversible-command"),
                "system.notes.write@0.1.0",
                provider_fn(|_| Ok(json!({"written": true}))),
            )
            .unwrap();

        // Butler 引导:只有协调动词 Grant,没有任何领域能力 Grant
        let _ = materialize_missing(&mut ledger, &[], &ids, &clock);
        assert!(
            ledger
                .active_for("butler:system", "system.mail.mock_send", clock.now())
                .is_empty()
        );
        assert!(
            ledger
                .active_for("butler:system", "system.notes.write", clock.now())
                .is_empty()
        );

        let broker = crate::broker::Broker::new(&registry, &mut ledger, &clock, &ids);
        // agent-derived(untrusted)× external-side-effect → 100% 升级审批
        let ctx = crate::broker::CallContext::content_chain(
            "butler:system",
            bm_contract::capability::DataTrust::Untrusted,
        )
        .unwrap();
        match broker.decide(&ctx, "system.mail.mock_send", &json!({"to": "a@x"})) {
            crate::broker::Decision::RequireApproval { .. } => {}
            other => panic!("领域副作用调用必须升级审批,实际 {other:?}"),
        }
        match broker.decide(&ctx, "system.notes.write", &json!({"p": 1})) {
            crate::broker::Decision::RequireApproval { .. } => {}
            other => panic!("领域可逆写必须升级审批,实际 {other:?}"),
        }
        // 协调动词本身不是 Broker 可调能力(Registry 无此 manifest)→ 查表
        // 未命中 = 默认拒绝;协调权由 Grant 台账承载、在命令入口校验(T4/T5)
        match broker.decide(&ctx, "task.create", &json!({})) {
            crate::broker::Decision::Denied { .. } => {}
            other => panic!("未知能力必须默认拒绝,实际 {other:?}"),
        }
    }
}
