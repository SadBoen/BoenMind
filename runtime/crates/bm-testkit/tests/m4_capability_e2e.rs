//! M4 端到端:capability.call 统一入口、审批暂停-续行、默认拒绝路径。
//! 承载基线 M4 通过条件的进程内面:所有能力调用经 Broker、高危审批、
//! 审批中断后可续行、审计事件归因链(capability.invoked/denied)。

use bm_contract::capability::CapabilityManifest;
use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::GetOperationParams;
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

async fn m4_rig() -> (RuntimeHandle, Arc<SeqIdGen>) {
    let connector = Arc::new(MockConnector::new(vec![]));
    let secrets = Arc::new(MemSecretStore::with("secret:model.x", "sk-demo"));
    let ids = Arc::new(SeqIdGen::new());
    let config = RuntimeConfig {
        capabilities: vec![
            (manifest("system.echo", "read-only"), provider_fn(Ok)),
            (
                manifest("system.notes.write", "reversible-command"),
                provider_fn(|_| Ok(json!({"written": true}))),
            ),
            (
                manifest("system.danger.purge", "high-risk-command"),
                provider_fn(|_| Ok(json!({"purged": true}))),
            ),
        ],
        version: "0.1.0-m4".into(),
        data_dir: None,
        store: None,
        connector,
        secret_store: secrets,
        id_gen: ids.clone(),
        clock: Arc::new(bm_core::clock::MockClock::at_ms(1_788_000_000_000)),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    };
    (RuntimeHandle::start(config).await, ids)
}

fn call_params(
    capability: &str,
    args: serde_json::Value,
) -> bm_contract::wire::CapabilityCallParams {
    bm_contract::wire::CapabilityCallParams {
        capability: capability.into(),
        args,
        idempotency_key: None,
        deadline_ms: Some(1000),
    }
}

#[tokio::test]
async fn t40_direct_call_and_unknown_capability_denied() {
    let (handle, ids) = m4_rig().await;

    // 直通:trusted × read-only × not-required → 执行成功
    let req = ids.next_id("req");
    let out = handle
        .capability_call(req, call_params("system.echo", json!({"msg": "ping"})))
        .await
        .expect("read-only 直通应成功");
    assert_eq!(out["state"], json!("succeeded"));
    assert_eq!(out["result"], json!({"msg": "ping"}));
    assert_eq!(out["principal"], json!("surface:user"));

    // 收据状态机:operations.get 与收据一致(INV-9)
    let op_id = bm_contract::ids::BmId::parse(out["operation_id"].as_str().unwrap()).unwrap();
    let receipt = handle
        .operations_get(GetOperationParams {
            operation_id: op_id.clone(),
        })
        .await
        .expect("收据可查");
    assert_eq!(receipt.state, OperationState::Succeeded);

    // 审计:capability.invoked(outcome=ok)含归因链字段
    let events = handle.events_all().await;
    let invoked = events
        .iter()
        .find(|e| e.event_type == EventType::CapabilityInvoked)
        .expect("应有 capability.invoked");
    assert_eq!(invoked.payload["outcome"], json!("ok"));
    assert_eq!(invoked.payload["principal"], json!("surface:user"));
    assert_eq!(invoked.payload["binding_epoch"], json!(1));
    assert_eq!(
        invoked.payload["provider_instance_id"],
        json!("system.echo@0.1.0")
    );

    // 越权:未注册能力 → permission_denied(审批不能补授权,ADR-0006)
    let req = ids.next_id("req");
    let err = handle
        .capability_call(req, call_params("system.ghost", json!({})))
        .await
        .expect_err("未知能力必须被拒");
    match &err {
        CoreError::Semantic(code, _) => {
            assert_eq!(*code, ErrorCode::PermissionDenied);
        }
        other => panic!("应为 permission_denied,实际 {other:?}"),
    }
    let events = handle.events_all().await;
    let denied = events
        .iter()
        .find(|e| e.event_type == EventType::CapabilityDenied)
        .expect("应有 capability.denied");
    assert_eq!(denied.payload["reason_code"], json!("unknown_capability"));
}

#[tokio::test]
async fn t41_high_risk_approval_deny_cycle() {
    let (handle, ids) = m4_rig().await;

    // 高风险恒审批(manifest 即使 not-required,双保险兜住)
    let req = ids.next_id("req");
    let err = handle
        .capability_call(req, call_params("system.danger.purge", json!({"t": 1})))
        .await
        .expect_err("高风险必须审批");
    match &err {
        CoreError::Semantic(code, _) => assert_eq!(*code, ErrorCode::ApprovalRequired),
        other => panic!("应为 approval_required,实际 {other:?}"),
    }

    // 审批对象可见,operation 停在 waiting_approval
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .expect("列表可查");
    let approvals = list["approvals"].as_array().unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0]["state"], json!("waiting_user"));
    assert_eq!(approvals[0]["risk_class"], json!("high-risk-command"));
    let approval_id = approvals[0]["approval_id"].as_str().unwrap();
    let op_id = approvals
        .iter()
        .find_map(|a| a.get("operation_id").and_then(|v| v.as_str()))
        .and_then(BmIdErr::ok);
    let _ = op_id; // operation_id 不在 approval 合同载荷中,经事件关联(见下)

    let events = handle.events_all().await;
    let requested = events
        .iter()
        .find(|e| e.event_type == EventType::ApprovalRequested)
        .expect("应有 approval.requested");
    assert_eq!(requested.payload["risk_class"], json!("high-risk-command"));
    let op_id =
        bm_contract::ids::BmId::parse(requested.payload["operation_id"].as_str().unwrap()).unwrap();
    let receipt = handle
        .operations_get(GetOperationParams {
            operation_id: op_id.clone(),
        })
        .await
        .expect("收据可查");
    assert_eq!(receipt.state, OperationState::WaitingApproval);

    // 拒绝 → operation cancelled(approval_denied_or_expired_or_withdrawn)
    let req = ids.next_id("req");
    let respond = handle
        .approval_respond(
            req,
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(approval_id).unwrap(),
                decision: "deny".into(),
                scope: None,
            },
        )
        .await
        .expect("裁决应成功");
    assert_eq!(respond["state"], json!("denied"));
    let receipt = handle
        .operations_get(GetOperationParams {
            operation_id: op_id,
        })
        .await
        .expect("收据可查");
    assert_eq!(receipt.state, OperationState::Cancelled);

    // 已终态的审批不得再裁决
    let req = ids.next_id("req");
    let err = handle
        .approval_respond(
            req,
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(approval_id).unwrap(),
                decision: "deny".into(),
                scope: None,
            },
        )
        .await
        .expect_err("已裁决审批不可重复裁决");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
}

// approval_id 字符串解析的轻量辅助(测试可读性)
struct BmIdErr;
impl BmIdErr {
    fn ok(s: &str) -> Option<bm_contract::ids::BmId> {
        bm_contract::ids::BmId::parse(s).ok()
    }
}

#[tokio::test]
async fn t42_approve_materializes_grant_and_completes() {
    let (handle, ids) = m4_rig().await;

    // reversible → 审批(trusted 直调 reversible+ 亦审批,规格 §5.4)
    let req = ids.next_id("req");
    let err = handle
        .capability_call(
            req,
            call_params("system.notes.write", json!({"path": "a.md"})),
        )
        .await
        .expect_err("reversible 应升级审批");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));

    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .expect("列表可查");
    let approval_id = list["approvals"][0]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 批准(count:5)→ Grant 物化 + operation 续行执行成功
    let req = ids.next_id("req");
    let respond = handle
        .approval_respond(
            req,
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(&approval_id).unwrap(),
                decision: "approve".into(),
                scope: Some("count:5".into()),
            },
        )
        .await
        .expect("批准应成功");
    assert_eq!(respond["state"], json!("approved"));
    assert!(respond["grant_id"].is_string());
    let grant_used = respond["grant_id"].as_str().unwrap().to_string();

    let events = handle.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::GrantCreated)
    );
    let resolved = events
        .iter()
        .find(|e| e.event_type == EventType::ApprovalResolved)
        .expect("应有 approval.resolved");
    assert_eq!(resolved.payload["outcome"], json!("approved"));
    let invoked: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::CapabilityInvoked)
        .collect();
    assert!(invoked.iter().any(|e| e.payload["outcome"] == json!("ok")));

    // 第二次调用:Grant(count:5)未耗尽 → 免审批直接执行(Grant 命中
    // 优先于审批判定——审批的产物就是 Grant,规格 §5.4)
    let req = ids.next_id("req");
    let out = handle
        .capability_call(
            req,
            call_params("system.notes.write", json!({"path": "b.md"})),
        )
        .await
        .expect("Grant 命中应免审批直接执行");
    assert_eq!(out["grant_used"], json!(grant_used));

    // scope 不在 choices → validation_failed
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .expect("列表可查");
    let new_approval = list["approvals"][0]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    let req = ids.next_id("req");
    let err = handle
        .approval_respond(
            req,
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(&new_approval).unwrap(),
                decision: "approve".into(),
                scope: Some("forever".into()),
            },
        )
        .await
        .expect_err("forever 不在 scope_choices,必须被拒");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
}

#[tokio::test]
async fn t43_approval_survives_restart() {
    let dir = tempfile::tempdir().expect("临时目录");
    // 第一次启动:发起可审批调用后「崩溃」(句柄即进程)
    std::fs::create_dir_all(dir.path().join("data")).expect("建数据目录");
    let store =
        Arc::new(bm_persist::PersistStore::open(&dir.path().join("data")).expect("打开持久层"));
    let handle1 = m4_rig_at(dir.path().join("data"), store.clone()).await;
    let req = bm_contract::ids::BmId::parse("req_01JAAAAAAAAAAAAAAAAAAAAA90").unwrap();
    let err = handle1
        .capability_call(
            req,
            call_params("system.notes.write", json!({"path": "a.md"})),
        )
        .await
        .expect_err("reversible 应升级审批");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));
    handle1.stop("crash-sim").await;

    // 第二次启动(同目录):审批对象恢复,waiting_user 仍可裁决
    let handle2 = m4_rig_at(dir.path().join("data"), store).await;
    let list = handle2
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .expect("恢复后列表可查");
    let approvals = list["approvals"].as_array().unwrap();
    assert_eq!(approvals.len(), 1, "审批对象必须跨重启恢复");
    assert_eq!(approvals[0]["state"], json!("waiting_user"));
    let approval_id = approvals[0]["approval_id"].as_str().unwrap().to_string();

    // 批准 → 重放执行载荷恢复 → 执行成功(审批中断后可以恢复,基线 M4)
    let req = bm_contract::ids::BmId::parse("req_01JAAAAAAAAAAAAAAAAAAAAA91").unwrap();
    let respond = handle2
        .approval_respond(
            req,
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(&approval_id).unwrap(),
                decision: "approve".into(),
                scope: Some("once".into()),
            },
        )
        .await
        .expect("恢复后批准应成功");
    assert_eq!(respond["state"], json!("approved"));
    assert!(respond["grant_id"].is_string());

    let events = handle2.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::GrantCreated)
    );
    assert!(events.iter().any(
        |e| e.event_type == EventType::CapabilityInvoked && e.payload["outcome"] == json!("ok")
    ));
}

/// 带持久层的 M4 装配(恢复测试用;clock 基准与 m4_rig 一致)。
async fn m4_rig_at(
    data_dir: std::path::PathBuf,
    store: Arc<dyn bm_persist::EventStore>,
) -> RuntimeHandle {
    let connector = Arc::new(MockConnector::new(vec![]));
    let secrets = Arc::new(MemSecretStore::with("secret:model.x", "sk-demo"));
    let ids = Arc::new(SeqIdGen::new());
    let config = RuntimeConfig {
        capabilities: vec![(
            manifest("system.notes.write", "reversible-command"),
            provider_fn(|_| Ok(json!({"written": true}))),
        )],
        version: "0.1.0-m4".into(),
        data_dir: Some(data_dir),
        store: Some(store),
        connector,
        secret_store: secrets,
        id_gen: ids,
        clock: Arc::new(bm_core::clock::MockClock::at_ms(1_788_000_000_000)),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    };
    RuntimeHandle::start(config).await
}
