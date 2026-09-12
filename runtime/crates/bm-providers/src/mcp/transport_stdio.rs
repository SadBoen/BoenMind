//! MCP stdio 子进程传输(自 mcp.rs 机械移出;ADR-0048)。
//! 含:子进程 spawn/重生、帧写入、stderr 环、进度聚合。

use super::*;

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
 /// R3:。
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
 // (。
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
/// R3(FULL-REVIEW-):
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
 // P0():子进程默认继承父进程全部环境 = 主密钥/令牌外泄
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
 // (。
        .stderr(std::process::Stdio::piped());
 // 外部审计:kill_on_drop 绑定子进程生命周期——连接器对象被丢弃时
 // 子进程随之终止,防止服务端异常退出后 Python App 成为孤儿进程。
    cmd.kill_on_drop(true);
 // ADR-0035 §4:Unix 在 exec 前经 pre_exec 施加 rlimit(fork 后仅
 // async-signal-safe 操作)。失败只告警不阻断(fail-open,与「单插件
 // 失败不中止装载」一致);Windows 的 Job Object 需 pid,spawn 后施加。
    if let Err(e) = bm_sandbox::pre_spawn(&mut cmd, &sandbox) {
        tracing::warn!(target: "plugin", command = %command, error = %e, "MCP 子进程 rlimit 施加失败(继续,不加限)");
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("MCP 子进程启动失败: {e}"))?;
    tracing::info!(target: "plugin", pid = ?child.id(), command = %command, generation = no, "MCP 子进程已拉起");
 // ADR-0035 §4:Windows 对已 spawn 的子进程纳入 Job Object(内存/活动进程
 // 上限 + KILL_ON_JOB_CLOSE)。守卫随本代子进程看护任务同寿命。
    let sandbox_guard = child.id().map(|pid| {
        let g = bm_sandbox::post_spawn(pid, &sandbox);
        tracing::info!(target: "plugin", pid, command = %command, "MCP 子进程资源上限已施加");
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
                tracing::info!(target: "plugin", command = %command_owned, status = ?status, "MCP 子进程退出");
            }
            _ = kill_rx => {
                if let Err(e) = child.start_kill() {
                    tracing::warn!(target: "plugin", command = %command_owned, error = %e, "MCP 子进程 kill 失败");
                }
                if let Err(e) = child.wait().await {
                    tracing::warn!(target: "plugin", command = %command_owned, error = %e, "MCP 子进程 wait 失败(疑似残留)");
                }
                tracing::info!(target: "plugin", command = %command_owned, "MCP 子进程被终止(reload/换装/销毁)");
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
 // 聚合端,重生代进度自动续流
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
 // R3(FULL-REVIEW-):respawn 去抖+上限——60s 滑动窗口
 // 内重生次数达 restart_limit = 故障循环,拒绝再生如实报错(
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
 // P2():窗口秒数随 limits 热值,不再写死 60。
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

/// issue #32:多代 stdio 进度聚合——重生后进度经聚合通道续流到原订阅。
/// 夹具 DIE_AFTER=3:第 1 代答完 initialize/tools/list/tools/call 后退出;
/// 第 2 次调用触发 respawn,新代进度必须流入 connect 期取走的同一订阅
/// 。真子进程测试沿用
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
