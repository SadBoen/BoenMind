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

pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 30_000;

// ---- 数据形状 --------------------------------------------------------------

/// tools/list 条目(发现面)。
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    #[allow(dead_code)]
    pub description: Option<String>,
    /// 工具 inputSchema(MCP JSON Schema;直通 manifest.input_schema)。
    pub input_schema: Value,
    /// MCP annotations(readOnlyHint / destructiveHint → effect 映射)。
    pub annotations: Value,
}

/// 服务端进度通知(notifications/progress 解析结果)。
#[derive(Debug, Clone)]
pub struct McpProgressNote {
    pub progress_token: String,
    pub progress: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
}

/// 工具名规范化:仅 `.` 分段;段内小写、连字符归一为下划线;
/// 任一段不匹配能力名段字符集 `^[a-z][a-z0-9_]*$` → None(拒注册)。
pub fn normalize_tool_name(tool: &str) -> Option<String> {
    let mut out = String::new();
    for raw in tool.split('.') {
        let seg = raw.to_ascii_lowercase().replace('-', "_");
        let ok = !seg.is_empty()
            && seg.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && seg
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !ok {
            return None;
        }
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&seg);
    }
    Some(out)
}

/// annotations → effect/approval 映射(M7 规格 S3;GT-05 形态):
/// readOnlyHint → read-only + not-required;destructiveHint →
/// external-side-effect + required;缺省 reversible-command + required
/// (未知风险首调审批,M7.7)。
pub fn tool_manifest(
    server: &str,
    tool: &McpToolDef,
    timeout_ms: u64,
) -> Option<CapabilityManifest> {
    let tool_norm = normalize_tool_name(&tool.name)?;
    let read_only = tool
        .annotations
        .get("readOnlyHint")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let destructive = tool
        .annotations
        .get("destructiveHint")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let effect = if read_only {
        "read-only"
    } else if destructive {
        "external-side-effect"
    } else {
        "reversible-command"
    };
    let approval = if read_only {
        "not-required"
    } else {
        "required"
    };
    let input_schema = if tool.input_schema.is_null() || tool.input_schema == json!({}) {
        json!({"type": "object"})
    } else {
        tool.input_schema.clone()
    };
    serde_json::from_value(json!({
        "capability": format!("mcp.{server}.{tool_norm}"),
        "provider": format!("mcp.{server}"),
        "version": "0.1.0",
        "input_schema": input_schema,
        "output_schema": {"type": "object"},
        "effect": effect,
        "idempotent": false,
        "cancellable": true,
        "timeout_ms": timeout_ms,
        "approval": approval,
        "scopes": [format!("domain:mcp.{server}")],
    }))
    .ok()
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
}

fn dead_progress_rx() -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
    rx
}

// ---- InProc 传输(测试替身)-------------------------------------------------

/// 工具行为脚本(InProc 测试)。
#[derive(Clone, Default)]
pub struct Behavior {
    /// Ok = CallResult(可为 {"isError": true} 形态模拟工具级失败);
    /// Err = JSON-RPC 层错误(传输/协议故障)。
    pub result: Option<Result<Value, String>>,
    /// 真实睡眠(制造进行中窗口;超时测试用)。
    pub delay_ms: u64,
    /// 调用前发送的进度步 (progress, total, message)。
    pub progress: Vec<(u64, Option<u64>, String)>,
}

impl Behavior {
    pub fn done(result: Value) -> Self {
        Self {
            result: Some(Ok(result)),
            ..Default::default()
        }
    }
}

/// 进程内 MCP server 测试替身。进度订阅:单消费者(先到先得)。
pub struct InProcMcpServer {
    tools: Mutex<Vec<McpToolDef>>,
    behaviors: Mutex<HashMap<String, Behavior>>,
    progress_tx: tokio::sync::mpsc::UnboundedSender<McpProgressNote>,
    progress_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>>>,
}

impl InProcMcpServer {
    pub fn new(tools: Vec<McpToolDef>) -> Arc<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(Self {
            tools: Mutex::new(tools),
            behaviors: Mutex::new(HashMap::new()),
            progress_tx: tx,
            progress_rx: Mutex::new(Some(rx)),
        })
    }

    pub fn set_behavior(&self, tool: &str, behavior: Behavior) {
        self.behaviors
            .lock()
            .expect("锁未中毒")
            .insert(tool.to_string(), behavior);
    }
}

#[async_trait]
impl McpTransport for InProcMcpServer {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "inproc", "version": "0.0.1"}
            })),
            "tools/list" => {
                let tools = self.tools.lock().expect("锁未中毒").clone();
                Ok(json!({"tools": tools.iter().map(|t| json!({
                    "name": t.name,
                    "inputSchema": t.input_schema,
                    "annotations": t.annotations,
                })).collect::<Vec<_>>()}))
            }
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let token = params
                    .get("_meta")
                    .and_then(|m| m.get("progressToken"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let behavior = self
                    .behaviors
                    .lock()
                    .expect("锁未中毒")
                    .get(&name)
                    .cloned()
                    .unwrap_or_default();
                for (p, total, msg) in &behavior.progress {
                    let _ = self.progress_tx.send(McpProgressNote {
                        progress_token: token.clone(),
                        progress: *p,
                        total: *total,
                        message: Some(msg.clone()),
                    });
                }
                if behavior.delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(behavior.delay_ms)).await;
                }
                match behavior.result {
                    Some(Ok(v)) => Ok(v),
                    Some(Err(e)) => Err(e),
                    None => Ok(json!({"content": [{"type": "text", "text": "ok"}]})),
                }
            }
            other => Err(format!("inproc 不支持方法 {other}")),
        }
    }

    async fn notify(&self, _method: &str, _params: Value) -> Result<(), String> {
        Ok(())
    }

    fn subscribe_progress(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
        self.progress_rx
            .lock()
            .expect("锁未中毒")
            .take()
            .unwrap_or_else(dead_progress_rx)
    }
}

// ---- stdio 传输 ------------------------------------------------------------

/// stdio 子进程传输(newline-delimited JSON-RPC 2.0)。
pub struct StdioMcpTransport {
    inner: tokio::sync::Mutex<StdioInner>,
    progress_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>>>,
}

struct StdioInner {
    next_id: u64,
    /// 与读取泵共享的同一张在途表(request 注册,泵按 id 配对摘除)。
    pending: PendingMap,
    stdin: Option<tokio::process::ChildStdin>,
    alive: bool,
}

impl StdioMcpTransport {
    /// 拉起子进程并启动读取泵。env 值由调用方从 Secret Store 解析后传入
    /// (明文只进子进程环境,不入日志/事件,INV-5)。
    pub fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Arc<Self>, String> {
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio::process::Command;

        let mut cmd = Command::new(command);
        cmd.args(args)
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("MCP 子进程启动失败: {e}"))?;
        let stdin = child.stdin.take().ok_or("MCP 子进程 stdin 不可用")?;
        let stdout = child.stdout.take().ok_or("MCP 子进程 stdout 不可用")?;

        let (progress_tx, progress_rx) = tokio::sync::mpsc::unbounded_channel();
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

        // 读取泵:响应按 id 配对;通知解析进度;通道关闭 = 子进程退出
        let pending_reader = pending.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if let Some(id) = msg.get("id").and_then(|v| v.as_u64()) {
                    let slot = pending_reader.lock().expect("锁未中毒").remove(&id);
                    if let Some(tx) = slot {
                        match msg.get("error") {
                            Some(err) => {
                                let _ = tx.send(Err(format!(
                                    "rpc-error:{}",
                                    err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1)
                                )));
                            }
                            None => {
                                let _ =
                                    tx.send(Ok(msg.get("result").cloned().unwrap_or(json!({}))));
                            }
                        }
                    }
                } else if msg.get("method").and_then(|v| v.as_str())
                    == Some("notifications/progress")
                {
                    let p = msg.get("params").cloned().unwrap_or(json!({}));
                    let _ = progress_tx.send(McpProgressNote {
                        progress_token: p
                            .get("progressToken")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        progress: p.get("progress").and_then(|v| v.as_u64()).unwrap_or(0),
                        total: p.get("total").and_then(|v| v.as_u64()),
                        message: p.get("message").and_then(|v| v.as_str()).map(String::from),
                    });
                }
            }
            let mut map = pending_reader.lock().expect("锁未中毒");
            for (_, tx) in map.drain() {
                let _ = tx.send(Err("stdio-closed".into()));
            }
        });

        Ok(Arc::new(Self {
            inner: tokio::sync::Mutex::new(StdioInner {
                next_id: 0,
                pending,
                stdin: Some(stdin),
                alive: true,
            }),
            progress_rx: Mutex::new(Some(progress_rx)),
        }))
    }
}

#[async_trait]
impl McpTransport for StdioMcpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        use tokio::io::AsyncWriteExt;
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut inner = self.inner.lock().await;
            if !inner.alive {
                return Err("stdio-closed".into());
            }
            inner.next_id += 1;
            let id = inner.next_id;
            inner.pending.lock().expect("锁未中毒").insert(id, tx);
            let stdin = inner.stdin.as_mut().expect("stdin 在活着时存在");
            let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            let mut bytes = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
            bytes.push('\n');
            stdin
                .write_all(bytes.as_bytes())
                .await
                .map_err(|e| format!("stdio 写失败: {e}"))?;
            stdin
                .flush()
                .await
                .map_err(|e| format!("stdio 写失败: {e}"))?;
        }
        match rx.await {
            Ok(r) => r,
            Err(_) => Err("stdio-closed".into()),
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        use tokio::io::AsyncWriteExt;
        let mut inner = self.inner.lock().await;
        if !inner.alive {
            return Err("stdio-closed".into());
        }
        let stdin = inner.stdin.as_mut().expect("stdin 在活着时存在");
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let mut bytes = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
        bytes.push('\n');
        stdin
            .write_all(bytes.as_bytes())
            .await
            .map_err(|e| format!("stdio 写失败: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("stdio 写失败: {e}"))
    }

    fn subscribe_progress(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
        self.progress_rx
            .lock()
            .expect("锁未中毒")
            .take()
            .unwrap_or_else(dead_progress_rx)
    }
}

// ---- Hub:握手/发现/路由/异步执行器 -----------------------------------------

type ProgressSink = Box<dyn Fn(ProgressNotice) + Send + Sync>;

/// 在途请求表(request 注册,读取泵按 id 配对摘除)。
type PendingMap = Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Result<Value, String>>>>>;

struct Route {
    transport: Arc<dyn McpTransport>,
    tool: String,
    #[allow(dead_code)]
    server: String,
}

/// MCP Hub:多 server 路由 + `AsyncCapabilityExecutor` 端口实现。
pub struct McpHub {
    routes: Mutex<HashMap<String, Route>>,
    sink: Arc<Mutex<Option<ProgressSink>>>,
}

impl Default for McpHub {
    fn default() -> Self {
        Self::new_inner()
    }
}

impl McpHub {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::new_inner())
    }

    fn new_inner() -> Self {
        Self {
            routes: Mutex::new(HashMap::new()),
            sink: Arc::new(Mutex::new(None)),
        }
    }

    /// 握手 + 发现:initialize → initialized → tools/list → 生成 manifests
    /// 并建立路由。不合规工具名跳过(拒注册,tracing 留痕)。
    pub async fn connect(
        self: &Arc<Self>,
        server: &str,
        transport: Arc<dyn McpTransport>,
        tool_timeout_ms: u64,
    ) -> Result<Vec<CapabilityManifest>, String> {
        let init = transport
            .request(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": "boenmind", "version": "0.1"}
                }),
            )
            .await?;
        if init
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .is_none()
        {
            return Err("initialize 响应缺 protocolVersion".into());
        }
        transport
            .notify("notifications/initialized", json!({}))
            .await?;
        let listed = transport.request("tools/list", json!({})).await?;
        let tools = listed
            .get("tools")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut manifests = Vec::new();
        {
            let mut routes = self.routes.lock().expect("锁未中毒");
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
                                server: server.to_string(),
                            },
                        );
                        manifests.push(m);
                    }
                    None => tracing::warn!(server, tool = %name, "MCP 工具名不合规,拒注册"),
                }
            }
        }

        // 进度泵:通知 → sink 回注(Hub 活多久,泵多久;sink 为共享单元)
        let mut rx = transport.subscribe_progress();
        let sink = self.sink.clone();
        tokio::spawn(async move {
            while let Some(note) = rx.recv().await {
                let guard = sink.lock().expect("锁未中毒");
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
    pub fn capability_entries(
        manifests: Vec<CapabilityManifest>,
    ) -> Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)> {
        manifests
            .into_iter()
            .map(|m| {
                (
                    m,
                    bm_core::broker::provider_fn(|_| Err("mcp 能力仅限异步路径".into())),
                )
            })
            .collect()
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
            let routes = self.routes.lock().expect("锁未中毒");
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
        let resp = tokio::select! {
            _ = tokio::time::sleep(deadline) => return Err(AsyncCallError::Timeout),
            r = route.0.request("tools/call", req) => r.map_err(AsyncCallError::Transport)?,
        };
        let is_err = resp
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_err {
            return Err(AsyncCallError::ToolError);
        }
        let mut text = String::new();
        if let Some(items) = resp.get("content").and_then(|v| v.as_array()) {
            for it in items {
                if it.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(it.get("text").and_then(|v| v.as_str()).unwrap_or_default());
                }
            }
        }
        let out = match resp.get("structuredContent") {
            Some(v @ Value::Object(_)) => v.clone(),
            _ => json!({"text": text}),
        };
        Ok(out)
    }

    fn set_progress_sink(&self, sink: ProgressSink) {
        *self.sink.lock().expect("锁未中毒") = Some(sink);
    }
}
