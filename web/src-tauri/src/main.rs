//! BoenMind 桌面壳(ADR-0009:Windows Tauri 壳,复用 Web 前端)。
//! 窗口直载 boenmind-server 的 Web Surface(默认 http://127.0.0.1:7531/):
//! 壳不含业务逻辑,数据一律经鉴权 API——与 CLI/Web 同一 Runtime API(基线 §14)。
//!
//! 前置:boenmind-server 已在运行(端口可经 --bind 配置);
//! 目标地址可用环境变量 BOENMIND_SURFACE_URL 覆盖。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let url: tauri::Url = std::env::var("BOENMIND_SURFACE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:7531/".to_string())
        .parse()
        .expect("BOENMIND_SURFACE_URL 非法");

    tauri::Builder::default()
        .setup(move |app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(url),
            )
            .title("BoenMind")
            .inner_size(1100.0, 760.0)
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("BoenMind 桌面壳启动失败");
}
