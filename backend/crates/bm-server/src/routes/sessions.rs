//! 会话 CRUD：创建（含首次命名）、列表、详情（含消息）、重命名、删除。

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use uuid::Uuid;

use crate::{ApiResult, api_error};

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// 场景（架构 §四·B 补充）：chat/coding/…默认 chat；引擎按场景组装工具面
    #[serde(default)]
    pub app: Option<String>,
}

/// 会话面取用（SERVICE_FACES #9）：kernel 可用时经服务；None = 退化直调。
fn session_port(state: &crate::AppState) -> Option<Arc<dyn bm_protocol::SessionPort>> {
    state
        .kernel
        .as_ref()
        .and_then(|k| k.port::<dyn bm_protocol::SessionPort>("session").ok())
}

pub async fn create_session(
    State(state): crate::SharedState,
    Json(req): Json<CreateSessionRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let id = Uuid::new_v4().to_string();
    let session = if let Some(port) = session_port(&state) {
        port.create(&id, req.provider_id.as_deref(), req.model.as_deref(), req.app.as_deref().unwrap_or("chat"))
            .await
            .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
    } else {
        let session = state
            .db
            .create_session(&id, req.provider_id.as_deref(), req.model.as_deref(), req.app.as_deref().unwrap_or("chat"))
            .await
            .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        serde_json::to_value(&session)
            .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
    };
    let mut session = session;
    if let Some(title) = req.title
        && !title.trim().is_empty()
    {
        if let Some(port) = session_port(&state) {
            port.rename(&id, title.trim())
                .await
                .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        } else {
            state
                .db
                .rename_session(&id, title.trim())
                .await
                .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        }
        session["title"] = serde_json::json!(title.trim());
    }
    Ok(Json(session))
}

pub async fn list_sessions(State(state): crate::SharedState) -> ApiResult<Json<serde_json::Value>> {
    // 会话面（SERVICE_FACES #9）：kernel 可用时经服务；退化直调
    if let Some(port) = session_port(&state) {
        let list = port
            .list()
            .await
            .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        return Ok(Json(list));
    }
    state
        .db
        .list_sessions()
        .await
        .map(|list| Json(serde_json::to_value(&list).unwrap_or(serde_json::Value::Null)))
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

pub async fn get_session(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    if let Some(port) = session_port(&state) {
        let session = port
            .get(&id)
            .await
            .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("会话不存在: {id}")))?;
        let messages = port
            .messages(&id)
            .await
            .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        return Ok(Json(serde_json::json!({
            "session": session,
            "messages": messages,
        })));
    }
    let session = state
        .db
        .get_session(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("会话不存在: {id}")))?;
    let messages = state
        .db
        .list_messages(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(serde_json::json!({
        "session": session,
        "messages": messages,
    })))
}

/// 会话的任务记录（断线续跑 + 心跳进度；时间倒序，最新一条在前）。
/// 前端打开会话时据此恢复任务状态条展示。
pub async fn list_session_tasks(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<Vec<bm_core::db::Task>>> {
    let tasks = state
        .db
        .list_tasks(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(tasks))
}

#[derive(Deserialize)]
pub struct RenameSessionRequest {
    pub title: String,
}

pub async fn rename_session(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<RenameSessionRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let title = req.title.trim().to_string();
    if title.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "标题不能为空"));
    }
    state
        .db
        .rename_session(&id, &title)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_session(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let rows = state
        .db
        .delete_session(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if rows == 0 {
        return Err(api_error(StatusCode::NOT_FOUND, format!("会话不存在: {id}")));
    }
    // 清理对应的 bm 引擎 agent 会话句柄（状态全在事件日志，弃置零损失）
    state.loop_agents.lock().await.remove(&id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// POST /api/sessions/{id}/fork — 会话级分叉（2026-08-16 用户定调）：
/// 新会话 + 复制源会话历史（到 at_message 含）。前端"答复末尾分叉"按钮用。
#[derive(Deserialize)]
pub struct ForkSessionRequest {
    /// 从哪条消息之后分叉（含该消息的历史复制到新会话）
    pub at_message: i64,
}

pub async fn fork_session(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<ForkSessionRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let new_id = Uuid::new_v4().to_string();
    let session = state
        .db
        .fork_session(&id, &new_id, req.at_message)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    serde_json::to_value(&session)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
        .map(Json)
}

// ---------------------------------------------------------------------------
// A5 事件流（SSE）：前端投影引擎前置
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct EventsQuery {
    /// 只推 seq > after 的事件（增量续订）；缺省从头（replay-prefix 全量）
    #[serde(default)]
    pub after: Option<u64>,
}

/// GET /api/sessions/{id}/events?after=N —— SSE 事件流。
///
/// 先推 `after` 之后的既有事件（replay-prefix），之后实时推新事件（tail，
/// 250ms 轮询）。客户端断开（receiver drop）→ 发送失败/watchdog 置位 → 订阅退出。
/// 事件 data = SessionEvent 信封 JSON（kind 已 flatten：{seq,type,session_id,…}）。
pub async fn events_stream(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<EventsQuery>,
) -> axum::response::Response {
    use axum::response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio_stream::{StreamExt, wrappers::UnboundedReceiverStream};

    match state.db.get_session(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return api_error(StatusCode::NOT_FOUND, format!("会话不存在: {id}")).into_response(),
        Err(err) => {
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    }
    let Some(dual) = &state.dual_writer else {
        return api_error(StatusCode::SERVICE_UNAVAILABLE, "事件日志不可用").into_response();
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<bm_protocol::SessionEvent>();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    // watchdog：receiver drop（客户端断开/流结束）1s 内置位停止开关，
    // 兜底"断开后无新事件"场景（有新事件时发送失败会立即置位）
    {
        let stop = stop.clone();
        let tx_watch = tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if tx_watch.is_closed() {
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
            }
        });
    }
    // 订阅任务：持订阅直至停止（subscription drop 即 stop，幂等）
    {
        let stop = stop.clone();
        let store = dual.event_store();
        let sid = bm_protocol::SessionId::new(&id);
        let bid = bm_protocol::BranchId::new("main");
        let after = query.after;
        let tx_send = tx.clone();
        let stop_send = stop.clone();
        let stop_cb = stop_send.clone();
        let stop_arg = stop_cb.clone();
        tokio::spawn(async move {
            let sub = bm_kernel::subscribe_events(
                store,
                sid,
                bid,
                after,
                move |ev| {
                    if stop_cb.load(Ordering::Relaxed) {
                        return;
                    }
                    if tx_send.send(ev).is_err() {
                        stop_cb.store(true, Ordering::Relaxed);
                    }
                },
                stop_arg,
            )
            .await;
            match sub {
                Ok(_sub) => {
                    // 持有订阅直到停止开关置位（watchdog / 发送失败）
                    while !stop_send.load(Ordering::Relaxed) {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                }
                Err(err) => {
                    tracing::warn!(event = "bm.events_subscribe_failed", error = %err, session = %id);
                }
            }
        });
    }

    let stream = UnboundedReceiverStream::new(rx).map(|ev| {
        let json = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".to_string());
        Ok::<_, std::convert::Infallible>(Event::default().data(json))
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default().interval(std::time::Duration::from_secs(15)))
        .into_response()
}

/// DELETE /api/sessions/{id}/events —— 清空该会话事件日志（回收站 C2 用户主动清除）。
/// messages 表不动（会话仍可继续聊，事件日志从 seq 1 重新记录）。
pub async fn clear_session_events(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    match state.db.get_session(&id).await {
        Ok(Some(_)) => {}
        Ok(None) => return Err(api_error(StatusCode::NOT_FOUND, format!("会话不存在: {id}"))),
        Err(err) => {
            return Err(api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()));
        }
    }
    let Some(dual) = &state.dual_writer else {
        return Err(api_error(StatusCode::SERVICE_UNAVAILABLE, "事件日志不可用"));
    };
    let cleared = dual
        .clear_session(bm_protocol::SessionId::new(&id))
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true, "cleared": cleared })))
}

/// 会话 token 用量统计（状态栏数据源，2026-08-15）：聚合事件日志
/// assistant/message 事件的 usage（input/output tokens）。
/// 事件日志不可用（kernel 未装配）→ 全零（前端显示为空态，不报错）。
/// 服务面铺开（SERVICE_FACES #12）：聚合逻辑归 stats 服务，路由层取服务。
pub async fn get_session_usage(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    use bm_protocol::SessionId;
    let Some(kernel) = &state.kernel else {
        return Ok(Json(serde_json::json!({
            "input_tokens": 0, "output_tokens": 0, "messages": 0,
        })));
    };
    let stats = kernel
        .port::<dyn bm_protocol::StatsPort>("stats")
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let usage = stats
        .session_usage(&SessionId::new(&id))
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(serde_json::json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "messages": usage.messages,
    })))
}
