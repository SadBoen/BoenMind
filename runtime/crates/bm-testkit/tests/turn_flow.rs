//! M1.3/M1.4 回合流:单回合成功、降级链耗尽、显式取消、多回合。
//! 承载 INV-1 / INV-2 / INV-4 / INV-11 / INV-12(取消入口)。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::states::OperationState;
use bm_contract::wire::GetOperationParams;
use bm_core::CoreError;
use bm_providers::mock_model::Step;
use bm_testkit::invariants::{assert_event_stream_wellformed, assert_single_terminal};
use bm_testkit::replay::TestRig;

async fn wait_terminal(rig: &TestRig, op: &bm_contract::ids::BmId) -> bm_contract::wire::Receipt {
    loop {
        let r = rig
            .handle
            .operations_get(GetOperationParams {
                operation_id: op.clone(),
            })
            .await
            .expect("收据查询");
        if r.state.is_terminal() {
            return r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn t07_single_turn_success_event_shape() {
    // GT 场景 A 的前 8 条事件形态
    let rig = TestRig::standard(vec![Step::ok("幂等性是指……", 412, 58)]).await;
    let (sess, agent) = rig.create_session().await.expect("会话创建成功");
    let receipt = rig
        .send(&sess, &agent, "用一句话解释什么是幂等性")
        .await
        .expect("回合发起");

    let final_receipt = wait_terminal(&rig, &receipt.operation_id).await;
    assert_eq!(final_receipt.state, OperationState::Succeeded);
    assert!(final_receipt.completed_at.is_some());
    let result_ref = final_receipt.result_reference.expect("成功收据带结果引用");
    assert_eq!(
        result_ref.kind,
        bm_contract::wire::ResultRefKind::ExecutionLog
    );
    assert_eq!(result_ref.r#ref, format!("log:{}", receipt.operation_id));

    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);
    // 过滤启动期 bootstrap Grant 事件(系统事实,非回合流;M5 起)
    let types: Vec<EventType> = events
        .iter()
        .filter(|e| e.event_type != EventType::GrantCreated)
        .map(|e| e.event_type)
        .collect();
    assert_eq!(
        types,
        vec![
            EventType::RuntimeStarted,
            EventType::SessionCreated,
            EventType::AgentCreated,
            EventType::AgentTurnStarted,
            EventType::AgentWaitingModel,
            EventType::ModelInvocationCompleted,
            EventType::CapabilityInvoked, // M7 S1:模型调用过 Broker 的审计事件
            EventType::OperationStateChanged,
            EventType::AgentCompleted,
        ],
        "GT 场景 A 前 9 条事件形态(M7 起含 capability.invoked)"
    );

    // INV-1
    assert_single_terminal(&events, receipt.operation_id.as_str());
    // 完成事件携带用量(GT-A3)
    let completed = events
        .iter()
        .find(|e| e.event_type == EventType::ModelInvocationCompleted)
        .expect("存在");
    assert_eq!(completed.payload["usage_in"], 412);
    assert_eq!(completed.payload["usage_out"], 58);
    assert_eq!(completed.payload["attempt"], 1);
    assert_eq!(completed.payload["latency_ms"], 1873);

    rig.stop().await;
}

#[tokio::test]
async fn t08_chain_exhausted_maps_to_failed_not_outcome_unknown() {
    // GT 场景 B:两次尝试均 timeout → failed(INV-11:无外部副作用不得 outcome_unknown)
    let rig = TestRig::standard(vec![Step::timeout(), Step::timeout()]).await;
    let (sess, agent) = rig.create_session().await.expect("会话创建成功");
    let receipt = rig
        .send(&sess, &agent, "会超时的问题")
        .await
        .expect("回合发起");

    let final_receipt = wait_terminal(&rig, &receipt.operation_id).await;
    assert_eq!(
        final_receipt.state,
        OperationState::Failed,
        "INV-11:无外部副作用失败落 failed"
    );
    let err = final_receipt.error.expect("failed 收据带错误");
    assert_eq!(err.code.get(), ErrorCode::Timeout);
    assert!(!err.retryable, "降级链耗尽后 retryable=false(GT-B 信封)");

    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);
    // 过滤启动期 bootstrap Grant 事件(系统事实,非回合流;M5 起)
    let types: Vec<EventType> = events
        .iter()
        .filter(|e| e.event_type != EventType::GrantCreated)
        .map(|e| e.event_type)
        .collect();
    assert_eq!(
        types,
        vec![
            EventType::RuntimeStarted,
            EventType::SessionCreated,
            EventType::AgentCreated,
            EventType::AgentTurnStarted,
            EventType::AgentWaitingModel,
            EventType::ModelInvocationFailed,
            EventType::ModelInvocationFailed,
            EventType::CapabilityInvoked, // M7 S1:链耗尽落定时的审计(outcome=error)
            EventType::OperationStateChanged,
            EventType::AgentFailed,
        ],
        "GT 场景 B 事件形态(简版补全;M7 起含 capability.invoked)"
    );
    // INV-4:每次尝试恰好一条 failed 事件
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == EventType::ModelInvocationFailed)
            .count(),
        2
    );
    // 绝对下标改为按类型过滤定位(启动期 bootstrap Grant 事件使序号位移)
    let failed_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::ModelInvocationFailed)
        .collect();
    let failed1 = failed_events[0];
    assert_eq!(failed1.payload["attempt"], 1);
    assert_eq!(failed1.payload["model_id"], bm_testkit::replay::MODEL_A);
    let failed2 = failed_events[1];
    assert_eq!(failed2.payload["attempt"], 2);
    assert_eq!(
        failed2.payload["model_id"],
        bm_testkit::replay::MODEL_B,
        "第二尝试降级到链上第二个模型"
    );

    assert_single_terminal(&events, receipt.operation_id.as_str());
    assert!(
        !events
            .iter()
            .any(|e| e.payload["to"].as_str() == Some("outcome_unknown")),
        "INV-11:模型调用超时不得落 outcome_unknown"
    );

    rig.stop().await;
}

#[tokio::test]
async fn t09_explicit_cancel_lands_cancelled() {
    // INV-12:唯一 cancelled 入口 = agent.cancel
    let rig = TestRig::standard(vec![Step::ok_after("来不及", 10_000)]).await;
    let (sess, agent) = rig.create_session().await.expect("会话创建成功");
    let receipt = rig.send(&sess, &agent, "取消我").await.expect("回合发起");

    let cancel = rig
        .handle
        .agent_cancel(bm_contract::wire::CancelParams {
            session_id: sess.clone(),
            agent_id: agent.clone(),
            operation_id: receipt.operation_id.clone(),
        })
        .await
        .expect("取消受理");
    assert!(cancel.accepted);

    let final_receipt = wait_terminal(&rig, &receipt.operation_id).await;
    assert_eq!(final_receipt.state, OperationState::Cancelled);
    assert_eq!(
        final_receipt
            .error
            .as_ref()
            .expect("cancelled 带错误码")
            .code
            .get(),
        ErrorCode::Cancelled
    );

    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);
    assert_single_terminal(&events, receipt.operation_id.as_str());
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::AgentCancelled)
    );

    // 取消后 agent 终态 stopped(经 stopping),会话仍可查询收据(INV-9)
    let again = rig
        .handle
        .operations_get(GetOperationParams {
            operation_id: receipt.operation_id.clone(),
        })
        .await
        .expect("收据查询");
    assert_eq!(again, final_receipt);

    rig.stop().await;
}

#[tokio::test]
async fn t10_cancel_on_terminal_operation_rejected() {
    let rig = TestRig::standard(vec![Step::ok("快答", 10, 5)]).await;
    let (sess, agent) = rig.create_session().await.expect("会话创建成功");
    let receipt = rig.send(&sess, &agent, "取消我").await.expect("回合发起");
    let final_receipt = wait_terminal(&rig, &receipt.operation_id).await;
    assert_eq!(final_receipt.state, OperationState::Succeeded);

    let err = rig
        .handle
        .agent_cancel(bm_contract::wire::CancelParams {
            session_id: sess.clone(),
            agent_id: agent.clone(),
            operation_id: receipt.operation_id.clone(),
        })
        .await
        .expect_err("终态不可取消");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));

    rig.stop().await;
}

#[tokio::test]
async fn t11_multi_turn_sequence_keeps_invariants() {
    // INV-1 属性的确定性验证:连续 5 回合,每回合恰好一个终态
    let rig = TestRig::standard(bm_testkit::replay::repeat(Step::ok("答", 100, 50), 10)).await;
    let (sess, agent) = rig.create_session().await.expect("会话创建成功");

    let mut op_ids = Vec::new();
    for i in 1..=5 {
        let receipt = rig
            .send(&sess, &agent, &format!("问题{i}"))
            .await
            .expect("回合发起");
        let turn_index = i as i64;
        let r = wait_terminal(&rig, &receipt.operation_id).await;
        assert_eq!(r.state, OperationState::Succeeded, "回合 {i} 成功");
        op_ids.push(receipt.operation_id.clone());
        // turn_index 递增
        let started = rig
            .all_events()
            .await
            .into_iter()
            .rev()
            .find(|e| e.event_type == EventType::AgentTurnStarted)
            .expect("存在");
        assert_eq!(started.payload["turn_index"], turn_index);
    }

    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);
    for op in &op_ids {
        assert_single_terminal(&events, op.as_str());
    }
    // 回合事件计数:5 个 started / 5 个 completed
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == EventType::AgentTurnStarted)
            .count(),
        5
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == EventType::AgentCompleted)
            .count(),
        5
    );

    rig.stop().await;
}
