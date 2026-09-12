//! MCP(Model Context Protocol)接入(M7.2/M7.3/M7.5;M7 规格 S3/S4)。
//!
//! 分层:传输(McpTransport,JSON-RPC 2.0 承载)→ Hub(握手/发现/路由/
//! manifest 生成/异步执行器实现)。内核不感知 MCP——只依赖
//! `AsyncCapabilityExecutor` 端口;`manifest.provider = "mcp.<server>"`
//! 是异步路由标记。
//!
//! 脱敏纪律(INV-5):传输错误只携带类别描述,不携带报文原文。

use async_trait::async_trait;
use bm_contract::capability::CapabilityManifest;
use bm_core::ports::{AsyncCallError, AsyncCapabilityExecutor, ProgressNotice};
use bm_core::registry::CapabilityProvider;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const DEFAULT_TOOL_TIMEOUT_MS: u64 = 30_000;

/// 已知 MCP 协议版本(spec );握手协商与
/// SSE 解析共用(未知版本告警不拒——工具面跨版本兼容,拒了反断可用网关)。
pub(crate) const KNOWN_PROTOCOL_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];
/// MCP 协议版本(与 SDK 单源);握手 `initialize` 用。
pub use boenmind_plugin_sdk::MCP_PROTOCOL_VERSION;
/// 宿主客户端协议 codec(#60,ADR-0034 §4):信封构造/响应解析与插件 SDK
/// 单源,消除 JSON-RPC 字面量与错误码在此模块的多处手拼。
use boenmind_plugin_sdk::client as rpc;
/// 带限时的 stdio 帧写入(write_all + flush),所有持锁写管道路径的统一出口
/// (限时走 limits:子进程挂起(非崩溃)时调用方持锁跨 await,无超时则该
/// 域能力永久坏死,respawn 也拿不到锁)。
async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(
    stdin: &mut W,
    bytes: &[u8],
    timeout: Duration,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    tokio::time::timeout(timeout, stdin.write_all(bytes))
        .await
        .map_err(|_| "stdio 写超时(子进程未消费,判定挂起)".to_string())?
        .map_err(|e| format!("stdio 写失败: {e}"))?;
    stdin
        .flush()
        .await
        .map_err(|e| format!("stdio 写失败: {e}"))
}

// ---- stderr 采集(issue #28)--------------------------------------------------

/// 子进程 stderr 环形缓冲容量(行)。跨 respawn 共享,足够排障又不无限膨胀。
const MCP_STDERR_CAPACITY: usize = 400;

// ADR-0046:`StderrLine` 上移 core(`ports::mcp_admin`),此处 re-export 保持公共路径。
pub use bm_core::ports::mcp_admin::StderrLine;

/// stdio 子进程 stderr 环形缓冲:管道采集替代 `Stdio::inherit()` 直通,
/// 跨 respawn 保留最近 [`MCP_STDERR_CAPACITY`] 行并按代标记,供管理面回看。
#[derive(Default)]
pub struct StderrBuffer {
    lines: Mutex<std::collections::VecDeque<StderrLine>>,
    capacity: usize,
}

impl StderrBuffer {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            lines: Mutex::new(std::collections::VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    fn push(&self, generation: u64, text: String) {
        let mut q = self
            .lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if q.len() >= self.capacity {
            q.pop_front();
        }
        q.push_back(StderrLine { generation, text });
    }

    /// 取最近 n 行(旧→新)。
    pub fn tail(&self, n: usize) -> Vec<StderrLine> {
        let q = self
            .lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let skip = q.len().saturating_sub(n);
        q.iter().skip(skip).cloned().collect()
    }
}

// ---- 传输端口 --------------------------------------------------------------

#[async_trait]
pub trait McpTransport: Send + Sync {
    /// JSON-RPC 请求-响应(错误 = 传输层故障描述,已脱敏)。
    async fn request(&self, method: &str, params: Value) -> Result<Value, String>;
    /// JSON-RPC 通知(无响应)。
    async fn notify(&self, method: &str, params: Value) -> Result<(), String>;
    /// 订阅服务端通知流(进度)。每连接取一次(先到先得)。
    fn subscribe_progress(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>;

    /// 按进度令牌取消在途请求(MCP notifications/cancelled;尽力终止)。
    fn cancel_by_token(&self, _token: &str) {}

    /// 子进程 stderr 环形缓冲(issue #28;stdio 专属,远程传输恒 None)。
    fn stderr_buffer(&self) -> Option<Arc<StderrBuffer>> {
        None
    }

    /// #3:握手期记录 initialize 结果(默认忽略;远程传输记录供管理面露出)。
    fn remember_init(&self, _v: Value) {}

    /// #3:读取记录的 initialize 结果(默认 None)。
    fn init_snapshot(&self) -> Option<Value> {
        None
    }
}

fn dead_progress_rx() -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
    rx
}

/// 进度订阅单次取走(stdio/http 两传输同实现):首次调用返回真实通道,
/// 之后恒返回已关闭的哑通道(take 后为 None)。
fn take_progress_rx(
    cell: &Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>>>,
) -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
    cell.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .unwrap_or_else(dead_progress_rx)
}

// ---- Hub:握手/发现/路由/异步执行器 -----------------------------------------

type ProgressSink = Box<dyn Fn(ProgressNotice) + Send + Sync>;

/// 在途请求表(request 注册,读取泵按 id 配对摘除)。
type PendingMap = Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, String>>>>>;

struct Route {
    transport: Arc<dyn McpTransport>,
    tool: String,
}

/// MCP Hub:多 server 路由 + `AsyncCapabilityExecutor` 端口实现。
pub struct McpHub {
    routes: Mutex<HashMap<String, Route>>,
    sink: Arc<Mutex<Option<ProgressSink>>>,
    /// 在途调用:operation_id → 传输(取消通知定位)。
    inflight: Mutex<HashMap<String, Arc<dyn McpTransport>>>,
}

impl McpHub {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::new_inner())
    }

    fn new_inner() -> Self {
        Self {
            routes: Mutex::new(HashMap::new()),
            sink: Arc::new(Mutex::new(None)),
            inflight: Mutex::new(HashMap::new()),
        }
    }

    /// 握手 + 发现:initialize → initialized → tools/list → 生成 manifests
    /// 并建立路由。不合规工具名跳过(拒注册,tracing 留痕)。
    /// P1-10: 整体握手包 15s 超时守卫,防止异常插件挂死热重载或服务启动
    pub async fn connect(
        self: &Arc<Self>,
        server: &str,
        transport: Arc<dyn McpTransport>,
        tool_timeout_ms: u64,
    ) -> Result<Vec<CapabilityManifest>, String> {
        let handshake = async {
            let init = transport
                .request("initialize", rpc::initialize_params("boenmind", "0.1"))
                .await?;
            let version = init
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if version.is_empty() {
                return Err("initialize 响应缺 protocolVersion".into());
            }
            // #3:协商——响应版本不在已知集则告警不拒(工具调用面版本间兼容)
            if !KNOWN_PROTOCOL_VERSIONS.contains(&version.as_str()) {
                tracing::warn!(target: "plugin", server, version = %version, "MCP server 响应未知协议版本,按兼容继续");
            }
            transport.remember_init(init.clone());
            transport
                .notify("notifications/initialized", json!({}))
                .await?;
            let listed = transport.request("tools/list", json!({})).await?;
            Ok(listed)
        };

        let listed: Value = tokio::time::timeout(std::time::Duration::from_secs(15), handshake)
            .await
            .map_err(|_| format!("MCP 插件 {server} 握手或 tools/list 超时(15s)"))?
            .map_err(|e: String| format!("MCP 插件 {server} 握手失败: {e}"))?;

        let tools = listed
            .get("tools")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut manifests = Vec::new();
        {
            let mut routes = self
                .routes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for t in tools {
                let name = t
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let def = McpToolDef {
                    name: name.clone(),
                    description: t
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    input_schema: t.get("inputSchema").cloned().unwrap_or(Value::Null),
                    annotations: t.get("annotations").cloned().unwrap_or(json!({})),
                };
                match tool_manifest(server, &def, tool_timeout_ms) {
                    Some(m) => {
                        routes.insert(
                            m.capability.clone(),
                            Route {
                                transport: transport.clone(),
                                tool: name,
                            },
                        );
                        manifests.push(m);
                    }
                    None => {
                        tracing::warn!(target: "plugin", server, tool = %name, "MCP 工具名不合规,拒注册")
                    }
                }
            }
        }

        // 进度泵:通知 → sink 回注(Hub 活多久,泵多久;sink 为共享单元)
        let mut rx = transport.subscribe_progress();
        let sink = self.sink.clone();
        tokio::spawn(async move {
            while let Some(note) = rx.recv().await {
                let guard = sink
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(f) = guard.as_ref() {
                    f(ProgressNotice {
                        operation_id: note.progress_token,
                        progress: note.progress,
                        total: note.total,
                        message: note.message,
                    });
                }
            }
        });
        Ok(manifests)
    }

    /// 装配 stub Provider 集:执行体即拒(Wire 直调不得绕过异步路径)。
    /// W2 管理面探活:对该 server 的任一路由发 tools/list(轻量、无副作用)。
    /// 返回 Ok((工具数, 工具简要信息列表)) = 联通;Err = 断连/超时摘要。
    pub async fn probe_server(&self, server: &str) -> Result<(usize, Vec<Value>), String> {
        let transport = self.transport_for(server)?;
        let listed = transport.request("tools/list", json!({})).await?;
        let tools = listed
            .get("tools")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tool_summaries: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "description": t.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                })
            })
            .collect();
        Ok((tools.len(), tool_summaries))
    }

    /// 对指定 server 的任一路由发送任意 JSON-RPC 方法并返回原始结果。
    /// 与 probe_server 同款路由查找(按 `mcp.<server>.` 前缀),但可指定任意
    /// method(如管理面对插件的 `web_search_test` / `web_usage` 扩展)。
    /// 传输层 `McpTransport::request` 本就接受任意 method,这里把「按名称找
    /// 到该 server 的 transport」暴露出来供 webadmin 使用。
    pub async fn raw_request(
        &self,
        server: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let transport = self.transport_for(server)?;
        transport.request(method, params).await
    }

    /// 采集指定 server 的子进程 stderr 尾部(issue #28)。
    /// 路由按能力名组织,取该 server 任一路由的 transport;未连接或远程
    /// 传输(无子进程)= Err。工具名全被拒注册的 server 无路由,同样报未连接。
    pub fn stderr_tail(&self, server: &str, lines: usize) -> Result<Vec<StderrLine>, String> {
        let transport = self.transport_for(server)?;
        let Some(buf) = transport.stderr_buffer() else {
            return Err("该 server 无 stderr 采集(远程传输无子进程)".into());
        };
        Ok(buf.tail(lines))
    }

    /// #3:读取指定 server 的 initialize 结果(协议版本/capabilities/
    /// serverInfo;握手时记录)。未连接 = Err。
    pub fn server_capabilities(&self, server: &str) -> Result<Value, String> {
        let transport = self.transport_for(server)?;
        transport
            .init_snapshot()
            .ok_or_else(|| "该 server 无握手记录(旧版传输或未完成 initialize)".to_string())
    }

    /// 按 `mcp.<server>.` 前缀取该 server 任一路由的 transport;未连接 = Err。
    fn transport_for(&self, server: &str) -> Result<Arc<dyn McpTransport>, String> {
        let prefix = format!(
            "mcp.{}.",
            normalize_server_name(server).ok_or_else(|| "未连接".to_string())?
        );
        let routes = self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        routes
            .iter()
            .find(|(k, _)| k.starts_with(&prefix))
            .map(|(_, r)| r.transport.clone())
            .ok_or_else(|| "未连接".into())
    }

    pub fn capability_entries(
        manifests: Vec<CapabilityManifest>,
    ) -> Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)> {
        manifests
            .into_iter()
            .map(|m| {
                // ADR-0045:MCP 能力也声明插件身份——id 取 manifest.provider
                // (即 `mcp.<server>`),使发现面/管理面能按真实 kind 呈现,
                // 与内置 provider 同一读取路径。
                let meta = bm_contract::plugin::PluginMeta::new(
                    m.provider.clone(),
                    m.version.clone(),
                    bm_contract::plugin::PluginKind::Tool,
                );
                (
                    m,
                    bm_core::broker::provider_fn_with_meta(meta, |_| {
                        Err("mcp 能力仅限异步路径".into())
                    }),
                )
            })
            .collect()
    }

    /// 热拔/重载摘除指定 server 的全部路由，并向 transport 发送 shutdown 通知。
    /// 返回被摘除的能力列表(用于通知 Registry 和 Persist 摘除)。
    pub async fn disconnect_server(&self, server: &str) -> Vec<String> {
        let Some(server_norm) = normalize_server_name(server) else {
            return Vec::new();
        };
        let prefix = format!("mcp.{server_norm}.");
        let (removed_caps, transports) = {
            let mut routes = self
                .routes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut caps = Vec::new();
            let mut trans = Vec::new();
            let keys: Vec<String> = routes.keys().cloned().collect();
            for k in keys {
                if k.starts_with(&prefix)
                    && let Some(r) = routes.remove(&k)
                {
                    caps.push(k);
                    trans.push(r.transport);
                }
            }
            (caps, trans)
        };
        for t in transports {
            // 尽力发送 shutdown / 退出通知
            let _ = t.notify("shutdown", json!({})).await;
            let _ = t.notify("exit", json!({})).await;
        }
        removed_caps
    }
}

/// ADR-0046:MCP 管理面端口适配器——surface 经此驱动,不再持具体 `McpHub`。
/// 用组合而非 `impl McpAdmin for McpHub`:因为 `connect`/`sync_from_config`
/// 需要 `&Arc<McpHub>`(握手期在 Arc 上建路由),而 trait 方法只给 `&self`。
/// 适配器持有 `Arc<McpHub>`,由 `McpHub::new()`(已返回 Arc)经 `as_admin()` 得到。
pub struct McpAdminAdapter(pub Arc<McpHub>);

impl McpHub {
    /// 取本 hub 的管理面端口视图(ADR-0046)。
    pub fn as_admin(self: &Arc<Self>) -> Arc<dyn bm_core::ports::mcp_admin::McpAdmin> {
        Arc::new(McpAdminAdapter(self.clone()))
    }
}

#[async_trait]
impl bm_core::ports::mcp_admin::McpAdmin for McpAdminAdapter {
    async fn probe_server(&self, server: &str) -> Result<(usize, Vec<Value>), String> {
        self.0.probe_server(server).await
    }

    async fn raw_request(
        &self,
        server: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        self.0.raw_request(server, method, params).await
    }

    fn stderr_tail(
        &self,
        server: &str,
        lines: usize,
    ) -> Result<Vec<bm_core::ports::mcp_admin::StderrLine>, String> {
        self.0.stderr_tail(server, lines)
    }

    fn server_capabilities(&self, server: &str) -> Result<Value, String> {
        self.0.server_capabilities(server)
    }

    async fn disconnect_server(&self, server: &str) -> Vec<String> {
        self.0.disconnect_server(server).await
    }

    async fn sync(
        &self,
        cfg_path: &std::path::Path,
        secrets: Arc<dyn bm_core::ports::SecretStore>,
        loaded_names: Vec<String>,
        registrar: &dyn bm_core::ports::mcp_admin::CapabilityRegistrar,
        limits: &bm_core::limits::LimitsCell,
    ) -> bm_core::ports::mcp_admin::SyncOutcome {
        crate::mcp::supervisor::sync_from_config(
            &self.0,
            cfg_path,
            secrets,
            loaded_names,
            registrar,
            limits,
        )
        .await
    }
}

#[async_trait]
impl AsyncCapabilityExecutor for McpHub {
    async fn call(
        &self,
        operation_id: &str,
        capability: &str,
        args: Value,
        deadline: Duration,
    ) -> Result<Value, AsyncCallError> {
        let route = {
            let routes = self
                .routes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            routes
                .get(capability)
                .map(|r| (r.transport.clone(), r.tool.clone()))
                .ok_or(AsyncCallError::Transport("未知异步能力".into()))?
        };
        let req = json!({
            "name": route.1,
            "arguments": args,
            "_meta": {"progressToken": operation_id},
        });
        self.inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(operation_id.to_string(), route.0.clone());
        let resp = tokio::select! {
            _ = tokio::time::sleep(deadline) => {
                self.inflight
                    .lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(operation_id);
                return Err(AsyncCallError::Timeout);
            }
            r = route.0.request("tools/call", req) => {
                self.inflight
                    .lock().unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(operation_id);
                r.map_err(AsyncCallError::Transport)?
            }
        };
        let is_err = resp
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_err {
            return Err(AsyncCallError::ToolError);
        }
        let text = resp
            .get("content")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter(|it| it.get("type").and_then(|v| v.as_str()) == Some("text"))
                    .map(|it| it.get("text").and_then(|v| v.as_str()).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let out = match resp.get("structuredContent") {
            Some(v @ Value::Object(_)) => v.clone(),
            _ => json!({"text": text}),
        };
        Ok(out)
    }

    fn set_progress_sink(&self, sink: ProgressSink) {
        *self
            .sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sink);
    }

    fn cancel_op(&self, operation_id: &str) {
        let transport = self
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(operation_id);
        if let Some(t) = transport {
            t.cancel_by_token(operation_id);
        }
    }
}

// ---- 安装配置装载(M7.7)----------------------------------------------------

/// 单个 MCP server 的运行解析结果(env/bearer_token 已从 Secret Store 解析;
/// 明文只进子进程/请求环境,不入日志/事件,INV-5)。
#[derive(Debug, Clone)]
pub struct McpServerSetup {
    pub name: String,
    pub transport: String,
    pub url: Option<String>,
    pub bearer_token: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub env_resolved: HashMap<String, String>,
    pub tool_timeout_ms: u64,
    pub restart_limit: u32,
    /// ADR-0035:合同 trust 字段的解析值。缺省 `explicit-config`(配置显式
    /// 列出即安装批准,M7 既有语义);显式给出非枚举值在合同校验步即被拒。
    /// 消费点=装载日志留痕,使来源可见。
    pub trust: String,
    /// ADR-0035:完整性校验目标(可选)。解释器型条目(command=解释器,
    /// args=脚本)须以此声明真实载荷;存在时 sha256 一律哈希 payload。
    pub payload: Option<String>,
}

/// 从配置文件装载 MCP server 安装清单(每项过 mcp-server.v0_1 合同校验)。
/// 文件显式列出 = 用户安装批准;env/bearer_token 一律 secret: 引用(明文拒绝由合同承担)。
/// `default_restart_limit` = 条目未声明 `restart_limit` 时的回落,由调用方
/// 从 `limits.mcp_restart_limit` 传入(
/// 该项对 MCP 空转——与 skill 默认超时同类缺陷)。条目级值仍优先。
pub fn load_mcp_setups(
    path: &std::path::Path,
    store: &dyn bm_core::ports::SecretStore,
    default_restart_limit: u32,
) -> Result<Vec<McpServerSetup>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("读取 MCP 配置失败: {e}"))?;
    let arr: Vec<Value> =
        serde_json::from_str(&text).map_err(|e| format!("MCP 配置不是 JSON 数组: {e}"))?;
    let mut out = Vec::new();
    for item in &arr {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        if let Err(e) =
            bm_contract::schemas::validate(bm_contract::registries::MCP_SERVER_SCHEMA, item)
        {
            tracing::warn!(target: "plugin", server = %name, error = %e, "MCP 配置项合同校验失败(已跳过)");
            continue;
        }

        let transport = item
            .get("transport")
            .and_then(|v| v.as_str())
            .unwrap_or("stdio")
            .to_string();

        let mut bearer_token = None;
        if let Some(tok_ref) = item.get("bearer_token").and_then(|v| v.as_str()) {
            match bm_core::ports::SecretStore::get(store, tok_ref) {
                Ok(val) => bearer_token = Some(val),
                Err(e) => {
                    tracing::warn!(target: "plugin", server = %name, secret_ref = %tok_ref, error = ?e, "MCP bearer_token 解析失败(已跳过)");
                    continue;
                }
            }
        }

        let mut env_resolved = HashMap::new();
        let mut env_err = false;
        if let Some(env) = item.get("env").and_then(|v| v.as_object()) {
            for (k, v) in env {
                let Some(ref_) = v.as_str() else {
                    tracing::warn!(target: "plugin", server = %name, key = %k, "MCP env 值不是字符串(已跳过该服务)");
                    env_err = true;
                    break;
                };
                match bm_core::ports::SecretStore::get(store, ref_) {
                    Ok(value) => {
                        env_resolved.insert(k.clone(), value);
                    }
                    Err(e) => {
                        tracing::warn!(target: "plugin", server = %name, key = %k, secret_ref = %ref_, error = ?e,
                            "MCP env 密钥引用解析失败(已跳过该服务)");
                        env_err = true;
                        break;
                    }
                }
            }
        }
        if env_err {
            continue;
        }
        // ADR-0035:trust 显式消费(缺省 explicit-config,即「配置显式列出=
        // 安装批准」;非枚举值已在上面合同校验步被拒)。解析值随条目留痕,
        // 使来源在装载日志可见。
        let trust = item
            .get("trust")
            .and_then(|v| v.as_str())
            .unwrap_or("explicit-config");
        tracing::info!(target: "plugin", server = %name, trust = %trust, "MCP 装载 trust(来源显式配置)");

        // 外部评审
        // 「校验目标」哈希,不符拒载(防安装后被替换)。校验目标 = payload(若
        // 声明)否则 command;解释器型条目(command 非本地文件、args 指向本地
        // 脚本)未声明 payload 时语义歧义,fail-closed 拒载,杜绝「哈希解释器
        // 却宣称校验了插件」的假保证。
        if let Some(expected) = item.get("sha256").and_then(|v| v.as_str()) {
            match resolve_integrity_target(item) {
                Ok(target) => {
                    if let Err(e) = verify_integrity(&target, expected) {
                        tracing::warn!(target: "plugin", server = %name, target = %target, error = %e,
                            "MCP 完整性校验不符(已跳过,疑似被替换)");
                        continue;
                    }
                }
                Err(e) => {
                    tracing::warn!(target: "plugin", server = %name, error = %e, "MCP 完整性校验目标不明确(已跳过)");
                    continue;
                }
            }
        }
        out.push(McpServerSetup {
            name: item["name"].as_str().unwrap_or_default().to_string(),
            transport,
            url: item["url"].as_str().map(String::from),
            bearer_token,
            command: item["command"].as_str().unwrap_or_default().to_string(),
            args: item["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            env_resolved,
            tool_timeout_ms: item
                .get("tool_timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_TOOL_TIMEOUT_MS),
            restart_limit: item
                .get("restart_limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(default_restart_limit as u64) as u32,
            trust: trust.to_string(),
            payload: item
                .get("payload")
                .and_then(|v| v.as_str())
                .map(String::from),
        });
    }
    Ok(out)
}

/// 外部评审 )。
pub fn sha256_file(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 {path} 失败: {e}"))?;
    Ok(bm_contract::hash::sha256_hex(&bytes))
}

/// ADR-0035:依条目解析完整性校验目标。
/// 目标 = `payload`(若声明)否则 `command`。解释器型条目(`command` 非本地
/// 常规文件,而 `args` 指向本地脚本)未声明 `payload` 时,**目标语义歧义**——
/// 此时哈希 `command` 会校验解释器而非脚本,给出假保证;故返回 Err 由调用方
/// fail-closed 拒载。裸解释器名(如 `python`)且 args 无本地文件时回退
/// `command`,读取失败即拒载(既有行为)。
fn resolve_integrity_target(item: &Value) -> Result<String, String> {
    if let Some(p) = item.get("payload").and_then(|v| v.as_str()) {
        return Ok(p.to_string());
    }
    let command = item["command"].as_str().unwrap_or_default();
    if std::path::Path::new(command).is_file() {
        return Ok(command.to_string());
    }
    if let Some(arg) = item.get("args").and_then(|v| v.as_array()).and_then(|a| {
        a.iter()
            .filter_map(|v| v.as_str())
            .find(|s| std::path::Path::new(s).is_file())
    }) {
        return Err(format!(
            "解释器型条目 command='{command}' 非本地文件而 args 指向本地文件 '{arg}';哈希目标歧义,请在条目声明 payload"
        ));
    }
    Ok(command.to_string())
}

/// 装载/重载前复验:不符 = Err(调用方跳过该条目并告警)。
pub fn verify_integrity(path: &str, expected: &str) -> Result<(), String> {
    let actual = sha256_file(path)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "SHA-256 不符(期望 {expected},实际 {actual};文件可能在安装后被替换)"
        ))
    }
}

/// MCP 子进程继承环境白名单(测试可见):父进程其余环境变量一律不下发。
fn child_inherited_env() -> Vec<(&'static str, String)> {
    const ALLOW: &[&str] = &[
        "PATH",
        "Path",
        "SYSTEMROOT",
        "SystemRoot",
        "systemroot",
        "COMSPEC",
        "ComSpec",
        "TEMP",
        "TMP",
        "TMPDIR",
        "HOME",
        "USERPROFILE",
    ];
    ALLOW
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (*k, v)))
        .collect()
}

#[cfg(test)]
mod m9_review_env_tests {
    use super::child_inherited_env;

    /// P0()验收:子进程继承面只含白名单——主密钥等父进程
    /// 敏感环境变量一律不下发。
    #[test]
    fn child_env_allowlist_excludes_parent_secrets() {
        const ALLOW: &[&str] = &[
            "PATH",
            "Path",
            "SYSTEMROOT",
            "SystemRoot",
            "systemroot",
            "COMSPEC",
            "ComSpec",
            "TEMP",
            "TMP",
            "TMPDIR",
            "HOME",
            "USERPROFILE",
        ];
        for (k, _) in child_inherited_env() {
            assert!(
                ALLOW.contains(&k),
                "白名单外的环境变量不得下发给 MCP 子进程: {k}"
            );
            assert!(
                !k.to_ascii_uppercase().starts_with("BOEN_"),
                "BOEN_* 变量(主密钥/开关)禁止下发: {k}"
            );
        }
    }
}

#[cfg(test)]
mod integrity_tests {
    use super::*;
    use crate::secret::MemSecretStore;
    use std::io::Write;

    #[test]
    fn sha256_matches_and_mismatch_is_detected() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("plugin.exe");
        std::fs::write(&p, b"binary-content").expect("写");
        let path = p.display().to_string();
        let good = sha256_file(&path).expect("哈希");
        assert_eq!(good.len(), 64);
        assert!(verify_integrity(&path, &good).is_ok());
        let wrong = "0".repeat(64);
        let err = verify_integrity(&path, &wrong).expect_err("不符必须报错");
        assert!(err.contains("SHA-256 不符"));
    }

    #[test]
    fn load_skips_entry_when_hash_mismatches() {
        let dir = tempfile::tempdir().expect("tmp");
        let exe = dir.path().join("plugin.exe");
        std::fs::write(&exe, b"payload").expect("写");
        let cfg = dir.path().join("mcp.json");
        let real = sha256_file(&exe.display().to_string()).expect("哈希");
        let wrong = format!("{:0>64}", "ab");

        // 篡改条目被跳过,未篡改条目保留
        std::fs::write(
            &cfg,
            format!(
                r#"[{{"name":"bad","transport":"stdio","command":"{cmd}","sha256":"{wrong}"}},
                   {{"name":"good","transport":"stdio","command":"{cmd}","sha256":"{real}"}}]"#,
                cmd = exe.display().to_string().replace('\\', "\\\\")
            ),
        )
        .expect("写配置");
        let store = MemSecretStore::new();
        let setups = load_mcp_setups(&cfg, &store, 3).expect("解析");
        let names: Vec<&str> = setups.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["good"], "不符条目必须被跳过:{names:?}");

        // 无 sha256 的旧条目照常兼容
        std::fs::write(
            &cfg,
            format!(
                r#"[{{"name":"legacy","transport":"stdio","command":"{cmd}"}}]"#,
                cmd = exe.display().to_string().replace('\\', "\\\\")
            ),
        )
        .expect("写配置");
        let setups = load_mcp_setups(&cfg, &store, 3).expect("解析");
        assert_eq!(setups.len(), 1);
        assert_eq!(setups[0].name, "legacy");
    }

    #[test]
    fn sha256_file_missing_errors() {
        assert!(
            sha256_file("Z:/no/such/file.exe").is_err()
                || sha256_file("/no/such/file.exe").is_err()
        );
        let _ = std::io::sink().write(&[]);
    }

    /// ADR-0035:解释器型条目(command=解释器名)声明 payload 时,哈希目标
    /// = 脚本载荷(而非解释器);篡改脚本被检出。
    #[test]
    fn interpreter_entry_hashes_payload_not_launcher() {
        let dir = tempfile::tempdir().expect("tmp");
        let script = dir.path().join("server.py");
        std::fs::write(&script, b"print('v1')").expect("写脚本");
        let cfg = dir.path().join("mcp.json");
        let good = sha256_file(&script.display().to_string()).expect("哈希脚本");
        std::fs::write(
            &cfg,
            format!(
                r#"[{{"name":"py","transport":"stdio","command":"python",
                    "args":["{s}"],"payload":"{s}","sha256":"{good}"}}]"#,
                s = script.display().to_string().replace('\\', "\\\\")
            ),
        )
        .expect("写配置");
        let store = MemSecretStore::new();
        let setups = load_mcp_setups(&cfg, &store, 3).expect("解析");
        assert_eq!(setups.len(), 1);
        assert_eq!(
            setups[0].payload.as_deref(),
            Some(script.display().to_string().as_str())
        );

        // 脚本被替换 -> 哈希不符 -> 拒载(即使解释器本身未变)
        std::fs::write(&script, b"print('v2-evil')").expect("改脚本");
        let setups = load_mcp_setups(&cfg, &store, 3).expect("解析");
        assert!(setups.is_empty(), "payload 被替换必须拒载");
    }

    /// ADR-0035:解释器型条目(非本地 command + args 指向本地脚本)未声明
    /// payload 时语义歧义 -> fail-closed 拒载,不给「哈希解释器」的假保证。
    #[test]
    fn interpreter_entry_without_payload_fails_closed() {
        let dir = tempfile::tempdir().expect("tmp");
        let script = dir.path().join("server.py");
        std::fs::write(&script, b"print('x')").expect("写脚本");
        let cfg = dir.path().join("mcp.json");
        std::fs::write(
            &cfg,
            format!(
                r#"[{{"name":"amb","transport":"stdio","command":"python",
                    "args":["{s}"],"sha256":"{h}"}}]"#,
                s = script.display().to_string().replace('\\', "\\\\"),
                h = "a".repeat(64)
            ),
        )
        .expect("写配置");
        let store = MemSecretStore::new();
        let setups = load_mcp_setups(&cfg, &store, 3).expect("解析");
        assert!(setups.is_empty(), "目标歧义必须拒载(fail-closed)");
    }

    /// ADR-0035:trust 缺省解析为 explicit-config;非枚举值在合同校验步被拒。
    #[test]
    fn trust_defaults_and_invalid_value_rejected() {
        let dir = tempfile::tempdir().expect("tmp");
        let exe = dir.path().join("plugin.exe");
        std::fs::write(&exe, b"bin").expect("写");
        let cfg = dir.path().join("mcp.json");
        let cmd = exe.display().to_string().replace('\\', "\\\\");
        // 缺省 -> explicit-config
        std::fs::write(
            &cfg,
            format!(r#"[{{"name":"a","transport":"stdio","command":"{cmd}"}}]"#),
        )
        .expect("写配置");
        let store = MemSecretStore::new();
        let setups = load_mcp_setups(&cfg, &store, 3).expect("解析");
        assert_eq!(setups.len(), 1);
        assert_eq!(setups[0].trust, "explicit-config");
        // 非枚举值 -> 合同校验拒 -> 跳过
        std::fs::write(
            &cfg,
            format!(
                r#"[{{"name":"b","transport":"stdio","command":"{cmd}","trust":"agent-registered"}}]"#
            ),
        )
        .expect("写配置");
        let setups = load_mcp_setups(&cfg, &store, 3).expect("解析");
        assert!(setups.is_empty(), "非枚举 trust 必须被合同校验拒");
    }

    /// 传入的默认值**(生产 = `limits.mcp_restart_limit`),而非硬编码 3——
    /// 。条目级声明仍优先。
    #[test]
    fn restart_limit_defaults_from_caller_and_entry_overrides() {
        let dir = tempfile::tempdir().expect("tmp");
        let exe = dir.path().join("plugin.exe");
        std::fs::write(&exe, b"bin").expect("写");
        let cfg = dir.path().join("mcp.json");
        let cmd = exe.display().to_string().replace('\\', "\\\\");
        let store = MemSecretStore::new();

        // 未声明 → 用调用方传入的默认(此处刻意传非 3 值,证明真的读了参数)。
        std::fs::write(
            &cfg,
            format!(r#"[{{"name":"a","transport":"stdio","command":"{cmd}"}}]"#),
        )
        .expect("写配置");
        let setups = load_mcp_setups(&cfg, &store, 7).expect("解析");
        assert_eq!(setups[0].restart_limit, 7, "缺省须随调用方默认(limits)");

        // 条目级声明优先于默认。
        std::fs::write(
            &cfg,
            format!(r#"[{{"name":"b","transport":"stdio","command":"{cmd}","restart_limit":2}}]"#),
        )
        .expect("写配置");
        let setups = load_mcp_setups(&cfg, &store, 7).expect("解析");
        assert_eq!(setups[0].restart_limit, 2, "条目级 restart_limit 优先");
    }
}

/// issue #28:stderr 管道采集入环形缓冲。真子进程测试沿用 t104 惯例
/// (#[ignore] + BOEN_MCP_STDIO_TEST=1;CI 三平台 ubuntu 无 `python` 别名)。
#[cfg(test)]
mod stderr_tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    #[ignore = "stdio 子进程测试:BOEN_MCP_STDIO_TEST=1 启用"]
    async fn stderr_piped_into_ring_buffer_with_generation() {
        let code = "import sys, time; sys.stderr.write('boom-diagnostic-line\\n'); sys.stderr.flush(); time.sleep(30)";
        let transport = StdioMcpTransport::spawn(
            "python",
            &["-c".to_string(), code.to_string()],
            &Default::default(),
            3,
        )
        .expect("子进程启动");

        // 泵是异步的:轮询至目标行出现(最多 5s)
        let mut hit = false;
        for _ in 0..50 {
            let tail = transport.stderr_tail(50);
            if tail
                .iter()
                .any(|l| l.generation == 1 && l.text.contains("boom-diagnostic-line"))
            {
                hit = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(hit, "stderr 行未入缓冲: {:?}", transport.stderr_tail(50));

        // 起止哨兵行:第 1 代启动标记在缓冲里
        assert!(
            transport
                .stderr_tail(50)
                .iter()
                .any(|l| l.text.contains("第 1 代子进程启动")),
            "缺启动哨兵行"
        );
    }

    #[test]
    fn stderr_ring_capacity_bounded() {
        let buf = StderrBuffer::with_capacity(5);
        for i in 0..20 {
            buf.push(1, format!("line-{i}"));
        }
        let tail = buf.tail(100);
        assert_eq!(tail.len(), 5, "环形缓冲必须封顶");
        assert_eq!(tail[0].text, "line-15", "最老的被挤出");
        assert_eq!(tail[4].text, "line-19");
    }
}

mod shape;
pub mod supervisor;
pub mod transport_http;
mod transport_stdio;
pub use shape::*;
pub use transport_http::*;
pub use transport_stdio::*;

/// 评审修复()回归:锁中毒必须自恢复,不得让一次 panic 把传输层
/// 永久毒化(。
#[cfg(test)]
mod lock_poison_recovery_tests {
    #[test]
    fn poisoned_lock_recovers_via_into_inner() {
        let m = std::sync::Mutex::new(0u32);
        // 制造毒化:持锁 panic
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("故意毒化");
        }));
        // 与 bm-providers 各文件一致的恢复语义:毒化后仍可取锁并修正状态
        let mut g = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *g += 1;
        assert_eq!(*g, 1);

        let rw = std::sync::RwLock::new(vec![1u8]);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = rw.read().unwrap();
            panic!("故意毒化读锁");
        }));
        let g = rw.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(g[0], 1);
    }
}
