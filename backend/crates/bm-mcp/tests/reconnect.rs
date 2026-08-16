//! 集成测试：真实 stdio server（tests/fixtures/echo_server.mjs）端到端——
//! legacy 协商回退、工具枚举/调用、崩溃自动重连。
//!
//! fixture 是纯 Node 脚本（零 npm 依赖）；环境无 node 时跳过（CI 兜底）。

use std::path::PathBuf;
use std::time::Duration;

use bm_mcp::{McpClientManager, McpServerConfig, McpService, McpTransportKind};

fn fixture_path() -> Option<PathBuf> {
    let probe = std::process::Command::new("node")
        .arg("--version")
        .output()
        .ok()?;
    if !probe.status.success() {
        eprintln!("[skip] node 不可用，跳过 MCP 集成测试");
        return None;
    }
    Some(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/echo_server.mjs"),
    )
}

fn echo_config(js: PathBuf) -> McpServerConfig {
    McpServerConfig {
        name: "echo".into(),
        transport: McpTransportKind::Stdio,
        command: Some("node".into()),
        args: vec![js.to_string_lossy().into_owned()],
        env: Default::default(),
        url: None,
        headers: Default::default(),
        tool_timeout_ms: Some(10_000),
        scopes: vec![],
    }
}

#[tokio::test]
async fn connect_legacy_negotiate_list_and_call() {
    let Some(js) = fixture_path() else { return };
    let manager = McpClientManager::new();
    let handle = manager.connect(echo_config(js)).await.expect("连接失败");

    // Auto 模式：fixture 无 server/discover → 回退 legacy 握手
    assert_eq!(
        handle.protocol_version.read().unwrap().as_str(),
        "2025-11-25",
        "legacy 回退应协商出 2025-11-25"
    );

    let tools = manager.tools();
    assert_eq!(tools.len(), 2, "echo + crash");
    let echo = tools.iter().find(|t| t.qualified_name == "mcp__echo__echo").expect("echo 工具");
    assert_eq!(echo.name, "echo");
    assert_eq!(echo.server_name, "echo");

    let result = manager
        .call_tool("mcp__echo__echo", serde_json::json!({"text": "hi"}))
        .await
        .expect("调用失败");
    assert_eq!(result, serde_json::json!("echo:hi"));

    manager.disconnect("echo").await.expect("断开失败");
}

#[tokio::test]
async fn crash_triggers_supervisor_reconnect() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .try_init();
    let Some(js) = fixture_path() else { return };
    let manager = McpClientManager::new();
    let handle = manager.connect(echo_config(js)).await.expect("连接失败");
    assert_eq!(manager.tools().len(), 2);

    // 调用 crash → server 进程 exit(1) → transport 关闭 → supervisor 重连
    let _ = manager
        .call_tool("mcp__echo__crash", serde_json::json!({}))
        .await; // 调用本身可能因断连报错，忽略

    // 等重连（初始退避 500ms + is_transport_closed 轮询 500ms，给足余量）。
    // 判定用连接状态而非工具缓存——旧缓存未被清空，误判会提前退出
    let mut recovered = false;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if handle.is_connected().await {
            recovered = true;
            break;
        }
    }
    assert!(recovered, "崩溃后 supervisor 应在退避窗口内重连");
    // 重连后工具快照应恢复（supervisor 已刷新缓存）
    assert!(
        manager.tools().iter().any(|t| t.qualified_name == "mcp__echo__echo"),
        "重连后工具快照应恢复"
    );

    // 重连后的新进程可用
    let result = manager
        .call_tool("mcp__echo__echo", serde_json::json!({"text": "again"}))
        .await
        .expect("重连后调用失败");
    assert_eq!(result, serde_json::json!("echo:again"));

    manager.disconnect("echo").await.expect("断开失败");
}
