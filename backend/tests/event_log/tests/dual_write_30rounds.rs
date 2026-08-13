//! 阶段 0 验收：模拟 30 轮对话经 DualWriter 双写
//! （与 bm-server chat.rs 完全相同的调用序列：回合开始 → 用户消息 →
//! 工具/助手/回合结束 batch），验证：
//! 1. 事件流重放两次字节一致；
//! 2. 消息面重建正确（30 轮 × user+assistant，assistant 挂工具调用）；
//! 3. 双写成败计数与事件数一致。

use std::sync::Arc;

use bm_kernel::{EventLog, InMemoryEventStore, SurfaceIntent};
use bm_protocol::{
    AssistantMsg, BranchId, CallId, CoreEvent, EventKind, SeqNo, SessionId, TokenUsage,
    ToolResultMsg, TurnEndReason, UserMsg, UserMsgSource,
};
use bm_storage_turso::dual_write::DualWriter;

#[tokio::test]
async fn thirty_rounds_dual_write_replay_identical() {
    let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
    let w = DualWriter::new(log);
    let sid = SessionId::new("sess_30rounds_dual");
    let bid = BranchId::new("main");

    for turn in 1..=30u32 {
        // 回合开始（chat.rs：TurnStart，best_effort）
        w.append_best_effort(
            sid.clone(),
            EventKind::Core(CoreEvent::TurnStart { turn }),
            SurfaceIntent::None,
        )
        .await;
        // 用户消息（chat.rs：UserMessage，best_effort）
        w.append_best_effort(
            sid.clone(),
            EventKind::Core(CoreEvent::UserMessage {
                msg: UserMsg {
                    content: format!("第 {turn} 轮问题"),
                },
                source: UserMsgSource::Human,
            }),
            SurfaceIntent::Append,
        )
        .await;
        // 回合收尾（chat.rs：工具/助手消息/回合结束 batch）
        let events: Vec<(EventKind, SurfaceIntent, bool, Option<Vec<SeqNo>>)> = vec![
            (
                EventKind::Core(CoreEvent::ToolCall {
                    turn,
                    step: 0,
                    call_id: CallId::new(format!("call_{turn}")),
                    name: "web_search".into(),
                    args: r#"{"q":"rust"}"#.into(),
                }),
                SurfaceIntent::None,
                false,
                None,
            ),
            (
                EventKind::Core(CoreEvent::ToolResult {
                    turn,
                    step: 0,
                    call_id: CallId::new(format!("call_{turn}")),
                    // 阶段 0 partial：输出内容暂不落日志（chat.rs 同）
                    result: ToolResultMsg {
                        ok: true,
                        output: String::new(),
                    },
                    meta: None,
                }),
                SurfaceIntent::None,
                false,
                None,
            ),
            (
                EventKind::Core(CoreEvent::AssistantMessage {
                    turn,
                    step: 1,
                    msg: AssistantMsg {
                        content: format!("第 {turn} 轮回答"),
                    },
                    usage: Some(TokenUsage {
                        input_tokens: 10 + turn as u64,
                        output_tokens: 20 + turn as u64,
                    }),
                }),
                SurfaceIntent::Append,
                false,
                None,
            ),
            (
                EventKind::Core(CoreEvent::TurnEnd {
                    turn,
                    reason: TurnEndReason::Completed,
                }),
                SurfaceIntent::None,
                false,
                None,
            ),
        ];
        w.append_batch(sid.clone(), events).await.unwrap();
    }

    // 1. 重放两次字节一致（确定性）
    let once = w.event_log().replay(&sid, &bid).await.unwrap();
    let twice = w.event_log().replay(&sid, &bid).await.unwrap();
    assert_eq!(
        serde_json::to_string(&once).unwrap(),
        serde_json::to_string(&twice).unwrap(),
        "30 轮双写事件流重放两次必须字节一致"
    );

    // 2. 消息面：30 轮 × (user + assistant) = 60 条，assistant 挂工具调用
    let msgs = w.event_log().derive_messages(&sid, &bid).await.unwrap();
    assert_eq!(msgs.len(), 60, "每轮 user + assistant 各一条");
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].content, "第 1 轮问题");
    assert_eq!(msgs[1].role, "assistant");
    assert_eq!(msgs[1].tool_calls.len(), 1, "assistant 消息挂 1 个工具调用");
    assert_eq!(msgs[1].tool_calls[0].call_id, "call_1");
    assert!(msgs[1].tool_calls[0].result.as_ref().unwrap().ok);
    assert_eq!(msgs[59].content, "第 30 轮回答");

    // 3. 双写计数 = 事件总数（30 轮 × 6 事件 = 180）
    assert_eq!(w.ok_count(), once.len() as u64);
    assert_eq!(once.len(), 180);
    assert_eq!(w.failed_count(), 0);
}

#[tokio::test]
async fn dual_write_turn_numbers_are_consecutive() {
    // 回合号推断逻辑（chat.rs 用 TurnStart 计数 + 1）：模拟多次追加后
    // 下一次回合号正确递增
    let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
    let w = DualWriter::new(log);
    let sid = SessionId::new("sess_turns");
    let bid = BranchId::new("main");

    let count_turns = |evs: &[bm_protocol::SessionEvent]| -> u32 {
        evs.iter()
            .filter(|e| matches!(&e.kind, EventKind::Core(CoreEvent::TurnStart { .. })))
            .count() as u32
    };

    for expected in 1..=5u32 {
        // 与 chat.rs 相同的回合号推断
        let evs = w.event_log().replay(&sid, &bid).await.unwrap();
        let turn = count_turns(&evs) + 1;
        assert_eq!(turn, expected);
        w.append_best_effort(
            sid.clone(),
            EventKind::Core(CoreEvent::TurnStart { turn }),
            SurfaceIntent::None,
        )
        .await;
        w.append_best_effort(
            sid.clone(),
            EventKind::Core(CoreEvent::TurnEnd {
                turn,
                reason: TurnEndReason::Completed,
            }),
            SurfaceIntent::None,
        )
        .await;
    }
}
