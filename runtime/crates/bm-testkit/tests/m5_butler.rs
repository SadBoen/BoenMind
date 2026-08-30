//! M5-T3 端到端:Butler 内置 App——bootstrap 协调权物化与跨重启稳定、
//! 撤销后 task.create 拒绝(重授走审批)、Task 授权声明校验(动词上界 +
//! safe/mutation 二分)。承载基线 M5 通过条件第 1 条「Butler 只有协调权限」。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::TaskCreateParams;
use bm_core::CoreError;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;

async fn butler_rig(dir: Option<&std::path::Path>) -> (RuntimeHandle, Arc<SeqIdGen>) {
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

fn grant_created_audience(events: &[bm_contract::events::EventEnvelope]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.event_type == EventType::GrantCreated)
        .map(|e| {
            e.payload["audience"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

fn create_params(title: &str, auth: serde_json::Value) -> TaskCreateParams {
    TaskCreateParams {
        title: title.into(),
        goal: "g".into(),
        authorization: serde_json::from_value(auth).unwrap(),
        budget: None,
        deadline: None,
    }
}

/// t60:引导物化——12 个协调动词 Grant(audience=butler:system,forever,
/// issued_by=runtime_bootstrap);跨重启幂等(不重复签发)。
#[tokio::test]
async fn t60_bootstrap_grants_materialized_and_stable() {
    let dir = tempfile::tempdir().expect("临时目录");
    let (handle, _ids) = butler_rig(Some(dir.path())).await;
    let events = handle.events_all().await;
    let created = grant_created_audience(&events);
    assert_eq!(
        created.iter().filter(|a| *a == "butler:system").count(),
        12,
        "首启签发 §10.1 全集 12 个协调动词"
    );
    // 形态:forever + 无审批父(引导权标记)+ delegation_depth=0
    let sample = events
        .iter()
        .find(|e| e.event_type == EventType::GrantCreated)
        .unwrap();
    assert_eq!(sample.payload["scope"], json!("forever"));
    assert_eq!(sample.payload["delegation_depth"], json!(0));
    assert_eq!(sample.payload["approval_id"], json!(null));
    handle.stop("restart").await;

    // 重启:不重复签发(幂等)
    let (handle2, _ids2) = butler_rig(Some(dir.path())).await;
    let events2 = handle2.events_all().await;
    assert_eq!(
        grant_created_audience(&events2)
            .iter()
            .filter(|a| *a == "butler:system")
            .count(),
        12,
        "重启后 Grant 总数不变(恢复自持久行,不重发)"
    );
    // 协调权在位:task.create 正常
    let created_task = handle2
        .task_create(
            bm_contract::ids::SeqIdGen::new().next_id("req"),
            create_params("重启后建单", json!([])),
        )
        .await
        .expect("协调权在位,建单成功");
    assert_eq!(created_task.state, bm_contract::states::TaskState::Running);
    handle2.stop("test_done").await;
}

/// t61:撤销——bootstrap Grant 集全撤(grant.revoked 审计),task.create
/// 被拒(permission_denied);既有 Task 不受影响;重启后不复活。
#[tokio::test]
async fn t61_butler_revoke_blocks_task_create() {
    let dir = tempfile::tempdir().expect("临时目录");
    let (handle, ids) = butler_rig(Some(dir.path())).await;
    // 撤销前:建单正常
    let t1 = handle
        .task_create(ids.next_id("req"), create_params("既有任务", json!([])))
        .await
        .expect("撤销前建单成功");

    let revoked = handle
        .butler_revoke("用户撤销协调权")
        .await
        .expect("撤销成功");
    assert_eq!(revoked, 12, "全集撤销");
    let events = handle.events_all().await;
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == EventType::GrantRevoked)
            .count(),
        12,
        "撤销有审计事件"
    );

    // 撤销后:task.create 拒绝(mutation 协调权已失)
    let err = handle
        .task_create(ids.next_id("req"), create_params("应被拒", json!([])))
        .await
        .expect_err("撤销后建单必须被拒");
    assert!(
        matches!(err, CoreError::Semantic(ErrorCode::PermissionDenied, _)),
        "{err:?}"
    );
    // 撤销不影响既有 Task(查询面照常)
    let list = handle
        .task_list(bm_contract::wire::TaskListParams {
            state_filter: None,
            limit: None,
        })
        .await
        .expect("查询照常");
    assert_eq!(list.tasks.len(), 1, "既有 Task 存在性不受撤销影响");
    assert_eq!(list.tasks[0]["task_id"], json!(t1.task_id.as_str()));
    handle.stop("restart").await;

    // 重启:已撤销的 bootstrap Grant 不复活(持久事实),建单仍被拒
    let (handle2, ids2) = butler_rig(Some(dir.path())).await;
    let events2 = handle2.events_all().await;
    assert_eq!(
        grant_created_audience(&events2)
            .iter()
            .filter(|a| *a == "butler:system")
            .count(),
        12,
        "重启后无新签发(撤销行已持久)"
    );
    let err = handle2
        .task_create(ids2.next_id("req"), create_params("仍应被拒", json!([])))
        .await
        .expect_err("重启后协调权仍处于撤销态");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::PermissionDenied, _)
    ));
    handle2.stop("test_done").await;
}

/// t62:Task 授权声明校验——领域动词不可授权(Butler 上界);mutation 动词
/// 必须显式 klass=mutation;合法清单(GT-03 A1 形态)通过。
#[tokio::test]
async fn t62_task_authorization_upper_bound() {
    let (handle, ids) = butler_rig(None).await;
    // 领域动词 → validation_failed(合同 enum 之外;运行期二次校验)
    let err = handle
        .task_create(
            ids.next_id("req"),
            create_params("越界", json!([{"verb": "mail.read", "klass": "safe"}])),
        )
        .await
        .expect_err("领域动词不可授权");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
    // mutation 动词缺 klass=mutation → validation_failed(须显式列出)
    let err = handle
        .task_create(
            ids.next_id("req"),
            create_params("隐式", json!([{"verb": "agent.spawn"}])),
        )
        .await
        .expect_err("mutation 动词必须显式 klass=mutation");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
    // safe 动词缺省 klass = 合法(可默认继承)
    let ok = handle
        .task_create(
            ids.next_id("req"),
            create_params("合法", json!([{"verb": "task.collect"}])),
        )
        .await
        .expect("safe 动词缺省 klass 合法");
    assert_eq!(ok.state, bm_contract::states::TaskState::Running);
    // GT-03 A1 形态(safe + mutation 混合显式)→ 合法
    let ok2 = handle
        .task_create(
            ids.next_id("req"),
            create_params(
                "GT-03 形态",
                json!([
                    {"verb": "task.collect", "klass": "safe"},
                    {"verb": "agent.spawn", "klass": "mutation"},
                    {"verb": "agent.stop", "klass": "mutation"}
                ]),
            ),
        )
        .await
        .expect("合法清单");
    assert_eq!(ok2.state, bm_contract::states::TaskState::Running);
    handle.stop("test_done").await;
}
