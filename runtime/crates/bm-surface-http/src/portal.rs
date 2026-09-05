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
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const SESSION_COOKIE: &str = "boen_session";

/// 登录失败限速(ADR-0009 决策 4 承兑,2026-09-05):同一来源 5 次失败
/// 锁定 15 分钟。取不到对端地址(测试 oneshot 等)时退化为全局门。
const LOGIN_MAX_FAILURES: u32 = 5;
const LOGIN_LOCKOUT: Duration = Duration::from_secs(15 * 60);
/// PBKDF2-HMAC-SHA256 迭代次数(2026-09-05 起;旧单层 SHA-256 条目在
/// 登录成功时透明升级,离线爆破成本从 10⁹/秒 量级降至 10⁵/秒 以下)
const PBKDF2_ITERS: u32 = 100_000;

pub struct PortalAuth {
    pub data_dir: PathBuf,
    /// `salt$sha256hex`(legacy)或 `pbkdf2$<iters>$<salt>$<hash>`;None = 未设密码。
    pub password_hash: Mutex<Option<String>>,
    pub sessions: Mutex<HashSet<String>>,
    /// 登录失败限速台账:来源 → (连续失败次数, 锁定到期时刻)。
    login_gate: Mutex<HashMap<String, (u32, Option<Instant>)>>,
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
            login_gate: Mutex::new(HashMap::new()),
        })
    }

    pub fn configured(&self) -> bool {
        self.password_hash.lock().expect("锁未中毒").is_some()
    }

    /// 该来源是否处于登录锁定中。
    fn login_locked(&self, key: &str) -> bool {
        self.login_gate
            .lock()
            .expect("锁未中毒")
            .get(key)
            .is_some_and(|(n, until)| {
                *n >= LOGIN_MAX_FAILURES && until.is_some_and(|t| Instant::now() < t)
            })
    }

    fn note_login_failure(&self, key: &str) {
        let mut gate = self.login_gate.lock().expect("锁未中毒");
        let e = gate.entry(key.to_string()).or_insert((0, None));
        e.0 = e.0.saturating_add(1);
        if e.0 >= LOGIN_MAX_FAILURES {
            e.1 = Some(Instant::now() + LOGIN_LOCKOUT);
        }
        // 台账 GC:条目过多时清掉不在锁定期的旧项(防无界增长)
        if gate.len() > 1024 {
            gate.retain(|_, (n, until)| {
                *n < LOGIN_MAX_FAILURES || until.is_some_and(|t| Instant::now() < t)
            });
        }
    }

    fn note_login_success(&self, key: &str) {
        self.login_gate.lock().expect("锁未中毒").remove(key);
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

/// HMAC-SHA256(RFC 2104;key 长度 > 64 字节时先压缩)。
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let mut h = Sha256::new();
        h.update(key);
        k[..32].copy_from_slice(&h.finalize());
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(msg);
    let ih = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(ih);
    outer.finalize().into()
}

/// PBKDF2-HMAC-SHA256(RFC 2898;dkLen = 32 字节 = 单块输出)。
fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iters: u32) -> [u8; 32] {
    let mut salt_block = salt.to_vec();
    salt_block.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac_sha256(password, &salt_block);
    let mut acc = u;
    for _ in 1..iters.max(1) {
        u = hmac_sha256(password, &u);
        for (a, b) in acc.iter_mut().zip(u.iter()) {
            *a ^= *b;
        }
    }
    acc
}

/// 存储新密码:`pbkdf2$<iters>$<salt>$<hash>`。
fn store_password(password: &str) -> String {
    let salt = new_salt();
    let dk = pbkdf2_hmac_sha256(password.as_bytes(), salt.as_bytes(), PBKDF2_ITERS);
    format!("pbkdf2${PBKDF2_ITERS}${salt}${}", hex(&dk))
}

/// 校验密码:兼容 legacy 单层 SHA-256 与 PBKDF2 两种存储形态。
fn verify_password(stored: &str, password: &str) -> bool {
    if let Some(rest) = stored.strip_prefix("pbkdf2$") {
        let Some((iters, rest)) = rest.split_once('$') else {
            return false;
        };
        let Ok(iters) = iters.parse::<u32>() else {
            return false;
        };
        let Some((salt, expect)) = rest.split_once('$') else {
            return false;
        };
        let dk = pbkdf2_hmac_sha256(password.as_bytes(), salt.as_bytes(), iters);
        constant_time_eq(hex(&dk).as_bytes(), expect.as_bytes())
    } else {
        let (salt, expect) = stored.split_once('$').unwrap_or(("", ""));
        let computed = hash_password(password, salt);
        constant_time_eq(computed.as_bytes(), expect.as_bytes())
    }
}

fn hex(bytes: &[u8]) -> String {
    bm_contract::hash::hex(bytes)
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
        // 逐段局部匹配:非本会话名的 cookie(浏览器可能排在前)必须跳过
        // 继续找,绝不能用 ? 让整个函数提前返回(2026-09-05 回看修复)。
        let Some(rest) = p.strip_prefix(SESSION_COOKIE) else {
            continue;
        };
        if let Some(v) = rest.strip_prefix('=') {
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
    state.portal.save(&store_password(pw));
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
/// 2026-09-05 回看加固:失败限速(ADR-0009 决策 4)+ PBKDF2 透明升级。
pub async fn portal_login(
    State(state): State<crate::AppState>,
    // ConnectInfo 由 into_make_service_with_connect_info 注入(本质是
    // Extension);Option 形态让 oneshot 测试等无 connect-info 场景自动
    // 退化为全局限速门
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    // 限速门:按对端 IP;取不到对端地址(测试 oneshot)时退化为全局门
    let gate_key = peer
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "global".to_string());
    if state.portal.login_locked(&gate_key) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error": {"message": "登录失败次数过多,请 15 分钟后再试"}})),
        )
            .into_response();
    }
    let pw = body["password"].as_str().unwrap_or_default();
    let stored = state.portal.password_hash.lock().expect("锁未中毒").clone();
    let ok = stored
        .as_ref()
        .map(|h| verify_password(h, pw))
        .unwrap_or(false);
    if !ok {
        state.portal.note_login_failure(&gate_key);
        return unauthorized("密码不对");
    }
    state.portal.note_login_success(&gate_key);
    // 透明升级:legacy 单层 SHA-256 登录成功即改存 PBKDF2(防离线爆破)
    if stored.as_ref().is_some_and(|h| !h.starts_with("pbkdf2$")) {
        state.portal.save(&store_password(pw));
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
        .map(|h| verify_password(h, body["old"].as_str().unwrap_or_default()))
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
    state.portal.save(&store_password(pw));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie_map(value: &str) -> HeaderMap {
        let mut m = HeaderMap::new();
        m.insert(
            header::COOKIE,
            header::HeaderValue::from_str(value).expect("合法头"),
        );
        m
    }

    #[test]
    fn cookie_session_skips_non_session_cookies_before_target() {
        // 2026-09-05 回看修复:此前首个非 boen_session 的 cookie 会因 ? 短路
        // 整个解析,合法会话被静默丢弃。回归锁死:目标 cookie 在任意位置都能取到。
        assert_eq!(
            cookie_session(&cookie_map("theme=dark; boen_session=abc123")),
            Some("abc123".to_string())
        );
        assert_eq!(
            cookie_session(&cookie_map("boen_session=xyz")),
            Some("xyz".to_string())
        );
        assert_eq!(cookie_session(&cookie_map("theme=dark; a=b")), None);
        assert_eq!(cookie_session(&HeaderMap::new()), None);
        // 同名前缀 cookie 不得误匹配(boen_session_extra)
        assert_eq!(
            cookie_session(&cookie_map("boen_session_extra=evil; theme=dark")),
            None
        );
        // 值中含 = 只切第一个
        assert_eq!(
            cookie_session(&cookie_map("boen_session=a=b")),
            Some("a=b".to_string())
        );
    }

    #[test]
    fn pbkdf2_hmac_sha256_known_vectors() {
        // RFC 7914 §11 PBKDF2-HMAC-SHA256 测试向量(P="password", S="salt")
        let v = |iters| {
            let dk = pbkdf2_hmac_sha256(b"password", b"salt", iters);
            hex(&dk)
        };
        assert_eq!(
            v(1),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        assert_eq!(
            v(4096),
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
        );
    }

    #[test]
    fn verify_password_supports_legacy_and_pbkdf2() {
        let legacy_pw = "hunter22";
        let salt = "deadbeef";
        let legacy = format!("{salt}${}", hash_password(legacy_pw, salt));
        assert!(verify_password(&legacy, legacy_pw));
        assert!(!verify_password(&legacy, "wrong"));

        let modern = store_password(legacy_pw);
        assert!(modern.starts_with("pbkdf2$"), "新密码走 PBKDF2 存储");
        assert!(verify_password(&modern, legacy_pw));
        assert!(!verify_password(&modern, "wrong"));
    }

    #[test]
    fn login_gate_locks_after_max_failures_and_clears_on_success() {
        let dir = tempfile::tempdir().expect("临时目录");
        let auth = PortalAuth::load(dir.path().to_path_buf());
        let key = "1.2.3.4";
        assert!(!auth.login_locked(key));
        for _ in 0..LOGIN_MAX_FAILURES {
            auth.note_login_failure(key);
        }
        assert!(auth.login_locked(key), "达上限即锁定");
        // 其他来源不受影响
        assert!(!auth.login_locked("5.6.7.8"));
        auth.note_login_success(key);
        assert!(!auth.login_locked(key), "成功登录清零");
    }
}
