//! # web-server —— Rust 协议兼容层
//!
//! dsh 前端接口合同 9 面 + 双栅栏的 Rust 镜像（v2 计划 §三）。
//! 实现子集按 §3.5 顺序：静态 SPA → RPC 信封 → WS 下行流 → 完整 API 面。

pub mod api;
pub mod events;
pub mod rpc;
pub mod static_spa;
pub mod trust;
pub mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{FromRequestParts, Path as AxumPath, Query, Request, State};
use axum::http::header::{CONTENT_TYPE, HOST, ORIGIN};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use api::AppState;
use rpc::{ClientRequest, ServerResponse};

/// 组装完整路由。
pub fn router(state: Arc<AppState>, dist_root: PathBuf, boot_json: Option<String>) -> Router {
    Router::new()
        .route("/api/{endpoint}", post(handle_rpc))
        .route("/api/respond", post(handle_respond))
        .route("/api/events.mux", get(handle_ws_mux))
        .route("/api/events.host", get(handle_ws_host))
        .route("/api/session.export", get(handle_session_export))
        .route(
            "/",
            get({
                let dist = dist_root.clone();
                let boot = boot_json.clone();
                move |method: Method, uri: axum::http::Uri| {
                    static_spa::static_handler(method, uri, dist.clone(), boot.clone())
                }
            }),
        )
        .route(
            "/{*path}",
            get({
                let dist = dist_root.clone();
                let boot = boot_json.clone();
                move |method: Method, uri: axum::http::Uri| {
                    static_spa::static_handler(method, uri, dist.clone(), boot.clone())
                }
            }),
        )
        .with_state(state)
}

/// RPC 入口（面 1 + 双栅栏）。
async fn handle_rpc(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(endpoint): AxumPath<String>,
    method: Method,
    body: String,
) -> Response {
    if method != Method::POST {
        return (StatusCode::METHOD_NOT_ALLOWED, "method not allowed").into_response();
    }

    // 栅栏 A：Host/Origin 信任（DNS-rebinding 防御）。
    let host = headers.get(HOST).and_then(|v| v.to_str().ok());
    let origin = headers.get(ORIGIN).and_then(|v| v.to_str().ok());
    let sec_fetch_site = headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok());
    if !trust::is_trusted_api_request(host, origin, sec_fetch_site, &state.trusted_hosts) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    // 栅栏 B：特权方法 loopback-pin（空 trustedHosts = 强制 loopback）。
    if trust::is_privileged_method(&format!("/api/{endpoint}")).is_some()
        && !trust::is_trusted_api_request(host, origin, sec_fetch_site, &[])
    {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    // 载体层：415 非 JSON / 400 非 JSON 体。
    let is_json = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| {
            let mime = ct.split(';').next().unwrap_or("").trim().to_lowercase();
            mime == "application/json"
        })
        .unwrap_or(false);
    if !is_json {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported media type").into_response();
    }

    let request: ClientRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => {
            let rpc_id = rpc::extract_rpc_id(&body);
            let resp = ServerResponse::bad_request(&rpc_id, "invalid request envelope");
            return (StatusCode::OK, Json(resp)).into_response();
        }
    };

    // 方法不匹配路径 → 200 + bad-request（台账 §4，Node 实测逐字命中）。
    if request.method != endpoint {
        let resp = ServerResponse::bad_request(
            &request.rpc_id,
            format!(
                "method \"{}\" does not match path \"{}\"",
                request.method, endpoint
            ),
        );
        return (StatusCode::OK, Json(resp)).into_response();
    }

    let result = api::dispatch(&state, &request.method, request.payload).await;
    let resp = ServerResponse {
        type_: "server-response",
        rpc_id: request.rpc_id,
        result,
    };
    (StatusCode::OK, Json(resp)).into_response()
}

/// WS mux 下行入口（面 2）：信任栅栏 + downlink-only。
/// 普通 GET（非升级）→ 426 upgrade required（Node 实测，台账 §1 行 3）。
async fn handle_ws_mux(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    handle_ws_upgrade(state, headers, req, true).await
}

/// WS host 下行入口（面 3）：同上。
async fn handle_ws_host(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    handle_ws_upgrade(state, headers, req, false).await
}

async fn handle_ws_upgrade(
    state: Arc<AppState>,
    headers: HeaderMap,
    req: Request,
    is_mux: bool,
) -> Response {
    let host = headers.get(HOST).and_then(|v| v.to_str().ok());
    let origin = headers.get(ORIGIN).and_then(|v| v.to_str().ok());
    let sec_fetch_site = headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok());
    if !trust::is_trusted_api_request(host, origin, sec_fetch_site, &state.trusted_hosts) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    // 非升级 GET → 426（台账：connection: Upgrade + upgrade: websocket 头）。
    let wants_upgrade = headers
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_lowercase() == "websocket")
        .unwrap_or(false);
    if !wants_upgrade {
        let mut h = HeaderMap::new();
        h.insert("connection", "Upgrade".parse().unwrap());
        h.insert("upgrade", "websocket".parse().unwrap());
        return (StatusCode::UPGRADE_REQUIRED, h).into_response();
    }
    // 升级路径：从请求 parts 构造 WebSocketUpgrade。
    let (mut parts, _body) = req.into_parts();
    let ws = match WebSocketUpgrade::from_request_parts(&mut parts, &state).await {
        Ok(ws) => ws,
        Err(rej) => return rej.into_response(),
    };
    if is_mux {
        ws.on_upgrade(move |socket| ws::mux_loop(socket, state))
            .into_response()
    } else {
        ws.on_upgrade(move |socket| ws::host_loop(socket, state))
            .into_response()
    }
}

/// POST /api/respond（面 7）：第一版无 pending 表 → not-pending。
async fn handle_respond(
    State(_state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let is_json = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.split(';').next().unwrap_or("").trim().to_lowercase() == "application/json")
        .unwrap_or(false);
    if !is_json {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported media type").into_response();
    }
    if serde_json::from_str::<Value>(&body).is_err() {
        return (StatusCode::BAD_REQUEST, "invalid JSON").into_response();
    }
    (StatusCode::OK, Json(json!({ "accepted": false, "reason": "not-pending" }))).into_response()
}

/// GET /api/session.export（面 8）：第一版无导出 → 404 session not found。
async fn handle_session_export(
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    match params.get("sessionId") {
        Some(_) => (StatusCode::NOT_FOUND, "session not found").into_response(),
        None => (
            StatusCode::BAD_REQUEST,
            "missing or invalid sessionId query parameter",
        )
            .into_response(),
    }
}
