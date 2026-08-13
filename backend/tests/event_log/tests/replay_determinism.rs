//! 确定性重放测试（Life Agent OS 验证过的姿势）：
//! 30 轮模拟对话事件流，重放两次字节一致；内存后端与 turso 后端
//! 重建的消息面语义一致。

use std::sync::Arc;

use bm_kernel::{EventLog, InMemoryEventStore, SurfaceIntent};
use bm_protocol::{
    AssistantMsg, BranchId, CallId, CoreEvent, EventKind, SessionEvent, SessionId, SurfaceOp,
    ToolResultMsg, TurnEndReason, UserMsg, UserMsgSource,
};

/// 构造 30 轮对话事件流（含流式块、工具调用、两次压缩、记忆写入）。
fn build_30_turn_stream() -> Vec<(EventKind, SurfaceIntent)> {
    let mut out = Vec::new();
    for turn in 1..=30u32 {
        out.push((
            EventKind::Core(CoreEvent::TurnStart { turn }),
            SurfaceIntent::None,
        ));
        out.push((
            EventKind::Core(CoreEvent::UserMessage {
                msg: UserMsg {
                    content: format!("第 {turn} 轮问题"),
                },
                source: UserMsgSource::Human,
            }),
            SurfaceIntent::Append,
        ));
        // 两个 chunk 合并成一条助手消息
        out.push((
            EventKind::Core(CoreEvent::AssistantChunk {
                turn,
                step: 1,
                chunk: bm_protocol::StreamChunk {
                    text: format!("第 {turn} 轮回答（前半）"),
                },
            }),
            SurfaceIntent::Append,
        ));
        out.push((
            EventKind::Core(CoreEvent::AssistantChunk {
                turn,
                step: 1,
                chunk: bm_protocol::StreamChunk {
                    text: "，后半部分".into(),
                },
            }),
            SurfaceIntent::Append,
        ));
        // 工具调用 + 结果
        out.push((
            EventKind::Core(CoreEvent::ToolCall {
                turn,
                step: 2,
                call_id: CallId::new(format!("call_{turn}")),
                name: "web_search".into(),
                args: format!(r#"{{"q":"问题{turn}"}}"#),
            }),
            SurfaceIntent::None,
        ));
        out.push((
            EventKind::Core(CoreEvent::ToolResult {
                turn,
                step: 2,
                call_id: CallId::new(format!("call_{turn}")),
                result: ToolResultMsg {
                    ok: turn % 3 != 0,
                    output: format!("搜索结果{turn}"),
                },
                meta: None,
            }),
            SurfaceIntent::None,
        ));
        out.push((
            EventKind::Core(CoreEvent::AssistantMessage {
                turn,
                step: 3,
                msg: AssistantMsg {
                    content: format!("第 {turn} 轮完整回答"),
                },
                usage: Some(bm_protocol::TokenUsage {
                    input_tokens: 100 + turn as u64,
                    output_tokens: 50 + turn as u64,
                }),
            }),
            SurfaceIntent::Append,
        ));
        out.push((
            EventKind::Core(CoreEvent::TurnEnd {
                turn,
                reason: TurnEndReason::Completed,
            }),
            SurfaceIntent::None,
        ));
        // 每 10 轮一次压缩（Replace 遮蔽前 5 轮区间）
        if turn == 10 || turn == 20 {
            out.push((
                EventKind::Core(CoreEvent::CompactionSummary {
                    msg: bm_protocol::CompactionSummaryMsg {
                        removed_start: 1,
                        removed_end: (turn * 8) as u64,
                        summary: format!("前 {turn} 轮内容摘要"),
                    },
                }),
                SurfaceIntent::None,
            ));
        }
    }
    out
}

fn snapshot(evs: &[SessionEvent]) -> String {
    // 字节级快照：完整信封 JSON（含 seq/time/kind 全字段）
    serde_json::to_string(evs).expect("events serializable")
}

#[tokio::test]
async fn replay_twice_is_byte_identical_memory() {
    let store = Arc::new(InMemoryEventStore::new());
    let log = EventLog::new(store);
    let sid = SessionId::new("sess_30rounds");
    let bid = BranchId::new("main");
    let stream = build_30_turn_stream();
    let batch = stream
        .into_iter()
        .map(|(kind, surface)| (kind, surface, false, None))
        .collect();
    let seqs = log.append_batch(sid.clone(), bid.clone(), batch).await.unwrap();
    // 每轮 8 事件 × 30 轮 + 2 次压缩 = 242
    assert_eq!(seqs.len(), 242);

    let once = log.replay(&sid, &bid).await.unwrap();
    let twice = log.replay(&sid, &bid).await.unwrap();
    assert_eq!(snapshot(&once), snapshot(&twice), "replay twice must be byte-identical");

    // 消息面重建两次一致
    let m1 = log.derive_messages(&sid, &bid).await.unwrap();
    let m2 = log.derive_messages(&sid, &bid).await.unwrap();
    assert_eq!(
        serde_json::to_string(&m1).unwrap(),
        serde_json::to_string(&m2).unwrap()
    );
}

#[tokio::test]
async fn memory_and_turso_agree_on_surface() {
    let path = format!(
        "{}/bm_replay_turso_{}.db",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&path);
    let turso_store = Arc::new(bm_storage_turso::TursoEventStore::open(&path).await.unwrap());
    let mem_log = EventLog::new(Arc::new(InMemoryEventStore::new()));
    let turso_log = EventLog::new(turso_store);
    let sid = SessionId::new("sess_cross");
    let bid = BranchId::new("main");

    let stream = build_30_turn_stream();
    let batch: Vec<_> = stream
        .into_iter()
        .map(|(kind, surface)| (kind, surface, false, None))
        .collect();
    let mem_seqs = mem_log.append_batch(sid.clone(), bid.clone(), batch.clone()).await.unwrap();
    let turso_seqs = turso_log.append_batch(sid.clone(), bid.clone(), batch).await.unwrap();
    assert_eq!(mem_seqs, turso_seqs, "both backends assign same seqs");

    let mem_msgs = mem_log.derive_messages(&sid, &bid).await.unwrap();
    let turso_msgs = turso_log.derive_messages(&sid, &bid).await.unwrap();
    assert_eq!(
        serde_json::to_string(&mem_msgs).unwrap(),
        serde_json::to_string(&turso_msgs).unwrap(),
        "memory and turso must rebuild identical message surface"
    );
    assert!(mem_msgs.len() > 30, "surface should contain all turns");

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn replace_op_masks_and_summary_visible() {
    // 单测已覆盖投影；这里验证整条链路：压缩事件通过 EventLog 追加后
    // 消息面正确重建（压缩后只余摘要消息）
    let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
    let sid = SessionId::new("sess_mask");
    let bid = BranchId::new("main");
    log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::UserMessage {
        msg: UserMsg { content: "a".into() },
        source: UserMsgSource::Human,
    }), SurfaceIntent::Append).await.unwrap();
    log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::AssistantMessage {
        turn: 1, step: 1, msg: AssistantMsg { content: "b".into() }, usage: None,
    }), SurfaceIntent::Append).await.unwrap();
    log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::CompactionSummary {
        msg: bm_protocol::CompactionSummaryMsg {
            removed_start: 1,
            removed_end: 2,
            summary: "ab 摘要".into(),
        },
    }), SurfaceIntent::Replace { start: 1, end: 2 }).await.unwrap();

    let msgs = log.derive_messages(&sid, &bid).await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].content.contains("ab 摘要"));
    // 遮蔽事件本身带 Replace op（surface_op 语义随事件落库）
    let evs = log.replay(&sid, &bid).await.unwrap();
    assert_eq!(evs[2].surface_op, Some(SurfaceOp::Replace { start: 1, end: 2 }));
}
