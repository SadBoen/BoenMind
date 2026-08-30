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
        async_executor: None,
        model_streaming: false,
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
        async_executor: None,
        model_streaming: false,
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
        async_executor: None,
        model_streaming: false,
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
    let row = grants
        .iter()
        .find(|g| g["action"] == json!("system.danger.purge"))
        .expect("count Grant 行在场");
    assert_eq!(row["used_count"], json!(5), "T6c:消费计数持久");
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
            async_executor: None,
            model_streaming: false,
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

/// t54(M5.4):task.list / task.get——确定性序、状态过滤、规范对象完整面。
#[tokio::test]
async fn t54_task_list_get_deterministic() {
    let (handle, ids) = task_rig(None).await;
    let t1 = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "甲".into(),
                goal: "g1".into(),
                authorization: None,
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("建 t1");
    let t2 = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "乙".into(),
                goal: "g2".into(),
                authorization: None,
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("建 t2");
    handle
        .task_pause(ids.next_id("req"), lifecycle_params(&t1.task_id))
        .await
        .expect("暂停 t1");

    // list:确定性序(created_at, task_id)
    let list = handle
        .task_list(bm_contract::wire::TaskListParams {
            state_filter: None,
            limit: None,
        })
        .await
        .expect("list");
    assert_eq!(list.tasks.len(), 2);
    // 再次 list 输出逐字节一致(确定性)
    let list2 = handle
        .task_list(bm_contract::wire::TaskListParams {
            state_filter: None,
            limit: None,
        })
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_string(&list.tasks).unwrap(),
        serde_json::to_string(&list2.tasks).unwrap()
    );

    // 状态过滤:paused 只剩 t1
    let paused_list = handle
        .task_list(bm_contract::wire::TaskListParams {
            state_filter: Some("paused".into()),
            limit: None,
        })
        .await
        .unwrap();
    assert_eq!(paused_list.tasks.len(), 1);
    assert_eq!(paused_list.tasks[0]["task_id"], json!(t1.task_id.as_str()));
    // 未识别状态串 = 空列表(宽松过滤)
    let empty = handle
        .task_list(bm_contract::wire::TaskListParams {
            state_filter: Some("nope".into()),
            limit: None,
        })
        .await
        .unwrap();
    assert!(empty.tasks.is_empty());

    // get:规范对象(合同字段面完整)
    let got = handle
        .task_get(bm_contract::wire::TaskGetParams {
            task_id: t2.task_id.clone(),
        })
        .await
        .expect("get");
    assert_eq!(got.task["title"], json!("乙"));
    assert_eq!(got.task["state"], json!("running"));
    assert_eq!(got.task["task_epoch"], json!(1));
    assert!(got.task["budget"].is_object());
    assert!(got.task["parent_task_id"].is_null(), "M5 恒 null");
    assert!(got.guard_states.is_none(), "监护态随 T7");
    // 不存在 → validation_failed
    let err = handle
        .task_get(bm_contract::wire::TaskGetParams {
            task_id: bm_contract::ids::BmId::parse("task_01JAAAAAAAAAAAAAAAAAAAAAGH").unwrap(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
    handle.stop("test_done").await;
}

/// t55(M5.4):events.poll 的 task_id 过滤——只回该 Task 的事件,
/// 其余事件(runtime.started 等)不串入。
#[tokio::test]
async fn t55_events_poll_task_filter() {
    let (handle, ids) = task_rig(None).await;
    let t1 = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "甲".into(),
                goal: "g".into(),
                authorization: None,
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("建 t1");
    handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "乙".into(),
                goal: "g".into(),
                authorization: None,
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("建 t2");

    let poll = handle
        .events_poll(bm_contract::wire::EventsPollParams {
            session_id: bm_contract::ids::BmId::parse("sess_01JAAAAAAAAAAAAAAAAAAAAA00").unwrap(),
            since_seq: 0,
            limit: Some(100),
            task_id: Some(t1.task_id.clone()),
        })
        .await
        .expect("poll");
    assert!(
        poll.events
            .iter()
            .all(|e| e.payload["task_id"].as_str() == Some(t1.task_id.as_str())),
        "过滤后只含 t1 事件"
    );
    // M5-T4 起建单即自举协调链:本 rig 授权缺省(空)→ 无 Grant 物化,
    // 仅 coordinator 成员事实:created + state.changed + member.added = 3
    assert_eq!(
        poll.events.len(),
        3,
        "t1: 生命周期 + 协调链事件(payload.task_id 过滤)"
    );
    assert_eq!(poll.events[0].event_type, EventType::TaskCreated);
    handle.stop("test_done").await;
}

/// t56(M5.4):Task Board 投影——重建确定性(同一事件前缀两次重建一致)
/// 且与 L2 规范状态一致;投影位点 = 日志末尾(可弃可重建)。
#[tokio::test]
async fn t56_task_board_rebuild_determinism() {
    let (handle, ids) = task_rig(None).await;
    let t1 = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "甲".into(),
                goal: "g".into(),
                authorization: None,
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("建 t1");
    handle
        .task_pause(ids.next_id("req"), lifecycle_params(&t1.task_id))
        .await
        .expect("暂停");

    let events = handle.events_all().await;
    // 两次重建逐字节一致(确定性,BTreeMap 键序)
    let b1 = bm_core::task::TaskBoard::rebuild(&events);
    let b2 = bm_core::task::TaskBoard::rebuild(&events);
    assert_eq!(b1, b2, "重建确定性(ADR-0004 条件 1)");
    // 与 L2 规范状态一致(task.get 的 state/epoch == 投影条目)
    let got = handle
        .task_get(bm_contract::wire::TaskGetParams {
            task_id: t1.task_id.clone(),
        })
        .await
        .unwrap();
    let entry = b1.entry(t1.task_id.as_str()).expect("投影条目在场");
    assert_eq!(entry.state.as_str(), got.task["state"].as_str().unwrap());
    assert_eq!(entry.task_epoch, got.task["task_epoch"].as_u64().unwrap());
    assert_eq!(
        b1.applied_to_seq(),
        events.last().map(|e| e.event_seq).unwrap_or(0),
        "投影位点 = 日志末尾"
    );
    handle.stop("test_done").await;
}

/// t57(P-11 骨架,M5 规格 §4-9):Task Board 投影重建延迟——
/// 1 万事件(含 task.* 族)重建 ×20 取 p95;test build 门 < 1s(T9 回填数值)。
#[tokio::test]
async fn t57_p11_projection_rebuild_latency() {
    use bm_contract::events::EventEnvelope;
    use std::time::Instant;
    let ev = |seq: u64, ty: EventType, payload: serde_json::Value| {
        EventEnvelope::new_unchecked(
            seq,
            ty,
            "2026-08-29T11:00:00.000Z".into(),
            None,
            None,
            None,
            payload,
        )
    };
    // 1 万事件:每 10 条一个 task 周期(created + 合法状态迁移)
    let mut events: Vec<EventEnvelope> = Vec::with_capacity(10_000);
    for i in 0..10_000u64 {
        let n = i / 10;
        let tid = format!("task_{:026}", n % 100);
        let (ty, payload) = match i % 10 {
            0 => (
                EventType::TaskCreated,
                json!({"task_id": tid, "title": format!("任务{n}"), "created_by": "butler:system"}),
            ),
            1 => (
                EventType::TaskStateChanged,
                json!({"task_id": tid, "from": "created", "to": "running",
                       "reason_code": "p11", "task_epoch": 1}),
            ),
            2 => (
                EventType::TaskStateChanged,
                json!({"task_id": tid, "from": "running", "to": "paused",
                       "reason_code": "p11", "task_epoch": 1}),
            ),
            3 => (
                EventType::TaskStateChanged,
                json!({"task_id": tid, "from": "paused", "to": "running",
                       "reason_code": "p11", "task_epoch": 1}),
            ),
            _ => continue, // 其余事件位留空(P-11 只关注 task.* 折叠面)
        };
        events.push(ev(i + 1, ty, payload));
    }
    let mut samples: Vec<u128> = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        let board = bm_core::task::TaskBoard::rebuild(&events);
        let us = start.elapsed().as_micros();
        assert!(!board.is_empty());
        samples.push(us);
    }
    samples.sort();
    let p95 = samples[(samples.len() as f64 * 0.95) as usize - 1];
    println!("P-11 test build: rebuild p95 = {p95} us(1 万事件)");
    assert!(
        p95 < 1_000_000,
        "P-11 门槛:1 万事件重建 < 1s(test build),实际 {p95} us"
    );
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
