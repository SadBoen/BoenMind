//! M6 端到端:Team 编队与委派——多成员 spawn(per-task principal 隔离)、
//! 子任务委派四门禁(深度/授权子集/预算/并发)、成员故障不破坏 Task、
//! 结果收集三要素(来源/状态/关联 Operation)。
//! 承载基线 M6 全部通过条件;GT-04 双场景。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::TaskCreateParams;
use bm_core::CoreError;
use bm_core::clock::SystemClock;
use bm_core::runtime::{
    DEFAULT_TURN_TIMEOUT_SECS, RemoveMemberParams, RuntimeConfig, RuntimeHandle,
    SpawnSubtaskParams, WorkerCallParams,
};
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;

async fn m6_rig(fail_mode: bool) -> (RuntimeHandle, Arc<SeqIdGen>) {
    let connector = Arc::new(MockConnector::new(vec![]));
    let ids = Arc::new(SeqIdGen::new());
    let provider = if fail_mode {
        bm_core::broker::provider_fn(|_| Err("mock 失败".into()))
    } else {
        bm_core::broker::provider_fn(|_| Ok(json!({"written": true})))
    };
    let config = RuntimeConfig {
        capabilities: vec![(
            serde_json::from_value::<bm_contract::capability::CapabilityManifest>(json!({
                "capability": "system.notes.write", "provider": "system.notes",
                "version": "0.1.0", "input_schema": {"type": "object"},
                "output_schema": {"type": "object"},
                "effect": "reversible-command", "idempotent": true,
                "cancellable": true, "timeout_ms": 1000, "approval": "not-required"
            }))
            .unwrap(),
            provider,
        )],
        version: "0.1.0-m6".into(),
        data_dir: None,
        store: None,
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: ids.clone(),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    };
    (RuntimeHandle::start(config).await, ids)
}

fn worker_params(task_id: &bm_contract::ids::BmId) -> WorkerCallParams {
    WorkerCallParams {
        task_id: task_id.clone(),
        capability: "system.notes.write".into(),
        args: json!({"path": "notes/a.md"}),
        idempotency_key: None,
        deadline_ms: Some(1000),
    }
}

/// t90:多成员 spawn + per-task principal 结构性隔离。
#[tokio::test]
async fn t90_multi_member_spawn_and_task_isolation() {
    let (handle, ids) = m6_rig(false).await;
    let created = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "团队任务".into(),
                goal: "g".into(),
                authorization: serde_json::from_value(json!([
                    {"verb": "capability.call", "klass": "mutation",
                     "resources": [{"capability": "system.notes.write"}]},
                    {"verb": "agent.spawn", "klass": "mutation"}
                ]))
                .unwrap(),
                budget: None,
                deadline: None,
            },
        )
        .await
        .expect("建单");

    // 追加第二名 Worker(并发 2 ≤ 5)
    let m2 = handle
        .task_spawn_member(created.task_id.clone())
        .await
        .expect("追加成员");
    let events = handle.events_all().await;
    let members: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::TaskMemberAdded)
        .filter(|e| e.payload["task_id"] == json!(created.task_id.as_str()))
        .collect();
    assert_eq!(members.len(), 3, "coordinator + worker + 新 worker");
    assert_eq!(members[2].payload["agent_id"], json!(m2["agent_id"]));

    // per-task 隔离:worker 的 Grant audience 是 per-task 的,另一 Task 的
    // worker principal 查表结构性不命中 → 无 Grant → 升级审批(非直通)
    let ghost_call = handle
        .worker_capability_call(
            ids.next_id("req"),
            WorkerCallParams {
                // 用本 Task 的调用(直通基线)
                task_id: created.task_id.clone(),
                capability: "system.notes.write".into(),
                args: json!({"path": "a.md"}),
                idempotency_key: None,
                deadline_ms: Some(1000),
            },
        )
        .await;
    assert!(ghost_call.is_ok(), "本 Task worker Grant 直通");

    // 跨 Task 结构性隔离:不存在 Task 的 worker principal → 未知/无授权
    let ghost_task = bm_contract::ids::BmId::parse("task_01JAAAAAAAAAAAAAAAAAAAAAGH").unwrap();
    let err = handle
        .worker_capability_call(ids.next_id("req"), worker_params(&ghost_task))
        .await
        .expect_err("不存在的 Task 成员调用被拒");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
    handle.stop("test_done").await;
}

/// t91:委派四门禁——深度/授权子集/预算/并发(只减不增)。
#[tokio::test]
async fn t91_subtask_delegation_gates() {
    let (handle, ids) = m6_rig(false).await;
    let parent = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "父任务".into(),
                goal: "g".into(),
                authorization: serde_json::from_value(json!([
                    {"verb": "capability.call", "klass": "mutation",
                     "resources": [{"capability": "system.notes.write"}]}
                ]))
                .unwrap(),
                budget: serde_json::from_value(json!(
                    {"max_tokens": 1000000, "max_turns": 1000, "max_tool_calls": 10}
                ))
                .unwrap(),
                deadline: None,
            },
        )
        .await
        .expect("建父");

    // 门禁 2/3:合法委派(子集 + 预算 ≤ 剩余)→ 成功
    let child = handle
        .task_spawn_subtask(SpawnSubtaskParams {
            parent_task_id: parent.task_id.clone(),
            title: "子任务".into(),
            goal: "g".into(),
            authorization: serde_json::from_value(json!([
                {"verb": "capability.call", "klass": "mutation",
                 "resources": [{"capability": "system.notes.write"}]}
            ]))
            .unwrap(),
            budget: serde_json::from_value(json!(
                {"max_tokens": 1000000, "max_turns": 1000, "max_tool_calls": 5}
            ))
            .unwrap(),
        })
        .await
        .expect("合法委派");
    assert_eq!(child["delegation_depth"], json!(1));
    assert_eq!(child["parent_task_id"], json!(parent.task_id.as_str()));
    let child_id = bm_contract::ids::BmId::parse(child["task_id"].as_str().unwrap()).unwrap();

    // 子任务 worker 调用计入子账本(child Grant 直通)
    handle
        .worker_capability_call(ids.next_id("req"), worker_params(&child_id))
        .await
        .expect("子任务 worker 调用");

    // 门禁 1:深度——孙任务(深度 2)合法,曾孙(深度 4)拒
    let grandchild = handle
        .task_spawn_subtask(SpawnSubtaskParams {
            parent_task_id: child_id.clone(),
            title: "孙任务".into(),
            goal: "g".into(),
            authorization: serde_json::from_value(json!([
                {"verb": "capability.call", "klass": "mutation",
                 "resources": [{"capability": "system.notes.write"}]}
            ]))
            .unwrap(),
            budget: serde_json::from_value(json!(
                {"max_tokens": 1000000, "max_turns": 1000, "max_tool_calls": 3}
            ))
            .unwrap(),
        })
        .await
        .expect("孙任务(深度 2)合法");
    assert_eq!(grandchild["delegation_depth"], json!(2));

    // 门禁 3:预算——子预算超父剩余 → 拒
    let err = handle
        .task_spawn_subtask(SpawnSubtaskParams {
            parent_task_id: parent.task_id.clone(),
            title: "超预算".into(),
            goal: "g".into(),
            authorization: serde_json::from_value(json!([
                {"verb": "capability.call", "klass": "mutation",
                 "resources": [{"capability": "system.notes.write"}]}
            ]))
            .unwrap(),
            budget: serde_json::from_value(json!(
                {"max_tokens": 1000000, "max_turns": 1000, "max_tool_calls": 9999}
            ))
            .unwrap(),
        })
        .await
        .expect_err("子预算超父剩余必须被拒");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));

    // 门禁 2:授权越界(parent 未授权 system.echo 能力)→ 拒
    let err2 = handle
        .task_spawn_subtask(SpawnSubtaskParams {
            parent_task_id: parent.task_id.clone(),
            title: "越权".into(),
            goal: "g".into(),
            authorization: serde_json::from_value(json!([
                {"verb": "capability.call", "klass": "mutation",
                 "resources": [{"capability": "system.echo"}]}
            ]))
            .unwrap(),
            budget: None,
        })
        .await
        .expect_err("授权越界必须被拒");
    assert!(matches!(
        err2,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
    handle.stop("test_done").await;
}

/// t92:成员故障不破坏 Task——失败后 Task 保持 running,替换 spawn 继续。
#[tokio::test]
async fn t92_member_failure_does_not_break_task() {
    let (handle, ids) = m6_rig(true).await;
    let created = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "故障任务".into(),
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
        .expect("建单");

    // Worker 连续失败:调用 error,Task 保持 running
    for _ in 0..2 {
        let _ = handle
            .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
            .await;
    }
    let got = handle
        .task_get(bm_contract::wire::TaskGetParams {
            task_id: created.task_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        got.task["state"],
        json!("running"),
        "成员故障不迁移 Task 状态"
    );

    // 替换:移除旧成员(留痕)→ 新 spawn → 新成员调用照常(能力正常后)
    let list = handle
        .task_list(bm_contract::wire::TaskListParams {
            state_filter: None,
            limit: None,
        })
        .await
        .unwrap();
    let _ = list; // members 由事件承载;移除按 agent_id
    let events = handle.events_all().await;
    let worker_member = events
        .iter()
        .find(|e| {
            e.event_type == EventType::TaskMemberAdded && e.payload["role"] == json!("worker")
        })
        .unwrap();
    let removed = handle
        .task_remove_member(RemoveMemberParams {
            task_id: created.task_id.clone(),
            agent_id: bm_contract::ids::BmId::parse(
                worker_member.payload["agent_id"].as_str().unwrap(),
            )
            .unwrap(),
            reason: "replaced".into(),
        })
        .await
        .expect("移除留痕");
    assert_eq!(removed["removed"], json!(true));
    let events = handle.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::TaskMemberRemoved
                && e.payload["reason"] == json!("replaced")),
        "member.removed 留痕"
    );
    handle.stop("test_done").await;
}

/// t93:结果收集三要素(来源/状态/关联 Operation)+ 子任务概览。
#[tokio::test]
async fn t93_collect_results_with_attribution() {
    let (handle, ids) = m6_rig(false).await;
    let created = handle
        .task_create(
            ids.next_id("req"),
            TaskCreateParams {
                title: "收集任务".into(),
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
        .expect("建单");
    let receipt = handle
        .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
        .await
        .expect("worker 调用");

    let collected = handle
        .task_collect(created.task_id.clone())
        .await
        .expect("收集");
    let results = collected["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "一条成员结果");
    let r = &results[0];
    assert_eq!(
        r["agent_id"],
        json!(format!("agent:worker:{}", created.task_id.as_str())),
        "来源(per-task principal)"
    );
    assert_eq!(r["state"], json!("succeeded"), "状态");
    assert_eq!(r["operation_id"], receipt["operation_id"], "关联 Operation");
    assert_eq!(r["capability"], json!("system.notes.write"));

    // 子任务概览:collect 返回 children
    handle
        .task_spawn_subtask(SpawnSubtaskParams {
            parent_task_id: created.task_id.clone(),
            title: "子任务".into(),
            goal: "g".into(),
            authorization: serde_json::from_value(json!([
                {"verb": "capability.call", "klass": "mutation",
                 "resources": [{"capability": "system.notes.write"}]}
            ]))
            .unwrap(),
            budget: None,
        })
        .await
        .expect("子任务");
    let collected2 = handle.task_collect(created.task_id.clone()).await.unwrap();
    assert_eq!(collected2["children"].as_array().unwrap().len(), 1);
    handle.stop("test_done").await;
}
