use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// MCP server 的传输方式。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum McpTransportKind {
    /// 子进程 stdio（本地 server，标准配置格式 `command`/`args`/`env`）。
    Stdio,
    /// Streamable HTTP（远程 server，标准配置格式 `url`/`headers`）。
    Http,
}

/// 单个 MCP server 的连接配置。
///
/// 字段形状与主流生态的 `mcpServers` 条目保持一致（Claude Code `.mcp.json`
/// 为基准），保证配置可直接移植。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// 唯一 server 名（工具名前缀来源，限 `[A-Za-z0-9_-]`）。
    pub name: String,
    pub transport: McpTransportKind,
    /// stdio：可执行程序（如 `node`、`uvx`、`npx`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// 传给子进程的环境变量（附加到继承环境之上）。
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// http：server 端点。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// 单次工具调用超时（毫秒），默认 60s。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_timeout_ms: Option<u64>,
    /// 作用域（设置架构 §八）：生效的 APP 列表。
    /// 空/含 "*" = 公共（所有 APP 会话工具面可见）；["chat"] = 仅聊天 APP。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

impl McpServerConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("server 名不能为空".into());
        }
        if !self
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(format!("server 名 `{}` 含非法字符（限字母/数字/_/-）", self.name));
        }
        match self.transport {
            McpTransportKind::Stdio => {
                if self.command.as_deref().unwrap_or_default().is_empty() {
                    return Err(format!("server `{}`（stdio）缺少 command", self.name));
                }
            }
            McpTransportKind::Http => {
                if self.url.as_deref().unwrap_or_default().is_empty() {
                    return Err(format!("server `{}`（http）缺少 url", self.name));
                }
            }
        }
        Ok(())
    }
}
