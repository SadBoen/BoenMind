//! BoenMind 后端独立二进制入口。
//! 完整逻辑见 `bm_server::serve`（与 Tauri 桌面壳共用）。
//!
//! 子代理模式：上游 subagent 工具 spawn 本二进制并传 `--mode json --print
//! --no-session ...` 参数——main 最先判别并转交 `subagent_child::run`，
//! 不初始化 tracing（stdout 是协议通道）也不启动 HTTP。
//!
//! 反向 MCP server：`--mcp-serve` 以 stdio MCP server 身份运行（同
//! subagent 模式——stdout 是协议通道，不初始化 tracing）。

use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if bm_server::subagent_child::should_enter_child_mode(&args) {
        std::process::exit(bm_server::subagent_child::run(&args).await);
    }
    if args.iter().any(|a| a == "--mcp-serve") {
        std::process::exit(match bm_server::mcp_serve::run().await {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("[bm-server] mcp-serve: {err}");
                1
            }
        });
    }

    // 日志双通道：stdout fmt（现状）+ 内存环形缓冲（设置中心「日志」页）。
    // 缓冲只收已通过 EnvFilter 的事件（默认 info,bm_server=debug）。
    let log_buffer = bm_server::log_buffer::LogBuffer::install();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,bm_server=debug")),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(bm_server::log_buffer::BufferLayer::new(log_buffer))
        .init();

    let port = std::env::var("BOENMIND_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(bm_server::DEFAULT_PORT);

    // 自更新残留：apply 已替换自身但未重启（崩溃/断电）→ 先 exec 完成升级。
    // 注意放在子代理模式判别之后：subagent 子进程不参与自更新。
    if let Err(err) = bm_server::consume_pending_update() {
        eprintln!("[bm-server] {err}");
    }

    if let Err(err) = bm_server::serve(port).await {
        eprintln!("[bm-server] 服务异常退出: {err}");
        std::process::exit(1);
    }
}
