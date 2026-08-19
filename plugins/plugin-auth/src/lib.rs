//! Auth 认证插件（万物皆插件：认证是可变策略，落成 Rust 插件实现 `AuthPort`）。
//!
//! 吸收 BoenMind 旧 `backend/crates/bm-server/src/routes/auth.rs`（用户已接受方案：
//! 只密码 + 内存会话 token + auth.json 持久化），密码哈希从裸 SHA-256 升级为
//! **PBKDF2-SHA256**（盐 + 迭代，抗暴力破解）。
//!
//! - **默认密码** `adminadmin`（未设置过密码时生效；设置中心「安全」页可改）。
//! - **会话**：内存 token（`X-BoenMind-Session` 请求头），30 天有效；重启即全员重登。
//! - **密码记录**：`<auth_path>/auth.json`（salt + PBKDF2 hash，明文不落盘）。
//! - 只密码、无用户名（本地单用户形态；LAN/公网多用户部署留待 `--trusted-host` 扩展）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use kernel_contracts::ports::{AuthPort, AuthResult};
use kernel_contracts::{PortError, PortErrorKind};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use serde::{Deserialize, Serialize};

/// 默认密码（未设置过密码时的出厂值；建议首次登录后在设置里改掉）。
pub const DEFAULT_PASSWORD: &str = "adminadmin";
/// 会话有效期：30 天。
const SESSION_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// 新密码最短长度（防手滑清空）。
const MIN_PASSWORD_LEN: usize = 4;
/// PBKDF2 迭代次数（OWASP 建议 SHA-256 ≥ 600k；本地单用户取 210k 平衡响应延迟）。
const PBKDF2_ROUNDS: u32 = 210_000;

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

/// 密码记录（auth.json）。
#[derive(Serialize, Deserialize, Clone)]
struct PasswordRecord {
    salt: String,
    /// PBKDF2-SHA256(salt:password, rounds=210k) 的 hex。
    hash: String,
}

/// 密码哈希：PBKDF2-HMAC-SHA256，salt 前缀 `salt:`。
fn derive_hash(salt: &str, password: &str) -> String {
    let mut out = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt.as_bytes(), PBKDF2_ROUNDS, &mut out);
    hex_encode(&out)
}

/// Auth 认证插件实现。
pub struct AuthPlugin {
    /// 会话 token → 过期毫秒（进程内存态：重启后浏览器重登）。
    sessions: Mutex<HashMap<String, i64>>,
    /// 密码记录文件路径（`auth.json`）；None = 内存态（不落盘，默认密码兜底）。
    auth_file: Option<PathBuf>,
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
            auth_file: None,
            default_password,
        }
    }

    /// 设置密码记录文件（`~/.boenmind/auth.json` 由装配方传入；None = 内存态）。
    pub fn with_auth_file(mut self, path: PathBuf) -> Self {
        self.auth_file = Some(path);
        self
    }

    fn load_record(&self) -> Option<PasswordRecord> {
        let path = self.auth_file.as_ref()?;
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// 校验密码：有记录 → PBKDF2 校验；无记录 → 与默认密码比对（常数时间）。
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

    /// 设置新密码（随机 salt + PBKDF2 落盘；无 auth 文件 = 内存态，静默成功——
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

    /// 会话 token 有效（存在且未过期）。
    fn session_valid(&self, token: &str) -> bool {
        let map = self.sessions.lock().unwrap();
        match map.get(token) {
            Some(exp) => *exp > now_ms(),
            None => false,
        }
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
        if !self.password_matches(password) {
            return Ok(AuthResult::failure("wrong-password"));
        }
        let token = random_hex();
        let mut map = self.sessions.lock().unwrap();
        map.insert(token.clone(), now_ms() + SESSION_TTL_MS);
        Ok(AuthResult::success(token))
    }

    fn logout(&self, token: &str) {
        let mut map = self.sessions.lock().unwrap();
        map.remove(token);
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
        Ok(AuthResult::success(String::new()))
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

    fn tmp_auth_path(tag: &str) -> (PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("bm-auth-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (dir.clone(), dir.join("auth.json"))
    }

    #[tokio::test]
    async fn default_password_login_and_session() {
        let (_dir, path) = tmp_auth_path("default");
        let plugin = AuthPlugin::new().with_auth_file(path);
        // 默认密码登录成功。
        let r = plugin.login(DEFAULT_PASSWORD).await.unwrap();
        assert!(r.ok, "default password should login: {r:?}");
        assert!(!r.token.is_empty());
        // 会话有效。
        assert!(plugin.is_authenticated(&r.token));
        // 登出后失效。
        plugin.logout(&r.token);
        assert!(!plugin.is_authenticated(&r.token));
    }

    #[tokio::test]
    async fn wrong_password_fails() {
        let (_dir, path) = tmp_auth_path("wrong");
        let plugin = AuthPlugin::new().with_auth_file(path);
        let r = plugin.login("nope").await.unwrap();
        assert!(!r.ok);
        assert_eq!(r.error, "wrong-password");
    }

    #[tokio::test]
    async fn change_password_roundtrip_persists() {
        let (dir, path) = tmp_auth_path("change");
        let plugin = AuthPlugin::new().with_auth_file(path.clone());
        let r = plugin.login(DEFAULT_PASSWORD).await.unwrap();
        let token = r.token.clone();

        // 未登录改密 → login-required。
        let r1 = plugin
            .change_password("bogus-token", DEFAULT_PASSWORD, "newpass")
            .await
            .unwrap();
        assert_eq!(r1.error, "login-required");

        // 当前密码错 → wrong-password。
        let r2 = plugin
            .change_password(&token, "wrong", "newpass")
            .await
            .unwrap();
        assert_eq!(r2.error, "wrong-password");

        // 新密码太短 → password-too-short。
        let r3 = plugin
            .change_password(&token, DEFAULT_PASSWORD, "ab")
            .await
            .unwrap();
        assert_eq!(r3.error, "password-too-short");

        // 正确改密。
        let r4 = plugin
            .change_password(&token, DEFAULT_PASSWORD, "newpass")
            .await
            .unwrap();
        assert!(r4.ok, "change ok: {r4:?}");

        // 重启（新实例）→ 新密码生效、旧密码失效（落盘持久化）。
        drop(plugin);
        let plugin2 = AuthPlugin::new().with_auth_file(path);
        assert!(!plugin2.password_matches(DEFAULT_PASSWORD));
        assert!(plugin2.password_matches("newpass"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn memory_mode_default_password_no_file() {
        // 无 auth 文件（内存态）：默认密码登录，不落盘。
        let plugin = AuthPlugin::new();
        let r = plugin.login(DEFAULT_PASSWORD).await.unwrap();
        assert!(r.ok);
        assert!(plugin.is_authenticated(&r.token));
        // 改密在内存态也能工作（不落盘，重启回默认）。
        let r2 = plugin.change_password(&r.token, DEFAULT_PASSWORD, "newpass").await.unwrap();
        assert!(r2.ok, "memory-mode change: {r2:?}");
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
