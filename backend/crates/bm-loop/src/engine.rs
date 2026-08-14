//! ReactLoopAgent：turn/step 双层自研循环（A6 主体）。
//!
//! 循环形态：
//! ```text
//! run_turn(req, cancel)
//!   ├─ UserMessage 落日志（loop 拥有回合全生命周期，集成方不得重复落）
//!   ├─ RequestHeader（prompt_hash = 完整模型可见输入哈希，A2 升级版）
//!   ├─ TurnStart
//!   └─ for step in 1..=max_steps:
//!        ├─ StepStart
//!        ├─ 投影（EventLog::derive_messages）→ OpenAI messages payload
//!        ├─ 硬触发检查（单步输入超窗 → 有压缩插件则压缩 → 仍超窗即回合失败）
//!        ├─ on_pre_step / on_request（扩展点可改写 payload）
//!        ├─ LLM 流式：chunk 经 EventFlusher 真序落日志（攒批 append_batch）
//!        ├─ AssistantMessage（权威内容 + 步内 usage）
//!        ├─ 工具调用 → ToolCall → on_tool_pre（可 Deny）→ 执行（cancel 可打断）
//!        │   → ToolResult → on_tool_post；无工具调用 = 回合收尾
//!        ├─ 软触发检查（步边界，压缩插件判定；无插件不动作）
//!        └─ on_turn_stopping（附加收尾条件）
//!   └─ TurnEnd（completed/cancelled/failed）
//! ```
//!
//! 与 pi 路径的关系：`bm-server/chat.rs` 当前用 pi SDK 回调 + LogItem 队列
//! 真序落日志（A1）；本 loop 落位后 pi loop 与新 loop 并行双开对比（拍板点 4），
//! 对比通过后 chat.rs 切换到本 loop（开关时机由用户拍板）。
//!
//! 取消语义：`cancel`（watch::Receiver<bool>）在流阶段与工具执行阶段都生效；
//! 中断后部分文本照常落日志，TurnEnd 记 Cancelled（对齐 pi 路径收尾语义）。

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bm_kernel::{EventLog, SurfaceIntent};
use bm_protocol::{
    AssistantMsg, BranchId, CallId, CoreEvent, EpochHeader, EventKind, HeaderReason, SeqNo,
    SessionId, StreamChunk, TokenUsage, ToolResultMsg, TurnEndReason, UserMsg,
};
use tokio::sync::{Notify, mpsc, watch};

use crate::compact::{compact, estimate_tokens, Compactor};
use crate::llm::{Llm, LlmError, LlmEvent, LlmRequest, LlmUsage};
use crate::model::ToolRegistry;
use crate::points::{LoopHooks, RequestCtx, StepCtx, StopCtx, ToolCtx, ToolGate};

/// 待处理回合（用户/目标输入）。
#[derive(Debug, Clone)]
pub struct TurnRequest {
    pub content: String,
    pub source: bm_protocol::UserMsgSource,
}

/// 回合内待执行步骤（目标驱动/继续指令注入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepRequest {
    pub turn: u32,
}

/// loop 运行错误（日志失败 / 超窗无法压缩 / 模型调用链不可恢复失败）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunError {
    pub message: String,
}

impl RunError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RunError {}

/// loop 配置（集成方从 bm-core 配置换算注入；本 crate 不依赖 bm-core）。
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// 系统提示（BoenMind 注入面：自定义提示 + skills + 工具代理说明）
    pub system_prompt: String,
    /// 提供商标识（request/header 审计）
    pub provider: Option<String>,
    /// 模型名
    pub model: String,
    /// 模型上下文窗口（token，客观属性——硬触发兜底的判定基准，与插件无关）
    pub context_window: u32,
    /// 单回合步数上限（防呆；超出记 Failed——配额到点不是正常完成）
    pub max_steps: u32,
    /// 压缩策略插件；None = 关闭压缩（软触发不动作、超窗即失败回合——
    /// 缺插件优雅失败：不崩、不静默丢历史）
    pub compactor: Option<Arc<dyn Compactor>>,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            system_prompt: String::new(),
            provider: None,
            model: "default-model".into(),
            context_window: 128_000,
            max_steps: 64,
            // 默认裸跑（loop 库不依赖任何插件 crate；组装层挂压缩插件）
            compactor: None,
        }
    }
}

/// 工具执行请求（B4：QuickJS 引擎经此接入——ToolRegistry 定接口，
/// 执行侧实现本 trait）。
#[derive(Debug, Clone)]
pub struct ToolCallRequest {
    pub call_id: String,
    pub name: String,
    /// 已解析参数（解析失败时为 Null，原串见 ToolCall 事件 args）
    pub args: serde_json::Value,
}

/// 工具执行结果。
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub ok: bool,
    pub output: String,
    pub meta: Option<serde_json::Value>,
}

/// 工具执行端口。RPITIT 显式 Send 约束（实现侧用 async fn 即可）。
pub trait ToolExecutor: Send + Sync {
    fn execute(
        &self,
        req: ToolCallRequest,
    ) -> impl std::future::Future<Output = ToolOutcome> + Send;
}

/// 回合结果。
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub turn: u32,
    pub steps: u32,
    pub reason: TurnEndReason,
    /// 最后一个 assistant 文本（无助手文本时为空）
    pub final_text: String,
    /// 回合累计 token 用量（无任何 usage 时为 None）
    pub usage: Option<LlmUsage>,
    /// 执行过的工具次数
    pub tool_calls_executed: usize,
}

/// 自研 loop：inbox 双队列 + turn/step 位置 + run 循环（A6 主体）。
pub struct ReactLoopAgent<H: LoopHooks, L: Llm, T: ToolExecutor> {
    hooks: H,
    tools: ToolRegistry,
    /// 事件日志（loop 的全部状态源：投影读、事件写）
    log: EventLog,
    session_id: SessionId,
    branch_id: BranchId,
    config: LoopConfig,
    llm: L,
    executor: T,
    /// inbox 双队列：next-turn（回合级）/ next-step（回合内步骤级）
    turn_queue: VecDeque<TurnRequest>,
    step_queue: VecDeque<StepRequest>,
    /// 当前位置（turn, step）；None = 空闲
    current: Option<(u32, u32)>,
    /// 回合计数（进程内镜像；恢复时以日志 TurnStart 计数为准）
    turn_count: u32,
}

impl<H: LoopHooks, L: Llm, T: ToolExecutor> ReactLoopAgent<H, L, T> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        hooks: H,
        tools: ToolRegistry,
        log: EventLog,
        session_id: SessionId,
        branch_id: BranchId,
        config: LoopConfig,
        llm: L,
        executor: T,
    ) -> Self {
        Self {
            hooks,
            tools,
            log,
            session_id,
            branch_id,
            config,
            llm,
            executor,
            turn_queue: VecDeque::new(),
            step_queue: VecDeque::new(),
            current: None,
            turn_count: 0,
        }
    }

    /// 入队一个回合（next-turn 队列）。
    pub fn enqueue_turn(&mut self, req: TurnRequest) {
        self.turn_queue.push_back(req);
    }

    /// 入队一个步骤（next-step 队列：回合内注入的继续指令）。
    pub fn enqueue_step(&mut self, req: StepRequest) {
        self.step_queue.push_back(req);
    }

    /// 待处理回合数。
    pub fn pending_turns(&self) -> usize {
        self.turn_queue.len()
    }

    /// 待处理步骤数。
    pub fn pending_steps(&self) -> usize {
        self.step_queue.len()
    }

    /// 当前位置（(turn, step)，None = 空闲）。
    pub fn current_position(&self) -> Option<(u32, u32)> {
        self.current
    }

    /// 工具注册表（B4：QuickJS 引擎的工具在此汇合）。
    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    pub fn tools_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tools
    }

    /// 扩展点访问（插件挂点）。
    pub fn hooks(&mut self) -> &mut H {
        &mut self.hooks
    }

    /// 事件日志句柄（恢复/订阅场景）。
    pub fn event_log(&self) -> &EventLog {
        &self.log
    }

    /// loop 配置可变访问（步数上限/压缩策略调参）。
    pub fn config_mut(&mut self) -> &mut LoopConfig {
        &mut self.config
    }

    /// 以日志为准设定位回合（恢复语义：进程计数器不跨重启）。
    pub fn begin_turn_at(&mut self, turn: u32) {
        self.turn_count = self.turn_count.max(turn);
        self.current = Some((turn, 0));
    }

    /// 步进到下一步（step + 1）。
    fn advance_step(&mut self) -> (u32, u32) {
        let (turn, step) = self.current.get_or_insert((self.turn_count.max(1), 0));
        *step += 1;
        (*turn, *step)
    }

    /// 回合收尾：清当前位置。
    fn end_turn(&mut self) {
        self.current = None;
    }

    /// 驱动：按序执行全部待处理回合（run_turn）。
    pub async fn run(
        &mut self,
        reason: HeaderReason,
        cancel: watch::Receiver<bool>,
    ) -> Vec<Result<TurnOutcome, RunError>> {
        let mut out = Vec::new();
        while let Some(req) = self.turn_queue.pop_front() {
            let mut c = cancel.clone();
            out.push(self.run_turn(req, reason, &mut c).await);
        }
        out
    }

    /// 跑一个回合（用户输入 → 收尾事件）。集成方必须先落 UserMessage
    /// 吗？**不需要**——loop 拥有回合全生命周期，UserMessage 在此落。
    pub async fn run_turn(
        &mut self,
        req: TurnRequest,
        reason: HeaderReason,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<TurnOutcome, RunError> {
        let sid = self.session_id.clone();
        let bid = self.branch_id.clone();

        // 回合号：日志 TurnStart 计数 + 1（恢复后进程计数不准确，以日志为准）
        let turn = self
            .log
            .count(&sid, &bid, Some("turn/start"))
            .await
            .map_err(|e| RunError::new(format!("turn 计数失败: {e}")))?
            as u32
            + 1;
        self.begin_turn_at(turn);

        let flusher = EventFlusher::new(EventLog::new(self.log.store()), sid.clone(), bid.clone());

        // 用户消息 → 日志（模型可见输入的一部分）
        flusher.push(
            EventKind::Core(CoreEvent::UserMessage {
                msg: UserMsg {
                    content: req.content.clone(),
                },
                source: req.source,
            }),
            SurfaceIntent::Append,
        );
        // 读回自己的写入前必须冲刷（prompt_hash 覆盖用户消息）
        flusher.flush().await.map_err(|e| RunError::new(format!("事件日志写入失败: {e}")))?;

        // 首步 payload（prompt_hash 需要完整输入；随后每步重建）
        let mut msgs = self
            .log
            .derive_messages(&sid, &bid)
            .await
            .map_err(|e| RunError::new(format!("投影失败: {e}")))?;
        let mut payload = self.build_payload(&msgs);
        let prompt_hash = prompt_hash_of_parts(&[
            &self.config.system_prompt,
            &serde_json::to_string(&self.tools.openai_tools_json()).unwrap_or_default(),
            &serde_json::to_string(&payload).unwrap_or_default(),
        ]);

        // —— A2 升级：request/header（prompt_hash 覆盖完整模型可见输入）——
        flusher.push(
            EventKind::Core(CoreEvent::RequestHeader {
                header: EpochHeader {
                    provider: self.config.provider.clone(),
                    model: Some(self.config.model.clone()),
                    created_at: now_ms(),
                    prompt_hash: Some(prompt_hash.clone()),
                },
                reason,
            }),
            SurfaceIntent::None,
        );
        flusher.push(EventKind::Core(CoreEvent::TurnStart { turn }), SurfaceIntent::None);

        // —— 步循环 ——
        let mut step = 0u32;
        let mut final_text = String::new();
        let mut usage_total: Option<LlmUsage> = None;
        let mut tool_calls_executed = 0usize;
        let mut reason_out = TurnEndReason::Completed;
        let mut fail_msg: Option<String> = None;
        // 上一请求的真实模型可见输入（usage 校准源：粗估 chars/4 对中文
        // 低估约 2 倍，压缩水线判定以 max(粗估, 真实值) 为准）
        let mut last_real_input = 0u64;

        loop {
            // 取消：回合中途用户停止
            if *cancel.borrow() {
                reason_out = TurnEndReason::Cancelled;
                break;
            }
            // 步数上限：最多执行 max_steps 步（先检查后递增）
            if step >= self.config.max_steps {
                reason_out = TurnEndReason::Failed;
                fail_msg = Some(format!("步数超上限（{}）", self.config.max_steps));
                break;
            }
            step += 1;
            let (t, s) = self.advance_step();
            debug_assert_eq!((t, s), (turn, step));
            flusher.push(EventKind::Core(CoreEvent::StepStart { turn, step }), SurfaceIntent::None);
            self.hooks.on_pre_step(&StepCtx { turn, step });

            // 每步重建投影与 payload（上一步工具结果已入日志）——
            // 先冲刷写线程，保证投影看到本回合已 push 的全部事件
            flusher.flush().await.map_err(|e| RunError::new(format!("事件日志写入失败: {e}")))?;
            msgs = self
                .log
                .derive_messages(&sid, &bid)
                .await
                .map_err(|e| RunError::new(format!("投影失败: {e}")))?;
            payload = self.build_payload(&msgs);

            // 硬触发：单步输入超窗 → 压缩 → 重建；仍超窗即失败
            if self.check_overflow(&msgs, last_real_input) {
                tracing::warn!(event = "bm.loop_overflow", turn, step, "单步输入超窗，硬触发压缩");
                self.compact_or_die(turn, cancel).await?;
                msgs = self
                    .log
                    .derive_messages(&sid, &bid)
                    .await
                    .map_err(|e| RunError::new(format!("投影失败: {e}")))?;
                payload = self.build_payload(&msgs);
                if self.check_overflow(&msgs, last_real_input) {
                    reason_out = TurnEndReason::Failed;
                    fail_msg = Some("压缩后输入仍超窗口".into());
                    break;
                }
            }

            self.hooks
                .on_request(&RequestCtx { turn, step, prompt_hash: Some(prompt_hash.clone()) }, &mut payload);

            // —— 模型流（可重试两次；cancel 可打断）——
            let mut content = String::new();
            let mut step_tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, args)
            let mut step_usage: Option<LlmUsage> = None;
            let mut attempt = 0u32;
            loop {
                attempt += 1;
                let stream = self.llm.stream_chat(LlmRequest { payload: payload.clone() });
                tokio::pin!(stream);
                let mut stream_err: Option<LlmError> = None;
                loop {
                    tokio::select! {
                        _ = cancel.changed() => {
                            // 流阶段取消：部分文本照常入库
                            reason_out = TurnEndReason::Cancelled;
                            break;
                        }
                        next = tokio_stream::StreamExt::next(&mut stream) => {
                            match next {
                                Some(Ok(ev)) => match ev {
                                    LlmEvent::TextDelta { text } => {
                                        content.push_str(&text);
                                        flusher.push(
                                            EventKind::Core(CoreEvent::AssistantChunk {
                                                turn,
                                                step,
                                                chunk: StreamChunk { text: text.clone() },
                                            }),
                                            SurfaceIntent::Append,
                                        );
                                        // SSE 前端流式通道（与日志同源同序）
                                        self.hooks.on_stream_chunk(&StepCtx { turn, step }, &text);
                                    }
                                    LlmEvent::ToolCallStart { id, name } => {
                                        step_tool_calls.push((id, name, String::new()));
                                    }
                                    LlmEvent::ToolCallArgs { id, args_delta } => {
                                        if let Some(entry) = step_tool_calls.iter_mut().find(|(cid, _, _)| *cid == id) {
                                            entry.2.push_str(&args_delta);
                                        }
                                    }
                                    LlmEvent::ToolCallEnd { id, arguments } => {
                                        if let Some(entry) = step_tool_calls.iter_mut().find(|(cid, _, _)| *cid == id) {
                                            entry.2 = arguments;
                                        }
                                    }
                                    LlmEvent::MessageEnd { content: authoritative, tool_calls, usage } => {
                                        if !authoritative.is_empty() {
                                            content = authoritative;
                                        }
                                        // 工具调用去重：流式路径已由 ToolCallStart/End 计入，
                                        // MessageEnd 的清单只补非流式兜底缺失的条目
                                        for tc in tool_calls {
                                            if !step_tool_calls.iter().any(|(cid, _, _)| *cid == tc.id) {
                                                step_tool_calls.push((tc.id, tc.name, tc.arguments));
                                            }
                                        }
                                        step_usage = usage;
                                        break; // 收口事件 = 流终结
                                    }
                                },
                                Some(Err(e)) => {
                                    stream_err = Some(e);
                                    break;
                                }
                                None => break,
                            }
                        }
                    }
                }
                if reason_out == TurnEndReason::Cancelled {
                    break;
                }
                match stream_err {
                    Some(e) if e.retryable && attempt < 2 => {
                        // 扩展点裁决：true = 重试本步，false = 回合失败
                        let retry = self
                            .hooks
                            .on_request_error(&RequestCtx { turn, step, prompt_hash: Some(prompt_hash.clone()) }, &e.message);
                        if retry {
                            tracing::warn!(event = "bm.loop_request_retry", turn, step, attempt, error = %e.message);
                            continue;
                        }
                        reason_out = TurnEndReason::Failed;
                        fail_msg = Some(e.message);
                        break;
                    }
                    Some(e) => {
                        reason_out = TurnEndReason::Failed;
                        fail_msg = Some(e.message);
                        break;
                    }
                    None => break, // 正常流终结
                }
            }
            if reason_out != TurnEndReason::Completed {
                break;
            }

            // 步内 usage 累计 + 观测日志（对齐 pi 路径 bm.prompt_usage 口径，
            // 双开对比 analyze.mjs 同源解析；不打 payload 正文）
            if let Some(u) = step_usage {
                let total = usage_total.get_or_insert(LlmUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read: 0,
                    cache_write: 0,
                });
                total.input_tokens += u.input_tokens;
                total.output_tokens += u.output_tokens;
                // 真实模型可见输入校准源（input_tokens = prompt 全量，含缓存命中）
                last_real_input = u.input_tokens;
                tracing::info!(
                    event = "bm.prompt_usage",
                    input = u.input_tokens,
                    output = u.output_tokens,
                    cache_read = u.cache_read,
                    cache_write = u.cache_write,
                    total = u.input_tokens + u.output_tokens,
                    session_total = total.input_tokens + total.output_tokens,
                );
            }
            if !content.is_empty() {
                final_text = content.clone();
            }
            flusher.push(
                EventKind::Core(CoreEvent::AssistantMessage {
                    turn,
                    step,
                    msg: AssistantMsg { content },
                    usage: step_usage.map(|u| TokenUsage {
                        input_tokens: u.input_tokens,
                        output_tokens: u.output_tokens,
                    }),
                }),
                SurfaceIntent::Append,
            );

            // —— 工具执行 ——
            for (call_id, name, args) in &step_tool_calls {
                flusher.push(
                    EventKind::Core(CoreEvent::ToolCall {
                        turn,
                        step,
                        call_id: CallId::new(call_id.clone()),
                        name: name.clone(),
                        args: args.clone(),
                    }),
                    SurfaceIntent::None,
                );
                let parsed = serde_json::from_str::<serde_json::Value>(args)
                    .unwrap_or(serde_json::Value::Null);
                let ctx = ToolCtx {
                    turn,
                    step,
                    call_id: call_id.clone(),
                    name: name.clone(),
                    args: parsed.clone(),
                };
                // 执行前拦截（权限/预算挂点：Deny = 不执行，拒绝原因进结果）
                let (ok, output) = match self.hooks.on_tool_pre(&ctx) {
                    ToolGate::Deny(reason) => (false, reason),
                    ToolGate::Allow => {
                        let req = ToolCallRequest {
                            call_id: call_id.clone(),
                            name: name.clone(),
                            args: parsed,
                        };
                        // 工具执行阶段 cancel 同样可打断（长工具停止按钮响应）
                        tokio::select! {
                            _ = cancel.changed() => {
                                reason_out = TurnEndReason::Cancelled;
                                break;
                            }
                            out = self.executor.execute(req) => (out.ok, out.output),
                        }
                    }
                };
                if reason_out == TurnEndReason::Cancelled {
                    break;
                }
                tool_calls_executed += 1;
                flusher.push(
                    EventKind::Core(CoreEvent::ToolResult {
                        turn,
                        step,
                        call_id: CallId::new(call_id.clone()),
                        result: ToolResultMsg { ok, output },
                        meta: None,
                    }),
                    SurfaceIntent::None,
                );
                self.hooks.on_tool_post(&ctx, ok);
            }
            if reason_out == TurnEndReason::Cancelled {
                break;
            }

            // 软触发（步边界）：0.8 水线 → 压缩事务
            // 先冲刷：本步的 AssistantMessage/工具结果必须已落盘再投影
            flusher.flush().await.map_err(|e| RunError::new(format!("事件日志写入失败: {e}")))?;
            msgs = self
                .log
                .derive_messages(&sid, &bid)
                .await
                .map_err(|e| RunError::new(format!("投影失败: {e}")))?;
            let est = self.estimate_context(&msgs).max(last_real_input);
            // 软触发（步边界）：压缩插件判定（None = 关闭压缩，永不动作）
            if self.config.compactor.as_ref().is_some_and(|c| c.should_compact(est, self.config.context_window as u64)) {
                tracing::info!(event = "bm.loop_compact_soft", turn, step, est_tokens = est);
                self.compact_or_die(turn, cancel).await?;
            }

            // 收尾判定：无工具调用 = 模型决定不再调用工具（正常完成）；
            // 有工具调用时询问扩展点是否强制收尾（默认继续）
            let stopping = if step_tool_calls.is_empty() {
                true
            } else {
                self.hooks.on_turn_stopping(&StopCtx { turn }, &final_text)
            };
            if stopping {
                break;
            }
            // next-step 队列：本回合注入的继续指令已被本轮工具循环覆盖，
            // 队内匹配本回合的注入步骤在此消费（防积压）
            while self.step_queue.front().is_some_and(|s| s.turn == turn) {
                self.step_queue.pop_front();
            }
        }

        // TurnEnd（reason 由全程状态定）；取消时部分文本已入库（对齐 pi 路径）
        flusher.push(
            EventKind::Core(CoreEvent::TurnEnd { turn, reason: reason_out }),
            SurfaceIntent::None,
        );
        self.end_turn();
        flusher.finish().await.map_err(|e| RunError::new(format!("事件日志写入失败: {e}")))?;

        if let Some(msg) = fail_msg {
            tracing::warn!(event = "bm.loop_turn_failed", turn, reason = %msg);
        }
        Ok(TurnOutcome {
            turn,
            steps: step,
            reason: reason_out,
            final_text,
            usage: usage_total,
            tool_calls_executed,
        })
    }

    /// 构建 OpenAI 兼容 payload：system（注入面）+ 投影消息 + tools。
    fn build_payload(&self, msgs: &[bm_kernel::SurfaceMessage]) -> serde_json::Value {
        let mut messages = Vec::new();
        if !self.config.system_prompt.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": self.config.system_prompt,
            }));
        }
        messages.extend(projection_to_openai_messages(msgs));
        serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "tools": self.tools.openai_tools_json(),
            // OpenAI 兼容 API 流式默认不回 usage（MiniMax 实测全帧 usage:null），
            // 显式 include_usage 才能拿到 token 统计（双开对比观测依赖）
            "stream_options": {"include_usage": true},
        })
    }

    /// 上下文粗估（token）：系统提示 + 工具 schema + 投影消息。
    fn estimate_context(&self, msgs: &[bm_kernel::SurfaceMessage]) -> u64 {
        let mut est = estimate_tokens(&self.config.system_prompt);
        est += estimate_tokens(
            &serde_json::to_string(&self.tools.openai_tools_json()).unwrap_or_default(),
        );
        est += msgs
            .iter()
            .map(|m| estimate_tokens(&m.content))
            .sum::<u64>();
        est
    }

    /// 硬触发判定：单步输入本身超窗（模型客观窗口，与压缩插件无关）。
    /// `last_real_input` = 上一请求真实模型可见输入（usage 校准，粗估低估兜底）。
    fn check_overflow(&self, msgs: &[bm_kernel::SurfaceMessage], last_real_input: u64) -> bool {
        self.estimate_context(msgs).max(last_real_input) >= self.config.context_window as u64
    }

    /// 压缩事务；返回 Err = 日志失败（回合失败）。
    /// 无压缩插件：直接返回——硬触发链随后判定超窗失败回合（缺插件
    /// 优雅失败：不崩、不静默丢历史；框架重点不是裸跑，v0.17 定调）。
    async fn compact_or_die(&mut self, turn: u32, cancel: &mut watch::Receiver<bool>) -> Result<(), RunError> {
        let Some(policy) = self.config.compactor.clone() else {
            return Ok(());
        };
        let sid = self.session_id.clone();
        let bid = self.branch_id.clone();
        let model = self.config.model.clone();
        let window = self.config.context_window as u64;
        tokio::select! {
            _ = cancel.changed() => Ok(()), // 取消优先：压缩可推迟
            r = compact(&self.log, &self.llm, &sid, &bid, turn, &model, window, policy.as_ref()) => {
                r.map(|_| ()).map_err(|e| RunError::new(format!("压缩事务失败: {e}")))
            }
        }
    }
}

// ============================================================================
// 事件冲刷器：真序入队 → 攒批 append_batch（A1 模式，库内组件化）
// ============================================================================

/// 冲刷条目：事件 或 屏障（loop 在读回自己的写入前必须 flush，
/// 否则投影会看到比实际少的事件——异步写线程竞态）。
enum FlushItem {
    Event(EventKind, SurfaceIntent),
    Barrier(tokio::sync::oneshot::Sender<Result<(), bm_protocol::ProtocolError>>),
}

/// 真序事件冲刷器：push 同步入队（流回调友好），写线程攒批落盘
/// （一次 append_batch 一个事务，chunk 突发自然成批）；[`Self::flush`]
/// 以屏障保证此前 push 的事件已落盘；`finish` 排空后返回写线程捕获的
/// 最后一个错误（日志失败 = 回合失败，调用方定）。
struct EventFlusher {
    tx: mpsc::UnboundedSender<FlushItem>,
    done: Arc<AtomicBool>,
    notify: Arc<Notify>,
    join: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    last_error: Arc<std::sync::Mutex<Option<bm_protocol::ProtocolError>>>,
}

impl EventFlusher {
    fn new(log: EventLog, sid: SessionId, bid: BranchId) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<FlushItem>();
        let done = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let last_error: Arc<std::sync::Mutex<Option<bm_protocol::ProtocolError>>> =
            Arc::new(std::sync::Mutex::new(None));
        let (done_w, notify_w, err_w) = (done.clone(), notify.clone(), last_error.clone());
        let join = tokio::spawn(async move {
            loop {
                // 排空到屏障或队空
                let mut items: VecDeque<(EventKind, SurfaceIntent)> = VecDeque::new();
                let mut barrier = None;
                while let Ok(item) = rx.try_recv() {
                    match item {
                        FlushItem::Event(k, s) => items.push_back((k, s)),
                        FlushItem::Barrier(tx) => {
                            barrier = Some(tx);
                            break;
                        }
                    }
                }
                if !items.is_empty() {
                    let events: Vec<(EventKind, SurfaceIntent, bool, Option<Vec<SeqNo>>)> =
                        items.into_iter().map(|(k, s)| (k, s, false, None)).collect();
                    if let Err(e) = log.append_batch(sid.clone(), bid.clone(), events).await {
                        *err_w.lock().unwrap() = Some(e);
                    }
                }
                if let Some(tx) = barrier {
                    let _ = tx.send(match err_w.lock().unwrap().clone() {
                        Some(e) => Err(e),
                        None => Ok(()),
                    });
                    continue;
                }
                if done_w.load(Ordering::Relaxed) {
                    break;
                }
                notify_w.notified().await;
            }
        });
        Self {
            tx,
            done,
            notify,
            join: std::sync::Mutex::new(Some(join)),
            last_error,
        }
    }

    /// 同步入队（无背压；unbounded 仅由流事件驱动，量受步内 chunk 数约束）。
    fn push(&self, kind: EventKind, surface: SurfaceIntent) {
        let _ = self.tx.send(FlushItem::Event(kind, surface));
        self.notify.notify_one();
    }

    /// 屏障冲刷：返回时此前 push 的事件已全部落盘（或错误已返回）。
    /// 写线程已死（异常路径）→ StoreUnavailable。
    async fn flush(&self) -> Result<(), bm_protocol::ProtocolError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self.tx.send(FlushItem::Barrier(tx));
        self.notify.notify_one();
        rx.await.map_err(|_| {
            bm_protocol::ProtocolError::new(
                bm_protocol::ErrorCode::StoreUnavailable,
                "事件写线程已停止",
            )
        })?
    }

    /// 排空收尾：置 done → 唤醒 → join；返回写线程最后错误。
    async fn finish(self) -> Result<(), bm_protocol::ProtocolError> {
        self.done.store(true, Ordering::Relaxed);
        self.notify.notify_one();
        // 先取出 join 再 await（MutexGuard 不得跨 await）
        let join = self.join.lock().unwrap().take();
        if let Some(join) = join {
            let _ = join.await;
        }
        match self.last_error.lock().unwrap().take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

// ============================================================================
// 投影 → OpenAI messages（含工具调用/结果的工具角色展开）
// ============================================================================

/// 投影消息 → OpenAI 兼容 messages。assistant 的工具调用（已闭合）随后
/// 展开为 role=tool 结果消息；未闭合调用（无 result）不进入 payload
/// （正常流程不存在——loop 每步必执行工具后才有新投影）。
pub fn projection_to_openai_messages(msgs: &[bm_kernel::SurfaceMessage]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for m in msgs {
        match m.role.as_str() {
            "user" => out.push(serde_json::json!({"role": "user", "content": m.content})),
            "assistant" => {
                let mut entry = serde_json::Map::new();
                entry.insert("role".into(), "assistant".into());
                if !m.content.is_empty() {
                    entry.insert("content".into(), m.content.clone().into());
                }
                let closed: Vec<&bm_kernel::SurfaceToolCall> = m
                    .tool_calls
                    .iter()
                    .filter(|tc| tc.result.is_some())
                    .collect();
                if !closed.is_empty() {
                    entry.insert(
                        "tool_calls".into(),
                        serde_json::Value::Array(
                            closed
                                .iter()
                                .map(|tc| {
                                    serde_json::json!({
                                        "id": tc.call_id,
                                        "type": "function",
                                        "function": {"name": tc.name, "arguments": tc.args},
                                    })
                                })
                                .collect(),
                        ),
                    );
                }
                out.push(serde_json::Value::Object(entry));
                // 工具结果消息（role=tool 紧随 assistant）
                for tc in closed {
                    let result = tc.result.as_ref().expect("filtered closed");
                    out.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tc.call_id,
                        "content": result.output,
                    }));
                }
            }
            // 压缩摘要在投影中也是 assistant 角色，直接入 payload
            other => out.push(serde_json::json!({"role": other, "content": m.content})),
        }
    }
    out
}

// ============================================================================
// 审计工具
// ============================================================================

/// prompt_hash：模型可见输入的审计锚点（sha256 hex，长度前缀分段防歧义拼接）。
/// 与 bm-server chat.rs 的 prompt_hash_of 同构（A2 升级：覆盖完整输入面）。
pub fn prompt_hash_of_parts(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for part in parts {
        h.update((part.len() as u64).to_le_bytes());
        h.update(part.as_bytes());
        h.update([0]);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// epoch ms（request/header 的 created_at）。
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

