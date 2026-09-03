//! 失败自愈回归(2026-09-03,外部评审复核 + BACKLOG「失败回合后 agent 卡死」):
//! 回合失败 agent 落 failed 后,同会话再次发消息必须自愈受理
//! (合同增发 failed→running,guard resend_after_failure),而不是
//! 恒 400「agent 不在可接单状态」把会话打死。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{BmId, IdGen};
use bm_contract::states::{AgentState, OperationState};
use bm_contract::wire::GetOperationParams;
use bm_providers::mock_model::Step;
use bm_testkit::replay::TestRig;
use std::time::Duration;

async fn wait_terminal(rig: &TestRig, operation_id: &BmId) -> OperationState {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let r = rig
            .handle
            .operations_get(GetOperationParams {
                operation_id: operation_id.clone(),
            })
            .await
            .expect("查询操作");
        if r.state.is_terminal() {
            return r.state;
        }
        assert!(tokio::time::Instant::now() < deadline, "回合未在时限内落定");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn failed_agent_self_heals_on_resend() {
    // 双模型链首个 attempt 即不可重试失败(直接终局,只耗 1 个脚本步)
    // → agent failed;第二个脚本步留给自愈后的第二回合。
    let script = vec![
        Step::Fail {
            error_code: ErrorCode::Internal,
            retryable: false,
        },
        Step::ok("自愈后正常回答", 10, 5),
    ];
    let rig = TestRig::standard(script).await;
    let (sid, aid) = rig.create_session().await.expect("建会话");

    // 第一回合:链耗尽,回合 Failed,agent 落 failed
    let r1 = rig.send(&sid, &aid, "第一问").await.expect("回合受理");
    assert_eq!(
        wait_terminal(&rig, &r1.operation_id).await,
        OperationState::Failed
    );
    let resumed_before = rig
        .all_events()
        .await
        .iter()
        .filter(|e| e.event_type == EventType::AgentResumed)
        .count();
    assert_eq!(resumed_before, 0, "失败落定前不应有 agent.resumed");

    // 第二回合:此前会恒 400「agent 不在可接单状态」;现在应自愈受理并成功
    let r2 = rig
        .send(&sid, &aid, "第二问")
        .await
        .expect("failed 后再发消息应自愈受理");
    assert_eq!(
        wait_terminal(&rig, &r2.operation_id).await,
        OperationState::Succeeded
    );

    // 自愈发过 agent.resumed,且投影/内存 agent 已回 running
    let resumed = rig
        .all_events()
        .await
        .iter()
        .filter(|e| e.event_type == EventType::AgentResumed)
        .count();
    assert_eq!(resumed, 1, "自愈恰好发一次 agent.resumed");
    let resume = rig
        .handle
        .session_resume(
            rig.ids.next_id("req"),
            bm_contract::wire::SessionResumeParams {
                session_id: sid.clone(),
                since_seq: None,
            },
        )
        .await
        .expect("resume 查询");
    assert_eq!(resume.agent_state, AgentState::Running);
}
