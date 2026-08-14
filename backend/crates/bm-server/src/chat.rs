//! 流式对话：POST /api/chat → SSE；POST /api/chat/stop 取消进行中的 prompt。
//!
//! 协议（SSE 事件，data 为 JSON）：
//! - `{"type":"textDelta","delta":"..."}`      正文增量（思考文本以 <think> 标签随正文下发）
//! - `{"type":"toolCallStart","name":"..."}`   工具调用开始（携带完整参数）
//! - `{"type":"toolCallEnd","name":"..."}`     工具调用结束（isError 决定颜色）
//! - `{"type":"turnEnd"}`                      回合结束
//! - `{"type":"done"}`                         整个 prompt 结束（含取消，前端据此固化内容）
//! - `{"type":"error","message":"..."}`        出错
//!
//! 取消语义：
//! - 前端点「停止」→ POST /api/chat/stop → 触发 pi AbortSignal → prompt 尽快返回，
//!   已生成的文本照常入库并下发 done；
//! - 客户端断开 SSE（关窗/切会话）→ 事件通道关闭 → 自动 abort 后端 prompt，
//!   避免继续烧 token（部分文本仍入库）。
//!
//! 用户消息与最终助手消息均持久化到 SQLite。

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use bm_core::agent::{AgentStreamEvent, create_session_handle, map_agent_event};
use bm_kernel::SurfaceIntent;
use bm_protocol::{
    AssistantMsg, BranchId, CallId, CoreEvent, EpochHeader, EventKind, HeaderReason, SeqNo,
    SessionId, StreamChunk, TokenUsage, ToolResultMsg, TurnEndReason, UserMsg, UserMsgSource,
};
use serde::Deserialize;
use tokio::sync::{Mutex, mpsc};
use tokio_stream::{StreamExt, wrappers::UnboundedReceiverStream};

use crate::{AppState, PermissionDecision, api_error};

/// 权限询问事件推送：经会话当前活跃 prompt 的 SSE 通道发给前端。
/// prompt 结束后通道已移除 → 事件丢失（询问仍会超时 fail-closed，无泄漏）。
/// 入参是 AppState 的 session_streams 组件（pi 权限桥与 bm 引擎桥共用；
/// bm 引擎的 CompatEngine 建于 AppState 之前，只能拿组件）。
pub async fn send_permission_request(
    session_streams: &tokio::sync::Mutex<
        HashMap<String, tokio::sync::mpsc::UnboundedSender<AgentStreamEvent>>,
    >,
    session_id: &str,
    request_id: &str,
    extension_id: &str,
    capability: &str,
    message: &str,
) {
    let tx = session_streams.lock().await.get(session_id).cloned();
    if let Some(tx) = tx {
        let _ = tx.send(AgentStreamEvent::PermissionRequest {
            id: request_id.to_string(),
            extension_id: Some(extension_id.to_string()),
            capability: capability.to_string(),
            message: message.to_string(),
        });
    }
}

/// 各语言下的默认新会话标题（前端创建会话时按界面语言传入，
/// 首条消息前识别为"未命名"，自动用消息开头命名）。
const DEFAULT_TITLES: [&str; 4] = ["新对话", "New chat", "新しいチャット", "새 채팅"];

/// prompt 总超时：上游挂起（连接建立后不返回数据）时不能永久锁死会话。
/// 超时走 abort 通道，与用户点停止同一条收尾路径（部分文本照常入库）。
pub(crate) const PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// 全局 prompt 序号：aborts 表中区分同会话的先后 prompt（见 AppState.aborts）。
static PROMPT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 取下一个 prompt 序号（pi 与 bm 引擎共用同一序号空间，身份匹配用）。
pub(crate) fn next_prompt_id() -> u64 {
    PROMPT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// 聊天请求：会话必须已存在（前端先创建会话再发消息）。
/// `provider`/`model`/`thinking` 可选，用于在当前会话即时切换提供商/模型与思考强度。
#[derive(Deserialize)]
pub struct ChatRequest {
    pub session_id: String,
    pub message: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub thinking: Option<String>,
}

pub async fn chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Response {
    let message = req.message.trim().to_string();
    if message.is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "消息不能为空").into_response();
    }

    let session = match state.db.get_session(&req.session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "会话不存在").into_response(),
        Err(err) => {
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    };

    // 首次消息时自动用消息开头命名会话（各语言默认标题均视为未命名）
    if DEFAULT_TITLES.contains(&session.title.as_str()) {
        let title: String = message.chars().take(24).collect();
        let _ = state.db.rename_session(&session.id, &title).await;
    }

    // 持久化用户消息
    if let Err(err) = state.db.add_message(&session.id, "user", &message).await {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    // —— A6 接线：bm 引擎分支（引擎选择：env 双开/回退通道 > settings 前端
    // 开关 > 默认 bm——自研引擎已切换为默认，pi 保留为可回退选项）——
    // 开关只影响新 prompt 的执行引擎；事件日志是两条路径的共同事实源
    //（bm 路径下 loop 拥有日志全生命周期，本函数此后不再落日志）。
    let loop_engine = crate::bm_engine::resolve_loop_engine(
        std::env::var("BM_LOOP_ENGINE").ok().as_deref(),
        state.config.read().await.loop_engine.as_deref(),
    );
    if crate::bm_engine::loop_engine_is_bm(&loop_engine) {
        return crate::bm_engine::chat_bm(state, session, message, req.provider, req.model, req.thinking).await;
    }

    // 阶段 0 双写：用户消息 → 事件日志（失败不阻断主链路）
    if let Some(w) = &state.dual_writer {
        w.append_best_effort(
            SessionId::new(&session.id),
            EventKind::Core(CoreEvent::UserMessage {
                msg: UserMsg {
                    content: message.clone(),
                },
                source: UserMsgSource::Human,
            }),
            SurfaceIntent::Append,
        )
        .await;
    }

    // 获取或创建 agent 会话句柄（Arc<Mutex<..>> 保证同一会话串行且 map 锁不长期占用）。
    // 返回 (句柄, 解析后的 provider_id, 解析后的 model)——request/header 事件用
    let (handle, resolved_provider, resolved_model) = match get_or_create_agent(
        &state,
        &session,
        req.provider.as_deref(),
        req.model.as_deref(),
        req.thinking.as_deref(),
    )
    .await
    {
        Ok(h) => h,
        Err((status, msg)) => return api_error(status, msg).into_response(),
    };

    // —— A2：request/header 审计锚点 ——
    // prompt_hash = BoenMind 注入面的 sha256（自定义系统提示词 + skills + 扩展路径
    // = QuickJS 已注册工具的确定性代理）；reason：请求改参数 = change，否则 initial
    let header_reason = if req.provider.is_some() || req.model.is_some() {
        HeaderReason::Change
    } else {
        HeaderReason::Initial
    };
    let prompt_hash = {
        let config = state.config.read().await;
        prompt_hash_of(
            config.custom_system_prompt.as_deref().unwrap_or(""),
            &bm_core::skills::enabled_skills_prompt(&config),
            &bm_core::plugins::enabled_extension_paths(&config),
        )
    };

    // 请求指定了 provider/model 且与会话记录不同：持久化，后续消息沿用新组合
    if req.provider.is_some() || req.model.is_some() {
        let _ = state
            .db
            .set_session_model(&session.id, req.provider.as_deref(), req.model.as_deref())
            .await;
    }

    // 本次 prompt 的取消原语（pi 官方 abort 机制）：注册到会话级表，
    // POST /api/chat/stop 按 session_id 触发；客户端断开时由 watcher 自动触发。
    // 带 prompt_id 身份：同会话连续请求时先结束的只删自己的条目（见清理处）。
    let (abort_handle, abort_signal) = pi::sdk::AgentSessionHandle::new_abort_handle();
    let prompt_id = next_prompt_id();
    state.aborts.lock().await.insert(session.id.clone(), (prompt_id, abort_handle.clone()));

    // 事件通道 → SSE（unbounded：背压由回调缓冲队列承担，见转发 task）
    let (tx, rx) = mpsc::unbounded_channel::<AgentStreamEvent>();
    // 注册会话的活跃事件通道：权限询问桥据此把询问事件推给前端
    state.session_streams.lock().await.insert(session.id.clone(), tx.clone());
    let stream = UnboundedReceiverStream::new(rx)
        .map(|ev| Ok::<_, std::convert::Infallible>(to_sse_event(&ev)));

    let state_clone = state.clone();
    let session_id = session.id.clone();
    let task_id = uuid::Uuid::new_v4().to_string();
    // 任务实体：一次 prompt 回合一条（断线续跑 + 心跳进度的持久化载体）
    if let Err(err) = state.db.create_task(&task_id, &session_id).await {
        tracing::warn!(event = "bm.task_create_failed", error = %err, session = %session_id);
    }
    tokio::spawn(async move {
        // prompt 总超时：上游挂起时 abort（与用户停止同路径收尾，任务标记 cancelled）。
        // prompt 正常结束后该任务残留至超时点再退出，此时 abort 已无监听者，无副作用。
        let timeout_abort = abort_handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(PROMPT_TIMEOUT).await;
            timeout_abort.abort();
        });
        run_prompt_and_persist(PromptParams {
            state: state_clone.clone(),
            session_id: session_id.clone(),
            task_id,
            handle,
            abort_signal,
            message,
            tx: tx.clone(),
            meta: PromptMeta {
                provider_id: resolved_provider,
                model: resolved_model,
                prompt_hash,
                reason: header_reason,
            },
        })
        .await;
        // 移除本 prompt 的活跃事件通道（同会话新 prompt 已注册新通道时不动）
        let mut streams = state_clone.session_streams.lock().await;
        if let Some(tx2) = streams.get(&session_id)
            && tx2.same_channel(&tx)
        {
            streams.remove(&session_id);
        }
        // 清理自己的 abort 条目：同会话已有新 prompt（prompt_id 已更新）时不动
        let mut aborts = state_clone.aborts.lock().await;
        if let Some((pid, _)) = aborts.get(&session_id)
            && *pid == prompt_id
        {
            aborts.remove(&session_id);
        }
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::default().interval(std::time::Duration::from_secs(15)))
        .into_response()
}

/// 取消进行中的 prompt（幂等：无进行中 prompt 时返回 ok）。
/// 后端 abort 后 prompt 尽快返回，已生成的部分文本照常入库并下发 done。
#[derive(Deserialize)]
pub struct StopChatRequest {
    pub session_id: String,
}

pub async fn stop_chat(
    State(state): State<AppState>,
    Json(req): Json<StopChatRequest>,
) -> Response {
    // pi 引擎 abort 与 bm 引擎 watch 取消通道都试一遍（幂等：
    // 任一命中即生效；同会话只会有一个引擎在跑）
    let mut stopped = false;
    if let Some((_, abort)) = state.aborts.lock().await.remove(&req.session_id) {
        abort.abort();
        stopped = true;
    }
    if let Some((_, cancel)) = state.bm_aborts.lock().await.remove(&req.session_id) {
        let _ = cancel.send(true);
        stopped = true;
    }
    if stopped {
        tracing::info!(event = "bm.chat_stopped", session = %req.session_id);
    }
    axum::Json(serde_json::json!({ "ok": true })).into_response()
}

/// 前端对插件权限询问的决策回传（允许一次 / 拒绝 / 总是允许-拒绝）。
/// 无挂起询问时幂等返回 ok（可能已超时）。
#[derive(Deserialize)]
pub struct PermissionResponseRequest {
    pub request_id: String,
    #[serde(default)]
    pub allow: bool,
    /// 总是允许/总是拒绝：写入白名单，下次不再询问
    #[serde(default)]
    pub always: bool,
}

pub async fn respond_permission(
    State(state): State<AppState>,
    Json(req): Json<PermissionResponseRequest>,
) -> Response {
    if let Some(tx) = state.permission_pending.lock().await.remove(&req.request_id) {
        let _ = tx.send(PermissionDecision {
            allow: req.allow,
            always: req.always,
        });
        tracing::info!(
            event = "bm.permission_responded",
            request = %req.request_id,
            allow = req.allow,
            always = req.always,
        );
    }
    axum::Json(serde_json::json!({ "ok": true })).into_response()
}

/// 获取会话的 agent 句柄；不存在则按会话记录的提供商/模型创建。
/// `provider`/`model`/`thinking` 有值时对已存在的会话即时生效（set_model / set_thinking_level）。
/// 返回 (句柄, 解析后的 provider_id, 解析后的 model)——request/header 事件标识用。
async fn get_or_create_agent(
    state: &AppState,
    session: &bm_core::db::Session,
    provider_override: Option<&str>,
    model_override: Option<&str>,
    thinking_override: Option<&str>,
) -> Result<(Arc<Mutex<pi::sdk::AgentSessionHandle>>, String, String), (StatusCode, String)> {
    // 解析提供商与模型：请求级 provider/model 优先（跨提供商切换模型时，
    // 只切 model 会导致 model 不属于会话原提供商 → pi 降级默认路由 → 401）
    let (provider, model) = {
        let config = state.config.read().await;
        let provider = bm_core::config::resolve_provider(
            &config,
            provider_override.or(session.provider_id.as_deref()),
        )
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "未配置任何模型提供商，请先在设置中配置".to_string(),
            )
        })?;
        let model = bm_core::config::resolve_model(&provider, model_override.or(session.model.as_deref()))
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("提供商 {} 未配置模型", provider.name),
                )
            })?;
        (provider, model)
    };

    // 会话句柄已存在：先 clone 出 handle（map 锁立即释放），再切换模型/思考强度。
    // 注意不能在持 map 锁时 await handle 锁：该会话的 prompt 可能还在跑，
    // handle 锁会阻塞到 prompt 结束，期间 map 锁被占用会卡住所有其它会话
    let handle_arc = {
        let mut agents = state.agents.lock().await;
        if let Some(entry) = agents.get_mut(&session.id) {
            entry.last_used = std::time::Instant::now();
            Some(entry.handle.clone())
        } else {
            None
        }
    };
    if let Some(arc) = handle_arc {
        {
            let mut handle = arc.lock().await;
            if let Some(pid) = model_override {
                let provider_name = provider.kind.pi_name(&provider.id);
                handle
                    .set_model(&provider_name, pid)
                    .await
                    .map_err(|err| (StatusCode::BAD_GATEWAY, format!("切换模型失败: {err}")))?;
            }
            if let Some(level) = thinking_override
                && let Ok(level) = level.parse::<pi::model::ThinkingLevel>() {
                    handle
                        .set_thinking_level(level)
                        .await
                        .map_err(|err| (StatusCode::BAD_GATEWAY, format!("切换思考强度失败: {err}")))?;
                }
        }
        return Ok((arc, provider.id.clone(), model.clone()));
    }

    // 新建句柄所需的配置字段一次读锁取齐（避免多次串行读锁）
    let (working_dir, extension_paths, skills_prompt, compaction, extension_policy, extension_allow_dangerous, custom_prompt) = {
        let config = state.config.read().await;
        (
            config.working_dir.clone(),
            bm_core::plugins::enabled_extension_paths(&config),
            bm_core::skills::enabled_skills_prompt(&config),
            bm_core::compaction::resolve_for_model_with_default_path(
                &config.compaction,
                &provider.kind.pi_name(&provider.id),
                &model,
            ),
            config.extension_policy.clone(),
            config.extension_allow_dangerous.unwrap_or(false),
            config.custom_system_prompt.clone().unwrap_or_default(),
        )
    };

    // 插件权限询问桥：能力确认转发给前端聊天界面（每个会话一个实例）
    let ui_handler: Option<Arc<dyn pi::extension_dispatcher::ExtensionUiHandler + Send + Sync>> =
        Some(Arc::new(crate::permission::PermissionBridge {
            state: state.clone(),
            session_id: session.id.clone(),
        }));

    let handle = create_session_handle(
        &provider,
        &model,
        &working_dir,
        extension_paths,
        &skills_prompt,
        thinking_override,
        compaction,
        extension_policy,
        extension_allow_dangerous,
        ui_handler,
        &custom_prompt,
    )
    .await
    .map_err(|err| (StatusCode::BAD_GATEWAY, format!("创建 agent 会话失败: {err}")))?;

    let mut agents = state.agents.lock().await;
    let entry = agents
        .entry(session.id.clone())
        .or_insert_with(|| crate::AgentSessionEntry {
            handle: Arc::new(Mutex::new(handle)),
            last_used: std::time::Instant::now(),
        });
    entry.last_used = std::time::Instant::now();
    Ok((entry.handle.clone(), provider.id.clone(), model.clone()))
}

/// 运行 prompt，将事件转发到通道；结束后把助手消息（含工具调用）写入数据库。
/// `task_id` 为本次 prompt 回合的任务实体（心跳进度 + 终态落库，见 db::Task）。
/// `meta` 为 request/header 事件内容（A2：模型调用链标识 + 输入审计锚点）。
struct PromptParams {
    state: AppState,
    session_id: String,
    task_id: String,
    handle: Arc<Mutex<pi::sdk::AgentSessionHandle>>,
    abort_signal: pi::sdk::AbortSignal,
    message: String,
    tx: mpsc::UnboundedSender<AgentStreamEvent>,
    meta: PromptMeta,
}

async fn run_prompt_and_persist(p: PromptParams) {
    let PromptParams {
        state,
        session_id,
        task_id,
        handle,
        abort_signal,
        message,
        tx,
        meta,
    } = p;
    // 同一会话的并发 prompt 串行（map 锁已在 get_or_create_agent 后释放）
    let mut handle = handle.lock().await;

    // —— 阶段 0 双写：回合开始（事件日志；失败不阻断主链路）——
    // 回合号 = 已落日志 TurnStart 计数 + 1（count 查询，不做全量重放）
    let dual = state.dual_writer.clone();
    let mut turn: u32 = 1;
    let log_sid = SessionId::new(&session_id);
    let log_bid = BranchId::new("main");
    if let Some(w) = &dual {
        match w.event_log().count(&log_sid, &log_bid, Some("turn/start")).await {
            Ok(n) => {
                turn = n as u32 + 1;
            }
            Err(err) => tracing::warn!(event = "bm.dual_write_turn_failed", error = %err),
        }
        // —— A2：回合请求头（一次模型调用链的标识 + 输入审计锚点）——
        w.append_best_effort(
            log_sid.clone(),
            EventKind::Core(CoreEvent::RequestHeader {
                header: EpochHeader {
                    provider: Some(meta.provider_id.clone()),
                    model: Some(meta.model.clone()),
                    created_at: now_ms(),
                    prompt_hash: Some(meta.prompt_hash.clone()),
                },
                reason: meta.reason,
            }),
            SurfaceIntent::None,
        )
        .await;
        w.append_best_effort(
            log_sid.clone(),
            EventKind::Core(CoreEvent::TurnStart { turn }),
            SurfaceIntent::None,
        )
        .await;
    }

    // 事件转发改为「回调同步入缓冲 + 独立 task 异步发送」：
    // 回调运行在 tokio 线程上不能 await；转发 task 从缓冲取事件发送，
    // 客户端断开时 send 失败即退出（watcher 负责 abort prompt）。
    let pending: Arc<std::sync::Mutex<std::collections::VecDeque<AgentStreamEvent>>> =
        Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
    let notify = Arc::new(tokio::sync::Notify::new());
    let tx_fwd = tx.clone();
    let pending_fwd = pending.clone();
    let notify_fwd = notify.clone();
    tokio::spawn(async move {
        loop {
            let ev = pending_fwd.lock().unwrap().pop_front();
            match ev {
                Some(ev) => {
                    if tx_fwd.send(ev).is_err() {
                        break; // 客户端断开（接收端已丢弃）
                    }
                }
                None => notify_fwd.notified().await,
            }
        }
    });

    let accumulated = Arc::new(std::sync::Mutex::new(String::new()));
    let acc = accumulated.clone();
    // 工具调用收集：执行中的按 id 挂起，ToolCallEnd 后移入完成列表入库
    let pending_tools: Arc<std::sync::Mutex<HashMap<String, (String, serde_json::Value)>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    let done_tools: Arc<std::sync::Mutex<Vec<(String, serde_json::Value, bool)>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let pending_tools_cb = pending_tools.clone();
    let done_tools_cb = done_tools.clone();
    let tx_cb = tx.clone();
    let pending_cb = pending.clone();
    let notify_cb = notify.clone();
    // 是否已向客户端发送过终态事件（done/error）——prompt 异常路径（如取消）可能
    // 不发 AgentEnd，结束后兜底补发 done，保证前端总能固化流式内容
    let done_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_sent_cb = done_sent.clone();
    // 本次 prompt 的 token 用量统计（取自 assistant 消息的 usage；工具结果消息为 0）
    let usage_total = Arc::new(std::sync::Mutex::new(0u64));
    let usage_total_cb = usage_total.clone();
    // —— A1 真序事件日志：回调同步入队 → 写线程攒批 append_batch ——
    // 回调运行在 tokio 线程不可 await；写线程每次唤醒全量排空（chunk 突发自然攒批，
    // 一次 append_batch 一个事务）。seq 由存储层分配，事件保持回调真实顺序。
    let log_queue: Arc<std::sync::Mutex<std::collections::VecDeque<LogItem>>> =
        Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
    let log_notify = Arc::new(tokio::sync::Notify::new());
    let log_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let log_enabled = dual.is_some();
    // step 状态（回调线程）：当前 step 号 + 步内流式文本（MessageEnd 时作为权威内容）
    let step_state: Arc<std::sync::Mutex<(u32, String)>> =
        Arc::new(std::sync::Mutex::new((0, String::new())));
    let step_state_cb = step_state.clone();
    // 任务心跳进度（最近活动摘要：工具名 / 回复尾部），由心跳 task 定时落库 + 推 SSE
    let progress: Arc<std::sync::Mutex<String>> = Arc::new(std::sync::Mutex::new(String::new()));
    let progress_cb = progress.clone();
    let started_at = std::time::Instant::now();

    // 心跳：每 5s 把内存进度刷库并推送 taskProgress SSE（前端据此显示任务状态条）
    let beat_stop = Arc::new(tokio::sync::Notify::new());
    let beat_stop_h = beat_stop.clone();
    let db_beat = state.db.clone();
    let task_beat = task_id.clone();
    let progress_beat = progress.clone();
    let tx_beat = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.tick().await; // 跳过首次立即触发
        loop {
            tokio::select! {
                _ = beat_stop_h.notified() => break,
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

    // 写线程：排空队列 → append_batch（seq 连续分配，失败不阻断主链路）。
    // prompt 结束后主线程置 log_done + notify → 排空剩余 → join（保证 TurnEnd 最后一条）。
    let (writer_q, writer_n, writer_d, writer_dual, writer_sid) = (
        log_queue.clone(),
        log_notify.clone(),
        log_done.clone(),
        dual.clone(),
        log_sid.clone(),
    );
    let writer_join = tokio::spawn(async move {
        let Some(w) = writer_dual.as_deref() else {
            return;
        };
        loop {
            let items: std::collections::VecDeque<LogItem> =
                std::mem::take(&mut *writer_q.lock().unwrap());
            if items.is_empty() {
                if writer_d.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                writer_n.notified().await;
                continue;
            }
            let events: Vec<(EventKind, SurfaceIntent, bool, Option<Vec<SeqNo>>)> =
                items.into_iter().map(log_item_to_event).collect();
            if let Err(err) = w.append_batch(writer_sid.clone(), events).await {
                tracing::warn!(event = "bm.dual_write_batch_failed", error = %err);
            }
        }
    });

    // 回调内真序入队（log_enabled=false 时直接跳过，不囤积）
    let (log_q_cb, log_n_cb) = (log_queue.clone(), log_notify.clone());
    let log_push = move |item: LogItem| {
        if log_enabled {
            log_q_cb.lock().unwrap().push_back(item);
            log_n_cb.notify_one();
        }
    };

    let result = handle
        .prompt_with_abort(message, abort_signal, move |ev: pi::sdk::AgentEvent| {
            // —— A1 真序：先提取事件日志条目（借用阶段），再映射 SSE 事件 ——
            match &ev {
                pi::sdk::AgentEvent::MessageStart {
                    message: pi::model::Message::Assistant(_),
                } => {
                    let mut st = step_state_cb.lock().unwrap();
                    st.0 += 1; // 新 step：每次流式响应开始
                    st.1.clear();
                    let step = st.0;
                    drop(st);
                    log_push(LogItem::StepStart { turn, step });
                }
                pi::sdk::AgentEvent::MessageUpdate {
                    assistant_message_event:
                        pi::model::AssistantMessageEvent::TextDelta { delta, .. },
                    ..
                } => {
                    let step = {
                        let mut st = step_state_cb.lock().unwrap();
                        st.1.push_str(delta);
                        st.0
                    };
                    log_push(LogItem::Chunk { turn, step, text: delta.clone() });
                }
                pi::sdk::AgentEvent::MessageEnd {
                    message: pi::model::Message::Assistant(a),
                } => {
                    let (step, content) = {
                        let mut st = step_state_cb.lock().unwrap();
                        (st.0, std::mem::take(&mut st.1))
                    };
                    // 步内无流式文本（非流式/异常路径）：退回从消息内容提取
                    let content = if content.is_empty() { assistant_text(a) } else { content };
                    log_push(LogItem::AssistantMessage {
                        turn,
                        step,
                        content,
                        usage: Some(TokenUsage {
                            input_tokens: a.usage.input,
                            output_tokens: a.usage.output,
                        }),
                    });
                }
                pi::sdk::AgentEvent::ToolExecutionStart { tool_call_id, tool_name, args } => {
                    let step = step_state_cb.lock().unwrap().0;
                    log_push(LogItem::ToolCall {
                        turn,
                        step,
                        call_id: tool_call_id.clone(),
                        name: tool_name.clone(),
                        args: serde_json::to_string(args).unwrap_or_default(),
                    });
                }
                pi::sdk::AgentEvent::ToolExecutionEnd { tool_call_id, result, is_error, .. } => {
                    let step = step_state_cb.lock().unwrap().0;
                    log_push(LogItem::ToolResult {
                        turn,
                        step,
                        call_id: tool_call_id.clone(),
                        ok: !*is_error,
                        output: tool_output_text(result),
                        meta: result.details.clone(),
                    });
                }
                pi::sdk::AgentEvent::AutoCompactionStart { .. } => {
                    log_push(LogItem::Compaction { turn, start: true });
                }
                pi::sdk::AgentEvent::AutoCompactionEnd { .. } => {
                    log_push(LogItem::Compaction { turn, start: false });
                }
                _ => {}
            }
            // 统计 token 用量（日志观测用）
                if let pi::sdk::AgentEvent::MessageEnd { message: m } = &ev
                && let pi::model::Message::Assistant(a) = m {
                    let u = &a.usage;
                    if u.total_tokens > 0 {
                        let mut total = usage_total_cb.lock().unwrap();
                        *total = total.saturating_add(u.total_tokens);
                        tracing::info!(
                            event = "bm.prompt_usage",
                            input = u.input,
                            output = u.output,
                            cache_read = u.cache_read,
                            cache_write = u.cache_write,
                            total = u.total_tokens,
                            session_total = *total,
                        );
                    }
                }
            let mapped = map_agent_event(ev);
            for mapped in mapped {
                match &mapped {
                    AgentStreamEvent::ToolCallStart { id, name, args } => {
                        pending_tools_cb
                            .lock()
                            .unwrap()
                            .insert(id.clone(), (name.clone(), args.clone()));
                        // 任务心跳：最近活动摘要（工具名 + 关键短参）
                        let summary = tool_summary(name, args);
                        *progress_cb.lock().unwrap() = summary;
                    }
                    AgentStreamEvent::ToolCallEnd { id, is_error, .. } => {
                        let mut pt = pending_tools_cb.lock().unwrap();
                        if let Some((name, args)) = pt.get(id) {
                            // 工具调用入库收集（DB messages/tool_calls 用；
                            // 事件日志已在 ToolExecutionStart/End 实时落盘）
                            done_tools_cb
                                .lock()
                                .unwrap()
                                .push((name.clone(), args.clone(), *is_error));
                            pt.remove(id);
                        }
                    }
                    AgentStreamEvent::TextDelta { delta } => {
                        acc.lock().unwrap().push_str(delta);
                        // 心跳：回复尾部（保留最近 80 字符）
                        let tail: String = delta
                            .trim_end()
                            .chars()
                            .rev()
                            .take(80)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        if !tail.is_empty() {
                            *progress_cb.lock().unwrap() = format!("回复中：…{tail}");
                        }
                    }
                    // 权限询问事件：不参与文本/工具聚合，直接透传前端弹窗
                    AgentStreamEvent::PermissionRequest { .. } => {}
                    // 心跳事件：前端消费（不经此 switch 再分发）
                    AgentStreamEvent::TaskProgress { .. } => {}
                    AgentStreamEvent::Done | AgentStreamEvent::Error { .. } => {
                        done_sent_cb.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                // 入缓冲由转发 task 异步发送（背压见上方说明）；客户端断开后
                // 通道已关闭，停止入队（转发 task 退出，watcher 会 abort prompt）
                if !tx_cb.is_closed() {
                    pending_cb.lock().unwrap().push_back(mapped);
                    notify_cb.notify_one();
                }
            }
        })
        .await;

    tracing::info!(
        event = "bm.prompt_done",
        total_tokens = *usage_total.lock().unwrap(),
        elapsed_ms = started_at.elapsed().as_millis(),
        result_ok = result.is_ok(),
    );

    // 无论成功/取消/失败，先把已生成的文本与工具调用入库
    let final_text = accumulated.lock().unwrap().clone();
    let done_tools = done_tools.lock().unwrap().clone();
    if !final_text.trim().is_empty() {
        if let Ok(assistant_msg) = state.db.add_message(&session_id, "assistant", &final_text).await
            && !done_tools.is_empty() {
                let _ = state.db.add_tool_calls(assistant_msg.id, &done_tools).await;
            }
        let _ = state.db.touch_session(&session_id).await;
    }

    // —— A1 收尾：写线程排空 + join（保证 TurnEnd 是日志最后一条）——
    log_done.store(true, std::sync::atomic::Ordering::Relaxed);
    log_notify.notify_one();
    let _ = writer_join.await;

    // 回合结束尾事件（reason 由运行结果定：completed/cancelled/failed）
    if let Some(w) = &dual {
        let reason = match &result {
            Ok(_) => TurnEndReason::Completed,
            Err(pi::sdk::Error::Aborted) => TurnEndReason::Cancelled,
            Err(_) => TurnEndReason::Failed,
        };
        w.append_best_effort(
            log_sid,
            EventKind::Core(CoreEvent::TurnEnd { turn, reason }),
            SurfaceIntent::None,
        )
        .await;
    }

    // refine-suggest 截获：代理调用 submit_refinement_suggestions 提交的改进建议
    // 在此入库（status=pending，用户审批后才生效）。工具调用参数已在 done_tools 中。
    for (name, args, is_error) in &done_tools {
        if name != "submit_refinement_suggestions" || *is_error {
            continue;
        }
        let (Some(target), Some(quote), Some(suggested), Some(reason)) = (
            args.get("target").and_then(serde_json::Value::as_str),
            args.get("quote").and_then(serde_json::Value::as_str),
            args.get("suggested").and_then(serde_json::Value::as_str),
            args.get("reason").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let suggestion_id = uuid::Uuid::new_v4().to_string();
        match state
            .db
            .insert_refinement_suggestion(&suggestion_id, Some(&session_id), target, quote, suggested, reason)
            .await
        {
            Ok(()) => tracing::info!(
                event = "bm.refine_suggestion_recorded",
                suggestion = %suggestion_id,
                target = %target,
                session = %session_id,
            ),
            Err(err) => tracing::warn!(
                event = "bm.refine_suggestion_record_failed",
                error = %err,
                session = %session_id,
            ),
        }
    }

    // 兜底补发终态事件（pi 取消路径不发 AgentEnd；失败且未发 error 时补 error）
    if !done_sent.load(std::sync::atomic::Ordering::Relaxed) {
        let terminal = if result.is_err() {
            AgentStreamEvent::Error { message: "agent 执行失败".to_string() }
        } else {
            AgentStreamEvent::Done
        };
        let _ = tx.send(terminal);
    }

    // 任务终态落库（completed / cancelled / failed）；停掉心跳 task
    let (task_status, task_error) = match &result {
        Ok(_) => ("completed", None),
        Err(pi::sdk::Error::Aborted) => ("cancelled", Some("已取消".to_string())),
        Err(e) => ("failed", Some(e.to_string())),
    };
    if let Err(err) = state
        .db
        .finish_task(&task_id, task_status, task_error.as_deref())
        .await
    {
        tracing::warn!(event = "bm.task_finish_failed", error = %err, task = %task_id);
    }
    beat_stop.notify_one();
}

/// 工具调用的心跳摘要（工具名 + 关键短参，避免长参数刷屏）。
fn tool_summary(name: &str, args: &serde_json::Value) -> String {
    const KEY_FIELDS: &[&str] = &["task", "query", "url", "path", "command", "message"];
    let mut picked = None;
    for key in KEY_FIELDS {
        if let Some(v) = args.get(key).and_then(serde_json::Value::as_str)
            && !v.is_empty()
        {
            let v: String = v.chars().take(60).collect();
            picked = Some(v);
            break;
        }
    }
    match picked {
        Some(v) => format!("正在执行工具 {name}：{v}"),
        None => format!("正在执行工具 {name}"),
    }
}

/// request/header 事件内容（A2）：一次 prompt 的模型调用链标识。
struct PromptMeta {
    provider_id: String,
    model: String,
    prompt_hash: String,
    reason: HeaderReason,
}

/// epoch ms（request/header 的 created_at）。
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// prompt_hash：模型可见输入的审计锚点（sha256 hex，长度前缀分段防歧义拼接）。
///
/// 阶段 1（pi 引擎）覆盖 BoenMind 注入面：自定义系统提示词 + skills 注入 +
/// 扩展路径（= QuickJS 已注册工具的确定性代理）。pi 内部系统提示词与工具
/// schema 属于上游，A6 自研 loop 后 hash 覆盖完整模型可见输入。
fn prompt_hash_of(custom_prompt: &str, skills_prompt: &str, ext_paths: &[std::path::PathBuf]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    let ext = ext_paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    for part in [custom_prompt, skills_prompt, ext.as_str()] {
        h.update((part.len() as u64).to_le_bytes());
        h.update(part.as_bytes());
        h.update([0]);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 真序事件日志条目（回调线程提取 → 写线程落盘）。
/// 与 pi AgentEvent 一一映射，但携带已解析的 (turn, step) 与权威内容——
/// 回调内所有上下文（回合号/步骤号/步内文本）同步解析完毕，写线程无状态转换。
enum LogItem {
    /// 步骤开始（pi 每次流式响应开始 = 一个 step）
    StepStart { turn: u32, step: u32 },
    /// 助手流式块（TextDelta；思考内容已由 pi 并入正文）
    Chunk { turn: u32, step: u32, text: String },
    /// 工具调用（ToolExecutionStart）
    ToolCall { turn: u32, step: u32, call_id: String, name: String, args: String },
    /// 工具结果（ToolExecutionEnd.result → 输出文本 + details 入 meta）
    ToolResult { turn: u32, step: u32, call_id: String, ok: bool, output: String, meta: Option<serde_json::Value> },
    /// 助手完整消息（MessageEnd：权威内容 + 本步 token 用量）
    AssistantMessage { turn: u32, step: u32, content: String, usage: Option<TokenUsage> },
    /// pi 自动压缩起止（可审计；摘要区间语义留待 A6 自研压缩引擎）
    Compaction { turn: u32, start: bool },
}

/// LogItem → 事件日志 tuple (kind, surface, ignorable, source_seqs)。
///
/// source_seqs 暂不落：chunk seq 由存储层在 append 时分配，批内无法预引用；
/// chunk→message 归并由投影层按 (turn, step) 完成（见 bm-kernel projection.rs）。
fn log_item_to_event(item: LogItem) -> (EventKind, SurfaceIntent, bool, Option<Vec<SeqNo>>) {
    let (kind, surface) = match item {
        LogItem::StepStart { turn, step } => {
            (EventKind::Core(CoreEvent::StepStart { turn, step }), SurfaceIntent::None)
        }
        LogItem::Chunk { turn, step, text } => (
            EventKind::Core(CoreEvent::AssistantChunk {
                turn,
                step,
                chunk: StreamChunk { text },
            }),
            SurfaceIntent::Append,
        ),
        LogItem::ToolCall { turn, step, call_id, name, args } => (
            EventKind::Core(CoreEvent::ToolCall {
                turn,
                step,
                call_id: CallId::new(call_id),
                name,
                args,
            }),
            SurfaceIntent::None,
        ),
        LogItem::ToolResult { turn, step, call_id, ok, output, meta } => (
            EventKind::Core(CoreEvent::ToolResult {
                turn,
                step,
                call_id: CallId::new(call_id),
                result: ToolResultMsg { ok, output },
                meta,
            }),
            SurfaceIntent::None,
        ),
        LogItem::AssistantMessage { turn, step, content, usage } => (
            EventKind::Core(CoreEvent::AssistantMessage {
                turn,
                step,
                msg: AssistantMsg { content },
                usage,
            }),
            SurfaceIntent::Append,
        ),
        LogItem::Compaction { turn, start: true } => {
            (EventKind::Core(CoreEvent::CompactionStart { turn }), SurfaceIntent::None)
        }
        LogItem::Compaction { turn, start: false } => {
            (EventKind::Core(CoreEvent::CompactionEnd { turn }), SurfaceIntent::None)
        }
    };
    (kind, surface, false, None)
}

/// ToolOutput → 字符串：Text 块拼接优先；无文本时退回整包 JSON（保真审计，
/// 如纯图片输出）。注意不截断——事件日志是审计之家，静默截断违反"模型可见即已记录"。
fn tool_output_text(output: &pi::sdk::ToolOutput) -> String {
    let text: Vec<&str> = output
        .content
        .iter()
        .filter_map(|b| match b {
            pi::model::ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    if !text.is_empty() {
        return text.join("\n");
    }
    serde_json::to_string(output).unwrap_or_default()
}

/// AssistantMessage → 文本（Text 块拼接）。
/// 正常路径不用它（用步内流式文本，含 pi 已并入正文的思考）；仅非流式/异常兜底。
fn assistant_text(msg: &pi::model::AssistantMessage) -> String {
    let parts: Vec<&str> = msg
        .content
        .iter()
        .filter_map(|b| match b {
            pi::model::ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    parts.join("\n")
}

/// AgentStreamEvent → SSE 事件。
pub(crate) fn to_sse_event(event: &AgentStreamEvent) -> Event {
    let json = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    Event::default().data(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_block(s: &str) -> pi::model::ContentBlock {
        pi::model::ContentBlock::Text(pi::model::TextContent::new(s))
    }

    #[test]
    fn tool_output_text_joins_text_blocks() {
        let out = pi::sdk::ToolOutput {
            content: vec![text_block("第一段"), text_block("第二段")],
            details: None,
            is_error: false,
        };
        assert_eq!(tool_output_text(&out), "第一段\n第二段");
    }

    #[test]
    fn tool_output_text_falls_back_to_json_when_no_text() {
        // 无 Text 块（如纯图片输出）→ 整包 JSON 保真
        let out = pi::sdk::ToolOutput {
            content: vec![pi::model::ContentBlock::Image(pi::model::ImageContent {
                data: "AAAA".into(),
                mime_type: "image/png".into(),
            })],
            details: None,
            is_error: false,
        };
        let text = tool_output_text(&out);
        assert!(text.contains("image/png"), "JSON 兜底应保留原始输出: {text}");
    }

    #[test]
    fn assistant_text_extracts_text_blocks_only() {
        let mut msg = pi::model::AssistantMessage::default();
        msg.content = vec![text_block("正文"), text_block("续")];
        assert_eq!(assistant_text(&msg), "正文\n续");
    }

    #[test]
    fn log_item_event_type_names() {
        let cases = [
            (
                LogItem::StepStart { turn: 1, step: 1 },
                "step/start",
            ),
            (
                LogItem::Chunk { turn: 1, step: 1, text: "x".into() },
                "assistant/chunk",
            ),
            (
                LogItem::ToolCall {
                    turn: 1,
                    step: 1,
                    call_id: "c1".into(),
                    name: "exec".into(),
                    args: "{}".into(),
                },
                "tool/call",
            ),
            (
                LogItem::ToolResult {
                    turn: 1,
                    step: 1,
                    call_id: "c1".into(),
                    ok: true,
                    output: "o".into(),
                    meta: None,
                },
                "tool/result",
            ),
            (
                LogItem::AssistantMessage {
                    turn: 1,
                    step: 1,
                    content: "a".into(),
                    usage: None,
                },
                "assistant/message",
            ),
            (LogItem::Compaction { turn: 1, start: true }, "compaction/start"),
            (LogItem::Compaction { turn: 1, start: false }, "compaction/end"),
        ];
        for (item, want) in cases {
            let (kind, _, _, _) = log_item_to_event(item);
            assert_eq!(kind.name(), want);
        }
    }

    #[test]
    fn log_item_surface_intents() {
        // 消息面事件（chunk/assistant message）带 Append；其余 None
        let append = [
            LogItem::Chunk { turn: 1, step: 1, text: "x".into() },
            LogItem::AssistantMessage { turn: 1, step: 1, content: "a".into(), usage: None },
        ];
        for item in append {
            let (_, surface, _, _) = log_item_to_event(item);
            assert_eq!(surface, SurfaceIntent::Append);
        }
        let none = [
            LogItem::StepStart { turn: 1, step: 1 },
            LogItem::ToolCall { turn: 1, step: 1, call_id: "c".into(), name: "n".into(), args: "{}".into() },
            LogItem::ToolResult { turn: 1, step: 1, call_id: "c".into(), ok: true, output: "o".into(), meta: None },
            LogItem::Compaction { turn: 1, start: true },
        ];
        for item in none {
            let (_, surface, _, _) = log_item_to_event(item);
            assert_eq!(surface, SurfaceIntent::None);
        }
    }

    #[test]
    fn tool_result_keeps_meta_details() {
        let (kind, _, _, _) = log_item_to_event(LogItem::ToolResult {
            turn: 2,
            step: 3,
            call_id: "c9".into(),
            ok: false,
            output: "boom".into(),
            meta: Some(serde_json::json!({"code": 1})),
        });
        match kind {
            EventKind::Core(CoreEvent::ToolResult { turn, step, result, meta, .. }) => {
                assert_eq!((turn, step), (2, 3));
                assert!(!result.ok);
                assert_eq!(result.output, "boom");
                assert_eq!(meta, Some(serde_json::json!({"code": 1})));
            }
            other => panic!("应为 tool/result，得到 {other:?}"),
        }
    }

    #[test]
    fn prompt_hash_deterministic_and_sensitive() {
        let p = |x: &str| std::path::PathBuf::from(x);
        let a = prompt_hash_of("custom", "skills", &[p("/ext/a.ts")]);
        let b = prompt_hash_of("custom", "skills", &[p("/ext/a.ts")]);
        assert_eq!(a, b, "同输入同 hash");
        assert_eq!(a.len(), 64, "sha256 hex");

        // 任一注入面变化 → hash 变化
        assert_ne!(a, prompt_hash_of("custom2", "skills", &[p("/ext/a.ts")]));
        assert_ne!(a, prompt_hash_of("custom", "skills2", &[p("/ext/a.ts")]));
        assert_ne!(a, prompt_hash_of("custom", "skills", &[p("/ext/b.ts")]));
        assert_ne!(a, prompt_hash_of("custom", "skills", &[p("/ext/a.ts"), p("/ext/b.ts")]));

        // 长度前缀分段：拼接歧义不碰撞
        assert_ne!(
            prompt_hash_of("ab", "c", &[]),
            prompt_hash_of("a", "bc", &[]),
            "ab+c 与 a+bc 不得同 hash（长度前缀）"
        );
    }
}
