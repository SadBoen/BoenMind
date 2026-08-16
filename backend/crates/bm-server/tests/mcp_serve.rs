//! 反向 MCP server 集成测试：spawn `bm-server --mcp-serve`，用 bm-mcp
//! client（stdio）接入，验证工具列表与真实调用（read/ls 工作目录内）。

use bm_mcp::{McpClientManager, McpServerConfig, McpService, McpTransportKind};

#[tokio::test]
async fn serve_stdio_exposes_and_executes_builtin_tools() {
    let exe = env!("CARGO_BIN_EXE_bm-server");
    let manager = McpClientManager::new();
    let config = McpServerConfig {
        name: "reverse".into(),
        transport: McpTransportKind::Stdio,
        command: Some(exe.to_string()),
        args: vec!["--mcp-serve".into()],
        env: Default::default(),
        url: None,
        headers: Default::default(),
        tool_timeout_ms: Some(30_000),
    };

    let handle = manager.connect(config).await.expect("反向 server 连接失败");
    assert_eq!(
        handle.protocol_version.read().unwrap().as_str(),
        "2026-07-28",
        "反向 server 原生 2.0（server/discover 直通）"
    );

    let tools = manager.tools();
    eprintln!("反向 server 工具: {:?}", tools.iter().map(|t| &t.qualified_name).collect::<Vec<_>>());
    assert!(
        tools.iter().any(|t| t.qualified_name == "mcp__reverse__read"),
        "内置工具 read 应暴露"
    );
    assert!(
        tools.iter().any(|t| t.qualified_name == "mcp__reverse__bash"),
        "内置工具 bash 应暴露"
    );

    // 调用 ls（无参数：默认工作目录）
    let result = manager
        .call_tool("mcp__reverse__ls", serde_json::json!({}))
        .await
        .expect("ls 调用失败");
    assert!(result.is_string() || result.is_array(), "ls 应返回内容");

    // 调用 read 读 Cargo.toml（相对路径按工作目录解析）
    let result = manager
        .call_tool(
            "mcp__reverse__read",
            serde_json::json!({ "path": "Cargo.toml" }),
        )
        .await
        .expect("read 调用失败");
    let text = result.as_str().unwrap_or_default();
    assert!(text.contains("[package]"), "read 应返回 Cargo.toml 内容");

    manager.disconnect("reverse").await.expect("断开失败");
}
