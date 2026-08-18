//! 事件翻译：内部 SessionEvent（kernel 词汇）→ dsh wire SessionEvent。
//!
//! wire 信封逐字形状（台账 §3.2）：`{type, seq, time(epoch ms), data}`。
//! 翻译器是状态机：内部事件不带 turn/step 的（UserMessage/AssistantChunk/
//! AssistantMessage/ToolCall/ToolResult）需要从 Turn/Step 事件游标推断。

use serde::Serialize;
use serde_json::{json, Value};
use kernel_contracts::llm::block_to_wire;
use kernel_contracts::session::{SessionEvent, StepPhase, TurnEndReason, TurnEvent};

/// wire SessionEvent 信封。
#[derive(Debug, Clone, Serialize)]
pub struct WireSessionEvent {
    #[serde(rename = "type")]
    pub type_: String,
    pub seq: i64,
    pub time: i64,
    pub data: Value,
}

/// 单条增量翻译器（WS 实时转发用）：内部事件 → wire 事件，游标随调用推进。
#[derive(Default)]
pub struct EventTranslator {
    cur_turn: i64,
    cur_step: i64,
}

impl EventTranslator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 翻译单条内部事件；无 wire 对应（SessionStarted/SessionEnded）→ None。
    pub fn translate_one(&mut self, ev: &SessionEvent) -> Option<WireSessionEvent> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let (type_, data) = match ev {
            SessionEvent::SessionStarted { .. } | SessionEvent::SessionEnded { .. } => {
                return None
            }
            SessionEvent::UserMessage { text } => {
                self.cur_step = 0;
                (
                    "user/message",
                    json!({
                        "content": [{ "type": "text", "text": text }],
                        "source": { "kind": "human" }
                    }),
                )
            }
            SessionEvent::Turn(TurnEvent::Started { turn }) => {
                self.cur_turn = *turn as i64;
                self.cur_step = 0;
                ("turn/start", json!({ "turn": turn }))
            }
            SessionEvent::Turn(TurnEvent::Ended { turn, reason }) => (
                "turn/end",
                json!({ "turn": turn, "reason": turn_reason_wire(reason) }),
            ),
            SessionEvent::Step { turn, step, phase } => {
                self.cur_turn = *turn as i64;
                self.cur_step = *step as i64;
                match phase {
                    StepPhase::Started => ("step/start", json!({ "turn": turn, "step": step })),
                    StepPhase::Ended => ("step/end", json!({ "turn": turn, "step": step })),
                }
            }
            SessionEvent::AssistantChunk { chunk } => {
                let wire = chunk.to_wire();
                // finish 块：`{type:'finish', reason}`（usage 也在流中，逐块透传）。
                (
                    "assistant/chunk",
                    json!({
                        "turn": self.cur_turn,
                        "step": self.cur_step,
                        "chunk": wire,
                    }),
                )
            }
            SessionEvent::AssistantMessage { content, usage } => {
                let blocks: Vec<Value> = content
                    .iter()
                    .map(block_to_wire)
                    .filter(|v| !v.is_null())
                    .collect();
                let mut data = json!({
                    "turn": self.cur_turn,
                    "step": self.cur_step,
                    "message": { "content": blocks }
                });
                if let Some(u) = usage {
                    data["usage"] = u.to_wire();
                }
                ("assistant/message", data)
            }
            SessionEvent::ToolCall { call } => (
                "tool/call",
                json!({
                    "turn": self.cur_turn,
                    "step": self.cur_step,
                    "callId": call.id,
                    "name": call.name,
                    // 模型原始 JSON 文本（未解析）——wire 透传保真。
                    "arguments": call.arguments,
                }),
            ),
            SessionEvent::ToolResult { result } => {
                let data = if result.is_error {
                    json!({
                        "turn": self.cur_turn,
                        "step": self.cur_step,
                        "message": { "callId": result.call_id, "output": result.output },
                        "error": { "name": "ToolError", "code": "tool-failed" }
                    })
                } else {
                    json!({
                        "turn": self.cur_turn,
                        "step": self.cur_step,
                        "message": { "callId": result.call_id, "output": result.output }
                    })
                };
                ("tool/result", data)
            }
        };
        Some(WireSessionEvent {
            type_: type_.to_string(),
            seq: -1, // 增量翻译不负责 seq；调用方（ws 层）维护
            time: now_ms,
            data,
        })
    }
}

/// 把内部回合结束原因转 wire 形状（对齐 DSH `TurnEndReasonMap` 的 kind 词汇）。
fn turn_reason_wire(reason: &TurnEndReason) -> Value {
    match reason {
        TurnEndReason::Completed => json!({ "kind": "completed" }),
        TurnEndReason::Aborted { reason } => json!({
            "kind": "aborted",
            "reason": { "kind": reason },
        }),
        TurnEndReason::Blocked => json!({ "kind": "blocked" }),
        TurnEndReason::Error { message, code, request_id } => {
            let mut error = json!({ "message": message, "code": code });
            if let Some(rid) = request_id {
                error["requestId"] = json!(rid);
            }
            json!({ "kind": "error", "error": error })
        }
        TurnEndReason::MaxTokens => json!({ "kind": "max-tokens" }),
        TurnEndReason::Interrupted => json!({ "kind": "interrupted" }),
    }
}

/// 把内部事件日志翻译为 wire 事件序列（seq 从 0 连续，对齐 `seq = log.length` 契约）。
pub fn translate_events(events: &[SessionEvent]) -> Vec<WireSessionEvent> {
    let mut out = Vec::new();
    let mut translator = EventTranslator::new();
    let mut seq: i64 = 0;
    for ev in events {
        if let Some(mut wire) = translator.translate_one(ev) {
            wire.seq = seq;
            out.push(wire);
            seq += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel_contracts::session::{SessionHeader, SessionId};
    use kernel_contracts::llm::{ContentBlock, StreamChunk, TokenUsage, ToolCall, ToolCallResult};

    fn header() -> SessionHeader {
        SessionHeader {
            id: SessionId("s1".into()),
            app: "test".into(),
            profile: "headless".into(),
            workspace: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn seq_continuous_from_zero() {
        let events = vec![
            SessionEvent::SessionStarted { header: header() },
            SessionEvent::UserMessage { text: "hi".into() },
            SessionEvent::Turn(TurnEvent::Started { turn: 1 }),
            SessionEvent::AssistantChunk {
                chunk: StreamChunk::BlockStart { index: 0, block_type: "text".into() },
            },
            SessionEvent::AssistantChunk {
                chunk: StreamChunk::TextDelta { index: 0, text: "hello".into() },
            },
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Completed,
            }),
        ];
        let wire = translate_events(&events);
        // SessionStarted 跳过：user/message seq0, turn/start seq1, chunk seq2/3, turn/end seq4
        assert_eq!(wire.len(), 5);
        assert_eq!(wire[0].seq, 0);
        assert_eq!(wire[1].seq, 1);
        assert_eq!(wire[2].seq, 2);
        assert_eq!(wire[3].seq, 3);
        assert_eq!(wire[4].seq, 4);
        assert_eq!(wire[0].type_, "user/message");
        assert_eq!(wire[1].type_, "turn/start");
        // turn/end 带 reason 词汇。
        assert_eq!(wire[4].type_, "turn/end");
        assert_eq!(wire[4].data["reason"], json!({ "kind": "completed" }));
    }

    #[test]
    fn chunk_and_usage_wire_shapes() {
        let events = vec![
            SessionEvent::Turn(TurnEvent::Started { turn: 2 }),
            SessionEvent::Step { turn: 2, step: 1, phase: StepPhase::Started },
            SessionEvent::AssistantChunk {
                chunk: StreamChunk::BlockStart { index: 0, block_type: "reasoning".into() },
            },
            SessionEvent::AssistantChunk {
                chunk: StreamChunk::ReasoningDelta { index: 0, text: "think".into() },
            },
            SessionEvent::AssistantChunk {
                chunk: StreamChunk::BlockEnd {
                    index: 0,
                    block: ContentBlock::Reasoning("think".into()),
                },
            },
            SessionEvent::AssistantMessage {
                content: vec![ContentBlock::Text("answer".into())],
                usage: Some(TokenUsage {
                    input: 27,
                    output: 69,
                    cache_read: Some(256),
                    cache_write: None,
                    reasoning: Some(24),
                }),
            },
            SessionEvent::Step { turn: 2, step: 1, phase: StepPhase::Ended },
        ];
        let wire = translate_events(&events);
        let chunk = wire.iter().find(|w| w.type_ == "assistant/chunk" && w.data["chunk"]["type"] == "reasoning-delta").unwrap();
        assert_eq!(chunk.data["turn"], 2);
        assert_eq!(chunk.data["step"], 1);
        assert_eq!(chunk.data["chunk"]["index"], 0);
        assert_eq!(chunk.data["chunk"]["text"], "think");
        let msg = wire.iter().find(|w| w.type_ == "assistant/message").unwrap();
        // usage 随 assistant/message（DSH 语义），且是 disjoint 计数。
        assert_eq!(msg.data["usage"]["inputTokens"], 27);
        assert_eq!(msg.data["usage"]["cacheReadTokens"], 256);
        assert_eq!(msg.data["usage"]["reasoningTokens"], 24);
        // 文本块 shape 对齐 DSH：{type:'text', text}。
        assert_eq!(msg.data["message"]["content"][0], json!({ "type": "text", "text": "answer" }));
    }

    #[test]
    fn tool_events_shapes() {
        let events = vec![
            SessionEvent::ToolCall {
                call: ToolCall {
                    id: "call_0".into(),
                    name: "echo".into(),
                    arguments: r#"{"text":"x"}"#.into(),
                },
            },
            SessionEvent::ToolResult {
                result: ToolCallResult {
                    call_id: "call_0".into(),
                    output: "echo: x".into(),
                    is_error: false,
                },
            },
        ];
        let wire = translate_events(&events);
        assert_eq!(wire[0].type_, "tool/call");
        assert_eq!(wire[0].data["name"], "echo");
        // arguments 是模型原始 JSON 文本（字符串）
        assert_eq!(wire[0].data["arguments"], r#"{"text":"x"}"#);
        assert_eq!(wire[1].type_, "tool/result");
        assert!(wire[1].data.get("error").is_none());
    }

    #[test]
    fn turn_end_error_reason_wire() {
        let events = vec![SessionEvent::Turn(TurnEvent::Ended {
            turn: 1,
            reason: TurnEndReason::Error {
                message: "bad model".into(),
                code: "MAX_STEPS".into(),
                request_id: Some("req-123".into()),
            },
        })];
        let wire = translate_events(&events);
        assert_eq!(wire[0].data["reason"]["kind"], "error");
        assert_eq!(wire[0].data["reason"]["error"]["code"], "MAX_STEPS");
        assert_eq!(wire[0].data["reason"]["error"]["requestId"], "req-123");

        // 无 request_id → wire 上不出该字段（精确形状）。
        let plain = vec![SessionEvent::Turn(TurnEvent::Ended {
            turn: 2,
            reason: TurnEndReason::Error {
                message: "m".into(),
                code: "C".into(),
                request_id: None,
            },
        })];
        let wire2 = translate_events(&plain);
        assert!(wire2[0].data["reason"]["error"].get("requestId").is_none());
    }
}
