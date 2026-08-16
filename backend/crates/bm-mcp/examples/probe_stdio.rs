//! MCP client probe：连一个 MCP server，打印协商版本 / 工具列表 / 一次工具调用。
//!
//! 用法:
//!   cargo run -p bm-mcp --example probe_stdio -- <server_js> <workdir>   # stdio
//!   cargo run -p bm-mcp --example probe_stdio -- --http <url>            # streamable http / legacy SSE
//!
//! 验证目标：
//! - dual-era 协商（Auto 模式：首选 2026-07-28，legacy 回退 2025-11-25）
//! - tools/list 枚举与 mcp__ 命名
//! - tools/call 调用与结果提取

use bm_mcp::{McpClientManager, McpServerConfig, McpService, McpTransportKind};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args: Vec<String> = std::env::args().collect();
    let config = match args.as_slice() {
        [_, flag, url] if flag == "--http" => McpServerConfig {
            name: "probe".into(),
            transport: McpTransportKind::Http,
            command: None,
            args: vec![],
            env: Default::default(),
            url: Some(url.clone()),
            headers: Default::default(),
            tool_timeout_ms: Some(30_000),
            scopes: vec![],
        },
        [_, js, dir] => McpServerConfig {
            name: "probe".into(),
            transport: McpTransportKind::Stdio,
            command: Some("node".into()),
            args: vec![js.clone(), dir.clone()],
            env: Default::default(),
            url: None,
            headers: Default::default(),
            tool_timeout_ms: Some(30_000),
            scopes: vec![],
        },
        _ => {
            eprintln!("用法: probe_stdio <server_js> <workdir> | probe_stdio --http <url>");
            std::process::exit(2);
        }
    };

    let manager = McpClientManager::new();
    let handle = manager.connect(config).await.expect("连接失败");
    println!("[ok] 协商协议版本: {}", handle.protocol_version.read().unwrap());

    let tools = manager.tools();
    println!("[ok] 工具数量: {}", tools.len());
    for t in tools.iter().take(5) {
        println!("     - {} (wire: {})", t.qualified_name, t.name);
    }

    for t in tools.iter() {
        let result = manager.call_tool(&t.qualified_name, serde_json::json!({})).await;
        match result {
            Ok(v) => {
                println!("[ok] 调用 `{}` 结果: {}", t.qualified_name, v);
                break;
            }
            Err(e) => println!("[!] 调用 `{}` 失败（跳过）: {}", t.qualified_name, e),
        }
    }

    manager.disconnect("probe").await.expect("断开失败");
    println!("[ok] 断开完成");
}
