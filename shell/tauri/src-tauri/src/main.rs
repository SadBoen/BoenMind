//! BoenMind Windows 桌面壳(Tauri v2;ADR-0009)。
//! 最小入口:窗口加载与 Web UI 同源的静态页(runtime/web/index.html,
//! 经 tauri.conf.json frontendDist 打包)。业务逻辑全部在页面 JS 内,
//! 直连 boenmind-server HTTP API——壳不持有令牌/密钥。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("BoenMind 壳启动失败");
}
