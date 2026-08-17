//! 事件翻译：内部 SessionEvent（kernel 词汇）→ dsh wire SessionEvent。
//!
//! wire 信封逐字形状（台账 §3.2）：`{type, seq, time(epoch ms), data}`。
//! 翻译器是状态机：内部事件不带 turn/step 的（UserMessage/AssistantChunk/
//! AssistantMessage/ToolCall/ToolResult）需要从 Turn/Step 事件游标推断。

use serde::Serialize;
use serde_json::{json, Value};
use kernel_contracts::session::{SessionEvent, StepPhase, TurnEvent};

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
            SessionEvent::Turn(TurnEvent::Ended { turn }) => {
                ("turn/end", json!({ "turn": turn, "reason": "completed" }))
            }
            SessionEvent::Step { turn, step, phase } => {
                self.cur_turn = *turn as i64;
                self.cur_step = *step as i64;
                match phase {
                    StepPhase::Started => ("step/start", json!({ "turn": turn, "step": step })),
                    StepPhase::Ended => ("step/end", json!({ "turn": turn, "step": step })),
                }
            }
            SessionEvent::AssistantChunk { text } => (
                "assistant/chunk",
                json!({
                    "turn": self.cur_turn,
                    "step": self.cur_step,
                    "chunk": { "type": "text", "text": text }
                }),
            ),
            SessionEvent::AssistantMessage { content } => {
                let blocks: Vec<Value> = content
                    .iter()
                    .map(|b| match b {
                        kernel_contracts::ContentBlock::Text(t) => {
                            json!({ "type": "text", "text": t })
                        }
                        kernel_contracts::ContentBlock::Reasoning(t) => {
                            json!({ "type": "reasoning", "text": t })
                        }
                        kernel_contracts::ContentBlock::ToolCall(c) => json!({
                            "type": "tool_call",
                            "id": c.id,
                            "name": c.name,
                            "arguments": serde_json::to_string(&c.arguments).unwrap_or_default()
                        }),
                        kernel_contracts::ContentBlock::ToolResult(_) => Value::Null,
                    })
                    .filter(|v| !v.is_null())
                    .collect();
                (
                    "assistant/message",
                    json!({
                        "turn": self.cur_turn,
                        "step": self.cur_step,
                        "message": { "content": blocks }
                    }),
                )
            }
            SessionEvent::ToolCall { call } => (
                "tool/call",
                json!({
                    "turn": self.cur_turn,
                    "step": self.cur_step,
                    "callId": call.id,
                    "name": call.name,
                    "arguments": serde_json::to_string(&call.arguments).unwrap_or_default()
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
    use kernel_contracts::{ContentBlock, ToolCall, ToolCallResult};

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
            SessionEvent::AssistantMessage {
                content: vec![ContentBlock::Text("hello".into())],
            },
            SessionEvent::Turn(TurnEvent::Ended { turn: 1 }),
        ];
        let wire = translate_events(&events);
        // SessionStarted 跳过：user/message seq0, turn/start seq1, ...
        assert_eq!(wire.len(), 4);
        assert_eq!(wire[0].seq, 0);
        assert_eq!(wire[1].seq, 1);
        assert_eq!(wire[2].seq, 2);
        assert_eq!(wire[3].seq, 3);
        assert_eq!(wire[0].type_, "user/message");
        assert_eq!(wire[1].type_, "turn/start");
    }

    #[test]
    fn turn_step_inference() {
        let events = vec![
            SessionEvent::Turn(TurnEvent::Started { turn: 2 }),
            SessionEvent::Step { turn: 2, step: 1, phase: StepPhase::Started },
            SessionEvent::AssistantChunk { text: "partial".into() },
            SessionEvent::Step { turn: 2, step: 1, phase: StepPhase::Ended },
        ];
        let wire = translate_events(&events);
        // chunk 事件拿到 turn=2 step=1
        let chunk = wire.iter().find(|w| w.type_ == "assistant/chunk").unwrap();
        assert_eq!(chunk.data["turn"], 2);
        assert_eq!(chunk.data["step"], 1);
        assert_eq!(chunk.data["chunk"]["text"], "partial");
    }

    #[test]
    fn tool_events_shapes() {
        let events = vec![
            SessionEvent::ToolCall {
                call: ToolCall {
                    id: "call_0".into(),
                    name: "echo".into(),
                    arguments: serde_json::json!({ "text": "x" }),
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
}
