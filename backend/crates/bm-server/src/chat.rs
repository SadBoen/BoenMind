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
use serde::Deserialize;
use tokio::sync::{Mutex, mpsc};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};

use crate::{AppState, PermissionDecision, api_error};

/// 权限询问事件推送：经会话当前活跃 prompt 的 SSE 通道发给前端。
/// prompt 结束后通道已移除 → 事件丢失（询问仍会超时 fail-closed，无泄漏）。
pub async fn send_permission_request(
    state: &AppState,
    session_id: &str,
    request_id: &str,
    extension_id: &str,
    capability: &str,
    message: &str,
) {
    let tx = state.session_streams.lock().await.get(session_id).cloned();
    if let Some(tx) = tx {
        let _ = tx
            .send(AgentStreamEvent::PermissionRequest {
                id: request_id.to_string(),
                extension_id: Some(extension_id.to_string()),
                capability: capability.to_string(),
                message: message.to_string(),
            })
            .await;
    }
}

/// 各语言下的默认新会话标题（前端创建会话时按界面语言传入，
/// 首条消息前识别为"未命名"，自动用消息开头命名）。
const DEFAULT_TITLES: [&str; 4] = ["新对话", "New chat", "新しいチャット", "새 채팅"];

/// prompt 总超时：上游挂起（连接建立后不返回数据）时不能永久锁死会话。
/// 超时走 abort 通道，与用户点停止同一条收尾路径（部分文本照常入库）。
const PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// 全局 prompt 序号：aborts 表中区分同会话的先后 prompt（见 AppState.aborts）。
static PROMPT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 聊天请求：会话必须已存在（前端先创建会话再发消息）。
/// `model`/`thinking` 可选，用于在当前会话即时切换模型与思考强度。
#[derive(Deserialize)]
pub struct ChatRequest {
    pub session_id: String,
    pub message: String,
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

    // 获取或创建 agent 会话句柄（Arc<Mutex<..>> 保证同一会话串行且 map 锁不长期占用）
    let handle = match get_or_create_agent(&state, &session, req.model.as_deref(), req.thinking.as_deref()).await {
        Ok(h) => h,
        Err((status, msg)) => return api_error(status, msg).into_response(),
    };

    // 本次 prompt 的取消原语（pi 官方 abort 机制）：注册到会话级表，
    // POST /api/chat/stop 按 session_id 触发；客户端断开时由 watcher 自动触发。
    // 带 prompt_id 身份：同会话连续请求时先结束的只删自己的条目（见清理处）。
    let (abort_handle, abort_signal) = pi::sdk::AgentSessionHandle::new_abort_handle();
    let prompt_id = PROMPT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state.aborts.lock().await.insert(session.id.clone(), (prompt_id, abort_handle.clone()));

    // 事件通道 → SSE（512 容量；消费端即 HTTP 响应流）
    let (tx, rx) = mpsc::channel::<AgentStreamEvent>(512);
    // 注册会话的活跃事件通道：权限询问桥据此把询问事件推给前端
    state.session_streams.lock().await.insert(session.id.clone(), tx.clone());
    let stream = ReceiverStream::new(rx).map(|ev| Ok::<_, std::convert::Infallible>(to_sse_event(&ev)));

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
        run_prompt_and_persist(state_clone.clone(), session_id.clone(), task_id, handle, abort_signal, message, tx.clone()).await;
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
    if let Some((_, abort)) = state.aborts.lock().await.remove(&req.session_id) {
        abort.abort();
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
/// `model`/`thinking` 有值时对已存在的会话即时生效（set_model / set_thinking_level）。
async fn get_or_create_agent(
    state: &AppState,
    session: &bm_core::db::Session,
    model_override: Option<&str>,
    thinking_override: Option<&str>,
) -> Result<Arc<Mutex<pi::sdk::AgentSessionHandle>>, (StatusCode, String)> {
    // 解析提供商与模型
    let (provider, model) = {
        let config = state.config.read().await;
        let provider = bm_core::config::resolve_provider(&config, session.provider_id.as_deref())
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
        return Ok(arc);
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
    Ok(entry.handle.clone())
}

/// 运行 prompt，将事件转发到通道；结束后把助手消息（含工具调用）写入数据库。
/// `task_id` 为本次 prompt 回合的任务实体（心跳进度 + 终态落库，见 db::Task）。
async fn run_prompt_and_persist(
    state: AppState,
    session_id: String,
    task_id: String,
    handle: Arc<Mutex<pi::sdk::AgentSessionHandle>>,
    abort_signal: pi::sdk::AbortSignal,
    message: String,
    tx: mpsc::Sender<AgentStreamEvent>,
) {
    // 同一会话的并发 prompt 串行（map 锁已在 get_or_create_agent 后释放）
    let mut handle = handle.lock().await;

    // 事件转发改为「回调同步入缓冲 + 独立 task 异步发送」：
    // 回调运行在 tokio 线程上不能阻塞等待通道空间，而 try_send 在通道满
    // （客户端消费慢）时会丢事件导致流静默中断；转发 task 用 send().await
    // 天然有背压，客户端断开时 send 失败即退出（watcher 负责 abort prompt）。
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
                    if tx_fwd.send(ev).await.is_err() {
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
                let _ = tx_beat.send(AgentStreamEvent::TaskProgress { progress: p }).await;
            }
        }
    });

    let result = handle
        .prompt_with_abort(message, abort_signal, move |ev: pi::sdk::AgentEvent| {
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
                        if let Some((name, args)) = pending_tools_cb.lock().unwrap().remove(id) {
                            done_tools_cb.lock().unwrap().push((name, args, *is_error));
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
        let _ = tx.send(terminal).await;
    }

    // 任务终态落库（completed / cancelled / failed）；停掉心跳 task
    let (task_status, task_error) = match &result {
        Ok(_) => ("completed", None),
        Err(e) if matches!(e, pi::sdk::Error::Aborted) => {
            ("cancelled", Some("已取消".to_string()))
        }
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

/// AgentStreamEvent → SSE 事件。
fn to_sse_event(event: &AgentStreamEvent) -> Event {
    let json = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    Event::default().data(json)
}
