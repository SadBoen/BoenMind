//! BoenMind 桌面壳（管家模式）。
//!
//! 壳 = 纯启动器：优先运行 `~/.boenmind/runtime/` 下最新版的 bm-server 独立
//! 二进制（热更新落盘的产物），没有则回退到编译进壳的内嵌后端（兜底）。
//! 后端以子进程或内嵌线程方式运行，前端统一访问 `http://127.0.0.1:17321`
//! （index.html 注入 `__BOENMIND_API__`）。
//!
//! 热更新流程：About 页点升级 → 后端下载新版本到 runtime 目录 → 前端调
//! `backend_restart` 命令 → 壳停掉当前后端（kill 子进程 / 给内嵌线程发关闭
//! 信号）→ 监控循环检测到退出 → 按最新版本重新拉起 → 前端 health 轮询
//! 检测版本变化后刷新页面。应用窗口全程不关、不需要重装。

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::Mutex;
use tauri::Manager as _;

/// 当前 bm-server 子进程（Some = runtime 子进程模式；None = 内嵌线程兜底或未启动）
#[derive(Default)]
struct BackendManager {
    child: Mutex<Option<Child>>,
    /// 内嵌兜底线程的结束通知（serve 返回后发出）
    embedded_done: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    /// 内嵌兜底 serve 的优雅关闭触发（backend_restart 时发送）
    embedded_shutdown: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
}

/// runtime 目录（与 bm-core 约定一致：`~/.boenmind/runtime`，BOENMIND_HOME 可覆盖）
fn runtime_dir() -> PathBuf {
    std::env::var_os("BOENMIND_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join(".boenmind")
        .join("runtime")
}

/// 便携包根：当前可执行文件所在目录中**存在 web/ 目录**时即为便携形态
/// （2026-08-16 用户拍板多文件：web/server/skills/plugins/mcps/data）。
/// 老单文件形态（无 web/）不设置 → bm-server 走 ~/.boenmind（行为不变）。
fn portable_root() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()?
        .parent()
        .map(PathBuf::from)?;
    exe_dir.join("web").is_dir().then_some(exe_dir)
}

/// 便携形态数据目录：包内 data/ 存在 = 完全便携（BOENMIND_HOME 指向包内）；
/// 不存在 = 数据走用户主目录（老数据不丢）。壳只做检测与传参。
fn portable_data_home() -> Option<PathBuf> {
    let root = portable_root()?;
    let data = root.join("data");
    data.is_dir().then_some(data)
}

/// 便携形态环境变量注入（子进程与内嵌 serve 共用）：
/// BOENMIND_PORTABLE_DIR + BOENMIND_WEB_DIR（包内 web/ 磁盘形态静态服务）+
/// 可选 BOENMIND_HOME（包内 data/）。未设置的环境变量需显式移除，
/// 防止上次启动残留污染本次启动。
fn apply_portable_env() {
    match portable_root() {
        Some(root) => {
            unsafe { std::env::set_var("BOENMIND_PORTABLE_DIR", &root) };
            let web = root.join("web");
            unsafe { std::env::set_var("BOENMIND_WEB_DIR", web) };
            if let Some(data) = portable_data_home() {
                unsafe { std::env::set_var("BOENMIND_HOME", data) };
            }
        }
        None => {
            unsafe { std::env::remove_var("BOENMIND_PORTABLE_DIR") };
            unsafe { std::env::remove_var("BOENMIND_WEB_DIR") };
        }
    }
}

/// 扫描 runtime 目录，返回最新版本的后端二进制（无则 None）。
/// 命名约定 `boenmind-runtime-<ver>-<triple>[.exe]`，与发布管线一致。
fn latest_runtime_binary() -> Option<PathBuf> {
    let dir = runtime_dir();
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut best: Option<(String, PathBuf)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(rest) = name.strip_prefix("boenmind-runtime-") else { continue };
        // 跳过签名/下载临时/压缩包残留文件
        if rest.contains(".sig") || rest.contains(".download-") || rest.contains(".bak") || rest.contains(".gz") {
            continue;
        }
        // 版本 = 第一个 `-` 之前的部分（`0.2.0-x86_64-pc-windows-msvc.exe` → `0.2.0`）
        let version = rest.split('-').next().unwrap_or("").to_string();
        if version.is_empty() {
            continue;
        }
        let is_newer = best
            .as_ref()
            .is_none_or(|(v, _)| bm_core::updates::compare_versions(&version, v) == std::cmp::Ordering::Greater);
        if is_newer {
            best = Some((version, entry.path()));
        }
    }
    best.map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 扫描排序：按版本选最新，跳过签名/下载临时文件（本测试修改全局
    /// BOENMIND_HOME，用进程级串行即可——壳测试无并行风险）
    #[test]
    fn picks_latest_runtime_binary() {
        let original = std::env::var_os("BOENMIND_HOME");
        let dir = std::env::temp_dir().join(format!("bm-shell-runtime-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("BOENMIND_HOME", &dir) };
        let rd = runtime_dir();
        std::fs::create_dir_all(&rd).unwrap();
        for name in [
            "boenmind-runtime-0.1.1-aarch64-apple-darwin",
            "boenmind-runtime-0.2.0-aarch64-apple-darwin",
            "boenmind-runtime-0.2.0-aarch64-apple-darwin.sig", // 签名文件跳过
            "boenmind-runtime-0.3.0-x86_64-apple-darwin", // 更高版本（其它平台名）也应选中
            "boenmind-runtime-0.1.1-x86_64-pc-windows-msvc.exe",
            ".download-boenmind-runtime-0.9.0-aarch64-apple-darwin", // 下载临时文件跳过
            "boenmind-runtime-0.9.9-aarch64-apple-darwin.gz", // 压缩包残留跳过（解压后落盘无 .gz）
        ] {
            std::fs::write(rd.join(name), b"x").unwrap();
        }

        let best = latest_runtime_binary().expect("应找到 runtime 二进制");
        assert!(best.file_name().unwrap().to_string_lossy().contains("0.3.0"));

        // 目录不存在 → None（首次安装，走内嵌兜底）
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("BOENMIND_HOME", dir.join("empty")) };
        assert!(latest_runtime_binary().is_none());

        match original {
            Some(v) => unsafe { std::env::set_var("BOENMIND_HOME", v) },
            None => unsafe { std::env::remove_var("BOENMIND_HOME") },
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// 启动后端（子进程优先，内嵌线程兜底）。幂等：只负责"当前没有后端时"拉起。
fn start_backend(manager: &BackendManager) {
    if let Some(path) = latest_runtime_binary() {
        eprintln!("[boenmind] 启动 runtime 后端: {}", path.display());
        match Command::new(&path).env("BOENMIND_MANAGED", "1").spawn() {
            Ok(child) => {
                *manager.child.lock().unwrap() = Some(child);
                return;
            }
            Err(err) => eprintln!("[boenmind] 启动 runtime 后端失败（{err}），回退内嵌后端"),
        }
    }

    // 内嵌兜底：axum 需要自己的 tokio runtime，放在独立线程中运行。
    // 支持优雅关闭：backend_restart 时通过 watch 通知 serve 收尾退出，
    // 线程结束后监控循环会重新拉起（此时 runtime 目录已有新版本）。
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                eprintln!("[boenmind] 创建 tokio runtime 失败: {err}");
                return;
            }
        };
        if let Err(err) = rt.block_on(bm_server::serve_managed(bm_server::DEFAULT_PORT, shutdown_rx)) {
            eprintln!("[boenmind] 内嵌后端启动失败: {err}");
        }
        let _ = done_tx.send(());
    });
    *manager.embedded_shutdown.lock().unwrap() = Some(shutdown_tx);
    *manager.embedded_done.lock().unwrap() = Some(done_rx);
    eprintln!("[boenmind] 启动内嵌后端（兜底）");
}

/// 停止当前后端（热更新换新版）：子进程 kill；内嵌线程发优雅关闭信号。
/// 停止后由监控循环负责按最新版本重新拉起。
fn stop_backend(manager: &BackendManager) {
    // 1. runtime 子进程：kill + wait（防僵尸）
    let mut guard = manager.child.lock().unwrap();
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
        eprintln!("[boenmind] 已停止 runtime 后端子进程");
    }
    drop(guard);
    // 2. 内嵌兜底：发关闭信号（serve_managed 优雅退出，线程自然结束）
    if let Some(tx) = manager.embedded_shutdown.lock().unwrap().take() {
        let _ = tx.send(true);
        eprintln!("[boenmind] 已向内嵌后端发送关闭信号");
    }
}

/// 监控循环：后端（子进程/内嵌线程）退出后，按最新版本重新拉起。
fn spawn_backend_watchdog(manager: Arc<BackendManager>) {
    std::thread::spawn(move || {
        loop {
            let child_exited = {
                let mut guard = manager.child.lock().unwrap();
                match guard.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(_)) => {
                            *guard = None;
                            true
                        }
                        Ok(None) => false,
                        Err(_) => {
                            *guard = None;
                            true
                        }
                    },
                    None => false,
                }
            };
            let embedded_exited = manager
                .embedded_done
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|rx| rx.try_recv().is_ok());
            if child_exited || embedded_exited {
                eprintln!("[boenmind] 后端已退出，重新拉起…");
                start_backend(&manager);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

/// 热更新后重启后端：停掉当前后端，监控循环随即按最新版本拉起（前端随后
/// 轮询 health 检测版本变化并刷新页面）。
#[tauri::command]
fn backend_restart(manager: tauri::State<'_, Arc<BackendManager>>) {
    eprintln!("[boenmind] 收到 backend_restart 命令");
    stop_backend(&manager);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 便携形态环境变量先于一切后端启动注入（子进程 spawn 与内嵌 serve 都读它）
    apply_portable_env();
    let manager = Arc::new(BackendManager::default());

    // 启动后端（runtime 最新版优先，无则内嵌兜底）
    start_backend(&manager);
    spawn_backend_watchdog(manager.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(manager.clone())
        .invoke_handler(tauri::generate_handler![backend_restart])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 应用退出时清理后端子进程（防孤儿进程）
            if matches!(event, tauri::RunEvent::Exit) {
                let manager = app.state::<Arc<BackendManager>>();
                stop_backend(&manager);
            }
        });
}
