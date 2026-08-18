//! headless 二进制：门禁 1 的验收载体。
//!
//! 在 mock LLM 下跑完整 headless 回合（消息→工具→回复），并支持
//! kill -9 恢复测试所需的三个模式：
//!
//! - `roundtrip <db> <session>`：建会话 + 跑一轮工具回合（echo 工具），退出码 0
//! - `abort <db> <session> <step>`：自死在指定 step 落盘之后（模拟 kill -9 断点）
//! - `resume <db> <session>`：恢复会话，断言尾部无 torn，续跑一轮
//! - `verify-tail <db> <session>`：加载日志，断言尾部结构完整（无悬空 Step/Turn）
//!
//! 事件日志尾部完整性规则（对齐 dsh persistence 语义）：日志永不出现
//! torn-tail——每条事件要么在事务内整体落盘，要么整体不落；回合可能停在
//! 半截，但"已落盘的部分"必须配对完整（Step Started ↔ Ended，Turn Started ↔ Ended）。

use std::path::PathBuf;
use std::sync::Arc;

use kernel_assembly::Runtime;
use kernel_contracts::session::{SessionEvent, SessionHeader, SessionId, StepPhase, TurnEvent};
use kernel_contracts::tools::{ToolExecutionResult, ToolHandler};
use kernel_llm::MockTurn;

/// echo 工具：headless 工具回合的验证工具。
struct EchoTool;

#[async_trait::async_trait]
impl ToolHandler for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "echoes back the given text"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" }
            }
        })
    }
    async fn execute(
        &self,
        input: kernel_contracts::ToolExecutionInput,
    ) -> Result<ToolExecutionResult, kernel_contracts::ToolError> {
        let text = input.arguments.get("text").and_then(|v| v.as_str()).unwrap_or("");
        Ok(ToolExecutionResult::ok(format!("echo: {text}")))
    }
}

fn usage() -> ! {
    eprintln!("usage: headless <roundtrip|abort|resume|verify-tail|dump> <db> <session> [step]");
    std::process::exit(2);
}

fn header(id: &str) -> SessionHeader {
    SessionHeader {
        id: SessionId(id.to_string()),
        app: "headless".into(),
        profile: "headless".into(),
        workspace: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn install_tools(rt: &Runtime) {
    rt.tools.register(Arc::new(EchoTool)).unwrap();
    rt.gate.enable("echo");
}

/// 把 headless 的 mock LLM 换成"固定脚本"：第一轮工具调用，第二轮文本。
fn install_scripted_llm(rt: &mut Runtime) {
    // Runtime 字段公开可改，直接替换 llm。
    rt.llm = Arc::new(kernel_llm::ScriptLlm::new(
        "mock".to_string(),
        "mock-1".to_string(),
        vec![
            MockTurn::Tool {
                name: "echo".to_string(),
                arguments: serde_json::json!({ "text": "hello" }),
                then_text: "你好，我是 BoenMind 微内核。".to_string(),
            },
        ],
    ));
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        usage();
    }
    let mode = args[1].as_str();
    let db = PathBuf::from(&args[2]);
    let session = &args[3];

    match mode {
        "roundtrip" => {
            let mut rt = Runtime::headless(db).unwrap();
            install_scripted_llm(&mut rt);
            install_tools(&rt);
            let agent = rt.create_session(header(session)).await.unwrap();
            let outcome = agent.run_turn(Some("hi")).await.unwrap();
            assert!(outcome.steps >= 2, "expected >=2 steps, got {}", outcome.steps);
            assert_eq!(outcome.reason, kernel_contracts::session::TurnEndReason::Completed);
            // 最后一段文本从日志投影：AssistantMessage 文本块。
            let last_text = agent
                .session()
                .events()
                .iter()
                .rev()
                .find_map(|r| match &r.event {
                    SessionEvent::AssistantMessage { content, .. } => {
                        let t: String = content
                            .iter()
                            .filter_map(|b| match b {
                                kernel_contracts::ContentBlock::Text(t) => Some(t.clone()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        if t.is_empty() { None } else { Some(t) }
                    }
                    _ => None,
                });
            assert_eq!(
                last_text.as_deref(),
                Some("你好，我是 BoenMind 微内核。")
            );
            println!("roundtrip OK: {} steps", outcome.steps);
        }
        "abort" => {
            // 自死模式：复刻 loop 事件序列到指定 step 的断点，然后退出。
            // 断点 1 = 落完 Step{1,1,Started} 后；断点 2 = 落完第一轮文本 chunk 后。
            let step: usize = args
                .get(4)
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| {
                    eprintln!("abort requires step (1 or 2)");
                    std::process::exit(2);
                });
            let rt = Runtime::headless(db).unwrap();
            let agent = rt.create_session(header(session)).await.unwrap();
            let persist = Arc::clone(&rt.persist);
            let sid = session.to_string();

            // UserMessage
            agent
                .session()
                .append(SessionEvent::UserMessage { text: "hi".into() });
            persist.append_events(&sid, &[SessionEvent::UserMessage { text: "hi".into() }]).await.unwrap();
            // Turn Started（对齐真实 loop 的回合开启事件；断点语义保持）
            agent.session().append(SessionEvent::Turn(TurnEvent::Started { turn: 1 }));
            persist.append_events(&sid, &[SessionEvent::Turn(TurnEvent::Started { turn: 1 })]).await.unwrap();
            // Step Started（断点 1：落盘后自死）
            agent.session().append(SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Started,
            });
            persist.append_events(&sid, &[SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Started,
            }]).await.unwrap();
            if step == 1 {
                std::process::abort();
            }
            // 模拟模型文本 chunk（断点 2：落盘后自死）
            let chunk = SessionEvent::AssistantChunk {
                chunk: kernel_contracts::StreamChunk::TextDelta {
                    index: 0,
                    text: "part".into(),
                },
            };
            agent.session().append(chunk.clone());
            persist.append_events(&sid, &[chunk]).await.unwrap();
            if step == 2 {
                std::process::abort();
            }
            eprintln!("abort mode: unknown step (only 1 or 2)");
            std::process::exit(2);
        }
        "resume" => {
            let rt = Runtime::headless(db).unwrap();
            install_tools(&rt);
            // 恢复（restore_session 内部自动做 interrupted-turn 修复）。
            let agent = match rt.restore_session(session).await {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("resume failed: {e}");
                    std::process::exit(1);
                }
            };
            // 修复后尾部必须完整。
            if !verify_tail(&rt, session).await {
                eprintln!("tail is torn after repair");
                std::process::exit(1);
            }
            // 恢复后的会话必须能续跑（turn 编号接续）。
            let outcome = agent.run_turn(None).await.unwrap();
            println!("resume OK: continued {} steps", outcome.steps);
        }
        "verify-tail" => {
            let rt = Runtime::headless(db).unwrap();
            if verify_tail(&rt, session).await {
                println!("tail OK");
            } else {
                eprintln!("tail is torn");
                std::process::exit(1);
            }
        }
        "dump" => {
            let rt = Runtime::headless(db).unwrap();
            let events = rt
                .persist
                .load_events(session)
                .await
                .expect("load events");
            match events {
                Some(events) => {
                    for (i, e) in events.iter().enumerate() {
                        println!("[{i}] {}", serde_json::to_string(e).unwrap());
                    }
                }
                None => eprintln!("session not found"),
            }
        }
        _ => usage(),
    }
}

/// 尾部完整性检查：从日志尾部往回，断言没有未配对的 Step Started / Turn Started。
async fn verify_tail(rt: &Runtime, session_id: &str) -> bool {
    let events = match rt.persist.load_events(session_id).await {
        Ok(Some(e)) => e,
        _ => return false,
    };
    let mut step_open = 0u64;
    let mut turn_open = false;
    for ev in events.iter().rev() {
        match ev {
            SessionEvent::Step { phase: StepPhase::Ended, .. } => step_open += 1,
            SessionEvent::Step { phase: StepPhase::Started, .. } => {
                if step_open > 0 {
                    step_open -= 1;
                } else {
                    return false; // 未配对 Step Started = torn
                }
            }
            SessionEvent::Turn(TurnEvent::Ended { .. }) => turn_open = true,
            SessionEvent::Turn(TurnEvent::Started { .. }) => {
                if turn_open {
                    turn_open = false;
                } else {
                    return false; // 未配对 Turn Started = torn
                }
            }
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use kernel_contracts::llm::{FinishReason, LlmPort, StreamChunk};

    /// 脚本 LLM 端到端：固定脚本产出工具调用 + 续文本。
    #[tokio::test]
    async fn scripted_llm_produces_tool_then_text() {
        let llm = kernel_llm::ScriptLlm::new(
            "mock".to_string(),
            "mock-1".to_string(),
            vec![MockTurn::Tool {
                name: "echo".to_string(),
                arguments: serde_json::json!({ "text": "hi" }),
                then_text: "done".to_string(),
            }],
        );
        let mut stream = llm.stream(kernel_contracts::GenerateOptions {
            provider: "mock".into(),
            model: "mock-1".into(),
            messages: vec![],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            session_id: None,
            signal: None,
            reasoning_effort: None,
            thinking: None,
            purpose: None,
        });
        let mut saw_tool_block = false;
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamChunk::BlockEnd { block, .. }) = &chunk {
                if matches!(block, kernel_contracts::ContentBlock::ToolCall(c) if c.name == "echo") {
                    saw_tool_block = true;
                }
            }
            if let Ok(StreamChunk::Finish(FinishReason::ToolCalls)) = chunk {
                break;
            }
        }
        assert!(saw_tool_block);
    }

    #[test]
    fn verify_tail_logic() {
        // 完整配对 → true
        let events = vec![
            SessionEvent::Step { turn: 1, step: 1, phase: StepPhase::Started },
            SessionEvent::Step { turn: 1, step: 1, phase: StepPhase::Ended },
            SessionEvent::Turn(TurnEvent::Started { turn: 1 }),
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: kernel_contracts::TurnEndReason::Completed,
            }),
        ];
        assert!(check_tail(&events));
        // 悬空 Step Started → false
        let events = vec![
            SessionEvent::Step { turn: 1, step: 1, phase: StepPhase::Started },
            SessionEvent::Step { turn: 1, step: 1, phase: StepPhase::Ended },
            SessionEvent::Step { turn: 1, step: 2, phase: StepPhase::Started },
        ];
        assert!(!check_tail(&events));
    }

    fn check_tail(events: &[SessionEvent]) -> bool {
        let mut step_open = 0u64;
        let mut turn_open = false;
        for ev in events.iter().rev() {
            match ev {
                SessionEvent::Step { phase: StepPhase::Ended, .. } => step_open += 1,
                SessionEvent::Step { phase: StepPhase::Started, .. } => {
                    if step_open > 0 {
                        step_open -= 1;
                    } else {
                        return false;
                    }
                }
                SessionEvent::Turn(TurnEvent::Ended { .. }) => turn_open = true,
                SessionEvent::Turn(TurnEvent::Started { .. }) => {
                    if turn_open {
                        turn_open = false;
                    } else {
                        return false;
                    }
                }
                _ => {}
            }
        }
        true
    }
}
