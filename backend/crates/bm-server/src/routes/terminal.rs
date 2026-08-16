//! 终端 API（TerminalPane 一期）：创建 pty 会话、写输入、调尺寸、SSE 输出流、关闭。
//!
//! - `POST /api/terminal`            创建会话 `{ cwd?, cols?, rows? }` → `{ id }`
//! - `POST /api/terminal/{id}/input` 写输入 `{ data }`（base64）
//! - `POST /api/terminal/{id}/resize` 调尺寸 `{ cols, rows }`
//! - `GET  /api/terminal/{id}/stream` SSE 输出流（output/exit 事件）
//! - `DELETE /api/terminal/{id}`     关闭会话

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    response::sse::Event,
};
use serde::Deserialize;

use crate::{ApiResult, api_error};

#[derive(Deserialize)]
pub struct CreateTerminalRequest {
    /// 启动目录（缺省 = 配置工作目录）；前端传当前项目根
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default = "default_cols")]
    pub cols: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
}

fn default_cols() -> u16 {
    100
}
fn default_rows() -> u16 {
    30
}

pub async fn create_terminal(
    State(state): crate::SharedState,
    Json(req): Json<CreateTerminalRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let cwd = {
        let config = state.config.read().expect("config poisoned");
        match req.cwd {
            Some(raw) if !raw.trim().is_empty() => {
                let candidate = std::path::PathBuf::from(raw.trim());
                let allowed = bm_core::workspace::trusted_roots(&config);
                if bm_core::workspace::path_under_any(&candidate, &allowed) {
                    Some(candidate.to_string_lossy().into_owned())
                } else {
                    return Err(api_error(
                        StatusCode::BAD_REQUEST,
                        format!("终端 cwd 未登记：{}", candidate.display()),
                    ));
                }
            }
            _ => Some(config.working_dir.to_string_lossy().into_owned()),
        }
    };
    let id = state
        .terminal
        .create(cwd, req.cols, req.rows)
        .await
        .map_err(|msg| api_error(StatusCode::INTERNAL_SERVER_ERROR, msg))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct TerminalInputRequest {
    /// base64 编码的输入字节（任意字节安全过 JSON）
    pub data: String,
}

pub async fn terminal_input(
    State(state): crate::SharedState,
    axum::extract::Path(id): Path<String>,
    Json(req): Json<TerminalInputRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &req.data)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("输入不是合法 base64: {e}")))?;
    let session = state
        .terminal
        .get(&id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "终端会话不存在"))?;
    session
        .write(&bytes)
        .await
        .map_err(|msg| api_error(StatusCode::INTERNAL_SERVER_ERROR, msg))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct TerminalResizeRequest {
    pub cols: u16,
    pub rows: u16,
}

pub async fn terminal_resize(
    State(state): crate::SharedState,
    axum::extract::Path(id): Path<String>,
    Json(req): Json<TerminalResizeRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    state
        .terminal
        .resize(&id, req.cols, req.rows)
        .await
        .map_err(|msg| api_error(StatusCode::NOT_FOUND, msg))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// SSE 输出流：`{"type":"output","data":"<base64>"}` / `{"type":"exit","code":N}`。
/// 客户端断开后流结束；会话退出时注册表已自清理（下次访问 404）。
pub async fn terminal_stream(
    State(state): crate::SharedState,
    axum::extract::Path(id): Path<String>,
) -> Response {
    use axum::response::sse::KeepAlive;
    use tokio_stream::{StreamExt, wrappers::BroadcastStream};

    let rx = match state.terminal.subscribe(&id) {
        Ok(rx) => rx,
        Err(_) => return api_error(StatusCode::NOT_FOUND, "终端会话不存在").into_response(),
    };
    let stream = BroadcastStream::new(rx).filter_map(|item| match item {
        Ok(ev) => {
            let json = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".to_string());
            Some(Ok::<_, std::convert::Infallible>(Event::default().data(json)))
        }
        // 会话广播通道关闭（全部 sender drop）→ 流结束
        Err(_) => None,
    });
    axum::response::sse::Sse::new(stream)
        .keep_alive(KeepAlive::default().interval(std::time::Duration::from_secs(15)))
        .into_response()
}

pub async fn close_terminal(
    State(state): crate::SharedState,
    axum::extract::Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    state
        .terminal
        .kill(&id)
        .await
        .map_err(|msg| api_error(StatusCode::NOT_FOUND, msg))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
