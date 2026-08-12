//! 工作文件夹：目录枚举与文件读取（文本 / 二进制 base64）。

use axum::{Json, extract::{Query, State}, http::StatusCode};
use bm_core::workspace;
use serde::Deserialize;

use crate::{ApiResult, api_error};

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
