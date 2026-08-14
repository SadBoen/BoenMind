//! 自研压缩引擎（A6 主体第二件）：双触发 + 摘要事务。
//!
//! 双触发（handoff §四 A6）：
//! - **软触发**：步边界检查，上下文占用 ≥ 窗口 × [`CompactionPolicy::watermark`]
//!   （默认 0.8，比 pi 引擎的 0.5 更激进——30 轮 A/B 对比方法论验收后再定稿）；
//! - **硬触发**：单步输入本身 ≥ 窗口（请求发出前检查，压缩后仍超窗即回合失败）。
//!
//! 事务（[`compact`]）：选可压缩区间（尾部按 keep_recent 保留）→ 调 LLM 摘要
//! （无工具、同模型）→ 一次 append_batch 落 `CompactionStart → CompactionSummary
//! {removed 区间 + 摘要, surface_op = Replace} → CompactionEnd`。投影层对
//! Replace 遮蔽旧消息、摘要以 assistant 消息入消息面（bm-kernel projection）。
//!
//! **fail-safe**：摘要失败不遮蔽（宁可超窗由硬触发失败回合，也不静默丢历史）——
//! 压缩是优化不是正确性依赖，模型可见历史只增不减才满足"模型可见即已记录"。

use bm_kernel::{EventLog, SurfaceIntent};
use bm_protocol::{
    BranchId, CompactionSummaryMsg, CoreEvent, EventKind, ProtocolError, SessionId,
};

use crate::llm::{Llm, LlmError, LlmEvent, LlmRequest};

/// 压缩策略（构造值由集成方从 bm-core compaction 配置换算注入）。
#[derive(Debug, Clone, PartialEq)]
pub struct CompactionPolicy {
    /// 总开关；false = 双触发全部不动作
    pub enabled: bool,
    /// 模型上下文窗口（token）
    pub context_window: u32,
    /// 软水线（0.0 ~ 1.0，占用窗口比例）
    pub watermark: f64,
    /// 尾部保留比例（占窗口比例）
    pub keep_recent_ratio: f64,
    /// 尾部保留 token 下限
    pub keep_recent_floor: u32,
}

impl Default for CompactionPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            context_window: 128_000,
            watermark: 0.8,
            keep_recent_ratio: 0.10,
            keep_recent_floor: 4_000,
        }
    }
}

/// 触发判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionTrigger {
    /// 不触发
    None,
    /// 软触发：占用过水线，可压缩后继续
    Soft,
    /// 硬触发：单步输入超窗口，压缩后仍超窗则回合失败
    Overflow,
}

impl CompactionPolicy {
    /// 文本 token 粗估：chars/4（中英混合近似；有 usage 时 run 循环以
    /// usage 累计加权修正，见 engine.rs）。
    pub fn estimate_tokens(text: &str) -> u64 {
        (text.chars().count() as u64).div_ceil(4)
    }

    /// 双触发判定。
    pub fn check(&self, total_tokens: u64) -> CompactionTrigger {
        if !self.enabled {
            return CompactionTrigger::None;
        }
        let window = self.context_window as u64;
        if total_tokens >= window {
            return CompactionTrigger::Overflow;
        }
        let soft = (window as f64 * self.watermark) as u64;
        if total_tokens >= soft.max(1) {
            return CompactionTrigger::Soft;
        }
        CompactionTrigger::None
    }

    /// 尾部保留预算（token）：max(窗口 × ratio, floor)。
    fn keep_recent_tokens(&self) -> u64 {
        let ratio = (self.context_window as f64 * self.keep_recent_ratio) as u64;
        ratio.max(self.keep_recent_floor as u64)
    }
}

/// 压缩摘要的 LLM 请求（prompt 注入到 payload.messages 末尾）。
fn summary_payload(model: &str, dialogue: &str) -> LlmRequest {
    let prompt = format!(
        "请总结以下对话历史（保留用户意图、关键事实、已完成的工具操作与结论；\
         用与原文相同的语言，控制在 300 字内）：\n\n{dialogue}"
    );
    LlmRequest {
        payload: serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.3,
        }),
    }
}

/// 压缩引擎：投影 → 选区间 → 摘要 → 三事件事务。
///
/// 返回 Ok(true) = 已压缩；Ok(false) = 无需压缩（中部不足）或摘要失败
/// （fail-safe，已 warn）。Err 仅来自日志读写本身（调用方按日志失败处理）。
pub async fn compact<L: Llm>(
    log: &EventLog,
    llm: &L,
    sid: &SessionId,
    bid: &BranchId,
    turn: u32,
    model: &str,
    policy: &CompactionPolicy,
) -> Result<bool, ProtocolError> {
    if !policy.enabled {
        return Ok(false);
    }
    let msgs = log.derive_messages(sid, bid).await?;
    let keep = policy.keep_recent_tokens();

    // 尾部保留：从后往前累积到 keep 预算，其余为可压缩中部
    let mut tail_start = msgs.len();
    let mut acc = 0u64;
    for (i, m) in msgs.iter().enumerate().rev() {
        if acc >= keep {
            tail_start = i + 1;
            break;
        }
        acc += CompactionPolicy::estimate_tokens(&m.content);
    }
    let middle = &msgs[..tail_start];
    // 中部不足两条（或内容过少）不值得压缩
    let middle_tokens: u64 = middle
        .iter()
        .map(|m| CompactionPolicy::estimate_tokens(&m.content))
        .sum();
    if middle.len() < 2 || middle_tokens < 512 {
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
    let summary = match summarize(llm, summary_payload(model, &dialogue)).await {
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
    fn trigger_thresholds() {
        let p = CompactionPolicy {
            enabled: true,
            context_window: 100,
            watermark: 0.8,
            keep_recent_ratio: 0.1,
            keep_recent_floor: 10,
        };
        assert_eq!(p.check(0), CompactionTrigger::None);
        assert_eq!(p.check(79), CompactionTrigger::None);
        assert_eq!(p.check(80), CompactionTrigger::Soft, "80/100 = 0.8 水线");
        assert_eq!(p.check(100), CompactionTrigger::Overflow, "满窗硬触发");
        let off = CompactionPolicy { enabled: false, ..p };
        assert_eq!(off.check(1000), CompactionTrigger::None);
    }

    #[test]
    fn estimate_tokens_chars_over_four() {
        assert_eq!(CompactionPolicy::estimate_tokens(""), 0);
        assert_eq!(CompactionPolicy::estimate_tokens("abcd"), 1);
        assert_eq!(CompactionPolicy::estimate_tokens("abcde"), 2);
        assert_eq!(CompactionPolicy::estimate_tokens("你好世界"), 1, "中文 4 字 ≈ 1 token 粗估");
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

        // 极小策略：尾部保留 ~2 条短消息（floor 10 token）→ 中部 = 前 4 条长消息
        let policy = CompactionPolicy {
            enabled: true,
            context_window: 1000,
            watermark: 0.8,
            keep_recent_ratio: 0.0,
            keep_recent_floor: 10,
        };
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

        let done = compact(&log, &FixedLlm, &sid, &bid, 1, "test-model", &policy)
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
