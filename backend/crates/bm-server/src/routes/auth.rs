//! UI 登录门（公网站点）：只密码、无用户名。
//!
//! - 默认密码 `adminadmin`（未设置过密码时生效）；设置中心「安全」页可改。
//! - 会话：内存 token（`X-BoenMind-Session` 请求头），30 天有效；重启即全员重登。
//! - 密码记录持久化在 `~/.boenmind/auth.json`（salt + SHA-256，明文不落盘）。
//! - 与 `BOENMIND_TOKEN`（API 守卫，`Authorization: Bearer`）互不干扰：
//!   `/api/auth/*` 在 auth_middleware 中豁免，密码本身即浏览器入口守卫。
//!
//! 浏览器必须先过登录页（未登录不能进聊天/编程/WIKI/设置）；API 客户端
//! 继续走 BOENMIND_TOKEN。桌面壳（Tauri）本地使用不强制登录——前端按
//! `window.__TAURI_INTERNALS__` 判定跳过本门。

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::{ApiResult, api_error};

/// 默认密码（未设置过密码时的出厂值；建议首次登录后在设置里改掉）。
const DEFAULT_PASSWORD: &str = "adminadmin";
/// 会话有效期：30 天。
const SESSION_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// 浏览器会话 token 携带头（与 BOENMIND_TOKEN 的 Authorization 头分离）。
pub const SESSION_HEADER: &str = "x-boenmind-session";
const AUTH_FILE: &str = "auth.json";
/// 新密码最短长度（防手滑清空）。
const MIN_PASSWORD_LEN: usize = 4;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 会话 token → 过期毫秒。进程内存态（无跨重启持久化）：重启后浏览器重登。
static SESSIONS: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, i64>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 随机 token / salt（UUID v4 128 位熵，32 位 hex）。
fn random_hex() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// 密码记录（~/.boenmind/auth.json）。
#[derive(Serialize, Deserialize, Clone)]
struct PasswordRecord {
    salt: String,
    hash: String,
}

fn auth_file() -> std::path::PathBuf {
    bm_core::config::app_dir().join(AUTH_FILE)
}

fn load_password_record() -> Option<PasswordRecord> {
    let data = std::fs::read_to_string(auth_file()).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_password_record(rec: &PasswordRecord) -> std::io::Result<()> {
    let path = auth_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let data =
        serde_json::to_string_pretty(rec).map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(&path, data)
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

/// 常数时间比较（长度不同直接 false；同长逐字节 XOR 累加）。
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 校验密码：有记录 → salt+hash 校验；无记录 → 与默认密码比对。
fn password_matches(password: &str) -> bool {
    match load_password_record() {
        Some(rec) => {
            let digest = Sha256::digest(format!("{}:{}", rec.salt, password).as_bytes());
            let computed = hex_encode(&digest);
            ct_eq(computed.as_bytes(), rec.hash.as_bytes())
        }
        None => {
            let a = Sha256::digest(DEFAULT_PASSWORD.as_bytes());
            let b = Sha256::digest(password.as_bytes());
            ct_eq(&a, &b)
        }
    }
}

/// 设置新密码（随机 salt + SHA-256 落盘）。
fn set_password(new_password: &str) -> std::io::Result<()> {
    let salt = random_hex();
    let digest = Sha256::digest(format!("{salt}:{new_password}").as_bytes());
    save_password_record(&PasswordRecord {
        salt,
        hash: hex_encode(&digest),
    })
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn session_valid(headers: &HeaderMap, map: &HashMap<String, i64>) -> bool {
    let Some(token) = session_token(headers) else {
        return false;
    };
    match map.get(&token) {
        Some(exp) => *exp > now_ms(),
        None => false,
    }
}

/// GET /api/auth/status — 当前浏览器会话是否有效。
pub async fn status(headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    let map = sessions().lock().await;
    let authenticated = session_valid(&headers, &map);
    Ok(Json(serde_json::json!({ "authenticated": authenticated })))
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub password: String,
}

/// POST /api/auth/login — 只密码登录；成功签发会话 token。
pub async fn login(Json(body): Json<LoginBody>) -> ApiResult<Json<serde_json::Value>> {
    if !password_matches(&body.password) {
        return Err(api_error(StatusCode::UNAUTHORIZED, "wrong password"));
    }
    let token = random_hex();
    let mut map = sessions().lock().await;
    map.insert(token.clone(), now_ms() + SESSION_TTL_MS);
    Ok(Json(serde_json::json!({ "ok": true, "token": token })))
}

/// POST /api/auth/logout — 作废当前会话 token（幂等）。
pub async fn logout(headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    let mut map = sessions().lock().await;
    if let Some(token) = session_token(&headers) {
        map.remove(&token);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ChangePasswordBody {
    pub current_password: String,
    pub new_password: String,
}

/// PUT /api/auth/password — 修改密码（设置中心「安全」页）。
///
/// 校验：会话有效 + 当前密码正确。CSRF 由 origin_middleware 兜底（带
/// Origin/Referer 的跨源状态变更被拒）。新密码过短 → 400。
pub async fn change_password(
    headers: HeaderMap,
    Json(body): Json<ChangePasswordBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let map = sessions().lock().await;
    if !session_valid(&headers, &map) {
        return Err(api_error(StatusCode::UNAUTHORIZED, "login required"));
    }
    drop(map);
    if body.new_password.trim().len() < MIN_PASSWORD_LEN {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "new password too short",
        ));
    }
    if !password_matches(&body.current_password) {
        return Err(api_error(StatusCode::UNAUTHORIZED, "wrong current password"));
    }
    set_password(&body.new_password)
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("io: {e}")))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub fn router() -> Router<crate::AppState> {
    Router::new()
        .route("/api/auth/status", get(status))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/password", put(change_password))
}
