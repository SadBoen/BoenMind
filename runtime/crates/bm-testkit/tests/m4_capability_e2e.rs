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
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
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

async fn m4_rig() -> (RuntimeHandle, Arc<SeqIdGen>, Arc<bm_core::clock::MockClock>) {
    let connector = Arc::new(MockConnector::new(vec![]));
    let secrets = Arc::new(MemSecretStore::with("secret:model.x", "sk-demo"));
    let ids = Arc::new(SeqIdGen::new());
    let clock = Arc::new(bm_core::clock::MockClock::at_ms(1_788_000_000_000));
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
        clock: clock.clone(),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
        model_streaming: false,
    };
    (RuntimeHandle::start(config).await, ids, clock)
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
    let (handle, ids, _clock) = m4_rig().await;

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
    let (handle, ids, _clock) = m4_rig().await;

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

// A-11(审计台账 2026-08-31)验收:到期审批由 approval.list 前置扫描收敛。
// expire_if_due 生产接线——无人裁决的滞留项不进 waiting_user 队列,
// 关联 operation 连带取消,approval.expired 事件在案,过期后不可再裁决。
#[tokio::test]
async fn t41b_expired_approval_swept_from_waiting_list() {
    let (handle, ids, clock) = m4_rig().await;

    // 高风险调用 → waiting_user 审批(TTL 300_000ms,审批窗口)
    let req = ids.next_id("req");
    let _ = handle
        .capability_call(req, call_params("system.danger.purge", json!({"t": 1})))
        .await
        .expect_err("高风险必须审批");
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .expect("列表可查");
    let approval_id = list["approvals"][0]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    let events = handle.events_all().await;
    let requested = events
        .iter()
        .find(|e| e.event_type == EventType::ApprovalRequested)
        .expect("应有 approval.requested");
    let op_id =
        bm_contract::ids::BmId::parse(requested.payload["operation_id"].as_str().unwrap()).unwrap();

    // 时钟拨过审批窗口 → 下一次 approval.list 前置扫描应收敛
    clock.advance_ms(300_001);

    let swept = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .expect("列表可查");
    assert_eq!(
        swept["approvals"].as_array().unwrap().len(),
        0,
        "过期审批不得留在待裁决队列"
    );

    // 显式过滤可见:state=expired,对象字段保持完整
    let expired = handle
        .approval_list(bm_contract::wire::ApprovalListParams {
            state_filter: Some("expired".into()),
        })
        .await
        .expect("列表可查");
    let expired_rows = expired["approvals"].as_array().unwrap();
    assert_eq!(expired_rows.len(), 1);
    assert_eq!(expired_rows[0]["approval_id"], json!(approval_id));
    assert_eq!(expired_rows[0]["state"], json!("expired"));

    // 关联 operation 连带取消(基线 §9.6 同 respond 过期分支)
    let receipt = handle
        .operations_get(GetOperationParams {
            operation_id: op_id,
        })
        .await
        .expect("收据可查");
    assert_eq!(receipt.state, OperationState::Cancelled);

    // approval.expired 事件在案
    let events = handle.events_all().await;
    let expired_event = events
        .iter()
        .find(|e| e.event_type == EventType::ApprovalExpired)
        .expect("应有 approval.expired");
    assert_eq!(expired_event.payload["approval_id"], json!(approval_id));

    // 已过期审批不可再裁决(扫后状态已终态,respond 报 AlreadyResolved)
    let req = ids.next_id("req");
    let err = handle
        .approval_respond(
            req,
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(approval_id).unwrap(),
                decision: "approve".into(),
                scope: None,
            },
        )
        .await
        .expect_err("过期审批不可再裁决");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
}

#[tokio::test]
async fn t42_approve_materializes_grant_and_completes() {
    let (handle, ids, _clock) = m4_rig().await;

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

    // scope 不在 choices → validation_failed:构造新审批(高危恒审批)
    let req = ids.next_id("req");
    let err = handle
        .capability_call(req, call_params("system.danger.purge", json!({})))
        .await
        .expect_err("高危应升级审批");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));
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
        async_executor: None,
        model_streaming: false,
    };
    RuntimeHandle::start(config).await
}

/// M4-T6:幂等抑制与副作用前门禁(硬约束 5/11;ADR-0002 条件 6)。
/// 同 idempotency_key 的等价 external-side-effect 请求:Provider 恰执行一次,
/// 第二次返回原收据且落 outcome=suppressed 审计(可从审计日志单独证明);
/// intent 事件先于 ok(副作用前门禁,ADR-0001 条件 5)。
#[tokio::test]
async fn t44_idempotency_suppression_and_intent_gate() {
    use std::sync::atomic::{AtomicU64, Ordering};

    let send_count = Arc::new(AtomicU64::new(0));
    let sc = send_count.clone();
    let mail = bm_core::broker::provider_fn(move |_| {
        sc.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({"message_id": "mock-000001", "queued": true}))
    });
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: vec![(
            serde_json::from_value::<bm_contract::capability::CapabilityManifest>(
                serde_json::json!({
                    "capability": "system.mail.mock_send", "provider": "system.mail",
                    "version": "0.1.0", "input_schema": {"type": "object"},
                    "output_schema": {"type": "object"},
                    "effect": "external-side-effect", "idempotent": true,
                    "cancellable": true, "timeout_ms": 2000, "approval": "not-required"
                }),
            )
            .unwrap(),
            mail,
        )],
        version: "0.1.0-m4".into(),
        data_dir: None,
        store: None,
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
        model_streaming: false,
    })
    .await;

    let call_once = |handle: &RuntimeHandle, req: bm_contract::ids::BmId, key: &str| {
        let handle = handle.clone();
        let key = key.to_string();
        async move {
            handle
                .capability_call(
                    req,
                    bm_contract::wire::CapabilityCallParams {
                        capability: "system.mail.mock_send".into(),
                        args: serde_json::json!({"to": "a@x"}),
                        idempotency_key: Some(key),
                        deadline_ms: Some(1000),
                    },
                )
                .await
        }
    };

    // 第一次:升级审批 → 批准 once → 执行成功
    let req = bm_contract::ids::BmId::parse("req_01JAAAAAAAAAAAAAAAAAAAAA90").unwrap();
    let err = call_once(&handle, req, "idem-1")
        .await
        .expect_err("external-side-effect 应升级审批");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .unwrap();
    let aid1 = list["approvals"][0]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    let respond = handle
        .approval_respond(
            bm_contract::ids::BmId::parse("req_01JAAAAAAAAAAAAAAAAAAAAA91").unwrap(),
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(&aid1).unwrap(),
                decision: "approve".into(),
                scope: Some("once".into()),
            },
        )
        .await
        .expect("批准应成功");
    assert_eq!(respond["state"], serde_json::json!("approved"));

    // 第二次:同幂等键同参 → 再审批 → 批准 → 抑制(不执行)
    let req = bm_contract::ids::BmId::parse("req_01JAAAAAAAAAAAAAAAAAAAAA92").unwrap();
    let err = call_once(&handle, req, "idem-1")
        .await
        .expect_err("第二次调用仍需审批(Grant 已耗)");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .unwrap();
    let aid2 = list["approvals"][0]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    let respond = handle
        .approval_respond(
            bm_contract::ids::BmId::parse("req_01JAAAAAAAAAAAAAAAAAAAAA93").unwrap(),
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(&aid2).unwrap(),
                decision: "approve".into(),
                scope: Some("once".into()),
            },
        )
        .await
        .expect("第二次批准应成功");
    assert_eq!(respond["state"], serde_json::json!("approved"));

    // 断言 1:Provider 恰执行一次(重复副作用 = 0,ADR-0002 条件 6)
    assert_eq!(
        send_count.load(Ordering::SeqCst),
        1,
        "Provider 必须恰执行一次"
    );

    // 断言 2:审计可证——事件流含 intent→ok→suppressed 序,
    // suppressed 与 ok 的 idempotency_key_hash 一致(等价请求证明)
    let events = handle.events_all().await;
    let invocations: Vec<String> = events
        .iter()
        .filter(|e| e.event_type == EventType::CapabilityInvoked)
        .map(|e| e.payload["outcome"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        invocations,
        vec!["intent", "ok", "suppressed"],
        "事件序: {invocations:?}"
    );
    let ok_hash = events
        .iter()
        .find(|e| {
            e.event_type == EventType::CapabilityInvoked
                && e.payload["outcome"] == serde_json::json!("ok")
        })
        .map(|e| {
            e.payload["idempotency_key_hash"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .unwrap();
    let sup_hash = events
        .iter()
        .find(|e| {
            e.event_type == EventType::CapabilityInvoked
                && e.payload["outcome"] == serde_json::json!("suppressed")
        })
        .map(|e| {
            e.payload["idempotency_key_hash"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .unwrap();
    assert_eq!(ok_hash, sup_hash, "同一幂等键的抑制可从审计证明");
}

/// M4-T6b:恢复三路(硬约束 5;基线 §13.3)。intent 落盘而结果不在(崩溃
/// 窗口注入)→ 重启后 operation 以 outcome_unknown 重建,等待裁定入口
/// (recovery_settle);禁止自动重放(ADR-0004)。裁定后审计闭环。
#[tokio::test]
async fn t45_outbox_pending_recovers_to_outcome_unknown() {
    let dir = tempfile::tempdir().expect("临时目录");
    let store: Arc<dyn bm_persist::EventStore> =
        Arc::new(bm_persist::PersistStore::open(dir.path()).expect("打开持久层"));
    // 注入崩溃窗口:intent 已落(outbox pending)而结果不在
    store
        .outbox_upsert(
            "op_01JAAAAAAAAAAAAAAAAAAAAA0F",
            "side_effect",
            "pending",
            r#"{"capability":"system.mail.mock_send","key_hash":"aa"}"#,
            "2026-08-29T10:40:00.000Z",
        )
        .expect("注入 pending");

    // 重启
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: vec![bm_providers::builtin::model_invoke_cap()],
        version: "0.1.0-m4".into(),
        data_dir: None,
        store: Some(store),
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
        model_streaming: false,
    })
    .await;

    // 恢复面:outcome_unknown 可查(未执行、未自动重放)
    let receipt = handle
        .operations_get(bm_contract::wire::GetOperationParams {
            operation_id: bm_contract::ids::BmId::parse("op_01JAAAAAAAAAAAAAAAAAAAAA0F").unwrap(),
        })
        .await
        .expect("恢复的 operation 可查");
    assert_eq!(receipt.state, OperationState::OutcomeUnknown);

    // 裁定入口(用户核验外部系统后裁定 succeeded)→ 审计闭环
    let settled = handle
        .recovery_settle(
            bm_contract::ids::BmId::parse("op_01JAAAAAAAAAAAAAAAAAAAAA0F").unwrap(),
            bm_core::runtime::RecoveryVerdict::Succeeded,
        )
        .await
        .expect("裁定成功");
    assert_eq!(settled.state, OperationState::Succeeded);
    let events = handle.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::OperationStateChanged
                && e.payload["to"] == serde_json::json!("succeeded"))
    );
    handle.stop("test_done").await;
}

/// M4-T7 降级 B 态(硬约束 6;规格 §5.7-B):持久写路径故障 → Runtime 进入
/// 拒写降级:后续 capability 调用拒绝(安全侧),规范状态查询照常
/// (operations_get 走内存视图);恢复 = 重启以持久层为准。
struct FailingStore;
impl bm_persist::EventStore for FailingStore {
    fn record(
        &self,
        _e: &bm_contract::events::EventEnvelope,
    ) -> bm_persist::error::StoreResult<()> {
        Err(bm_persist::error::StoreError::Io(std::io::Error::other(
            "injected persist failure",
        )))
    }
    fn recover(&self) -> bm_persist::error::StoreResult<bm_persist::recovery::RecoveryReport> {
        Ok(bm_persist::recovery::RecoveryReport {
            last_applied_seq: 0,
            replayed: 0,
            interrupted_recovered: 0,
        })
    }
    fn pending_operations(&self) -> bm_persist::error::StoreResult<Vec<(String, String, String)>> {
        Ok(vec![])
    }
    fn load_rows(&self) -> bm_persist::error::StoreResult<bm_persist::recovery::WorldRows> {
        Ok(bm_persist::recovery::WorldRows::default())
    }
    fn materialize_event(
        &self,
        _e: &bm_contract::events::EventEnvelope,
    ) -> bm_persist::error::StoreResult<()> {
        Ok(())
    }
    fn save_op_input(&self, _o: &str, _c: &str) -> bm_persist::error::StoreResult<()> {
        Ok(())
    }
    fn op_input(&self, _o: &str) -> bm_persist::error::StoreResult<Option<String>> {
        Ok(None)
    }
    fn save_task(
        &self,
        _row: bm_persist::sqlite_state::TaskRow<'_>,
    ) -> bm_persist::error::StoreResult<()> {
        Err(bm_persist::error::StoreError::Io(std::io::Error::other(
            "injected persist failure",
        )))
    }
    fn list_tasks(&self) -> bm_persist::error::StoreResult<Vec<serde_json::Value>> {
        Ok(vec![])
    }
    fn save_idem_receipt(
        &self,
        _h: &str,
        _p: &str,
        _t: &str,
    ) -> bm_persist::error::StoreResult<()> {
        Err(bm_persist::error::StoreError::Io(std::io::Error::other(
            "injected persist failure",
        )))
    }
    fn list_idem_receipts(&self) -> bm_persist::error::StoreResult<Vec<serde_json::Value>> {
        Ok(vec![])
    }
    fn save_task_budget(
        &self,
        _t: &str,
        _a: &str,
        _u: u64,
        _tok: u64,
        _now: &str,
    ) -> bm_persist::error::StoreResult<()> {
        Err(bm_persist::error::StoreError::Io(std::io::Error::other(
            "injected persist failure",
        )))
    }
    fn list_task_budget(&self) -> bm_persist::error::StoreResult<Vec<serde_json::Value>> {
        Ok(vec![])
    }
    fn save_observation(
        &self,
        _t: &str,
        _v: &str,
        _g: &str,
        _p: &str,
        _now: &str,
    ) -> bm_persist::error::StoreResult<u64> {
        Err(bm_persist::error::StoreError::Io(std::io::Error::other(
            "injected persist failure",
        )))
    }
    fn memory_put(
        &self,
        _i: &str,
        _s: &str,
        _c: &str,
        _cp: Option<&str>,
        _t: &str,
        _sr: Option<&str>,
        _co: Option<&str>,
        _p: &str,
        _now: &str,
    ) -> bm_persist::error::StoreResult<()> {
        Err(bm_persist::error::StoreError::Io(std::io::Error::other(
            "injected persist failure",
        )))
    }
    fn memory_search(
        &self,
        _s: &str,
        _q: &str,
    ) -> bm_persist::error::StoreResult<Vec<serde_json::Value>> {
        Ok(vec![])
    }
    fn memory_delete(&self, _i: &str) -> bm_persist::error::StoreResult<usize> {
        Err(bm_persist::error::StoreError::Io(std::io::Error::other(
            "injected persist failure",
        )))
    }
    fn append(
        &self,
        _e: &bm_contract::events::EventEnvelope,
    ) -> bm_persist::error::StoreResult<()> {
        Err(bm_persist::error::StoreError::Io(std::io::Error::other(
            "injected persist failure",
        )))
    }
    fn replay_since(
        &self,
        _s: u64,
    ) -> bm_persist::error::StoreResult<Vec<bm_contract::events::EventEnvelope>> {
        Ok(vec![])
    }
    fn last_log_seq(&self) -> bm_persist::error::StoreResult<u64> {
        Ok(0)
    }
    fn last_applied_seq(&self) -> bm_persist::error::StoreResult<u64> {
        Ok(0)
    }
    fn mark_applied(&self, _s: u64) -> bm_persist::error::StoreResult<()> {
        Ok(())
    }
    fn snapshot(&self) -> bm_persist::error::StoreResult<u64> {
        Ok(0)
    }
    fn compact(&self, _u: u64) -> bm_persist::error::StoreResult<usize> {
        Ok(0)
    }
    fn save_approval(
        &self,
        _r: bm_persist::sqlite_state::ApprovalRow<'_>,
    ) -> bm_persist::error::StoreResult<()> {
        Ok(())
    }
    fn list_approvals(&self) -> bm_persist::error::StoreResult<Vec<serde_json::Value>> {
        Ok(vec![])
    }
    fn save_grant(
        &self,
        _r: bm_persist::sqlite_state::GrantRow<'_>,
    ) -> bm_persist::error::StoreResult<()> {
        Ok(())
    }
    fn list_grants(&self) -> bm_persist::error::StoreResult<Vec<serde_json::Value>> {
        Ok(vec![])
    }
    fn save_capability_binding(
        &self,
        _r: bm_persist::sqlite_state::CapabilityRow<'_>,
    ) -> bm_persist::error::StoreResult<()> {
        Ok(())
    }
    fn delete_capability_binding(&self, _c: &str) -> bm_persist::error::StoreResult<()> {
        Ok(())
    }
    fn list_capability_bindings(&self) -> bm_persist::error::StoreResult<Vec<serde_json::Value>> {
        Ok(vec![])
    }
    fn outbox_upsert(
        &self,
        _o: &str,
        _k: &str,
        _s: &str,
        _p: &str,
        _n: &str,
    ) -> bm_persist::error::StoreResult<()> {
        Ok(())
    }
    fn list_outbox_by_state(
        &self,
        _s: &str,
    ) -> bm_persist::error::StoreResult<Vec<serde_json::Value>> {
        Ok(vec![])
    }

    fn save_evaluation_report(
        &self,
        _report_id: &str,
        _from_seq: u64,
        _to_seq: u64,
        _payload: &str,
        _created_at: &str,
    ) -> bm_persist::error::StoreResult<()> {
        Err(bm_persist::error::StoreError::Io(std::io::Error::other(
            "injected persist failure",
        )))
    }

    fn list_evaluation_reports(&self) -> bm_persist::error::StoreResult<Vec<serde_json::Value>> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn t46_persist_failure_degrades_safely() {
    use bm_contract::capability::CapabilityManifest;
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: vec![(
            serde_json::from_value::<CapabilityManifest>(serde_json::json!({
                "capability": "system.echo", "provider": "system.echo",
                "version": "0.1.0", "input_schema": {"type": "object"},
                "output_schema": {"type": "object"}, "effect": "read-only",
                "idempotent": true, "cancellable": true,
                "timeout_ms": 1000, "approval": "not-required"
            }))
            .unwrap(),
            bm_core::broker::provider_fn(Ok),
        )],
        version: "0.1.0-m4".into(),
        data_dir: None,
        store: Some(Arc::new(FailingStore)),
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
        model_streaming: false,
    })
    .await;

    // 第一次调用:事件写穿失败 → Runtime 进入降级(拒写)态
    let req = bm_contract::ids::BmId::parse("req_01JAAAAAAAAAAAAAAAAAAAAA94").unwrap();
    let _ = handle
        .capability_call(
            req,
            bm_contract::wire::CapabilityCallParams {
                capability: "system.echo".into(),
                args: serde_json::json!({}),
                idempotency_key: None,
                deadline_ms: None,
            },
        )
        .await;

    // 降级生效:后续 capability 调用拒绝(安全侧;bus.degraded 已尽力入分发)
    let req = bm_contract::ids::BmId::parse("req_01JAAAAAAAAAAAAAAAAAAAAA95").unwrap();
    let err = handle
        .capability_call(
            req,
            bm_contract::wire::CapabilityCallParams {
                capability: "system.echo".into(),
                args: serde_json::json!({}),
                idempotency_key: None,
                deadline_ms: None,
            },
        )
        .await
        .expect_err("降级态必须拒绝后续调用");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::Unavailable, _)
    ));

    // 规范状态查询面照常可答(INV-9 精神:降级不影响既有收据可查)
    let _ = handle
        .operations_get(bm_contract::wire::GetOperationParams {
            operation_id: bm_contract::ids::BmId::parse("op_01JAAAAAAAAAAAAAAAAAAAAA90").unwrap(),
        })
        .await;
    handle.stop("test_done").await;
}
