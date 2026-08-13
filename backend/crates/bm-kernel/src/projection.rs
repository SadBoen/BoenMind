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

    /// 追加内容到最后一个 assistant 消息（chunk 合并）。
    fn append_to_last_assistant(&mut self, seq: u64, text: &str) {
        if let Some(last) = self.messages.last_mut().filter(|m| m.role == "assistant") {
            last.content.push_str(text);
            return;
        }
        self.messages.push(SurfaceMessage {
            seq,
            role: "assistant".into(),
            content: text.to_string(),
            tool_calls: Vec::new(),
        });
    }

    fn attach_tool_call(&mut self, seq: u64, call: SurfaceToolCall) {
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
                    });
                }
                CoreEvent::AssistantMessage { msg, .. } => {
                    // 合并规则：最后的 assistant 若是"工具占位"（无内容仅挂
                    // 工具调用，由 ToolCall 事件创建）→ 填充内容，不新建消息
                    let merged = self
                        .messages
                        .last_mut()
                        .is_some_and(|last| {
                            last.role == "assistant"
                                && last.content.is_empty()
                                && !last.tool_calls.is_empty()
                        });
                    if merged {
                        let last = self.messages.last_mut().expect("checked above");
                        last.content = msg.content.clone();
                    } else {
                        self.messages.push(SurfaceMessage {
                            seq: ev.seq.as_u64(),
                            role: "assistant".into(),
                            content: msg.content.clone(),
                            tool_calls: Vec::new(),
                        });
                    }
                }
                CoreEvent::AssistantChunk { chunk, .. } => {
                    self.append_to_last_assistant(ev.seq.as_u64(), &chunk.text);
                }
                CoreEvent::ToolCall { call_id, name, args, .. } => {
                    self.attach_tool_call(
                        ev.seq.as_u64(),
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
