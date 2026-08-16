//! 反向 MCP server（`bm-server --mcp-serve`，吸收 Claude Code `claude mcp
//! serve` 模式）：把内置工具面（read/write/edit/grep/find/ls/bash——与
//! 主引擎工具面同款）暴露成 MCP server（stdio），供外部 MCP client
//! （Claude Code / Claude Desktop / Cursor 等）经 `mcpServers` 配置接入。
//!
//! 权限治理：反向暴露的工具由**外部客户端**的权限系统治理（与 subagent
//! 子进程模式同理）；本模式不初始化 tracing（stdout 是 MCP 协议通道）。

use std::sync::Arc;

use bm_mcp::serve::McpServeTool;

/// 以 stdio MCP server 身份运行，直到外部 client 断开。
pub async fn run() -> Result<(), String> {
    // 工作目录 = 当前进程目录（外部 client 通常在项目根 spawn）
    let working_dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let tools: Vec<McpServeTool> = crate::builtin_tools::BuiltinTools::definitions()
        .into_iter()
        .map(|def| {
            let name = def.name.clone();
            // BuiltinTools 只含 cwd（PathBuf 可 Clone）——闭包内重建
            let cwd = working_dir.clone();
            McpServeTool {
                name: def.name,
                description: def.description,
                input_schema: def.input_schema,
                execute: Arc::new(move |args| {
                    let cwd = cwd.clone();
                    let name = name.clone();
                    Box::pin(async move {
                        let executor = crate::builtin_tools::BuiltinTools::new(cwd);
                        match executor.execute(&name, args).await {
                            Ok(value) => Ok(value),
                            Err(err) => Err(err.to_string()),
                        }
                    })
                }),
            }
        })
        .collect();
    tracing::warn!(event = "bm.mcp_serve_start", tools = tools.len(), "反向 MCP server 启动（stdio）");
    bm_mcp::serve::serve_stdio(tools).await
}
