//! M5-T1 端到端:Task 生命周期(创建/暂停/恢复/停止)、L2 规范状态持久化、
//! 跨重启恢复、T6c 收紧(count 类 Grant 消费余量与幂等收据落表)。
//! 承载基线 M5 通过条件的进程内面:Task 状态不依赖编排器内存(ADR-0004)。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::{TaskCreateParams, TaskLifecycleParams};
use bm_core::CoreError;
use bm_core::clock::SystemClock;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;

fn lifecycle_params(task_id: &bm_contract::ids::BmId) -> TaskLifecycleParams {
    TaskLifecycleParams {
        task_id: task_id.clone(),
        reason: Some("测试".into()),
        note: None,
    }
}

async fn task_rig(dir: Option<&std::path::Path>) -> (RuntimeHandle, Arc<SeqIdGen>) {
    let connector = Arc::new(MockConnector::new(vec![]));
    let ids = Arc::new(SeqIdGen::new());
    let store: Option<Arc<dyn bm_persist::EventStore>> = dir.map(|d| {
        let s: Arc<dyn bm_persist::EventStore> =
            Arc::new(bm_persist::PersistStore::open(d).expect("打开持久层"));
        s
    });
    let config = RuntimeConfig {
        capabilities: Vec::new(),
        version: "0.1.0-m5".into(),
        data_dir: dir.map(|d| d.to_path_buf()),
        store,
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk-demo")),
        id_gen: ids.clone(),
        clock: Arc::new(bm_core::clock::MockClock::at_ms(1_788_000_000_000)),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    };
    (RuntimeHandle::start(config).await, ids)
}

/// t50:生命周期主链——事件序、状态机边、行同步、表外拒绝。
#[tokio::test]
async fn t50_task_lifecycle_events_and_guards() {
    let dir = tempfile::tempdir().expect("临时目录");
    let (handle, ids) = task_rig(Some(dir.path())).await;

    // 创建:即启动(created→running)
    let created = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "整理读书笔记".into(),
                goal: "把 inbox 笔记归档到 notes 并复核".into(),
                authorization: None,
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("创建成功");
    assert_eq!(
        created.state,
        bm_contract::states::TaskState::Running,
        "created→running 即启动(GT-03 A1)"
    );

    // 暂停 / 恢复 / 停止
    let paused = handle
        .task_pause(ids.next_id("req"), lifecycle_params(&created.task_id))
        .await
        .expect("暂停成功");
    assert_eq!(paused.state, bm_contract::states::TaskState::Paused);
    let resumed = handle
        .task_resume(ids.next_id("req"), lifecycle_params(&created.task_id))
        .await
        .expect("恢复成功");
    assert_eq!(resumed.state, bm_contract::states::TaskState::Running);
    let stopped = handle
        .task_stop(ids.next_id("req"), lifecycle_params(&created.task_id))
        .await
        .expect("停止成功");
    assert_eq!(stopped.state, bm_contract::states::TaskState::Cancelled);

    // 事件序:task.created + 4 次 task.state.changed(from/to/reason/epoch)
    let events = handle.events_all().await;
    let task_events: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e.event_type,
                EventType::TaskCreated | EventType::TaskStateChanged
            )
        })
        .collect();
    assert_eq!(task_events.len(), 5, "task.created + 4 次状态迁移");
    assert_eq!(task_events[0].event_type, EventType::TaskCreated);
    assert_eq!(
        task_events[0].payload["task_id"],
        json!(created.task_id.as_str())
    );
    let expect_edges = [
        ("created", "running", "task_started"),
        ("running", "paused", "task_paused"),
        ("paused", "running", "task_resumed"),
        ("running", "cancelled", "task_cancelled"),
    ];
    for (i, (from, to, reason)) in expect_edges.iter().enumerate() {
        let e = task_events[i + 1];
        assert_eq!(e.event_type, EventType::TaskStateChanged);
        assert_eq!(e.payload["from"], json!(*from), "边 {i} from");
        assert_eq!(e.payload["to"], json!(*to), "边 {i} to");
        assert_eq!(e.payload["reason_code"], json!(*reason), "边 {i} guard");
        assert_eq!(e.payload["task_epoch"], json!(1), "epoch 随事件在场");
    }

    // 表外拒绝:终态后再暂停 → validation_failed(状态不变)
    let err = handle
        .task_pause(ids.next_id("req"), lifecycle_params(&created.task_id))
        .await
        .expect_err("终态不可迁出");
    match &err {
        CoreError::Semantic(code, msg) => {
            assert_eq!(*code, ErrorCode::ValidationFailed);
            assert!(msg.contains("表外迁移"), "{msg}");
        }
        _ => panic!("应为语义错误"),
    }
    // 不存在的 Task → validation_failed
    let ghost = TaskLifecycleParams {
        task_id: bm_contract::ids::BmId::parse("task_01JAAAAAAAAAAAAAAAAAAAAAGH").unwrap(),
        reason: None,
        note: None,
    };
    assert!(handle.task_pause(ids.next_id("req"), ghost).await.is_err());

    // L2 行同步:tasks 表 state=cancelled、epoch=1、载荷为合同形态
    let store = bm_persist::PersistStore::open(dir.path()).expect("重开持久层");
    let rows = store.state().list_tasks().expect("读 tasks");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["state"], json!("cancelled"));
    assert_eq!(rows[0]["task_epoch"], json!(1));
    let payload: serde_json::Value =
        serde_json::from_str(rows[0]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["title"], json!("整理读书笔记"));
    assert!(payload["budget"].is_object(), "budget 缺省落显式包络");
    handle.stop("test_done").await;
}

/// t51:Task 规范状态跨重启恢复(L2 持有,不依赖编排器内存;ADR-0004)。
#[tokio::test]
async fn t51_task_state_survives_restart() {
    let dir = tempfile::tempdir().expect("临时目录");
    let (handle, ids) = task_rig(Some(dir.path())).await;
    let created = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "周报汇总".into(),
                goal: "汇总本周笔记".into(),
                authorization: None,
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("创建成功");
    handle
        .task_pause(ids.next_id("req"), lifecycle_params(&created.task_id))
        .await
        .expect("暂停成功");
    handle.stop("restart").await;

    // 重启:同一目录恢复(事件日志 + tasks 表)
    let (handle2, ids2) = task_rig(Some(dir.path())).await;
    // 恢复后的 Task 可继续生命周期命令(World.tasks 已自持久层装载)
    let resumed = handle2
        .task_resume(ids2.next_id("req"), lifecycle_params(&created.task_id))
        .await
        .expect("恢复后 Task 可 resume(状态不依赖编排器内存)");
    assert_eq!(resumed.state, bm_contract::states::TaskState::Running);

    // epoch 不回退:重启后事件里的 task_epoch 仍为 1(持久计数,单调)
    let events = handle2.events_all().await;
    let resumed_event = events
        .iter()
        .rev()
        .find(|e| e.event_type == EventType::TaskStateChanged)
        .expect("重启后有状态事件");
    assert_eq!(resumed_event.payload["from"], json!("paused"));
    assert_eq!(
        resumed_event.payload["task_epoch"],
        json!(1),
        "epoch 恢复不回退"
    );

    // 行同步:重启侧的持久行推进到 running
    let store = bm_persist::PersistStore::open(dir.path()).expect("重开持久层");
    let rows = store.state().list_tasks().expect("读 tasks");
    assert_eq!(rows[0]["state"], json!("running"));
    handle2.stop("test_done").await;
}

/// t52(T6c 收紧 1):count 类 Grant 消费余量跨重启不回满——重启前耗尽的
/// 授权,重启后不再免审批放行(ADR-0002 条件 6 的持久面)。
#[tokio::test]
async fn t52_count_grant_exhaustion_survives_restart() {
    let dir = tempfile::tempdir().expect("临时目录");
    let manifest: bm_contract::capability::CapabilityManifest = serde_json::from_value(json!({
        "capability": "system.danger.purge", "provider": "system.danger",
        "version": "0.1.0", "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": "high-risk-command", "idempotent": true, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    }))
    .unwrap();
    let provider = bm_core::broker::provider_fn(|_| Ok(json!({"purged": true})));

    let connector: Arc<dyn bm_core::ports::ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let ids = Arc::new(SeqIdGen::new());
    let store: Arc<dyn bm_persist::EventStore> =
        Arc::new(bm_persist::PersistStore::open(dir.path()).expect("打开持久层"));
    let config = RuntimeConfig {
        capabilities: vec![(manifest.clone(), provider.clone())],
        version: "0.1.0-m5".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store),
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: ids.clone(),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    };
    let handle = RuntimeHandle::start(config).await;
    let call = |h: &RuntimeHandle, r: bm_contract::ids::BmId| {
        let h = h.clone();
        async move {
            h.capability_call(
                r,
                bm_contract::wire::CapabilityCallParams {
                    capability: "system.danger.purge".into(),
                    args: json!({"target": "notes"}),
                    idempotency_key: None,
                    deadline_ms: Some(1000),
                },
            )
            .await
        }
    };

    // 高危恒审批 → 批准 count:5 → 五次消耗(Grant 余量归零)
    let req = ids.next_id("req");
    let err = call(&handle, req).await.expect_err("首次应升级审批");
    let approval_id = match &err {
        CoreError::Semantic(code, _) => {
            assert_eq!(*code, ErrorCode::ApprovalRequired);
            approval_id_of(&handle).await.expect("审批对象在场")
        }
        _ => panic!("应为审批语义错误"),
    };
    handle
        .approval_respond(
            ids.next_id("req"),
            bm_contract::wire::ApprovalRespondParams {
                approval_id,
                decision: "approve".into(),
                scope: Some("count:5".into()),
            },
        )
        .await
        .expect("批准成功");
    // 批准重放已消耗第 1 次;再显式调用 4 次共 5 次(count:5 归零)
    for i in 1..=4 {
        call(&handle, ids.next_id("req"))
            .await
            .unwrap_or_else(|_| panic!("第 {} 次消耗", i + 1));
    }
    // 重启前:余量已尽 → 审批兜底(Grant 不再命中)
    assert!(
        call(&handle, ids.next_id("req")).await.is_err(),
        "重启前 count:5 已耗尽"
    );
    handle.stop("restart").await;

    // 重启:T6c 修复后 used_count 随行恢复,Grant 仍耗尽 → 审批兜底
    // (修复前 used 回 0,免审批放行 = 静默扩权)
    let connector2: Arc<dyn bm_core::ports::ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let ids2 = Arc::new(SeqIdGen::new());
    let store2: Arc<dyn bm_persist::EventStore> =
        Arc::new(bm_persist::PersistStore::open(dir.path()).expect("重开持久层"));
    let config2 = RuntimeConfig {
        capabilities: vec![(manifest, provider)],
        version: "0.1.0-m5".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store2),
        connector: connector2,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: ids2.clone(),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    };
    let handle2 = RuntimeHandle::start(config2).await;
    let err = call(&handle2, ids2.next_id("req"))
        .await
        .expect_err("重启后耗尽的 Grant 不得复活");
    match &err {
        CoreError::Semantic(code, _) => {
            assert_eq!(
                *code,
                ErrorCode::ApprovalRequired,
                "耗尽 Grant → 审批兜底(余量未回满)"
            );
        }
        _ => panic!("应为审批语义错误"),
    }
    // 持久面直接证明:used_count = 2
    let store_check = bm_persist::PersistStore::open(dir.path()).expect("重开");
    let grants = store_check.state().list_grants().expect("读 grants");
    assert_eq!(grants[0]["used_count"], json!(5), "T6c:消费计数持久");
    handle2.stop("test_done").await;
}

/// t53(T6c 收紧 2):幂等收据跨重启——恢复后同 key 等价请求仍被抑制,
/// Provider 恰执行一次(副作用防重的持久面)。
#[tokio::test]
async fn t53_idem_receipt_survives_restart() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let dir = tempfile::tempdir().expect("临时目录");
    let send_count = Arc::new(AtomicU64::new(0));
    let sc = send_count.clone();
    let mail = bm_core::broker::provider_fn(move |_| {
        sc.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"message_id": "mock-000001", "queued": true}))
    });
    let manifest: bm_contract::capability::CapabilityManifest = serde_json::from_value(json!({
        "capability": "system.mail.mock_send", "provider": "system.mail",
        "version": "0.1.0", "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": "external-side-effect", "idempotent": true,
        "cancellable": true, "timeout_ms": 2000, "approval": "not-required"
    }))
    .unwrap();

    let make_config = |store: Option<Arc<dyn bm_persist::EventStore>>,
                       ids: Arc<SeqIdGen>,
                       mail: Arc<dyn bm_core::registry::CapabilityProvider>|
     -> RuntimeConfig {
        RuntimeConfig {
            capabilities: vec![(manifest.clone(), mail)],
            version: "0.1.0-m5".into(),
            data_dir: Some(dir.path().to_path_buf()),
            store,
            connector: Arc::new(MockConnector::new(vec![])),
            secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
            id_gen: ids,
            clock: Arc::new(SystemClock),
            turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
            max_attempts: None,
        }
    };

    let ids = Arc::new(SeqIdGen::new());
    let store: Arc<dyn bm_persist::EventStore> =
        Arc::new(bm_persist::PersistStore::open(dir.path()).expect("打开持久层"));
    let handle = RuntimeHandle::start(make_config(Some(store), ids.clone(), mail)).await;
    let call_with_key = |h: &RuntimeHandle, r: bm_contract::ids::BmId, key: &str| {
        let h = h.clone();
        let key = key.to_string();
        async move {
            h.capability_call(
                r,
                bm_contract::wire::CapabilityCallParams {
                    capability: "system.mail.mock_send".into(),
                    args: json!({"to": "a@x"}),
                    idempotency_key: Some(key),
                    deadline_ms: Some(1000),
                },
            )
            .await
        }
    };

    // 首次:升级审批 → 批准 once → 执行(Provider 计 1)
    let err = call_with_key(&handle, ids.next_id("req"), "idem-r1")
        .await
        .expect_err("首调应升级审批");
    let approval_id = match &err {
        CoreError::Semantic(code, _) => {
            assert_eq!(*code, ErrorCode::ApprovalRequired);
            approval_id_of(&handle).await.expect("审批在场")
        }
        _ => panic!("应为审批语义错误"),
    };
    handle
        .approval_respond(
            ids.next_id("req"),
            bm_contract::wire::ApprovalRespondParams {
                approval_id,
                decision: "approve".into(),
                scope: Some("once".into()),
            },
        )
        .await
        .expect("批准成功");
    // 批准重放即执行(Provider 恰 1 次);once Grant 已随重放消耗
    assert_eq!(send_count.load(Ordering::SeqCst), 1);
    handle.stop("restart").await;

    // 重启:幂等收据自持久层装载(收据行在场为 T6c 直接证据)
    let ids2 = Arc::new(SeqIdGen::new());
    let send_count2 = Arc::new(AtomicU64::new(0));
    let sc2 = send_count2.clone();
    let mail2 = bm_core::broker::provider_fn(move |_| {
        // 重启后的 Provider 实例:计数从 0 开始,若被误执行将变 1
        sc2.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"message_id": "mock-000001", "queued": true}))
    });
    let store2: Arc<dyn bm_persist::EventStore> =
        Arc::new(bm_persist::PersistStore::open(dir.path()).expect("重开持久层"));
    let handle2 = RuntimeHandle::start(make_config(Some(store2), ids2.clone(), mail2)).await;
    {
        let store_check = bm_persist::PersistStore::open(dir.path()).expect("重开");
        let receipts = store_check.state().list_idem_receipts().expect("读收据");
        assert_eq!(receipts.len(), 1, "T6c:幂等收据落表");
    }
    // once Grant 已消耗 → 同 key 调用先走审批(Grant 耗尽兜底,与 M4 t44 同构)
    let err = call_with_key(&handle2, ids2.next_id("req"), "idem-r1")
        .await
        .expect_err("Grant 耗尽应审批兜底");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));
    let approval_id = approval_id_of(&handle2).await.expect("审批在场");
    handle2
        .approval_respond(
            ids2.next_id("req"),
            bm_contract::wire::ApprovalRespondParams {
                approval_id,
                decision: "approve".into(),
                scope: Some("once".into()),
            },
        )
        .await
        .expect("批准成功");
    // 批准重放执行路径命中恢复的幂等收据仓 → 抑制,不重放副作用
    assert_eq!(
        send_count2.load(Ordering::SeqCst),
        0,
        "T6c:恢复后抑制不重放副作用"
    );
    let events = handle2.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::CapabilityInvoked
                && e.payload["outcome"] == json!("suppressed")),
        "重启后抑制有审计事件"
    );
    handle2.stop("test_done").await;
}

/// 从事件流找 waiting_user 审批对象 id(测试辅助)。
async fn approval_id_of(handle: &RuntimeHandle) -> Option<bm_contract::ids::BmId> {
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .ok()?;
    let first = list["approvals"].as_array()?.first()?.clone();
    bm_contract::ids::BmId::parse(first["approval_id"].as_str()?).ok()
}
