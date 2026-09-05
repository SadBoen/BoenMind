//! M5-T6/T7 端到端:Task 预算包络(软限告警/硬限 blocked/用户扩容恢复)
//! 与 Watchdog 长期监护(停滞检测/编排重启事实事件/硬顶/waiting_approval
//! 豁免/重复动作)。承载基线 §9.7 预算三层强制点与 ADR-0004 条件 6。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::TaskCreateParams;
use bm_core::CoreError;
use bm_core::clock::MockClock;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle, WorkerCallParams};
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use serde_json::json;
use std::sync::Arc;

const BASE_MS: u128 = 1_788_000_000_000; // 2026-08-29T10:40:00.000Z

async fn bw_rig(script_err: bool) -> (RuntimeHandle, Arc<SeqIdGen>, Arc<MockClock>) {
    let connector = Arc::new(MockConnector::new(vec![]));
    let ids = Arc::new(SeqIdGen::new());
    let clock = Arc::new(MockClock::at_ms(BASE_MS));
    let provider = if script_err {
        bm_core::broker::provider_fn(|_| Err("mock 失败".into()))
    } else {
        bm_core::broker::provider_fn(|_| Ok(json!({"written": true})))
    };
    let config = RuntimeConfig {
        capabilities: vec![
            (
                serde_json::from_value::<bm_contract::capability::CapabilityManifest>(json!({
                    "capability": "system.notes.write", "provider": "system.notes",
                    "version": "0.1.0", "input_schema": {"type": "object"},
                    "output_schema": {"type": "object"},
                    "effect": "reversible-command", "idempotent": true,
                    "cancellable": true, "timeout_ms": 1000, "approval": "not-required"
                }))
                .unwrap(),
                provider,
            ),
            (
                serde_json::from_value::<bm_contract::capability::CapabilityManifest>(json!({
                    "capability": "system.mail.mock_send", "provider": "system.mail",
                    "version": "0.1.0", "input_schema": {"type": "object"},
                    "output_schema": {"type": "object"},
                    "effect": "external-side-effect", "idempotent": true,
                    "cancellable": true, "timeout_ms": 1000, "approval": "not-required"
                }))
                .unwrap(),
                bm_core::broker::provider_fn(|_| Ok(json!({"sent": true}))),
            ),
        ],
        version: "0.1.0-m5".into(),
        data_dir: None,
        store: None,
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk")),
        id_gen: ids.clone(),
        clock: clock.clone(),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
        async_executor: None,
        model_streaming: false,
    };
    (RuntimeHandle::start(config).await, ids, clock)
}

fn create_params(title: &str, max_tool_calls: Option<u64>) -> TaskCreateParams {
    let budget = max_tool_calls
        .map(|n| json!({"max_tokens": 1000000, "max_turns": 1000, "max_tool_calls": n}));
    TaskCreateParams {
        title: title.into(),
        goal: "g".into(),
        authorization: serde_json::from_value(json!([
            {"verb": "capability.call", "klass": "mutation",
             "resources": [{"capability": "system.notes.write"}]}
        ]))
        .unwrap(),
        budget: serde_json::from_value(budget.unwrap_or(json!(null))).unwrap(),
        deadline: None,
    }
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

/// t80:软限 80% 告警 + 硬限 blocked(budget_exhausted)。
#[tokio::test]
async fn t80_budget_soft_warning_and_hard_block() {
    let (handle, ids, _clock) = bw_rig(false).await;
    let created = handle
        .task_create(ids.next_id("req"), create_params("包络任务", Some(5)))
        .await
        .expect("建单");

    // 第 1..=3 次正常(4 = 80%×5,第 4 次调用触发软限)
    for i in 1..=3 {
        handle
            .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
            .await
            .unwrap_or_else(|_| panic!("第 {i} 次应成功"));
    }
    handle
        .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
        .await
        .expect("第 4 次仍在包络内(软限只告警)");
    let events = handle.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::BudgetWarning),
        "软限 80% 有 budget.warning"
    );

    // 第 6 次:used=5,+1=6 > 5 → 拒绝 + Task blocked
    handle
        .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
        .await
        .expect("第 5 次恰在包络边界");
    let err = handle
        .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
        .await
        .expect_err("第 6 次超硬限");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::BudgetExceeded, _)
    ));
    let events = handle.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::BudgetExceeded)
    );
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::TaskStateChanged
                && e.payload["to"] == json!("blocked")
                && e.payload["reason_code"] == json!("budget_exhausted")),
        "硬限 → blocked(budget_exhausted)"
    );
    // blocked 后成员调用被拒
    let err = handle
        .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
        .await
        .expect_err("blocked 后调用被拒");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));
    handle.stop("test_done").await;
}

/// t81:用户扩容 → task.budget.increased + blocked 恢复运行(user_resolved)。
#[tokio::test]
async fn t81_budget_increase_unblocks() {
    let (handle, ids, _clock) = bw_rig(false).await;
    let created = handle
        .task_create(ids.next_id("req"), create_params("扩容任务", Some(2)))
        .await
        .expect("建单");
    handle
        .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
        .await
        .expect("第 1 次");
    handle
        .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
        .await
        .expect("第 2 次");
    let err = handle
        .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
        .await
        .expect_err("第 3 次超限 → blocked");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::BudgetExceeded, _)
    ));

    // 用户扩容:2 → 10;blocked 恢复运行(user_resolved)
    let out = handle
        .task_budget_increase(created.task_id.clone(), 10)
        .await
        .expect("扩容成功");
    assert_eq!(out["state"], json!("running"));
    let events = handle.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::TaskBudgetIncreased
                && e.payload["old_limit"] == json!(2)
                && e.payload["new_limit"] == json!(10)),
        "扩容事实事件"
    );
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::TaskStateChanged
                && e.payload["from"] == json!("blocked")
                && e.payload["to"] == json!("running")
                && e.payload["reason_code"] == json!("user_resolved")),
        "blocked→running(user_resolved)"
    );
    // 恢复后调用继续(记账从 3 起累计)
    handle
        .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
        .await
        .expect("扩容后调用恢复");
    handle.stop("test_done").await;
}

/// t82:停滞检测 → 编排重启事实事件;硬顶 → blocked(不再自动重启)。
#[tokio::test]
async fn t82_watchdog_stall_and_hard_limit() {
    let (handle, ids, clock) = bw_rig(false).await;
    let created = handle
        .task_create(ids.next_id("req"), create_params("会停滞的任务", None))
        .await
        .expect("建单");

    // 推进 16 分钟(> stalled_after=15min)→ 手动扫描
    clock.advance_ms(16 * 60 * 1000);
    let n = handle.watchdog_scan().await.expect("扫描");
    assert_eq!(n, 2, "task.stalled + watchdog.reorchestration.triggered");
    let events = handle.events_all().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::TaskStalled)
    );
    assert!(
        events.iter().any(
            |e| e.event_type == EventType::WatchdogReorchestrationTriggered
                && e.payload["trigger"] == json!("watchdog")
        ),
        "编排重启触发事实事件(ADR-0004 条件 6:触发者之二)"
    );
    // 同 episode 不重复通告
    clock.advance_ms(60 * 1000);
    assert_eq!(handle.watchdog_scan().await.unwrap(), 0, "不重复通告");
    // Task 仍为 running(重编程由编排器消费事实事件后自行决策;监督层不动步)
    let got = handle
        .task_get(bm_contract::wire::TaskGetParams {
            task_id: created.task_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(got.task["state"], json!("running"));

    // 推进跨硬顶(自最近进度 25 小时)→ blocked(stall_hard_limit)
    clock.advance_ms(25 * 60 * 60 * 1000);
    let n = handle.watchdog_scan().await.expect("扫描");
    assert_eq!(n, 1, "硬顶只发状态变更");
    let got = handle
        .task_get(bm_contract::wire::TaskGetParams {
            task_id: created.task_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(got.task["state"], json!("blocked"));
    handle.stop("test_done").await;
}

/// t83:waiting_approval 豁免——成员调用停在审批(等人)不判停滞。
#[tokio::test]
async fn t83_waiting_approval_exempt_from_stall() {
    let (handle, ids, clock) = bw_rig(false).await;
    let created = handle
        .task_create(ids.next_id("req"), create_params("等审批的任务", None))
        .await
        .expect("建单");

    // mail 未进任务授权资源谓词 → 无 Grant → 100% 升级审批(等用户)
    let err = handle
        .worker_capability_call(
            ids.next_id("req"),
            WorkerCallParams {
                task_id: created.task_id.clone(),
                capability: "system.mail.mock_send".into(),
                args: json!({"to": "a@x"}),
                idempotency_key: None,
                deadline_ms: Some(1000),
            },
        )
        .await
        .expect_err("未授权能力升级审批");
    assert!(matches!(
        err,
        CoreError::ApprovalNeeded { .. }
    ));

    // 推进 16 分钟 → 扫描:审批挂起(waiting_approval)豁免,不判停滞
    clock.advance_ms(16 * 60 * 1000);
    let n = handle.watchdog_scan().await.expect("扫描");
    assert_eq!(n, 0, "等人的时间不算停滞(waiting_approval 豁免)");
    handle.stop("test_done").await;
}

/// t84:重复动作检测——同 capability+args+outcome 连续 3 次 → task.repeating。
#[tokio::test]
async fn t84_repeat_detection() {
    let (handle, ids, _clock) = bw_rig(true).await;
    let created = handle
        .task_create(ids.next_id("req"), create_params("空转任务", None))
        .await
        .expect("建单");
    for i in 1..=3 {
        let _ = handle
            .worker_capability_call(ids.next_id("req"), worker_params(&created.task_id))
            .await;
        if i < 3 {
            // 前两次:重复计数累积,不发事件
            let events = handle.events_all().await;
            assert!(
                !events
                    .iter()
                    .any(|e| e.event_type == EventType::TaskRepeating),
                "第 {i} 次不应触发 repeating"
            );
        }
    }
    let events = handle.events_all().await;
    let repeating = events
        .iter()
        .find(|e| e.event_type == EventType::TaskRepeating)
        .expect("第 3 次重复触发 repeating");
    assert_eq!(repeating.payload["repeat_count"], json!(3));
    assert_eq!(repeating.payload["capability"], json!("system.notes.write"));
    handle.stop("test_done").await;
}
