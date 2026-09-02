//! W7:「关于」与在线升级。
//!
//! - GET  /admin/about:当前版本/平台/数据目录/更新源仓库;
//! - POST /admin/about/check-update:查 GitHub latest release,三方版本比较,
//!   按当前平台选资产(只读,不落地任何东西);
//! - POST /admin/about/apply-update:**仅回环地址**——下载→校验→解包→换装→
//!   以 BOEN_UPGRADE_CHILD=1 拉起新进程→本进程优雅排空退出。
//!
//! 铁规矩(2026-09-02 用户明示):发新版本必须用户明说;本模块只消费已发布的
//! release,绝不触发发布。

use crate::webadmin::{AdminConfig, admin_error};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const DEFAULT_REPO: &str = "SadBoen/BoenMind";

/// GET /admin/about
pub async fn about(State(cfg): State<AdminConfig>) -> Response {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "platform": format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
        "dataDir": cfg.data_dir.display().to_string(),
        "repo": update_repo(),
    }))
    .into_response()
}

fn update_repo() -> String {
    std::env::var("BOEN_UPDATE_REPO")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_REPO.to_string())
}

/// 三段数值版本比较:None = 无法解析(不可比较)。a<b → Less。
fn version_cmp(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let parse = |s: &str| -> Option<Vec<u64>> {
        let core = s.trim().trim_start_matches('v');
        let core = core.split(['-', '+']).next()?;
        let parts: Result<Vec<u64>, _> = core.split('.').map(|p| p.parse::<u64>()).collect();
        parts.ok()
    };
    let (va, vb) = (parse(a)?, parse(b)?);
    for i in 0..3 {
        let x = va.get(i).copied().unwrap_or(0);
        let y = vb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            o => return Some(o),
        }
    }
    Some(Ordering::Equal)
}

/// 当前平台对应的发布资产后缀(发布包统一 .tar.gz)。
fn platform_asset_suffix() -> &'static str {
    if std::env::consts::OS == "windows" {
        "windows-x86_64.tar.gz"
    } else {
        "linux-x86_64.tar.gz"
    }
}

struct ReleaseInfo {
    tag: String,
    notes: String,
    /// (资产名, 下载地址) 清单。
    assets: Vec<(String, String)>,
}

impl ReleaseInfo {
    /// 按平台后缀挑资产。
    fn asset(&self) -> Option<(String, String)> {
        let suffix = platform_asset_suffix();
        self.assets.iter().find_map(|(name, url)| {
            if name.ends_with(suffix) {
                Some((name.clone(), url.clone()))
            } else {
                None
            }
        })
    }
}

async fn fetch_latest_release(repo: &str) -> Result<ReleaseInfo, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let client = reqwest::Client::builder()
        .user_agent(concat!("boenmind-server/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("HTTP 客户端构造失败: {e}"))?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("访问 GitHub 失败: {e}"))?
        .error_for_status()
        .map_err(|e| format!("GitHub 响应异常: {e}"))?;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("GitHub JSON 解析失败: {e}"))?;
    let tag = v["tag_name"]
        .as_str()
        .ok_or("响应缺少 tag_name(仓库可能还没有 release)")?
        .to_string();
    let notes = v["body"].as_str().unwrap_or_default().to_string();
    let assets = v["assets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| {
            Some((
                a["name"].as_str()?.to_string(),
                a["browser_download_url"].as_str()?.to_string(),
            ))
        })
        .collect();
    Ok(ReleaseInfo { tag, notes, assets })
}

/// POST /admin/about/check-update(只读)。是否可升由版本比较决定;资产缺失
/// 不算错误——真有更新但无本平台资产时以 note 提示。
pub async fn check_update(State(_cfg): State<AdminConfig>) -> Response {
    let current = env!("CARGO_PKG_VERSION");
    match fetch_latest_release(&update_repo()).await {
        Ok(r) => {
            let update_available = version_cmp(current, &r.tag)
                .map(|o| o == std::cmp::Ordering::Less)
                .unwrap_or(false);
            let (asset, note) = match r.asset() {
                Some((name, url)) => (Some(json!({ "name": name, "url": url })), None),
                None if update_available => (
                    None,
                    Some(format!(
                        "发现新版本 {},但 latest release 缺少本平台资产({})",
                        r.tag,
                        platform_asset_suffix()
                    )),
                ),
                None => (None, None),
            };
            Json(json!({
                "ok": true,
                "current": current,
                "latest": r.tag,
                "updateAvailable": update_available,
                "asset": asset,
                "note": note,
                "notes": r.notes,
            }))
            .into_response()
        }
        Err(e) => Json(json!({ "ok": false, "current": current, "error": e })).into_response(),
    }
}

/// POST /admin/about/apply-update —— 仅回环地址。
pub async fn apply_update(
    State(cfg): State<AdminConfig>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
) -> Response {
    if !addr.ip().is_loopback() {
        return admin_error(
            StatusCode::FORBIDDEN,
            "在线升级仅允许本机(回环地址)发起;远程主机请登录后在本机操作",
        );
    }
    let current = env!("CARGO_PKG_VERSION");
    let info = match fetch_latest_release(&update_repo()).await {
        Ok(r) => r,
        Err(e) => return admin_error(StatusCode::BAD_GATEWAY, format!("检查更新失败: {e}")),
    };
    match version_cmp(current, &info.tag) {
        Some(o) if o != std::cmp::Ordering::Less => {
            return admin_error(StatusCode::BAD_REQUEST, "已是最新版本,无需升级");
        }
        Some(_) => {}
        None => {
            return admin_error(
                StatusCode::BAD_REQUEST,
                format!("版本号不可比较(当前 {current} vs latest {})", info.tag),
            );
        }
    }
    let Some((asset_name, asset_url)) = info.asset() else {
        return admin_error(
            StatusCode::BAD_REQUEST,
            format!(
                "发现新版本 {},但 latest release 缺少本平台资产({})",
                info.tag,
                platform_asset_suffix()
            ),
        );
    };
    let sha_url = format!("{asset_url}.sha256");

    let work = cfg.data_dir.join("upgrade");
    if let Err(e) = std::fs::create_dir_all(&work) {
        return admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("升级目录创建失败: {e}"),
        );
    }

    // 1. 下载资产与校验和
    let client = match reqwest::Client::builder()
        .user_agent(concat!("boenmind-server/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return admin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("客户端构造失败: {e}"),
            );
        }
    };
    let pkg_path = work.join(&asset_name);
    if let Err(e) = download_to(&client, &asset_url, &pkg_path).await {
        return admin_error(StatusCode::BAD_GATEWAY, format!("下载失败: {e}"));
    }
    let sha_path = work.join(format!("{asset_name}.sha256"));
    if let Err(e) = download_to(&client, &sha_url, &sha_path).await {
        return admin_error(StatusCode::BAD_GATEWAY, format!("下载校验文件失败: {e}"));
    }

    // 2. 校验和比对(期望格式:<hex>  <文件名>)
    let bytes = match std::fs::read(&pkg_path) {
        Ok(b) => b,
        Err(e) => {
            return admin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("读取包失败: {e}"),
            );
        }
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest_bytes = hasher.finalize();
    let digest = digest_bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let Ok(expected_sha) = std::fs::read_to_string(&sha_path) else {
        return admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "读取 sha256 校验文件失败",
        );
    };
    let expected = expected_sha
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    if digest != expected {
        return admin_error(
            StatusCode::BAD_GATEWAY,
            format!("校验和不匹配: 计算值 {digest} vs 期望值 {expected}"),
        );
    }

    // 3. 解包到 staging
    let staging = work.join(format!("staging-{}", info.tag.trim_start_matches('v')));
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    if let Err(e) = unpack_tar_gz(&pkg_path, &staging) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("解包失败: {e}"));
    }

    // 4. 换装(staging 内有唯一顶层目录)
    let inner = match std::fs::read_dir(&staging) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .find(|e| e.path().is_dir())
            .map(|e| e.path()),
        Err(_) => None,
    };
    let Some(src) = inner else {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, "包结构异常:缺少顶层目录");
    };
    if let Err(e) = install(&cfg, &src) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("换装失败: {e}"));
    }

    // 5. 拉起子进程(原 args/env/cwd;BOEN_UPGRADE_CHILD=1 → 子进程容忍端口
    //    被本进程暂时占用,重试绑定),随后本进程优雅排空退出。
    let exe = std::env::current_exe().map_err(|e| format!("current_exe 失败: {e}"));
    let exe = match exe {
        Ok(p) => p,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let child = std::process::Command::new(&exe)
        .args(&args)
        .env("BOEN_UPGRADE_CHILD", "1")
        .spawn();
    if let Err(e) = child {
        return admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("新进程拉起失败: {e}"),
        );
    }
    eprintln!("[W7] 在线升级:新版本 {} 已拉起,本进程排空退出", info.tag);
    if let Some(shutdown) = &cfg.shutdown {
        shutdown.notify_waiters();
    }
    Json(json!({
        "ok": true,
        "restarting": true,
        "note": format!("已换装 {} 并重启服务;页面将在服务就绪后自动刷新", info.tag),
    }))
    .into_response()
}

async fn download_to(
    client: &reqwest::Client,
    url: &str,
    path: &std::path::Path,
) -> Result<(), String> {
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("{e}"))?
        .error_for_status()
        .map_err(|e| format!("{e}"))?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取响应失败: {e}"))?;
    std::fs::write(path, &bytes).map_err(|e| format!("写文件失败: {e}"))
}

fn unpack_tar_gz(archive: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    let f = std::fs::File::open(archive).map_err(|e| format!("打开包失败: {e}"))?;
    let gz = flate2::read::GzDecoder::new(f);
    let mut tar = tar::Archive::new(gz);
    // 安全解包:拒绝绝对路径与 .. 逃逸(tar crate 自带防护,双保险)
    tar.set_preserve_permissions(false);
    tar.unpack(dest).map_err(|e| format!("{e}"))
}

/// 把 staging 顶层目录内容换装进运行环境:
/// - 二进制 → current_exe(旧文件改名 .old-<ts> 保留,Windows 允许改名运行中的 exe);
/// - webapp/dist → web_dir(覆盖);
/// - plugins/* → exe 同级 plugins/(合并覆盖)。
fn install(cfg: &AdminConfig, src: &std::path::Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let exe_dir = exe.parent().ok_or("exe 无父目录")?.to_path_buf();

    // 二进制
    let new_bin = if cfg!(windows) {
        src.join("boenmind-server.exe")
    } else {
        src.join("boenmind-server")
    };
    if !new_bin.exists() {
        return Err("包内缺少 boenmind-server".into());
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let old = exe.with_extension(format!(
        "old-{ts}{}",
        if cfg!(windows) { "exe" } else { "" }
    ));
    let _ = std::fs::rename(&exe, &old); // 改名失败(个别 FS)则尝试直接覆盖
    std::fs::copy(&new_bin, &exe).map_err(|e| format!("新二进制落位失败: {e}"))?;

    // 前端 dist(覆盖 web_dir)
    if let Some(web_dir) = &cfg.web_dir {
        let web_dir = if web_dir.is_absolute() {
            web_dir.clone()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("current_dir: {e}"))?
                .join(web_dir)
        };
        let new_dist = src.join("webapp/dist");
        if new_dist.exists() {
            if web_dir.exists() {
                std::fs::remove_dir_all(&web_dir).map_err(|e| format!("清理旧 dist 失败: {e}"))?;
            }
            std::fs::create_dir_all(&web_dir).map_err(|e| format!("dist 目录创建失败: {e}"))?;
            copy_dir_recursive(&new_dist, &web_dir)?;
        }
    }

    // 官方插件(exe 同级 plugins/,合并覆盖)
    let new_plugins = src.join("plugins");
    if new_plugins.exists() {
        let dst_plugins = exe_dir.join("plugins");
        std::fs::create_dir_all(&dst_plugins).map_err(|e| format!("plugins 目录创建失败: {e}"))?;
        copy_dir_recursive(&new_plugins, &dst_plugins)?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("create_dir {}: {e}", dst.display()))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("read_dir {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("{e}"))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("copy {}: {e}", from.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::version_cmp;
    use std::cmp::Ordering;

    #[test]
    fn t_w7_version_compare() {
        assert_eq!(version_cmp("0.0.2", "v0.0.2"), Some(Ordering::Equal));
        assert_eq!(version_cmp("0.0.2", "v0.0.3"), Some(Ordering::Less));
        assert_eq!(version_cmp("0.1.0", "v0.0.9"), Some(Ordering::Greater));
        assert_eq!(version_cmp("0.0.2", "v0.0.10"), Some(Ordering::Less));
        assert_eq!(version_cmp("0.0.2-m1", "v0.0.3"), Some(Ordering::Less));
        assert_eq!(version_cmp("abc", "0.0.2"), None);
    }
}
