//! # web-server —— Rust 协议兼容层
//!
//! dsh 前端接口合同 9 面 + 双栅栏的 Rust 镜像（v2 计划 §三）。
//! 实现子集按 §3.5 顺序：静态 SPA → RPC 信封 → WS 下行流 → 完整 API 面。

pub mod api;
pub mod approval;
pub mod events;
pub mod goal;
pub mod goal_driver;
pub mod pending;
pub mod rpc;
pub mod rpc_m3;
pub mod scheduler;
pub mod static_spa;
pub mod trust;
pub mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{FromRequestParts, Path as AxumPath, Query, Request, State};
use axum::http::header::{CONTENT_TYPE, HOST, ORIGIN};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use api::AppState;
use rpc::{ClientRequest, ClientResponse, ServerResponse};

/// 更新服务目录（`--update-dir`）：托管 Tauri updater 的 latest.json 与更新包。
pub type UpdateDir = Option<PathBuf>;

/// 组装完整路由。
/// - `update_dir`：`--update-dir` 传入时为 `Some(dir)`，挂载 `/update/{*path}`
///   静态服务（桌面壳热更新拉取 latest.json / 更新包用）。
pub fn router(
    state: Arc<AppState>,
    dist_root: PathBuf,
    boot_json: Option<String>,
    update_dir: UpdateDir,
) -> Router {
    let app = Router::new()
        .route("/api/{endpoint}", post(handle_rpc))
        .route("/api/respond", post(handle_respond))
        .route("/api/events.mux", get(handle_ws_mux))
        .route("/api/events.host", get(handle_ws_host))
        .route("/api/session.export", get(handle_session_export))
        .route("/api/host.download", get(handle_host_download))
        .route("/api/host.upload", post(handle_host_upload))
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
        );

    // 热更新托管：/update/{*path} → update_dir 下文件（Tauri updater 端点）。
    // 仅 GET；越界同 static_spa 拒（../ 逃逸）。
    // 注意：必须在 .route("/{*path}") 之后挂——axum 0.8 具体段优先，
    // /update/... 命中本路由而非 SPA 兜底。
    if let Some(dir) = update_dir {
        return Router::new()
            .merge(app)
            .route(
                "/update/{*path}",
                get({
                    let dir = dir.clone();
                    move |method: Method, uri: axum::http::Uri| {
                        // axum 的 {*path} 不剥路由前缀：uri.path() 含 /update/ 段，
                        // 而 root=update_dir 不含该前缀，先剥掉再 resolve。
                        // 剥成 owned String 供 async handler 持有。
                        let stripped: String = uri
                            .path()
                            .strip_prefix("/update")
                            .unwrap_or(uri.path())
                            .to_string();
                        static_spa::static_handler_no_spa(method, stripped, dir.clone())
                    }
                }),
            )
            .with_state(state);
    }

    app.with_state(state)
}

/// 会话 token：优先 `x-boenmind-session` 头（API 客户端），其次 `Cookie: dsh_bm_session=...`
/// （浏览器自动携带，HttpOnly）。未登录返回 None。
fn session_token_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(t) = headers
        .get("x-boenmind-session")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return Some(t);
    }
    if let Some(cookie) = headers.get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in cookie.split(';') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix("dsh_bm_session=") {
                let v = v.trim().trim_matches('"').to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
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
    let token = session_token_from_headers(&headers);
    let result = api::dispatch(&state, &request.method, request.payload, token.as_deref()).await;
    let mut resp = ServerResponse {
        type_: "server-response",
        rpc_id: request.rpc_id,
        result,
        set_cookie: None,
    };
    // 登录成功 → Set-Cookie（HttpOnly + SameSite=Strict，浏览器自动携带；
    // 对齐 dsh-webui-auth 会话携带形态——前端零改动）。
    if request.method == "auth.login" {
        if let Some(t) = resp.result.get("value").and_then(|v| v.get("token")).and_then(|v| v.as_str()) {
            let cookie = format!(
                "dsh_bm_session={t}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000"
            );
            resp.set_cookie = Some(cookie);
        }
    }
    // set_cookie → 真实响应头（Set-Cookie 不进 JSON 信封）。
    if let Some(cookie) = resp.set_cookie.take() {
        let mut response = (StatusCode::OK, Json(resp)).into_response();
        if let Ok(v) = HeaderValue::from_str(&cookie) {
            response.headers_mut().insert(axum::http::header::SET_COOKIE, v);
        }
        return response;
    }
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
    // 栅栏 A：respond 也是受信任通道（approval/question 应答会触发权限副作用），
    // 必须与 handle_rpc 同一信任判定——DNS-rebinding 下未栅栏等同放开审批面。
    let host = headers.get(HOST).and_then(|v| v.to_str().ok());
    let origin = headers.get(ORIGIN).and_then(|v| v.to_str().ok());
    let sec_fetch_site = headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok());
    if !trust::is_trusted_api_request(host, origin, sec_fetch_site, &state.trusted_hosts) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
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
        // 唤醒等待中的 loop 审批调用（Allowed-once ↔ Allowed / rejected ↔ Rejected）。
        let verdict = match outcome.as_str() {
            "allowed-once" => bm_ports::ApprovalVerdict::Allowed,
            _ => bm_ports::ApprovalVerdict::Rejected,
        };
        crate::approval::resolve_approval_waiter(state, &approval_id, verdict);
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
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    // 栅栏 A：export 下载完整会话日志（含工具输出/凭据引用的审计面），
    // 未栅栏时 DNS-rebinding 可静默拉走全部会话 JSONL。
    let host = headers.get(HOST).and_then(|v| v.to_str().ok());
    let origin = headers.get(ORIGIN).and_then(|v| v.to_str().ok());
    let sec_fetch_site = headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok());
    if !trust::is_trusted_api_request(host, origin, sec_fetch_site, &state.trusted_hosts) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
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
    let records = match state.runtime.persist.load_events(session_id).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "session not found").into_response(),
        Err(e) => {
            tracing::error!("session.export load failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "export failed").into_response();
        }
    };
    let events: Vec<kernel_contracts::session::SessionEvent> =
        records.into_iter().map(|r| r.event).collect();
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

/// GET /api/host.download（特权，工作目录作用域）：下载 workdir 内文件。
/// 鉴权：同 handle_rpc 的 token 判定（`x-boenmind-session` 头 / HttpOnly cookie），
/// --auth 装配时未认证 → 401（不暴露存在性）。`<img src>` 直接内嵌预览的通道：
/// 图片扩展名 → `Content-Disposition: inline` + allowlist MIME + nosniff + private。
/// 其余 → `attachment`（svg/html 永远附件——防 XSS）。
async fn handle_host_download(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    // 栅栏 A：download 也是受信通道（读任意 workdir 文件）——DNS-rebinding 未栅栏
    // 等同放开文件读取面，必须与 handle_rpc 同一信任判定。
    let host = headers.get(HOST).and_then(|v| v.to_str().ok());
    let origin = headers.get(ORIGIN).and_then(|v| v.to_str().ok());
    let sec_fetch_site = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok());
    if !trust::is_trusted_api_request(host, origin, sec_fetch_site, &state.trusted_hosts) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    // 栅栏 B：特权方法 loopback-pin（下载触碰文件系统，同 trust.rs 特权表语义）。
    if !trust::is_trusted_api_request(host, origin, sec_fetch_site, &[]) {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    // 认证：--auth 装配时要求有效会话（fail-closed），未装配放行（旧行为）。
    if let Some(auth) = &state.runtime.auth {
        let token = session_token_from_headers(&headers);
        if !auth.is_authenticated(token.as_deref().unwrap_or("")) {
            return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
        }
    }
    let Some(rel) = params.get("path") else {
        return (StatusCode::BAD_REQUEST, "missing path query parameter").into_response();
    };
    let Some(wd) = api::host_workdir(&state) else {
        return (StatusCode::CONFLICT, "workdir-not-configured").into_response();
    };
    let target = match host_fs::resolve_in_workdir(&wd, rel) {
        Ok(t) => t,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, e.code()).into_response();
        }
    };
    if !target.is_file() {
        return (StatusCode::NOT_FOUND, "file not found").into_response();
    }
    let ext = target
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let filename = target.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let bytes = match std::fs::read(&target) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("host.download read failed: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "read failed").into_response();
        }
    };
    let inline = host_fs::is_image_previewable(&ext);
    let mut h = HeaderMap::new();
    h.insert("content-type", host_fs::mime_for_ext(&ext).parse().unwrap());
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("cache-control", "private, no-store".parse().unwrap());
    let disposition = if inline {
        format!("inline; filename=\"{filename}\"")
    } else {
        format!("attachment; filename=\"{filename}\"")
    };
    h.insert("content-disposition", disposition.parse().unwrap());
    (StatusCode::OK, h, bytes).into_response()
}

/// POST /api/host.upload（特权，工作目录作用域）：上传单个文件到 workdir 内目录。
/// multipart 字段：`dir`（相对路径，空 = workdir 根）+ `file`（文件，须含原始文件名）。
/// 鉴权同 download；大小上限 100MiB（超限先拒）；目标名须合法单段（禁 / \ .. 与控制字符）；
/// 已存在 → 409（显式 x-bm-overwrite:true 覆盖）。写盘：先收字节（上限内），原子 rename。
async fn handle_host_upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    mut multipart: axum::extract::Multipart,
) -> Response {
    // 栅栏 A + B + 认证：与 download 同一套（上传是写面，风险更高）。
    let host = headers.get(HOST).and_then(|v| v.to_str().ok());
    let origin = headers.get(ORIGIN).and_then(|v| v.to_str().ok());
    let sec_fetch_site = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok());
    if !trust::is_trusted_api_request(host, origin, sec_fetch_site, &state.trusted_hosts)
        || !trust::is_trusted_api_request(host, origin, sec_fetch_site, &[])
    {
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    if let Some(auth) = &state.runtime.auth {
        let token = session_token_from_headers(&headers);
        if !auth.is_authenticated(token.as_deref().unwrap_or("")) {
            return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
        }
    }
    let Some(wd) = api::host_workdir(&state) else {
        return (StatusCode::CONFLICT, "workdir-not-configured").into_response();
    };
    let mut dir_rel: String = String::new();
    let mut file_name: Option<String> = None;
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                tracing::error!("host.upload field failed: {e}");
                return (StatusCode::BAD_REQUEST, "multipart read failed").into_response();
            }
        };
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "dir" => {
                if let Ok(t) = field.text().await {
                    dir_rel = t.trim().to_string();
                }
            }
            "file" => {
                file_name = field.file_name().map(str::to_string);
                match field.bytes().await {
                    Ok(b) => bytes = b.to_vec(),
                    Err(e) => {
                        tracing::error!("host.upload file bytes failed: {e}");
                        return (StatusCode::BAD_REQUEST, "file read failed").into_response();
                    }
                }
            }
            _ => {}
        }
    }
    let file_name = match file_name {
        Some(n) if !n.is_empty() => n,
        _ => return (StatusCode::BAD_REQUEST, "missing file part (with filename)").into_response(),
    };
    if bytes.len() as u64 > host_fs::MAX_UPLOAD_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "file exceeds size limit").into_response();
    }
    let overwrite = headers
        .get("x-bm-overwrite")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "true")
        .unwrap_or(false);
    // 文件名须合法单段（禁 / \ .. 与控制字符）——防路径注入。
    if file_name == "."
        || file_name == ".."
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains('\0')
        || file_name.chars().any(|c| c.is_control() || ":\"*?<>|".contains(c))
    {
        return (StatusCode::BAD_REQUEST, "invalid file name").into_response();
    }
    let parent = match host_fs::resolve_in_workdir(&wd, &dir_rel) {
        Ok(t) => t,
        Err(e) => return (StatusCode::BAD_REQUEST, e.code()).into_response(),
    };
    if !parent.is_dir() {
        return (StatusCode::BAD_REQUEST, "wrong-file-kind").into_response();
    }
    let target = parent.join(&file_name);
    if let Err(e) = host_fs::atomic_write(&target, &bytes, overwrite) {
        let code = e.code();
        return (StatusCode::CONFLICT, code).into_response();
    }
    (StatusCode::OK, "uploaded").into_response()
}
