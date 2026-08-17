//! web-server 二进制：Rust 协议兼容层服务入口。
//!
//! 用法：web-server [--db <path>] [--dist <dist_root>] [--port <port>] [--trusted-host <host>]

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
    let mut dist = PathBuf::from("frontend/dist");
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
                    "usage: web-server [--db <path>] [--dist <dir>] [--port <n>] [--trusted-host <host>]"
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

    // boot 3 槽（面 6）：__DSH_BOOT__ JSON。
    let boot_json = format!(
        r#"{{"rev":"{:012x}","entries":[]}}"#,
        simple_hash(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    );

    let runtime = match Runtime::headless(db.clone()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runtime init failed: {e}");
            std::process::exit(1);
        }
    };
    let state = Arc::new(AppState::with_trusted_hosts(runtime, trusted_hosts.clone()));
    state.attach_event_bus();

    let app = web_server::router(Arc::clone(&state), dist.clone(), Some(boot_json));

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

/// 简单 12 位 hex 哈希（rev 字段：boot manifest 要求 12 位 hex）。
fn simple_hash(n: u128) -> u64 {
    (n ^ (n >> 32) ^ (n >> 64) ^ (n >> 96)) as u64
}
