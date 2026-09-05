//! 第四轮评审 P0 验收:t161 取消×审批竞态——取消等待审批的操作后批准,
//! 不得触发表外迁移 panic(单写者死亡),且运行时保持可用。

use bm_contract::events::EventType;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::{
    ApprovalListParams, ApprovalRespondParams, CapabilityCallParams, CapabilityCancelParams,
};
use bm_core::CoreError;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;

async fn rig() -> (RuntimeHandle, Arc<SeqIdGen>) {
    let ids = Arc::new(SeqIdGen::new());
    let manifest: bm_contract::capability::CapabilityManifest = serde_json::from_value(json!({
        "capability": "system.high", "provider": "system.high", "version": "0.1.0",
        "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": "high-risk-command", "idempotent": false, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    }))
    .unwrap();
    let config = RuntimeConfig {
        capabilities: vec![(
            manifest,
            bm_core::broker::provider_fn(|_| Ok(json!({"done": true}))),
        )],
        version: "0.1.0-m9".into(),
        data_dir: None,
        store: None,
        connector: Arc::new(MockConnector::new(vec![])),
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: ids.clone(),
        clock: Arc::new(bm_core::clock::MockClock::at_ms(1_788_000_000_000)),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
        model_streaming: false,
    };
    (RuntimeHandle::start(config).await, ids)
}

/// t161:高危调用 → 审批挂起 → 取消该操作 → 用户随后批准 →
/// 必须得到业务错误(而非核心循环 panic),运行时保持可用。
#[tokio::test]
async fn t161_cancel_then_approve_no_panic_runtime_alive() {
    let (handle, ids) = rig().await;

    // ① 发起高危调用 → 挂起等审批
    let err = handle
        .capability_call(
            ids.next_id("req"),
            CapabilityCallParams {
                capability: "system.high".into(),
                args: json!({}),
                idempotency_key: None,
                deadline_ms: Some(1000),
            },
        )
        .await
        .expect_err("高危调用应挂起等审批");
    assert!(matches!(err, CoreError::ApprovalNeeded { .. }));

    // 找到审批 id
    let list = handle
        .approval_list(ApprovalListParams {
            state_filter: Some("waiting_user".into()),
        })
        .await
        .expect("审批列表");
    let approval_id = list["approvals"][0]["approval_id"]
        .as_str()
        .expect("审批 ID")
        .to_string();
    // op id 从 approval.requested 事实事件取(审批对象本身不携带)
    let events = handle.events_all().await;
    let op_id = events
        .iter()
        .find(|e| e.event_type == EventType::ApprovalRequested)
        .and_then(|e| e.payload["operation_id"].as_str())
        .expect("requested 事件含 op id")
        .to_string();

    // ② 用户取消该操作(等待审批态可取消)
    handle
        .capability_cancel(
            ids.next_id("req"),
            CapabilityCancelParams {
                operation_id: bm_contract::ids::BmId::parse(&op_id).expect("解析"),
                reason: Some("不想做了".into()),
            },
        )
        .await
        .expect("取消受理");

    // ③ 用户随后批准 → 必须是业务错误,绝不能 panic(否则整个单写者死亡)
    let respond = handle
        .approval_respond(
            ids.next_id("req"),
            ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(&approval_id).expect("解析"),
                decision: "approve".into(),
                scope: Some("once".into()),
            },
        )
        .await;
    assert!(respond.is_err(), "已取消的操作不得重放执行:{respond:?}");

    // ④ 运行时存活:后续任意调用正常工作
    let alive = handle
        .capability_call(
            ids.next_id("req"),
            CapabilityCallParams {
                capability: "system.high".into(),
                args: json!({}),
                idempotency_key: Some("k2".into()),
                deadline_ms: Some(1000),
            },
        )
        .await;
    assert!(
        matches!(alive, Err(CoreError::ApprovalNeeded { .. })),
        "运行时必须仍然可用(新调用照常走审批):{alive:?}"
    );
}
