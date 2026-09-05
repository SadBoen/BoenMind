//! M5-T8 端到端:Observation 完成判定门禁(声称完成必经核验;unverified
//! 不得 completed)+ memory.* 三能力(经 Broker;作用域即边界;纠正覆盖;
//! 删除级联)。承载基线 M5 通过条件第 4 条与 memory 数据域隔离面。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::TaskCreateParams;
use bm_core::CoreError;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle, WorkerCallParams};
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;

async fn om_rig() -> (RuntimeHandle, Arc<SeqIdGen>) {
    let dir = tempfile::tempdir().expect("临时目录");
    let dir_path = dir.path().to_path_buf();
    // 测试进程存续期保持目录(泄漏 TempDir 跳过析构:rig 生命周期 = 测试)
    std::mem::forget(dir);
    let store: Arc<dyn bm_persist::EventStore> =
        Arc::new(bm_persist::PersistStore::open(&dir_path).expect("打开"));
    let connector = Arc::new(MockConnector::new(vec![]));
    let ids = Arc::new(SeqIdGen::new());
    let mut caps = vec![
        (
            serde_json::from_value::<bm_contract::capability::CapabilityManifest>(json!({
                "capability": "system.notes.write", "provider": "system.notes",
                "version": "0.1.0", "input_schema": {"type": "object"},
                "output_schema": {"type": "object"},
                "effect": "reversible-command", "idempotent": true,
                "cancellable": true, "timeout_ms": 1000, "approval": "not-required",
                "verification": {"query": "system.echo", "expect": "exists", "within_ms": 2000}
            }))
            .unwrap(),
            bm_core::broker::provider_fn(|_| Ok(json!({"written": true}))),
        ),
        (
            serde_json::from_value::<bm_contract::capability::CapabilityManifest>(json!({
                "capability": "system.echo", "provider": "system.echo",
                "version": "0.1.0", "input_schema": {"type": "object"},
                "output_schema": {"type": "object"},
                "effect": "read-only", "idempotent": true,
                "cancellable": true, "timeout_ms": 1000, "approval": "not-required"
            }))
            .unwrap(),
            bm_core::broker::provider_fn(|args| Ok(json!({"echo": args, "exists": true}))),
        ),
    ];
    caps.extend(bm_core::memory::memory_capabilities(
        store.clone(),
        ids.clone(),
    ));
    let config = RuntimeConfig {
        capabilities: caps,
        version: "0.1.0-m5".into(),
        data_dir: Some(dir_path),
        store: Some(store),
        connector,
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

async fn create_task(
    handle: &RuntimeHandle,
    ids: &Arc<SeqIdGen>,
    title: &str,
) -> bm_contract::wire::TaskCreateResult {
    handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: title.into(),
                goal: "g".into(),
                authorization: serde_json::from_value(json!([
                    {"verb": "capability.call", "klass": "mutation",
                     "resources": [{"capability": "system.notes.write"}]}
                ]))
                .unwrap(),
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("建单")
}

/// t85:声称完成 → verification 核验(确定性查询)→ completed。
#[tokio::test]
async fn t85_verified_completion_gate() {
    let (handle, ids) = om_rig().await;
    let created = create_task(&handle, &ids, "核验完成").await;

    // Worker 执行(直通)+ 声称完成(带所涉 Operation)
    let receipt = handle
        .worker_capability_call(
            ids.next_id("req"),
            WorkerCallParams {
                task_id: created.task_id.clone(),
                capability: "system.notes.write".into(),
                args: json!({"path": "notes/a.md"}),
                idempotency_key: None,
                deadline_ms: Some(1000),
            },
        )
        .await
        .expect("worker 调用");
    let op_id = bm_contract::ids::BmId::parse(receipt["operation_id"].as_str().unwrap()).unwrap();

    let report = handle
        .task_report_completion(created.task_id.clone(), "声称归档完成", Some(op_id))
        .await
        .expect("核验报告");
    assert_eq!(report["verdict"], json!("verified"));
    assert_eq!(report["state"], json!("completed"));

    // 观测面:observation.recorded 事件 + Observation Log 行
    let events = handle.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::ObservationRecorded
                && e.payload["verdict"] == json!("verified")),
        "核验观测有事件"
    );
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::TaskStateChanged
                && e.payload["to"] == json!("completed")
                && e.payload["reason_code"] == json!("verified_completion")),
        "完成经 verified_completion 门禁"
    );
    handle.stop("test_done").await;
}

/// t86:无证据声称 → unverified → blocked(outcome_unknown_pending)等用户
/// 裁定;用户恢复后可继续(禁止自动标成功)。
#[tokio::test]
async fn t86_unverified_claim_blocks() {
    let (handle, ids) = om_rig().await;
    let created = create_task(&handle, &ids, "无证据声称").await;

    let report = handle
        .task_report_completion(created.task_id.clone(), "我就是做完了", None)
        .await
        .expect("报告受理");
    assert_eq!(report["verdict"], json!("unverified"));
    assert_eq!(report["state"], json!("blocked"));

    let events = handle.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::ObservationRecorded
                && e.payload["verdict"] == json!("unverified")
                && e.payload["guard_state"] == json!("outcome_unknown")),
        "unverified 观测落地"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.event_type == EventType::TaskStateChanged
                && e.payload["to"] == json!("completed")),
        "禁止自动标成功"
    );

    // 用户裁定恢复(user_resolved)
    handle
        .task_resume(
            ids.next_id("req"),
            bm_contract::wire::TaskLifecycleParams {
                task_id: created.task_id.clone(),
                reason: None,
                note: Some("我确认,继续".into()),
            },
        )
        .await
        .expect("用户裁定恢复");
    let got = handle
        .task_get(bm_contract::wire::TaskGetParams {
            task_id: created.task_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(got.task["state"], json!("running"));
    handle.stop("test_done").await;
}

/// t87:memory.* 三能力经 Broker——写入/检索/删除(墓碑+级联),纠正覆盖。
#[tokio::test]
async fn t87_memory_capabilities_and_cascade() {
    let (handle, ids) = om_rig().await;
    let call = |h: &RuntimeHandle,
                r: bm_contract::ids::BmId,
                capability: &str,
                args: serde_json::Value| {
        let h = h.clone();
        let capability = capability.to_string();
        async move {
            h.capability_call(
                r,
                bm_contract::wire::CapabilityCallParams {
                    capability,
                    args,
                    idempotency_key: None,
                    deadline_ms: Some(1000),
                },
            )
            .await
        }
    };
    let scope = format!("memory:task:{}", "task_01JAAAAAAAAAAAAAAAAAAAAAB2");

    // 写入两条(来源级联:B 引用 A)
    let a = call(
        &handle,
        ids.next_id("req"),
        "memory.write",
        json!({"scope": scope, "content_ref": "protected://mem/a",
               "content_preview": "用户偏好:深色主题"}),
    )
    .await
    .expect("写 A");
    let entry_a = a["result"]["entry_id"].as_str().unwrap().to_string();
    let b = call(
        &handle,
        ids.next_id("req"),
        "memory.write",
        json!({"scope": scope, "content_ref": "protected://mem/b",
               "content_preview": "引用:主题设置来源", "source_ref": entry_a}),
    )
    .await
    .expect("写 B");
    let entry_b = b["result"]["entry_id"].as_str().unwrap().to_string();
    assert_ne!(entry_a, entry_b);

    // 检索命中
    let found = call(
        &handle,
        ids.next_id("req"),
        "memory.search",
        json!({"scope": scope, "query": "深色主题"}),
    )
    .await
    .expect("检索");
    assert_eq!(found["result"]["count"], json!(1), "FTS/LIKE 检索命中 A");

    // 删除 A:reversible 类 → 审批(删除需审批)→ 批准 once → 执行
    let err = call(
        &handle,
        ids.next_id("req"),
        "memory.delete",
        json!({"entry_id": entry_a}),
    )
    .await
    .expect_err("删除属可逆类,需审批");
    assert!(matches!(err, CoreError::ApprovalNeeded { .. }));
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .unwrap();
    let del_appr = list["approvals"][0]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    handle
        .approval_respond(
            ids.next_id("req"),
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(del_appr).unwrap(),
                decision: "approve".into(),
                scope: Some("once".into()),
            },
        )
        .await
        .expect("批准删除");
    // 批准重放已执行删除(once Grant 恰一次);级联效果以检索面验证
    let found2 = call(
        &handle,
        ids.next_id("req"),
        "memory.search",
        json!({"scope": scope, "query": "深色主题"}),
    )
    .await
    .expect("再检索");
    assert_eq!(
        found2["result"]["count"],
        json!(0),
        "删除+级联后检索不复活(A 与其来源 B 均墓碑)"
    );
    let found_b = call(
        &handle,
        ids.next_id("req"),
        "memory.search",
        json!({"scope": scope, "query": "主题设置来源"}),
    )
    .await
    .expect("检索 B");
    assert_eq!(
        found_b["result"]["count"],
        json!(0),
        "来源被删除 → 级联失效 B"
    );

    // 用户纠正:correction_of 即时墓碑化被纠正条目
    let c = call(
        &handle,
        ids.next_id("req"),
        "memory.write",
        json!({"scope": scope, "content_ref": "protected://mem/c",
               "content_preview": "主题设置:跟随系统", "correction_of": entry_b}),
    )
    .await
    .expect("纠正写 C");
    assert!(c["result"]["entry_id"].as_str().is_some());
    let found3 = call(
        &handle,
        ids.next_id("req"),
        "memory.search",
        json!({"scope": scope, "query": "主题设置"}),
    )
    .await
    .expect("检索主题");
    assert_eq!(found3["result"]["count"], json!(1), "被纠正条目不再命中");
    handle.stop("test_done").await;
}

/// t88:非法 scope 被拒(作用域即权限边界的形态面)。
#[tokio::test]
async fn t88_memory_scope_boundary() {
    let (handle, ids) = om_rig().await;
    let err = handle
        .capability_call(
            ids.next_id("req"),
            bm_contract::wire::CapabilityCallParams {
                capability: "memory.write".into(),
                args: json!({"scope": "memory:everything", "content_ref": "x"}),
                idempotency_key: None,
                deadline_ms: Some(1000),
            },
        )
        .await;
    // 合同 scope pattern 之外:provider 校验拒(memory:everything 形态非法)
    // — manifest input_schema 不约束,Provider 端做形态校验
    match err {
        Err(CoreError::Semantic(ErrorCode::Internal, _))
        | Err(CoreError::Internal)
        | Err(CoreError::Semantic(ErrorCode::ValidationFailed, _)) => {}
        Ok(_) => panic!("非法 scope 必须被拒"),
        Err(e) => panic!("意外错误 {e:?}"),
    }
    handle.stop("test_done").await;
}
