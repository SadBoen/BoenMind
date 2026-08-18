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
    /// surface 标记（对齐 DSH `SurfaceOp`：append/replace）。仅 user/message、
    /// assistant/message、tool/result 三型 surface 事件携带——前端匹配器
    /// `isAppendSurfaceEvent` 靠它识别可渲染消息（缺省 = 非 surface，前端跳过）。
    #[serde(rename = "surfaceOp", skip_serializing_if = "Option::is_none")]
    pub surface_op: Option<&'static str>,
    pub data: Value,
}

/// 单条增量翻译器（WS 实时转发用）：内部事件 → wire 事件，游标随调用推进。
#[derive(Default)]
pub struct EventTranslator {
    cur_turn: i64,
    cur_step: i64,
    /// 已产出的 wire 事件计数（= 下一事件在 wire 序列中的下标）。与外部 seq
    /// 计数器同源（history 全量重建从 0、实时增量从历史尾部接续），message id
    /// 据此生成，保证同一事件跨 history/实时两条路径拿到稳定一致的 id。
    emitted: i64,
}

impl EventTranslator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 实时路径构造器：把已产出的计数预填为历史 wire 长度，使后续增量事件
    /// 的 message id 与 history 全量重建保持一致。
    pub fn with_emitted(seed: i64) -> Self {
        Self {
            cur_turn: 0,
            cur_step: 0,
            emitted: seed,
        }
    }

    /// 翻译单条内部事件；无 wire 对应（SessionStarted/SessionEnded）→ None。
    pub fn translate_one(&mut self, ev: &SessionEvent) -> Option<WireSessionEvent> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let my_seq = self.emitted;
        let (type_, surface_op, data) = match ev {
            SessionEvent::SessionStarted { .. } | SessionEvent::SessionEnded { .. } => {
                return None
            }
            SessionEvent::UserMessage { text } => {
                self.cur_step = 0;
                (
                    "user/message",
                    Some("append"),
                    json!({
                        // 对齐官方 Message 契约：id（稳定身份）+ content 块 +
                        // source.kind='user'（前端按 kind 区分 user/context 节点，
                        // 'human' 会被误分类为注入上下文）。
                        "id": String::from("user-") + &my_seq.to_string(),
                        "content": [{ "type": "text", "text": text }],
                        "source": { "kind": "user" }
                    }),
                )
            }
            SessionEvent::Turn(TurnEvent::Started { turn }) => {
                self.cur_turn = *turn as i64;
                self.cur_step = 0;
                ("turn/start", None, json!({ "turn": turn }))
            }
            SessionEvent::Turn(TurnEvent::Ended { turn, reason }) => (
                "turn/end",
                None,
                json!({ "turn": turn, "reason": turn_reason_wire(reason) }),
            ),
            SessionEvent::Step { turn, step, phase } => {
                self.cur_turn = *turn as i64;
                self.cur_step = *step as i64;
                match phase {
                    StepPhase::Started => (
                        "step/start",
                        None,
                        json!({ "turn": turn, "step": step }),
                    ),
                    StepPhase::Ended => ("step/end", None, json!({ "turn": turn, "step": step })),
                }
            }
            SessionEvent::AssistantChunk { chunk } => {
                let wire = chunk.to_wire();
                // finish 块：`{type:'finish', reason}`（usage 也在流中，逐块透传）。
                (
                    "assistant/chunk",
                    None,
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
                    "message": {
                        // 对齐官方 AssistantMessage：id + role + content + source（kind=model）。
                        "id": String::from("assistant-") + &my_seq.to_string(),
                        "role": "assistant",
                        "content": blocks,
                        "source": { "kind": "model" }
                    }
                });
                if let Some(u) = usage {
                    data["usage"] = u.to_wire();
                }
                ("assistant/message", Some("append"), data)
            }
            SessionEvent::ToolCall { call } => (
                "tool/call",
                None,
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
                // 对齐官方 ToolResultMessage：message 是完整 Message（id/role/content
                // tool-result 块 + source.callId），error/meta 可选。
                let block = json!({
                    "type": "tool-result",
                    "toolCallId": result.call_id,
                    "content": [{ "type": "text", "text": result.output }],
                    "isError": result.is_error,
                });
                let message = json!({
                    "id": String::from("tool-") + &my_seq.to_string(),
                    "role": "user",
                    "content": [block],
                    "source": { "kind": "tool", "callId": result.call_id }
                });
                let mut data = json!({
                    "turn": self.cur_turn,
                    "step": self.cur_step,
                    "message": message
                });
                if result.is_error {
                    data["error"] = json!({ "name": "ToolError", "code": "tool-failed" });
                }
                ("tool/result", Some("append"), data)
            }
        };
        self.emitted += 1;
        Some(WireSessionEvent {
            type_: type_.to_string(),
            seq: -1, // 增量翻译不负责 seq；调用方（ws 层）维护
            time: now_ms,
            surface_op,
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
        // surface 标记 + 官方 Message 契约字段（前端 isAppendSurfaceEvent 依赖）。
        assert_eq!(wire[0].surface_op, Some("append"));
        let ser = serde_json::to_value(&wire[0]).unwrap();
        assert_eq!(ser["surfaceOp"], "append");
        assert_eq!(ser["type"], "user/message");
        assert_eq!(wire[0].data["id"], "user-0");
        assert_eq!(wire[0].data["source"], json!({ "kind": "user" }));
        // 非 surface 事件不序列化 surfaceOp（信封顶层无该键）。
        let ser_turn = serde_json::to_value(&wire[1]).unwrap();
        assert!(ser_turn.get("surfaceOp").is_none());
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
        // surface 标记 + 官方 AssistantMessage 契约（id/role/content/source）。
        assert_eq!(msg.surface_op, Some("append"));
        assert_eq!(msg.data["message"]["id"], "assistant-5");
        assert_eq!(msg.data["message"]["role"], "assistant");
        assert_eq!(msg.data["message"]["source"], json!({ "kind": "model" }));
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
        assert_eq!(wire[1].surface_op, Some("append"));
        assert_eq!(wire[1].data["message"]["source"], json!({ "kind": "tool", "callId": "call_0" }));
        assert_eq!(wire[1].data["message"]["content"][0]["type"], "tool-result");
        assert_eq!(wire[1].data["message"]["content"][0]["toolCallId"], "call_0");
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
