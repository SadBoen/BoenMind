//! BoenMind 后端独立二进制入口。
//! 完整逻辑见 `bm_server::serve`（与 Tauri 桌面壳共用）。
//!
//! 子代理模式：上游 subagent 工具 spawn 本二进制并传 `--mode json --print
//! --no-session ...` 参数——main 最先判别并转交 `subagent_child::run`，
//! 不初始化 tracing（stdout 是协议通道）也不启动 HTTP。

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if bm_server::subagent_child::should_enter_child_mode(&args) {
        std::process::exit(bm_server::subagent_child::run(&args).await);
    }

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
