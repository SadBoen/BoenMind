//! 活任务清单 REST 面（M2）：前端任务面板的手动操作入口。
//!
//! 与 `todo` 工具共用同一 apply 逻辑与同一事实源（事件日志 todo/write
//! 快照）——模型回合内改动与用户面板改动都落同一事件链，前端投影
//! 对两者一视同仁。

use axum::{Json, extract::{Path, State}};
use serde::Deserialize;

use crate::ApiResult;

/// 用户侧清单操作（形状与 todo 工具参数对齐；index 1 起）。
#[derive(Deserialize)]
pub struct TodoOpParams {
    pub action: String,
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
}

/// 当前清单（初始投影；无清单 → 空数组）。
pub async fn get_todos(
    State(state): crate::SharedState,
    Path(session_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let Some(log) = crate::event_log_of(&state) else {
        return Err(crate::api_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "事件日志不可用",
        ));
    };
    let todos = crate::todo_tool::apply_todo_op(&log, &session_id, "list", None, None, None, None)
        .await
        .map_err(crate::api_error_bad_request)?;
    Ok(Json(serde_json::json!({ "todos": todos })))
}

/// 应用一次清单操作（增/改/删），返回最新完整清单。
pub async fn post_todos(
    State(state): crate::SharedState,
    Path(session_id): Path<String>,
    Json(params): Json<TodoOpParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let Some(log) = crate::event_log_of(&state) else {
        return Err(crate::api_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "事件日志不可用",
        ));
    };
    let todos = crate::todo_tool::apply_todo_op(
        &log,
        &session_id,
        &params.action,
        params.index,
        params.content.as_deref(),
        params.status.as_deref(),
        params.priority.as_deref(),
    )
    .await
    .map_err(crate::api_error_bad_request)?;
    Ok(Json(serde_json::json!({ "todos": todos })))
}
