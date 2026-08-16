//! 标准 MCP 配置发现与解析（兼容性主体，吸收 pi-mcp-adapter / Warp 的
//! 多生态自动读取）：项目级 `.mcp.json`（Claude Code 格式）+ 用户级
//! `~/.agents/mcp.json`、`~/.config/mcp/mcp.json`，按标准 `mcpServers`
//! 对象解析为 [`McpServerConfig`]，env 展开 + HTTP url SSRF 校验。
//!
//! 优先级：显式配置（config.toml）> 项目 `.mcp.json` > `~/.agents/mcp.json`
//! > `~/.config/mcp/mcp.json`——同名 server 高优先级覆盖。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::{McpServerConfig, McpTransportKind};
use crate::expand::expand_config_strings;

/// 自动发现的配置来源（诊断/日志用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpConfigSource {
    /// 项目根 `.mcp.json`（随 git 共享，Claude Code 项目级格式）
    Project,
    /// `~/.agents/mcp.json`（pi 生态标准位置）
    Agents,
    /// `~/.config/mcp/mcp.json`（pi-mcp-adapter 标准位置）
    Config,
}

pub const PROJECT_MCP_FILE: &str = ".mcp.json";
pub const AGENTS_MCP_FILE: &str = "agents/mcp.json";
pub const CONFIG_MCP_FILE: &str = "config/mcp/mcp.json";

/// 按优先级返回自动发现的 server 配置（已展开 env、已校验 url）。
/// 返回 (server, 来源)；同名高优先级在前。
pub fn discover_servers(project_dir: &Path) -> Vec<(McpServerConfig, McpConfigSource)> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    let mut out = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut push_file = |path: PathBuf, source: McpConfigSource| {
        if !path.is_file() {
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_mcp_servers_file(&text) {
                Ok(servers) => {
                    for server in servers {
                        if seen.insert(server.name.clone()) {
                            tracing::info!(
                                event = "bm.mcp_discovered",
                                server = %server.name,
                                source = ?source,
                                path = %path.display(),
                            );
                            out.push((server, source));
                        }
                    }
                }
                Err(err) => tracing::warn!(
                    event = "bm.mcp_discover_invalid",
                    path = %path.display(),
                    error = %err,
                ),
            },
            Err(err) => tracing::debug!(event = "bm.mcp_discover_unreadable", path = %path.display(), error = %err),
        }
    };

    // 高优先级在前（调用方按序取用，遇重名跳过）
    push_file(project_dir.join(PROJECT_MCP_FILE), McpConfigSource::Project);
    if let Some(home) = &home {
        push_file(home.join(AGENTS_MCP_FILE), McpConfigSource::Agents);
        push_file(home.join(CONFIG_MCP_FILE), McpConfigSource::Config);
    }
    out
}

/// 解析标准 `mcpServers` 配置文件（Claude Code `.mcp.json` 格式）：
/// ```json
/// { "mcpServers": { "name": { "type": "stdio"|"http", "command": "...",
///     "args": [], "env": {}, "url": "...", "headers": {} } } }
/// ```
/// 无 `type` 但有 `command` → stdio；有 `url` → http。
/// `sse`/`ws` 类型已弃用，跳过并告警（不报错——存量配置含旧条目不阻断）。
pub fn parse_mcp_servers_file(text: &str) -> Result<Vec<McpServerConfig>, String> {
    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let servers = root
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "缺少顶层 `mcpServers` 对象".to_string())?;

    let mut out = Vec::new();
    for (name, entry) in servers {
        match parse_server_entry(name, entry) {
            Ok(config) => out.push(config),
            Err(err) => tracing::warn!(event = "bm.mcp_entry_skipped", server = %name, error = %err),
        }
    }
    Ok(out)
}

fn parse_server_entry(name: &str, entry: &serde_json::Value) -> Result<McpServerConfig, String> {
    let obj = entry
        .as_object()
        .ok_or_else(|| "条目必须是对象".to_string())?;
    // 标准格式用 `type`；TS 插件注册面（pi.registerMcpServer）用 `transport`
    let entry_type = obj
        .get("type")
        .or_else(|| obj.get("transport"))
        .and_then(|v| v.as_str());
    let command = obj.get("command").and_then(|v| v.as_str()).map(str::to_string);
    let url = obj.get("url").and_then(|v| v.as_str()).map(str::to_string);

    let transport = match entry_type {
        Some("stdio") | None if command.is_some() => McpTransportKind::Stdio,
        Some("http") => McpTransportKind::Http,
        // legacy SSE 端点（GET /sse + POST /messages）已弃用（SEP-2596，
        // 12 个月后移除）；rmcp 3 的 streamable-http client 只处理该传输
        // 内部的 SSE 响应流，不支持纯 legacy 双端点模式。存量 SSE server
        // 请升级 streamable HTTP 或经 stdio 接入——如实拒绝而非虚假支持。
        Some("sse") => {
            return Err("sse 传输已弃用且当前 client 不支持 legacy SSE 端点（升级 server 至 streamable HTTP，或改用 stdio）".to_string());
        }
        Some("ws") => return Err("传输 `ws` 不支持（主流生态亦未采用）".to_string()),
        Some(other) => return Err(format!("未知传输类型 `{other:?}`")),
        None if url.is_some() => McpTransportKind::Http,
        None => return Err("既无 command 也无 url".to_string()),
    };

    let mut config = McpServerConfig {
        name: name.to_string(),
        transport,
        command,
        args: obj
            .get("args")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        env: obj
            .get("env")
            .and_then(|v| v.as_object())
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default(),
        url,
        headers: obj
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default(),
        tool_timeout_ms: None,
        scopes: obj
            .get("scopes")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    };
    config
        .validate()
        .map_err(|e| format!("server 名非法: {e}"))?;
    // env 展开（${VAR} / ${VAR:-default}）
    expand_config_strings(
        &mut config.command,
        &mut config.args,
        &mut config.env,
        &mut config.url,
        &mut config.headers,
    );
    // HTTP server 的 url 必须通过 SSRF 校验（与提供商端点同语义：
    // 私网/链路本地拦截，localhost 放行——本地模型服务是合法场景）
    if config.transport == McpTransportKind::Http
        && let Some(u) = &config.url
    {
        validate_http_url(u).map_err(|e| format!("url 校验失败: {e}"))?;
    }
    Ok(config)
}

/// 解析 TS 插件注册面（`pi.registerMcpServer`）的 spec：
/// `{ name, transport?, command?, url?, args?, env? }`。
pub fn parse_ts_server_spec(spec: &serde_json::Value) -> Result<McpServerConfig, String> {
    let name = spec
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "缺少 name".to_string())?;
    parse_server_entry(name, spec)
}

/// SSRF 防护（语义对齐 bm-core providers.rs `validate_base_url`）：
/// 必须 http(s)；localhost/回环放行（本地 server 合法）；私网/链路本地/
/// 未指定地址拦截（防局域网横向与云元数据探测）。
pub fn validate_http_url(url: &str) -> Result<(), String> {
    let parsed =
        url::Url::parse(url).map_err(|_| "必须是完整的 http(s):// URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("只支持 http/https".to_string());
    }
    let Some(host) = parsed.host_str() else {
        return Err("缺少主机名".to_string());
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(());
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if ip.is_loopback() {
            return Ok(());
        }
        let blocked = match ip {
            std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_unspecified(),
            std::net::IpAddr::V6(v6) => {
                v6.is_unique_local() || v6.is_unicast_link_local() || v6.is_unspecified()
            }
        };
        if blocked {
            return Err(format!("不允许指向私网/本机地址（{ip}）"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_stdio_entry() {
        let text = r#"{
            "mcpServers": {
                "fs": {
                    "type": "stdio",
                    "command": "node",
                    "args": ["server.js", "${WORKDIR:-.}"],
                    "env": { "TOKEN": "${MCP_FS_TOKEN}" }
                }
            }
        }"#;
        // env 展开依赖宿主 env；MCP_FS_TOKEN 未定义 → 空串
        let servers = parse_mcp_servers_file(text).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "fs");
        assert_eq!(servers[0].transport, McpTransportKind::Stdio);
        assert_eq!(servers[0].command.as_deref(), Some("node"));
        assert_eq!(servers[0].args[1], ".");
        assert_eq!(servers[0].env.get("TOKEN").map(String::as_str), Some(""));
    }

    #[test]
    fn parse_http_entry_with_headers() {
        let text = r#"{
            "mcpServers": {
                "remote": {
                    "type": "http",
                    "url": "https://example.com/mcp",
                    "headers": { "Authorization": "Bearer ${TOKEN:-}" }
                }
            }
        }"#;
        let servers = parse_mcp_servers_file(text).unwrap();
        assert_eq!(servers[0].transport, McpTransportKind::Http);
        assert_eq!(servers[0].url.as_deref(), Some("https://example.com/mcp"));
        assert_eq!(servers[0].headers.get("Authorization").map(String::as_str), Some("Bearer "));
    }

    #[test]
    fn url_implies_http_without_type() {
        let text = r#"{ "mcpServers": { "r": { "url": "https://x.dev/mcp" } } }"#;
        let servers = parse_mcp_servers_file(text).unwrap();
        assert_eq!(servers[0].transport, McpTransportKind::Http);
    }

    #[test]
    fn deprecated_sse_ws_skipped_not_fatal() {
        let text = r#"{
            "mcpServers": {
                "old-ws": { "type": "ws", "url": "wss://x.dev/mcp" },
                "legacy-sse": { "type": "sse", "url": "https://x.dev/sse" },
                "new": { "type": "stdio", "command": "npx" }
            }
        }"#;
        let servers = parse_mcp_servers_file(text).unwrap();
        assert_eq!(servers.len(), 1, "ws 与 legacy sse 跳过，stdio 保留");
        assert_eq!(servers[0].name, "new");
    }

    #[test]
    fn missing_mcp_servers_key_is_error() {
        assert!(parse_mcp_servers_file(r#"{ "other": 1 }"#).is_err());
    }

    #[test]
    fn ssrf_blocks_private_but_allows_localhost() {
        assert!(validate_http_url("http://localhost:8000/mcp").is_ok());
        assert!(validate_http_url("http://127.0.0.1:8000/mcp").is_ok());
        assert!(validate_http_url("https://example.com/mcp").is_ok());
        assert!(validate_http_url("http://192.168.1.5/mcp").is_err());
        assert!(validate_http_url("http://10.0.0.1/mcp").is_err());
        assert!(validate_http_url("http://169.254.169.254/meta").is_err(), "云元数据探测必须拦截");
        assert!(validate_http_url("ftp://example.com").is_err());
        assert!(validate_http_url("not-a-url").is_err());
    }
}
