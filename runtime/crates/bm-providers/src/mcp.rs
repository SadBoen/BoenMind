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
/// stdio 写管道限时:子进程挂起(非崩溃)时调用方持锁跨
/// await,无超时则该域能力永久坏死(respawn 也拿不到锁)。
const STDIO_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// 带限时的 stdio 帧写入(write_all + flush),所有持锁写管道路径的统一出口。
async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(
    stdin: &mut W,
    bytes: &[u8],
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    tokio::time::timeout(STDIO_WRITE_TIMEOUT, stdin.write_all(bytes))
        .await
        .map_err(|_| "stdio 写超时(子进程 10s 未消费,判定挂起)".to_string())?
        .map_err(|e| format!("stdio 写失败: {e}"))?;
    stdin
        .flush()
        .await
        .map_err(|e| format!("stdio 写失败: {e}"))
}

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
    // 外部审计 X-05(P2):冲突标注裁决——destructiveHint 优先(第三方
    // 元数据只能提高风险、不能降低)。readOnly+destructive 并存 → 按
    // external-side-effect + required 注册,绝不降级为免审批只读。
    let read_only = read_only && !destructive;
    let effect = if destructive {
        "external-side-effect"
    } else if read_only {
        "read-only"
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

    /// 按进度令牌取消在途请求(MCP notifications/cancelled;尽力终止)。
    fn cancel_by_token(&self, _token: &str) {}
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
    /// tools/call 次数(健康面测试:断言封禁后不再触达执行器)。
    calls: Mutex<HashMap<String, u32>>,
    /// 进度令牌 → 取消标志(M8.3:语义取消的 InProc 贯穿)。
    cancel_flags: Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
    progress_tx: tokio::sync::mpsc::UnboundedSender<McpProgressNote>,
    progress_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>>>,
}

impl InProcMcpServer {
    pub fn new(tools: Vec<McpToolDef>) -> Arc<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(Self {
            tools: Mutex::new(tools),
            behaviors: Mutex::new(HashMap::new()),
            calls: Mutex::new(HashMap::new()),
            cancel_flags: Mutex::new(HashMap::new()),
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

    /// 某 tool 的 tools/call 次数(测试断言面)。
    pub fn call_count(&self, tool: &str) -> u32 {
        self.calls
            .lock()
            .expect("锁未中毒")
            .get(tool)
            .copied()
            .unwrap_or(0)
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
                *self
                    .calls
                    .lock()
                    .expect("锁未中毒")
                    .entry(name.clone())
                    .or_insert(0) += 1;
                if !token.is_empty() {
                    self.cancel_flags
                        .lock()
                        .expect("锁未中毒")
                        .entry(token.clone())
                        .or_insert_with(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));
                }
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
                    let flag = self
                        .cancel_flags
                        .lock()
                        .expect("锁未中毒")
                        .get(&token)
                        .cloned();
                    let stop_at =
                        tokio::time::Instant::now() + Duration::from_millis(behavior.delay_ms);
                    tokio::select! {
                        _ = tokio::time::sleep_until(stop_at) => {}
                        _ = async {
                            if let Some(f) = flag {
                                while !f.load(std::sync::atomic::Ordering::Relaxed) {
                                    tokio::time::sleep(Duration::from_millis(5)).await;
                                }
                            } else {
                                std::future::pending::<()>().await;
                            }
                        } => {}
                    }
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

    fn cancel_by_token(&self, token: &str) {
        if let Some(f) = self.cancel_flags.lock().expect("锁未中毒").get(token) {
            f.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

// ---- stdio 传输 ------------------------------------------------------------

/// stdio 子进程传输(newline-delimited JSON-RPC 2.0)。
/// 子进程退出后,下次 request 自动重生一代子进程(M7.4 重启语义;
/// 重连上限由内核健康面计量)。
pub struct StdioMcpTransport {
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    inner: Arc<tokio::sync::Mutex<StdioInner>>,
    progress_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>>>,
    alive: Arc<std::sync::atomic::AtomicBool>,
}

struct StdioInner {
    next_id: u64,
    /// 与读取泵共享的同一张在途表(request 注册,泵按 id 配对摘除)。
    pending: PendingMap,
    stdin: Option<tokio::process::ChildStdin>,
    /// 进度令牌 → 在途 rpc id(取消通知定位;响应即摘除)。
    token_to_id: HashMap<String, u64>,
}

impl StdioMcpTransport {
    /// 拉起子进程并启动读取泵。env 值由调用方从 Secret Store 解析后传入
    /// (明文只进子进程环境,不入日志/事件,INV-5)。
    pub fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Arc<Self>, String> {
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (pending, stdin, progress_rx) = spawn_generation(command, args, env, alive.clone())?;
        Ok(Arc::new(Self {
            command: command.to_string(),
            args: args.to_vec(),
            env: env.clone(),
            inner: Arc::new(tokio::sync::Mutex::new(StdioInner {
                next_id: 0,
                pending,
                stdin: Some(stdin),
                token_to_id: HashMap::new(),
            })),
            progress_rx: Mutex::new(Some(progress_rx)),
            alive,
        }))
    }
}

/// 拉起一代子进程:返回在途表 / stdin / 进度接收端。
fn spawn_generation(
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
    alive: Arc<std::sync::atomic::AtomicBool>,
) -> Result<
    (
        PendingMap,
        tokio::process::ChildStdin,
        tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>,
    ),
    String,
> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    let mut cmd = Command::new(command);
    // P0(第四轮评审):子进程默认继承父进程全部环境 = 主密钥/令牌外泄
    // (INV-5)。清空后仅放行运行所需白名单,再加各 server 显式配置的 env。
    cmd.env_clear();
    for (k, v) in child_inherited_env() {
        cmd.env(k, v);
    }
    cmd.args(args)
        .envs(env)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit()); // W2 诊断:子进程报错直通 server.log
    // 外部审计:kill_on_drop 绑定子进程生命周期——连接器对象被丢弃时
    // 子进程随之终止,防止服务端异常退出后 Python App 成为孤儿进程。
    cmd.kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("MCP 子进程启动失败: {e}"))?;
    eprintln!("MCP 子进程已拉起 pid={:?} command={}", child.id(), command);
    let stdin = child.stdin.take().ok_or("MCP 子进程 stdin 不可用")?;
    let stdout = child.stdout.take().ok_or("MCP 子进程 stdout 不可用")?;
    // W2 修复:Child 必须有人持有并 wait——kill_on_drop(true) 下被丢弃会
    // 立刻杀死子进程(热装载路径 spawn_generation 返回即 drop,连接器
    // 尚未建立路由,表现为 stdio-closed)。移入看护任务自然等待。
    let command_owned = command.to_string();
    tokio::spawn(async move {
        let status = child.wait().await;
        eprintln!("MCP 子进程退出: command={command_owned} status={status:?}");
    });

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
                            let _ = tx.send(Ok(msg.get("result").cloned().unwrap_or(json!({}))));
                        }
                    }
                }
            } else if msg.get("method").and_then(|v| v.as_str()) == Some("notifications/progress") {
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
        alive.store(false, std::sync::atomic::Ordering::Relaxed);
        let mut map = pending_reader.lock().expect("锁未中毒");
        for (_, tx) in map.drain() {
            let _ = tx.send(Err("stdio-closed".into()));
        }
    });

    Ok((pending, stdin, progress_rx))
}

impl StdioMcpTransport {
    /// 重生一代子进程(M7.4:下次调用重连)。旧代在途请求以
    /// stdio-closed 收场(内核侧计为一次失败/探针)。
    async fn respawn(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        {
            let mut map = inner.pending.lock().expect("锁未中毒");
            for (_, tx) in map.drain() {
                let _ = tx.send(Err("stdio-closed".into()));
            }
        }
        let (pending, stdin, progress_rx) =
            spawn_generation(&self.command, &self.args, &self.env, self.alive.clone())?;
        inner.pending = pending;
        inner.stdin = Some(stdin);
        let mut slot = self.progress_rx.lock().expect("锁未中毒");
        if slot.is_none() {
            *slot = Some(progress_rx); // 首代订阅位空缺时续上重生代进度
        }
        self.alive.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
}

#[async_trait]
impl McpTransport for StdioMcpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        if !self.alive.load(std::sync::atomic::Ordering::Relaxed) {
            self.respawn().await?;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut inner = self.inner.lock().await;
            inner.next_id += 1;
            let id = inner.next_id;
            inner.pending.lock().expect("锁未中毒").insert(id, tx);
            if let Some(tok) = params
                .get("_meta")
                .and_then(|m| m.get("progressToken"))
                .and_then(|v| v.as_str())
            {
                inner.token_to_id.insert(tok.to_string(), id);
            }
            let stdin = inner.stdin.as_mut().expect("stdin 在活着时存在");
            let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            let mut bytes = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
            bytes.push('\n');
            write_frame(stdin, bytes.as_bytes()).await?;
        }
        let out = match rx.await {
            Ok(r) => r,
            Err(_) => Err("stdio-closed".into()),
        };
        {
            let mut inner = self.inner.lock().await;
            if let Some(tok) = params
                .get("_meta")
                .and_then(|m| m.get("progressToken"))
                .and_then(|v| v.as_str())
            {
                inner.token_to_id.remove(tok);
            }
        }
        out
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        let stdin = inner.stdin.as_mut().expect("stdin 在活着时存在");
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let mut bytes = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
        bytes.push('\n');
        write_frame(stdin, bytes.as_bytes()).await
    }

    fn subscribe_progress(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
        self.progress_rx
            .lock()
            .expect("锁未中毒")
            .take()
            .unwrap_or_else(dead_progress_rx)
    }

    fn cancel_by_token(&self, token: &str) {
        let inner = self.inner.clone();
        let token = token.to_string();
        tokio::spawn(async move {
            let mut guard = inner.lock().await;
            if let Some(id) = guard.token_to_id.remove(&token)
                && let Some(stdin) = guard.stdin.as_mut()
            {
                let msg = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/cancelled",
                    "params": {"requestId": id}
                });
                if let Ok(mut bytes) = serde_json::to_vec(&msg) {
                    bytes.push(b'\n');
                    let _ = write_frame(stdin, &bytes).await;
                }
            }
        });
    }
}

// ---- HTTP / SSE 远程传输 (Remote HTTP/SSE Transport) -----------------------

/// 远程 MCP HTTP/SSE 传输层 (单进程 HTTP POST + 可选 Bearer 鉴权).
pub struct HttpMcpTransport {
    url: String,
    bearer_token: Option<String>,
    client: reqwest::Client,
    progress_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>>>,
}

impl HttpMcpTransport {
    pub fn new(url: &str, bearer_token: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            url: url.to_string(),
            bearer_token,
            client: reqwest::Client::new(),
            progress_rx: Mutex::new(None),
        })
    }
}

#[async_trait]
impl McpTransport for HttpMcpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        let mut req = self.client.post(&self.url).json(&msg);
        if let Some(token) = &self.bearer_token {
            req = req.bearer_auth(token);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("远程 MCP 请求失败: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("远程 MCP HTTP 状态异常: {}", resp.status()));
        }
        let val: Value = resp
            .json()
            .await
            .map_err(|e| format!("远程 MCP 响应解析失败: {e}"))?;
        if let Some(err) = val.get("error") {
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("");
            return Err(format!("rpc-error:{code} {msg}"));
        }
        Ok(val.get("result").cloned().unwrap_or(json!({})))
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let mut req = self.client.post(&self.url).json(&msg);
        if let Some(token) = &self.bearer_token {
            req = req.bearer_auth(token);
        }
        let _ = req.send().await;
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
    /// 在途调用:operation_id → 传输(取消通知定位)。
    inflight: Mutex<HashMap<String, Arc<dyn McpTransport>>>,
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
            inflight: Mutex::new(HashMap::new()),
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
    /// W2 管理面探活:对该 server 的任一路由发 tools/list(轻量、无副作用)。
    /// 返回 Ok(工具数) = 联通;Err = 断连/超时摘要。
    pub async fn probe_server(&self, server: &str) -> Result<usize, String> {
        let prefix = format!("mcp.{server}.");
        let transport = {
            let routes = self.routes.lock().expect("锁未中毒");
            routes
                .iter()
                .find(|(k, _)| k.starts_with(&prefix))
                .map(|(_, r)| r.transport.clone())
        };
        let Some(transport) = transport else {
            return Err("未连接".into());
        };
        let listed = transport.request("tools/list", json!({})).await?;
        Ok(listed
            .get("tools")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0))
    }

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

    /// 热拔/重载摘除指定 server 的全部路由，并向 transport 发送 shutdown 通知。
    /// 返回被摘除的能力列表(用于通知 Registry 和 Persist 摘除)。
    pub async fn disconnect_server(&self, server: &str) -> Vec<String> {
        let prefix = format!("mcp.{server}.");
        let (removed_caps, transports) = {
            let mut routes = self.routes.lock().expect("锁未中毒");
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
        self.inflight
            .lock()
            .expect("锁未中毒")
            .insert(operation_id.to_string(), route.0.clone());
        let resp = tokio::select! {
            _ = tokio::time::sleep(deadline) => {
                self.inflight
                    .lock()
                    .expect("锁未中毒")
                    .remove(operation_id);
                return Err(AsyncCallError::Timeout);
            }
            r = route.0.request("tools/call", req) => {
                self.inflight
                    .lock()
                    .expect("锁未中毒")
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

    fn cancel_op(&self, operation_id: &str) {
        let transport = self.inflight.lock().expect("锁未中毒").remove(operation_id);
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
}

/// 从配置文件装载 MCP server 安装清单(每项过 mcp-server.v0_1 合同校验)。
/// 文件显式列出 = 用户安装批准;env/bearer_token 一律 secret: 引用(明文拒绝由合同承担)。
pub fn load_mcp_setups(
    path: &std::path::Path,
    store: &dyn bm_core::ports::SecretStore,
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
            eprintln!("[MCP] 配置项 {name} 合同校验失败 (已跳过): {e}");
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
                    eprintln!("[MCP] 配置项 {name} bearer_token 引用 {tok_ref} 解析失败: {e:?}");
                    continue;
                }
            }
        }

        let mut env_resolved = HashMap::new();
        let mut env_err = false;
        if let Some(env) = item.get("env").and_then(|v| v.as_object()) {
            for (k, v) in env {
                let Some(ref_) = v.as_str() else {
                    eprintln!("[MCP] 配置项 {name} env {k} 值不是字符串 (已跳过该服务)");
                    env_err = true;
                    break;
                };
                match bm_core::ports::SecretStore::get(store, ref_) {
                    Ok(value) => {
                        env_resolved.insert(k.clone(), value);
                    }
                    Err(e) => {
                        eprintln!(
                            "[MCP] 配置项 {name} env {k} 密钥引用 {ref_} 解析失败 (已跳过该服务): {e:?}"
                        );
                        env_err = true;
                        break;
                    }
                }
            }
        }
        if env_err {
            continue;
        }
        // 外部评审 2026-09-03 #2:完整性校验——条目带 sha256 时复验可执行
        // 文件哈希,不符拒载(防安装后二进制被替换);无 sha256 的旧条目照常。
        if let Some(expected) = item.get("sha256").and_then(|v| v.as_str()) {
            let command = item["command"].as_str().unwrap_or_default();
            if let Err(e) = verify_integrity(command, expected) {
                eprintln!("[MCP] 配置项 {name} 完整性校验不符 (已跳过,疑似被替换): {e}");
                continue;
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
                .unwrap_or(3) as u32,
        });
    }
    Ok(out)
}

/// 外部评审 2026-09-03 #2:插件文件 SHA-256(批准接入时记录)。
pub fn sha256_file(path: &str) -> Result<String, String> {
    use sha2::Digest;
    let bytes = std::fs::read(path).map_err(|e| format!("读取 {path} 失败: {e}"))?;
    let mut h = sha2::Sha256::new();
    h.update(&bytes);
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
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

#[cfg(test)]
mod x05_tests {
    use super::*;
    use serde_json::json;

    fn def(name: &str, annotations: serde_json::Value) -> McpToolDef {
        McpToolDef {
            name: name.into(),
            description: None,
            input_schema: json!({"type": "object"}),
            annotations,
        }
    }

    /// X-05:readOnly+destructive 并存 → external-side-effect + required
    /// (元数据只能提高风险,不能降级为免审批只读)。
    #[test]
    fn conflicting_annotations_escalate() {
        let m = tool_manifest(
            "srv",
            &def("t", json!({"readOnlyHint": true, "destructiveHint": true})),
            1000,
        )
        .expect("manifest");
        assert_eq!(m.effect.as_str(), "external-side-effect");
        assert_eq!(
            m.approval,
            bm_contract::capability::ApprovalRequirement::Required
        );
    }

    #[test]
    fn read_only_only_stays_passthrough() {
        let m =
            tool_manifest("srv", &def("t", json!({"readOnlyHint": true})), 1000).expect("manifest");
        assert_eq!(m.effect.as_str(), "read-only");
        assert_eq!(
            m.approval,
            bm_contract::capability::ApprovalRequirement::NotRequired
        );
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

    /// P0(第四轮评审)验收:子进程继承面只含白名单——主密钥等父进程
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
        let setups = load_mcp_setups(&cfg, &store).expect("解析");
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
        let setups = load_mcp_setups(&cfg, &store).expect("解析");
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
}
