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

/// 一行子进程 stderr:`gen` = 子进程代数(1 起,respawn 递增)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct StderrLine {
    pub generation: u64,
    pub text: String,
}

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

// ---- 数据形状 --------------------------------------------------------------

/// tools/list 条目(发现面)。
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    /// 工具功能描述(ADR-0022:进 manifest.description 面向模型展示)。
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

/// server 名归一(冻结能力名段字符集):「mcp.<server>」前缀组合的唯一真源,
/// capability/provider/路由前缀三处同用——连字符等非法字符原样拼进能力名,
/// 会撞冻结合同的 capability pattern(注册期门禁会拒)。不合法 → None,
/// 与工具名非法同口径(跳过/未连接)。
pub fn normalize_server_name(server: &str) -> Option<String> {
    normalize_tool_name(server)
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
    let server_norm = normalize_server_name(server)?;
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
    // ADR-0022:工具自描述进 manifest,对话工具清单不再丢描述。
    let mut manifest_json = json!({
        "capability": format!("mcp.{server_norm}.{tool_norm}"),
        "provider": format!("mcp.{server_norm}"),
        "version": "0.1.0",
        "input_schema": input_schema,
        "output_schema": {"type": "object"},
        "effect": effect,
        "idempotent": false,
        "cancellable": true,
        "timeout_ms": timeout_ms,
        "approval": approval,
        "scopes": [format!("domain:mcp.{server_norm}")],
        "execution_mode": "async",
    });
    if let Some(d) = tool
        .description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        manifest_json["description"] = json!(d);
    }
    serde_json::from_value(manifest_json).ok()
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

// ---- stdio 传输 ------------------------------------------------------------

/// stdio 子进程传输(newline-delimited JSON-RPC 2.0)。
/// 子进程退出后,下次 request 自动重生一代子进程(M7.4 重启语义;
/// 重连上限由内核健康面计量)。
pub struct StdioMcpTransport {
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    inner: Arc<tokio::sync::Mutex<StdioInner>>,
    /// #32:进度聚合通道接收端(订阅一次,跨 respawn 代不断线)。
    progress_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>>>,
    /// #32:进度聚合通道发送端(每次 spawn_generation 转发汇入)。
    progress_agg_tx: tokio::sync::mpsc::UnboundedSender<McpProgressNote>,
    alive: Arc<std::sync::atomic::AtomicBool>,
    /// 现行代的终止开关(Drop = 杀子进程;换代 = 换灯)。
    kill: Mutex<Option<ChildKill>>,
    /// respawn 时间窗(limits 默认 60s 滑动),配合 restart_limit 限流。
    respawn_times: Arc<Mutex<Vec<std::time::Instant>>>,
    /// R3:此前解析后零消费的死配置;现为 respawn 窗口上限。
    restart_limit: u32,
    /// W10(ADR-0024):窗口/写超时热读单元(缺省 = 代码默认)。
    limits: bm_core::limits::LimitsCell,
    /// 子进程 stderr 环形缓冲(issue #28):跨 respawn 共享,按代标记。
    stderr: Arc<StderrBuffer>,
    /// 当前子进程代数(1 起;respawn 递增,stderr 行随代标记)。
    generation: Arc<std::sync::atomic::AtomicU64>,
    /// ADR-0035 §4:各代子进程 OS 级资源上限(respawn 沿用)。
    sandbox: bm_sandbox::SandboxLimits,
}

impl Drop for StdioMcpTransport {
    fn drop(&mut self) {
        // 闭灯 = 看护任务 start_kill:reload/换装/销毁不再留僵尸
        // (此前只发 shutdown/exit 通知,插件不理会即悬挂)。
        if let Some(kill) = self
            .kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            drop(kill);
        }
    }
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
        restart_limit: u32,
    ) -> Result<Arc<Self>, String> {
        // 无显式上限(测试/直接调用):空操作,行为与既有完全一致。
        Self::spawn_with_sandbox(
            command,
            args,
            env,
            restart_limit,
            bm_sandbox::SandboxLimits::default(),
        )
    }

    /// ADR-0035 §4:带 OS 级资源上限的 spawn(生产装配走此入口)。
    pub fn spawn_with_sandbox(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        restart_limit: u32,
        sandbox: bm_sandbox::SandboxLimits,
    ) -> Result<Arc<Self>, String> {
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let stderr = Arc::new(StderrBuffer::with_capacity(MCP_STDERR_CAPACITY));
        let generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
        // #32:聚合通道 = 订阅端唯一数据源,跨代不断线
        let (agg_tx, agg_rx) = tokio::sync::mpsc::unbounded_channel();
        let (pending, stdin, kill) = spawn_generation(
            command,
            args,
            env,
            SpawnCtx {
                alive: alive.clone(),
                stderr_buf: stderr.clone(),
                generation: generation.clone(),
                progress_agg: agg_tx.clone(),
                sandbox,
            },
        )?;
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
            progress_rx: Mutex::new(Some(agg_rx)),
            progress_agg_tx: agg_tx,
            alive,
            kill: Mutex::new(Some(kill)),
            respawn_times: Arc::new(Mutex::new(Vec::new())),
            restart_limit: restart_limit.max(1),
            limits: bm_core::limits::LimitsCell::with_default(),
            stderr,
            generation,
            sandbox,
        }))
    }

    /// W10:注入共享 limits 单元(重生窗口/写超时随之;supervisor 装配用)。
    pub fn with_limits(mut self: Arc<Self>, limits: bm_core::limits::LimitsCell) -> Arc<Self> {
        // Arc 内不可变字段:借 Cell 共享即可,无需可变——字段本身是 Cell。
        let _ = &mut self;
        let s = Arc::into_inner(self).expect("supervisor 装配期独占");
        let mut s = s;
        s.limits = limits;
        Arc::new(s)
    }

    fn respawn_window(&self) -> Duration {
        Duration::from_millis(self.limits.get().mcp_respawn_window_ms)
    }

    fn write_timeout(&self) -> Duration {
        Duration::from_millis(self.limits.get().mcp_stdio_write_timeout_ms)
    }

    /// 子进程 stderr 尾部(旧→新,带代标记;issue #28)。
    pub fn stderr_tail(&self, lines: usize) -> Vec<StderrLine> {
        self.stderr.tail(lines)
    }
}

/// 子进程终止开关:持有方丢弃(或显式 drop)= 看护任务 start_kill 子进程。
/// R3(FULL-REVIEW-2026-09-05 §7):此前看护任务独占 Child,kill_on_drop
/// 永不触发,reload 只发 shutdown 通知 = 插件不理会就僵尸。
pub type ChildKill = tokio::sync::oneshot::Sender<()>;

/// 跨代共享的 spawn 上下文(存活标志/代数/进度聚合/OS 上限)。
struct SpawnCtx {
    alive: Arc<std::sync::atomic::AtomicBool>,
    stderr_buf: Arc<StderrBuffer>,
    generation: Arc<std::sync::atomic::AtomicU64>,
    /// #32:各代进度统一汇入同一聚合通道(订阅端跨代不断线)。
    progress_agg: tokio::sync::mpsc::UnboundedSender<McpProgressNote>,
    /// ADR-0035 §4:OS 级资源上限(尽力而为;空操作时跳过)。
    sandbox: bm_sandbox::SandboxLimits,
}

/// 拉起一代子进程:返回在途表 / stdin / 终止开关。
/// `ctx.stderr_buf`/`ctx.generation` 跨代共享:代数递增写入缓冲行标记(issue #28)。
fn spawn_generation(
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
    ctx: SpawnCtx,
) -> Result<(PendingMap, tokio::process::ChildStdin, ChildKill), String> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    let SpawnCtx {
        alive,
        stderr_buf,
        generation,
        progress_agg,
        sandbox,
    } = ctx;
    let no = generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
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
        // issue #28:stderr 由 inherit 直通改为管道采集入环形缓冲
        // (此前子进程报错混入 server.log,无按插件回看通道)。
        .stderr(std::process::Stdio::piped());
    // 外部审计:kill_on_drop 绑定子进程生命周期——连接器对象被丢弃时
    // 子进程随之终止,防止服务端异常退出后 Python App 成为孤儿进程。
    cmd.kill_on_drop(true);
    // ADR-0035 §4:Unix 在 exec 前经 pre_exec 施加 rlimit(fork 后仅
    // async-signal-safe 操作)。失败只告警不阻断(fail-open,与「单插件
    // 失败不中止装载」一致);Windows 的 Job Object 需 pid,spawn 后施加。
    if let Err(e) = bm_sandbox::pre_spawn(&mut cmd, &sandbox) {
        tracing::warn!(command = %command, error = %e, "MCP 子进程 rlimit 施加失败(继续,不加限)");
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("MCP 子进程启动失败: {e}"))?;
    tracing::info!(pid = ?child.id(), command = %command, generation = no, "MCP 子进程已拉起");
    // ADR-0035 §4:Windows 对已 spawn 的子进程纳入 Job Object(内存/活动进程
    // 上限 + KILL_ON_JOB_CLOSE)。守卫随本代子进程看护任务同寿命。
    let sandbox_guard = child.id().map(|pid| {
        let g = bm_sandbox::post_spawn(pid, &sandbox);
        tracing::info!(pid, command = %command, "MCP 子进程资源上限已施加");
        g
    });
    let stdin = child.stdin.take().ok_or("MCP 子进程 stdin 不可用")?;
    let stdout = child.stdout.take().ok_or("MCP 子进程 stdout 不可用")?;
    let stderr_pipe = child.stderr.take().ok_or("MCP 子进程 stderr 不可用")?;
    // W2 修复:Child 必须有人持有并 wait——kill_on_drop(true) 下被丢弃会
    // 立刻杀死子进程(热装载路径 spawn_generation 返回即 drop,连接器
    // 尚未建立路由,表现为 stdio-closed)。移入看护任务自然等待。
    let command_owned = command.to_string();
    let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let mut child = child;
        // ADR-0035:Job Object 守卫随看护任务同寿命——任务退出(子进程已死
        // 或被 kill)方 drop 守卫,避免 KILL_ON_JOB_CLOSE 过早杀子进程。
        let _sandbox_guard = sandbox_guard;
        tokio::select! {
            status = child.wait() => {
                tracing::info!(command = %command_owned, status = ?status, "MCP 子进程退出");
            }
            _ = kill_rx => {
                if let Err(e) = child.start_kill() {
                    tracing::warn!(command = %command_owned, error = %e, "MCP 子进程 kill 失败");
                }
                if let Err(e) = child.wait().await {
                    tracing::warn!(command = %command_owned, error = %e, "MCP 子进程 wait 失败(疑似残留)");
                }
                tracing::info!(command = %command_owned, "MCP 子进程被终止(reload/换装/销毁)");
            }
        }
    });

    // stderr 泵:本代子进程 stderr → 环形缓冲(带代标记;起止哨兵行助读)
    {
        let buf = stderr_buf.clone();
        tokio::spawn(async move {
            buf.push(no, format!("── 第 {no} 代子进程启动 ──"));
            let reader = BufReader::new(stderr_pipe);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                buf.push(no, line);
            }
            buf.push(no, format!("── 第 {no} 代子进程 stderr 关闭 ──"));
        });
    }

    // #32:每代独立 channel 经转发任务汇入聚合通道——上一代关闭不影响
    // 聚合端,重生代进度自动续流(修复单代订阅:重生后进度静默丢失)
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    {
        let progress_agg = progress_agg.clone();
        tokio::spawn(async move {
            while let Some(note) = progress_rx.recv().await {
                let _ = progress_agg.send(note);
            }
        });
    }
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
                let slot = pending_reader
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&id);
                if let Some(tx) = slot {
                    match msg.get("error") {
                        Some(_) => {
                            let _ = tx.send(Err(format!(
                                "rpc-error:{}",
                                rpc::error_code(&msg).unwrap_or(-1)
                            )));
                        }
                        None => {
                            let _ = tx.send(Ok(rpc::take_result(&msg)));
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
        let mut map = pending_reader
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (_, tx) in map.drain() {
            let _ = tx.send(Err("stdio-closed".into()));
        }
    });

    Ok((pending, stdin, kill_tx))
}

impl StdioMcpTransport {
    async fn request_once(&self, method: &str, params: Value) -> Result<Value, String> {
        if !self.alive.load(std::sync::atomic::Ordering::Relaxed) {
            self.respawn().await?;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        let mut registered_id: Option<u64> = None;
        let write_result = (async {
            let mut inner = self.inner.lock().await;
            inner.next_id += 1;
            let id = inner.next_id;
            inner
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(id, tx);
            registered_id = Some(id);
            if let Some(tok) = params
                .get("_meta")
                .and_then(|m| m.get("progressToken"))
                .and_then(|v| v.as_str())
            {
                inner.token_to_id.insert(tok.to_string(), id);
            }
            let stdin = inner.stdin.as_mut().expect("stdin 在活着时存在");
            // params 其后仍用于 token 映射清理(codec 取所有权),此处克隆。
            let msg = rpc::request(id, method, params.clone());
            let mut bytes = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
            bytes.push('\n');
            write_frame(stdin, bytes.as_bytes(), self.write_timeout()).await
        })
        .await;
        // 2026-09-05 回看修复:写失败必须在返回前清账,否则 pending/token
        // 映射随失败累积泄漏(长跑守护进程内存无界上爬)。
        if let Err(e) = write_result {
            if let Some(id) = registered_id {
                let mut inner = self.inner.lock().await;
                inner
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&id);
                inner.token_to_id.retain(|_, v| *v != id);
            }
            return Err(e);
        }
        let id = registered_id.expect("写入成功必有 id");
        let out = match rx.await {
            Ok(r) => r,
            Err(_) => Err("stdio-closed".into()),
        };
        // 收尾清账:pending 常态由读取端按响应清理,此处 remove 幂等兜底
        {
            let mut inner = self.inner.lock().await;
            inner
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id);
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

    /// 重生一代子进程(M7.4:下次调用重连)。旧代在途请求以
    /// stdio-closed 收场(内核侧计为一次失败/探针)。
    async fn respawn(&self) -> Result<(), String> {
        // R3(FULL-REVIEW-2026-09-05 §7):respawn 去抖+上限——60s 滑动窗口
        // 内重生次数达 restart_limit = 故障循环,拒绝再生如实报错(此前
        // restart_limit 是解析后零消费的死配置)。
        {
            let mut times = self
                .respawn_times
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let now = std::time::Instant::now();
            let window = self.respawn_window();
            times.retain(|t| now.duration_since(*t) < window);
            if times.len() >= self.restart_limit as usize {
                // P2(2026-09-07 架构评审):窗口秒数随 limits 热值,不再写死 60。
                return Err(format!(
                    "MCP 子进程 {} 秒内已重生 {} 次(上限 {}),疑似故障循环已熔断;请检查插件或经管理面重载",
                    window.as_secs(),
                    times.len(),
                    self.restart_limit
                ));
            }
            times.push(now);
        }
        let mut inner = self.inner.lock().await;
        {
            let mut map = inner
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (_, tx) in map.drain() {
                let _ = tx.send(Err("stdio-closed".into()));
            }
        }
        // 换代前闭灯杀旧代(消灭僵尸窗口),再挂新开关
        if let Some(old_kill) = self
            .kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            drop(old_kill);
        }
        let (pending, stdin, kill) = spawn_generation(
            &self.command,
            &self.args,
            &self.env,
            SpawnCtx {
                alive: self.alive.clone(),
                stderr_buf: self.stderr.clone(),
                generation: self.generation.clone(),
                progress_agg: self.progress_agg_tx.clone(),
                sandbox: self.sandbox,
            },
        )?;
        *self
            .kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(kill);
        inner.pending = pending;
        inner.stdin = Some(stdin);
        // #32:进度经聚合通道自动续流,订阅位无需重生代回填
        self.alive.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
}

#[async_trait]
impl McpTransport for StdioMcpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        // #32:命中「子进程已死但 alive 未及置否」窗口的请求以 stdio-closed
        // 收场——此时强制重生一代并重试一次(restart_limit 去抖仍然生效),
        // 让 M7.4「下次调用重连」语义覆盖死亡窗口内的在途请求。
        match self.request_once(method, params.clone()).await {
            Err(e) if e == "stdio-closed" => {
                self.alive
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                self.respawn().await?;
                self.request_once(method, params).await
            }
            out => out,
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        let stdin = inner.stdin.as_mut().expect("stdin 在活着时存在");
        let msg = rpc::notification(method, params);
        let mut bytes = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
        bytes.push('\n');
        write_frame(stdin, bytes.as_bytes(), self.write_timeout()).await
    }

    fn subscribe_progress(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
        take_progress_rx(&self.progress_rx)
    }

    fn cancel_by_token(&self, token: &str) {
        let inner = self.inner.clone();
        let token = token.to_string();
        let write_timeout = self.write_timeout();
        tokio::spawn(async move {
            let mut guard = inner.lock().await;
            if let Some(id) = guard.token_to_id.remove(&token)
                && let Some(stdin) = guard.stdin.as_mut()
            {
                let msg = rpc::notification("notifications/cancelled", json!({"requestId": id}));
                if let Ok(mut bytes) = serde_json::to_vec(&msg) {
                    bytes.push(b'\n');
                    let _ = write_frame(stdin, &bytes, write_timeout).await;
                }
            }
        });
    }

    fn stderr_buffer(&self) -> Option<Arc<StderrBuffer>> {
        Some(self.stderr.clone())
    }
}

// ---- HTTP / SSE 远程传输 (Remote HTTP/SSE Transport) -----------------------

/// 远程 MCP HTTP/SSE 传输层(Streamable HTTP:单 POST + 可选 Bearer +
/// Mcp-Session-Id 会话管理 + SSE 响应流解析;issue #3 完整握手)。
pub struct HttpMcpTransport {
    url: String,
    bearer_token: Option<String>,
    client: reqwest::Client,
    progress_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>>>,
    /// W10(ADR-0024):单请求超时热读单元(缺省 60s)。
    limits: bm_core::limits::LimitsCell,
    /// #3:服务器下发的会话 id(Mcp-Session-Id 响应头),后续请求回带。
    session_id: Mutex<Option<String>>,
    /// #3:initialize 结果(协议版本/capabilities/serverInfo),管理面露出。
    init_result: Mutex<Option<Value>>,
}

impl HttpMcpTransport {
    pub fn new(url: &str, bearer_token: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            url: url.to_string(),
            bearer_token,
            client: reqwest::Client::new(),
            progress_rx: Mutex::new(None),
            limits: bm_core::limits::LimitsCell::with_default(),
            session_id: Mutex::new(None),
            init_result: Mutex::new(None),
        })
    }

    /// W10:注入共享 limits 单元(supervisor 装配用)。
    pub fn with_limits(self: Arc<Self>, limits: bm_core::limits::LimitsCell) -> Arc<Self> {
        let s = Arc::into_inner(self).expect("supervisor 装配期独占");
        let mut s = s;
        s.limits = limits;
        Arc::new(s)
    }
}

/// SSE 解析已知协议版本(spec 2024-11-05/2025-03-26/2025-06-18);未知版本
/// 告警不拒——工具调用面在版本间兼容,拒了反断可用网关(#3 协商口径)。
const KNOWN_PROTOCOL_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

/// 从 SSE 文本(text/event-stream)提取 id 匹配的 JSON-RPC 响应:以空行分块,
/// `data:` 行拼合为一条 JSON(#3;POST 作用域的响应流,服务器发完即关)。
fn parse_sse_response(body: &str, id: u64) -> Result<Value, String> {
    for block in body.split(
        "

",
    ) {
        let data: Vec<&str> = block
            .lines()
            .filter(|l| l.starts_with("data:"))
            .map(|l| l.trim_start_matches("data:").trim_start_matches(' '))
            .collect();
        if data.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&data.join(
            "
",
        )) else {
            continue;
        };
        if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
            return Ok(v);
        }
    }
    Err("SSE 流中未找到匹配 id 的响应".to_string())
}

impl HttpMcpTransport {
    /// request/notify 共用的 POST 组装:bearer + Accept 双类型(spec 要求,
    /// 服务端可回 SSE 流)+ 会话头回带(initialize 后服务器下发 Mcp-Session-Id)。
    fn post_rpc(&self, msg: Value) -> reqwest::RequestBuilder {
        let mut req = self.client.post(&self.url).json(&msg);
        if let Some(token) = &self.bearer_token {
            req = req.bearer_auth(token);
        }
        req = req.header("Accept", "application/json, text/event-stream");
        if let Some(sid) = self
            .session_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            req = req.header("Mcp-Session-Id", sid);
        }
        req
    }
}

#[async_trait]
impl McpTransport for HttpMcpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let msg = rpc::request(1, method, params);
        let req = self.post_rpc(msg);
        // R3(FULL-REVIEW-2026-09-05 §7):裸 send 无超时 = 远端挂起即调用
        // 悬挂;默认 60s 硬顶(W10 走 limits),远端长任务应自行异步化。
        let remote_timeout =
            std::time::Duration::from_millis(self.limits.get().mcp_remote_timeout_ms);
        let resp = tokio::time::timeout(remote_timeout, req.send())
            .await
            // P2(2026-09-07 架构评审):秒数随 limits 热值,不再写死 60。
            .map_err(|_| {
                format!(
                    "远程 MCP 请求超时({}ms)",
                    self.limits.get().mcp_remote_timeout_ms
                )
            })?
            .map_err(|e| format!("远程 MCP 请求失败: {e}"))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND
            && self
                .session_id
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
        {
            return Err("远程 MCP 会话已失效(HTTP 404),请重载该 server".to_string());
        }
        if !resp.status().is_success() {
            return Err(format!("远程 MCP HTTP 状态异常: {}", resp.status()));
        }
        // #3:会话头记录(initialize 响应下发;后续请求回带)
        if let Some(sid) = resp
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
        {
            *self
                .session_id
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sid.to_string());
        }
        let is_sse = resp
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.contains("text/event-stream"));
        let val: Value = if is_sse {
            let body = resp
                .text()
                .await
                .map_err(|e| format!("远程 MCP SSE 流读取失败: {e}"))?;
            parse_sse_response(&body, 1)?
        } else {
            resp.json()
                .await
                .map_err(|e| format!("远程 MCP 响应解析失败: {e}"))?
        };
        if val.get("error").is_some() {
            let code = rpc::error_code(&val).unwrap_or(-1);
            let msg = rpc::error_message(&val).unwrap_or("");
            return Err(format!("rpc-error:{code} {msg}"));
        }
        Ok(rpc::take_result(&val))
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let msg = rpc::notification(method, params);
        let req = self.post_rpc(msg);
        // P1-9: notify 加上 10s 超时,防止半开远端挂死卸载/重载/关闭请求
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), req.send()).await;
        Ok(())
    }

    fn subscribe_progress(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
        take_progress_rx(&self.progress_rx)
    }

    fn remember_init(&self, v: Value) {
        *self
            .init_result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(v);
    }

    fn init_snapshot(&self) -> Option<Value> {
        self.init_result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
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
                tracing::warn!(server, version = %version, "MCP server 响应未知协议版本,按兼容继续");
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
                    None => tracing::warn!(server, tool = %name, "MCP 工具名不合规,拒注册"),
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
    ///
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
        // ADR-0035:trust 显式消费(缺省 explicit-config,即「配置显式列出=
        // 安装批准」;非枚举值已在上面合同校验步被拒)。解析值随条目留痕,
        // 使来源在装载日志可见。
        let trust = item
            .get("trust")
            .and_then(|v| v.as_str())
            .unwrap_or("explicit-config");
        eprintln!("[MCP] 配置项 {name} trust={trust}(来源显式配置)");

        // 外部评审 2026-09-03 #2 + ADR-0035:完整性校验——条目带 sha256 时复验
        // 「校验目标」哈希,不符拒载(防安装后被替换)。校验目标 = payload(若
        // 声明)否则 command;解释器型条目(command 非本地文件、args 指向本地
        // 脚本)未声明 payload 时语义歧义,fail-closed 拒载,杜绝「哈希解释器
        // 却宣称校验了插件」的假保证。
        if let Some(expected) = item.get("sha256").and_then(|v| v.as_str()) {
            match resolve_integrity_target(item) {
                Ok(target) => {
                    if let Err(e) = verify_integrity(&target, expected) {
                        eprintln!(
                            "[MCP] 配置项 {name} 完整性校验不符 (已跳过,疑似被替换;目标 {target}): {e}"
                        );
                        continue;
                    }
                }
                Err(e) => {
                    eprintln!("[MCP] 配置项 {name} 完整性校验目标不明确 (已跳过): {e}");
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
                .unwrap_or(3) as u32,
            trust: trust.to_string(),
            payload: item
                .get("payload")
                .and_then(|v| v.as_str())
                .map(String::from),
        });
    }
    Ok(out)
}

/// 外部评审 2026-09-03 #2:插件文件 SHA-256(批准接入时记录)。
pub fn sha256_file(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 {path} 失败: {e}"))?;
    Ok(bm_contract::hash::sha256_hex(&bytes))
}

/// ADR-0035:依条目解析完整性校验目标。
///
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
        let setups = load_mcp_setups(&cfg, &store).expect("解析");
        assert_eq!(setups.len(), 1);
        assert_eq!(
            setups[0].payload.as_deref(),
            Some(script.display().to_string().as_str())
        );

        // 脚本被替换 -> 哈希不符 -> 拒载(即使解释器本身未变)
        std::fs::write(&script, b"print('v2-evil')").expect("改脚本");
        let setups = load_mcp_setups(&cfg, &store).expect("解析");
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
        let setups = load_mcp_setups(&cfg, &store).expect("解析");
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
        let setups = load_mcp_setups(&cfg, &store).expect("解析");
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
        let setups = load_mcp_setups(&cfg, &store).expect("解析");
        assert!(setups.is_empty(), "非枚举 trust 必须被合同校验拒");
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

pub mod supervisor;

/// issue #3:远程 MCP 完整握手协商——本地 axum mock Streamable HTTP server。
/// t1 握手:initialize 记录 session id/capabilities/版本,tools/list 回带
/// Mcp-Session-Id;t2 tools/call 走 SSE 流响应可解析;t3 未知协议版本容忍。
#[cfg(test)]
mod remote_http_tests {
    use super::*;
    use axum::extract::Request;
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;

    /// 起一个极简 Streamable HTTP MCP mock:initialize 下发会话头,
    /// tools/list 校验会话头,tools/call 以 SSE 流回响应。
    async fn spawn_mock(unknown_version: bool) -> String {
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::post(move |headers: HeaderMap, req: Request| async move {
                let bytes = axum::body::to_bytes(req.into_body(), 1 << 20)
                    .await
                    .unwrap();
                let msg: Value = serde_json::from_slice(&bytes).unwrap();
                let id = msg.get("id").and_then(|v| v.as_u64());
                let method = msg["method"].as_str().unwrap_or("");
                let session = headers
                    .get("Mcp-Session-Id")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let result = match method {
                    "initialize" => {
                        let version = if unknown_version {
                            "1999-01-01"
                        } else {
                            "2025-03-26"
                        };
                        json!({
                            "protocolVersion": version,
                            "capabilities": {"tools": {"listChanged": false}},
                            "serverInfo": {"name": "mock-remote", "version": "0.1"}
                        })
                    }
                    "tools/list" => {
                        assert_eq!(session.as_deref(), Some("mock-session-1"), "tools/list 必须回带会话头");
                        json!({"tools": [{"name": "echo", "inputSchema": {"type": "object"}}]})
                    }
                    "tools/call" => {
                        assert_eq!(session.as_deref(), Some("mock-session-1"));
                        return (
                            axum::http::StatusCode::OK,
                            [("Content-Type", "text/event-stream"), ("Mcp-Session-Id", "mock-session-1")],
                            format!(
                                "event: message
data: {{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":\"sse-pong\"}}]}}}}

",
                                id.unwrap_or(1)
                            ),
                        )
                            .into_response();
                    }
                    _ => json!({}),
                };
                let body = json!({"jsonrpc": "2.0", "id": id, "result": result});
                (
                    axum::http::StatusCode::OK,
                    [("Mcp-Session-Id", "mock-session-1")],
                    axum::Json(body),
                )
                    .into_response()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/mcp")
    }

    async fn handshake(url: &str, unknown_version: bool) -> Arc<McpHub> {
        let transport = HttpMcpTransport::new(url, None);
        let hub = McpHub::new();
        let manifests = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            hub.connect("remote", transport, 5_000),
        )
        .await
        .expect("握手超时")
        .expect("握手成功");
        assert_eq!(manifests.len(), 1, "tools/list 应发现 1 工具");
        let caps = hub.server_capabilities("remote").expect("握手记录存在");
        assert_eq!(caps["serverInfo"]["name"], "mock-remote");
        let _ = unknown_version;
        hub
    }

    #[tokio::test]
    async fn t_remote_handshake_session_and_capabilities() {
        let url = spawn_mock(false).await;
        let hub = handshake(&url, false).await;
        let caps = hub.server_capabilities("remote").unwrap();
        assert_eq!(caps["protocolVersion"], "2025-03-26", "协商记录响应版本");
        assert_eq!(caps["capabilities"]["tools"]["listChanged"], false);
        // tools/call 全链路:SSE 响应流解析 + 会话头回带
        let out = bm_core::ports::AsyncCapabilityExecutor::call(
            hub.as_ref(),
            "op_remote",
            "mcp.remote.echo",
            json!({"text": "x"}),
            std::time::Duration::from_secs(5),
        )
        .await
        .expect("远程调用成功");
        assert!(out.to_string().contains("sse-pong"), "{out}");
    }

    #[tokio::test]
    async fn t_remote_unknown_protocol_version_tolerated() {
        let url = spawn_mock(true).await;
        handshake(&url, true).await;
    }
}

/// issue #32:多代 stdio 进度聚合——重生后进度经聚合通道续流到原订阅。
/// 夹具 DIE_AFTER=3:第 1 代答完 initialize/tools/list/tools/call 后退出;
/// 第 2 次调用触发 respawn,新代进度必须流入 connect 期取走的同一订阅
/// (修复前:单代订阅位,重生代进度静默丢失)。真子进程测试沿用
/// #[ignore] + BOEN_MCP_STDIO_TEST=1 惯例。
#[cfg(test)]
mod progress_gen_tests {
    use super::*;
    use std::time::Duration;

    const FIXTURE: &str = r#"import json, sys, os
die_after = int(os.environ.get("DIE_AFTER", "0") or 0)
answered = 0
def send(obj):
    sys.stdout.write(json.dumps(obj) + chr(10))
    sys.stdout.flush()
while True:
    line = sys.stdin.readline()
    if not line:
        break
    line = line.strip()
    if not line:
        continue
    try:
        msg = json.loads(line)
    except ValueError:
        continue
    method = msg.get("method", "")
    mid = msg.get("id")
    if mid is None:
        continue
    params = msg.get("params", {}) or {}
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {"name": "progress-mcp"}}})
    elif method == "tools/list":
        send({"jsonrpc": "2.0", "id": mid, "result": {"tools": [{"name": "ping", "inputSchema": {"type": "object"}}]}})
    elif method == "tools/call":
        token = (params.get("_meta") or {}).get("progressToken", "t")
        send({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": token, "progress": 1, "message": "half"}})
        send({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": token, "progress": 2, "message": "done"}})
        send({"jsonrpc": "2.0", "id": mid, "result": {"content": [{"type": "text", "text": "pong"}]}})
    else:
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    answered += 1
    if die_after and answered >= die_after:
        sys.exit(0)
"#;

    /// 轮询 sink 收集面至攒够 want 条(异步到达;上限 5s)。
    async fn wait_notes(
        collected: &Arc<Mutex<Vec<ProgressNotice>>>,
        want: usize,
    ) -> Vec<ProgressNotice> {
        for _ in 0..50 {
            let cur = collected
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len();
            if cur >= want {
                break;
            }
            tokio::time::timeout(Duration::from_millis(100), async {})
                .await
                .ok();
        }
        collected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[tokio::test]
    #[ignore = "stdio 子进程测试:BOEN_MCP_STDIO_TEST=1 启用"]
    async fn progress_flows_across_respawn_generations() {
        let dir = tempfile::tempdir().expect("tmp");
        let fixture = dir.path().join("progress_mcp.py");
        std::fs::write(&fixture, FIXTURE).expect("写夹具");
        let env: HashMap<String, String> = [("DIE_AFTER".to_string(), "3".to_string())]
            .into_iter()
            .collect();
        let transport =
            StdioMcpTransport::spawn("python", &[fixture.to_string_lossy().to_string()], &env, 5)
                .expect("子进程启动");
        let hub = McpHub::new();
        let manifests = tokio::time::timeout(
            Duration::from_secs(15),
            hub.connect("genx", transport.clone(), 10_000),
        )
        .await
        .expect("握手超时")
        .map_err(|e| format!("握手失败: {e}; stderr={:?}", transport.stderr_tail(30)))
        .expect("握手成功");
        assert_eq!(manifests.len(), 1);

        // 进度经 hub 泵(订阅在 connect 期完成)汇入 sink;测试用 sink 收集
        let collected: Arc<Mutex<Vec<ProgressNotice>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_view = collected.clone();
        hub.set_progress_sink(Box::new(move |n| {
            sink_view
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(n);
        }));

        // 第 1 代调用:进度 2 条
        let r1 = bm_core::ports::AsyncCapabilityExecutor::call(
            hub.as_ref(),
            "op_g1",
            "mcp.genx.ping",
            json!({"_meta": {"progressToken": "tok-g1"}}),
            Duration::from_secs(10),
        )
        .await
        .expect("第 1 代调用成功");
        assert!(r1.to_string().contains("pong"));
        let notes1 = wait_notes(&collected, 2).await;
        assert_eq!(notes1.len(), 2, "第 1 代应收到 2 条进度");
        assert_eq!(notes1[0].message.as_deref(), Some("half"));
        assert_eq!(notes1[1].progress, 2);
        assert_eq!(notes1[1].message.as_deref(), Some("done"));

        // DIE_AFTER=3:第 1 代已退出;本调用触发 respawn → 第 2 代
        let r2 = bm_core::ports::AsyncCapabilityExecutor::call(
            hub.as_ref(),
            "op_g2",
            "mcp.genx.ping",
            json!({"_meta": {"progressToken": "tok-g2"}}),
            Duration::from_secs(10),
        )
        .await
        .map_err(|e| {
            format!(
                "gen2 失败: {e:?}; alive={}; stderr={:?}",
                transport.alive.load(std::sync::atomic::Ordering::Relaxed),
                transport.stderr_tail(40)
            )
        })
        .expect("第 2 代调用成功");
        assert!(r2.to_string().contains("pong"));
        let before = notes1.len();
        let notes2 = wait_notes(&collected, before + 2).await[before..].to_vec();
        assert_eq!(
            notes2.len(),
            2,
            "重生代进度必须续流到原订阅(修复单代订阅缺陷)"
        );
        assert_eq!(notes2[0].message.as_deref(), Some("half"));
        assert_eq!(notes2[1].progress, 2);
        assert_eq!(notes2[1].message.as_deref(), Some("done"));
    }
}

/// 评审修复(2026-09-10)回归:锁中毒必须自恢复,不得让一次 panic 把传输层
/// 永久毒化(此前 `.expect("锁未中毒")` 会在锁中毒后对每次调用逐次放大 panic)。
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
