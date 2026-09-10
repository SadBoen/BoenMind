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
// W10(ADR-0024):失败次数/锁定时长/Cookie 有效期走 self.limits
// (代码默认:5 次 / 15 分钟 / 30 天,见 Limits::default)。
/// PBKDF2-HMAC-SHA256 迭代次数(2026-09-05 起;旧单层 SHA-256 条目在
/// 登录成功时透明升级,离线爆破成本从 10⁹/秒 量级降至 10⁵/秒 以下)
const PBKDF2_ITERS: u32 = 100_000;

/// issue #47:OIDC/OAuth2 配置(portal.json 的可选 `oauth` 节;未配置 =
/// 门户墙行为与现状完全一致)。手动端点声明(不做 .well-known 发现,
/// 私有部署 IdP 语义最少);client_secret = confidential client 背通道交换。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OidcConfig {
    /// 可选:校验 id_token.iss(未配则跳过 iss 校验)。
    pub issuer: Option<String>,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default = "default_oidc_scopes")]
    pub scopes: Vec<String>,
    /// 可选:回调地址覆盖(缺省由请求 Host 推导 http://{host}/api/portal/oauth/callback)。
    pub redirect_uri: Option<String>,
}

fn default_oidc_scopes() -> Vec<String> {
    vec!["openid".into(), "email".into(), "profile".into()]
}

pub struct PortalAuth {
    pub data_dir: PathBuf,
    /// W10(ADR-0024):锁定阈值/时长/Cookie 有效期热读单元。
    pub limits: bm_core::limits::LimitsCell,
    /// `salt$sha256hex`(legacy)或 `pbkdf2$<iters>$<salt>$<hash>`;None = 未设密码。
    pub password_hash: Mutex<Option<String>>,
    pub sessions: Mutex<HashSet<String>>,
    /// 登录失败限速台账:来源 → (连续失败次数, 锁定到期时刻)。
    login_gate: Mutex<HashMap<String, (u32, Option<Instant>)>>,
    /// #47:OIDC 配置(None = 未启用)。
    pub oauth: Option<OidcConfig>,
    /// #47:OAuth 流程防伪状态:state → 创建时刻(TTL 内一次性)。
    oauth_states: Mutex<HashMap<String, Instant>>,
}

/// OAuth state 有效期。
const OAUTH_STATE_TTL: Duration = Duration::from_secs(600);

impl PortalAuth {
    pub fn load_with_limits(data_dir: PathBuf, limits: bm_core::limits::LimitsCell) -> Arc<Self> {
        let auth = Self::load(data_dir);
        let a = Arc::into_inner(auth).expect("装配方独占");
        let mut a = a;
        a.limits = limits;
        Arc::new(a)
    }

    pub fn load(data_dir: PathBuf) -> Arc<Self> {
        let cfg = std::fs::read_to_string(data_dir.join("config/portal.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        let hash = cfg
            .as_ref()
            .and_then(|v| v["password_hash"].as_str().map(String::from));
        let oauth = cfg
            .as_ref()
            .and_then(|v| v.get("oauth"))
            .and_then(|val| serde_json::from_value(val.clone()).ok());
        Arc::new(Self {
            data_dir,
            limits: bm_core::limits::LimitsCell::with_default(),
            password_hash: Mutex::new(hash),
            sessions: Mutex::new(HashSet::new()),
            login_gate: Mutex::new(HashMap::new()),
            oauth,
            oauth_states: Mutex::new(HashMap::new()),
        })
    }

    /// #47:OAuth 是否已配置(登录页据此显示 SSO 入口)。
    pub fn oauth_configured(&self) -> bool {
        self.oauth.is_some()
    }

    pub fn configured(&self) -> bool {
        self.password_hash.lock().expect("锁未中毒").is_some()
    }

    /// 该来源是否处于登录锁定中。
    fn login_locked(&self, key: &str) -> bool {
        let max_failures = self.limits.get().login_max_failures;
        self.login_gate
            .lock()
            .expect("锁未中毒")
            .get(key)
            .is_some_and(|(n, until)| {
                *n >= max_failures && until.is_some_and(|t| Instant::now() < t)
            })
    }

    fn note_login_failure(&self, key: &str) {
        let lim = self.limits.get();
        let max_failures = lim.login_max_failures;
        let lockout = Duration::from_secs(lim.login_lockout_secs);
        let mut gate = self.login_gate.lock().expect("锁未中毒");
        let e = gate.entry(key.to_string()).or_insert((0, None));
        e.0 = e.0.saturating_add(1);
        if e.0 >= max_failures {
            e.1 = Some(Instant::now() + lockout);
        }
        // 台账 GC:条目过多时清掉不在锁定期的旧项(防无界增长)
        if gate.len() > 1024 {
            gate.retain(|_, (n, until)| {
                *n < max_failures || until.is_some_and(|t| Instant::now() < t)
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
        if let Err(e) =
            bm_core::ports::persist::atomic_write(&cfg.join("portal.json"), text.as_bytes())
        {
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

/// PBKDF2-HMAC-SHA256(RustCrypto 实现,符合 RFC 2898;dkLen = 32 字节)。
fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iters: u32) -> [u8; 32] {
    let mut dk = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, iters.max(1), &mut dk);
    dk
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
        crate::auth::constant_time_eq(hex(&dk).as_bytes(), expect.as_bytes())
    } else {
        let (salt, expect) = stored.split_once('$').unwrap_or(("", ""));
        let computed = hash_password(password, salt);
        crate::auth::constant_time_eq(computed.as_bytes(), expect.as_bytes())
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

/// 仅校验门户会话 Cookie(不含 Bearer;Bearer 判定在 auth.rs 严格先行)。
pub(crate) fn cookie_authed(state: &crate::AppState, headers: &HeaderMap) -> bool {
    match cookie_session(headers) {
        Some(s) => state.portal.sessions.lock().expect("锁未中毒").contains(&s),
        None => false,
    }
}

fn authed(state: &crate::AppState, headers: &HeaderMap) -> bool {
    // P2(2026-09-07 架构评审):常数时间比较收口 auth.rs 单一实现。
    let bearer_ok = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|given| crate::auth::constant_time_eq(given.as_bytes(), state.token.as_bytes()))
        .unwrap_or(false);
    bearer_ok || cookie_authed(state, headers)
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
        || path == "/api/portal/bootstrap"
        // issue #47:OIDC 登录入口与回跳必须是免墙路径(否则未认证用户
        // 永远到不了 IdP;callback 自带 state 防伪,安全性不降)
        || path == "/api/portal/oauth/login"
        || path == "/api/portal/oauth/callback";
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

fn session_cookie(value: &str, max_age_secs: u64) -> String {
    format!("{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}")
}

/// 签发新会话并登记;返回 Set-Cookie 值(bootstrap/login/oauth 三处同款)。
fn issue_session(state: &crate::AppState) -> String {
    let session = new_session();
    state
        .portal
        .sessions
        .lock()
        .expect("锁未中毒")
        .insert(session.clone());
    session_cookie(
        &session,
        state.portal.limits.get().portal_cookie_max_age_secs,
    )
}

/// 限速门 key:对端 IP;测试 oneshot 等无 connect-info 场景退化为全局门。
fn gate_key_of(peer: Option<&axum::Extension<ConnectInfo<SocketAddr>>>) -> String {
    peer.map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "global".to_string())
}

/// 与本模块错误 JSON 形状一致的 401/429([`crate::webadmin::admin_error`] 同口径)。
fn unauthorized(msg: &str) -> Response {
    crate::webadmin::admin_error(StatusCode::UNAUTHORIZED, msg)
}

fn too_many(msg: &str) -> Response {
    crate::webadmin::admin_error(StatusCode::TOO_MANY_REQUESTS, msg)
}

/// GET /api/portal/state:登录页据此显示「创建访问密码」或「登录」。
pub async fn portal_state(State(state): State<crate::AppState>, headers: HeaderMap) -> Response {
    Json(json!({
        "configured": state.portal.configured(),
        "authed": authed(&state, &headers),
        // #47:登录页据此显示 SSO 入口
        "oauth": state.portal.oauth_configured(),
    }))
    .into_response()
}

/// POST /api/portal/bootstrap {password}:仅未设密码时可用一次;设置并登录。
/// P1-3(2026-09-07 架构评审):补登录同款限速门——未配置密码的窗口期内,
/// 抢注尝试按对端 IP 计入 login_gate,失败过多即锁。
pub async fn portal_bootstrap(
    State(state): State<crate::AppState>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let gate_key = gate_key_of(peer.as_ref());
    if state.portal.login_locked(&gate_key) {
        return too_many("尝试次数过多,请稍后再试");
    }
    if state.portal.configured() {
        return crate::webadmin::conflict("访问密码已设置,请直接登录");
    }
    let pw = body["password"].as_str().unwrap_or_default();
    if pw.chars().count() < 6 {
        state.portal.note_login_failure(&gate_key);
        return crate::webadmin::bad_request("密码至少 6 位");
    }
    state.portal.save(&store_password(pw));
    state.portal.note_login_success(&gate_key);
    (
        [(header::SET_COOKIE, issue_session(&state))],
        Json(json!({"ok": true})),
    )
        .into_response()
}

/// POST /api/portal/login {password}。
/// 2026-09-05 回看加固:失败限速(ADR-0009 决策 4)+ PBKDF2 透明升级。
pub async fn portal_login(
    State(state): State<crate::AppState>,
    peer: Option<axum::Extension<ConnectInfo<SocketAddr>>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let gate_key = gate_key_of(peer.as_ref());
    if state.portal.login_locked(&gate_key) {
        return too_many("登录失败次数过多,请 15 分钟后再试");
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
    (
        [(header::SET_COOKIE, issue_session(&state))],
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
        return crate::webadmin::bad_request("新密码至少 6 位");
    }
    state.portal.save(&store_password(pw));
    state.portal.sessions.lock().expect("锁未中毒").clear();
    Json(json!({"ok": true, "note": "密码已更新,请重新登录"})).into_response()
}

/// GET /login:登录页(web_dir 下 login.html)。
/// base64url 解码(JWT payload 提取用;无填充,容忍标准字母表变体)。
fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let s = s.trim_end_matches('=');
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(s))
        .ok()
}

/// #47:GET /api/portal/oauth/login——302 到 IdP 授权端点(response_type=code
/// + state 防伪)。未配置 oauth = 404。
pub async fn portal_oauth_login(
    State(state): State<crate::AppState>,
    headers: HeaderMap,
) -> Response {
    let Some(cfg) = state.portal.oauth.clone() else {
        return unauthorized("OAuth 未配置");
    };
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1");
    let redirect_uri = cfg
        .redirect_uri
        .clone()
        .unwrap_or_else(|| format!("http://{host}/api/portal/oauth/callback"));
    let st = new_session();
    {
        let mut states = state.portal.oauth_states.lock().expect("锁未中毒");
        states.retain(|_, t| t.elapsed() < OAUTH_STATE_TTL);
        states.insert(st.clone(), Instant::now());
    }
    let loc = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
        cfg.authorization_endpoint,
        crate::percent_encode(&cfg.client_id),
        crate::percent_encode(&redirect_uri),
        crate::percent_encode(&cfg.scopes.join(" ")),
        crate::percent_encode(&st),
    );
    ([(header::LOCATION, loc)], StatusCode::FOUND).into_response()
}

/// #47:GET /api/portal/oauth/callback?code=&state=——code 换 token(背通道),
/// 校验 id_token(iss/aud/exp)后签发 boen_session 并回首页。
pub async fn portal_oauth_callback(
    State(state): State<crate::AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(cfg) = state.portal.oauth.clone() else {
        return unauthorized("OAuth 未配置");
    };
    let Some(code) = q.get("code").cloned() else {
        return unauthorized("回调缺 code");
    };
    // state 一次性校验:取出即删(TTL 外/不存在 = 拒)
    let st_ok = q
        .get("state")
        .and_then(|st| {
            state
                .portal
                .oauth_states
                .lock()
                .expect("锁未中毒")
                .remove(st)
        })
        .is_some_and(|t| t.elapsed() < OAUTH_STATE_TTL);
    if !st_ok {
        return unauthorized("OAuth state 无效或已过期");
    }

    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("127.0.0.1");
    let redirect_uri = cfg
        .redirect_uri
        .clone()
        .unwrap_or_else(|| format!("http://{host}/api/portal/oauth/callback"));
    // 背通道 code 换 token(form 形态,兼容面最广)
    let token_resp = match reqwest::Client::new()
        .post(&cfg.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
        ])
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return unauthorized(&format!("token 端点请求失败: {e}")),
    };
    if !token_resp.status().is_success() {
        return unauthorized("token 端点返回非 2xx");
    }
    let tv: serde_json::Value = match token_resp.json().await {
        Ok(v) => v,
        Err(_) => return unauthorized("token 响应解析失败"),
    };
    let Some(id_token) = tv["id_token"].as_str() else {
        return unauthorized("token 响应缺 id_token");
    };
    // JWT payload 校验:iss/aud/exp(TLS 背通道直取,id_token 签名校验按
    // OIDC 规格在此形态下可省;本地生态不引 JWKS 依赖)
    let parts: Vec<&str> = id_token.split('.').collect();
    let claims = parts
        .get(1)
        .and_then(|pl| b64url_decode(pl))
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let Some(claims) = claims else {
        return unauthorized("id_token 解析失败");
    };
    if let Some(want) = cfg.issuer.as_ref()
        && claims["iss"].as_str() != Some(want.as_str())
    {
        return unauthorized("id_token iss 不匹配");
    }
    let aud_ok = match &claims["aud"] {
        serde_json::Value::String(s) => s == &cfg.client_id,
        serde_json::Value::Array(a) => a.iter().any(|x| x.as_str() == Some(cfg.client_id.as_str())),
        _ => false,
    };
    if !aud_ok {
        return unauthorized("id_token aud 不匹配");
    }
    let now_secs = crate::unix_now() as i64;
    if let Some(exp) = claims["exp"].as_i64()
        && now_secs > exp + 60
    {
        return unauthorized("id_token 已过期");
    }

    let session_cookie = issue_session(&state);
    (
        [
            (header::SET_COOKIE, session_cookie),
            (header::LOCATION, "/".to_string()),
        ],
        StatusCode::FOUND,
    )
        .into_response()
}

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
        for _ in 0..bm_core::limits::Limits::default().login_max_failures {
            auth.note_login_failure(key);
        }
        assert!(auth.login_locked(key), "达上限即锁定");
        // 其他来源不受影响
        assert!(!auth.login_locked("5.6.7.8"));
        auth.note_login_success(key);
        assert!(!auth.login_locked(key), "成功登录清零");
    }
}
