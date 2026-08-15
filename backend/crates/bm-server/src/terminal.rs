//! 终端会话（TerminalPane 一期，上游吸收 T2：portable-pty，见
//! backend/vendor/UPSTREAM_TRACKING.md）。
//!
//! 定位：用户显式操作的能力面（与 workspace 文件操作同级）——用户自己开的
//! 终端不触发插件权限询问、不进事件日志（模型命令可视化与审计留二期，届时
//! 走工具执行侧的事件链）。会话内存态，关闭/断开即弃。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tokio::sync::broadcast;

/// 终端输出事件（SSE 下行）：输出字节（base64，含任意字节）与进程退出。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TerminalEvent {
    Output { data: String },
    Exit { code: i32 },
}

/// 一个 pty 终端会话：writer 供输入，master 供 resize，读线程推输出。
/// 输出走 broadcast（多订阅者：SSE 可多开；迟订阅丢历史——终端输出无重放
/// 价值，模型命令可视化二期走事件链不依赖它）。
pub struct TerminalSession {
    pub id: String,
    pub cwd: String,
    pub created_at: i64,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    /// 独占写端（take_writer 只能取一次；输入/二期模型命令注入共用）
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    output: broadcast::Sender<TerminalEvent>,
    killed: AtomicBool,
}

/// 终端会话注册表（AppState 组件；一期内存态，重启即清）。
/// std Mutex：所有方法都不跨 await 持锁（insert/get 后立即释放；
/// 读线程自清理同步 lock），无需 async 锁。
pub struct TerminalStore {
    sessions: Arc<Mutex<HashMap<String, Arc<TerminalSession>>>>,
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 默认 shell：Windows → cmd.exe；Unix → $SHELL 回退 /bin/bash。
fn default_shell() -> String {
    if cfg!(windows) {
        "cmd.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
}

impl TerminalStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 创建 pty 会话：cwd 缺省 = 配置工作目录（前端传当前项目根）。
    /// 读线程阻塞读 → 输出事件；EOF（shell 退出/被杀）→ 退出码 → 自清理。
    pub async fn create(&self, cwd: Option<String>, cols: u16, rows: u16) -> Result<String, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("创建 pty 失败: {e}"))?;

        let mut cmd = CommandBuilder::new(default_shell());
        if let Some(dir) = cwd.as_deref() {
            cmd.cwd(dir);
        }
        // 常规终端语义（颜色/光标定位依赖）
        cmd.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("启动 shell 失败: {e}"))?;
        drop(pair.slave); // 从父侧释放 slave，否则部分 shell 不退出

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("终端读通道失败: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("终端写通道失败: {e}"))?;

        let (tx, _rx) = broadcast::channel::<TerminalEvent>(1024);
        let id = uuid::Uuid::new_v4().to_string();
        let session = Arc::new(TerminalSession {
            id: id.clone(),
            cwd: cwd.unwrap_or_default(),
            created_at: now_ts(),
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
            child: Arc::new(Mutex::new(child)),
            output: tx,
            killed: AtomicBool::new(false),
        });

        // 阻塞读线程（portable-pty 是 blocking API，wezterm 同款线程路线）：
        // 读到 EOF（shell 退出/被杀）→ 取退出码 → Exit 事件 → 注册表自清理
        {
            let sessions = self.sessions.clone();
            let sid = id.clone();
            let session_ref = session.clone();
            let writer_ref = session.writer.clone();
            let out_tx = session.output.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            // ConPTY 启动时发 ESC[6n（光标位置查询），宿主必须应答
                            // ESC[row;colR 否则 cmd 输出被阻塞（Windows Terminal/VS Code
                            // 同款处理；Unix shell 也发此查询，应答同样无害）
                            if buf[..n].windows(4).any(|w| w == b"\x1b[6n") {
                                if let Ok(mut w) = writer_ref.lock() {
                                    let _ = w.write_all(b"\x1b[1;1R");
                                    let _ = w.flush();
                                }
                            }
                            let data = base64::Engine::encode(
                                &base64::engine::general_purpose::STANDARD,
                                &buf[..n],
                            );
                            // 无 receiver（SSE 未订阅）时 send 返回 Err——继续读，
                            // 订阅者出现后即恢复（终端输出无重放价值，丢历史可接受）
                            let _ = out_tx.send(TerminalEvent::Output { data });
                        }
                    }
                }
                // EOF：kill 置位时不给退出码（进程已杀，非自然退出）
                let code = if session_ref.killed.load(Ordering::Relaxed) {
                    -1
                } else {
                    session_ref
                        .child
                        .lock()
                        .ok()
                        .and_then(|mut c| c.wait().ok())
                        .map(|s| s.exit_code() as i32)
                        .unwrap_or(-1)
                };
                let _ = out_tx.send(TerminalEvent::Exit { code });
                // 自清理（会话已结束，进程已退出）
                if let Ok(mut map) = sessions.lock() {
                    map.remove(&sid);
                }
            });
        }

        if let Ok(mut map) = self.sessions.lock() {
            map.insert(id.clone(), session);
        }
        Ok(id)
    }

    /// 取会话（None = 不存在/已结束自清理）。
    pub fn get(&self, id: &str) -> Option<Arc<TerminalSession>> {
        self.sessions
            .lock()
            .ok()
            .and_then(|map| map.get(id).cloned())
    }

    /// 订阅输出事件流（SSE 用；broadcast 多订阅，迟订阅丢历史）。
    pub fn subscribe(&self, id: &str) -> Result<broadcast::Receiver<TerminalEvent>, String> {
        let session = self
            .get(id)
            .ok_or_else(|| "终端会话不存在".to_string())?;
        Ok(session.output.subscribe())
    }

    /// 调整终端尺寸（前端 xterm fit/resize 时同步）。
    pub async fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let session = self
            .get(id)
            .ok_or_else(|| "终端会话不存在".to_string())?;
        session
            .master
            .lock()
            .map_err(|e| format!("终端锁失败: {e}"))?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("调整终端尺寸失败: {e}"))?;
        Ok(())
    }

    /// 关闭会话：置 kill 标志 + kill 进程（读线程 EOF → Exit → 自清理）。
    pub async fn kill(&self, id: &str) -> Result<(), String> {
        let session = self
            .get(id)
            .ok_or_else(|| "终端会话不存在".to_string())?;
        session.killed.store(true, Ordering::Relaxed);
        let child = session.child.clone();
        tokio::task::spawn_blocking(move || {
            let _ = child.lock().ok().and_then(|mut c| c.kill().ok());
        })
        .await
        .map_err(|e| format!("kill 任务失败: {e}"))?;
        Ok(())
    }
}

impl TerminalSession {
    /// 写输入（用户键入 / 二期模型命令注入共用此入口）。base64 → 原始字节。
    pub async fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|e| format!("终端锁失败: {e}"))?;
        writer
            .write_all(data)
            .map_err(|e| format!("终端写入失败: {e}"))?;
        writer
            .flush()
            .map_err(|e| format!("终端写入失败: {e}"))
    }
}
