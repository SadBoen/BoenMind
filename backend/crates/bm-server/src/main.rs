//! BoenMind 后端独立二进制入口。
//! 完整逻辑见 `bm_server::serve`（与 Tauri 桌面壳共用）。

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,bm_server=debug")),
        )
        .init();

    let port = std::env::var("BOENMIND_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(bm_server::DEFAULT_PORT);
    if let Err(err) = bm_server::serve(port).await {
        eprintln!("[bm-server] 服务异常退出: {err}");
        std::process::exit(1);
    }
}
