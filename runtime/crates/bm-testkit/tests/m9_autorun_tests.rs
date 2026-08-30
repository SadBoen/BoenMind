//! M9-S3 worker 自主环 v0:t150 哨兵完成 / t152 停滞出口 / t153 暂停即时生效 /
//! t154 轮数上限出口。事件面 task.autorun.state.changed 全程留痕。
//! (规格 t151 的 worker 工具预算硬限作用于 worker_capability_call 路径,
//! 自主环 v0 回合由 max_turns 与 agent token 预算约束——偏差在 M9 回看裁决。)

use bm_contract::events::EventType;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::{TaskCreateParams, TaskLifecycleParams};
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::mock_model::{MockConnector, Step};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

async fn rig(script: Vec<Step>) -> (RuntimeHandle, Arc<SeqIdGen>) {
    let ids = Arc::new(SeqIdGen::new());
    let dir = tempfile::tempdir().expect("临时目录");
    let store: Arc<dyn bm_persist::EventStore> =
        Arc::new(bm_persist::PersistStore::open(dir.path()).expect("打开"));
    // 目录生命周期随进程(运行时仍需写 store;测试进程短命,泄漏可接受)
    std::mem::forget(dir);
    let config = RuntimeConfig {
        capabilities: vec![bm_providers::builtin::model_invoke_cap()],
        version: "0.1.0-m9".into(),
        data_dir: None,
        store: Some(store),
        connector: Arc::new(MockConnector::new(script)),
        secret_store: Arc::new(bm_providers::secret::MemSecretStore::with(
            "secret:model.x",
            "sk",
        )),
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
                title: "自主环演练".into(),
                goal: "完成一件事".into(),
                authorization: None,
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("建单");
    created.task_id
}

async fn autorun_events(handle: &RuntimeHandle) -> Vec<(String, u64, Option<String>)> {
    handle
        .events_all()
        .await
        .iter()
        .filter(|e| e.event_type == EventType::TaskAutorunStateChanged)
        .map(|e| {
            (
                e.payload["phase"].as_str().unwrap().to_string(),
                e.payload["turn"].as_u64().unwrap(),
                e.payload["reason"].as_str().map(|s| s.to_string()),
            )
        })
        .collect()
}

/// t150:三轮推进,第三轮哨兵 → 提交完成报告;证据未经核验 → 任务
/// blocked(outcome_unknown)等用户验收(M5 证据纪律),自主环出口 done。
#[tokio::test]
async fn t150_autorun_done_sentinel_completes_task() {
    let (handle, ids) = rig(vec![
        Step::ok("第一步:调研", 10, 5),
        Step::ok("第二步:写稿", 10, 5),
        Step::ok("[[AUTORUN_DONE]] 已完成:稿件就绪", 10, 5),
    ])
    .await;
    let task_id = running_task(&handle, &ids).await;
    let r = handle
        .task_autorun(
            ids.next_id("req"),
            bm_contract::wire::TaskAutorunParams {
                task_id: task_id.clone(),
                max_turns: None,
            },
        )
        .await
        .expect("受理");
    assert!(r.accepted);
    // 收敛:任务 completed
    let mut final_state = None;
    for _ in 0..300 {
        if let Some(v) = handle
            .task_list(bm_contract::wire::TaskListParams {
                state_filter: None,
                limit: None,
            })
            .await
            .ok()
            .and_then(|v| {
                v.tasks
                    .iter()
                    .find(|t| t["task_id"].as_str() == Some(task_id.as_str()))
                    .and_then(|t| t["state"].as_str().map(|x| x.to_string()))
            })
            && (v == "completed" || v == "blocked")
        {
            final_state = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        final_state.as_deref(),
        Some("blocked"),
        "未核验的完成声明应转 blocked 等验收:{final_state:?} evs={:?}",
        autorun_events(&handle).await
    );
    // 观测面留痕:verdict=unverified,guard=outcome_unknown(用户验收入口)
    let events = handle.events_all().await;
    let obs = events
        .iter()
        .find(|e| e.event_type == EventType::ObservationRecorded)
        .expect("完成报告应有观测记录");
    assert_eq!(obs.payload["verdict"], json!("unverified"));
    let evs = autorun_events(&handle).await;
    assert_eq!(evs[0].0, "started");
    assert_eq!(evs.last().unwrap().0, "finished");
    assert_eq!(evs.last().unwrap().2.as_deref(), Some("done"));
    assert_eq!(evs.last().unwrap().1, 3, "第三轮完成");
    // 事件面 phase 序列
    let phases: Vec<_> = evs.iter().map(|(p, _, _)| p.as_str()).collect();
    assert!(phases.contains(&"turn_completed"));
}

/// t152:连续两轮相同输出 → blocked(stalled)。
#[tokio::test]
async fn t152_autorun_stall_blocks_task() {
    let (handle, ids) = rig(vec![
        Step::ok("同一句话", 10, 5),
        Step::ok("同一句话", 10, 5),
        Step::ok("不应被消费", 10, 5),
    ])
    .await;
    let task_id = running_task(&handle, &ids).await;
    handle
        .task_autorun(
            ids.next_id("req"),
            bm_contract::wire::TaskAutorunParams {
                task_id: task_id.clone(),
                max_turns: None,
            },
        )
        .await
        .expect("受理");
    let mut final_state = None;
    for _ in 0..300 {
        if let Some(v) = handle
            .task_list(bm_contract::wire::TaskListParams {
                state_filter: None,
                limit: None,
            })
            .await
            .ok()
            .and_then(|v| {
                v.tasks
                    .iter()
                    .find(|t| t["task_id"].as_str() == Some(task_id.as_str()))
                    .and_then(|t| t["state"].as_str().map(|x| x.to_string()))
            })
            && v == "blocked"
        {
            final_state = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        final_state.as_deref(),
        Some("blocked"),
        "任务应被阻塞:{final_state:?}"
    );
    let evs = autorun_events(&handle).await;
    assert_eq!(evs.last().unwrap().2.as_deref(), Some("stalled"));
}

/// t153:受理后、回合在途时暂停 → 回合终局即退出(finished=paused)。
#[tokio::test]
async fn t153_autorun_pause_between_takes_effect() {
    let (handle, ids) = rig(vec![Step::ok_after("缓慢推进", 150)]).await;
    let task_id = running_task(&handle, &ids).await;
    handle
        .task_autorun(
            ids.next_id("req"),
            bm_contract::wire::TaskAutorunParams {
                task_id: task_id.clone(),
                max_turns: None,
            },
        )
        .await
        .expect("受理");
    // 回合在途窗口内暂停
    handle
        .task_pause(
            ids.next_id("req"),
            TaskLifecycleParams {
                task_id: task_id.clone(),
                reason: Some("用户暂停".into()),
                note: None,
            },
        )
        .await
        .expect("暂停");
    // 第一回合终局
    for _ in 0..300 {
        let evs = handle.events_all().await;
        if evs
            .iter()
            .any(|e| e.event_type == EventType::ModelInvocationCompleted)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    // pump 应裁决退出(paused),且不再发下一回合
    let evs = autorun_events(&handle).await;
    assert_eq!(evs.last().unwrap().0, "finished");
    assert_eq!(evs.last().unwrap().2.as_deref(), Some("paused"));
    tokio::time::sleep(Duration::from_millis(200)).await;
    let final_evs = autorun_events(&handle).await;
    assert_eq!(final_evs.len(), evs.len(), "退出后不得再推进");
}

/// t154:轮数上限 → blocked(max_turns)。
#[tokio::test]
async fn t154_autorun_max_turns_blocks_task() {
    let (handle, ids) = rig(vec![
        Step::ok("a", 10, 5),
        Step::ok("b", 10, 5),
        Step::ok("c 不应被消费", 10, 5),
    ])
    .await;
    let task_id = running_task(&handle, &ids).await;
    handle
        .task_autorun(
            ids.next_id("req"),
            bm_contract::wire::TaskAutorunParams {
                task_id: task_id.clone(),
                max_turns: Some(2),
            },
        )
        .await
        .expect("受理");
    let mut final_state = None;
    for _ in 0..300 {
        if let Some(v) = handle
            .task_list(bm_contract::wire::TaskListParams {
                state_filter: None,
                limit: None,
            })
            .await
            .ok()
            .and_then(|v| {
                v.tasks
                    .iter()
                    .find(|t| t["task_id"].as_str() == Some(task_id.as_str()))
                    .and_then(|t| t["state"].as_str().map(|x| x.to_string()))
            })
            && v == "blocked"
        {
            final_state = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        final_state.as_deref(),
        Some("blocked"),
        "任务应被阻塞:{final_state:?}"
    );
    let evs = autorun_events(&handle).await;
    assert_eq!(evs.last().unwrap().2.as_deref(), Some("max_turns"));
    assert_eq!(evs.last().unwrap().1, 2, "恰好两轮");
    let _ = OperationState::Succeeded; // 保持导入(收据口径备注)
}
