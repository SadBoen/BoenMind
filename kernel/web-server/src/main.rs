//! web-server 二进制：Rust 协议兼容层服务入口。
//!
//! 用法：web-server [--db <path>] [--dist <dist_root>] [--boot-json <file>] [--port <port>] [--trusted-host <host>]
//!
//! 默认 `--dist` 指向内置前端快照 `kernel/web-server/frontend/`（dsh rc.6 壳层 +
//! 真实 boot 清单 + 38 插件 client bundle，见同目录 README）。快照 index.html 已含
//! `window.__DSH_BOOT__` 注入，无需再注入；对自备 dist 可用 `--boot-json` 提供清单。

use std::path::PathBuf;
use std::sync::Arc;

use kernel_assembly::Runtime;
use web_server::api::AppState;
use web_server::rpc::API_PATH;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let mut db = PathBuf::from("boenmind.db");
    let mut dist = PathBuf::from("kernel/web-server/frontend");
    let mut boot_json: Option<String> = None;
    let mut port: u16 = 3080;
    let mut trusted_hosts: Vec<String> = vec![];

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                i += 1;
                db = PathBuf::from(&args[i]);
            }
            "--dist" => {
                i += 1;
                dist = PathBuf::from(&args[i]);
            }
            "--boot-json" => {
                i += 1;
                let path = &args[i];
                boot_json = Some(
                    std::fs::read_to_string(path)
                        .unwrap_or_else(|e| panic!("cannot read boot json {path}: {e}")),
                );
            }
            "--port" => {
                i += 1;
                port = args[i].parse().expect("port must be a number");
            }
            "--trusted-host" => {
                i += 1;
                trusted_hosts.push(args[i].clone());
            }
            "--help" | "-h" => {
                println!(
                    "usage: web-server [--db <path>] [--dist <dir>] [--boot-json <file>] [--port <n>] [--trusted-host <host>]"
                );
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let runtime = match Runtime::headless(db.clone()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runtime init failed: {e}");
            std::process::exit(1);
        }
    };
    let state = Arc::new(AppState::with_trusted_hosts(runtime, trusted_hosts.clone()));
    // 持有总线监听器句柄到进程结束（drop 即注销，实时事件流依赖它）。
    let _bus_listener = state.attach_event_bus();

    let app = web_server::router(Arc::clone(&state), dist.clone(), boot_json);

    let addr = format!("127.0.0.1:{port}");
    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap_or_else(|e| {
                eprintln!("bind {addr} failed: {e}");
                std::process::exit(1);
            });
            tracing::info!(
                "web-server listening on http://{addr} (db={}, dist={}, api={API_PATH}, trusted={:?})",
                db.display(),
                dist.display(),
                trusted_hosts
            );
            axum::serve(listener, app).await.unwrap();
        });
}

