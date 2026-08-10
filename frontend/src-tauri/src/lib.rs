//! BoenMind 桌面壳：应用启动时在独立线程内嵌拉起 bm-server（axum），
//! 前端通过 `http://127.0.0.1:17321` 访问（由 index.html 注入 `__BOENMIND_API__`）。

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 内嵌后端：axum 需要自己的 tokio runtime，放在独立线程中运行
    std::thread::spawn(|| {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(err) => {
                eprintln!("[boenmind] 创建 tokio runtime 失败: {err}");
                return;
            }
        };
        if let Err(err) = rt.block_on(bm_server::serve(bm_server::DEFAULT_PORT)) {
            eprintln!("[boenmind] 内嵌后端启动失败: {err}");
        }
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
