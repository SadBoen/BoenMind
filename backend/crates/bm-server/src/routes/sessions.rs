//! 会话 CRUD：创建（含首次命名）、列表、详情（含消息）、重命名、删除。

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
}

pub async fn create_session(
    State(state): crate::SharedState,
    Json(req): Json<CreateSessionRequest>,
) -> ApiResult<Json<bm_core::db::Session>> {
    let id = Uuid::new_v4().to_string();
    let session = state
        .db
        .create_session(&id, req.provider_id.as_deref(), req.model.as_deref())
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let mut session = session;
    if let Some(title) = req.title
        && !title.trim().is_empty() {
            state
                .db
                .rename_session(&session.id, title.trim())
                .await
                .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
            session.title = title.trim().to_string();
        }
    Ok(Json(session))
}

pub async fn list_sessions(State(state): crate::SharedState) -> ApiResult<Json<Vec<bm_core::db::Session>>> {
    state
        .db
        .list_sessions()
        .await
        .map(Json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

pub async fn get_session(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
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
    // 清理对应的 agent 会话句柄
    state.agents.lock().await.remove(&id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// 插件
