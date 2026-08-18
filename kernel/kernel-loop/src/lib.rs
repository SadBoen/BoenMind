//! # kernel-loop
//!
//! 回合循环：turn/step 驱动，waterfall 事件语义。
//!
//! 核心纪律 **model-visible-means-logged**：模型看到的一切都从会话事件日志
//! 投影（`Session::derive_messages`），每次模型调用前日志已完整；
//! 流式块逐块以**原始 chunk** 入日志（`AssistantChunk { chunk }`，对齐 DSH
//! agent-loop：raw chunk 入日志保 replay 保真），工具调用先记 `ToolCall` 再执行。
//!
//! 单回合流程（`run_turn`）：
//! 1. `append(UserMessage)`（可选，恢复续跑时已入日志则传 `None`）；
//! 2. step 循环（上限 [`LoopRuntime::max_steps`]，超限判 torn）：
//!    a. `append(Step Started)`；
//!    b. 投影消息 + enabled 工具 schema；
//!    c. `llm.stream(request)` 逐块消费（原始 chunk 入日志 + `BlockAssembler` 累积）；
//!    d. 流末按 finish 分派：usage 随 `AssistantMessage` 入日志；工具调用 →
//!    `AssistantMessage(ToolCall)` + 逐个 `ToolCall`/`ToolResult`，否则 →
//!    `AssistantMessage(blocks)` 收尾。//! 3. `append(Turn Ended{reason})`——completed / max-tokens / error。
//!
//! 持久化纪律：**logged-means-persisted**——每个事件 append 进会话日志后立即
//! 经 [`LoopRuntime::persist`] 落盘（单事件事务）。kill -9 发生时已 append 的事件
//! 必然已持久化，日志永不出现 torn-tail。

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::StreamExt;
use kernel_contracts::llm::{
    ContentBlock, FinishReason, GenerateOptions, LlmPort, StreamChunk, ToolCall, ToolCallResult,
    TokenUsage,
};
use kernel_contracts::ports::SessionPersistPort;
use kernel_contracts::session::{SessionEvent, StepPhase, TurnEndReason, TurnEvent};
use kernel_contracts::tools::ToolExecutionInput;
use kernel_session::{Session, SessionStore};
use kernel_tools::{ToolGate, ToolRegistry};

/// 单回合最大 step 数默认值；超限即 `LoopError::MaxSteps`（torn）。数值属策略，
/// 装配方可经 `LoopRuntime::max_steps` 覆盖。
pub const DEFAULT_MAX_STEPS: u64 = 32;

/// 回合循环运行时装配：LLM 端口 + 会话存储 + 工具注册表/门控 + provider 标识 + 持久化端口。
pub struct LoopRuntime {
    pub llm: Arc<dyn LlmPort>,
    pub store: Arc<SessionStore>,
    pub tools: Arc<ToolRegistry>,
    pub gate: Arc<ToolGate>,
    pub persist: Arc<dyn SessionPersistPort>,
    pub provider: String,
    pub model: String,
    /// 单回合最大 step 数（默认 [`DEFAULT_MAX_STEPS`]）。
    pub max_steps: u64,
}

/// 单回合结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    /// 实际执行的 step 数。
    pub steps: u64,
    /// 回合结束原因。
    pub reason: TurnEndReason,
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
    #[error("turn exceeded max steps ({0})")]
    MaxSteps(u64),
}

/// 反应式回合代理：一次用户输入 → 若干 step，直到模型产出文本。
pub struct ReactLoopAgent {
    rt: Arc<LoopRuntime>,
    session: Arc<Session>,
    /// per-session 模型覆盖（session.selectModel 语义）：Some((provider, model)) 时
    /// 优先于 LoopRuntime 全局默认。未设置 = 用运行时默认。
    model_override: std::sync::Mutex<Option<(String, String)>>,
    /// 当前活跃回合的取消信号（session.cancel 端口触发；None = 无活跃回合）。
    cancel: std::sync::Mutex<Option<kernel_contracts::AbortSignal>>,
}

/// 增量 chunk → 消息装配器（对齐 DSH `BlockAssembler`：原始 chunk 累积，
/// `block-end` 权威冻结，delta-only 协议也容忍；usage/finish 单独持有）。
struct BlockAssembler {
    partials: BTreeMap<usize, PartialBlock>,
    order: Vec<usize>,
    usage: Option<TokenUsage>,
    finish: Option<FinishReason>,
}

struct PartialBlock {
    block_type: String,
    text: String,
    tool_call_id: String,
    tool_call_name: String,
    /// block-end 已闭：权威块，冻结 partial。
    block: Option<ContentBlock>,
}

impl BlockAssembler {
    fn new() -> Self {
        Self {
            partials: BTreeMap::new(),
            order: Vec::new(),
            usage: None,
            finish: None,
        }
    }

    fn push(&mut self, chunk: &StreamChunk) {
        match chunk {
            StreamChunk::BlockStart { index, block_type } => {
                if !self.partials.contains_key(index) {
                    self.order.push(*index);
                    self.partials.insert(
                        *index,
                        PartialBlock {
                            block_type: block_type.clone(),
                            text: String::new(),
                            tool_call_id: String::new(),
                            tool_call_name: String::new(),
                            block: None,
                        },
                    );
                }
            }
            StreamChunk::TextDelta { index, text } | StreamChunk::ReasoningDelta { index, text } => {
                let is_text = matches!(chunk, StreamChunk::TextDelta { .. });
                let partial = self.ensure(*index, if is_text { "text" } else { "reasoning" });
                if partial.block.is_some() {
                    return; // 已闭：忽略迟到增量
                }
                partial.text.push_str(text);
            }
            StreamChunk::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                let partial = self.ensure(*index, "tool-call");
                if partial.block.is_some() {
                    return;
                }
                if !id.is_empty() {
                    partial.tool_call_id.clone_from(id);
                }
                if let Some(n) = name {
                    partial.tool_call_name.clone_from(n);
                }
                partial.text.push_str(arguments_delta);
            }
            StreamChunk::BlockEnd { index, block } => {
                let partial = self.ensure(*index, "");
                // 首闭胜出：忽略重闭迟到块。
                if partial.block.is_some() {
                    return;
                }
                partial.block = Some(block.clone());
            }
            StreamChunk::Usage(u) => {
                self.usage = Some(u.clone());
            }
            StreamChunk::Finish(reason) => {
                self.finish = Some(reason.clone());
            }
        }
    }

    fn ensure(&mut self, index: usize, block_type: &str) -> &mut PartialBlock {
        if !self.partials.contains_key(&index) {
            self.order.push(index);
        }
        self.partials
            .entry(index)
            .or_insert_with(|| PartialBlock {
                block_type: block_type.to_string(),
                text: String::new(),
                tool_call_id: String::new(),
                tool_call_name: String::new(),
                block: None,
            })
    }

    fn assemble(&self, index: usize, partial: &PartialBlock) -> ContentBlock {
        if let Some(b) = &partial.block {
            return b.clone();
        }
        match partial.block_type.as_str() {
            "text" => ContentBlock::Text(partial.text.clone()),
            "reasoning" => ContentBlock::Reasoning(partial.text.clone()),
            "tool-call" => ContentBlock::ToolCall(ToolCall {
                id: if partial.tool_call_id.is_empty() {
                    format!("call-{index}")
                } else {
                    partial.tool_call_id.clone()
                },
                name: partial.tool_call_name.clone(),
                arguments: partial.text.clone(),
            }),
            _ => ContentBlock::Text(partial.text.clone()),
        }
    }

    /// 组装全部已见块（按开块序）。max-tokens 截断丢弃无法安全执行的 tool-call 块。
    fn blocks(&self) -> Vec<ContentBlock> {
        let blocks: Vec<ContentBlock> = self
            .order
            .iter()
            .map(|i| self.assemble(*i, self.partials.get(i).expect("order invariant")))
            .collect();
        if self.finish() == FinishReason::MaxTokens {
            blocks
                .into_iter()
                .filter(|b| !matches!(b, ContentBlock::ToolCall(_)))
                .collect()
        } else {
            blocks
        }
    }

    fn usage(&self) -> Option<TokenUsage> {
        self.usage.clone()
    }

    /// finish 缺省 stop（对齐 DSH `get finish()`）。
    fn finish(&self) -> FinishReason {
        self.finish.clone().unwrap_or(FinishReason::Stop)
    }
}

impl ReactLoopAgent {
    pub fn new(rt: Arc<LoopRuntime>, session: Arc<Session>) -> Self {
        Self {
            rt,
            session,
            model_override: std::sync::Mutex::new(None),
            cancel: std::sync::Mutex::new(None),
        }
    }

    /// 触发当前活跃回合的取消（对齐 DSH `session.cancel`：请求 signal abort →
    /// 流以 finish{kind:'aborted', code:'ABORTED'} 收尾）。无活跃回合时无效果。
    pub fn abort(&self) {
        let signal = self.cancel.lock().unwrap().clone();
        if let Some(s) = signal {
            s.abort();
        }
    }

    pub fn session(&self) -> Arc<Session> {
        Arc::clone(&self.session)
    }

    /// 设置本会话的模型选择（provider, model）。之后 run_turn 的 GenerateOptions
    /// 携带该 provider/model，MultiProviderLlm 据此路由到对应通道。
    pub fn set_model_override(&self, provider: impl Into<String>, model: impl Into<String>) {
        *self.model_override.lock().unwrap() = Some((provider.into(), model.into()));
    }

    /// 当前会话的模型覆盖（None = 用运行时默认）。
    pub fn model_override(&self) -> Option<(String, String)> {
        self.model_override.lock().unwrap().clone()
    }

    pub fn clear_model_override(&self) {
        *self.model_override.lock().unwrap() = None;
    }

    /// 本次请求实际使用的 (provider, model)：override 优先，否则运行时默认。
    fn request_provider_model(&self) -> (String, String) {
        self.model_override
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| (self.rt.provider.clone(), self.rt.model.clone()))
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
    /// 回合结束（含 error 收尾）必写 `Turn Ended{reason}`——对齐 DSH 断连恢复语义。
    pub async fn run_turn(&self, user_text: Option<&str>) -> Result<TurnOutcome, LoopError> {
        let turn = self.next_turn();

        if let Some(text) = user_text {
            let rec = self.session.append(SessionEvent::UserMessage {
                text: text.to_string(),
            });
            self.persist(&rec).await?;
        }

        let mut steps: u64 = 0;

        // 本回合取消信号：session.cancel 经 agent.abort() 触发，贯穿整个回合
        // （多 step 共用同一信号——对齐 DSH 请求 signal 语义）。
        let signal = kernel_contracts::AbortSignal::new();
        *self.cancel.lock().unwrap() = Some(signal.clone());
        // 回合结束（任意 return 路径）清空取消槽：避免残留信号被下一回合复用。
        struct ClearCancel<'a>(&'a std::sync::Mutex<Option<kernel_contracts::AbortSignal>>);
        impl Drop for ClearCancel<'_> {
            fn drop(&mut self) {
                *self.0.lock().unwrap() = None;
            }
        }
        let _clear = ClearCancel(&self.cancel);

        loop {
            steps += 1;
            if steps > self.rt.max_steps {
                let rec = self
                    .session
                    .append(SessionEvent::Turn(TurnEvent::Ended {
                        turn,
                        reason: TurnEndReason::Error {
                            message: format!("turn exceeded max steps ({})", self.rt.max_steps),
                            code: "MAX_STEPS".to_string(),
                        },
                    }));
                let _ = self.persist(&rec).await;
                return Err(LoopError::MaxSteps(self.rt.max_steps));
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
            let (provider, model) = self.request_provider_model();

            let request = GenerateOptions {
                provider,
                model,
                messages,
                tools,
                temperature: None,
                max_tokens: None,
                session_id: Some(self.session.id().as_str().to_string()),
                signal: Some(signal.clone()),
                reasoning_effort: None,
                thinking: None,
                purpose: None,
            };

            // ---- 消费模型流：原始 chunk 入日志 + 装配器累积 ----
            let mut stream = self.rt.llm.stream(request);
            let mut assembler = BlockAssembler::new();

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    // torn 流：禁止静默中断。
                    Err(e) => {
                        let rec = self
                            .session
                            .append(SessionEvent::Turn(TurnEvent::Ended {
                                turn,
                                reason: TurnEndReason::Error {
                                    message: e.message.clone(),
                                    code: "LLM_STREAM".to_string(),
                                },
                            }));
                        let _ = self.persist(&rec).await;
                        return Err(LoopError::Llm(e.message));
                    }
                };
                // raw chunk 入日志（对齐 DSH agent-loop：`session.append('assistant/chunk',
                // {turn, step, chunk}).seq` 保 replay 保真）。
                let rec = self.session.append(SessionEvent::AssistantChunk {
                    chunk: chunk.clone(),
                });
                self.persist(&rec).await?;
                assembler.push(&chunk);
            }

            let finish = assembler.finish();
            let usage = assembler.usage();

            // finish 缺失 = torn（端口契约要求流以 Finish 收尾）。
            if assembler.finish.is_none() {
                let rec = self
                    .session
                    .append(SessionEvent::Turn(TurnEvent::Ended {
                        turn,
                        reason: TurnEndReason::Error {
                            message: "stream ended without Finish (torn)".to_string(),
                            code: "STREAM_CLOSED".to_string(),
                        },
                    }));
                let _ = self.persist(&rec).await;
                return Err(LoopError::Llm(
                    "stream ended without Finish (torn)".to_string(),
                ));
            }

            // error/aborted finish：回合收尾（无重试瀑布，M3 直错）。
            if matches!(finish, FinishReason::Error { .. } | FinishReason::Cancelled) {
                match &finish {
                    // 取消请求中断回合 → TurnEndReason::Aborted（对齐 DSH：aborted
                    // 是独立 reason 词汇，不再归 Error{ABORTED}）。
                    FinishReason::Cancelled => {
                        let rec = self
                            .session
                            .append(SessionEvent::Turn(TurnEvent::Ended {
                                turn,
                                reason: TurnEndReason::Aborted {
                                    reason: "request cancelled".to_string(),
                                },
                            }));
                        let _ = self.persist(&rec).await;
                        return Err(LoopError::Llm(
                            "model finish: cancelled (ABORTED)".to_string(),
                        ));
                    }
                    FinishReason::Error { message, code, .. } => {
                        let rec = self
                            .session
                            .append(SessionEvent::Turn(TurnEvent::Ended {
                                turn,
                                reason: TurnEndReason::Error {
                                    message: message.clone(),
                                    code: code.clone(),
                                },
                            }));
                        let _ = self.persist(&rec).await;
                        return Err(LoopError::Llm(format!(
                            "model finish: {message} ({code})"
                        )));
                    }
                    _ => unreachable!("guarded by matches!"),
                }
            }

            // 组装最终消息块（max-tokens 截断丢弃 tool-call 块，对齐 DSH assembler.blocks()）。
            let blocks = assembler.blocks();
            let tool_calls: Vec<ToolCall> = blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolCall(c) => Some(c.clone()),
                    _ => None,
                })
                .collect();

            // 模型即将看到的内容：工具调用或文本块。
            let rec = self.session.append(SessionEvent::AssistantMessage {
                content: if tool_calls.is_empty() { blocks } else {
                    tool_calls.iter().cloned().map(ContentBlock::ToolCall).collect()
                },
                usage,
            });
            self.persist(&rec).await?;

            if finish == FinishReason::MaxTokens {
                let rec = self.session.append(SessionEvent::Step {
                    turn,
                    step: steps,
                    phase: StepPhase::Ended,
                });
                self.persist(&rec).await?;
                let rec = self
                    .session
                    .append(SessionEvent::Turn(TurnEvent::Ended {
                        turn,
                        reason: TurnEndReason::MaxTokens,
                    }));
                self.persist(&rec).await?;
                return Ok(TurnOutcome {
                    steps,
                    reason: TurnEndReason::MaxTokens,
                });
            }

            if tool_calls.is_empty() {
                // 文本收尾（stop）。
                let rec = self.session.append(SessionEvent::Step {
                    turn,
                    step: steps,
                    phase: StepPhase::Ended,
                });
                self.persist(&rec).await?;
                let rec = self
                    .session
                    .append(SessionEvent::Turn(TurnEvent::Ended {
                        turn,
                        reason: TurnEndReason::Completed,
                    }));
                self.persist(&rec).await?;
                return Ok(TurnOutcome {
                    steps,
                    reason: TurnEndReason::Completed,
                });
            }

            // 工具调用序列：逐个执行。
            for call in &tool_calls {
                // 先记调用，再执行（对齐 dsh tool/call 事件语义）。
                let rec = self
                    .session
                    .append(SessionEvent::ToolCall { call: call.clone() });
                self.persist(&rec).await?;
                let input = ToolExecutionInput {
                    name: call.name.clone(),
                    // 模型原始 JSON 文本解析为 Value 供 schema 校验；解析失败 → Null。
                    arguments: serde_json::from_str(&call.arguments)
                        .unwrap_or(serde_json::Value::Null),
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

    /// 脚本式 LLM：每次 `stream()` 弹出队首一步（按 DSH 流协议产出块）。
    #[derive(Clone)]
    enum ScriptStep {
        /// 一步模型调用 → `[BlockStart(text), TextDelta, BlockEnd, Finish(Stop)]`。
        Text(String),
        /// 一步模型调用 → `[BlockStart(tool-call), ToolCallDelta, BlockEnd, Finish(ToolCalls)]`；
        /// `then_text` 为下一轮调用的文本（预插队）。
        Tool {
            name: String,
            arguments: serde_json::Value,
            then_text: Option<String>,
        },
        /// 一步模型调用 → `[Usage, Finish(MaxTokens)]`（无块）。
        MaxTokens,
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
                    Ok(StreamChunk::BlockStart { index: 0, block_type: "text".to_string() }),
                    Ok(StreamChunk::TextDelta { index: 0, text: text.clone() }),
                    Ok(StreamChunk::BlockEnd { index: 0, block: ContentBlock::Text(text) }),
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
                        Ok(StreamChunk::BlockStart { index: 0, block_type: "tool-call".to_string() }),
                        Ok(StreamChunk::ToolCallDelta {
                            index: 0,
                            id: "call_0".to_string(),
                            name: Some(name.clone()),
                            arguments_delta: args.clone(),
                        }),
                        Ok(StreamChunk::BlockEnd {
                            index: 0,
                            block: ContentBlock::ToolCall(ToolCall {
                                id: "call_0".to_string(),
                                name,
                                arguments: args,
                            }),
                        }),
                        Ok(StreamChunk::Finish(FinishReason::ToolCalls)),
                    ]))
                }
                ScriptStep::MaxTokens => Box::pin(futures::stream::iter(vec![
                    Ok(StreamChunk::Usage(TokenUsage {
                        input: 10,
                        output: 5,
                        cache_read: None,
                        cache_write: None,
                        reasoning: None,
                    })),
                    Ok(StreamChunk::Finish(FinishReason::MaxTokens)),
                ])),
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
                context_window: None,
                max_tokens: None,
                reasoning: None,
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
            max_steps: DEFAULT_MAX_STEPS,
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

    /// 从日志找最后一个指定事件（辅助断言）。
    fn last_turn_end(events: &[SessionEvent]) -> &TurnEndReason {
        events
            .iter()
            .rev()
            .find_map(|e| match e {
                SessionEvent::Turn(TurnEvent::Ended { reason, .. }) => Some(reason),
                _ => None,
            })
            .expect("turn/end present")
    }

    // ---------- tests ----------

    /// 纯文本回合：UserMessage → Step Started → AssistantChunk(原始块) →
    /// AssistantMessage(文本) → Step Ended → Turn Ended(completed)。
    #[tokio::test]
    async fn plain_text_turn_sequence() {
        let (rt, session) = runtime(vec![ScriptStep::Text("你好".to_string())], &[]);
        let agent = ReactLoopAgent::new(rt, Arc::clone(&session));
        let outcome = agent.run_turn(Some("hi")).await.unwrap();

        assert_eq!(outcome.steps, 1);
        assert_eq!(outcome.reason, TurnEndReason::Completed);

        let events: Vec<SessionEvent> = session.events().into_iter().map(|r| r.event).collect();
        // [0] SessionStarted, [1] UserMessage, [2] Step{1,1,Started},
        // [3..6] AssistantChunk（BlockStart/TextDelta/BlockEnd/Finish 四块——对齐 DSH：
        //        agent-loop 对流的每个 chunk（含 finish）都 append assistant/chunk 保 replay 保真）,
        // [7] AssistantMessage(Text), [8] Step Ended, [9] Turn Ended
        assert_eq!(events.len(), 10);
        assert!(matches!(&events[1], SessionEvent::UserMessage { text } if text.as_str() == "hi"));
        assert!(matches!(
            &events[2],
            SessionEvent::Step { turn: 1, step: 1, phase: StepPhase::Started }
        ));
        assert!(matches!(
            &events[3],
            SessionEvent::AssistantChunk { chunk }
                if matches!(chunk, StreamChunk::BlockStart { index: 0, block_type } if block_type == "text")
        ));
        assert!(matches!(
            &events[4],
            SessionEvent::AssistantChunk { chunk }
                if matches!(chunk, StreamChunk::TextDelta { index: 0, text } if text == "你好")
        ));
        assert!(matches!(
            &events[5],
            SessionEvent::AssistantChunk { chunk }
                if matches!(chunk, StreamChunk::BlockEnd { index: 0, .. })
        ));
        assert!(matches!(
            &events[6],
            SessionEvent::AssistantChunk { chunk }
                if matches!(chunk, StreamChunk::Finish(FinishReason::Stop))
        ));
        assert!(matches!(
            &events[7],
            SessionEvent::AssistantMessage { content, usage: None }
                if matches!(&content[..], [ContentBlock::Text(t)] if t.as_str() == "你好")
        ));
        assert!(matches!(
            &events[8],
            SessionEvent::Step { turn: 1, step: 1, phase: StepPhase::Ended }
        ));
        assert!(matches!(
            &events[9],
            SessionEvent::Turn(TurnEvent::Ended { turn: 1, reason: TurnEndReason::Completed })
        ));
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
        assert_eq!(outcome.reason, TurnEndReason::Completed);

        let events: Vec<SessionEvent> = session.events().into_iter().map(|r| r.event).collect();
        // [0] SessionStarted, [1] UserMessage, [2] Step{1,1,Started},
        // [3..6] AssistantChunk（tool-call 块 + finish）, [7] AssistantMessage([ToolCall]),
        // [8] ToolCall, [9] ToolResult, [10] Step{1,1,Ended},
        // [11] Step{1,2,Started}, [12..15] AssistantChunk（文本块 + finish）,
        // [16] AssistantMessage(Text), [17] Step{1,2,Ended}, [18] Turn Ended
        let chunk_seq: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                SessionEvent::AssistantChunk { chunk } => Some(chunk.to_wire()["type"].as_str().unwrap_or("").to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            chunk_seq,
            vec![
                "block-start", "tool-call-delta", "block-end", "finish",
                "block-start", "text-delta", "block-end", "finish",
            ]
        );
        assert!(matches!(
            &events[7],
            SessionEvent::AssistantMessage { content, .. }
                if matches!(&content[..], [ContentBlock::ToolCall(_)])
        ));
        assert!(matches!(
            &events[8],
            SessionEvent::ToolCall { call }
                if call.name.as_str() == "echo" && call.id.as_str() == "call_0"
        ));
        assert!(matches!(
            &events[9],
            SessionEvent::ToolResult { result } if !result.is_error
        ));
        let text_msg = events.iter().find_map(|e| match e {
            SessionEvent::AssistantMessage { content, .. }
                if matches!(&content[..], [ContentBlock::Text(_)]) =>
            {
                Some(content.clone())
            }
            _ => None,
        });
        assert!(matches!(&text_msg, Some(c) if matches!(&c[..], [ContentBlock::Text(t)] if t.as_str() == "完成")));
        assert!(matches!(
            &events[events.len() - 1],
            SessionEvent::Turn(TurnEvent::Ended { turn: 1, reason: TurnEndReason::Completed })
        ));
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

    /// MaxSteps：脚本给 40 个工具回合（> 32）→ Err(MaxSteps) + turn/end(MAX_STEPS)。
    #[tokio::test]
    async fn max_steps_is_torn() {
        let steps: Vec<ScriptStep> = (0..40).map(tool_step).collect();
        let (rt, session) = runtime(steps, &["echo"]);
        let agent = ReactLoopAgent::new(rt, Arc::clone(&session));
        let err = agent.run_turn(Some("loop")).await.unwrap_err();
        assert!(matches!(err, LoopError::MaxSteps(_)));
        // 回合仍以 error reason 收尾（断连恢复语义：日志无 torn tail）。
        let events: Vec<SessionEvent> = session.events().into_iter().map(|r| r.event).collect();
        assert!(matches!(
            last_turn_end(&events),
            TurnEndReason::Error { code, .. } if code == "MAX_STEPS"
        ));
    }

    /// max-tokens 回合：usage 随 AssistantMessage、turn/end reason = max-tokens。
    #[tokio::test]
    async fn max_tokens_turn_records_usage_and_reason() {
        let (rt, session) = runtime(vec![ScriptStep::MaxTokens], &[]);
        let agent = ReactLoopAgent::new(rt, Arc::clone(&session));
        let outcome = agent.run_turn(Some("long")).await.unwrap();
        assert_eq!(outcome.reason, TurnEndReason::MaxTokens);

        let events: Vec<SessionEvent> = session.events().into_iter().map(|r| r.event).collect();
        let msg = events
            .iter()
            .find_map(|e| match e {
                SessionEvent::AssistantMessage { content, usage } => Some((content, usage)),
                _ => None,
            })
            .expect("assistant message present");
        assert_eq!(msg.0.len(), 0); // 无块
        assert_eq!(msg.1, &Some(TokenUsage { input: 10, output: 5, cache_read: None, cache_write: None, reasoning: None }));
        assert!(matches!(
            last_turn_end(&events),
            TurnEndReason::MaxTokens
        ));
    }

    /// 记录最近一次 GenerateOptions 的 provider/model，验证 override 路由生效。
    #[derive(Clone, Default)]
    struct CapturingLlm {
        seen: Arc<std::sync::Mutex<Vec<(String, String)>>>,
    }

    #[async_trait::async_trait]
    impl LlmPort for CapturingLlm {
        async fn list_models(&self, _provider: &str) -> Result<Vec<LlmModelInfo>, LlmError> {
            Ok(vec![])
        }
        fn stream(&self, request: GenerateOptions) -> ChunkStream {
            self.seen.lock().unwrap().push((request.provider, request.model));
            Box::pin(futures::stream::iter(vec![
                Ok(StreamChunk::BlockStart { index: 0, block_type: "text".to_string() }),
                Ok(StreamChunk::TextDelta { index: 0, text: "ok".to_string() }),
                Ok(StreamChunk::BlockEnd { index: 0, block: ContentBlock::Text("ok".to_string()) }),
                Ok(StreamChunk::Finish(FinishReason::Stop)),
            ]))
        }
    }

    #[tokio::test]
    async fn model_override_routes_generate_options() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let llm = Arc::new(CapturingLlm { seen: Arc::clone(&seen) });
        let tools = Arc::new(ToolRegistry::new());
        let gate = Arc::new(ToolGate::new());
        let store = Arc::new(SessionStore::new());
        let persist: Arc<dyn SessionPersistPort> =
            Arc::new(InMemoryPersist(std::sync::Mutex::new(vec![])));
        let rt = Arc::new(LoopRuntime {
            llm,
            store: Arc::clone(&store),
            tools,
            gate,
            persist,
            provider: "script".to_string(),
            model: "script-1".to_string(),
            max_steps: DEFAULT_MAX_STEPS,
        });
        let session = store.create(test_header("s1"), EventBus::new());
        let agent = ReactLoopAgent::new(rt, Arc::clone(&session));
        // 覆盖为其他 provider/model。
        agent.set_model_override("minimax", "MiniMax-M3");
        agent.run_turn(Some("hi")).await.unwrap();
        let seen1 = seen.lock().unwrap().clone();
        assert_eq!(seen1.len(), 1);
        assert_eq!(seen1[0], ("minimax".to_string(), "MiniMax-M3".to_string()));
        // 清除后回落运行时默认。
        agent.clear_model_override();
        agent.run_turn(Some("hi2")).await.unwrap();
        let seen2 = seen.lock().unwrap().clone();
        assert_eq!(seen2[1], ("script".to_string(), "script-1".to_string()));
    }
}
