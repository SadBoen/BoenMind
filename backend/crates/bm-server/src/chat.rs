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
//! - 前端点「停止」→ POST /api/chat/stop → bm 引擎 watch 取消通道 → prompt 尽快返回，
//!   已生成的文本照常入库并下发 done；
//! - 客户端断开 SSE（关窗/切会话）→ 事件通道关闭 → StreamHooks 自动触发取消，
//!   避免继续烧 token（部分文本仍入库）。
//!
//! 用户消息与最终助手消息均持久化到 SQLite。
//!
//! 执行引擎 = 自研 bm-loop（pi 引擎已于 2026-08-15 废除，见 HANDOFF §十四 ③）。
//! 本模块只保留：会话校验/命名、路由壳、权限询问转发、SSE 事件形状；
//! 引擎逻辑全部在 bm_engine.rs（chat_bm）。

use std::collections::HashMap;

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response, sse::Event},
};
use bm_core::agent::AgentStreamEvent;
use serde::Deserialize;

use crate::{AppState, api_error};

/// 权限询问事件推送：经会话当前活跃 prompt 的 SSE 通道发给前端。
/// prompt 结束后通道已移除 → 事件丢失（询问仍会超时 fail-closed，无泄漏）。
/// 入参是 AppState 的 session_streams 组件（bm 引擎的 CompatEngine 建于
/// AppState 之前，只能拿组件）。
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
/// 超时走取消通道，与用户点停止同一条收尾路径（部分文本照常入库）。
pub(crate) const PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// 全局 prompt 序号：bm_aborts 表中区分同会话的先后 prompt（见 AppState.bm_aborts）。
static PROMPT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 取下一个 prompt 序号（取消身份匹配用）。
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

    // 唯一执行引擎 = 自研 bm-loop（pi 已废除）。事件日志由 loop 拥有全生命周期。
    crate::bm_engine::chat_bm(state, session, message, req.provider, req.model, req.thinking).await
}

/// 取消进行中的 prompt（幂等：无进行中 prompt 时返回 ok）。
/// 后端取消后 prompt 尽快返回，已生成的部分文本照常入库并下发 done。
#[derive(Deserialize)]
pub struct StopChatRequest {
    pub session_id: String,
}

pub async fn stop_chat(
    State(state): State<AppState>,
    Json(req): Json<StopChatRequest>,
) -> Response {
    let mut stopped = false;
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
    // 权限面（SERVICE_FACES #14）：kernel 可用时经服务；退化直调询问表
    if let Some(kernel) = &state.kernel
        && let Ok(port) = kernel.port::<dyn bm_protocol::GatePort>("gate")
    {
        match port.respond(&req.request_id, req.allow, req.always).await {
            Ok(()) => {
                tracing::info!(
                    event = "bm.permission_responded",
                    request = %req.request_id,
                    allow = req.allow,
                    always = req.always,
                );
            }
            Err(e) => {
                tracing::warn!(event = "bm.permission_respond_failed", request = %req.request_id, error = %e);
            }
        }
        return axum::Json(serde_json::json!({ "ok": true })).into_response();
    }
    if let Some(tx) = state.permission_pending.lock().await.remove(&req.request_id) {
        let _ = tx.send(crate::PermissionDecision {
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

/// ask_user 询问事件推送：经会话当前活跃 prompt 的 SSE 通道发给前端。
/// prompt 结束后通道已移除 → 事件丢失（询问仍会超时收尾，无泄漏）。
pub async fn send_ask_user(
    session_streams: &tokio::sync::Mutex<
        HashMap<String, tokio::sync::mpsc::UnboundedSender<AgentStreamEvent>>,
    >,
    session_id: &str,
    request_id: &str,
    question: &str,
) {
    let tx = session_streams.lock().await.get(session_id).cloned();
    if let Some(tx) = tx {
        let _ = tx.send(AgentStreamEvent::AskUser {
            id: request_id.to_string(),
            question: question.to_string(),
        });
    }
}

/// ask_user 回答回传（前端弹窗填写后 POST）。
/// 无挂起询问时幂等返回 ok（可能已超时）。
#[derive(Deserialize)]
pub struct AskResponseRequest {
    pub request_id: String,
    pub answer: String,
}

pub async fn respond_ask(
    State(state): State<AppState>,
    Json(req): Json<AskResponseRequest>,
) -> Response {
    if let Some(tx) = state.ask_pending.lock().await.remove(&req.request_id) {
        let _ = tx.send(req.answer);
        tracing::info!(event = "bm.ask_responded", request = %req.request_id);
    }
    axum::Json(serde_json::json!({ "ok": true })).into_response()
}

/// AgentStreamEvent → SSE data 行（前端事件形状；bm 引擎与权限询问共用）。
pub(crate) fn to_sse_event(event: &AgentStreamEvent) -> Event {
    let json = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    Event::default().data(json)
}
