//! M4-T8:架构守护三件套(常驻 CI;硬约束 7;ADR-0001 条件 7)。
//! G1 = Bus 不得当 RPC(命令语义事件持久化前拒绝——承载于
//!      bm-core runtime.rs t7_event_shape_tests,本文件补端到端复述);
//! G2 = 审批/Task 命令混入事件流即失败(事件流只含已裁决事实);
//! G3 = 混层缓存超出「可丢失运行时缓存」范畴即失败(清空缓存后行为一致)。

use bm_contract::capability::CapabilityManifest;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_core::CoreError;
use bm_core::broker::provider_fn;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;

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

async fn guard_rig() -> (RuntimeHandle, Arc<SeqIdGen>) {
    let connector: Arc<dyn bm_core::ports::ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: vec![
            (manifest("system.echo", "read-only"), provider_fn(Ok)),
            (
                manifest("system.danger.purge", "high-risk-command"),
                provider_fn(|_| Ok(json!({"purged": true}))),
            ),
        ],
        version: "0.1.0-m4".into(),
        data_dir: None,
        store: None,
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(bm_core::clock::SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
        model_streaming: false,
    })
    .await;
    (handle, Arc::new(SeqIdGen::new()))
}

/// G2:跑完整审批流(请求→拒绝),事件流中 approval 相关事件只允许出现
/// 「已发生事实」类型(approval.requested/resolved/expired)——裁决命令
/// (approve/deny)本身不得作为事件出现在事件流;命令走 Wire/Broker。
#[tokio::test]
async fn g2_approval_commands_never_appear_as_events() {
    let (handle, ids) = guard_rig().await;

    // 发起高危调用 → 审批等待 → deny(命令经 Wire,不落事件)
    let req = ids.next_id("req");
    let err = handle
        .capability_call(
            req,
            bm_contract::wire::CapabilityCallParams {
                capability: "system.danger.purge".into(),
                args: json!({}),
                idempotency_key: None,
                deadline_ms: None,
            },
        )
        .await
        .expect_err("高危应升级审批");
    assert!(matches!(err, CoreError::ApprovalNeeded { .. }));
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .unwrap();
    let aid = list["approvals"][0]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    let req = ids.next_id("req");
    let respond = handle
        .approval_respond(
            req,
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(&aid).unwrap(),
                decision: "deny".into(),
                scope: None,
            },
        )
        .await
        .expect("裁决成功");
    assert_eq!(respond["state"], json!("denied"));

    // 守护断言:事件流中 approval 事件 ⊆ 事实类型集合;deny 命令不入流
    let events = handle.events_all().await;
    let approval_event_types: Vec<&str> = events
        .iter()
        .filter_map(|e| {
            let t = e.event_type.as_str();
            t.starts_with("approval.").then_some(t)
        })
        .collect();
    assert!(
        approval_event_types.iter().all(|t| matches!(
            *t,
            "approval.requested" | "approval.resolved" | "approval.expired"
        )),
        "G2: 事件流中只允许裁决事实,实际 {approval_event_types:?}"
    );
    assert!(
        approval_event_types.contains(&"approval.resolved"),
        "deny 的结果(resolved 事实)必须在场"
    );
    // resolved 事实的 outcome=denied(裁决结果入流,裁决动作本身不入流)
    let resolved = events
        .iter()
        .find(|e| e.event_type.as_str() == "approval.resolved")
        .unwrap();
    assert_eq!(resolved.payload["outcome"], json!("denied"));
}

/// G3:清空可丢失运行时缓存后,Broker 授权行为与缓存命中时完全一致
/// (缓存可丢失性;守护「混层缓存」侵蚀——决策不得依赖不可重建状态)。
#[test]
fn g3_authorization_identical_after_cache_loss() {
    use bm_core::broker::{Broker, CallContext, GrantLedger};
    use bm_core::clock::MockClock;
    use bm_core::registry::CapabilityRegistry;

    let build = || {
        let mut reg = CapabilityRegistry::new();
        for (name, effect) in [
            ("system.ro", "read-only"),
            ("system.low", "low-risk-command"),
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
    };

    let mut reg = build();
    let mut grants = GrantLedger::new();
    let clock = MockClock::at_ms(1_788_000_000_000);
    let ids = SeqIdGen::new();
    let ctx = CallContext::surface("surface:user");

    // 缓存命中时的决策快照
    let mut decisions_cached = Vec::new();
    {
        let broker = Broker::new(&reg, &mut grants, &clock, &ids);
        for cap in ["system.ro", "system.low"] {
            decisions_cached.push(broker.decide(&ctx, cap, &json!({})));
        }
    }

    // 清空可丢失运行时缓存(句柄/健康位)——逻辑目录不受影响
    reg.clear_runtime_cache();

    // 缓存重建后决策必须逐字节一致(G3)
    reg.attach_handle("system.ro", provider_fn(Ok)).unwrap();
    reg.attach_handle("system.low", provider_fn(Ok)).unwrap();
    let broker = Broker::new(&reg, &mut grants, &clock, &ids);
    for (cap, expected) in [("system.ro", 0), ("system.low", 1)] {
        let d = broker.decide(&ctx, cap, &json!({}));
        assert_eq!(
            d, decisions_cached[expected],
            "G3: {cap} 缓存清空后决策漂移"
        );
    }
}
