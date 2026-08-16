use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use bm_protocol::BoxFuture;
use rmcp::model::{CallToolRequestParams, ContentBlock, ProtocolVersion, Tool};
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{ClientLifecycleMode, ClientServiceExt, RoleClient};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

use crate::config::{McpServerConfig, McpTransportKind};

/// 工具命名契约（吸收 dsh mcp-client）：`mcp__<server>__<tool>`，
/// 总长 64 字符，超出则截断并追加 12 位确定性哈希防撞。
pub const TOOL_NAME_MAX: usize = 64;
const TOOL_HASH_LEN: usize = 12;

/// 一个已连接的 MCP server 的运行句柄。
pub struct McpServerHandle {
    pub config: McpServerConfig,
    /// rmcp 运行中的 client 服务（Deref 到 `Peer<RoleClient>`）。
    /// 重连时整体替换（supervisor 任务持有同一 Arc）。
    running: Mutex<rmcp::service::RunningService<RoleClient, ()>>,
    /// 协商成功的协议版本（dual-era 协商结果；重连后更新）。
    pub protocol_version: std::sync::RwLock<ProtocolVersion>,
    /// 主动断开标志（disconnect 置位；supervisor 见 true 退出，不重连）。
    disconnected: std::sync::atomic::AtomicBool,
}

impl McpServerHandle {
    /// 是否已主动断开（诊断/测试用）。
    pub fn disconnected(&self) -> bool {
        self.disconnected.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 传输是否已关闭（崩溃/断开检测；重连完成后为 false）。
    pub async fn is_connected(&self) -> bool {
        !self.running.lock().await.is_transport_closed()
    }
}

/// 重连策略参数（吸收 dsh mcp-client）：指数退避 500ms → 30s，
/// 连续失败 10 次熔断（不再重连，日志告警）。
const RECONNECT_BACKOFF_INITIAL_MS: u64 = 500;
const RECONNECT_BACKOFF_MAX_MS: u64 = 30_000;
const RECONNECT_MAX_FAILURES: u32 = 10;

/// 合入模型工具面的 MCP 工具定义。
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub server_name: String,
    /// server 内原始工具名（wire 上调用时使用）。
    pub name: String,
    /// 模型侧公开名：`mcp__<server>__<tool>`（规范化 + 哈希防撞）。
    pub qualified_name: String,
    pub description: String,
    /// 透传的 JSON Schema（MCP `inputSchema`）。
    pub input_schema: serde_json::Value,
}

/// 服务面：MCP 能力的消费接口（bm-server 组装层经 kernel port "mcp" 取用）。
///
/// 工具快照为同步读（执行线程用）；`call_tool` 按模型侧
/// `qualified_name` 反查定位（规避 server/tool 名内含 `__` 的解析歧义）。
pub trait McpService: Send + Sync {
    /// 全部已连接 server 的工具快照（connect/refresh 后刷新）。
    fn tools(&self) -> Vec<McpToolDef>;
    /// 已连接 server 列表与协商版本。
    fn servers(&self) -> Vec<McpServerInfo>;
    /// 调用工具（按 `qualified_name`）。
    fn call_tool(
        &self,
        qualified_name: &str,
        arguments: serde_json::Value,
    ) -> BoxFuture<'_, Result<serde_json::Value, String>>;
}

/// server 状态快照（设置页/诊断用）。
#[derive(Debug, Clone)]
pub struct McpServerInfo {
    pub name: String,
    /// 协商成功的协议版本（如 "2026-07-28" / "2025-11-25"）。
    pub protocol_version: String,
    pub transport: McpTransportKind,
}

/// MCP client 管理器：server 注册表 + 连接 + 工具枚举 + 调用。
#[derive(Default)]
pub struct McpClientManager {
    servers: RwLock<HashMap<String, Arc<McpServerHandle>>>,
    /// 工具快照缓存（connect/refresh 后刷新；std 锁——执行线程可能在
    /// tokio 上下文调用同步面，不能用 blocking_*）。Arc 共享给重连
    /// supervisor 任务更新。
    tools_cache: Arc<std::sync::RwLock<Vec<McpToolDef>>>,
    /// server 状态快照（同步读）。
    servers_info: Arc<std::sync::RwLock<Vec<McpServerInfo>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("server 配置非法: {0}")]
    InvalidConfig(String),
    #[error("server `{0}` 已连接")]
    AlreadyConnected(String),
    #[error("server `{0}` 未连接")]
    NotConnected(String),
    #[error("连接失败: {0}")]
    Connect(String),
    #[error("工具 `{0}` 不存在（未连接或未刷新）")]
    ToolNotFound(String),
    #[error("工具调用失败: {0}")]
    Call(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
}

impl McpClientManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 连接一个 MCP server（stdio spawn 或 streamable HTTP），
    /// 以 dual-era 协商协议版本，成功后刷新工具快照。
    pub async fn connect(&self, config: McpServerConfig) -> Result<Arc<McpServerHandle>, McpError> {
        config.validate().map_err(McpError::InvalidConfig)?;

        {
            let servers = self.servers.read().await;
            if servers.contains_key(&config.name) {
                return Err(McpError::AlreadyConnected(config.name.clone()));
            }
        }

        let lifecycle = ClientLifecycleMode::Auto {
            // 首选 2026-07-28（MCP 2.0 无状态核心），rmcp 探测 `server/discover`，
            // 失败自动回退 legacy 握手（`Auto` 语义）。
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            legacy_version: Some(ProtocolVersion::V_2025_11_25),
        };
        let (running, protocol_version) = establish(&config, lifecycle).await?;
        info!(server = %config.name, version = %protocol_version, "MCP server 已连接");

        let handle = Arc::new(McpServerHandle {
            config,
            running: Mutex::new(running),
            protocol_version: std::sync::RwLock::new(protocol_version),
            disconnected: std::sync::atomic::AtomicBool::new(false),
        });
        self.servers.write().await.insert(handle.config.name.clone(), handle.clone());
        self.refresh().await?;
        self.sync_servers_info().await;
        // 崩溃重连监督（吸收 dsh mcp-client：指数退避 + 熔断）；主动
        // disconnect 置位后 supervisor 自然退出
        self.spawn_supervisor(handle.clone());
        Ok(handle)
    }

    pub async fn disconnect(&self, server: &str) -> Result<(), McpError> {
        let handle = self
            .servers
            .write()
            .await
            .remove(server)
            .ok_or_else(|| McpError::NotConnected(server.to_string()))?;
        // 先置位再 close：supervisor 看到主动断开标志即退出（不重连）
        handle.disconnected.store(true, std::sync::atomic::Ordering::SeqCst);
        handle
            .running
            .lock()
            .await
            .close()
            .await
            .map_err(|e| McpError::Call(format!("关闭失败: {e}")))?;
        info!(server, "MCP server 已断开");
        self.refresh().await?;
        self.sync_servers_info().await;
        Ok(())
    }

    /// 崩溃重连监督任务：等连接退出（is_closed 轮询）→ 指数退避重连 →
    /// 成功替换 running 并刷新该 server 工具快照；连续失败 10 次熔断。
    fn spawn_supervisor(&self, handle: Arc<McpServerHandle>) {
        let tools_cache = self.tools_cache.clone();
        let servers_info = self.servers_info.clone();
        tokio::spawn(async move {
            let mut backoff_ms = RECONNECT_BACKOFF_INITIAL_MS;
            let mut failures = 0u32;
            loop {
                // 等连接退出（500ms 轮询；用 is_transport_closed——被动断开
                // 时 is_closed 不会置位，只有显式 close/cancel 才置位）
                loop {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    let closed = handle.running.lock().await.is_transport_closed();
                    if closed {
                        break;
                    }
                }
                if handle.disconnected.load(std::sync::atomic::Ordering::SeqCst) {
                    break; // 主动断开，不重连
                }
                warn!(server = %handle.config.name, "MCP server 连接退出，尝试重连");
                match try_reconnect(&handle.config).await {
                    Ok((running, version)) => {
                        *handle.running.lock().await = running;
                        *handle.protocol_version.write().unwrap() = version.clone();
                        // 工具快照：该 server 旧条目移除 + 新条目并入
                        match refresh_one(&handle).await {
                            Ok(tools) => {
                                let mut cache = tools_cache.write().unwrap();
                                cache.retain(|t| t.server_name != handle.config.name);
                                cache.extend(tools);
                            }
                            Err(err) => warn!(server = %handle.config.name, error = %err, "重连后工具快照刷新失败"),
                        }
                        {
                            let mut info = servers_info.write().unwrap();
                            if let Some(entry) = info.iter_mut().find(|i| i.name == handle.config.name) {
                                entry.protocol_version = version.to_string();
                            }
                        }
                        info!(server = %handle.config.name, version = %version, "MCP server 重连成功");
                        backoff_ms = RECONNECT_BACKOFF_INITIAL_MS;
                        failures = 0;
                    }
                    Err(err) => {
                        failures += 1;
                        if failures >= RECONNECT_MAX_FAILURES {
                            warn!(server = %handle.config.name, "MCP server 重连熔断（连续失败 {failures} 次），停止自动重连");
                            break;
                        }
                        warn!(server = %handle.config.name, backoff_ms, error = %err, "重连失败，退避后重试");
                        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                        backoff_ms = (backoff_ms * 2).min(RECONNECT_BACKOFF_MAX_MS);
                    }
                }
            }
        });
    }

    /// 重新枚举全部已连接 server 的工具（`tools/list` → 快照）。
    pub async fn refresh(&self) -> Result<(), McpError> {
        let mut out = Vec::new();
        for handle in self.servers().await {
            match refresh_one(&handle).await {
                Ok(tools) => out.extend(tools),
                Err(err) => tracing::warn!(event = "bm.mcp_refresh_failed", server = %handle.config.name, error = %err),
            }
        }
        *self.tools_cache.write().unwrap() = out;
        Ok(())
    }

    async fn sync_servers_info(&self) {
        let mut out = Vec::new();
        for handle in self.servers().await {
            out.push(McpServerInfo {
                name: handle.config.name.clone(),
                protocol_version: handle.protocol_version.read().unwrap().to_string(),
                transport: handle.config.transport,
            });
        }
        *self.servers_info.write().unwrap() = out;
    }

    pub async fn server(&self, server: &str) -> Option<Arc<McpServerHandle>> {
        self.servers.read().await.get(server).cloned()
    }

    pub async fn servers(&self) -> Vec<Arc<McpServerHandle>> {
        self.servers.read().await.values().cloned().collect()
    }
}

impl McpService for McpClientManager {
    fn tools(&self) -> Vec<McpToolDef> {
        self.tools_cache.read().unwrap().clone()
    }

    fn servers(&self) -> Vec<McpServerInfo> {
        self.servers_info.read().unwrap().clone()
    }

    fn call_tool(
        &self,
        qualified_name: &str,
        arguments: serde_json::Value,
    ) -> BoxFuture<'_, Result<serde_json::Value, String>> {
        let qualified_name = qualified_name.to_string(); // owned，规避 async move 捕获引用
        Box::pin(async move {
            let result: Result<serde_json::Value, McpError> = async {
                let target = self
                    .tools_cache
                    .read()
                    .unwrap()
                    .iter()
                    .find(|t| t.qualified_name == qualified_name)
                    .map(|t| (t.server_name.clone(), t.name.clone()))
                    .ok_or_else(|| McpError::ToolNotFound(qualified_name.to_string()))?;

                let (server, tool) = target;
                let handle = self
                    .server(&server)
                    .await
                    .ok_or_else(|| McpError::NotConnected(server.clone()))?;
                let timeout = handle
                    .config
                    .tool_timeout_ms
                    .map(Duration::from_millis)
                    .unwrap_or(Duration::from_secs(60));
                let running = handle.running.lock().await;
                let arguments = arguments.as_object().cloned().ok_or_else(|| {
                    McpError::Call(format!("工具参数必须是 JSON 对象，收到: {arguments}"))
                })?;
                let result = tokio::time::timeout(
                    timeout,
                    running.call_tool(
                        CallToolRequestParams::new(tool.clone()).with_arguments(arguments),
                    ),
                )
                .await
                .map_err(|_| {
                    McpError::Call(format!(
                        "server `{server}` 工具 `{tool}` 调用超时（{timeout:?}）"
                    ))
                })?
                .map_err(|e| {
                    McpError::Call(format!("server `{server}` 工具 `{tool}` 调用失败: {e}"))
                })?;
                Ok(call_result_to_json(result))
            }
            .await;
            result.map_err(|e| e.to_string())
        })
    }
}

/// 建立与一个 server 的连接（stdio spawn 或 streamable HTTP）+ dual-era 协商。
/// 供 connect 与重连 supervisor 共用。
async fn establish(
    config: &McpServerConfig,
    lifecycle: ClientLifecycleMode,
) -> Result<
    (
        rmcp::service::RunningService<RoleClient, ()>,
        ProtocolVersion,
    ),
    McpError,
> {
    let running = match config.transport {
        McpTransportKind::Stdio => {
            let command = config.command.as_deref().unwrap();
            let mut cmd = tokio::process::Command::new(command);
            cmd.args(&config.args).envs(&config.env);
            debug!(server = %config.name, %command, args = ?config.args, "spawn MCP stdio server");
            ()
                .serve_with_lifecycle(TokioChildProcess::new(cmd)?, lifecycle)
                .await
        }
        McpTransportKind::Http => {
            let url = config.url.as_deref().unwrap();
            let transport = StreamableHttpClientTransport::from_uri(url);
            ()
                .serve_with_lifecycle(transport, lifecycle)
                .await
        }
    }
    .map_err(|e| McpError::Connect(format!("{}: {e}", config.name)))?;
    let protocol_version = running
        .peer_info()
        .map(|info| info.protocol_version.clone())
        .unwrap_or_default();
    Ok((running, protocol_version))
}

/// 枚举单个 server 的工具（`tools/list` → 快照条目）。
async fn refresh_one(
    handle: &McpServerHandle,
) -> Result<Vec<McpToolDef>, McpError> {
    let running = handle.running.lock().await;
    let tools = running
        .list_all_tools()
        .await
        .map_err(|e| McpError::Call(format!("server `{}` tools/list 失败: {e}", handle.config.name)))?;
    Ok(tools
        .into_iter()
        .map(|t| to_mcp_tool_def(&handle.config.name, t))
        .collect())
}

/// 重连入口（supervisor 用）：与 establish 同构，语义分离便于日志。
async fn try_reconnect(
    config: &McpServerConfig,
) -> Result<
    (
        rmcp::service::RunningService<RoleClient, ()>,
        ProtocolVersion,
    ),
    McpError,
> {
    let lifecycle = ClientLifecycleMode::Auto {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        legacy_version: Some(ProtocolVersion::V_2025_11_25),
    };
    establish(config, lifecycle).await
}

/// 工具名规范化：`mcp__<server>__<tool>`，超长截断 + 12 位哈希防撞
/// （确定性纯函数：同输入恒同输出，server 顺序无关，重连后名字稳定）。
pub fn qualify_tool_name(server: &str, tool: &str) -> String {
    let raw = format!("mcp__{server}__{tool}");
    if raw.len() <= TOOL_NAME_MAX {
        return raw;
    }
    let hash = stable_hex_hash(&raw);
    let keep = TOOL_NAME_MAX - TOOL_HASH_LEN - 2; // 留 `__` 分隔
    format!("{}__{hash}", &raw[..keep])
}

fn stable_hex_hash(input: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:012x}", hasher.finish())[..TOOL_HASH_LEN].to_string()
}

fn to_mcp_tool_def(server: &str, tool: Tool) -> McpToolDef {
    let name = tool.name.to_string();
    McpToolDef {
        server_name: server.to_string(),
        qualified_name: qualify_tool_name(server, &name),
        name,
        description: tool.description.unwrap_or_default().into_owned(),
        input_schema: serde_json::to_value(tool.input_schema).unwrap_or(serde_json::json!({})),
    }
}

/// 把 MCP 工具调用结果转成对模型友好的 JSON：
/// 优先 `structuredContent`，否则拼装文本内容。
fn call_result_to_json(result: rmcp::model::CallToolResult) -> serde_json::Value {
    if let Some(content) = result.structured_content {
        return serde_json::to_value(content).unwrap_or(serde_json::json!({}));
    }
    let mut texts = Vec::new();
    for part in result.content {
        match part {
            ContentBlock::Text(t) => texts.push(t.text),
            ContentBlock::Image(img) => {
                warn!(mime = %img.mime_type, "MCP 工具返回图片内容，已跳过（模型侧不可见）");
            }
            ContentBlock::Audio(a) => {
                warn!(mime = %a.mime_type, "MCP 工具返回音频内容，已跳过");
            }
            other => {
                warn!(content = ?other, "MCP 工具返回未识别内容类型");
            }
        }
    }
    if texts.len() == 1 {
        serde_json::Value::String(texts.pop().unwrap())
    } else {
        serde_json::Value::Array(texts.into_iter().map(serde_json::Value::String).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpServerConfig;

    #[test]
    fn qualify_short_name_passthrough() {
        assert_eq!(
            qualify_tool_name("fs", "read_file"),
            "mcp__fs__read_file"
        );
    }

    #[test]
    fn qualify_long_name_hashed_stable() {
        let long_server = "a".repeat(40);
        let long_tool = "tool".repeat(20); // 80 字符
        let a = qualify_tool_name(&long_server, &long_tool);
        let b = qualify_tool_name(&long_server, &long_tool);
        assert_eq!(a, b, "同输入必须同输出（确定性）");
        assert!(a.len() <= TOOL_NAME_MAX, "超长名必须截断到契约长度");
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        // 不同输入不同哈希（防撞）
        let c = qualify_tool_name(&long_server, &format!("{long_tool}x"));
        assert_ne!(a, c);
    }

    #[test]
    fn qualify_differs_by_server_and_order_independent() {
        let ab = qualify_tool_name("a_b", "c");
        let ba = qualify_tool_name("a", "b_c");
        assert_ne!(ab, ba, "server/tool 边界不得混淆");
    }

    #[test]
    fn config_validate() {
        let ok = McpServerConfig {
            name: "fs".into(),
            transport: McpTransportKind::Stdio,
            command: Some("node".into()),
            args: vec![],
            env: Default::default(),
            url: None,
            headers: Default::default(),
            tool_timeout_ms: None,
        };
        assert!(ok.validate().is_ok());

        let bad_name = McpServerConfig {
            name: "bad name!".into(),
            ..ok.clone()
        };
        assert!(bad_name.validate().is_err());

        let no_cmd = McpServerConfig {
            command: None,
            ..ok.clone()
        };
        assert!(no_cmd.validate().is_err());

        let no_url = McpServerConfig {
            name: "http1".into(),
            transport: McpTransportKind::Http,
            command: None,
            args: vec![],
            env: Default::default(),
            url: None,
            headers: Default::default(),
            tool_timeout_ms: None,
        };
        assert!(no_url.validate().is_err());
    }

    #[test]
    fn result_extraction_prefers_structured_content() {
        let result: rmcp::model::CallToolResult = serde_json::from_value(serde_json::json!({
            "content": [{ "type": "text", "text": "plain" }],
            "structuredContent": { "greeting": "hi" },
        }))
        .unwrap();
        let v = call_result_to_json(result);
        assert_eq!(v, serde_json::json!({"greeting": "hi"}));
    }

    #[test]
    fn result_extraction_text_only() {
        let result: rmcp::model::CallToolResult = serde_json::from_value(serde_json::json!({
            "content": [{ "type": "text", "text": "hello" }],
        }))
        .unwrap();
        assert_eq!(call_result_to_json(result), serde_json::json!("hello"));
    }
}
