//! ReactLoopAgent 集成测试：完整 turn 循环在 InMemoryEventStore 上跑真序
//! 事件链（mock LLM / mock 执行器 / 记录 hooks），覆盖取消、重试、步数上限、
//! 软触发压缩、工具闸门拒绝等路径。

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use bm_kernel::{EventLog, InMemoryEventStore, SurfaceIntent, SurfaceToolCall};
use bm_loop::engine::{
    LoopConfig, ReactLoopAgent, RunError, StepRequest, ToolCallRequest, ToolExecutor, ToolOutcome,
    TurnOutcome, TurnRequest, projection_to_openai_messages,
};
use bm_loop::llm::{Llm, LlmError, LlmEvent, LlmRequest, LlmToolCall, LlmUsage};
use bm_loop::points::{LoopHooks, RequestCtx, StepCtx, StopCtx, ToolCtx, ToolGate};
use bm_protocol::{
    AssistantMsg, BranchId, CoreEvent, EventKind, HeaderReason, SessionId, ToolResultMsg,
    TurnEndReason, UserMsg, UserMsgSource, core_type_name,
};
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::UnboundedReceiverStream;

// —— 测试件：脚本 LLM / 记录执行器 / 记录 hooks ——

/// 脚本 mock：每次请求按序吐预设事件流；脚本耗尽报不可重试错。
struct ScriptLlm {
    scripts: Mutex<VecDeque<Vec<Result<LlmEvent, LlmError>>>>,
    requests: AtomicUsize,
}

impl ScriptLlm {
    fn new(scripts: Vec<Vec<Result<LlmEvent, LlmError>>>) -> Self {
        Self {
            scripts: Mutex::new(scripts.into()),
            requests: AtomicUsize::new(0),
        }
    }
}

impl Llm for ScriptLlm {
    fn stream_chat(
        &self,
        _req: LlmRequest,
    ) -> impl tokio_stream::Stream<Item = Result<LlmEvent, LlmError>> + Send {
        self.requests.fetch_add(1, Ordering::Relaxed);
        let script = self
            .scripts
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| vec![Err(LlmError::new("脚本耗尽", false))]);
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            for ev in script {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        });
        UnboundedReceiverStream::new(rx)
    }
}

/// 永不结束的流（取消测试）。
struct HangingLlm;
impl Llm for HangingLlm {
    fn stream_chat(
        &self,
        _req: LlmRequest,
    ) -> impl tokio_stream::Stream<Item = Result<LlmEvent, LlmError>> + Send {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            tx.send(Ok(LlmEvent::TextDelta { text: "半截".into() })).unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        });
        UnboundedReceiverStream::new(rx)
    }
}

#[derive(Default)]
struct MockExecutor {
    calls: Mutex<Vec<(String, String)>>,
}

impl ToolExecutor for MockExecutor {
    async fn execute(&self, req: ToolCallRequest) -> ToolOutcome {
        self.calls
            .lock()
            .unwrap()
            .push((req.name.clone(), req.args.to_string()));
        ToolOutcome {
            ok: true,
            output: format!("执行了 {}", req.name),
            meta: None,
        }
    }
}

/// 记录 hooks：events 以 Arc 共享，克隆一份留在测试侧断言调用点。
#[derive(Clone, Default)]
struct RecorderHooks {
    events: Arc<Mutex<Vec<String>>>,
    retry: bool,
    deny: bool,
}

impl LoopHooks for RecorderHooks {
    fn on_pre_step(&mut self, ctx: &StepCtx) {
        self.events
            .lock()
            .unwrap()
            .push(format!("pre-step:{}/{}", ctx.turn, ctx.step));
    }
    fn on_stream_chunk(&mut self, _ctx: &StepCtx, text: &str) {
        self.events.lock().unwrap().push(format!("stream:{text}"));
    }
    fn on_request_error(&mut self, _ctx: &RequestCtx, _err: &str) -> bool {
        self.events.lock().unwrap().push("request-error".into());
        self.retry
    }
    fn on_tool_pre(&mut self, ctx: &ToolCtx) -> ToolGate {
        self.events
            .lock()
            .unwrap()
            .push(format!("tool-pre:{}", ctx.name));
        if self.deny {
            ToolGate::Deny("策略拒绝".into())
        } else {
            ToolGate::Allow
        }
    }
    fn on_tool_post(&mut self, ctx: &ToolCtx, ok: bool) {
        self.events
            .lock()
            .unwrap()
            .push(format!("tool-post:{}:{}", ctx.name, ok));
    }
    fn on_turn_stopping(&mut self, _ctx: &StopCtx, _text: &str) -> bool {
        self.events.lock().unwrap().push("turn-stopping".into());
        false
    }
}

fn script_text(text: &str) -> Vec<Result<LlmEvent, LlmError>> {
    vec![
        Ok(LlmEvent::TextDelta { text: text.to_string() }),
        Ok(LlmEvent::MessageEnd {
            content: text.to_string(),
            tool_calls: Vec::new(),
            usage: Some(LlmUsage { input_tokens: 10, output_tokens: 5 }),
        }),
    ]
}

fn script_tool() -> Vec<Result<LlmEvent, LlmError>> {
    vec![
        Ok(LlmEvent::TextDelta { text: "让我查一下".into() }),
        Ok(LlmEvent::ToolCallStart { id: "c1".into(), name: "web_search".into() }),
        Ok(LlmEvent::ToolCallArgs { id: "c1".into(), args_delta: "{\"q\":\"rust\"}".into() }),
        Ok(LlmEvent::ToolCallEnd { id: "c1".into(), arguments: "{\"q\":\"rust\"}".into() }),
        Ok(LlmEvent::MessageEnd {
            content: "让我查一下".into(),
            tool_calls: vec![LlmToolCall {
                id: "c1".into(),
                name: "web_search".into(),
                arguments: "{\"q\":\"rust\"}".into(),
            }],
            usage: Some(LlmUsage { input_tokens: 20, output_tokens: 8 }),
        }),
    ]
}

const SID: &str = "sess_a6";

fn make_agent<H: LoopHooks, L: Llm, T: ToolExecutor>(
    hooks: H,
    llm: L,
    executor: T,
) -> (ReactLoopAgent<H, L, T>, Arc<InMemoryEventStore>) {
    let store = Arc::new(InMemoryEventStore::new());
    let log = EventLog::new(store.clone());
    let agent = ReactLoopAgent::new(
        hooks,
        bm_loop::ToolRegistry::new(),
        log,
        SessionId::new(SID),
        BranchId::new("main"),
        LoopConfig::default(),
        llm,
        executor,
    );
    (agent, store)
}

/// 取消通道：sender 与 receiver 一起返回（sender 存活保证 changed() 不误报）。
fn cancel_channel() -> (watch::Sender<bool>, watch::Receiver<bool>) {
    watch::channel(false)
}

async fn type_sequence(store: Arc<InMemoryEventStore>) -> Vec<String> {
    let log = EventLog::new(store.clone());
    log.replay(&SessionId::new(SID), &BranchId::new("main"))
        .await
        .unwrap()
        .iter()
        .map(|e| match &e.kind {
            EventKind::Core(c) => core_type_name(c).to_string(),
            _ => "?".into(),
        })
        .collect()
}

async fn last_turn_end(store: Arc<InMemoryEventStore>) -> TurnEndReason {
    let log = EventLog::new(store.clone());
    let evs = log.replay(&SessionId::new(SID), &BranchId::new("main")).await.unwrap();
    match &evs.last().unwrap().kind {
        EventKind::Core(CoreEvent::TurnEnd { reason, .. }) => *reason,
        _ => panic!("末事件应为 turn/end"),
    }
}

#[test]
fn inbox_queues_fifo() {
    let (mut a, _) = make_agent((), ScriptLlm::new(vec![]), MockExecutor::default());
    a.enqueue_turn(TurnRequest { content: "t1".into(), source: UserMsgSource::Human });
    a.enqueue_turn(TurnRequest { content: "t2".into(), source: UserMsgSource::Goal });
    a.enqueue_step(StepRequest { turn: 1 });
    assert_eq!(a.pending_turns(), 2);
    assert_eq!(a.pending_steps(), 1);
}

#[tokio::test]
async fn turn_without_tools_completes_with_full_event_chain() {
    let (mut a, store) =
        make_agent((), ScriptLlm::new(vec![script_text("你好")]), MockExecutor::default());
    let (_tx, mut rx) = cancel_channel();
    let out: TurnOutcome = a
        .run_turn(
            TurnRequest { content: "hi".into(), source: UserMsgSource::Human },
            HeaderReason::Initial,
            &mut rx,
        )
        .await
        .unwrap();
    assert_eq!(out.reason, TurnEndReason::Completed);
    assert_eq!(out.steps, 1);
    assert_eq!(out.final_text, "你好");
    assert_eq!(out.usage.unwrap().output_tokens, 5);

    let types = type_sequence(store.clone()).await;
    assert_eq!(
        types,
        vec![
            "user/message",
            "request/header",
            "turn/start",
            "step/start",
            "assistant/chunk",
            "assistant/message",
            "turn/end",
        ]
    );
    assert_eq!(last_turn_end(store.clone()).await, TurnEndReason::Completed);
}

#[tokio::test]
async fn tool_loop_executes_and_records_hooks() {
    let hooks = RecorderHooks::default();
    let spy = hooks.clone();
    let (mut a, store) = make_agent(
        hooks,
        ScriptLlm::new(vec![script_tool(), script_text("查到了：Rust 很火")]),
        MockExecutor::default(),
    );
    let (_tx, mut rx) = cancel_channel();
    let out = a
        .run_turn(
            TurnRequest { content: "查 rust".into(), source: UserMsgSource::Human },
            HeaderReason::Initial,
            &mut rx,
        )
        .await
        .unwrap();
    assert_eq!(out.reason, TurnEndReason::Completed);
    assert_eq!(out.steps, 2);
    assert_eq!(out.tool_calls_executed, 1);

    let types = type_sequence(store.clone()).await;
    assert_eq!(
        types,
        vec![
            "user/message", "request/header", "turn/start",
            "step/start", "assistant/chunk", "assistant/message",
            "tool/call", "tool/result",
            "step/start", "assistant/chunk", "assistant/message",
            "turn/end",
        ],
        "真序事件链：模型消息（含工具意图）先行，工具执行随后（自研 loop 语义）"
    );

    // hooks 调用点（步 1 有工具调用 → 收尾判定询问 on_turn_stopping，默认 false 继续；
    // 流式块先于工具执行，两步入 stream 钩子各一次——SSE 前端通道同源同序）
    let hook_evs = spy.events.lock().unwrap().clone();
    assert_eq!(
        hook_evs,
        vec![
            "pre-step:1/1",
            "stream:让我查一下",
            "tool-pre:web_search",
            "tool-post:web_search:true",
            "turn-stopping",
            "pre-step:1/2",
            "stream:查到了：Rust 很火",
        ]
    );

    // 投影：工具结果进入消息面
    let log = EventLog::new(store.clone());
    let msgs = log.derive_messages(&SessionId::new(SID), &BranchId::new("main")).await.unwrap();
    assert!(msgs.iter().any(|m| m.tool_calls.iter().any(|tc| tc
        .result
        .as_ref()
        .is_some_and(|r| r.output == "执行了 web_search"))));
}

#[tokio::test]
async fn cancel_mid_stream_records_cancelled_with_partial_text() {
    let (mut a, store) = make_agent((), HangingLlm, MockExecutor::default());
    let (tx, mut rx) = watch::channel(false);
    // 首事件后取消
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let _ = tx.send(true);
    });
    let out = a
        .run_turn(
            TurnRequest { content: "长任务".into(), source: UserMsgSource::Human },
            HeaderReason::Initial,
            &mut rx,
        )
        .await
        .unwrap();
    assert_eq!(out.reason, TurnEndReason::Cancelled);

    let types = type_sequence(store.clone()).await;
    assert!(types.contains(&"assistant/chunk".to_string()), "部分文本入库");
    assert_eq!(types.last().unwrap(), "turn/end");
    assert_eq!(last_turn_end(store.clone()).await, TurnEndReason::Cancelled);
}

#[tokio::test]
async fn retry_on_retryable_error_then_succeed() {
    let hooks = RecorderHooks { retry: true, ..RecorderHooks::default() };
    let spy = hooks.clone();
    let (mut a, _store) = make_agent(
        hooks,
        ScriptLlm::new(vec![
            vec![Err(LlmError::new("上游 503", true))],
            script_text("重试成功"),
        ]),
        MockExecutor::default(),
    );
    let (_tx, mut rx) = cancel_channel();
    let out = a
        .run_turn(
            TurnRequest { content: "x".into(), source: UserMsgSource::Human },
            HeaderReason::Initial,
            &mut rx,
        )
        .await
        .unwrap();
    assert_eq!(out.reason, TurnEndReason::Completed);
    assert_eq!(out.final_text, "重试成功");
    assert!(
        spy.events.lock().unwrap().contains(&"request-error".to_string()),
        "on_request_error 被调用且返回 true 触发重试"
    );
}

#[tokio::test]
async fn non_retryable_error_fails_turn() {
    let (mut a, store) = make_agent(
        (),
        ScriptLlm::new(vec![vec![Err(LlmError::new("鉴权失败 401", false))]]),
        MockExecutor::default(),
    );
    let (_tx, mut rx) = cancel_channel();
    let out = a
        .run_turn(
            TurnRequest { content: "x".into(), source: UserMsgSource::Human },
            HeaderReason::Initial,
            &mut rx,
        )
        .await
        .unwrap();
    assert_eq!(out.reason, TurnEndReason::Failed);
    assert_eq!(last_turn_end(store.clone()).await, TurnEndReason::Failed);
}

#[tokio::test]
async fn step_limit_fails_turn_when_model_never_stops_calling_tools() {
    let (mut a, store) = make_agent(
        (),
        ScriptLlm::new(vec![script_tool(), script_tool(), script_tool()]),
        MockExecutor::default(),
    );
    // max_steps = 2 → 工具循环不收敛 → Failed
    a.config_mut().max_steps = 2;
    let (_tx, mut rx) = cancel_channel();
    let out = a
        .run_turn(
            TurnRequest { content: "x".into(), source: UserMsgSource::Human },
            HeaderReason::Initial,
            &mut rx,
        )
        .await
        .unwrap();
    assert_eq!(out.reason, TurnEndReason::Failed);
    assert_eq!(out.steps, 2);
    assert_eq!(last_turn_end(store.clone()).await, TurnEndReason::Failed);
}

#[tokio::test]
async fn soft_compaction_triggers_after_step() {
    // 预置长历史：3 × 1610 字 ≈ 403 token 各（seq 1..=3）
    let store = Arc::new(InMemoryEventStore::new());
    let log = EventLog::new(store.clone());
    let long = "很长的历史消息".repeat(230);
    for is_user in [true, false, true] {
        let kind = if is_user {
            EventKind::Core(CoreEvent::UserMessage {
                msg: UserMsg { content: long.clone() },
                source: UserMsgSource::Human,
            })
        } else {
            EventKind::Core(CoreEvent::AssistantMessage {
                turn: 0,
                step: 0,
                msg: AssistantMsg { content: long.clone() },
                usage: None,
            })
        };
        log.append(SessionId::new(SID), BranchId::new("main"), kind, SurfaceIntent::Append)
            .await
            .unwrap();
    }

    // 窗口 1500 token、水线 0.8 → 历史 1209 + 新消息 ≈ 1212 落入 [1200, 1500) 软触发
    // （步开始前不超窗，不走硬触发）
    let cfg = LoopConfig {
        compaction: bm_loop::compact::CompactionPolicy {
            enabled: true,
            context_window: 1500,
            watermark: 0.8,
            keep_recent_ratio: 0.1,
            keep_recent_floor: 10,
        },
        ..LoopConfig::default()
    };
    let mut a = ReactLoopAgent::new(
        (),
        bm_loop::ToolRegistry::new(),
        EventLog::new(store.clone()),
        SessionId::new(SID),
        BranchId::new("main"),
        cfg,
        // 第一个请求 = 步内 chat；第二个 = 摘要
        ScriptLlm::new(vec![script_text("收到"), script_text("摘要：长历史")]),
        MockExecutor::default(),
    );
    let (_tx, mut rx) = cancel_channel();
    let out = a
        .run_turn(
            TurnRequest { content: "继续".into(), source: UserMsgSource::Human },
            HeaderReason::Initial,
            &mut rx,
        )
        .await
        .unwrap();
    assert_eq!(out.reason, TurnEndReason::Completed);

    let types = type_sequence(store.clone()).await;
    assert!(
        types.contains(&"compaction/start".to_string())
            && types.contains(&"compaction/summary".to_string())
            && types.contains(&"compaction/end".to_string()),
        "软触发压缩事务应落三事件: {types:?}"
    );
    // 遮蔽后投影含摘要
    let log2 = EventLog::new(store.clone());
    let msgs = log2.derive_messages(&SessionId::new(SID), &BranchId::new("main")).await.unwrap();
    assert!(msgs.iter().any(|m| m.content.contains("摘要")));
}

#[tokio::test]
async fn deny_gate_skips_execution_but_records_result() {
    // 执行前拦截：Deny → 不调用执行器，拒绝原因作为工具结果入日志（模型可见）
    let (mut a, store) = make_agent(
        RecorderHooks { deny: true, ..RecorderHooks::default() },
        ScriptLlm::new(vec![script_tool(), script_text("被拒了，换个方法")]),
        MockExecutor::default(),
    );
    let (_tx, mut rx) = cancel_channel();
    let out = a
        .run_turn(
            TurnRequest { content: "x".into(), source: UserMsgSource::Human },
            HeaderReason::Initial,
            &mut rx,
        )
        .await
        .unwrap();
    assert_eq!(out.reason, TurnEndReason::Completed);
    assert_eq!(out.tool_calls_executed, 1, "拒绝也计入工具结果条数（模型可见）");
    let types = type_sequence(store.clone()).await;
    assert!(types.contains(&"tool/result".to_string()));

    // 拒绝原因进了工具结果（投影可见）
    let log = EventLog::new(store.clone());
    let msgs = log.derive_messages(&SessionId::new(SID), &BranchId::new("main")).await.unwrap();
    assert!(msgs.iter().any(|m| m
        .tool_calls
        .iter()
        .any(|tc| tc.result.as_ref().is_some_and(|r| !r.ok && r.output == "策略拒绝"))));
}

#[test]
fn projection_to_openai_roles_and_tool_messages() {
    let msgs = vec![
        bm_kernel::SurfaceMessage {
            seq: 1,
            role: "user".into(),
            content: "查一下".into(),
            tool_calls: Vec::new(),
            turn: 0,
            step: 0,
        },
        bm_kernel::SurfaceMessage {
            seq: 2,
            role: "assistant".into(),
            content: "让我查".into(),
            tool_calls: vec![
                SurfaceToolCall {
                    call_id: "c1".into(),
                    name: "web_search".into(),
                    args: "{\"q\":\"x\"}".into(),
                    result: Some(ToolResultMsg { ok: true, output: "找到 3 条".into() }),
                },
                SurfaceToolCall {
                    call_id: "c2".into(),
                    name: "exec".into(),
                    args: "{}".into(),
                    result: None, // 未闭合：不进 payload
                },
            ],
            turn: 1,
            step: 1,
        },
    ];
    let out = projection_to_openai_messages(&msgs);
    assert_eq!(out.len(), 3, "user + assistant + 1 条 tool 结果");
    assert_eq!(out[0]["role"], "user");
    assert_eq!(out[1]["role"], "assistant");
    assert_eq!(out[1]["tool_calls"][0]["id"], "c1");
    assert_eq!(out[1]["tool_calls"].as_array().unwrap().len(), 1, "未闭合调用不进 payload");
    assert_eq!(out[2]["role"], "tool");
    assert_eq!(out[2]["tool_call_id"], "c1");
    assert_eq!(out[2]["content"], "找到 3 条");
}

/// 从未出错的 run_turn 结果（编译期签名兜底：错误路径错误也走同一类型）。
#[allow(dead_code)]
fn _assert_send(_: &Result<TurnOutcome, RunError>) {}
