//! REST 路由：健康检查、配置、会话 CRUD、工作文件夹。

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use bm_core::{AppConfig, workspace};
use serde::Deserialize;
use uuid::Uuid;

use crate::{ApiResult, VERSION, api_error};

// ---------------------------------------------------------------------------
// 健康检查
// ---------------------------------------------------------------------------

pub async fn health(State(state): crate::SharedState) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    Json(serde_json::json!({
        "status": "ok",
        "version": VERSION,
        "workingDir": config.working_dir,
        "providers": config.providers.len(),
        "theme": config.theme,
    }))
}

// ---------------------------------------------------------------------------
// 配置
// ---------------------------------------------------------------------------

pub async fn get_config(State(state): crate::SharedState) -> Json<AppConfig> {
    Json(state.config.read().await.clone())
}

pub async fn put_config(
    State(state): crate::SharedState,
    Json(config): Json<AppConfig>,
) -> ApiResult<Json<serde_json::Value>> {
    // 基本校验：提供商 id 唯一
    let mut seen = std::collections::HashSet::new();
    for p in &config.providers {
        if p.id.trim().is_empty() {
            return Err(api_error(StatusCode::BAD_REQUEST, "提供商 id 不能为空"));
        }
        if !seen.insert(p.id.clone()) {
            return Err(api_error(StatusCode::BAD_REQUEST, format!("提供商 id 重复: {}", p.id)));
        }
    }
    if let Some(default_id) = &config.default_provider {
        if !config.providers.iter().any(|p| &p.id == default_id) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                format!("默认提供商不存在: {default_id}"),
            ));
        }
    }

    // 持久化 + 同步 pi models.json + 更新内存
    if let Err(err) = bm_core::config::save(&config) {
        return Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("配置保存失败: {err}"),
        ));
    }
    if let Err(err) = bm_core::config::sync_pi_models_json(&config) {
        return Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("pi models.json 同步失败: {err}"),
        ));
    }
    let _ = bm_core::config::ensure_working_dir(&config);
    *state.config.write().await = config;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// 会话
// ---------------------------------------------------------------------------

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
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let mut session = session;
    if let Some(title) = req.title {
        if !title.trim().is_empty() {
            state
                .db
                .rename_session(&session.id, title.trim())
                .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
            session.title = title.trim().to_string();
        }
    }
    Ok(Json(session))
}

pub async fn list_sessions(State(state): crate::SharedState) -> ApiResult<Json<Vec<bm_core::db::Session>>> {
    state
        .db
        .list_sessions()
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
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("会话不存在: {id}")))?;
    let messages = state
        .db
        .list_messages(&id)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(serde_json::json!({
        "session": session,
        "messages": messages,
    })))
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
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_session(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    state
        .db
        .delete_session(&id)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    // 清理对应的 agent 会话句柄
    state.agents.lock().await.remove(&id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// 工作文件夹
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListWorkspaceParams {
    #[serde(default)]
    pub dir: String,
}

pub async fn list_workspace(
    State(state): crate::SharedState,
    Query(params): Query<ListWorkspaceParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().await;
    let root = config.working_dir.clone();
    drop(config);
    match workspace::list_dir(&root, &params.dir) {
        Ok(entries) => Ok(Json(serde_json::json!({
            "dir": params.dir,
            "entries": entries,
        }))),
        Err(workspace::WorkspaceError::OutsideRoot(msg)) => {
            Err(api_error(StatusCode::BAD_REQUEST, format!("路径越界: {msg}")))
        }
        Err(err) => Err(api_error(StatusCode::BAD_REQUEST, err.to_string())),
    }
}

#[derive(Deserialize)]
pub struct ReadFileParams {
    pub path: String,
}

/// 读取工作文件夹内文件。文本文件返回 UTF-8 内容，二进制（图片/PDF）返回 base64。
pub async fn read_workspace_file(
    State(state): crate::SharedState,
    Query(params): Query<ReadFileParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().await;
    let root = config.working_dir.clone();
    drop(config);

    let bytes = match workspace::read_file(&root, &params.path) {
        Ok(b) => b,
        Err(workspace::WorkspaceError::OutsideRoot(msg)) => {
            return Err(api_error(StatusCode::BAD_REQUEST, format!("路径越界: {msg}")));
        }
        Err(err) => {
            return Err(api_error(StatusCode::BAD_REQUEST, err.to_string()));
        }
    };
    let name = params
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&params.path)
        .to_string();
    let mime = workspace::mime_for(&name);

    if workspace::is_text(mime) {
        match String::from_utf8(bytes) {
            Ok(content) => Ok(Json(serde_json::json!({
                "name": name,
                "path": params.path,
                "mime": mime,
                "kind": "text",
                "content": content,
                "size": content.len(),
            }))),
            Err(_) => Err(api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "文件不是合法的 UTF-8 文本",
            )),
        }
    } else {
        Ok(Json(serde_json::json!({
            "name": name,
            "path": params.path,
            "mime": mime,
            "kind": "binary",
            "content": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes),
            "size": bytes.len(),
        })))
    }
}
