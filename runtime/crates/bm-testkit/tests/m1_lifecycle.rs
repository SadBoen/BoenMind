//! M1.1/M1.2 生命周期:启停、Session 创建/恢复/关闭、事件拉取。
//! 承载 INV-3 / INV-6 / INV-8 / INV-9 / INV-12(排空路径)。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::IdGen;
use bm_contract::states::{OperationState, SessionState};
use bm_contract::wire::{GetOperationParams, SessionCloseParams, SessionResumeParams};
use bm_core::CoreError;
use bm_providers::mock_model::Step;
use bm_testkit::invariants::{assert_event_stream_wellformed, assert_single_terminal};
use bm_testkit::replay::TestRig;

fn repeating_ok() -> Vec<Step> {
    bm_testkit::replay::repeat(Step::ok("回答", 412, 58), 20)
}

#[tokio::test]
async fn t02_session_lifecycle_and_poll() {
    let rig = TestRig::standard(repeating_ok()).await;
    let (sess, _agent) = rig.create_session().await.expect("会话创建成功");

    // 事件:1 runtime.started, 2 session.created, 3 agent.created
    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);
    assert_eq!(events.len(), 3, "创建期恰好 3 条事件");
    assert_eq!(events[0].event_type, EventType::RuntimeStarted);
    assert_eq!(events[1].event_type, EventType::SessionCreated);
    assert_eq!(events[2].event_type, EventType::AgentCreated);

    // 合同 events.poll:按会话过滤 + since_seq 增量
    let poll = rig
        .handle
        .events_poll(bm_contract::wire::EventsPollParams {
            session_id: sess.clone(),
            since_seq: 2,
            limit: Some(10),
            task_id: None,
        })
        .await
        .expect("poll 成功");
    assert_eq!(poll.events.len(), 1);
    assert_eq!(poll.events[0].event_type, EventType::AgentCreated);

    // resume(active 会话幂等重连)
    let resumed = rig
        .handle
        .session_resume(
            rig.ids.next_id("req"),
            SessionResumeParams {
                session_id: sess.clone(),
                since_seq: Some(0),
            },
        )
        .await
        .expect("resume 成功");
    assert_eq!(resumed.session_state, SessionState::Active);
    assert_eq!(
        resumed.agent_state,
        bm_contract::states::AgentState::Running
    );
    assert_eq!(
        resumed.events.len(),
        2,
        "补发 since 之后的事件(会话过滤,不含 runtime.started)"
    );

    // 关闭后 resume → validation_failed;二次 close 同样
    rig.handle
        .session_close(
            rig.ids.next_id("req"),
            SessionCloseParams {
                session_id: sess.clone(),
                reason: Some("user_request".into()),
            },
        )
        .await
        .expect("close 成功");
    let err = rig
        .handle
        .session_resume(
            rig.ids.next_id("req"),
            SessionResumeParams {
                session_id: sess.clone(),
                since_seq: Some(0),
            },
        )
        .await
        .expect_err("closed 不可 resume");
    assert!(matches!(
        err,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));

    let err2 = rig
        .handle
        .session_close(
            rig.ids.next_id("req"),
            SessionCloseParams {
                session_id: sess.clone(),
                reason: None,
            },
        )
        .await
        .expect_err("二次 close 报错");
    assert!(matches!(
        err2,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));

    // 未知会话
    let err3 = rig
        .handle
        .session_close(
            rig.ids.next_id("req"),
            SessionCloseParams {
                session_id: rig.ids.next_id("sess"),
                reason: None,
            },
        )
        .await
        .expect_err("未知会话");
    assert!(matches!(
        err3,
        CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));

    // 停机事件:event 4 = session.closed;5/6 = stopping/stopped
    rig.handle.stop("test_done").await;
    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);
    let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
    assert!(types.ends_with(&[
        EventType::SessionClosed,
        EventType::RuntimeStopping,
        EventType::RuntimeStopped,
    ]));
}

#[tokio::test]
async fn t04_close_during_flight_does_not_cancel() {
    // INV-6
    let rig = TestRig::standard(vec![Step::ok_after("慢回答", 300)]).await;
    let (sess, agent) = rig.create_session().await.expect("会话创建成功");
    let receipt = rig.send(&sess, &agent, "慢问题").await.expect("回合发起");
    assert_eq!(receipt.state, OperationState::Running);

    let close = rig
        .handle
        .session_close(
            rig.ids.next_id("req"),
            SessionCloseParams {
                session_id: sess.clone(),
                reason: Some("user_request".into()),
            },
        )
        .await
        .expect("close 成功");
    assert_eq!(close.agent_final_state, "waiting_model");

    // close 后收据仍可查询,且状态未被 close 改变
    let mid = rig
        .handle
        .operations_get(GetOperationParams {
            operation_id: receipt.operation_id.clone(),
        })
        .await
        .expect("收据查询");
    assert_eq!(mid.state, OperationState::Running);

    // 轮询至终态:回合自然完成
    let final_receipt = loop {
        let r = rig
            .handle
            .operations_get(GetOperationParams {
                operation_id: receipt.operation_id.clone(),
            })
            .await
            .expect("收据查询");
        if r.state.is_terminal() {
            break r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };
    assert_eq!(
        final_receipt.state,
        OperationState::Succeeded,
        "INV-6:close 后回合自然完成"
    );

    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);
    assert_single_terminal(&events, receipt.operation_id.as_str());
    assert!(
        !events
            .iter()
            .any(|e| e.event_type == EventType::AgentCancelled),
        "INV-6/12:close 不产生 cancelled"
    );

    rig.stop().await;
}

#[tokio::test]
async fn t05_stop_drains_in_flight_turn() {
    // INV-12:runtime.stop 走排空,不取消
    let rig = TestRig::standard(vec![Step::ok_after("排空后回答", 200)]).await;
    let (sess, agent) = rig.create_session().await.expect("会话创建成功");
    let receipt = rig.send(&sess, &agent, "排空间题").await.expect("回合发起");

    // 与回合并发停机:核心循环排空到回合完成才退出
    let stop_handle = {
        let h = rig.handle.clone();
        tokio::spawn(async move { h.stop("shutdown").await })
    };
    // 停机期间事件端口仍应答(排空期照常),等 runtime.stopped 出现
    loop {
        let events = rig.all_events().await;
        if events
            .iter()
            .any(|e| e.event_type == EventType::RuntimeStopped)
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    stop_handle.await.expect("stop 任务完成");

    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);
    assert_single_terminal(&events, receipt.operation_id.as_str());
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::OperationStateChanged
                && e.payload["to"].as_str() == Some("succeeded")),
        "INV-12:排空后回合完成"
    );
    assert!(
        !events
            .iter()
            .any(|e| e.payload["to"].as_str() == Some("cancelled")),
        "INV-12:无 operation 落 cancelled"
    );
}

#[tokio::test]
async fn t06_receipt_idempotent_after_terminal() {
    // INV-9
    let rig = TestRig::standard(repeating_ok()).await;
    let (sess, agent) = rig.create_session().await.expect("会话创建成功");
    let receipt = rig.send(&sess, &agent, "幂等吗").await.expect("回合发起");

    let final_receipt = loop {
        let r = rig
            .handle
            .operations_get(GetOperationParams {
                operation_id: receipt.operation_id.clone(),
            })
            .await
            .expect("收据查询");
        if r.state.is_terminal() {
            break r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };

    let mut repeats = Vec::new();
    for _ in 0..5 {
        repeats.push(
            rig.handle
                .operations_get(GetOperationParams {
                    operation_id: receipt.operation_id.clone(),
                })
                .await
                .expect("收据查询"),
        );
    }
    repeats.push(final_receipt.clone());
    bm_testkit::invariants::assert_receipts_idempotent(&repeats);
    assert_eq!(final_receipt.state, OperationState::Succeeded);

    rig.stop().await;
}
