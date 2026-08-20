//! BoenMind 桌面壳。
//!
//! 职责：无边框（decorations:false）+ 透明窗口承载 React 前端；
//! 应用启动时在独立线程内嵌拉起 web-server（web-server.exe 子进程，
//! 前端通过 http://127.0.0.1:17321 访问）；提供"检查更新"命令（Tauri updater 插件）。

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use tauri::Manager;

/// web-server 后端监听地址（必须与 index.html 注入的 base 一致）。
const BACKEND_ADDR: &str = "127.0.0.1:17321";

/// 内嵌后端：spawn web-server.exe 子进程。
/// web-server 是主 workspace（bm/web-server）构建出的协议层二进制，
/// 与前端 dist 一起随包分发。找不到可执行文件时静默失败（开发模式由前端 vite 代劳）。
fn spawn_backend(resource_dir: PathBuf) {
    let exe = resource_dir.join("web-server.exe");
    std::thread::spawn(move || {
        if !exe.exists() {
            eprintln!("[boenmind] 未找到后端资源 web-server.exe（开发模式请用 npm run dev 直连 3080）");
            return;
        }
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--port").arg("17321").arg("--dist").arg("frontend/dist");
        match cmd.spawn() {
            Ok(_) => eprintln!("[boenmind] 内嵌后端已启动: {BACKEND_ADDR}"),
            Err(e) => eprintln!("[boenmind] 内嵌后端启动失败: {e}"),
        }
    });
}

/// 检查更新命令（前端"设置 → 关于 → 检查更新"调用）。
#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<Option<serde_json::Value>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    if let Some(u) = update {
        Ok(Some(serde_json::json!({
            "available": true,
            "version": u.version().clone(),
            "date": u.date().map(|d| d.to_rfc3339()),
            "body": u.body().clone(),
        })))
    } else {
        Ok(Some(serde_json::json!({ "available": false })))
    }
}

/// 下载并安装更新（Tauri updater 自动替换 + 重启）。
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<bool, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    if let Some(u) = update {
        u.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 内嵌后端：spawn web-server.exe（资源目录 = 可执行文件旁）
            let resource_dir = app
                .path()
                .resource_dir()
                .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
            spawn_backend(resource_dir);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![check_update, install_update])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}