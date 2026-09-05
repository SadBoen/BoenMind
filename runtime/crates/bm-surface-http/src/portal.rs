//! 门户登录墙(2026-09-03 用户令):整站(静态页 + 全部 API 含 /admin、/v1)
//! 登录后方可访问,堵 /admin 无鉴权公网裸奔(VPS 实测暴露)。
//!
//! - 密码存 `<data_dir>/config/portal.json`(`salt$sha256hex`);未配置 = 墙
//!   未启用(既有测试与本地开发零影响);
//! - 首次访问 /login 显示「创建访问密码」(bootstrap,仅未配置时可用一次);
//! - 会话 = 内存随机 Cookie(HttpOnly,30 天),重启即失效需重登;
//! - Bearer 访问令牌(auth.v0_1)继续全效,程序化访问不受影响;
//! - /health、/login 页面本体与登录/状态接口豁免。

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const SESSION_COOKIE: &str = "boen_session";

pub struct PortalAuth {
    pub data_dir: PathBuf,
    /// `salt$sha256hex`;None = 未设密码(墙未启用,全部放行)。
    pub password_hash: Mutex<Option<String>>,
    pub sessions: Mutex<HashSet<String>>,
}

impl PortalAuth {
    pub fn load(data_dir: PathBuf) -> Arc<Self> {
        let hash = std::fs::read_to_string(data_dir.join("config/portal.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["password_hash"].as_str().map(String::from));
        Arc::new(Self {
            data_dir,
            password_hash: Mutex::new(hash),
            sessions: Mutex::new(HashSet::new()),
        })
    }

    pub fn configured(&self) -> bool {
        self.password_hash.lock().expect("锁未中毒").is_some()
    }

    fn save(&self, hash: &str) {
        let cfg = self.data_dir.join("config");
        if let Err(e) = std::fs::create_dir_all(&cfg) {
            eprintln!("[portal] 配置目录创建失败: {e}");
            return;
        }
        let text = serde_json::to_string_pretty(&json!({ "password_hash": hash })).expect("序列化");
        if let Err(e) = bm_persist::atomic_write(&cfg.join("portal.json"), text.as_bytes()) {
            eprintln!("[portal] 密码落盘失败: {e}");
        }
        *self.password_hash.lock().expect("锁未中毒") = Some(hash.to_string());
    }
}

pub fn hash_password(password: &str, salt: &str) -> String {
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update(b"$");
    h.update(password.as_bytes());
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn new_salt() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("系统熵源");
    hex(&b)
}

fn new_session() -> String {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("系统熵源");
    hex(&b)
}

fn cookie_session(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        let p = part.trim();
        if let Some(v) = p.strip_prefix(SESSION_COOKIE)?.strip_prefix('=') {
            return Some(v.to_string());
        }
    }
    None
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn authed(state: &crate::AppState, headers: &HeaderMap) -> bool {
    let bearer_ok = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|given| constant_time_eq(given.as_bytes(), state.token.as_bytes()))
        .unwrap_or(false);
    if bearer_ok {
        return true;
    }
    match cookie_session(headers) {
        Some(s) => state.portal.sessions.lock().expect("锁未中毒").contains(&s),
        None => false,
    }
}

/// 门户中间件:墙未启用→放行;Bearer/Cookie 通过→放行;豁免路径放行;
/// 否则 HTML 导航 302 /login,其余 401。
pub async fn require_portal(
    State(state): State<crate::AppState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let exempt = path == "/health"
        || path == "/login"
        || path == "/api/portal/state"
        || path == "/api/portal/login"
        || path == "/api/portal/bootstrap";
    // 外部评审 2026-09-03 #9:未配置密码时,公网绑定不再全站放行——仅
    // 健康检查与门户设置口可达(/v1、/admin、静态一律 401/302);回环
    // 绑定(本机开发)维持零影响放行;持 Bearer 令牌者不受影响。
    let open = if state.portal.configured() {
        exempt
    } else {
        !state.public_bind || exempt
    };
    if open || authed(&state, req.headers()) {
        return next.run(req).await;
    }
    let wants_html = path == "/"
        || req
            .headers()
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .map(|a| a.contains("text/html"))
            .unwrap_or(false);
    if wants_html {
        return (StatusCode::FOUND, [(header::LOCATION, "/login")]).into_response();
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": {"message": "需要登录"}})),
    )
        .into_response()
}

fn session_cookie(value: &str) -> String {
    format!("{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000")
}

fn unauthorized(msg: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": {"message": msg}})),
    )
        .into_response()
}

/// GET /api/portal/state:登录页据此显示「创建访问密码」或「登录」。
pub async fn portal_state(State(state): State<crate::AppState>, headers: HeaderMap) -> Response {
    Json(json!({
        "configured": state.portal.configured(),
        "authed": authed(&state, &headers),
    }))
    .into_response()
}

/// POST /api/portal/bootstrap {password}:仅未设密码时可用一次;设置并登录。
pub async fn portal_bootstrap(
    State(state): State<crate::AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if state.portal.configured() {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": {"message": "访问密码已设置,请直接登录"}})),
        )
            .into_response();
    }
    let pw = body["password"].as_str().unwrap_or_default();
    if pw.chars().count() < 6 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "密码至少 6 位"}})),
        )
            .into_response();
    }
    let salt = new_salt();
    state
        .portal
        .save(&format!("{salt}${}", hash_password(pw, &salt)));
    let session = new_session();
    state
        .portal
        .sessions
        .lock()
        .expect("锁未中毒")
        .insert(session.clone());
    (
        [(header::SET_COOKIE, session_cookie(&session))],
        Json(json!({"ok": true})),
    )
        .into_response()
}

/// POST /api/portal/login {password}。
pub async fn portal_login(
    State(state): State<crate::AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let ok = state
        .portal
        .password_hash
        .lock()
        .expect("锁未中毒")
        .as_ref()
        .map(|h| {
            let (salt, expect) = h.split_once('$').unwrap_or(("", ""));
            let computed = hash_password(body["password"].as_str().unwrap_or_default(), salt);
            constant_time_eq(computed.as_bytes(), expect.as_bytes())
        })
        .unwrap_or(false);
    if !ok {
        return unauthorized("密码不对");
    }
    let session = new_session();
    state
        .portal
        .sessions
        .lock()
        .expect("锁未中毒")
        .insert(session.clone());
    (
        [(header::SET_COOKIE, session_cookie(&session))],
        Json(json!({"ok": true})),
    )
        .into_response()
}

/// POST /api/portal/password {old, new}:改密(需已登录);改后作废全部
/// 会话,各端重新登录。
pub async fn portal_password(
    State(state): State<crate::AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if !authed(&state, &headers) {
        return unauthorized("需要登录");
    }
    let old_ok = state
        .portal
        .password_hash
        .lock()
        .expect("锁未中毒")
        .as_ref()
        .map(|h| {
            let (salt, expect) = h.split_once('$').unwrap_or(("", ""));
            let computed = hash_password(body["old"].as_str().unwrap_or_default(), salt);
            constant_time_eq(computed.as_bytes(), expect.as_bytes())
        })
        .unwrap_or(false);
    if !old_ok {
        return unauthorized("旧密码不对");
    }
    let pw = body["new"].as_str().unwrap_or_default();
    if pw.chars().count() < 6 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "新密码至少 6 位"}})),
        )
            .into_response();
    }
    let salt = new_salt();
    state
        .portal
        .save(&format!("{salt}${}", hash_password(pw, &salt)));
    state.portal.sessions.lock().expect("锁未中毒").clear();
    Json(json!({"ok": true, "note": "密码已更新,请重新登录"})).into_response()
}

/// GET /login:登录页(web_dir 下 login.html)。
pub async fn login_page(State(state): State<crate::AppState>) -> Response {
    match state.web_dir.as_ref() {
        Some(dir) => match std::fs::read_to_string(dir.join("login.html")) {
            Ok(html) => {
                ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
            }
            Err(_) => (StatusCode::NOT_FOUND, "login.html 缺失").into_response(),
        },
        None => (StatusCode::NOT_FOUND, "未挂载 Web 目录").into_response(),
    }
}
