//! BoenMind 默认官方 MCP 插件核心：MCP client。
//!
//! 职责：连接外部 MCP server（stdio / streamable HTTP），枚举工具并合入
//! 模型工具面，转发工具调用。协议目标 2026-07-28（无状态核心），
//! 经 [`rmcp::ClientLifecycleMode::Auto`] 对存量 2025 legacy server 回退。

pub mod client;
pub mod config;

pub use client::{
    McpClientManager, McpError, McpServerHandle, McpServerInfo, McpService, McpToolDef,
    qualify_tool_name,
};
pub use config::{McpServerConfig, McpTransportKind};
