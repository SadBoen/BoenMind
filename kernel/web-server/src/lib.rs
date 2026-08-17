//! # web-server —— Rust 协议兼容层
//!
//! dsh 前端接口合同 9 面 + 双栅栏的 Rust 镜像（v2 计划 §三）。
//! 实现子集按 §3.5 顺序：静态 SPA → RPC 信封 → WS 下行流 → 完整 API 面。

pub mod api;
pub mod events;
pub mod pending;
pub mod provider_config;
pub mod rpc;
pub mod rpc_m3;
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
use rpc::{ClientRequest, ClientResponse, ServerResponse};

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

    tracing::info!(method = %request.method, "rpc call");
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

/// POST /api/respond（面 7）：approval 先、question 后的 pending 表路由（台账 §1 行 7）。
async fn handle_respond(
    State(state): State<Arc<AppState>>,
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
    // 信封解析失败 → {accepted:false, reason:'bad-response'}（台账：fetch/handler.ts 同款）。
    let envelope: ClientResponse = match serde_json::from_str(&body) {
        Ok(e) => e,
        Err(_) => return (StatusCode::OK, Json(json!({ "accepted": false, "reason": "bad-response" }))).into_response(),
    };

    let receipt = respond_dispatch(&state, &envelope);
    (StatusCode::OK, Json(receipt)).into_response()
}

/// respond 分发（approval 先、question 后）。
fn respond_dispatch(state: &AppState, message: &ClientResponse) -> Value {
    let mut reg = state.pending.lock();
    // ---- approval 表先查 ----
    if let Some(pending) = reg.approvals.get(&message.rpc_id).cloned() {
        if !message.result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            return json!({ "accepted": false, "reason": "bad-response" });
        }
        let value = message.result.get("value").cloned().unwrap_or(Value::Null);
        // 应答负载须 {sessionId, approvalId, outcome:'allowed-once'|'rejected'}
        // 且 approvalId/sessionId 与登记一致（对齐 approvals.schema.ts）。
        let outcome = value.get("outcome").and_then(Value::as_str);
        let ok_outcome = matches!(outcome, Some("allowed-once") | Some("rejected"));
        let matches = value.get("sessionId").and_then(Value::as_str) == Some(pending.session_id.as_str())
            && value.get("approvalId").and_then(Value::as_str) == Some(pending.approval_id.as_str())
            && ok_outcome;
        if !matches {
            return json!({ "accepted": false, "reason": "bad-response" });
        }
        reg.approvals.remove(&message.rpc_id);
        let session_id = pending.session_id.clone();
        let approval_id = pending.approval_id.clone();
        let outcome = outcome.unwrap_or("rejected").to_string();
        drop(reg);
        // 纯推送：approval/resolved（outcome 'allowed-once'|'rejected'）。
        state.broadcast_mux_frame(
            uuid::Uuid::new_v4().to_string(),
            "approval/resolved",
            json!({
                "sessionId": session_id,
                "approvalId": approval_id,
                "outcome": outcome,
            }),
        );
        return json!({ "accepted": true });
    }
    // ---- question 表后查 ----
    let pending = match reg.questions.get(&message.rpc_id).cloned() {
        Some(p) => p,
        None => return json!({ "accepted": false, "reason": "not-pending" }),
    };
    let ok = message.result.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if !ok {
        // result.ok:false && error.code==='cancelled' → accepted（用户取消）。
        let code = message
            .result
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if code != "cancelled" {
            return json!({ "accepted": false, "reason": "bad-response" });
        }
        reg.questions.remove(&message.rpc_id);
        let session_id = pending.session_id.clone();
        let rpc_id = pending.rpc_id.clone();
        drop(reg);
        state.broadcast_mux_frame(
            uuid::Uuid::new_v4().to_string(),
            "question/resolved",
            json!({ "sessionId": session_id, "questionRpcId": rpc_id, "outcome": "cancelled" }),
        );
        return json!({ "accepted": true });
    }
    let value = message.result.get("value").cloned().unwrap_or(Value::Null);
    let session_id = value.get("sessionId").and_then(Value::as_str).unwrap_or("");
    let answers = value.get("answer").and_then(|a| a.get("answers")).cloned().unwrap_or(Value::Null);
    if !reg.matches_questions(&pending, session_id, &answers) {
        return json!({ "accepted": false, "reason": "bad-response" });
    }
    reg.questions.remove(&message.rpc_id);
    let sid = pending.session_id.clone();
    let rpc_id = pending.rpc_id.clone();
    drop(reg);
    state.broadcast_mux_frame(
        uuid::Uuid::new_v4().to_string(),
        "question/resolved",
        json!({ "sessionId": sid, "questionRpcId": rpc_id, "outcome": "answered" }),
    );
    json!({ "accepted": true })
}

/// GET /api/session.export（面 8）：会话日志 ZIP 下载（session.jsonl；无子代理/媒体段）。
async fn handle_session_export(
    State(state): State<Arc<AppState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    use std::io::Write;

    let Some(session_id) = params.get("sessionId") else {
        return (
            StatusCode::BAD_REQUEST,
            "missing or invalid sessionId query parameter",
        )
            .into_response();
    };
    if let Some(inc) = params.get("includeDescendants") {
        if inc != "true" && inc != "false" {
            return (
                StatusCode::BAD_REQUEST,
                "missing or invalid sessionId query parameter",
            )
                .into_response();
        }
    }
    let events = match state.runtime.persist.load_events(session_id).await {
        Ok(Some(e)) => e,
        Ok(None) => return (StatusCode::NOT_FOUND, "session not found").into_response(),
        Err(e) => {
            tracing::error!("session.export load failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "export failed").into_response();
        }
    };
    let wire = crate::events::translate_events(&events);
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options: zip::write::SimpleFileOptions =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        if zip.start_file("session.jsonl", options).is_err() {
            return (StatusCode::INTERNAL_SERVER_ERROR, "export failed").into_response();
        }
        for ev in &wire {
            let line = serde_json::to_string(ev).unwrap_or_default();
            if zip.write_all(line.as_bytes()).is_err() {
                return (StatusCode::INTERNAL_SERVER_ERROR, "export failed").into_response();
            }
            if zip.write_all(b"\n").is_err() {
                return (StatusCode::INTERNAL_SERVER_ERROR, "export failed").into_response();
            }
        }
        if zip.finish().is_err() {
            return (StatusCode::INTERNAL_SERVER_ERROR, "export failed").into_response();
        }
    }
    // id 非 [A-Za-z0-9_-] 字符替换为 _（台账：dsh-session-<id>.zip）。
    let safe_id: String = session_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let filename = format!("dsh-session-{safe_id}.zip");
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/zip".parse().unwrap());
    headers.insert(
        "content-disposition",
        format!("attachment; filename=\"{filename}\"").parse().unwrap(),
    );
    (StatusCode::OK, headers, buf).into_response()
}
