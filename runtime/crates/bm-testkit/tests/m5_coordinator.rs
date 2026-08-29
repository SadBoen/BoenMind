//! M5-T4/T5 端到端:Coordinator 三方交集物化(task:<id> Grant 启用)、
//! 授权签发链(parent 哈希)、Worker Agent 路径直通与上界默认拒绝、
//! Task 终态失效、approval task-scope 启用。
//! 承载 ADR-0002 条件 2 余项闭合与 GT-03 场景 A 的协调链面。

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

fn manifest(name: &str, effect: &str) -> bm_contract::capability::CapabilityManifest {
    serde_json::from_value(json!({
        "capability": name, "provider": name, "version": "0.1.0",
        "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": effect, "idempotent": true, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    }))
    .unwrap()
}

async fn coord_rig() -> (RuntimeHandle, Arc<SeqIdGen>) {
    let connector = Arc::new(MockConnector::new(vec![]));
    let ids = Arc::new(SeqIdGen::new());
    let config = RuntimeConfig {
        capabilities: vec![
            (
                manifest("system.notes.write", "reversible-command"),
                bm_core::broker::provider_fn(|_| Ok(json!({"written": true}))),
            ),
            (
                manifest("system.mail.mock_send", "external-side-effect"),
                bm_core::broker::provider_fn(|_| Ok(json!({"sent": true}))),
            ),
        ],
        version: "0.1.0-m5".into(),
        data_dir: None,
        store: None,
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: ids.clone(),
        clock: Arc::new(bm_core::clock::MockClock::at_ms(1_788_000_000_000)),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
    };
    (RuntimeHandle::start(config).await, ids)
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

fn worker_params(task_id: &bm_contract::ids::BmId, capability: &str) -> WorkerCallParams {
    WorkerCallParams {
        task_id: task_id.clone(),
        capability: capability.into(),
        args: json!({"path": "notes/a.md"}),
        idempotency_key: Some("task:step:1".into()),
        deadline_ms: Some(1000),
    }
}

/// t70:协调链自举——task.create 物化 Coordinator/Worker 的 task:<id>
/// Grant(事件审计)+ 成员事实(coordinator + worker)。
#[tokio::test]
async fn t70_coordination_bootstrap_on_task_create() {
    let (handle, ids) = coord_rig().await;
    let created = handle
        .task_create(
            ids.next_id("req"),
            create_params(
                "整理读书笔记",
                json!([
                    {"verb": "task.collect", "klass": "safe"},
                    {"verb": "agent.spawn", "klass": "mutation"},
                    {"verb": "capability.call", "klass": "mutation",
                     "resources": [{"capability": "system.notes.write"}]}
                ]),
            ),
        )
        .await
        .expect("建单");
    let events = handle.events_all().await;

    // grant.created:3 枚 Coordinator(动词)+ 1 枚 Worker(能力),
    // scope = task:<created>,由 butler:system / agent:coordinator 签发
    let grants: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::GrantCreated)
        .map(|e| e.payload.clone())
        .collect();
    let task_scope = format!("task:{}", created.task_id.as_str());
    let coord_aud = format!("agent:coord:{}", created.task_id.as_str());
    let worker_aud = format!("agent:worker:{}", created.task_id.as_str());
    let coord = grants
        .iter()
        .filter(|g| g["audience"] == json!(coord_aud))
        .count();
    let worker = grants
        .iter()
        .filter(|g| g["audience"] == json!(worker_aud))
        .count();
    assert_eq!(coord, 3, "每授权条目一枚 Coordinator Grant");
    assert_eq!(worker, 1, "capability.call 谓词逐枚 Worker Grant");
    for g in grants.iter().filter(|g| g["audience"] == json!(worker_aud)) {
        println!(
            "DBG scope={} vs {task_scope} | action={} | depth={} | hash_len={}",
            g["scope"],
            g["action"],
            g["delegation_depth"],
            g["parent_hash"].as_str().map(|h| h.len()).unwrap_or(0)
        );
    }
    assert!(
        grants
            .iter()
            .filter(|g| g["audience"] == json!(worker_aud))
            .all(|g| g["scope"] == json!(task_scope)
                && g["action"] == json!("system.notes.write")
                && g["delegation_depth"] == json!(0)
                && g["parent_hash"]
                    .as_str()
                    .map(|h| h.len() == 64)
                    .unwrap_or(false)),
        "Worker Grant:task scope + 不可转授 + 父链哈希(issued_by 在持久行,事件 payload 键集按注册表冻结)"
    );
    assert!(
        grants
            .iter()
            .filter(|g| g["audience"] == json!(coord_aud))
            .all(|g| g["scope"] == json!(task_scope) && g["delegation_depth"] == json!(0)),
        "Coordinator Grant:task scope(签发者 issued_by 在持久行,不在事件键集)"
    );

    // 成员事实:coordinator + worker,挂各自 Grant
    let members: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::TaskMemberAdded)
        .map(|e| e.payload.clone())
        .collect();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0]["role"], json!("coordinator"));
    assert_eq!(members[1]["role"], json!("worker"));
    assert_eq!(members[1]["task_id"], json!(created.task_id.as_str()));
    handle.stop("test_done").await;
}

/// t71:Worker Agent 路径直通(GT-03 A3)——task:<id> Grant 命中,免审批
/// 执行,收据 principal = agent:worker(来源标注);Task 终态后失效。
#[tokio::test]
async fn t71_worker_grant_direct_pass_and_terminal_revocation() {
    let (handle, ids) = coord_rig().await;
    let created = handle
        .task_create(
            ids.next_id("req"),
            create_params(
                "整理读书笔记",
                json!([
                    {"verb": "capability.call", "klass": "mutation",
                     "resources": [{"capability": "system.notes.write"}]}
                ]),
            ),
        )
        .await
        .expect("建单");

    // Worker 调用:Grant 命中直通(收据 principal = agent:worker = 来源标注)
    let receipt = handle
        .worker_capability_call(
            ids.next_id("req"),
            worker_params(&created.task_id, "system.notes.write"),
        )
        .await
        .expect("task grant 直通");
    assert_eq!(
        receipt["principal"],
        json!(format!("agent:worker:{}", created.task_id.as_str()))
    );
    assert_eq!(receipt["state"], json!("succeeded"));
    let events = handle.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::CapabilityInvoked
                && e.payload["principal"]
                    == json!(format!("agent:worker:{}", created.task_id.as_str()))
                && e.payload["outcome"] == json!("ok")),
        "审计归因:worker principal 的 capability.invoked"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.event_type == EventType::ApprovalRequested),
        "Grant 命中优先,不撞审批"
    );

    // 停止 Task:终态 → task:<id> Grant 全量撤销(Task 结束即失效)
    handle
        .task_stop(
            ids.next_id("req"),
            bm_contract::wire::TaskLifecycleParams {
                task_id: created.task_id.clone(),
                reason: Some("done".into()),
                note: None,
            },
        )
        .await
        .expect("停止");
    let events = handle.events_all().await;
    let revoked: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::GrantRevoked)
        .filter(|e| e.payload["reason"] == json!("task_cancelled"))
        .collect();
    assert!(!revoked.is_empty(), "终态撤销有审计事件");
    // 终态后 Worker 再调用 → 拒绝(Grant 已失效 + Task 已终态)
    let err = handle
        .worker_capability_call(
            ids.next_id("req"),
            worker_params(&created.task_id, "system.notes.write"),
        )
        .await
        .expect_err("终态后成员调用被拒");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
    handle.stop("test_done").await;
}

/// t72:上界默认拒绝——未授权能力(即使已注册)对 Worker 无 Grant:
/// untrusted × 副作用 → 100% 升级审批(不存在免审通道)。
#[tokio::test]
async fn t72_worker_unauthorized_capability_escalates() {
    let (handle, ids) = coord_rig().await;
    let created = handle
        .task_create(
            ids.next_id("req"),
            create_params(
                "只授权写笔记",
                json!([
                    {"verb": "capability.call", "klass": "mutation",
                     "resources": [{"capability": "system.notes.write"}]}
                ]),
            ),
        )
        .await
        .expect("建单");
    // mail 未在授权资源谓词内 → 无 Grant → 升级审批(非直通)
    let err = handle
        .worker_capability_call(
            ids.next_id("req"),
            worker_params(&created.task_id, "system.mail.mock_send"),
        )
        .await
        .expect_err("未授权能力必须升级审批");
    assert!(
        matches!(err, CoreError::Semantic(ErrorCode::ApprovalRequired, _)),
        "{err:?}"
    );
    handle.stop("test_done").await;
}

/// t73:approval task-scope 启用(M4 解读条款 4 兑现)——引用存在的
/// Task 可批准物化 task scope Grant;引用不存在的 Task = validation_failed。
#[tokio::test]
async fn t73_approval_task_scope_enabled() {
    let (handle, ids) = coord_rig().await;
    let created = handle
        .task_create(ids.next_id("req"), create_params("存在任务", json!([])))
        .await
        .expect("建单");

    // 直调高危 → 审批 → 以 task:<created> scope 批准
    let err = handle
        .capability_call(
            ids.next_id("req"),
            bm_contract::wire::CapabilityCallParams {
                capability: "system.mail.mock_send".into(),
                args: json!({"to": "a@x"}),
                idempotency_key: None,
                deadline_ms: Some(1000),
            },
        )
        .await
        .expect_err("高危升级审批");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));
    let list = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .unwrap();
    let approval_id = list["approvals"][0]["approval_id"].as_str().unwrap();
    let respond = handle
        .approval_respond(
            ids.next_id("req"),
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(approval_id).unwrap(),
                decision: "approve".into(),
                scope: Some(format!("task:{}", created.task_id.as_str())),
            },
        )
        .await
        .expect("task scope 批准应通过");
    assert_eq!(respond["state"], json!("approved"));

    // task scope Grant 生效:同类调用此后命中直通(批量预授权语义,
    // ADR-0002 裁决 4;直至 Task 终态失效)
    let again = handle
        .capability_call(
            ids.next_id("req"),
            bm_contract::wire::CapabilityCallParams {
                capability: "system.mail.mock_send".into(),
                args: json!({"to": "b@x"}),
                idempotency_key: None,
                deadline_ms: Some(1000),
            },
        )
        .await
        .expect("task scope Grant 命中直通");
    assert_eq!(again["state"], json!("succeeded"));

    // 引用不存在的 Task 的 task scope → validation_failed
    // (用未授权能力触发新审批,避免被已有 task scope Grant 吸收)
    let err2 = handle
        .capability_call(
            ids.next_id("req"),
            bm_contract::wire::CapabilityCallParams {
                capability: "system.notes.write".into(),
                args: json!({"p": 1}),
                idempotency_key: None,
                deadline_ms: Some(1000),
            },
        )
        .await
        .expect_err("notes.write 无 Grant → 升级审批");
    assert!(matches!(
        err2,
        CoreError::Semantic(ErrorCode::ApprovalRequired, _)
    ));
    let list2 = handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
        .unwrap();
    let approval_id2 = list2["approvals"][0]["approval_id"].as_str().unwrap();
    let err = handle
        .approval_respond(
            ids.next_id("req"),
            bm_contract::wire::ApprovalRespondParams {
                approval_id: bm_contract::ids::BmId::parse(approval_id2).unwrap(),
                decision: "approve".into(),
                scope: Some("task:task_01JAAAAAAAAAAAAAAAAAAAAAGH".into()),
            },
        )
        .await
        .expect_err("引用不存在的 Task 必须被拒");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
    handle.stop("test_done").await;
}
