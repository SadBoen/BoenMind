//! # kernel-supervisor
//!
//! 插件进程宿主雏形（M1）：拉起 / 健康检查 / 崩溃重启的最小实现。
//! M1 不接任何真实插件，只提供能力；M3 完整化。
//!
//! 实现要点：
//! - `spawn` 用 `tokio::process::Command` 拉起进程，`Child` 移入后台 wait 任务；
//!   `ChildHandle` 只存 pid + status，status 由 wait 任务更新。
//! - wait 任务用 `tokio::select!` 监听 `child.wait()` 与 kill 信号：
//!   自然退出 → `Exited(code)`；kill 信号 → `start_kill` + `wait` → `Killed`。
//! - 同 id 重复 `spawn` 报 `DuplicateId`；`restart` 先 kill 旧进程再按新 spec 重建。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use parking_lot::RwLock;
use tokio::sync::{oneshot, Notify};

/// 插件启动规格。
#[derive(Debug, Clone)]
pub struct PluginSpec {
    pub id: String,
    pub bin: PathBuf,
    pub args: Vec<String>,
}

/// 子进程生命周期状态。
#[derive(Debug, Clone, PartialEq)]
pub enum ChildStatus {
    /// 已拉起、wait 任务在跑。
    Running,
    /// 自然退出（含崩溃退出码）。
    Exited(i32),
    /// 被 `kill` 主动终止。
    Killed,
    /// 拉起/等待失败。
    LaunchFailed(String),
}

/// 子进程句柄：pid + 状态 + kill 信号。
/// `status` 由后台 wait 任务更新；`completion` 用于 `kill` 等待终止落定。
pub struct ChildHandle {
    pub id: String,
    pub pid: u32,
    status: Mutex<ChildStatus>,
    kill_signal: Arc<Notify>,
    completion: Mutex<Option<oneshot::Receiver<()>>>,
}

impl std::fmt::Debug for ChildHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildHandle")
            .field("id", &self.id)
            .field("pid", &self.pid)
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

impl ChildHandle {
    fn new(id: String, pid: u32, kill_signal: Arc<Notify>) -> Self {
        Self {
            id,
            pid,
            status: Mutex::new(ChildStatus::Running),
            kill_signal,
            completion: Mutex::new(None),
        }
    }

    fn status(&self) -> ChildStatus {
        self.status.lock().unwrap().clone()
    }
}

/// 进程宿主。
pub struct Supervisor {
    children: RwLock<HashMap<String, Arc<ChildHandle>>>,
    /// M3 插件全局编号预留。
    #[allow(dead_code)]
    next_id: AtomicU64,
}

impl Supervisor {
    pub fn new() -> Self {
        Self {
            children: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(0),
        }
    }

    /// 拉起一个插件进程。返回的句柄先置 `Running`；
    /// 后台 wait 任务负责把状态推进到 `Exited` / `Killed` / `LaunchFailed`。
    pub async fn spawn(&self, spec: PluginSpec) -> Result<Arc<ChildHandle>, SupervisorError> {
        let mut child = tokio::process::Command::new(&spec.bin)
            .args(&spec.args)
            .spawn()
            .map_err(|e| SupervisorError::Launch {
                bin: spec.bin.display().to_string(),
                msg: e.to_string(),
            })?;
        let pid = child.id().unwrap_or(0);

        let kill_signal = Arc::new(Notify::new());
        let handle = Arc::new(ChildHandle::new(
            spec.id.clone(),
            pid,
            Arc::clone(&kill_signal),
        ));
        let (tx, rx) = oneshot::channel();
        *handle.completion.lock().unwrap() = Some(rx);

        let mut children = self.children.write();
        if children.contains_key(&spec.id) {
            // 同 id 重复拉起：回收刚拉起的进程，避免泄漏。
            drop(children);
            let _ = child.start_kill();
            return Err(SupervisorError::DuplicateId(spec.id));
        }
        children.insert(spec.id.clone(), Arc::clone(&handle));
        drop(children);

        let h = Arc::clone(&handle);
        tokio::spawn(async move {
            let final_status = tokio::select! {
                wait_result = child.wait() => match wait_result {
                    Ok(code) => ChildStatus::Exited(code.code().unwrap_or(-1)),
                    Err(e) => ChildStatus::LaunchFailed(e.to_string()),
                },
                _ = kill_signal.notified() => {
                    let _ = child.start_kill();
                    match child.wait().await {
                        Ok(_) => ChildStatus::Killed,
                        Err(e) => ChildStatus::LaunchFailed(e.to_string()),
                    }
                }
            };
            *h.status.lock().unwrap() = final_status;
            let _ = tx.send(());
        });

        Ok(handle)
    }

    pub fn status(&self, id: &str) -> Option<ChildStatus> {
        self.children.read().get(id).map(|h| h.status())
    }

    /// Running 即健康。
    pub async fn is_healthy(&self, id: &str) -> bool {
        matches!(self.status(id), Some(ChildStatus::Running))
    }

    /// kill 后标记 `Killed`（等待 wait 任务落定后返回）。
    pub async fn kill(&self, id: &str) -> Result<(), SupervisorError> {
        let handle = self
            .children
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| SupervisorError::NotFound(id.to_string()))?;
        handle.kill_signal.notify_one();
        let rx = handle.completion.lock().unwrap().take();
        if let Some(rx) = rx {
            let _ = rx.await;
        }
        match handle.status() {
            ChildStatus::Killed | ChildStatus::Exited(_) => Ok(()),
            other => Err(SupervisorError::KillFailed {
                id: id.to_string(),
                status: other,
            }),
        }
    }

    /// 先杀旧进程再按新 spec 重建（蓝绿替换的雏形语义，M3 完整化）。
    pub async fn restart(
        &self,
        id: &str,
        spec: PluginSpec,
    ) -> Result<Arc<ChildHandle>, SupervisorError> {
        if self.children.read().contains_key(id) {
            self.kill(id).await?;
            self.children.write().remove(id);
        }
        self.spawn(spec).await
    }

    /// 当前托管的插件 id 列表（有序）。
    pub fn list(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.children.read().keys().cloned().collect();
        ids.sort();
        ids
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("plugin '{0}' is already running")]
    DuplicateId(String),
    #[error("no plugin with id '{0}'")]
    NotFound(String),
    #[error("failed to launch '{bin}': {msg}")]
    Launch { bin: String, msg: String },
    #[error("failed to kill '{id}': {status:?}")]
    KillFailed { id: String, status: ChildStatus },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const CMD: &str = "cmd";

    fn spec(id: &str, args: &[&str]) -> PluginSpec {
        PluginSpec {
            id: id.to_string(),
            bin: PathBuf::from(CMD),
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }

    async fn wait_until<F>(mut cond: F, timeout: Duration) -> bool
    where
        F: FnMut() -> bool,
    {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if cond() {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return cond();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// `cmd /C exit 0` → 短暂 Running → Exited(0)。
    #[tokio::test]
    async fn spawn_exit_zero_tracks_status() {
        let sup = Supervisor::new();
        let handle = sup.spawn(spec("exit0", &["/C", "exit", "0"])).await.unwrap();
        assert!(handle.pid > 0, "expected a real pid");
        assert!(sup.is_healthy("exit0").await, "should be healthy right after spawn");
        assert!(wait_until(
            || sup.status("exit0") == Some(ChildStatus::Exited(0)),
            Duration::from_secs(2)
        )
        .await, "child should exit with code 0 within 2s");
    }

    /// kill 长命令 → status == Killed。
    #[tokio::test]
    async fn kill_marks_killed() {
        let sup = Supervisor::new();
        sup.spawn(spec("k", &["/C", "ping", "127.0.0.1", "-n", "30"]))
            .await
            .unwrap();
        assert!(sup.is_healthy("k").await);
        sup.kill("k").await.unwrap();
        assert_eq!(sup.status("k"), Some(ChildStatus::Killed));
    }

    /// restart：旧进程被杀，新进程按新 spec 拉起。
    #[tokio::test]
    async fn restart_replaces_child() {
        let sup = Supervisor::new();
        let old = sup.spawn(spec("r", &["/C", "ping", "127.0.0.1", "-n", "30"])).await.unwrap();
        assert!(sup.is_healthy("r").await);

        let new = sup
            .restart("r", spec("r", &["/C", "exit", "0"]))
            .await
            .unwrap();
        assert!(new.pid > 0);
        assert_ne!(old.pid, new.pid, "restart should spawn a fresh process");
        assert!(wait_until(
            || sup.status("r") == Some(ChildStatus::Exited(0)),
            Duration::from_secs(2)
        )
        .await, "replacement child should exit with code 0 within 2s");
    }

    /// 同 id 重复 spawn → DuplicateId。
    #[tokio::test]
    async fn duplicate_spawn_rejected() {
        let sup = Supervisor::new();
        sup.spawn(spec("dup", &["/C", "exit", "0"])).await.unwrap();
        let err = sup.spawn(spec("dup", &["/C", "exit", "0"])).await.unwrap_err();
        assert!(matches!(err, SupervisorError::DuplicateId(_)));
    }

    /// 未知 id：status None / is_healthy false / kill NotFound。
    #[tokio::test]
    async fn unknown_id_handling() {
        let sup = Supervisor::new();
        assert_eq!(sup.status("nope"), None);
        assert!(!sup.is_healthy("nope").await);
        assert!(matches!(
            sup.kill("nope").await,
            Err(SupervisorError::NotFound(_))
        ));
        assert!(sup.list().is_empty());
    }

    /// list 包含已托管 id。
    #[tokio::test]
    async fn list_tracks_spawned_ids() {
        let sup = Supervisor::new();
        sup.spawn(spec("a", &["/C", "exit", "0"])).await.unwrap();
        sup.spawn(spec("b", &["/C", "exit", "0"])).await.unwrap();
        let mut ids = sup.list();
        ids.sort();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }
}
