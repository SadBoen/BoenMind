//! 压缩事务协议（核心件）+ 压缩策略接口（插件面）。
//!
//! **骨架/手脚分工（v0.17 用户定调）**：本模块只承载"事务协议"——三事件
//! 落盘（CompactionStart → CompactionSummary{Replace 遮蔽} → CompactionEnd）
//! 与 fail-safe（摘要失败不遮蔽，宁可超窗失败回合也不静默丢历史）。策略
//! （水线/尾部保留/摘要 prompt/要不要压）是压缩插件的自治面：
//! [`Compactor`] 契约，bm-compactor 为默认实现（D10），可换实现、可关闭。
//! **无压缩插件 = 优雅失败**：软触发永不动作，硬触发（输入超窗）直接失败
//! 回合——不崩、不静默丢历史；框架的重点不是裸跑，是装上插件后跑得好。
//!
//! 事务（[`compact`]）：选可压缩区间（尾部按策略保留预算保留）→ 摘要
//! （prompt 由策略构造，无工具、同模型）→ 一次 append_batch 落三事件。
//! 投影层对 Replace 遮蔽旧消息、摘要以 assistant 消息入消息面
//! （bm-kernel projection）。

use bm_kernel::{EventLog, SurfaceIntent};
use bm_protocol::{
    BranchId, CompactionSummaryMsg, CoreEvent, EventKind, ProtocolError, SessionId,
};

use crate::llm::{Llm, LlmError, LlmEvent, LlmRequest};

/// 压缩策略（插件面契约）：loop 步边界只问两件事——"压不压"与"怎么压"。
/// 事务落盘由 loop 执行（协议是核心的）。None（关闭插件）= 软触发永不动作。
/// Debug：LoopConfig 派生需要（实现侧一并 derive 即可）。
pub trait Compactor: Send + Sync + std::fmt::Debug {
    /// 软触发判定：占用是否达到该策略的水线（阈值插件自治；
    /// `context_window` 是模型客观属性，由 loop 传入）。
    fn should_compact(&self, total_tokens: u64, context_window: u64) -> bool;
    /// 尾部保留预算（token）：从后往前累积保留，其余为可压缩中部。
    fn keep_recent_tokens(&self, context_window: u64) -> u64;
    /// 中部不足多少 token 不值得压。
    fn min_middle_tokens(&self) -> u64;
    /// 摘要请求构造（摘要 prompt 插件自治）。
    fn summarize_request(&self, model: &str, dialogue: &str) -> LlmRequest;
}

/// 文本 token 粗估：chars/4（中英混合近似；有 usage 时 run 循环以
/// usage 累计加权修正，见 engine.rs）。
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

/// 压缩事务：投影 → 选区间（保留预算来自策略）→ 摘要（prompt 来自策略）
/// → 三事件事务。
///
/// 返回 Ok(true) = 已压缩；Ok(false) = 无需压缩（中部不足）或摘要失败
/// （fail-safe，已 warn）。Err 仅来自日志读写本身（调用方按日志失败处理）。
#[allow(clippy::too_many_arguments)] // 参数全是事务协议语义（log/llm/定位/模型/窗口/策略），分组反而啰嗦
pub async fn compact<L: Llm, C: Compactor + ?Sized>(
    log: &EventLog,
    llm: &L,
    sid: &SessionId,
    bid: &BranchId,
    turn: u32,
    model: &str,
    context_window: u64,
    policy: &C,
) -> Result<bool, ProtocolError> {
    let msgs = log.derive_messages(sid, bid).await?;
    let keep = policy.keep_recent_tokens(context_window);

    // 尾部保留：从后往前累积到 keep 预算，其余为可压缩中部
    let mut tail_start = msgs.len();
    let mut acc = 0u64;
    for (i, m) in msgs.iter().enumerate().rev() {
        if acc >= keep {
            tail_start = i + 1;
            break;
        }
        acc += estimate_tokens(&m.content);
    }
    let middle = &msgs[..tail_start];
    // 中部不足两条（或内容过少）不值得压缩
    let middle_tokens: u64 = middle
        .iter()
        .map(|m| estimate_tokens(&m.content))
        .sum();
    if middle.len() < 2 || middle_tokens < policy.min_middle_tokens() {
        return Ok(false);
    }

    // removed 区间 = 中部消息的 seq 范围（投影消息 seq 即事件 seq）
    let removed_start = middle.first().map(|m| m.seq).unwrap_or(0);
    let removed_end = middle.last().map(|m| m.seq).unwrap_or(0);
    if removed_start == 0 || removed_end < removed_start {
        return Ok(false);
    }

    // 摘要（无工具、同模型；流结束即 MessageEnd）
    let dialogue: String = middle
        .iter()
        .map(|m| format!("[{}] {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");
    let summary = match summarize(llm, policy.summarize_request(model, &dialogue)).await {
        Ok(s) if !s.trim().is_empty() => s,
        Ok(_) | Err(_) => {
            tracing::warn!(
                event = "bm.loop_compact_summary_failed",
                removed_start,
                removed_end,
                "摘要失败：不遮蔽（fail-safe，历史保留）"
            );
            return Ok(false);
        }
    };

    // 三事件事务：Start → Summary（Replace 遮蔽中部）→ End
    log.append_batch(
        sid.clone(),
        bid.clone(),
        vec![
            (
                EventKind::Core(CoreEvent::CompactionStart { turn }),
                SurfaceIntent::None,
                false,
                None,
            ),
            (
                EventKind::Core(CoreEvent::CompactionSummary {
                    msg: CompactionSummaryMsg {
                        removed_start,
                        removed_end,
                        summary,
                    },
                }),
                SurfaceIntent::Replace {
                    start: removed_start,
                    end: removed_end,
                },
                false,
                None,
            ),
            (
                EventKind::Core(CoreEvent::CompactionEnd { turn }),
                SurfaceIntent::None,
                false,
                None,
            ),
        ],
    )
    .await?;
    tracing::info!(
        event = "bm.loop_compacted",
        removed_start,
        removed_end,
        turn,
        "压缩事务已落"
    );
    Ok(true)
}

/// 摘要辅助：流消费到 MessageEnd。
async fn summarize<L: Llm>(llm: &L, req: LlmRequest) -> Result<String, LlmError> {
    use tokio_stream::StreamExt;
    let stream = llm.stream_chat(req);
    tokio::pin!(stream);
    let mut text = String::new();
    while let Some(ev) = stream.next().await {
        match ev? {
            LlmEvent::TextDelta { text: t } => text.push_str(&t),
            LlmEvent::MessageEnd { content, .. } => {
                if !content.is_empty() {
                    text = content;
                }
                break;
            }
            _ => {}
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_kernel::InMemoryEventStore;
    use bm_protocol::{AssistantMsg, UserMsg, UserMsgSource};
    use std::sync::Arc;
    use tokio_stream::wrappers::UnboundedReceiverStream;

    #[test]
    fn estimate_tokens_chars_over_four() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("你好世界"), 1, "中文 4 字 ≈ 1 token 粗估");
    }

    /// 内联测试策略（不依赖任何插件 crate——核心自足性：loop 的压缩
    /// 事务协议测试用内联实现即可跑）。
    #[derive(Debug)]
    struct TestPolicy;
    impl Compactor for TestPolicy {
        fn should_compact(&self, _total: u64, _window: u64) -> bool {
            true
        }
        fn keep_recent_tokens(&self, _window: u64) -> u64 {
            10
        }
        fn min_middle_tokens(&self) -> u64 {
            0
        }
        fn summarize_request(&self, model: &str, dialogue: &str) -> LlmRequest {
            LlmRequest {
                payload: serde_json::json!({
                    "model": model,
                    "messages": [{"role": "user", "content": format!("请总结：{dialogue}")}],
                }),
            }
        }
    }

    #[tokio::test]
    async fn compact_writes_three_event_transaction_and_masks() {
        let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
        let sid = SessionId::new("sess_c");
        let bid = BranchId::new("main");
        // 铺垫 6 条消息：前 4 条长历史（各 ~700 字 ≈ 175 token，seq 1..=4），
        // 尾部 2 条短消息（seq 5..=6）
        let long = "很长的历史消息".repeat(100);
        let contents = [
            long.clone(),
            long.clone(),
            long.clone(),
            long.clone(),
            "继续".to_string(),
            "接近完成".to_string(),
        ];
        for (i, content) in contents.iter().enumerate() {
            let kind = if i % 2 == 0 {
                EventKind::Core(CoreEvent::UserMessage {
                    msg: UserMsg { content: content.clone() },
                    source: UserMsgSource::Human,
                })
            } else {
                EventKind::Core(CoreEvent::AssistantMessage {
                    turn: 1,
                    step: (i as u32 + 1).div_ceil(2),
                    msg: AssistantMsg { content: content.clone() },
                    usage: None,
                })
            };
            log.append(sid.clone(), bid.clone(), kind, SurfaceIntent::Append)
                .await
                .unwrap();
        }

        // 测试策略：尾部保留 10 token（~2 条短消息）→ 中部 = 前 4 条长消息
        let policy = TestPolicy;
        // 摘要 LLM：直接吐一条文本
        struct FixedLlm;
        impl Llm for FixedLlm {
            fn stream_chat(
                &self,
                _req: LlmRequest,
            ) -> impl tokio_stream::Stream<Item = Result<LlmEvent, LlmError>> + Send {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                tx.send(Ok(LlmEvent::TextDelta { text: "摘要：用户想要 hello world".into() }))
                    .unwrap();
                tx.send(Ok(LlmEvent::MessageEnd {
                    content: "摘要：用户想要 hello world".into(),
                    tool_calls: Vec::new(),
                    usage: None,
                }))
                .unwrap();
                UnboundedReceiverStream::new(rx)
            }
        }

        let done = compact(&log, &FixedLlm, &sid, &bid, 1, "test-model", 1000, &policy)
            .await
            .unwrap();
        assert!(done, "应执行压缩");

        // 事件序列：… + compaction/start + compaction/summary + compaction/end
        let evs = log.replay(&sid, &bid).await.unwrap();
        let types: Vec<&str> = evs
            .iter()
            .skip(6)
            .map(|e| match &e.kind {
                EventKind::Core(c) => bm_protocol::core_type_name(c),
                _ => "",
            })
            .collect();
        assert_eq!(types, vec!["compaction/start", "compaction/summary", "compaction/end"]);

        // 投影遮蔽：中部 3 条（1..3）移除；跨阈值的第 4 条长消息留在尾部，
        // 尾部 3 条 + 摘要可见
        let msgs = log.derive_messages(&sid, &bid).await.unwrap();
        assert_eq!(msgs.len(), 4, "尾部 3 条 + 摘要，实际 {msgs:?}");
        assert!(msgs.iter().any(|m| m.content.contains("摘要")));
        assert!(msgs.iter().any(|m| m.seq == 4), "跨阈值消息留在尾部（先检查后累加）");
    }
}
