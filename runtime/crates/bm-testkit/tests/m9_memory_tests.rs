//! M9-S1 记忆抽屉授权(runtime 级;broker 单测见 broker.rs tests):
//! worker 主体只可写本任务抽屉(t130);越界升级审批,批准签发**带 scope
//! 谓词**的 Grant,重调命中台账(t131/t132);跨 agent 抽屉升级(t133);
//! search 对 user 抽屉放宽、他人抽屉升级(t135)。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::{ApprovalListParams, ApprovalRespondParams, TaskCreateParams};
use bm_core::CoreError;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle, WorkerCallParams};
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;

async fn rig() -> (RuntimeHandle, Arc<SeqIdGen>) {
    let dir = tempfile::tempdir().expect("临时目录");
    let store: Arc<dyn bm_persist::EventStore> =
        Arc::new(bm_persist::PersistStore::open(dir.path()).expect("打开"));
    let ids = Arc::new(SeqIdGen::new());
    let config = RuntimeConfig {
        capabilities: bm_core::memory::memory_capabilities(store, ids.clone()),
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

async fn running_task(handle: &RuntimeHandle, ids: &Arc<SeqIdGen>) -> bm_contract::ids::BmId {
    let created = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "抽屉授权演练".into(),
                goal: "验证 worker 抽屉边界".into(),
                authorization: serde_json::from_value(json!([
                    {"verb": "task.collect", "klass": "safe"}
                ]))
                .unwrap(),
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("建单");
    created.task_id
}

fn worker_mem(
    task_id: &bm_contract::ids::BmId,
    capability: &str,
    scope: &str,
    key: &str,
) -> WorkerCallParams {
    WorkerCallParams {
        task_id: task_id.clone(),
        capability: capability.into(),
        args: json!({"scope": scope, "content_ref": "protected://m9/x",
                     "content_preview": "预览", "query": "预览"}),
        idempotency_key: Some(key.into()),
        deadline_ms: Some(1000),
    }
}

/// t130:worker 写本任务抽屉 → 常量放行,真实落库(返回 entry_id)。
#[tokio::test]
async fn t130_worker_own_task_drawer_write_ok() {
    let (handle, ids) = rig().await;
    let task_id = running_task(&handle, &ids).await;
    let scope = format!("memory:task:{}", task_id.as_str());
    let r = handle
        .worker_capability_call(
            ids.next_id("req"),
            worker_mem(&task_id, "memory.write", &scope, "k1"),
        )
        .await
        .expect("本任务抽屉放行");
    assert!(r["result"]["entry_id"].as_str().is_some(), "{r}");
}

/// t131+t132:worker 写 user 抽屉 → 升级审批;批准签发的 Grant 带 scope
/// 谓词;重调命中台账放行(且仅限该 scope)。
#[tokio::test]
async fn t131_132_escalate_approve_grant_predicate_then_retry_ok() {
    let (handle, ids) = rig().await;
    let task_id = running_task(&handle, &ids).await;

    let err = handle
        .worker_capability_call(
            ids.next_id("req"),
            worker_mem(&task_id, "memory.write", "memory:user", "k1"),
        )
        .await
        .expect_err("越界必须升级");
    assert!(
        matches!(err, CoreError::Semantic(ErrorCode::ApprovalRequired, _)),
        "{err:?}"
    );

    // 找到挂起审批并批准(forever)
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
    handle
        .approval_respond(
            ids.next_id("req"),
            ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(approval_id).expect("解析"),
                decision: "approve".into(),
                scope: Some("count:5".into()),
            },
        )
        .await
        .expect("批准");

    // Grant 带抽屉谓词
    let events = handle.events_all().await;
    let grant = events
        .iter()
        .filter(|e| e.event_type == EventType::GrantCreated)
        .find(|e| e.payload["action"] == json!("memory.write"))
        .expect("签发 Grant")
        .payload
        .clone();
    assert_eq!(
        grant["resource"]["args_predicates"]["scope"],
        json!("memory:user"),
        "批准必须只覆盖被批准的抽屉:{grant}"
    );

    // 重调命中台账 → 放行
    let r = handle
        .worker_capability_call(
            ids.next_id("req"),
            worker_mem(&task_id, "memory.write", "memory:user", "k2"),
        )
        .await
        .expect("Grant 命中后放行");
    assert!(r["result"]["entry_id"].as_str().is_some());

    // 换一个未被批准的抽屉 → 仍升级(谓词限定生效)
    let err = handle
        .worker_capability_call(
            ids.next_id("req"),
            worker_mem(
                &task_id,
                "memory.write",
                "memory:agent:AGENTAGENTAGENTAGENTAG2",
                "k3",
            ),
        )
        .await
        .expect_err("谓词未覆盖的抽屉仍须升级");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));
}

/// t133:worker 写他人 agent 抽屉 → 升级审批。
#[tokio::test]
async fn t133_worker_cross_agent_drawer_escalates() {
    let (handle, ids) = rig().await;
    let task_id = running_task(&handle, &ids).await;
    let err = handle
        .worker_capability_call(
            ids.next_id("req"),
            worker_mem(
                &task_id,
                "memory.write",
                "memory:agent:AGENTAGENTAGENTAGENTAG2",
                "k1",
            ),
        )
        .await
        .expect_err("跨主体抽屉必须升级");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));
}

/// t135:search 对 user 抽屉放宽(读不污染);他人 agent 抽屉仍升级。
#[tokio::test]
async fn t135_worker_search_user_ok_cross_agent_escalates() {
    let (handle, ids) = rig().await;
    let task_id = running_task(&handle, &ids).await;
    let ok = handle
        .worker_capability_call(
            ids.next_id("req"),
            worker_mem(&task_id, "memory.search", "memory:user", "k1"),
        )
        .await
        .expect("user 抽屉检索放行");
    assert_eq!(ok["result"]["count"], json!(0));
    let err = handle
        .worker_capability_call(
            ids.next_id("req"),
            worker_mem(
                &task_id,
                "memory.search",
                "memory:agent:AGENTAGENTAGENTAGENTAG2",
                "k2",
            ),
        )
        .await
        .expect_err("他人抽屉检索须升级");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));
}
