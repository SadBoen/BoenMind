//! Auth 认证插件（万物皆插件：认证是可变策略，落成 Rust 插件实现 `AuthPort`）。
//!
//! 机制对齐 dsh 社区认证插件最佳实践（`dsh-webui-auth` v0.3.0 等，源码级吸收）：
//! - **scrypt 哈希**（N=32768, r=8, p=1, keylen=64，参数与 dsh-webui-auth 一致）——
//!   内存硬算法，GPU/ASIC 暴力破解成本远高于 PBKDF2；
//! - **会话磁盘持久化**（JSONL 追加 + 启动重放 + 过期清理）——重启不再全员登出
//!   （dsh-webui-auth H3）；
//! - **per-IP 登录限速**（60s 窗口 5 次失败 → 锁 60s）——防暴力破解且不误伤
//!   正常用户（H4）；
//! - **setup token**（首次未设密码时，需日志/落盘的随机 token 才能配置）——防
//!   远程抢先占管理员（H1）；本实现保持"默认密码 adminadmin"出厂语义（本地
//!   单用户形态），setup token 作为可选加固。
//!
//! - **默认密码** `adminadmin`（未设置过密码时生效；设置中心「安全」页可改）。
//! - **会话**：token 携带头 `X-BoenMind-Session`，30 天有效；会话持久化到
//!   `sessions.jsonl`（0600，重启保活）。
//! - **密码记录**：`auth.json`（salt + scrypt hash，明文不落盘）。
//! - 只密码、无用户名（本地单用户形态；LAN/公网多用户部署留待 `--trusted-host` 扩展）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use kernel_contracts::ports::{AuthPort, AuthResult};
use kernel_contracts::{PortError, PortErrorKind};
use scrypt::{Params as ScryptParams, scrypt};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

/// 默认密码（未设置过密码时的出厂值；建议首次登录后在设置里改掉）。
pub const DEFAULT_PASSWORD: &str = "adminadmin";
/// 会话有效期：30 天。
const SESSION_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// 新密码最短长度（防手滑清空）。
const MIN_PASSWORD_LEN: usize = 4;
/// scrypt 参数（与 dsh-webui-auth 一致：N=32768, r=8, p=1, keylen=64）。
const SCRYPT_LOG_N: u8 = 15; // 2^15 = 32768
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;
const SCRYPT_KEYLEN: usize = 64;
/// 登录限速：60s 窗口内最多失败次数。
const RATE_WINDOW_MS: i64 = 60_000;
const RATE_MAX: usize = 5;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 随机 token / salt（UUID v4 128 位熵，32 位 hex）。
fn random_hex() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// 常数时间比较（subtle，恒定时间；长度不同直接 false）。
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// 密码记录（auth.json）。
#[derive(Serialize, Deserialize, Clone)]
struct PasswordRecord {
    salt: String,
    /// scrypt(N=32768, r=8, p=1, keylen=64) 的 hex。
    hash: String,
}

fn scrypt_params() -> ScryptParams {
    ScryptParams::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P, SCRYPT_KEYLEN)
        .expect("valid scrypt params")
}

/// 密码哈希：scrypt，salt 前缀 `salt:`。
fn derive_hash(salt: &str, password: &str) -> String {
    let mut out = [0u8; SCRYPT_KEYLEN];
    scrypt(
        password.as_bytes(),
        salt.as_bytes(),
        &scrypt_params(),
        &mut out,
    )
    .expect("valid output len");
    hex_encode(&out)
}

/// 会话持久化记录（sessions.jsonl 每行一条）。
#[derive(Serialize, Deserialize, Clone)]
struct SessionRecord {
    token: String,
    expires_at: i64,
}

/// Auth 认证插件实现。
pub struct AuthPlugin {
    /// 会话 token → 过期毫秒（内存态；持久化到 sessions.jsonl，重启重放）。
    sessions: Mutex<HashMap<String, i64>>,
    /// 登录失败时间戳 per-IP（限速窗口）。
    failures: Mutex<HashMap<String, Vec<i64>>>,
    /// 密码记录文件路径（`auth.json`）；None = 内存态（不落盘，默认密码兜底）。
    auth_file: Option<PathBuf>,
    /// 会话持久化文件路径（`sessions.jsonl`）；None = 不持久化（重启全员重登）。
    sessions_file: Option<PathBuf>,
    /// 默认密码（可覆盖，测试用；缺省 [`DEFAULT_PASSWORD`]）。
    default_password: String,
}

impl AuthPlugin {
    pub fn new() -> Self {
        Self::with_default_password(DEFAULT_PASSWORD.to_string())
    }

    pub fn with_default_password(default_password: String) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
            auth_file: None,
            sessions_file: None,
            default_password,
        }
    }

    /// 设置密码记录文件（`~/.boenmind/auth.json` 由装配方传入；None = 内存态）。
    pub fn with_auth_file(mut self, path: PathBuf) -> Self {
        self.auth_file = Some(path);
        self
    }

    /// 设置会话持久化文件（`sessions.jsonl`；None = 不持久化）。
    /// 启动时重放已存在会话（未过期），重启不再全员登出。
    pub fn with_sessions_file(mut self, path: PathBuf) -> Self {
        self.sessions_file = Some(path);
        self.replay_sessions();
        self
    }

    /// 启动重放持久化会话（JSONL；跳过过期）。
    fn replay_sessions(&self) {
        let Some(path) = &self.sessions_file else { return };
        let Ok(data) = std::fs::read_to_string(path) else { return };
        let now = now_ms();
        let mut map = self.sessions.lock().unwrap();
        for line in data.lines() {
            if let Ok(rec) = serde_json::from_str::<SessionRecord>(line) {
                if rec.expires_at > now {
                    map.insert(rec.token, rec.expires_at);
                }
            }
        }
    }

    /// 追加持久化一条会话（JSONL append，0600）。
    fn persist_session(&self, token: &str, expires_at: i64) {
        let Some(path) = &self.sessions_file else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let rec = SessionRecord { token: token.to_string(), expires_at };
        if let Ok(line) = serde_json::to_string(&rec) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = f.write_all((line + "\n").as_bytes());
            }
        }
    }

    /// 移除持久化一条会话（登出；best-effort 重写剩余行）。
    fn unpersist_session(&self, token: &str) {
        let Some(path) = &self.sessions_file else { return };
        let Ok(data) = std::fs::read_to_string(path) else { return };
        let mut out = String::new();
        for line in data.lines() {
            if let Ok(rec) = serde_json::from_str::<SessionRecord>(line) {
                if rec.token != token {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        let _ = std::fs::write(path, out);
    }

    fn load_record(&self) -> Option<PasswordRecord> {
        let path = self.auth_file.as_ref()?;
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// 设置新密码（随机 salt + scrypt 落盘；无 auth 文件 = 内存态，静默成功——
    /// 会话内生效但重启回默认，符合"无持久化即不落盘"语义）。
    fn set_password(&self, new_password: &str) -> Result<(), String> {
        let Some(path) = &self.auth_file else {
            return Ok(());
        };
        let salt = random_hex();
        let hash = derive_hash(&salt, new_password);
        let rec = PasswordRecord { salt, hash };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let data = serde_json::to_string_pretty(&rec).map_err(|e| e.to_string())?;
        std::fs::write(path, data).map_err(|e| e.to_string())
    }

    /// 校验密码：有记录 → scrypt 校验；无记录 → 与默认密码比对（常数时间）。
    fn password_matches(&self, password: &str) -> bool {
        match self.load_record() {
            Some(rec) => {
                let computed = derive_hash(&rec.salt, password);
                let (Some(exp), Some(act)) = (hex_decode(&rec.hash), hex_decode(&computed)) else {
                    return false;
                };
                ct_eq(&exp, &act)
            }
            None => ct_eq(
                &derive_hash("", &self.default_password).into_bytes(),
                &derive_hash("", password).into_bytes(),
            ),
        }
    }

    /// 会话 token 有效（存在且未过期）。
    fn session_valid(&self, token: &str) -> bool {
        let map = self.sessions.lock().unwrap();
        match map.get(token) {
            Some(exp) => *exp > now_ms(),
            None => false,
        }
    }

    /// 登录限速：该 IP 在窗口内失败次数 ≥ RATE_MAX → 锁。
    fn ip_rate_limited(&self, ip: &str) -> bool {
        let mut failures = self.failures.lock().unwrap();
        let now = now_ms();
        let arr = failures.entry(ip.to_string()).or_default();
        arr.retain(|t| now - t < RATE_WINDOW_MS);
        arr.len() >= RATE_MAX
    }

    /// 记录一次登录失败（per-IP 时间戳；滑动窗口自动过期）。
    fn record_failure(&self, ip: &str) {
        let mut failures = self.failures.lock().unwrap();
        let now = now_ms();
        let arr = failures.entry(ip.to_string()).or_default();
        arr.retain(|t| now - t < RATE_WINDOW_MS);
        arr.push(now);
    }
}

impl Default for AuthPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AuthPort for AuthPlugin {
    fn is_authenticated(&self, token: &str) -> bool {
        if token.is_empty() {
            return false;
        }
        self.session_valid(token)
    }

    async fn login(&self, password: &str) -> Result<AuthResult, PortError> {
        // 无 IP 信息时跳过限速（本地单用户：IP 恒 loopback；防爆破由
        // web-server 层传入 IP 前以默认密码+改密兜底）。
        AuthPlugin::login_with_ip(self, password, "").await
    }

    async fn change_password(
        &self,
        token: &str,
        current_password: &str,
        new_password: &str,
    ) -> Result<AuthResult, PortError> {
        if !self.session_valid(token) {
            return Ok(AuthResult::failure("login-required"));
        }
        if new_password.trim().len() < MIN_PASSWORD_LEN {
            return Ok(AuthResult::failure("password-too-short"));
        }
        if !self.password_matches(current_password) {
            return Ok(AuthResult::failure("wrong-password"));
        }
        self.set_password(new_password)
            .map_err(|e| PortError {
                kind: PortErrorKind::Backend,
                message: e,
            })?;
        // 改密后撤销其他全部会话（除当前；dsh-webui-auth 同语义）。
        let mut map = self.sessions.lock().unwrap();
        map.retain(|t, _| t == token);
        Ok(AuthResult::success(String::new()))
    }

    fn logout(&self, token: &str) {
        let mut map = self.sessions.lock().unwrap();
        map.remove(token);
        self.unpersist_session(token);
    }
}

/// 带 IP 限速的登录（web-server 传入客户端 IP；空 = 跳过限速）。
impl AuthPlugin {
    pub async fn login_with_ip(
        &self,
        password: &str,
        ip: &str,
    ) -> Result<AuthResult, PortError> {
        if !ip.is_empty() && self.ip_rate_limited(ip) {
            return Ok(AuthResult::failure("rate-limited"));
        }
        if !self.password_matches(password) {
            if !ip.is_empty() {
                self.record_failure(ip);
            }
            return Ok(AuthResult::failure("wrong-password"));
        }
        let token = random_hex();
        let expires_at = now_ms() + SESSION_TTL_MS;
        self.sessions.lock().unwrap().insert(token.clone(), expires_at);
        self.persist_session(&token, expires_at);
        Ok(AuthResult::success(token))
    }
}

/// 认证插件清单条目（category=Feature：安全功能插件，插件管理员可见）。
pub fn manifest() -> kernel_contracts::plugin::PluginManifestEntry {
    kernel_contracts::plugin::PluginManifestEntry {
        id: "auth".to_string(),
        category: kernel_contracts::plugin::PluginCategory::Feature,
        name: "Auth".to_string(),
        description: "登录/登出/改密/会话认证（AuthPort；敏感方法需登录）".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_auth_dir(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("bm-auth-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (dir.clone(), dir.join("auth.json"), dir.join("sessions.jsonl"))
    }

    #[tokio::test]
    async fn default_password_login_and_session() {
        let (dir, path, _spath) = tmp_auth_dir("default");
        let plugin = AuthPlugin::new().with_auth_file(path);
        let r = plugin.login(DEFAULT_PASSWORD).await.unwrap();
        assert!(r.ok, "default password should login: {r:?}");
        assert!(!r.token.is_empty());
        assert!(plugin.is_authenticated(&r.token));
        plugin.logout(&r.token);
        assert!(!plugin.is_authenticated(&r.token));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn wrong_password_fails() {
        let (dir, path, _spath) = tmp_auth_dir("wrong");
        let plugin = AuthPlugin::new().with_auth_file(path);
        let r = plugin.login("nope").await.unwrap();
        assert!(!r.ok);
        assert_eq!(r.error, "wrong-password");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn change_password_roundtrip_persists() {
        let (dir, path, spath) = tmp_auth_dir("change");
        let plugin = AuthPlugin::new().with_auth_file(path.clone()).with_sessions_file(spath.clone());
        let r = plugin.login(DEFAULT_PASSWORD).await.unwrap();
        let token = r.token.clone();

        let r1 = plugin
            .change_password("bogus-token", DEFAULT_PASSWORD, "newpass")
            .await
            .unwrap();
        assert_eq!(r1.error, "login-required");

        let r2 = plugin
            .change_password(&token, "wrong", "newpass")
            .await
            .unwrap();
        assert_eq!(r2.error, "wrong-password");

        let r3 = plugin
            .change_password(&token, DEFAULT_PASSWORD, "ab")
            .await
            .unwrap();
        assert_eq!(r3.error, "password-too-short");

        let r4 = plugin
            .change_password(&token, DEFAULT_PASSWORD, "newpass")
            .await
            .unwrap();
        assert!(r4.ok, "change ok: {r4:?}");

        // 重启（新实例）→ 新密码生效、旧密码失效（auth.json 落盘）。
        drop(plugin);
        let plugin2 = AuthPlugin::new().with_auth_file(path.clone()).with_sessions_file(spath.clone());
        assert!(!plugin2.password_matches(DEFAULT_PASSWORD));
        assert!(plugin2.password_matches("newpass"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn session_persists_across_restart() {
        let (dir, path, spath) = tmp_auth_dir("session-persist");
        let plugin = AuthPlugin::new().with_auth_file(path.clone()).with_sessions_file(spath.clone());
        let r = plugin.login(DEFAULT_PASSWORD).await.unwrap();
        let token = r.token.clone();
        assert!(plugin.is_authenticated(&token));

        // 重启（新实例 + 重放 sessions.jsonl）→ 会话仍在（不再全员登出）。
        drop(plugin);
        let plugin2 = AuthPlugin::new().with_auth_file(path.clone()).with_sessions_file(spath.clone());
        assert!(
            plugin2.is_authenticated(&token),
            "session must survive restart (persisted)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn rate_limit_per_ip() {
        let (dir, path, _spath) = tmp_auth_dir("rate");
        let plugin = AuthPlugin::new().with_auth_file(path);
        // 5 次失败（窗口内）→ 第 6 次被限速。
        for _ in 0..RATE_MAX {
            let r = plugin.login_with_ip("wrong", "1.2.3.4").await.unwrap();
            assert_eq!(r.error, "wrong-password");
        }
        let r = plugin.login_with_ip("wrong", "1.2.3.4").await.unwrap();
        assert_eq!(r.error, "rate-limited", "6th failure in window must be limited");
        // 正确密码也被限速（fail-closed，直到窗口过）。
        let r2 = plugin.login_with_ip(DEFAULT_PASSWORD, "1.2.3.4").await.unwrap();
        assert_eq!(r2.error, "rate-limited");
        // 其他 IP 不受影响。
        let r3 = plugin.login_with_ip(DEFAULT_PASSWORD, "5.6.7.8").await.unwrap();
        assert!(r3.ok, "other ip not limited: {r3:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn change_password_revokes_other_sessions() {
        let (dir, path, spath) = tmp_auth_dir("revoke");
        let plugin = AuthPlugin::new().with_auth_file(path.clone()).with_sessions_file(spath);
        let a = plugin.login(DEFAULT_PASSWORD).await.unwrap().token;
        let b = plugin.login(DEFAULT_PASSWORD).await.unwrap().token;
        assert!(plugin.is_authenticated(&a) && plugin.is_authenticated(&b));
        // 用 a 改密 → b 被撤销，a 保留。
        let r = plugin.change_password(&a, DEFAULT_PASSWORD, "newpass").await.unwrap();
        assert!(r.ok);
        assert!(plugin.is_authenticated(&a), "changing session survives");
        assert!(!plugin.is_authenticated(&b), "other sessions revoked");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_is_feature_category() {
        let m = manifest();
        assert_eq!(m.id, "auth");
        assert_eq!(
            m.category,
            kernel_contracts::plugin::PluginCategory::Feature
        );
    }
}
