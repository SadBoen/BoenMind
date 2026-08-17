//! # kernel-loop
//!
//! 回合循环：turn/step 驱动，waterfall 事件语义。
//!
//! 核心纪律 **model-visible-means-logged**：模型看到的一切都从会话事件日志
//! 投影（`Session::derive_messages`），每次模型调用前日志已完整；
//! 流式文本逐块 `AssistantChunk` 入日志，工具调用先记 `ToolCall` 再执行。
//!
//! 单回合流程（`run_turn`）：
//! 1. `append(UserMessage)`（可选，恢复续跑时已入日志则传 `None`）；
//! 2. step 循环（上限 [`MAX_STEPS`]，超限判 torn）：
//!    a. `append(Step Started)`；
//!    b. 投影消息 + enabled 工具 schema；
//!    c. `llm.stream(request)` 逐块消费（文本→chunk 日志，工具→暂存，Finish 收尾）；
//!    d. 分派：工具调用 → `AssistantMessage(ToolCall)` + 逐个 `ToolCall`/`ToolResult`，否则 → `AssistantMessage(Text)` 收尾。
//! 3. `append(Turn Ended)`。
//!
//! 持久化纪律：**logged-means-persisted**——每个事件 append 进会话日志后立即
//! 经 [`LoopRuntime::persist`] 落盘（单事件事务）。kill -9 发生时已 append 的事件
//! 必然已持久化，日志永不出现 torn-tail。

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::StreamExt;
use kernel_contracts::llm::{
    ContentBlock, FinishReason, GenerateOptions, LlmPort, StreamChunk, ToolCall, ToolCallResult,
};
use kernel_contracts::ports::SessionPersistPort;
use kernel_contracts::session::{SessionEvent, StepPhase, TurnEvent};
use kernel_contracts::tools::ToolExecutionInput;
use kernel_session::{Session, SessionStore};
use kernel_tools::{ToolGate, ToolRegistry};

/// 单回合最大 step 数；超限即 `LoopError::MaxSteps`（torn）。
pub const MAX_STEPS: u64 = 32;

/// 回合循环运行时装配：LLM 端口 + 会话存储 + 工具注册表/门控 + provider 标识 + 持久化端口。
pub struct LoopRuntime {
    pub llm: Arc<dyn LlmPort>,
    pub store: Arc<SessionStore>,
    pub tools: Arc<ToolRegistry>,
    pub gate: Arc<ToolGate>,
    pub persist: Arc<dyn SessionPersistPort>,
    pub provider: String,
    pub model: String,
}

/// 单回合结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    /// 实际执行的 step 数。
    pub steps: u64,
    /// 最后一段非空模型文本（无则 None）。
    pub last_text: Option<String>,
}

/// 回合循环错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoopError {
    #[error("llm error: {0}")]
    Llm(String),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("persist error: {0}")]
    Persist(String),
    #[error("turn exceeded max steps (32)")]
    MaxSteps,
}

/// 反应式回合代理：一次用户输入 → 若干 step，直到模型产出文本。
pub struct ReactLoopAgent {
    rt: Arc<LoopRuntime>,
    session: Arc<Session>,
}

impl ReactLoopAgent {
    pub fn new(rt: Arc<LoopRuntime>, session: Arc<Session>) -> Self {
        Self { rt, session }
    }

    pub fn session(&self) -> Arc<Session> {
        Arc::clone(&self.session)
    }

    /// 下一条回合编号：日志中已出现过的最大 turn 序号 + 1（恢复续跑不重复）。
    fn next_turn(&self) -> u64 {
        self.session
            .events()
            .iter()
            .filter_map(|r| match &r.event {
                SessionEvent::Turn(TurnEvent::Started { turn }) => Some(*turn),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            + 1
    }

    /// append 后立即持久化（logged-means-persisted）。
    async fn persist(&self, rec: &kernel_contracts::SessionRecord) -> Result<(), LoopError> {
        self.rt
            .persist
            .append_events(self.session.id().as_str(), std::slice::from_ref(&rec.event))
            .await
            .map_err(|e| LoopError::Persist(e.to_string()))
    }

    /// 跑一个回合。`user_text = Some` 时先入日志（新回合）；
    /// `None` 表示恢复续跑（用户消息已在日志中，从断点重跑）。
    /// 事件序列见模块文档；torn 流（Err 或缺失 Finish）直接返回 Err。
    pub async fn run_turn(&self, user_text: Option<&str>) -> Result<TurnOutcome, LoopError> {
        let turn = self.next_turn();

        if let Some(text) = user_text {
            let rec = self.session.append(SessionEvent::UserMessage {
                text: text.to_string(),
            });
            self.persist(&rec).await?;
        }

        let mut steps: u64 = 0;
        let mut last_text: Option<String> = None;

        loop {
            steps += 1;
            if steps > MAX_STEPS {
                return Err(LoopError::MaxSteps);
            }

            let rec = self.session.append(SessionEvent::Step {
                turn,
                step: steps,
                phase: StepPhase::Started,
            });
            self.persist(&rec).await?;

            // model-visible-means-logged：模型看到的全部来自日志投影。
            let messages = self.session.derive_messages();
            let tools = self.rt.gate.enabled_schemas(&self.rt.tools);

            let request = GenerateOptions {
                provider: self.rt.provider.clone(),
                model: self.rt.model.clone(),
                messages,
                tools,
                temperature: None,
                max_tokens: None,
                session_id: Some(self.session.id().as_str().to_string()),
            };

            // ---- 消费模型流 ----
            let mut stream = self.rt.llm.stream(request);
            let mut step_text = String::new();
            let mut finish_reason: Option<FinishReason> = None;
            // index -> (name, 累积 arguments)
            let mut pending_calls: BTreeMap<usize, (String, String)> = BTreeMap::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(StreamChunk::TextDelta { text }) => {
                        step_text.push_str(&text);
                        let rec = self.session.append(SessionEvent::AssistantChunk { text });
                        self.persist(&rec).await?;
                    }
                    // M1：推理增量不进日志、不进上下文。
                    Ok(StreamChunk::ReasoningDelta { .. }) => {}
                    Ok(StreamChunk::ToolCallDelta {
                        index,
                        name,
                        arguments_delta,
                    }) => {
                        let entry = pending_calls
                            .entry(index)
                            .or_insert_with(|| (name, String::new()));
                        entry.1.push_str(&arguments_delta);
                    }
                    Ok(StreamChunk::ToolCallDone {
                        index,
                        name,
                        arguments,
                    }) => {
                        // ToolCallDone 自带完整 arguments；为空时用 delta 累积兜底。
                        let final_args = if arguments.is_empty() {
                            pending_calls
                                .remove(&index)
                                .map(|(_, acc)| acc)
                                .unwrap_or_default()
                        } else {
                            arguments
                        };
                        let parsed =
                            serde_json::from_str(&final_args).unwrap_or(serde_json::Value::Null);
                        tool_calls.push(ToolCall {
                            id: format!("call_{index}"),
                            name,
                            arguments: parsed,
                        });
                    }
                    // M1：token 用量不计入。
                    Ok(StreamChunk::Usage(_)) => {}
                    Ok(StreamChunk::Finish(reason)) => {
                        finish_reason = Some(reason);
                        break;
                    }
                    // torn 流：禁止静默中断。
                    Err(e) => return Err(LoopError::Llm(e.message)),
                }
            }

            // Finish 缺失 = torn（端口契约要求流以 Finish 收尾）。
            if finish_reason.is_none() {
                return Err(LoopError::Llm(
                    "stream ended without Finish (torn)".to_string(),
                ));
            }

            // 兜底：provider 只发 delta 没发 Done 时，把已累积 arguments 拼成调用。
            for (index, (name, acc)) in pending_calls {
                if !tool_calls.iter().any(|c| c.id == format!("call_{index}")) {
                    let parsed =
                        serde_json::from_str(&acc).unwrap_or(serde_json::Value::Null);
                    tool_calls.push(ToolCall {
                        id: format!("call_{index}"),
                        name,
                        arguments: parsed,
                    });
                }
            }

            if !step_text.trim().is_empty() {
                last_text = Some(step_text.clone());
            }

            let wants_tools =
                finish_reason == Some(FinishReason::ToolCalls) || !tool_calls.is_empty();

            if wants_tools {
                // 模型即将看到工具调用：先记 AssistantMessage。
                let content: Vec<ContentBlock> = tool_calls
                    .iter()
                    .map(|c| ContentBlock::ToolCall(c.clone()))
                    .collect();
                let rec = self.session.append(SessionEvent::AssistantMessage { content });
                self.persist(&rec).await?;

                for call in &tool_calls {
                    // 先记调用，再执行（对齐 dsh tool/call 事件语义）。
                    let rec = self
                        .session
                        .append(SessionEvent::ToolCall { call: call.clone() });
                    self.persist(&rec).await?;
                    let input = ToolExecutionInput {
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    };
                    let result = match self
                        .rt
                        .gate
                        .execute_guarded(&self.rt.tools, input)
                        .await
                    {
                        Ok(r) => ToolCallResult {
                            call_id: call.id.clone(),
                            output: r.output.clone(),
                            is_error: r.is_error,
                        },
                        // fail-closed / 执行失败也回写日志（is_error=true）。
                        Err(e) => ToolCallResult {
                            call_id: call.id.clone(),
                            output: format!("tool error: {e}"),
                            is_error: true,
                        },
                    };
                    let rec = self.session.append(SessionEvent::ToolResult { result });
                    self.persist(&rec).await?;
                }

                let rec = self.session.append(SessionEvent::Step {
                    turn,
                    step: steps,
                    phase: StepPhase::Ended,
                });
                self.persist(&rec).await?;
                continue;
            }

            // 文本收尾（Stop / MaxTokens / Cancelled / Error）。
            let content = if finish_reason == Some(FinishReason::Error) {
                vec![ContentBlock::Text(
                    "model generation failed (finish_reason=Error)".to_string(),
                )]
            } else {
                vec![ContentBlock::Text(step_text.clone())]
            };
            let rec = self.session.append(SessionEvent::AssistantMessage { content });
            self.persist(&rec).await?;
            let rec = self.session.append(SessionEvent::Step {
                turn,
                step: steps,
                phase: StepPhase::Ended,
            });
            self.persist(&rec).await?;
            break;
        }

        let rec = self
            .session
            .append(SessionEvent::Turn(TurnEvent::Ended { turn }));
        self.persist(&rec).await?;

        Ok(TurnOutcome { steps, last_text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel_contracts::error::{LlmError, ToolError};
    use kernel_contracts::llm::{ChunkStream, LlmModelInfo, LlmPort};
    use kernel_contracts::session::{SessionHeader, SessionId};
    use kernel_contracts::tools::{ToolExecutionResult, ToolHandler};
    use kernel_contracts::EventBus;
    use kernel_session::SessionStore;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    // ---------- mocks ----------

    /// 内存持久化桩：记录每会话事件，供持久化接线断言。
    struct InMemoryPersist(std::sync::Mutex<Vec<(String, Vec<SessionEvent>)>>);

    #[async_trait::async_trait]
    impl SessionPersistPort for InMemoryPersist {
        async fn create_session(&self, _header: &kernel_contracts::session::SessionHeader) -> Result<(), kernel_contracts::PortError> {
            Ok(())
        }
        async fn append_events(
            &self,
            session_id: &str,
            events: &[SessionEvent],
        ) -> Result<(), kernel_contracts::PortError> {
            self.0
                .lock()
                .unwrap()
                .push((session_id.to_string(), events.to_vec()));
            Ok(())
        }
        async fn load_events(
            &self,
            _session_id: &str,
        ) -> Result<Option<Vec<SessionEvent>>, kernel_contracts::PortError> {
            Ok(None)
        }
        async fn list_sessions(&self) -> Result<Vec<String>, kernel_contracts::PortError> {
            Ok(vec![])
        }
        async fn delete_session(&self, _session_id: &str) -> Result<(), kernel_contracts::PortError> {
            Ok(())
        }
    }

    /// 脚本式 LLM：每次 `stream()` 弹出队首一步。
    #[derive(Clone)]
    enum ScriptStep {
        /// 一步模型调用 → `[TextDelta, Finish(Stop)]`。
        Text(String),
        /// 一步模型调用 → `[ToolCallDone, Finish(ToolCalls)]`；
        /// `then_text` 为下一轮调用的文本（预插队）。
        Tool {
            name: String,
            arguments: serde_json::Value,
            then_text: Option<String>,
        },
    }

    struct ScriptLlm {
        queue: Mutex<VecDeque<ScriptStep>>,
        model: String,
    }

    impl ScriptLlm {
        fn new(steps: Vec<ScriptStep>) -> Self {
            Self {
                queue: Mutex::new(steps.into()),
                model: "script-1".to_string(),
            }
        }

        fn next_stream(&self) -> ChunkStream {
            let step = match self.queue.lock().unwrap().pop_front() {
                Some(s) => s,
                // 脚本耗尽：产出一次空 Finish(Stop)，避免 mock 误 panic。
                None => {
                    return Box::pin(futures::stream::iter(vec![Ok(
                        StreamChunk::Finish(FinishReason::Stop),
                    )]));
                }
            };
            match step {
                ScriptStep::Text(text) => Box::pin(futures::stream::iter(vec![
                    Ok(StreamChunk::TextDelta { text }),
                    Ok(StreamChunk::Finish(FinishReason::Stop)),
                ])),
                ScriptStep::Tool {
                    name,
                    arguments,
                    then_text,
                } => {
                    if let Some(t) = then_text {
                        self.queue.lock().unwrap().push_front(ScriptStep::Text(t));
                    }
                    let args = serde_json::to_string(&arguments)
                        .unwrap_or_else(|_| "null".to_string());
                    Box::pin(futures::stream::iter(vec![
                        Ok(StreamChunk::ToolCallDone {
                            index: 0,
                            name,
                            arguments: args,
                        }),
                        Ok(StreamChunk::Finish(FinishReason::ToolCalls)),
                    ]))
                }
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmPort for ScriptLlm {
        async fn list_models(&self, _provider: &str) -> Result<Vec<LlmModelInfo>, LlmError> {
            Ok(vec![LlmModelInfo {
                id: self.model.clone(),
                label: None,
                supports_tools: true,
            }])
        }

        fn stream(&self, _request: GenerateOptions) -> ChunkStream {
            self.next_stream()
        }
    }

    /// echo 工具：把 arguments 原样回显。
    struct EchoTool;

    #[async_trait::async_trait]
    impl ToolHandler for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echoes back the given arguments"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "x": { "type": "integer" }
                }
            })
        }
        async fn execute(&self, input: ToolExecutionInput) -> Result<ToolExecutionResult, ToolError> {
            Ok(ToolExecutionResult::ok(
                serde_json::to_string(&input.arguments).unwrap_or_default(),
            ))
        }
    }

    // ---------- helpers ----------

    fn test_header(id: &str) -> SessionHeader {
        SessionHeader {
            id: SessionId(id.to_string()),
            app: "test".into(),
            profile: "test".into(),
            workspace: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn runtime(steps: Vec<ScriptStep>, enabled: &[&str]) -> (Arc<LoopRuntime>, Arc<Session>) {
        let llm = Arc::new(ScriptLlm::new(steps));
        let tools = Arc::new(ToolRegistry::new());
        tools.register(Arc::new(EchoTool)).unwrap();
        let gate = Arc::new(ToolGate::new());
        for name in enabled {
            gate.enable(name);
        }
        let store = Arc::new(SessionStore::new());
        let persist: Arc<dyn SessionPersistPort> = Arc::new(InMemoryPersist(std::sync::Mutex::new(vec![])));
        let rt = Arc::new(LoopRuntime {
            llm,
            store: Arc::clone(&store),
            tools,
            gate,
            persist,
            provider: "script".to_string(),
            model: "script-1".to_string(),
        });
        let session = store.create(test_header("s1"), EventBus::new());
        (rt, session)
    }

    fn tool_step(i: u64) -> ScriptStep {
        ScriptStep::Tool {
            name: "echo".to_string(),
            arguments: json!({"x": i}),
            then_text: None,
        }
    }

    // ---------- tests ----------

    /// 纯文本回合：UserMessage → Step Started → AssistantChunk →
    /// AssistantMessage(文本) → Step Ended → Turn Ended。
    #[tokio::test]
    async fn plain_text_turn_sequence() {
        let (rt, session) = runtime(vec![ScriptStep::Text("你好".to_string())], &[]);
        let agent = ReactLoopAgent::new(rt, Arc::clone(&session));
        let outcome = agent.run_turn(Some("hi")).await.unwrap();

        assert_eq!(outcome.steps, 1);
        assert_eq!(outcome.last_text.as_deref(), Some("你好"));

        let events: Vec<SessionEvent> = session.events().into_iter().map(|r| r.event).collect();
        // [0] SessionStarted（Session::new 自动 append）, [1] UserMessage,
        // [2] Step{1,1,Started}, [3] AssistantChunk, [4] AssistantMessage(Text),
        // [5] Step{1,1,Ended}, [6] Turn Ended
        assert_eq!(events.len(), 7);
        assert!(matches!(&events[1], SessionEvent::UserMessage { text } if text.as_str() == "hi"));
        assert!(matches!(
            &events[2],
            SessionEvent::Step { turn: 1, step: 1, phase: StepPhase::Started }
        ));
        assert!(matches!(
            &events[3],
            SessionEvent::AssistantChunk { text } if text.as_str() == "你好"
        ));
        assert!(matches!(
            &events[4],
            SessionEvent::AssistantMessage { content }
                if matches!(&content[..], [ContentBlock::Text(t)] if t.as_str() == "你好")
        ));
        assert!(matches!(
            &events[5],
            SessionEvent::Step { turn: 1, step: 1, phase: StepPhase::Ended }
        ));
        assert!(matches!(&events[6], SessionEvent::Turn(TurnEvent::Ended { turn: 1 })));
    }

    /// 工具回合：ToolCall → ToolResult → 第二轮模型产出文本，steps >= 2。
    #[tokio::test]
    async fn tool_turn_executes_and_continues() {
        let (rt, session) = runtime(
            vec![ScriptStep::Tool {
                name: "echo".to_string(),
                arguments: json!({"x": 1}),
                then_text: Some("完成".to_string()),
            }],
            &["echo"],
        );
        let agent = ReactLoopAgent::new(rt, Arc::clone(&session));
        let outcome = agent.run_turn(Some("你好")).await.unwrap();

        assert!(outcome.steps >= 2, "expected >=2 steps, got {}", outcome.steps);
        assert_eq!(outcome.last_text.as_deref(), Some("完成"));

        let events: Vec<SessionEvent> = session.events().into_iter().map(|r| r.event).collect();
        // [0] SessionStarted, [1] UserMessage, [2] Step{1,1,Started},
        // [3] AssistantMessage([ToolCall]), [4] ToolCall, [5] ToolResult, [6] Step{1,1,Ended},
        // [7] Step{1,2,Started}, [8] AssistantChunk("完成"), [9] AssistantMessage(Text),
        // [10] Step{1,2,Ended}, [11] Turn Ended
        assert!(matches!(
            &events[3],
            SessionEvent::AssistantMessage { content }
                if matches!(&content[..], [ContentBlock::ToolCall(_)])
        ));
        assert!(matches!(
            &events[4],
            SessionEvent::ToolCall { call }
                if call.name.as_str() == "echo" && call.id.as_str() == "call_0"
        ));
        assert!(matches!(
            &events[5],
            SessionEvent::ToolResult { result } if !result.is_error
        ));
        assert!(matches!(
            &events[7],
            SessionEvent::Step { step: 2, phase: StepPhase::Started, .. }
        ));
        assert!(matches!(
            &events[8],
            SessionEvent::AssistantChunk { text } if text.as_str() == "完成"
        ));
        assert!(matches!(
            &events[9],
            SessionEvent::AssistantMessage { content }
                if matches!(&content[..], [ContentBlock::Text(t)] if t.as_str() == "完成")
        ));
        assert!(matches!(
            &events[10],
            SessionEvent::Step { step: 2, phase: StepPhase::Ended, .. }
        ));
        assert!(matches!(&events[11], SessionEvent::Turn(TurnEvent::Ended { turn: 1 })));
    }

    /// fail-closed：不 enable 工具就触发工具调用 → ToolResult.is_error == true。
    #[tokio::test]
    async fn fail_closed_tool_yields_is_error_result() {
        let (rt, session) = runtime(
            vec![ScriptStep::Tool {
                name: "echo".to_string(),
                arguments: json!({"x": 1}),
                then_text: Some("ok".to_string()),
            }],
            &[], // 不 enable 任何工具
        );
        let agent = ReactLoopAgent::new(rt, Arc::clone(&session));
        let outcome = agent.run_turn(Some("hello")).await.unwrap();
        assert!(outcome.steps >= 2);

        let events: Vec<SessionEvent> = session.events().into_iter().map(|r| r.event).collect();
        assert!(events
            .iter()
            .any(|e| matches!(e, SessionEvent::ToolCall { .. })));
        let error_output = events
            .iter()
            .find_map(|e| match e {
                SessionEvent::ToolResult { result } if result.is_error => {
                    Some(result.output.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected an is_error ToolResult event"));
        assert!(
            error_output.contains("disabled") || error_output.contains("not enabled"),
            "unexpected error output: {error_output}"
        );
        // 失败后循环继续到第二轮。
        assert!(events
            .iter()
            .any(|e| matches!(e, SessionEvent::Step { step: 2, .. })));
    }

    /// MaxSteps：脚本给 40 个工具回合（> 32）→ Err(MaxSteps)。
    #[tokio::test]
    async fn max_steps_is_torn() {
        let steps: Vec<ScriptStep> = (0..40).map(tool_step).collect();
        let (rt, session) = runtime(steps, &["echo"]);
        let agent = ReactLoopAgent::new(rt, Arc::clone(&session));
        let err = agent.run_turn(Some("loop")).await.unwrap_err();
        assert!(matches!(err, LoopError::MaxSteps));
    }
}
