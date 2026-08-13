//! 投影：状态恢复的 canonical 姿势（A10）。
//!
//! 不变量（实现方案 §5-6）：任何投影必须能通过 replay 重建，禁止
//! 依赖投影外状态。重放两次字节一致 = 同一事件流折叠结果逐字节相同
//! （Life Agent OS 验证过的姿势）。

use bm_protocol::{
    CoreEvent, ErrorCode, ProtocolError, SessionEvent, SurfaceOp, ToolResultMsg,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// 消息面投影 trait：逐个折叠 + 可序列化快照。
pub trait Projection: Send + Sync {
    fn on_event(&mut self, ev: &SessionEvent) -> Result<(), ProtocolError>;
    /// 可序列化快照（加速恢复 / checkpoint 用）
    fn checkpoint(&self) -> JsonValue;
}

/// 消息面工具调用（ToolCall/ToolResult 折叠结果）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceToolCall {
    pub call_id: String,
    pub name: String,
    pub args: String,
    pub result: Option<ToolResultMsg>,
}

/// 消息面消息（user/assistant/tool 排序视图）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceMessage {
    /// 产生该消息的事件 seq（Replace 遮蔽区间判定用）
    pub seq: u64,
    pub role: String,
    pub content: String,
    pub tool_calls: Vec<SurfaceToolCall>,
    /// 消息所属回合/步骤：chunk→message 归并依据（同 (turn, step) 才合并，
    /// 防止相邻回合/步骤的流式块串味）。0 = 无归属（如压缩摘要消息）。
    #[serde(default)]
    pub turn: u32,
    #[serde(default)]
    pub step: u32,
}

/// 消息面投影：从事件流折叠出用户/助手/工具消息。
pub struct SurfaceProjection {
    messages: Vec<SurfaceMessage>,
}

impl Default for SurfaceProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl SurfaceProjection {
    pub fn new() -> Self {
        Self { messages: Vec::new() }
    }

    pub fn into_messages(self) -> Vec<SurfaceMessage> {
        self.messages
    }

    pub fn messages(&self) -> &[SurfaceMessage] {
        &self.messages
    }

    /// 遮蔽 seq ∈ [start, end] 的贡献（压缩后旧消息移除）。
    fn mask_interval(&mut self, start: u64, end: u64) {
        self.messages.retain(|m| m.seq < start || m.seq > end);
    }

    /// 追加内容到当前 (turn, step) 的 assistant 消息（chunk 合并）。
    /// 不同回合/步骤的 chunk 新建消息——防止相邻步的流式块串味。
    fn append_to_last_assistant(&mut self, seq: u64, turn: u32, step: u32, text: &str) {
        if let Some(last) = self
            .messages
            .last_mut()
            .filter(|m| m.role == "assistant" && m.turn == turn && m.step == step)
        {
            last.content.push_str(text);
            return;
        }
        self.messages.push(SurfaceMessage {
            seq,
            role: "assistant".into(),
            content: text.to_string(),
            tool_calls: Vec::new(),
            turn,
            step,
        });
    }

    fn attach_tool_call(&mut self, seq: u64, turn: u32, step: u32, call: SurfaceToolCall) {
        if let Some(last) = self.messages.last_mut().filter(|m| m.role == "assistant") {
            last.tool_calls.push(call);
            return;
        }
        // 无助手消息挂靠：先建一个空的（内容由后续 chunk/message 补）
        self.messages.push(SurfaceMessage {
            seq,
            role: "assistant".into(),
            content: String::new(),
            tool_calls: vec![call],
            turn,
            step,
        });
    }

    fn attach_tool_result(&mut self, call_id: &str, result: ToolResultMsg) {        // 从后往前找：最近的 assistant 消息中的匹配 call_id
        for m in self.messages.iter_mut().rev() {
            if let Some(tc) = m.tool_calls.iter_mut().find(|tc| tc.call_id == call_id) {
                tc.result = Some(result);
                return;
            }
        }
    }
}

impl Projection for SurfaceProjection {
    fn on_event(&mut self, ev: &SessionEvent) -> Result<(), ProtocolError> {
        // 先处理消息面操作：Replace 遮蔽区间（压缩语义）
        if let Some(SurfaceOp::Replace { start, end }) = ev.surface_op {
            if start > end {
                return Err(ProtocolError::new(
                    ErrorCode::SurfaceViolation,
                    format!("replace interval {start}..{end} inverted"),
                ));
            }
            self.mask_interval(start, end);
        }

        match &ev.kind {
            bm_protocol::EventKind::Core(core) => match core {
                CoreEvent::UserMessage { msg, .. } => {
                    self.messages.push(SurfaceMessage {
                        seq: ev.seq.as_u64(),
                        role: "user".into(),
                        content: msg.content.clone(),
                        tool_calls: Vec::new(),
                        turn: 0,
                        step: 0,
                    });
                }
                CoreEvent::AssistantMessage { turn, step, msg, .. } => {
                    // 真序事件（A1）下每个 step 先有 chunk 后有 message：
                    // 同 (turn, step) 的 assistant 消息是 chunk 拼的草稿，
                    // message 是权威内容 → 原地覆写，不新建（防重复）。
                    // 兼容旧路径：工具占位消息（无内容仅挂工具调用）填充；
                    // 两者都不是才新建消息。
                    let merge = self.messages.last_mut().is_some_and(|last| {
                        last.role == "assistant"
                            && ((last.turn == *turn && last.step == *step)
                                || (last.content.is_empty() && !last.tool_calls.is_empty()))
                    });
                    if merge {
                        let last = self.messages.last_mut().expect("checked above");
                        last.content = msg.content.clone();
                    } else {
                        self.messages.push(SurfaceMessage {
                            seq: ev.seq.as_u64(),
                            role: "assistant".into(),
                            content: msg.content.clone(),
                            tool_calls: Vec::new(),
                            turn: *turn,
                            step: *step,
                        });
                    }
                }
                CoreEvent::AssistantChunk { turn, step, chunk, .. } => {
                    self.append_to_last_assistant(ev.seq.as_u64(), *turn, *step, &chunk.text);
                }
                CoreEvent::ToolCall { turn, step, call_id, name, args, .. } => {
                    self.attach_tool_call(
                        ev.seq.as_u64(),
                        *turn,
                        *step,
                        SurfaceToolCall {
                            call_id: call_id.to_string(),
                            name: name.clone(),
                            args: args.clone(),
                            result: None,
                        },
                    );
                }
                CoreEvent::ToolResult { call_id, result, .. } => {
                    self.attach_tool_result(call_id.as_str(), result.clone());
                }
                CoreEvent::CompactionSummary { msg } => {
                    // 压缩摘要：遮蔽区间 + 追加摘要消息（消息面可见）
                    self.mask_interval(msg.removed_start, msg.removed_end);
                    self.messages.push(SurfaceMessage {
                        seq: ev.seq.as_u64(),
                        role: "assistant".into(),
                        content: format!("[已压缩 {}..{}] {}", msg.removed_start, msg.removed_end, msg.summary),
                        tool_calls: Vec::new(),
                        turn: 0,
                        step: 0,
                    });
                }
                _ => {}
            },
            // 插件域事件不参与消息面（Custom 是应用层自有视图）
            bm_protocol::EventKind::Custom(_) => {}
        }
        Ok(())
    }

    fn checkpoint(&self) -> JsonValue {
        serde_json::to_value(&self.messages).unwrap_or(JsonValue::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_protocol::{BranchId, CallId, EventKind, SeqNo, SessionId};

    fn env(seq: u64, kind: EventKind) -> SessionEvent {
        SessionEvent {
            version: bm_protocol::SESSION_FORMAT_VERSION,
            seq: SeqNo::new(seq),
            session_id: SessionId::new("sess_p"),
            branch_id: BranchId::new("main"),
            time: 1,
            kind,
            ignorable: false,
            surface_op: None,
            source_seqs: None,
        }
    }

    fn fold(evs: Vec<SessionEvent>) -> Vec<SurfaceMessage> {
        let mut p = SurfaceProjection::new();
        for ev in evs {
            p.on_event(&ev).unwrap();
        }
        p.into_messages()
    }

    #[test]
    fn user_assistant_ordering() {
        let evs = vec![
            env(1, EventKind::Core(CoreEvent::UserMessage {
                msg: bm_protocol::UserMsg { content: "hi".into() },
                source: bm_protocol::UserMsgSource::Human,
            })),
            env(2, EventKind::Core(CoreEvent::AssistantMessage {
                turn: 1,
                step: 1,
                msg: bm_protocol::AssistantMsg { content: "hello".into() },
                usage: None,
            })),
        ];
        let msgs = fold(evs);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hi");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "hello");
    }

    #[test]
    fn chunks_merge_into_assistant_message() {
        let evs = vec![
            env(1, EventKind::Core(CoreEvent::UserMessage {
                msg: bm_protocol::UserMsg { content: "q".into() },
                source: bm_protocol::UserMsgSource::Human,
            })),
            env(2, EventKind::Core(CoreEvent::AssistantChunk {
                turn: 1,
                step: 1,
                chunk: bm_protocol::StreamChunk { text: "你".into() },
            })),
            env(3, EventKind::Core(CoreEvent::AssistantChunk {
                turn: 1,
                step: 1,
                chunk: bm_protocol::StreamChunk { text: "好".into() },
            })),
        ];
        let msgs = fold(evs);
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[1].content, "你好");
    }

    #[test]
    fn chunk_then_message_same_step_merges_no_duplicate() {
        // A1 真序：chunk 拼草稿 → AssistantMessage 权威覆写（不产生第二条消息）
        let evs = vec![
            env(1, EventKind::Core(CoreEvent::UserMessage {
                msg: bm_protocol::UserMsg { content: "q".into() },
                source: bm_protocol::UserMsgSource::Human,
            })),
            env(2, EventKind::Core(CoreEvent::AssistantChunk {
                turn: 1,
                step: 1,
                chunk: bm_protocol::StreamChunk { text: "草稿".into() },
            })),
            env(3, EventKind::Core(CoreEvent::AssistantMessage {
                turn: 1,
                step: 1,
                msg: bm_protocol::AssistantMsg { content: "权威内容".into() },
                usage: None,
            })),
        ];
        let msgs = fold(evs);
        assert_eq!(msgs.len(), 2, "chunk+message 不应产生重复消息");
        assert_eq!(msgs[1].content, "权威内容");
    }

    #[test]
    fn cross_step_chunks_do_not_blend() {
        // 相邻 step 的 chunk 不得串味；无 user 消息间隔也各自成条
        let evs = vec![
            env(1, EventKind::Core(CoreEvent::AssistantChunk {
                turn: 1,
                step: 1,
                chunk: bm_protocol::StreamChunk { text: "s1".into() },
            })),
            env(2, EventKind::Core(CoreEvent::AssistantMessage {
                turn: 1,
                step: 1,
                msg: bm_protocol::AssistantMsg { content: "S1".into() },
                usage: None,
            })),
            env(3, EventKind::Core(CoreEvent::AssistantChunk {
                turn: 1,
                step: 2,
                chunk: bm_protocol::StreamChunk { text: "s2".into() },
            })),
            env(4, EventKind::Core(CoreEvent::AssistantMessage {
                turn: 1,
                step: 2,
                msg: bm_protocol::AssistantMsg { content: "S2".into() },
                usage: None,
            })),
            env(5, EventKind::Core(CoreEvent::AssistantChunk {
                turn: 2,
                step: 1,
                chunk: bm_protocol::StreamChunk { text: "t2".into() },
            })),
        ];
        let msgs = fold(evs);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].content, "S1");
        assert_eq!(msgs[1].content, "S2");
        assert_eq!(msgs[2].content, "t2", "新 turn 的 chunk 不得并入上一步消息");
    }

    #[test]
    fn tool_placeholder_filled_by_message_of_same_step() {
        // 工具占位（无 chunk）→ 同 step 的 AssistantMessage 填充内容
        let evs = vec![
            env(1, EventKind::Core(CoreEvent::UserMessage {
                msg: bm_protocol::UserMsg { content: "run".into() },
                source: bm_protocol::UserMsgSource::Human,
            })),
            env(2, EventKind::Core(CoreEvent::ToolCall {
                turn: 1,
                step: 1,
                call_id: CallId::new("c1"),
                name: "exec".into(),
                args: r#"{"cmd":"ls"}"#.into(),
            })),
            env(3, EventKind::Core(CoreEvent::ToolResult {
                turn: 1,
                step: 1,
                call_id: CallId::new("c1"),
                result: ToolResultMsg { ok: true, output: "ok".into() },
                meta: None,
            })),
            env(4, EventKind::Core(CoreEvent::AssistantMessage {
                turn: 1,
                step: 1,
                msg: bm_protocol::AssistantMsg { content: "完成了".into() },
                usage: None,
            })),
        ];
        let msgs = fold(evs);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "完成了");
        assert_eq!(msgs[1].tool_calls.len(), 1, "工具挂靠不因填充丢失");
        assert_eq!(msgs[1].tool_calls[0].result.as_ref().unwrap().output, "ok");
    }

    #[test]
    fn tool_call_result_pairs_up() {
        let evs = vec![
            env(1, EventKind::Core(CoreEvent::UserMessage {
                msg: bm_protocol::UserMsg { content: "search rust".into() },
                source: bm_protocol::UserMsgSource::Human,
            })),
            env(2, EventKind::Core(CoreEvent::ToolCall {
                turn: 1,
                step: 1,
                call_id: CallId::new("c1"),
                name: "web_search".into(),
                args: r#"{"q":"rust"}"#.into(),
            })),
            env(3, EventKind::Core(CoreEvent::ToolResult {
                turn: 1,
                step: 1,
                call_id: CallId::new("c1"),
                result: ToolResultMsg { ok: true, output: "results...".into() },
                meta: None,
            })),
            env(4, EventKind::Core(CoreEvent::AssistantMessage {
                turn: 1,
                step: 1,
                msg: bm_protocol::AssistantMsg { content: "done".into() },
                usage: None,
            })),
        ];
        let msgs = fold(evs);
        let tool_msg = &msgs[1];
        assert_eq!(tool_msg.tool_calls.len(), 1);
        assert_eq!(tool_msg.tool_calls[0].call_id, "c1");
        assert_eq!(tool_msg.tool_calls[0].result.as_ref().unwrap().output, "results...");
    }

    #[test]
    fn compaction_replace_masks_interval() {
        let evs = vec![
            env(1, EventKind::Core(CoreEvent::UserMessage {
                msg: bm_protocol::UserMsg { content: "a".into() },
                source: bm_protocol::UserMsgSource::Human,
            })),
            env(2, EventKind::Core(CoreEvent::AssistantMessage {
                turn: 1,
                step: 1,
                msg: bm_protocol::AssistantMsg { content: "b".into() },
                usage: None,
            })),
        ];
        let mut p = SurfaceProjection::new();
        for ev in evs {
            p.on_event(&ev).unwrap();
        }
        // 压缩：遮蔽 1..2（surface_op=Replace）
        let mut compact = env(3, EventKind::Core(CoreEvent::CompactionSummary {
            msg: bm_protocol::CompactionSummaryMsg {
                removed_start: 1,
                removed_end: 2,
                summary: "用户问 a，助手答 b".into(),
            },
        }));
        compact.surface_op = Some(SurfaceOp::Replace { start: 1, end: 2 });
        p.on_event(&compact).unwrap();
        let msgs = p.into_messages();
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].content.starts_with("[已压缩 1..2]"));
    }

    #[test]
    fn inverted_replace_rejected() {
        let mut p = SurfaceProjection::new();
        let mut ev = env(1, EventKind::Core(CoreEvent::TurnStart { turn: 1 }));
        ev.surface_op = Some(SurfaceOp::Replace { start: 5, end: 2 });
        let err = p.on_event(&ev).unwrap_err();
        assert_eq!(err.code(), ErrorCode::SurfaceViolation);
    }

    #[test]
    fn checkpoint_is_stable_json() {
        let evs = vec![env(1, EventKind::Core(CoreEvent::TurnStart { turn: 1 }))];
        let mut p = SurfaceProjection::new();
        for ev in evs {
            p.on_event(&ev).unwrap();
        }
        // 快照可序列化（消息面为空也稳定）
        assert_eq!(p.checkpoint(), serde_json::json!([]));
    }
}
