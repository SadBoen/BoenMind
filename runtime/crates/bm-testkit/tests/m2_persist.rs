//! M2.1/M2.2 写穿与物化:回合事件落日志并物化进 SQLite 规范状态;
//! 重开持久层后,日志重放与状态位点自洽,行内容与内存终态一致。

use bm_contract::ids::IdGen;
use bm_contract::states::OperationState;
use bm_contract::wire::GetOperationParams;
use bm_persist::{EventStore, PersistStore};
use bm_providers::mock_model::Step;
use bm_testkit::invariants::assert_event_stream_wellformed;
use bm_testkit::replay::TestRig;

#[tokio::test]
async fn t18_write_through_materializes_canonical_state() {
    let rig = TestRig::standard(vec![Step::ok("幂等性是指……", 412, 58)]).await;
    let (sess, agent) = rig.create_session().await.expect("会话创建");
    let receipt = rig
        .send(&sess, &agent, "用一句话解释什么是幂等性")
        .await
        .expect("回合发起");

    // 等待终态
    let final_receipt = loop {
        let r = rig
            .handle
            .operations_get(GetOperationParams {
                operation_id: receipt.operation_id.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            break r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };
    assert_eq!(final_receipt.state, OperationState::Succeeded);

    // 内存事件流自洽
    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);
    let last_seq = events.last().expect("非空").event_seq;

    // 重开持久层(独立于运行中的内存视图):日志与位点自洽
    let dir = rig.data_dir.as_ref().expect("标准装配带落盘目录");
    let store = PersistStore::open(dir).expect("重开持久层");
    assert_eq!(
        EventStore::last_log_seq(&store).expect("日志末尾"),
        last_seq,
        "日志与内存事件流一致(写穿完整)"
    );
    assert_eq!(
        EventStore::last_applied_seq(&store).expect("位点"),
        last_seq,
        "状态位点追平日志(物化无遗漏)"
    );

    // 重放(自日志)条数一致
    let replayed = EventStore::replay_since(&store, 0).expect("重放");
    assert_eq!(replayed.len() as u64, last_seq);

    // 物化行:session active→? 未 close,仍是 active
    let sessions = store
        .state()
        .query_rows(
            "SELECT id, state, agent_id FROM sessions WHERE id=?1",
            &[&sess.as_str()],
        )
        .expect("查询");
    assert_eq!(sessions[0]["state"], "active");
    assert_eq!(sessions[0]["agent_id"], agent.as_str());

    // 物化行:operation 终态 succeeded
    let ops = store
        .state()
        .query_rows(
            "SELECT id, state, turn_index, completed_at FROM operations WHERE id=?1",
            &[&receipt.operation_id.as_str()],
        )
        .expect("查询");
    assert_eq!(ops[0]["state"], "succeeded");
    assert_eq!(ops[0]["turn_index"], 1);
    assert!(ops[0]["completed_at"].is_string(), "终态必有完成时间");

    // 物化行:预算账本自事件导出(412+58=470,1 回合)
    let agents = store
        .state()
        .query_rows(
            "SELECT budget_used_tokens, budget_turns_used, state FROM agents WHERE id=?1",
            &[&agent.as_str()],
        )
        .expect("查询");
    assert_eq!(agents[0]["budget_used_tokens"], 470);
    assert_eq!(agents[0]["budget_turns_used"], 1);
    assert_eq!(agents[0]["state"], "running");

    // 物化行:budget 上限自 agent.created 增补字段恢复(INV-7 跨重启前提)
    let limits = store
        .state()
        .query_rows(
            "SELECT budget_max_tokens, budget_max_turns FROM agents WHERE id=?1",
            &[&agent.as_str()],
        )
        .expect("查询");
    assert_eq!(limits[0]["budget_max_tokens"], 50_000);
    assert_eq!(limits[0]["budget_max_turns"], 10);

    rig.stop().await;
}

#[tokio::test]
async fn t19_write_through_survives_close_lifecycle() {
    // 会话关闭后重开:状态列= closed;二次 close 的拒绝语义不受持久化影响
    let rig = TestRig::standard(vec![Step::ok("答", 10, 5)]).await;
    let (sess, _agent) = rig.create_session().await.expect("会话创建");
    rig.handle
        .session_close(
            rig.ids.next_id("req"),
            bm_contract::wire::SessionCloseParams {
                session_id: sess.clone(),
                reason: Some("user_request".into()),
            },
        )
        .await
        .expect("close 成功");

    let dir = rig.data_dir.as_ref().expect("落盘目录");
    let store = PersistStore::open(dir).expect("重开");
    let sessions = store
        .state()
        .query_rows("SELECT state FROM sessions WHERE id=?1", &[&sess.as_str()])
        .expect("查询");
    assert_eq!(sessions[0]["state"], "closed");

    // 停机后 events_all 仍可读(只读残存态),且日志里 runtime.stopped 在场
    rig.handle.stop("test_done").await;
    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);
    let store2 = PersistStore::open(dir).expect("重开");
    assert_eq!(
        EventStore::last_log_seq(&store2).expect("日志末尾"),
        events.last().expect("非空").event_seq,
        "停机后日志追平全部事件"
    );
}
