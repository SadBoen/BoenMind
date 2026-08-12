//! 自更新（热升级）：检查 GitHub Releases → 下载 → 验签 → 替换 → 秒级重启。
//!
//! 服务器版整个应用 = 单个 bm-server 二进制（`--features embed` 内嵌前端与
//! 插件），因此"更新" = 替换这一个程序文件。运行模式由环境变量区分：
//! - **standalone**（默认，Linux 裸进程/systemd 部署）：原子替换自身后
//!   `exec` 重启——PID 不变，systemd `Restart=always` 无感知，连接只断一两秒
//! - **managed**（桌面壳 spawn 的子进程，`BOENMIND_MANAGED=1`）：新二进制落盘
//!   `~/.boenmind/runtime/`（按版本号命名不覆盖，天然回滚），由壳换上新版重启，
//!   应用窗口全程不关
//!
//! 下载产物一律校验 ed25519 签名（Blake2b-512 digest，与发布用的
//! tauri signer 同一链路），防 GitHub 侧被劫持或中间人注入恶意二进制。

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::app_dir;
use crate::error::AppError;
use crate::http_util::http_agent;

/// 发布仓库（检查更新与资产下载来源）
pub const UPDATE_REPO: &str = "SadBoen/BoenMind";
/// GitHub Releases API 端点
const RELEASES_API: &str = "https://api.github.com/repos/SadBoen/BoenMind/releases/latest";
/// 资产下载域名白名单（防 SSRF：只允许 GitHub 官方资产域名）
const ASSET_HOST_ALLOWLIST: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// 运行时二进制签名公钥（minisign 格式 base64）。
/// 与 `frontend/src-tauri/tauri.conf.json` `plugins.updater.pubkey` 同一把密钥，
/// 硬编码于此（公钥本身公开，无需配置文件）。
pub const UPDATE_PUBKEY_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDgzMzU0M0YyRkRDOUZBQUEKUldTcStzbjk4a00xZzBwa3JlaXJIaEZxU2ZGSmtuSmJ0ZURHWkxKZldxY1VIVmVTN0VBdi9yeU0K";

/// 待重启标记文件名（standalone：apply 替换自身后写，若进程未及重启
/// （崩溃/断电/手动关闭），下次启动检测到即自动完成升级）
pub const UPDATE_PENDING_FILE: &str = ".update-pending.json";

/// 检查更新结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateCheck {
    pub current: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<LatestRelease>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LatestRelease {
    pub version: String,
    pub notes: String,
    pub asset: ReleaseAsset,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// 签名资产下载地址（同一 Release 内 `<name>.sig`）
    pub sig_url: String,
}

/// 应用更新结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApplyOutcome {
    pub version: String,
    /// "managed"（桌面壳子进程：落盘 runtime 目录，由壳重启）| "standalone"
    /// （Linux 部署：已原子替换自身并写 pending 标记，调 restart 即 exec）
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// 是否由桌面壳托管（managed 模式）：壳 spawn 的子进程带 `BOENMIND_MANAGED=1`。
/// standalone（Linux 部署）时该变量不存在 → 升级直接替换自身并 exec。
pub fn is_managed() -> bool {
    std::env::var("BOENMIND_MANAGED").is_ok_and(|v| v == "1")
}

/// runtime 目录（managed 模式新二进制落盘处；standalone 模式旧版备份也放这）
pub fn runtime_dir() -> std::path::PathBuf {
    app_dir().join("runtime")
}

/// 当前运行版本（发布号，如 "0.1.1"）
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 目标平台资产名（发布命名约定：`boenmind-runtime-<ver>-<triple>[.exe]`）。
/// 与 release.yml 的产物命名保持同步，改动两侧需一致。
pub fn target_asset_name(version: &str) -> String {
    let triple = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        // 未知平台兜底（测试/新平台）
        (os, arch) => return format!("boenmind-runtime-{version}-{os}-{arch}"),
    };
    let name = format!("boenmind-runtime-{version}-{triple}");
    if std::env::consts::OS == "windows" {
        format!("{name}.exe")
    } else {
        name
    }
}

/// 语义版本比较（容忍 `v` 前缀与 `-` 分隔，如 "v0.2.0" / "0.2.0-beta.1"）。
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parse = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split(['.', '-'])
            .filter_map(|s| s.parse::<u64>().ok())
            .collect()
    };
    let (va, vb) = (parse(a), parse(b));
    for (x, y) in va.iter().zip(vb.iter()) {
        match x.cmp(y) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    va.len().cmp(&vb.len())
}

/// 检查更新（用户手动触发）：查 GitHub Releases 最新版，选当前平台的资产。
/// GitHub 未认证限流 60 req/h，手动触发频率无压力；不做任何自动检查。
pub fn check_update() -> Result<UpdateCheck, AppError> {
    let current = current_version().to_string();
    let resp = http_agent()
        .get(RELEASES_API)
        .header("User-Agent", "BoenMind")
        .call()
        .map_err(|e| AppError::upstream(format!("检查更新失败: {e}")))?;
    let code = resp.status().as_u16();
    if code != 200 {
        return Err(AppError::upstream(format!("检查更新失败: GitHub HTTP {code}")));
    }
    let body = resp
        .into_body()
        .into_with_config()
        .limit(2 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| AppError::upstream(format!("读取 GitHub 响应失败: {e}")))?;
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| AppError::upstream(format!("解析 GitHub 响应失败: {e}")))?;

    let version = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim_start_matches('v')
        .to_string();
    if version.is_empty() {
        return Err(AppError::upstream("GitHub 响应缺少版本号"));
    }
    // 与当前版本比较：不新于当前 → 无更新
    if compare_versions(&version, &current) != Ordering::Greater {
        return Ok(UpdateCheck { current, latest: None });
    }

    let wanted = target_asset_name(&version);
    let assets = json.get("assets").and_then(|a| a.as_array());
    let asset = assets
        .and_then(|list| list.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(wanted.as_str())))
        .ok_or_else(|| {
            AppError::upstream(format!(
                "新版 v{version} 未发布 {wanted} 资产（发布不完整，请稍后再试）"
            ))
        })?;
    let url = asset
        .get("browser_download_url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| AppError::upstream(format!("资产 {wanted} 缺少下载地址")))?;
    if !asset_url_allowed(url) {
        return Err(AppError::upstream(format!("资产 {wanted} 下载地址不在白名单内")));
    }
    let sig_url = assets
        .and_then(|list| list.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(&format!("{wanted}.sig"))))
        .and_then(|a| a.get("browser_download_url").and_then(|u| u.as_str()))
        .ok_or_else(|| AppError::upstream(format!("新版 v{version} 未发布 {wanted}.sig 签名资产")))?
        .to_string();
    let notes = json.get("body").and_then(|b| b.as_str()).unwrap_or_default().to_string();
    Ok(UpdateCheck {
        current,
        latest: Some(LatestRelease {
            version,
            notes,
            asset: ReleaseAsset {
                name: wanted,
                url: url.to_string(),
                size: asset.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                sig_url,
            },
        }),
    })
}

/// 资产下载 URL 白名单（SSRF 防护：只允许 GitHub 官方资产域名）
fn asset_url_allowed(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    matches!(parsed.scheme(), "https") && parsed.host_str().is_some_and(|h| ASSET_HOST_ALLOWLIST.contains(&h))
}

/// 验签：minisign 格式签名文件 + 内嵌公钥验证文件内容。
///
/// tauri signer 的 .sig 文件有两种等价形态（都接受）：
/// - **单行 base64 包装**（tauri 默认产物）：整体 base64，解码后为多行文本
/// - **明文多行文本**（minisign 标准）：`untrusted comment:` 行 + base64 签名体
///
/// 解码后的文本格式：
/// ```text
/// untrusted comment: <任意文本>
/// <base64(8B key_id + 64B ed25519 签名)>
/// trusted comment: <任意文本>       ← 可选，忽略
/// <base64(...)>                      ← 可选，忽略
/// ```
/// 签名消息 = 文件内容的 Blake2b-512 digest（tauri 自定义 prehash，
/// 即 `crypto.verify(null, blake2b512(file), pk, sig)`）。
pub fn verify_signature(file: &Path, sig_text: &str) -> Result<(), AppError> {
    let pubkey_raw = decode_pubkey()?;
    verify_with_pubkey(&pubkey_raw, file, sig_text)
}

/// 验签核心（公钥注入版，供测试与生产共用）：
/// `pubkey_raw` = [2B 算法标识][8B key_id][32B ed25519 pk]
fn verify_with_pubkey(pubkey_raw: &[u8], file: &Path, sig_text: &str) -> Result<(), AppError> {
    use base64::Engine as _;
    use ed25519_dalek::Verifier as _;

    if pubkey_raw.len() != 42 {
        return Err(AppError::internal("公钥长度非法（应为 42 字节）"));
    }
    let expected_key_id = &pubkey_raw[2..10];
    let pk_bytes: [u8; 32] = pubkey_raw[10..42]
        .try_into()
        .map_err(|_| AppError::internal("公钥长度非法"))?;

    // 解开外层 base64 包装（tauri .sig 是整体单行 base64），得到多行文本
    let text = decode_sig_wrapper(sig_text)?;

    // 签名文件：取非 comment 行的第一个 base64 体
    let sig_line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("untrusted comment") && !l.starts_with("trusted comment"))
        .ok_or_else(|| AppError::upstream("签名文件格式非法：找不到签名体"))?;
    let sig_raw = base64::engine::general_purpose::STANDARD
        .decode(sig_line)
        .map_err(|_| AppError::upstream("签名体不是合法 base64"))?;
    // minisign 签名体 = [2B "ED" 算法标识][8B key_id][64B ed25519 签名] = 74 字节
    if sig_raw.len() != 74 {
        return Err(AppError::upstream("签名体长度非法（应为 74B = ED 标识 + key_id + 签名）"));
    }
    if sig_raw[..2] != *b"ED" {
        return Err(AppError::upstream("签名体算法标识非法"));
    }
    if sig_raw[2..10] != *expected_key_id {
        return Err(AppError::upstream("签名密钥与 BoenMind 官方公钥不匹配"));
    }
    let sig = ed25519_dalek::Signature::from_slice(&sig_raw[10..74])
        .map_err(|_| AppError::upstream("签名体不是合法 ed25519 签名"))?;

    // 内容 digest → ed25519 验证
    let digest = {
        use blake2::digest::Digest as _;
        let mut hasher = blake2::Blake2b512::new();
        hasher.update(fs::read(file).map_err(|e| AppError::Internal(format!("读取待验签文件失败: {e}")))?);
        hasher.finalize()
    };
    let pk = ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes)
        .map_err(|_| AppError::internal("公钥非法"))?;
    pk.verify(digest.as_ref(), &sig)
        .map_err(|_| AppError::upstream("签名验证失败：文件内容与签名不匹配（可能被篡改或下载损坏）"))
}

/// 签名文件外层解码：tauri signer 的 .sig 是整体单行 base64（解码后为多行
/// minisign 文本）；minisign 标准是明文多行。统一返回多行文本。
fn decode_sig_wrapper(sig_text: &str) -> Result<String, AppError> {
    use base64::Engine as _;
    let trimmed = sig_text.trim();
    // 尝试解开外层 base64：成功且解码结果是含 untrusted comment 的多行文本才采用
    if let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(trimmed)
        && let Ok(text) = String::from_utf8(raw)
        && text.contains("untrusted comment")
    {
        return Ok(text);
    }
    Ok(trimmed.to_string())
}

/// 下载资产与签名并验签，返回临时文件路径（调用方负责落盘/清理）。
/// 顺序：先取签名文本 → 再下载二进制 → 验签通过才算数，任何一步失败清理临时文件。
pub fn download_and_verify(asset: &ReleaseAsset) -> Result<PathBuf, AppError> {
    fs::create_dir_all(runtime_dir())?;
    let tmp = runtime_dir().join(format!(".download-{}", asset.name));
    let _ = fs::remove_file(&tmp);

    // 1. 签名文本（先取，下载失败不值得浪费带宽）
    let sig_text = {
        let resp = http_agent()
            .get(&asset.sig_url)
            .header("User-Agent", "BoenMind")
            .call()
            .map_err(|e| AppError::upstream(format!("下载签名失败: {e}")))?;
        let code = resp.status().as_u16();
        if code != 200 {
            return Err(AppError::upstream(format!("下载签名失败: HTTP {code}")));
        }
        resp.into_body()
            .into_with_config()
            .limit(64 * 1024)
            .read_to_string()
            .map_err(|e| AppError::upstream(format!("读取签名失败: {e}")))?
    };

    // 2. 二进制本体（服务器版含内嵌前端，体积最大约一两百 MB）
    let resp = http_agent()
        .get(&asset.url)
        .header("User-Agent", "BoenMind")
        .call()
        .map_err(|e| AppError::upstream(format!("下载更新失败: {e}")))?;
    let code = resp.status().as_u16();
    if code != 200 {
        let _ = fs::remove_file(&tmp);
        return Err(AppError::upstream(format!("下载更新失败: HTTP {code}")));
    }
    let body = resp
        .into_body()
        .into_with_config()
        .limit(512 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| AppError::upstream(format!("读取下载内容失败: {e}")))?;
    fs::write(&tmp, &body).map_err(|e| AppError::internal(format!("写入临时文件失败: {e}")))?;

    // 3. 验签（失败即删，绝不落盘未验签内容）
    if let Err(err) = verify_signature(&tmp, &sig_text) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(tmp)
}

/// 应用更新：下载 → 验签 → 落盘。
/// - managed：新文件落盘 runtime 目录（版本号命名不覆盖 → 天然回滚）
/// - standalone：备份旧版 + 原子替换自身 + 写 pending 标记
///   （进程未及重启时，下次启动由 main.rs 检测标记自动 exec 完成升级）
pub fn apply_update() -> Result<ApplyOutcome, AppError> {
    let check = check_update()?;
    let latest = check.latest.ok_or_else(|| AppError::invalid("当前已是最新版本"))?;
    let tmp = download_and_verify(&latest.asset)?;
    let version = latest.version.clone();
    let asset_name = latest.asset.name.clone();

    let result = (|| -> Result<ApplyOutcome, AppError> {
        if is_managed() {
            let dest = runtime_dir().join(&asset_name);
            fs::rename(&tmp, &dest).map_err(|e| AppError::internal(format!("落盘新版失败: {e}")))?;
            Ok(ApplyOutcome { version, mode: "managed".into(), path: Some(dest.display().to_string()) })
        } else {
            let exe = std::env::current_exe().map_err(|e| AppError::internal(format!("定位自身程序失败: {e}")))?;
            // 备份旧版（手动回滚用）；已备份过则跳过
            let backup = runtime_dir().join(format!("bm-server-{}-{}.bak", check.current, std::env::consts::ARCH));
            fs::create_dir_all(runtime_dir())?;
            if !backup.exists() {
                let _ = fs::copy(&exe, &backup);
            }
            // 原子替换自身（Linux：运行中 rename 覆盖安全，旧 inode 保持到进程退出）
            fs::rename(&tmp, &exe).map_err(|e| AppError::internal(format!("替换自身失败: {e}")))?;
            // pending 标记：exec 未执行（崩溃/断电）时，下次启动自动完成升级
            let marker = runtime_dir().join(UPDATE_PENDING_FILE);
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            fs::write(&marker, serde_json::json!({ "version": version, "applied_at": ts }).to_string())
                .map_err(|e| AppError::internal(format!("写待重启标记失败: {e}")))?;
            Ok(ApplyOutcome { version, mode: "standalone".into(), path: None })
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// 解码内嵌公钥为原始字节（minisign 文本 → 单行 base64 → 42 字节）
fn decode_pubkey() -> Result<Vec<u8>, AppError> {
    use base64::Engine as _;
    let text = base64::engine::general_purpose::STANDARD
        .decode(UPDATE_PUBKEY_B64)
        .map_err(|_| AppError::internal("内嵌公钥不是合法 base64"))?;
    let text = String::from_utf8(text).map_err(|_| AppError::internal("内嵌公钥不是合法文本"))?;
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("untrusted comment"))
        .ok_or_else(|| AppError::internal("内嵌公钥缺少密钥体"))?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(line)
        .map_err(|_| AppError::internal("内嵌公钥密钥体不是合法 base64"))?;
    if raw.len() != 42 {
        return Err(AppError::internal("内嵌公钥长度非法（应为 42 字节）"));
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use ed25519_dalek::Signer as _;

    const TEST_KEY_ID: [u8; 8] = [0xAA, 0xBB, 0xCC, 0xDD, 0x11, 0x22, 0x33, 0x44];
    const TEST_SECRET: [u8; 32] = [7u8; 32]; // 确定性测试密钥

    /// 生成测试用 minisign 格式签名（与 tauri signer 相同的
    /// Blake2b-512 digest + ed25519 链路；签名体 = ED 标识 + key_id + 签名）
    fn sign_test(secret: &ed25519_dalek::SigningKey, data: &[u8]) -> String {
        use blake2::digest::Digest as _;
        let mut hasher = blake2::Blake2b512::new();
        hasher.update(data);
        let digest = hasher.finalize();
        let sig = secret.sign(digest.as_ref()).to_bytes();
        let mut sig_raw = Vec::with_capacity(74);
        sig_raw.extend_from_slice(b"ED");
        sig_raw.extend_from_slice(&TEST_KEY_ID);
        sig_raw.extend_from_slice(&sig);
        format!(
            "untrusted comment: signature from minisign secret key\n{}\ntrusted comment: timestamp:0000\n",
            base64::engine::general_purpose::STANDARD.encode(&sig_raw)
        )
    }

    /// 测试公钥原始字节：[2B 算法标识][8B key_id][32B ed25519 pk]
    fn test_pubkey_raw(pk: &ed25519_dalek::VerifyingKey, key_id: &[u8]) -> Vec<u8> {
        let mut raw = Vec::with_capacity(42);
        raw.extend_from_slice(b"Ed");
        raw.extend_from_slice(key_id);
        raw.extend_from_slice(&pk.to_bytes());
        raw
    }

    #[test]
    fn version_compare() {
        assert_eq!(compare_versions("0.2.0", "0.1.1"), Ordering::Greater);
        assert_eq!(compare_versions("0.1.1", "0.1.1"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.1", "0.2.0"), Ordering::Less);
        assert_eq!(compare_versions("v0.2.0", "0.1.9"), Ordering::Greater);
        assert_eq!(compare_versions("0.2.0-beta.1", "0.1.9"), Ordering::Greater);
        assert_eq!(compare_versions("0.1.10", "0.1.9"), Ordering::Greater);
    }

    #[test]
    fn verify_full_flow_genuine_then_tampered() {
        let secret = ed25519_dalek::SigningKey::from_bytes(&TEST_SECRET);
        let pk = secret.verifying_key();
        let pubkey_raw = test_pubkey_raw(&pk, &TEST_KEY_ID);

        let tmp = std::env::temp_dir().join(format!("bm-updates-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("payload.bin");

        // 正常路径：同一密钥签的合法文件 → 通过
        fs::write(&file, b"genuine payload").unwrap();
        let sig_text = sign_test(&secret, b"genuine payload");
        assert!(verify_with_pubkey(&pubkey_raw, &file, &sig_text).is_ok());

        // 篡改文件 → 同一签名必须失败
        fs::write(&file, b"tampered payload!").unwrap();
        assert!(verify_with_pubkey(&pubkey_raw, &file, &sig_text).is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn verify_rejects_wrong_key_id_and_malformed() {
        let secret = ed25519_dalek::SigningKey::from_bytes(&TEST_SECRET);
        let pk = secret.verifying_key();
        // 公钥 key_id 与签名 key_id 不一致 → 拒绝
        let wrong_key_id = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01];
        let pubkey_wrong = test_pubkey_raw(&pk, &wrong_key_id);
        let tmp = std::env::temp_dir().join(format!("bm-updates-keyid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("payload.bin");
        fs::write(&file, b"x").unwrap();
        let sig_text = sign_test(&secret, b"x");
        assert!(verify_with_pubkey(&pubkey_wrong, &file, &sig_text).is_err());

        // 畸形签名体 → 拒绝（不读文件）
        assert!(verify_with_pubkey(&pubkey_wrong, &file, "not-base64").is_err());
        let short = base64::engine::general_purpose::STANDARD.encode(vec![0u8; 10]);
        assert!(verify_with_pubkey(&pubkey_wrong, &file, &short).is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn embedded_pubkey_is_valid_minisign() {
        // 内嵌公钥本身可解析且长度正确（[2B 算法][8B key_id][32B pk]）
        let raw = decode_pubkey().expect("内嵌公钥应可解码");
        assert_eq!(raw.len(), 42);
        assert_eq!(&raw[..2], b"Ed");
    }

    #[test]
    fn asset_name_uses_platform_triple() {
        let name = target_asset_name("0.2.0");
        assert!(name.starts_with("boenmind-runtime-0.2.0-"), "命名约定: {name}");
        assert!(name.contains(std::env::consts::ARCH), "应包含架构: {name}");
        #[cfg(windows)]
        assert!(name.ends_with(".exe"), "Windows 资产应带 .exe: {name}");
    }

    #[test]
    fn asset_url_allowlist() {
        assert!(asset_url_allowed("https://github.com/SadBoen/BoenMind/releases/download/v0.2.0/x"));
        assert!(asset_url_allowed("https://objects.githubusercontent.com/abc/def?x=1"));
        assert!(asset_url_allowed("https://release-assets.githubusercontent.com/x"));
        assert!(!asset_url_allowed("http://github.com/x")); // 非 https
        assert!(!asset_url_allowed("https://evil.example.com/x"));
        assert!(!asset_url_allowed("https://github.com.evil.example/x"));
        assert!(!asset_url_allowed("file:///etc/passwd"));
        assert!(!asset_url_allowed("https://192.168.1.1/x"));
    }
}
