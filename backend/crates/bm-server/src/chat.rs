//! 流式对话：POST /api/chat → SSE。
//!
//! 协议（SSE 事件，data 为 JSON）：
//! - `{"type":"textDelta","delta":"..."}`      正文增量
//! - `{"type":"thinkingDelta","delta":"..."}`  思考过程增量
//! - `{"type":"toolCallStart","name":"..."}`   工具调用开始
//! - `{"type":"toolCallDelta","delta":"..."}`  工具参数增量
//! - `{"type":"turnEnd"}`                      回合结束
//! - `{"type":"done"}`                         整个 prompt 结束
//! - `{"type":"error","message":"..."}`        出错
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

use crate::{AppState, api_error};

/// 各语言下的默认新会话标题（前端创建会话时按界面语言传入，
/// 首条消息前识别为"未命名"，自动用消息开头命名）。
const DEFAULT_TITLES: [&str; 4] = ["新对话", "New chat", "新しいチャット", "새 채팅"];

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

    let session = match state.db.get_session(&req.session_id) {
        Ok(Some(s)) => s,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "会话不存在").into_response(),
        Err(err) => {
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    };

    // 首次消息时自动用消息开头命名会话（各语言默认标题均视为未命名）
    if DEFAULT_TITLES.contains(&session.title.as_str()) {
        let title: String = message.chars().take(24).collect();
        let _ = state.db.rename_session(&session.id, &title);
    }

    // 持久化用户消息
    if let Err(err) = state.db.add_message(&session.id, "user", &message) {
        return api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }

    // 获取或创建 agent 会话句柄（Arc<Mutex<..>> 保证同一会话串行且 map 锁不长期占用）
    let handle = match get_or_create_agent(&state, &session, req.model.as_deref(), req.thinking.as_deref()).await {
        Ok(h) => h,
        Err((status, msg)) => return api_error(status, msg).into_response(),
    };

    // 事件通道 → SSE
    let (tx, rx) = mpsc::channel::<AgentStreamEvent>(512);
    let stream = ReceiverStream::new(rx).map(|ev| Ok::<_, std::convert::Infallible>(to_sse_event(&ev)));

    let state_clone = state.clone();
    let session_id = session.id.clone();
    tokio::spawn(async move {
        run_prompt_and_persist(state_clone, session_id, handle, message, tx).await;
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::default().interval(std::time::Duration::from_secs(15)))
        .into_response()
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

    // 会话句柄已存在：即时切换模型 / 思考强度
    if let Some(entry) = state.agents.lock().await.get(&session.id) {
        let mut handle = entry.handle.lock().await;
        if let Some(pid) = model_override {
            let provider_name = provider.kind.pi_name(&provider.id);
            handle
                .set_model(&provider_name, pid)
                .await
                .map_err(|err| (StatusCode::BAD_GATEWAY, format!("切换模型失败: {err}")))?;
        }
        if let Some(level) = thinking_override {
            if let Ok(level) = level.parse::<pi::model::ThinkingLevel>() {
                handle
                    .set_thinking_level(level)
                    .await
                    .map_err(|err| (StatusCode::BAD_GATEWAY, format!("切换思考强度失败: {err}")))?;
            }
        }
        return Ok(entry.handle.clone());
    }

    let working_dir = {
        let config = state.config.read().await;
        config.working_dir.clone()
    };

    let extension_paths = {
        let config = state.config.read().await;
        bm_core::plugins::enabled_extension_paths(&config)
    };

    // 启用的 skill 注入文本（pi CLI 同款 available_skills 格式）
    let skills_prompt = {
        let config = state.config.read().await;
        bm_core::skills::enabled_skills_prompt(&config)
    };

    // 按模型解析压缩设置（水线/尾部保护/窗口；config.compaction.enabled=false 时为 None）
    let compaction = {
        let config = state.config.read().await;
        bm_core::compaction::resolve_for_model_with_default_path(
            &config.compaction,
            &provider.kind.pi_name(&provider.id),
            &model,
        )
    };

    let handle = create_session_handle(
        &provider,
        &model,
        &working_dir,
        extension_paths,
        &skills_prompt,
        thinking_override,
        compaction,
    )
    .await
    .map_err(|err| (StatusCode::BAD_GATEWAY, format!("创建 agent 会话失败: {err}")))?;

    let mut agents = state.agents.lock().await;
    let entry = agents
        .entry(session.id.clone())
        .or_insert_with(|| crate::AgentSessionEntry {
            handle: Arc::new(Mutex::new(handle)),
        });
    Ok(entry.handle.clone())
}

/// 运行 prompt，将事件转发到通道；结束后把助手消息（含工具调用）写入数据库。
async fn run_prompt_and_persist(
    state: AppState,
    session_id: String,
    handle: Arc<Mutex<pi::sdk::AgentSessionHandle>>,
    message: String,
    tx: mpsc::Sender<AgentStreamEvent>,
) {
    // 同一会话的并发 prompt 串行（map 锁已在 get_or_create_agent 后释放）
    let mut handle = handle.lock().await;

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
    // AgentEnd 携带 error 时已通过事件流发出错误，避免与 result Err 重复上报
    let error_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let error_sent_cb = error_sent.clone();
    // 本次 prompt 的 token 用量统计（取自 assistant 消息的 usage；工具结果消息为 0）
    let usage_total = Arc::new(std::sync::Mutex::new(0u64));
    let usage_total_cb = usage_total.clone();
    let started_at = std::time::Instant::now();

    let result = handle
        .prompt(message, move |ev: pi::sdk::AgentEvent| {
            // 统计 token 用量（日志观测用）
            if let pi::sdk::AgentEvent::MessageEnd { message: m } = &ev {
                if let pi::model::Message::Assistant(a) = m {
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
            }
            let mapped = map_agent_event(ev);
            for mapped in mapped {
                match &mapped {
                    AgentStreamEvent::ToolCallStart { id, name, args } => {
                        pending_tools_cb
                            .lock()
                            .unwrap()
                            .insert(id.clone(), (name.clone(), args.clone()));
                    }
                    AgentStreamEvent::ToolCallEnd { id, is_error, .. } => {
                        if let Some((name, args)) = pending_tools_cb.lock().unwrap().remove(id) {
                            done_tools_cb.lock().unwrap().push((name, args, *is_error));
                        }
                    }
                    AgentStreamEvent::TextDelta { delta } => {
                        acc.lock().unwrap().push_str(delta);
                    }
                    AgentStreamEvent::Error { .. } => {
                        error_sent_cb.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    _ => {}
                }
                // 注意：此处运行在 tokio 运行时线程上，不能用 blocking_send；
                // try_send 失败（通道满或客户端断开）时直接跳过该事件
                if tx_cb.try_send(mapped).is_err() {
                    return; // 客户端已断开，停止转发
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

    // 无论成功与否，先把已生成的文本与工具调用入库
    let final_text = accumulated.lock().unwrap().clone();
    let done_tools = done_tools.lock().unwrap().clone();
    if !final_text.trim().is_empty() {
        if let Ok(assistant_msg) = state.db.add_message(&session_id, "assistant", &final_text) {
            if !done_tools.is_empty() {
                let _ = state.db.add_tool_calls(assistant_msg.id, &done_tools);
            }
        }
        let _ = state.db.touch_session(&session_id);
    }
    if result.is_err() && !error_sent.load(std::sync::atomic::Ordering::Relaxed) {
        let _ = tx
            .send(AgentStreamEvent::Error {
                message: "agent 执行失败".to_string(),
            })
            .await;
    }
}

/// AgentStreamEvent → SSE 事件。
fn to_sse_event(event: &AgentStreamEvent) -> Event {
    let json = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    Event::default().data(json)
}
