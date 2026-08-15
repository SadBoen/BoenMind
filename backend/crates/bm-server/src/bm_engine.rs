//! 自研引擎接线（A6，bm-loop）：chat/管家回合的 bm 路径——provider 桥接 →
//! OpenAiClient 流式 → 事件日志真序 → SSE 前端形状零改动。
//! 切片②（B4）：工具方向——ToolRegistry 从 CompatEngine 工具快照汇合，
//! QuickJsToolExecutor 经 `__pi_execute_tool` 桥执行插件工具。
//! B5：权限桥（compat_engine.rs）。
//! B6（本轮）：插件事件（startup 懒发 / tool_call / tool_result 经
//! QuickJsToolExecutor 推送）+ 切片②顺手件（thinking 档位映射 → reasoning_
//! effort + 心跳 TaskProgress 与 pi 路径同构）。
//!
//! 与 pi 路径的分工（§九·三.6）：bm 路径下 **loop 拥有事件日志全生命周期**
//! （UserMessage/RequestHeader/TurnStart/Step*/TurnEnd 全由 loop 落），
//! 本模块不再重复落日志；SQLite 的 messages 表仍照常写（前端历史从 DB 读）。

use std::sync::Arc;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response, sse::{KeepAlive, Sse}},
};
use bm_core::agent::AgentStreamEvent;
use bm_loop::engine::{LoopConfig, ReactLoopAgent, TurnRequest};
use bm_loop::llm::{LlmConfig, OpenAiClient};
use bm_loop::points::{LoopHooks, RequestCtx, StepCtx, ToolCtx, ToolGate};
use bm_protocol::{BranchId, CoreEvent, EventKind, HeaderReason, SessionId, TurnEndReason, UserMsgSource};
use tokio::sync::{Mutex, mpsc, watch};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::compat_engine::{CompatEngine, QuickJsToolExecutor};
use crate::AppState;

/// bm 引擎的 agent 具体类型（H = StreamHooks / L = OpenAiClient /
/// T = QuickJsToolExecutor——B4 起工具执行经 QuickJS 桥；引擎未启用时
/// executor 兜底报错）。
pub type BmLoopAgent = ReactLoopAgent<StreamHooks, OpenAiClient, QuickJsToolExecutor>;

/// 会话级 bm-loop agent 条目（对齐 pi 的 AgentSessionEntry）。
/// agent 本体只是「日志 + 配置 + 客户端」的壳——事件日志是唯一状态源，
/// 换 provider/model 或空闲淘汰时弃置重建零损失。
pub struct LoopSessionEntry {
    pub agent: Arc<Mutex<BmLoopAgent>>,
    /// 会话串行锁：同会话任何回合（chat/steward/参数重建）必须先取此锁——
    /// 事件日志唯一状态源要求回合严格串行，仅靠 agent 锁会在并发重建时失效
    /// （回看 P0：check-then-build-then-insert 窗口产生双 agent 并行写日志）。
    pub serial: Arc<tokio::sync::Mutex<()>>,
    pub provider_id: String,
    pub model: String,
    /// thinking 档位（与 pi 路径一致：改变即重建 agent）
    pub thinking: String,
    /// startup 插件事件是否已发（每 agent 一次——重建/淘汰后重发，
    /// ctx-compactor 借此重新加载项目配置）
    pub startup_sent: bool,
    pub last_used: std::time::Instant,
}

/// thinking 档名 → OpenAI 兼容 `reasoning_effort`（切片②：七档折叠三档）。
/// off→不注入（端点默认）；minimal/low→low；medium→medium；high/xhigh/max→high。
pub fn reasoning_effort_for(level: &str) -> Option<&'static str> {
    match level.trim().to_lowercase().as_str() {
        "off" | "none" | "0" => None,
        "minimal" | "min" | "low" | "1" => Some("low"),
        "medium" | "med" | "2" => Some("medium"),
        "high" | "3" | "xhigh" | "4" | "max" | "5" => Some("high"),
        _ => None,
    }
}

/// 管家回合静默窗口默认值（秒）与解析：集中定义在 steward.rs
/// （StewardConfig::from_env 是 `BM_STEWARD_*` 唯一读取点），此处 re-export
/// 供本模块与测试引用。
pub use crate::steward::{DEFAULT_SILENCE_WINDOW_S, parse_silence_window};

/// Windows 平台段（M1 验收问题 4）：bash 工具实际跑在 cmd /C 下，模型若按
/// Git Bash 习惯（/d/... 路径、pwd）会在 cmd 下反复失败浪费步数——把平台
/// 差异写进系统提示，一次生效。
#[cfg(windows)]
const PLATFORM_HINT: &str = "\n\n运行环境提示（Windows）：\n- bash 工具运行在 cmd（/C）下，不是 Git Bash：路径用相对路径或 Windows 绝对路径（如 C:\\repo\\file.rs），不要用 /d/... 前缀；cmd 没有 pwd 命令，查看当前目录用不带参数的 cd。";
#[cfg(not(windows))]
const PLATFORM_HINT: &str = "";

// 静默窗口解析见上方 re-export（steward.rs 为 `BM_STEWARD_*` 唯一读取点）。

/// 管家回合 LLM 解析（成本杠杆，架构 §14.2）：env `BM_STEWARD_PROVIDER` /
/// `BM_STEWARD_MODEL` 显式指定时优先（24×7 心跳是主要烧钱点，可用低成本
/// 模型跑），否则回落会话级规则（与 chat 路径同）。env 配错（提供商不存在 /
/// 模型不在列表）→ warn + 回落，不让管家停摆。独立纯函数以便单测。
pub fn resolve_steward_llm(
    config: &bm_core::config::AppConfig,
    session_provider: Option<&str>,
    session_model: Option<&str>,
    env_provider: Option<&str>,
    env_model: Option<&str>,
) -> Result<(bm_core::config::ProviderConfig, String), String> {
    let env_provider = env_provider.map(str::trim).filter(|s| !s.is_empty());
    let env_model = env_model.map(str::trim).filter(|s| !s.is_empty());
    let provider = match env_provider {
        Some(id) => match bm_core::config::resolve_provider(config, Some(id)) {
            Some(p) => p.clone(),
            None => {
                tracing::warn!(
                    event = "bm.steward_provider_missing",
                    provider = %id,
                    "env 指定的管家提供商不存在，回落会话级"
                );
                bm_core::config::resolve_provider(config, session_provider)
                    .cloned()
                    .ok_or_else(|| "未配置任何模型提供商".to_string())?
            }
        },
        None => bm_core::config::resolve_provider(config, session_provider)
            .cloned()
            .ok_or_else(|| "未配置任何模型提供商".to_string())?,
    };
    let model = match env_model {
        Some(m) if provider.models.is_empty() || provider.models.iter().any(|x| x == m) => {
            m.to_string()
        }
        Some(m) => {
            tracing::warn!(
                event = "bm.steward_model_missing",
                model = %m,
                provider = %provider.id,
                "env 指定的管家模型不在提供商列表，回落会话级"
            );
            bm_core::config::resolve_model(&provider, session_model)
                .ok_or_else(|| format!("提供商 {} 未配置模型", provider.name))?
        }
        None => bm_core::config::resolve_model(&provider, session_model)
            .ok_or_else(|| format!("提供商 {} 未配置模型", provider.name))?,
    };
    Ok((provider, model))
}

// ============================================================================
// 流式通道 hooks：loop → 前端 SSE（on_stream_chunk 钩子的集成方实现）
// ============================================================================

/// 流式通道 hooks：`on_stream_chunk` 转发 TextDelta 给当前 prompt 的 SSE 通道；
/// 通道已关闭（客户端断开）→ 触发取消，避免继续烧 token。同时维护心跳
/// 进度（最近活动摘要：工具名 / 回复尾部 80 字符，chat_bm 的心跳 task 定时
/// 消费推 SSE + 落库）。
///
/// 挂点说明：hooks 存活于 agent（会话级），当前 prompt 的通道在 run_turn
/// 前后 attach/detach——同一会话 prompt 天然串行（agent 锁），无并发覆盖。
/// 内部用 std Mutex：LoopHooks 钩子都是同步方法（回调线程不可 await），
/// 且临界区仅克隆句柄，无阻塞风险。
pub struct StreamHooks {
    tx: std::sync::Mutex<Option<mpsc::UnboundedSender<AgentStreamEvent>>>,
    cancel: std::sync::Mutex<Option<watch::Sender<bool>>>,
    progress: Arc<std::sync::Mutex<String>>,
    /// 记忆插件（§6.1 最小实现，v0.17 双向奔赴）：on_request 注入 facts；
    /// Arc 共享——外部（未来 governance.memorize 调用点）可 remember。
    memory: Option<Arc<std::sync::Mutex<bm_memory::MemoryFilePlugin>>>,
}

impl StreamHooks {
    pub fn new(
        progress: Arc<std::sync::Mutex<String>>,
        memory: Option<Arc<std::sync::Mutex<bm_memory::MemoryFilePlugin>>>,
    ) -> Self {
        Self {
            tx: std::sync::Mutex::new(None),
            cancel: std::sync::Mutex::new(None),
            progress,
            memory,
        }
    }

    /// 记忆插件句柄（外部 remember 入口；无记忆插件时为 None）。
    pub fn memory(&self) -> Option<Arc<std::sync::Mutex<bm_memory::MemoryFilePlugin>>> {
        self.memory.clone()
    }

    /// 挂接当前 prompt 的 SSE 通道与取消通道（run_turn 前调用）。
    fn attach(&self, tx: mpsc::UnboundedSender<AgentStreamEvent>, cancel: watch::Sender<bool>) {
        *self.tx.lock().unwrap() = Some(tx);
        *self.cancel.lock().unwrap() = Some(cancel);
    }

    /// 摘除（run_turn 返回后调用，防旧 prompt 结束后残留通道）。
    fn detach(&self) {
        *self.tx.lock().unwrap() = None;
        *self.cancel.lock().unwrap() = None;
    }

    /// 发送事件到当前 prompt 通道；通道已关闭返回 false（unbounded 无满队列）。
    fn try_send(&self, ev: AgentStreamEvent) -> bool {
        let Some(tx) = self.tx.lock().unwrap().clone() else {
            return false;
        };
        if tx.is_closed() {
            return false;
        }
        tx.send(ev).is_ok()
    }

    /// 触发取消（客户端断开 → 停止按钮同路径收尾）。
    fn trigger_cancel(&self) {
        if let Some(tx) = self.cancel.lock().unwrap().clone() {
            let _ = tx.send(true);
        }
    }

    /// 心跳进度更新（回复尾部 80 字符；覆盖旧摘要——进度只表达"最近在干什么"）。
    fn set_progress(&self, text: &str) {
        let mut progress = self.progress.lock().unwrap();
        let tail: String = text.chars().rev().take(80).collect();
        *progress = tail.chars().rev().collect();
    }
}

impl LoopHooks for StreamHooks {
    fn on_request(&mut self, _ctx: &RequestCtx, payload: &mut serde_json::Value) {
        // 记忆插件注入（§6.1 最小实现）：facts 作为追加 system 段进模型请求
        if let Some(memory) = &self.memory
            && let Ok(mut m) = memory.lock()
        {
            m.on_request(_ctx, payload);
        }
    }

    fn on_stream_chunk(&mut self, _ctx: &StepCtx, text: &str) {
        self.set_progress(text);
        if !self.try_send(AgentStreamEvent::TextDelta { delta: text.to_string() }) {
            self.trigger_cancel();
        }
    }

    fn on_tool_pre(&mut self, ctx: &ToolCtx) -> ToolGate {
        // B4：工具调用开始事件（前端工具卡片）；权限裁决是 B5（此处恒 Allow——
        // 执行前拦截由 loop 的 ToolGate 语义承载，权限桥接上后在此返回 Deny）
        *self.progress.lock().unwrap() = format!("工具: {}", ctx.name);
        if !self.try_send(AgentStreamEvent::ToolCallStart {
            id: ctx.call_id.clone(),
            name: ctx.name.clone(),
            args: ctx.args.clone(),
        }) {
            self.trigger_cancel();
        }
        ToolGate::Allow
    }

    fn on_tool_post(&mut self, ctx: &ToolCtx, ok: bool) {
        if !self.try_send(AgentStreamEvent::ToolCallEnd {
            id: ctx.call_id.clone(),
            name: ctx.name.clone(),
            is_error: !ok,
        }) {
            self.trigger_cancel();
        }
    }
}

// ============================================================================
// 工具执行侧（B4）：ToolRegistry 汇合 + QuickJS 执行器（见 compat_engine.rs）
// ============================================================================

/// 组装 bm-loop agent（不依赖会话锁；调用方负责落 map）。
/// B4：工具从 CompatEngine 快照汇合进 ToolRegistry，执行侧 QuickJsToolExecutor。
/// 切片②：`thinking` 档位 → reasoning_effort 注入；`progress` 供心跳 task 消费。
#[allow(clippy::too_many_arguments)]
fn build_loop_agent(
    system_prompt: &str,
    kernel: Option<&Arc<bm_kernel::Kernel>>,
    compat: Option<&Arc<CompatEngine>>,
    steward: Option<&Arc<crate::steward::StewardStore>>,
    is_steward_session: bool,
    session_id: &str,
    provider: &bm_core::config::ProviderConfig,
    model: &str,
    thinking: Option<&str>,
    compaction: Option<bm_core::compaction::EffectiveCompaction>,
    progress: Arc<std::sync::Mutex<String>>,
) -> Result<BmLoopAgent, (StatusCode, String)> {
    // 内核（v0.21 接线）：事件日志与压缩服务都从 kernel 取——事件日志
    // 不可用时内核也不存在（同一开关）
    let Some(kernel) = kernel else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "事件日志不可用，bm 引擎无法启动".to_string(),
        ));
    };
    let llm = OpenAiClient::new(resolve_llm_config(provider, model, thinking)?);
    let mut tools = bm_loop::ToolRegistry::new();
    // B6：内置工具集 schema 进模型可见面（对齐 pi BUILTIN_TOOL_NAMES 全开），
    // 执行侧 QuickJsToolExecutor 按名分派到 BuiltinTools
    for tool in crate::builtin_tools::BuiltinTools::definitions() {
        if let Err(err) = tools.register(tool.clone()) {
            tracing::warn!(event = "bm.tool_register_failed", tool = %tool.name, error = %err.message);
        }
    }
    // 专家团队：subagent 父侧工具（pi 路径同款参数面；子进程协议不变）
    let subagent_def = crate::subagent_tool::tool_def();
    if let Err(err) = tools.register(subagent_def.clone()) {
        tracing::warn!(event = "bm.tool_register_failed", tool = %subagent_def.name, error = %err.message);
    }
    // 活任务清单（M2）：todo 工具注册进全部会话工具面（通用能力）；
    // 执行侧经 executor 挂的事件日志写 todo/write 快照
    let todo_def = crate::todo_tool::todo_def();
    if let Err(err) = tools.register(todo_def.clone()) {
        tracing::warn!(event = "bm.tool_register_failed", tool = %todo_def.name, error = %err.message);
    }
    // 管家（Steward 轮）：set_wake 只进管家会话的工具面——普通会话工具面
    // 零污染，管家身份由 BM_STEWARD_SESSION 宿主配置（不依赖模型自选）
    if is_steward_session {
        let wake_def = crate::steward::set_wake_def();
        if let Err(err) = tools.register(wake_def.clone()) {
            tracing::warn!(event = "bm.tool_register_failed", tool = %wake_def.name, error = %err.message);
        }
    }
    if let Some(compat) = compat {
        for tool in &compat.tools {
            // 快照在启动加载后固化；重名拒绝是防呆（工具名是 call_id 关联键）
            if let Err(err) = tools.register(tool.clone()) {
                tracing::warn!(event = "bm.tool_register_failed", tool = %tool.name, error = %err.message);
            }
        }
    }
    // 记忆插件（§6.1 最小实现）：facts.md 每会话 open 加载（跨会话记忆 =
    // 文件本身；多会话并发写靠单行 append 容忍，全局单例留 Steward 轮）
    let memory = Arc::new(std::sync::Mutex::new(bm_memory::MemoryFilePlugin::open(
        bm_core::config::app_dir().join("memory").join("facts.md"),
        20,
    )));
    Ok(ReactLoopAgent::new(
        StreamHooks::new(progress, Some(memory)),
        tools,
        bm_kernel::EventLog::new(kernel.event_store()),
        SessionId::new(session_id),
        BranchId::new("main"),
        LoopConfig {
            system_prompt: system_prompt.to_string(),
            provider: Some(provider.id.clone()),
            model: model.to_string(),
            // 模型窗口（客观属性）：暂取默认 128K——后续从模型注册表换算
            context_window: 128_000,
            max_steps: 128,
            // 挂压缩插件（可换可关；None = 裸跑，核心自足性 v0.17）——
            // 策略源 = kernel registry 里的 bm-compactor 服务（v0.21 接线），
            // 参数由组装层从 [compaction] 配置换算注入（EffectiveCompaction；
            // enabled=false → None 不挂），策略实现不读配置
            compactor: compaction.map(|c| {
                let base = kernel
                    .service::<bm_compactor::DefaultCompactor>("compactor")
                    .map(|svc| (*svc).clone())
                    .unwrap_or_default();
                std::sync::Arc::new(bm_compactor::DefaultCompactor {
                    watermark: c.watermark,
                    keep_recent_ratio: c.keep_recent_ratio,
                    keep_recent_floor: c.keep_recent_floor as u64,
                    ..base
                }) as std::sync::Arc<dyn bm_loop::Compactor>
            }),
        },
        llm,
        QuickJsToolExecutor::new(compat.cloned(), session_id, steward.cloned())
            .with_event_log(bm_kernel::EventLog::new(kernel.event_store())),
    ))
}

/// provider 配置 → LlmConfig（bm-core 不依赖 bm-loop，桥接在 bm-server 做；
/// 子代理子进程（subagent_child）复用同一解析）。
/// base_url：用户填写优先，否则官方端点；custom 必须填写（配置写入时已校验）。
/// thinking 档位 → reasoning_effort（切片②；None = 端点默认推理参数）。
pub(crate) fn resolve_llm_config(
    provider: &bm_core::config::ProviderConfig,
    model: &str,
    thinking: Option<&str>,
) -> Result<LlmConfig, (StatusCode, String)> {
    let base_url = provider
        .base_url
        .as_deref()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| bm_core::providers::official_base_url(provider.kind).map(str::to_string))
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                format!("提供商 {} 未配置 API 端点", provider.name),
            )
        })?;
    Ok(LlmConfig {
        base_url,
        api_key: provider.api_key.clone().unwrap_or_default(),
        model: model.to_string(),
        provider: Some(provider.id.clone()),
        reasoning_effort: thinking.and_then(reasoning_effort_for).map(str::to_string),
    })
}

/// 取会话的 bm-loop agent；不存在或 provider/model/thinking 不一致 → 重建
/// （状态都在日志，重建零损失——"EventLog 唯一状态源"相对 pi 句柄的优势）。
/// 返回 (agent, 会话串行锁)：调用方必须先取串行锁再跑回合——并发（chat +
/// steward、或参数重建窗口）下两个回合并行写同一事件日志会污染投影。
#[allow(clippy::too_many_arguments)]
async fn get_or_create_loop_agent(
    state: &AppState,
    session_id: &str,
    provider: &bm_core::config::ProviderConfig,
    model: &str,
    thinking: &str,
    progress: Arc<std::sync::Mutex<String>>,
) -> Result<(Arc<Mutex<BmLoopAgent>>, Arc<tokio::sync::Mutex<()>>), (StatusCode, String)> {
    // 既有条目：参数一致直接复用（map 锁立即释放）
    if let Some(entry) = {
        let mut map = state.loop_agents.lock().await;
        let existing = map.get_mut(session_id);
        if let Some(e) = existing
            && e.provider_id == provider.id
            && e.model == model
            && e.thinking == thinking
        {
            e.last_used = std::time::Instant::now();
            Some((e.agent.clone(), e.serial.clone()))
        } else {
            None
        }
    } {
        return Ok(entry);
    }

    // 管家身份判定：BM_STEWARD_SESSION 指定的会话才注册 set_wake 工具
    // 并追加管家提示词（身份/静默协议引导）
    let is_steward_session = match &state.steward {
        Some(store) => store.session_id().await.as_deref() == Some(session_id),
        None => false,
    };

    // 系统提示拼接（与 pi 路径同构：SYSTEM_PROMPT + skills + custom；
    // 管家会话再追管家职责段——Steward 轮）+ 压缩参数换算
    // （[compaction] 配置 → 策略插件构造参数；enabled=false → None 不挂）
    let (system_prompt, compaction) = {
        let config = state.config.read().await;
        let skills = bm_core::skills::enabled_skills_prompt(&config);
        let custom = config.custom_system_prompt.clone().unwrap_or_default();
        let base = if skills.is_empty() && custom.is_empty() {
            bm_core::agent::SYSTEM_PROMPT.to_string()
        } else {
            format!("{}{}{}", bm_core::agent::SYSTEM_PROMPT, skills, custom)
        };
        let system_prompt = if is_steward_session {
            format!("{base}{}{}", PLATFORM_HINT, crate::steward::STEWARD_SYSTEM_PROMPT)
        } else {
            format!("{base}{}", PLATFORM_HINT)
        };
        (system_prompt, config.compaction.effective(&provider.id, model))
    };
    let agent = build_loop_agent(
        &system_prompt,
        state.kernel.as_ref(),
        state.compat.as_ref(),
        state.steward.as_ref(),
        is_steward_session,
        session_id,
        provider,
        model,
        Some(thinking),
        compaction,
        progress,
    )?;
    let arc = Arc::new(Mutex::new(agent));
    let serial = Arc::new(tokio::sync::Mutex::new(()));
    let mut map = state.loop_agents.lock().await;
    let entry = map.entry(session_id.to_string()).or_insert_with(|| LoopSessionEntry {
        agent: arc.clone(),
        serial: serial.clone(),
        provider_id: provider.id.clone(),
        model: model.to_string(),
        thinking: thinking.to_string(),
        startup_sent: false,
        last_used: std::time::Instant::now(),
    });
    // 既有条目参数不一致（前一个请求建了别的 provider/model）→ 以本次为准替换。
    // 串行锁保留旧条目持有的那把：并发在飞回合锁的就是它，替换后新回合仍会
    // 排队等旧回合结束（否则重建窗口两回合并行写同一事件日志）。
    if entry.provider_id != provider.id || entry.model != model || entry.thinking != thinking {
        *entry = LoopSessionEntry {
            agent: arc.clone(),
            serial: entry.serial.clone(),
            provider_id: provider.id.clone(),
            model: model.to_string(),
            thinking: thinking.to_string(),
            startup_sent: false,
            last_used: std::time::Instant::now(),
        };
    }
    entry.last_used = std::time::Instant::now();
    // 返回 map 中实际生效的组合（并发者已插入时用它的 agent 与锁）
    Ok((entry.agent.clone(), entry.serial.clone()))
}

// ============================================================================
// chat 入口（bm 引擎分支）
// ============================================================================

/// BM_LOOP_ENGINE=bm 的 chat 入口（切片 ①：无工具会话跑通）。
/// 与 pi 路径共享：会话校验 / 命名 / add_message（user 消息入 DB 由 chat.rs 完成）；
/// 不同：事件日志完全由 loop 拥有，本函数不再落任何事件。
pub async fn chat_bm(
    state: AppState,
    session: bm_core::db::Session,
    message: String,
    provider_override: Option<String>,
    model_override: Option<String>,
    thinking_override: Option<String>,
) -> Response {
    // 解析提供商与模型（与 pi 路径同规则：请求级 > 会话级 > 默认）
    let provider = {
        let config = state.config.read().await;
        match bm_core::config::resolve_provider(
            &config,
            provider_override.as_deref().or(session.provider_id.as_deref()),
        )
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "未配置任何模型提供商，请先在设置中配置".to_string(),
            )
        }) {
            Ok(p) => p,
            Err((status, msg)) => return crate::api_error(status, msg).into_response(),
        }
    };
    let model = {
        match bm_core::config::resolve_model(
            &provider,
            model_override.as_deref().or(session.model.as_deref()),
        )
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                format!("提供商 {} 未配置模型", provider.name),
            )
        }) {
            Ok(m) => m,
            Err((status, msg)) => return crate::api_error(status, msg).into_response(),
        }
    };

    // thinking 档位：切片② 已接映射（reasoning_effort 注入请求体；
    // 默认 off = 端点默认推理参数）
    let thinking = thinking_override.unwrap_or_else(|| "off".to_string());

    // 请求改参数 → header reason = change（与 pi 路径一致）
    let reason = if provider_override.is_some() || model_override.is_some() {
        HeaderReason::Change
    } else {
        HeaderReason::Initial
    };

    // 持久化新 provider/model 组合（后续消息沿用）
    if provider_override.is_some() || model_override.is_some() {
        let _ = state
            .db
            .set_session_model(&session.id, provider_override.as_deref(), model_override.as_deref())
            .await;
    }

    // 取消原语：watch 通道（run_turn 的 cancel 参数）。带 prompt_id 身份，
    // 清理时只删自己的条目（与 pi 路径同纪律）
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let prompt_id = crate::chat::next_prompt_id();
    state
        .bm_aborts
        .lock()
        .await
        .insert(session.id.clone(), (prompt_id, cancel_tx.clone()));

    // SSE 通道（unbounded：chunk 量受模型输出速率约束；hooks 只关心是否关闭）
    let (tx, rx) = mpsc::unbounded_channel::<AgentStreamEvent>();
    state.session_streams.lock().await.insert(session.id.clone(), tx.clone());
    let stream = UnboundedReceiverStream::new(rx)
        .map(|ev| Ok::<_, std::convert::Infallible>(crate::chat::to_sse_event(&ev)));

    let task_id = uuid::Uuid::new_v4().to_string();
    if let Err(err) = state.db.create_task(&task_id, &session.id).await {
        tracing::warn!(event = "bm.task_create_failed", error = %err, session = %session.id);
    }

    // 心跳进度（最近活动摘要：工具名 / 回复尾部，StreamHooks 更新）
    let progress: Arc<std::sync::Mutex<String>> =
        Arc::new(std::sync::Mutex::new(String::new()));

    let state_run = state.clone();
    let session_id = session.id.clone();
    tokio::spawn(async move {
        // 总超时：与 pi 路径同 15min，超时走取消通道（部分文本照常入库）
        let cancel_timeout = cancel_tx.clone();
        let timeout_task = tokio::spawn(async move {
            tokio::time::sleep(crate::chat::PROMPT_TIMEOUT).await;
            let _ = cancel_timeout.send(true);
        });

        // 心跳：每 5s 把内存进度刷库并推送 taskProgress SSE（切片②，
        // 与 pi 路径 chat.rs 同构——前端任务状态条零改动）
        let beat_stop = Arc::new(tokio::sync::Notify::new());
        {
            let beat_stop = beat_stop.clone();
            let db_beat = state_run.db.clone();
            let task_beat = task_id.clone();
            let progress_beat = progress.clone();
            let tx_beat = tx.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
                interval.tick().await; // 跳过首次立即触发
                loop {
                    tokio::select! {
                        _ = beat_stop.notified() => break,
                        _ = interval.tick() => {}
                    }
                    let p = progress_beat.lock().unwrap().clone();
                    if p.is_empty() {
                        continue;
                    }
                    let _ = db_beat.update_task_progress(&task_beat, &p).await;
                    if !tx_beat.is_closed() {
                        let _ = tx_beat.send(AgentStreamEvent::TaskProgress { progress: p });
                    }
                }
            });
        }

        run_bm_prompt(BmPromptParams {
            state: state_run.clone(), // 清理段还要用 state_run
            session_id: session_id.clone(), // 清理段还要用 session_id
            task_id,
            message,
            provider,
            model,
            thinking,
            reason,
            progress,
            cancel_tx,
            cancel_rx,
            tx: tx.clone(), // 清理段还要用 tx 做 same_channel 身份比对
        })
        .await;
        beat_stop.notify_waiters(); // 停心跳

        // 清理：身份匹配纪律与 pi 路径一致（新 prompt 已注册新通道/条目时不动）
        let mut streams = state_run.session_streams.lock().await;
        if let Some(tx2) = streams.get(&session_id)
            && tx2.same_channel(&tx)
        {
            streams.remove(&session_id);
        }
        let mut aborts = state_run.bm_aborts.lock().await;
        if let Some((pid, _)) = aborts.get(&session_id)
            && *pid == prompt_id
        {
            aborts.remove(&session_id);
        }
        timeout_task.abort(); // 15min 兜底任务随 prompt 结束即停，不驻留
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::default().interval(std::time::Duration::from_secs(15)))
        .into_response()
}

/// 一个 bm 引擎 prompt 回合的参数（对齐 pi 路径 PromptParams；所有权入参，
/// run_bm_prompt 是每个 prompt 一次的长任务，克隆成本可忽略）。
struct BmPromptParams {
    state: AppState,
    session_id: String,
    task_id: String,
    message: String,
    provider: bm_core::config::ProviderConfig,
    model: String,
    thinking: String,
    reason: HeaderReason,
    progress: Arc<std::sync::Mutex<String>>,
    cancel_tx: watch::Sender<bool>,
    cancel_rx: watch::Receiver<bool>,
    tx: mpsc::UnboundedSender<AgentStreamEvent>,
}

/// memory/write 写回契约（§5.1 记忆域）：事实写入记忆插件的同时落事件
/// 日志——记忆投影写回日志，防重放漂移（日志是唯一事实源，facts.md 是
/// 投影，损坏可由日志重放重建）。事件在回合开始前 append（审计序：
/// 先记忆后回合）。失败只 warn——记忆是增强不是正确性依赖（fail-safe）。
async fn append_memory_write_event(state: &AppState, session_id: &str, fact: &str) {
    let Some(dual) = state.dual_writer.as_ref() else {
        return;
    };
    let ev = EventKind::Core(CoreEvent::MemoryWrite {
        key: "facts.md".into(),
        data: serde_json::json!({ "fact": fact }),
    });
    if let Err(err) = dual
        .event_log()
        .append(
            SessionId::new(session_id),
            BranchId::new("main"),
            ev,
            bm_kernel::SurfaceIntent::None,
        )
        .await
    {
        tracing::warn!(
            event = "bm.memory_event_failed",
            error = %err,
            chars = fact.chars().count(),
            "memory/write 落日志失败（记忆文件已写，日志缺一条审计事实）"
        );
    }
}

/// 运行一个 bm 引擎 prompt 回合：agent 锁串行 → run_turn → 持久化 + 终态事件。
async fn run_bm_prompt(p: BmPromptParams) {
    let BmPromptParams {
        state,
        session_id,
        task_id,
        message,
        provider,
        model,
        thinking,
        reason,
        progress,
        cancel_tx,
        mut cancel_rx,
        tx,
    } = p;

    let (agent, serial) = match get_or_create_loop_agent(&state, &session_id, &provider, &model, &thinking, progress).await {
        Ok(pair) => pair,
        Err((_status, msg)) => {
            let _ = tx.send(AgentStreamEvent::Error { message: msg.clone() });
            let _ = state.db.finish_task(&task_id, "failed", Some(&msg)).await;
            return;
        }
    };
    // 会话串行：先取串行锁（同会话任何回合排队——chat/steward/重建互斥，
    // 事件日志唯一状态源），再取 agent 锁
    let _serial = serial.lock().await;

    // startup 插件事件懒发（每会话一次；ctx-compactor 借此加载项目配置）。
    // 挂点在 agent 就绪后、首条消息运行前；与 prompt 串行、最多 EVENT_TIMEOUT。
    if let Some(compat) = state.compat.clone() {
        let should_send = {
            let mut map = state.loop_agents.lock().await;
            match map.get_mut(&session_id) {
                Some(entry) if !entry.startup_sent => {
                    entry.startup_sent = true;
                    true
                }
                _ => false,
            }
        };
        if should_send {
            let cwd = {
                let config = state.config.read().await;
                config.working_dir.display().to_string()
            };
            if let Err(err) = compat
                .dispatch_event(
                    "startup",
                    serde_json::json!({ "type": "startup", "version": "1.0.0" }),
                    serde_json::json!({ "cwd": cwd, "hasUI": false }),
                )
                .await
            {
                tracing::debug!(event = "bm.plugin_event_failed", name = "startup", error = %err);
            }
        }
    }

    // 会话串行：agent 锁（同会话并发 prompt 排队，与 pi 路径 handle 锁同纪律）
    let mut agent = agent.lock().await;
    // governance.memorize 雏形（HANDOFF_KERNEL_PHASE1.md §九 第 2 条）：
    // 用户消息命中「记住」指令 → 记忆插件 remember + memory/write 事件落
    // 日志（写回契约：日志是唯一事实源，记忆文件是投影，重放可重建）。
    // 在 attach 之前执行（已持 agent 锁，取 hooks 内存句柄零竞态）；
    // 命中与否由 memorize 内部打日志（只记字符数，不落事实全文——
    // 用户内容不打日志纪律；事件日志本身是事实流，user/message 同源）。
    if let Some(memory) = agent.hooks().memory()
        && let Some(fact) = crate::governance::memorize(&memory, &message)
    {
        append_memory_write_event(&state, &session_id, &fact).await;
    }
    agent.hooks().attach(tx.clone(), cancel_tx);

    let outcome = agent
        .run_turn(
            TurnRequest {
                content: message,
                source: UserMsgSource::Human,
            },
            reason,
            &mut cancel_rx,
        )
        .await;
    agent.hooks().detach();
    // 释放 agent 锁（drop 前退出作用域约定；显式 drop 防 detach 后继续持锁）
    drop(agent);

    // 收尾：与 pi 路径同语义——无论成功/取消/失败，已生成文本照常入库
    let (task_status, task_error, terminal) = match &outcome {
        Ok(o) => {
            if !o.final_text.trim().is_empty() {
                let _ = state.db.add_message(&session_id, "assistant", &o.final_text).await;
                let _ = state.db.touch_session(&session_id).await;
            }
            tracing::info!(
                event = "bm.prompt_done",
                turn = o.turn,
                steps = o.steps,
                reason = ?o.reason,
                session = %session_id,
            );
            match o.reason {
                TurnEndReason::Completed => ("completed", None, AgentStreamEvent::Done),
                TurnEndReason::Cancelled => ("cancelled", Some("已取消".to_string()), AgentStreamEvent::Done),
                other => (
                    "failed",
                    Some(format!("回合失败: {other:?}")),
                    AgentStreamEvent::Error { message: "agent 执行失败".to_string() },
                ),
            }
        }
        Err(e) => (
            "failed",
            Some(e.to_string()),
            AgentStreamEvent::Error { message: e.to_string() },
        ),
    };
    if !tx.is_closed() {
        let _ = tx.send(terminal);
    }
    if let Err(err) = state.db.finish_task(&task_id, task_status, task_error.as_deref()).await {
        tracing::warn!(event = "bm.task_finish_failed", error = %err, task = %task_id);
    }
}

// ============================================================================
// 管家（Steward）—— 自我驱动三件套（架构 §14.1/§14.2，v0.19）
// ============================================================================
//
// 投喂侧组件：与 chat 路径共用同一套 loop agent（回合源三分法），区别只在
// 回合来源（Goal = 调度器定时到期 / Inject = OS 层汇报）与是否接前端 SSE
// （管家回合无人监听，attach 用内部通道；事件日志照常落，会话投影可见）。

/// 运行一个管家回合（调度器到点 / inject 汇报共用）。
///
/// - 会话/provider/model 解析与 chat 路径同规则（请求级 > 会话级 > 默认）；
/// - 回合源 = Goal（定时唤醒）或 Inject（OS 汇报）；
/// - 不走 session_streams（无前端监听）、不建 task 记录（无进度条消费）；
/// - 15min 超时兜底（与 chat 同纪律，防模型挂死阻塞会话锁）；
/// - 结果落库 assistant 消息 + touch_session（与 chat 收尾同语义）。
pub async fn run_steward_turn(
    state: &AppState,
    session_id: &str,
    message: String,
    source: UserMsgSource,
) -> Result<(), String> {
    // 会话必须存在（被删则不投喂；调度器侧已查，inject 侧兜底）
    let Some(session) = state
        .db
        .get_session(session_id)
        .await
        .map_err(|e| format!("读取管家会话失败: {e}"))?
    else {
        return Err(format!("管家会话不存在: {session_id}"));
    };
    // 成本杠杆（v0.20）：StewardConfig（BM_STEWARD_PROVIDER/BM_STEWARD_MODEL）
    // 指定时管家回合用低成本模型（24×7 心跳主要烧钱点，§14.2）；配错回落会话级
    let (provider, model) = {
        let config = state.config.read().await;
        resolve_steward_llm(
            &config,
            session.provider_id.as_deref(),
            session.model.as_deref(),
            state.steward_cfg.provider.as_deref(),
            state.steward_cfg.model.as_deref(),
        )?
    };
    let progress: Arc<std::sync::Mutex<String>> = Arc::new(std::sync::Mutex::new(String::new()));
    let (agent, serial) = get_or_create_loop_agent(state, session_id, &provider, &model, "off", progress)
        .await
        .map_err(|(_, msg)| msg)?;
    // 会话串行：chat 回合进行中管家回合必须排队（同一事件日志唯一状态源）
    let _serial = serial.lock().await;

    // attach 内部通道：无人消费的 unbounded（hooks.try_send 只关心关闭）；
    // 取消 watch：15min 超时兜底
    let (tx, _rx) = mpsc::unbounded_channel::<AgentStreamEvent>();
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    let timeout_tx = cancel_tx.clone();
    let timeout_task = tokio::spawn(async move {
        tokio::time::sleep(crate::chat::PROMPT_TIMEOUT).await;
        let _ = timeout_tx.send(true);
    });

    // 静默窗口 watchdog（v0.20，架构 §14.1"1 分钟无汇报主动上报"落地）：
    // 回合进行中若超过窗口无任何新事件（head_seq 不变 = 无文本/工具活动，
    // 模型可能挂死/网络卡死），宿主侧主动取消 + 告警——15min 总超时是兜底，
    // 静默窗口提前掐断无进展回合防烧 token。窗口秒数来自 StewardConfig
    // （BM_STEWARD_SILENCE_WINDOW_S，0 = 禁用）；事件日志是唯一活动源
    // （progress 是会话级共享内存，可能属于 chat 路径创建的 agent，不可作
    // 管家活动依据）。
    let silence_window = state.steward_cfg.silence_window_s;
    let watchdog = if silence_window > 0 {
        let cancel_watch = cancel_tx.clone();
        let store_watch = state.dual_writer.as_ref().map(|d| d.event_log().store());
        let sid_watch = SessionId::new(session_id);
        let bid_watch = BranchId::new("main");
        let session_watch = session_id.to_string();
        Some(tokio::spawn(async move {
            let Some(store) = store_watch else {
                return;
            };
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            interval.tick().await; // 跳过首次立即触发
            let mut last = store.head_seq(&sid_watch, &bid_watch).await.ok().flatten();
            let mut silent_since = std::time::Instant::now();
            loop {
                interval.tick().await;
                let cur = store.head_seq(&sid_watch, &bid_watch).await.ok().flatten();
                if cur != last {
                    last = cur;
                    silent_since = std::time::Instant::now();
                    continue;
                }
                if silent_since.elapsed()
                    >= std::time::Duration::from_secs(silence_window as u64)
                {
                    tracing::warn!(
                        event = "bm.steward_silence_timeout",
                        window_s = silence_window,
                        session = %session_watch,
                        "静默窗口超时（回合内无任何事件），宿主主动取消"
                    );
                    let _ = cancel_watch.send(true);
                    return;
                }
            }
        }))
    } else {
        None
    };

    let mut agent = agent.lock().await;
    agent.hooks().attach(tx, cancel_tx);
    let outcome = agent
        .run_turn(
            TurnRequest {
                content: message,
                source: source.clone(),
            },
            HeaderReason::Initial,
            &mut cancel_rx,
        )
        .await;
    agent.hooks().detach();
    drop(agent);
    if let Some(w) = watchdog {
        w.abort(); // 回合结束即停（否则会继续 tick 直到窗口超时）
    }
    timeout_task.abort(); // 15min 兜底任务随回合结束即停，不驻留

    match &outcome {
        Ok(o) => {
            if !o.final_text.trim().is_empty() {
                let _ = state.db.add_message(session_id, "assistant", &o.final_text).await;
                let _ = state.db.touch_session(session_id).await;
            }
            tracing::info!(
                event = "bm.steward_turn_done",
                source = ?source,
                turn = o.turn,
                steps = o.steps,
                reason = ?o.reason,
                chars = o.final_text.chars().count(),
                session = %session_id,
            );
            Ok(())
        }
        Err(err) => Err(format!("管家回合失败: {err}")),
    }
}

/// 投喂一个管家回合的公共封装（调度器 / boot 汇报共用）：in_flight 防重叠 +
/// 回合执行 + 锚点推进（note_round_done 只推 last_wake_at，不清 next_wake_at
/// ——管家回合内 set_wake 写好的下次唤醒原样保留；失败/没写 = 0 静默，
/// 不触发失败重试风暴）。调用方必须已确认管家启用且会话存在。
pub async fn dispatch_steward_round(
    state: &AppState,
    store: &crate::steward::StewardStore,
    message: String,
    source: UserMsgSource,
) {
    if store.in_flight().await {
        return;
    }
    let Some(session_id) = store.session_id().await else {
        return;
    };
    store.set_in_flight(true).await;
    let now = crate::steward::now_ms();
    if let Err(err) = run_steward_turn(state, &session_id, message, source).await {
        tracing::warn!(
            event = "bm.steward_turn_failed",
            error = %err,
            session = %session_id
        );
        // 失败不重试：清掉到点的唤醒登记（不清会每 10s 重投失败回合，
        // 回看 P1——注释承诺"失败=0 静默"但此前实现未做）
        store.clear_next_wake().await;
    }
    store.set_in_flight(false).await;
    store.note_round_done(now).await;
}

/// 调度器（三件套 ①）：每 10s 检查管家唤醒登记，到点投喂 Goal 回合。
///
/// 治理（OpenClaw next_check 吸收）：管家回合内用 set_wake 提议下次唤醒，
/// 治理层夹 [pacing-min, pacing-max]；回合失败不重试（next_wake_at 保持
/// 0 = 静默），防止失败风暴烧 token。in_flight 防重叠投喂。
pub fn spawn_steward_scheduler(state: AppState, store: Arc<crate::steward::StewardStore>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        interval.tick().await; // 跳过首次立即触发
        loop {
            interval.tick().await;
            let now = crate::steward::now_ms();
            if !store.should_wake(now).await {
                continue;
            }
            let Some(session_id) = store.session_id().await else {
                continue;
            };
            // 会话已删：清掉唤醒登记（不再追投）并复位锚点
            let Ok(Some(_)) = state.db.get_session(&session_id).await else {
                tracing::warn!(event = "bm.steward_session_gone", session = %session_id);
                store.note_round_done(now).await;
                continue;
            };
            let reason = store.last_reason().await.unwrap_or_default();
            let since_s = (now.saturating_sub(
                store.snapshot().await.last_wake_at_ms,
            )) / 1000;
            let message = format!(
                "你是管家。这是你的定时唤醒回合：距离上次回合 {since_s} 秒，上次登记原因：{reason}。\
                 请观察当前状态；如有需要采取的行动请执行；回合结束时调用 set_wake \
                 登记下次唤醒时间（无事请登记较长间隔保持静默）。"
            );
            dispatch_steward_round(&state, &store, message, UserMsgSource::Goal).await;
        }
    });
}

/// OS 层主动汇报通道（三件套 ②）：事件 → 立即投喂一个 Inject 回合，
/// 可顺带登记下次唤醒（夹区间）。HTTP handler 见 lib.rs 路由。
///
/// 锚点推进与 dispatch_steward_round 一致：inject 同样是完成的回合，
/// 不推 last_wake_at 会让前端"上次回合"误显静默、调度器 since 从 0 起算
/// （2026-08-15 真实验收实证，见 HANDOFF §〇·五 38）。
pub async fn steward_inject(
    state: AppState,
    store: Arc<crate::steward::StewardStore>,
    message: String,
    wake_after_seconds: Option<i64>,
) -> Result<(), String> {
    let Some(session_id) = store.session_id().await else {
        return Err("管家未启用（BM_STEWARD_SESSION 未设置）".to_string());
    };
    store.register_wake(wake_after_seconds).await?;
    let now = crate::steward::now_ms();
    let result = run_steward_turn(&state, &session_id, message, UserMsgSource::Inject).await;
    if result.is_err() {
        // 失败回退本次唤醒登记（失败不重试，防失败风暴，与 dispatch 同语义）
        store.clear_next_wake().await;
    }
    store.note_round_done(now).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_config_prefers_user_base_url() {
        use bm_core::config::{ProviderConfig, ProviderKind};
        let p = ProviderConfig {
            id: "custom-1".into(),
            name: "Custom".into(),
            kind: ProviderKind::Deepseek,
            base_url: Some("https://my.deepseek.example/v1/".into()),
            api_key: Some("sk-test".into()),
            models: vec!["deepseek-chat".into()],
            default_model: Some("deepseek-chat".into()),
        };
        let cfg = resolve_llm_config(&p, "deepseek-chat", None).unwrap();
        assert_eq!(cfg.base_url, "https://my.deepseek.example/v1", "用户端点优先且去尾斜杠");
        assert_eq!(cfg.api_key, "sk-test");
        assert_eq!(cfg.provider.as_deref(), Some("custom-1"));
        assert!(cfg.reasoning_effort.is_none(), "off/None 不注入推理参数");

        // 官方端点回退
        let p2 = ProviderConfig { base_url: None, ..p };
        let cfg2 = resolve_llm_config(&p2, "deepseek-chat", Some("high")).unwrap();
        assert!(cfg2.base_url.contains("api.deepseek.com"));
        assert_eq!(cfg2.reasoning_effort.as_deref(), Some("high"), "high 档映射 high");

        // custom 无端点 → 拒绝
        let p3 = ProviderConfig {
            id: "c".into(),
            kind: ProviderKind::Custom,
            base_url: None,
            ..p2
        };
        let err = resolve_llm_config(&p3, "m", None).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn thinking_maps_to_reasoning_effort() {
        assert_eq!(reasoning_effort_for("off"), None);
        assert_eq!(reasoning_effort_for("none"), None);
        assert_eq!(reasoning_effort_for("minimal"), Some("low"));
        assert_eq!(reasoning_effort_for("low"), Some("low"));
        assert_eq!(reasoning_effort_for("medium"), Some("medium"));
        assert_eq!(reasoning_effort_for("high"), Some("high"));
        assert_eq!(reasoning_effort_for("xhigh"), Some("high"));
        assert_eq!(reasoning_effort_for("max"), Some("high"));
        assert_eq!(reasoning_effort_for(""), None);
    }

    #[test]
    fn silence_window_parsing() {
        // 合法正数 → 启用；0 → 禁用；负数/非法/缺失 → None（回落默认）
        assert_eq!(parse_silence_window(Some("60")), Some(60));
        assert_eq!(parse_silence_window(Some(" 120 ")), Some(120), "容忍空白");
        assert_eq!(parse_silence_window(Some("0")), Some(0), "0 = 禁用");
        assert_eq!(parse_silence_window(Some("-5")), None);
        assert_eq!(parse_silence_window(Some("abc")), None);
        assert_eq!(parse_silence_window(Some("")), None);
        assert_eq!(parse_silence_window(None), None);
    }

    fn test_config() -> bm_core::config::AppConfig {
        use bm_core::config::{AppConfig, ProviderConfig, ProviderKind};
        let mut config = AppConfig::default();
        config.providers = vec![
            ProviderConfig {
                id: "minimax".into(),
                name: "MiniMax".into(),
                kind: ProviderKind::Minimax,
                base_url: None,
                api_key: Some("sk-test".into()),
                models: vec!["MiniMax-M3".into(), "MiniMax-Text-01".into()],
                default_model: Some("MiniMax-M3".into()),
            },
            ProviderConfig {
                id: "deepseek".into(),
                name: "DeepSeek".into(),
                kind: ProviderKind::Deepseek,
                base_url: None,
                api_key: Some("sk-test2".into()),
                models: vec!["deepseek-chat".into()],
                default_model: Some("deepseek-chat".into()),
            },
        ];
        config.default_provider = Some("minimax".into());
        config
    }

    #[test]
    fn steward_llm_falls_back_to_session_rules() {
        let config = test_config();
        // 无 env：会话级 provider（deepseek）+ 会话级 model
        let (p, m) = resolve_steward_llm(&config, Some("deepseek"), Some("deepseek-chat"), None, None)
            .unwrap();
        assert_eq!(p.id, "deepseek");
        assert_eq!(m, "deepseek-chat");
        // 会话无 model → 提供商默认模型
        let (p, m) = resolve_steward_llm(&config, Some("minimax"), None, None, None).unwrap();
        assert_eq!(p.id, "minimax");
        assert_eq!(m, "MiniMax-M3");
        // 会话无 provider → 默认提供商
        let (p, _) = resolve_steward_llm(&config, None, None, None, None).unwrap();
        assert_eq!(p.id, "minimax");
    }

    #[test]
    fn steward_llm_env_overrides_provider_and_model() {
        let config = test_config();
        // env provider + env model 同时接管（低成本模型跑 24×7 心跳）
        let (p, m) = resolve_steward_llm(
            &config,
            Some("minimax"),
            Some("MiniMax-M3"),
            Some("deepseek"),
            Some("deepseek-chat"),
        )
        .unwrap();
        assert_eq!(p.id, "deepseek", "env 提供商优先于会话级");
        assert_eq!(m, "deepseek-chat");
        // 只给 env model（在提供商列表内 → 直接用）
        let (p, m) =
            resolve_steward_llm(&config, Some("minimax"), None, None, Some("MiniMax-Text-01"))
                .unwrap();
        assert_eq!(p.id, "minimax");
        assert_eq!(m, "MiniMax-Text-01");
        // env model 但提供商 models 为空（模型列表未知）→ 信任 env
        let mut empty_models = test_config();
        empty_models.providers[0].models = vec![];
        let (_, m) =
            resolve_steward_llm(&empty_models, Some("minimax"), None, None, Some("mini-any"))
                .unwrap();
        assert_eq!(m, "mini-any");
    }

    #[test]
    fn steward_llm_bad_env_falls_back() {
        let config = test_config();
        // env 提供商不存在 → 回落会话级
        let (p, _) = resolve_steward_llm(&config, Some("minimax"), None, Some("nope"), None).unwrap();
        assert_eq!(p.id, "minimax");
        // env 模型不在提供商列表 → 回落会话级解析
        let (_, m) = resolve_steward_llm(&config, Some("minimax"), None, None, Some("no-such-model"))
            .unwrap();
        assert_eq!(m, "MiniMax-M3");
        // 无任何提供商 → 报错
        let mut empty = test_config();
        empty.providers = vec![];
        let err = resolve_steward_llm(&empty, None, None, None, None).unwrap_err();
        assert!(err.contains("未配置任何模型提供商"), "{err}");
    }

    #[tokio::test]
    async fn stream_hooks_forwards_and_detects_disconnect() {
        let hooks = StreamHooks::new(Arc::new(std::sync::Mutex::new(String::new())), None);
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentStreamEvent>();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        hooks.attach(tx, cancel_tx);

        // 通道存活：转发 TextDelta
        let mut h = hooks;
        h.on_stream_chunk(&StepCtx { turn: 1, step: 1 }, "你好");
        match rx.recv().await {
            Some(AgentStreamEvent::TextDelta { delta }) => assert_eq!(delta, "你好"),
            other => panic!("应收到 TextDelta，得到 {other:?}"),
        }

        // B4：工具调用起止事件（前端工具卡片），起 Allow 止 is_error
        let tool_ctx = ToolCtx {
            turn: 1,
            step: 1,
            call_id: "c1".into(),
            name: "hello".into(),
            args: serde_json::json!({ "name": "Boen" }),
        };
        assert_eq!(h.on_tool_pre(&tool_ctx), ToolGate::Allow);
        match rx.recv().await {
            Some(AgentStreamEvent::ToolCallStart { id, name, args }) => {
                assert_eq!((id.as_str(), name.as_str()), ("c1", "hello"));
                assert_eq!(args, serde_json::json!({ "name": "Boen" }));
            }
            other => panic!("应收到 ToolCallStart，得到 {other:?}"),
        }
        h.on_tool_post(&tool_ctx, false);
        match rx.recv().await {
            Some(AgentStreamEvent::ToolCallEnd { id, is_error, .. }) => {
                assert_eq!(id, "c1");
                assert!(is_error, "执行失败应标记 is_error");
            }
            other => panic!("应收到 ToolCallEnd，得到 {other:?}"),
        }

        // 通道关闭（客户端断开）：触发取消
        drop(rx);
        h.on_stream_chunk(&StepCtx { turn: 1, step: 1 }, "断开");
        assert!(*cancel_rx.borrow(), "断开后应置取消");

        h.detach();
        h.on_stream_chunk(&StepCtx { turn: 1, step: 1 }, "已摘除");
    }
}
