//! W10(ADR-0025):后台作业台账。
//!
//! 长命令脱离回合生命周期:ExecExecutor 转轨时在此登记,进程独立 tokio
//! 任务执行,输出追加写 `<数据目录>/jobs/<op_id>.log`;模型经
//! `system.job_output` 轮询收取(对标 DSH job_output),回合 system prompt
//! 注入在跑摘要(JobBoard 端口)。台账进程内 + 日志落盘;重启即丢(如实
//! 报告),保留上限走 limits(job_retention_max / job_retention_max_bytes)。

use bm_core::limits::LimitsCell;
use bm_core::ports::JobBoard;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;

/// P0-3():剥离 BOEN_* 内部变量再继承——
/// 主密钥/模型令牌等内部命名空间不外泄给子进程;其余用户环境(PATH/HOME/
/// venv 等)原样保留,因为 exec 是审批闸后的任意命令执行,11 项白名单会
/// 破坏常规用法且挡不住有完整文件系统访问权的命令,真正的边界=内部密钥面。
pub(crate) fn strip_internal_env(
    vars: impl Iterator<Item = (String, String)>,
) -> Vec<(String, String)> {
    vars.filter(|(k, _)| !k.starts_with("BOEN_")).collect()
}

/// 平台 shell 命令构造(system.exec 与后台作业同款;Windows=PowerShell,
/// 其余=bash;原生命令失败退出码经 $LASTEXITCODE 透传——ADR-0022 后续批)。
pub(crate) fn platform_shell(command: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        let mut c = tokio::process::Command::new("powershell");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "{command}\nif ($null -ne $LASTEXITCODE -and $LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}"
            ),
        ]);
        c.env_clear();
        c.envs(strip_internal_env(std::env::vars()));
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = tokio::process::Command::new("bash");
        c.arg("-c").arg(command);
        c.env_clear();
        c.envs(strip_internal_env(std::env::vars()));
        c
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
}

impl JobStatus {
    fn as_str(self) -> &'static str {
        match self {
            JobStatus::Running => "running",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
        }
    }
}

pub struct JobEntry {
    pub id: String,
    pub command: String,
    pub log_path: PathBuf,
    pub started_at_ms: u64,
    pub status: Mutex<JobStatus>,
    pub exit_code: Mutex<Option<i32>>,
}

impl JobEntry {
    fn status(&self) -> JobStatus {
        *self
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub struct JobTable {
    dir: PathBuf,
    limits: LimitsCell,
    jobs: Mutex<VecDeque<Arc<JobEntry>>>,
}

impl JobTable {
    pub fn new(data_dir: &std::path::Path, limits: LimitsCell) -> Arc<Self> {
        Arc::new(Self {
            dir: data_dir.join("jobs"),
            limits,
            jobs: Mutex::new(VecDeque::new()),
        })
    }

    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// 转轨登记 + 拉起独立执行任务。id 用调用方 operation_id(收据可对齐)。
    pub fn spawn(
        self: &Arc<Self>,
        id: &str,
        command: &str,
        cwd: Option<&str>,
    ) -> Result<(String, PathBuf), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("建作业目录失败: {e}"))?;
        let log_path = self.dir.join(format!("{id}.log"));
        let mut cmd = platform_shell(command);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = cwd.filter(|s| !s.is_empty()) {
            cmd.current_dir(cwd);
        }
        let mut child = cmd.spawn().map_err(|e| format!("后台进程启动失败: {e}"))?;
        let out = child.stdout.take();
        let err = child.stderr.take();
        let log = std::fs::File::create(&log_path).map_err(|e| format!("建日志失败: {e}"))?;
        let log_err = log
            .try_clone()
            .map_err(|e| format!("克隆日志句柄失败: {e}"))?;
        let entry = Arc::new(JobEntry {
            id: id.to_string(),
            command: command.to_string(),
            log_path: log_path.clone(),
            started_at_ms: bm_contract::timestamp::unix_now_ms(),
            status: Mutex::new(JobStatus::Running),
            exit_code: Mutex::new(None),
        });
        {
            let mut q = self
                .jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            q.push_back(entry.clone());
        }
        tokio::spawn(pump_to_file(out, log));
        tokio::spawn(pump_to_file(err, log_err));
        // (pump 内部已处理 None 流)
        let table = self.clone();
        let watcher_entry = entry.clone();
        tokio::spawn(async move {
            let code = match child.wait().await {
                Ok(s) => s.code().unwrap_or(-1),
                Err(_) => -1,
            };
            *watcher_entry
                .status
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = if code == 0 {
                JobStatus::Succeeded
            } else {
                JobStatus::Failed
            };
            *watcher_entry
                .exit_code
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(code);
            table.enforce_retention();
        });
        self.enforce_retention();
        Ok((id.to_string(), log_path))
    }

    /// 轮询收取:wait_ms 内每 200ms 查一次终态(钳 ≤60s);输出取尾部。
    /// async:经异步能力管线执行(tokio 睡眠),绝不阻塞单写者核心循环。
    pub async fn output(&self, job_id: &str, wait_ms: u64) -> Value {
        let wait = wait_ms.min(60_000);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(wait);
        let entry = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|e| e.id == job_id)
            .cloned();
        let Some(entry) = entry else {
            return json!({
                "job_id": job_id,
                "status": "unknown",
                "note": "作业不存在或已随重启丢失(台账仅存于本进程);可重跑并传 run_in_background=true",
            });
        };
        loop {
            let status = entry.status();
            if status != JobStatus::Running || wait == 0 || tokio::time::Instant::now() >= deadline
            {
                return json!({
                    "job_id": entry.id,
                    "status": status.as_str(),
                    "exit_code": *entry.exit_code.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
                    "elapsed_ms": bm_contract::timestamp::unix_now_ms().saturating_sub(entry.started_at_ms),
                    "output_tail": self.tail(&entry.log_path),
                    "log_path": entry.log_path.display().to_string(),
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    /// 管理面列表(/admin/jobs;新→旧)。
    pub fn list(&self) -> Vec<Value> {
        self.jobs
            .lock().unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .rev()
            .map(|e| {
                json!({
                    "id": e.id,
                    "command": e.command.chars().take(200).collect::<String>(),
                    "status": e.status().as_str(),
                    "exit_code": *e.exit_code.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
                    "elapsed_ms": bm_contract::timestamp::unix_now_ms().saturating_sub(e.started_at_ms),
                    "log_path": e.log_path.display().to_string(),
                })
            })
            .collect()
    }

    fn tail(&self, path: &std::path::Path) -> String {
        const TAIL_BYTES: u64 = 16 * 1024;
        let Ok(meta) = std::fs::metadata(path) else {
            return String::new();
        };
        let start = meta.len().saturating_sub(TAIL_BYTES);
        let Ok(f) = std::fs::File::open(path) else {
            return String::new();
        };
        use std::io::{Read, Seek, SeekFrom};
        let mut f = f;
        if f.seek(SeekFrom::Start(start)).is_err() {
            return String::new();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return String::new();
        }
        let mut text = String::from_utf8_lossy(&buf).to_string();
        // 截断起点可能落在多字节字符中间:丢首行残段。
        if start > 0
            && let Some(pos) = text.find('\n')
        {
            text = text[pos + 1..].to_string();
        }
        text
    }

    /// LRU 清理:先按个数、再按日志总字节;只驱逐已终态作业(连同其日志)。
    fn enforce_retention(&self) {
        let l = self.limits.get();
        let mut q = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let evict = |q: &mut VecDeque<Arc<JobEntry>>| -> u64 {
            let mut total: u64 = q
                .iter()
                .map(|e| std::fs::metadata(&e.log_path).map(|m| m.len()).unwrap_or(0))
                .sum();
            let mut removed = 0u64;
            while q.len() > l.job_retention_max
                || (total > l.job_retention_max_bytes && q.len() > 1)
            {
                // 从最旧端找第一个已终态的;全在跑则停。
                let Some(pos) = q.iter().position(|e| e.status() != JobStatus::Running) else {
                    break;
                };
                let e = q.remove(pos).expect("pos 有效");
                total -= std::fs::metadata(&e.log_path).map(|m| m.len()).unwrap_or(0);
                let _ = std::fs::remove_file(&e.log_path);
                removed += 1;
            }
            removed
        };
        let _ = evict(&mut q);
    }
}

impl JobBoard for JobTable {
    fn summary(&self) -> String {
        let q = self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let running: Vec<&Arc<JobEntry>> = q
            .iter()
            .filter(|e| e.status() == JobStatus::Running)
            .collect();
        if running.is_empty() {
            return String::new();
        }
        let mut s = String::from("\n[后台作业] 以下命令在后台执行中(不受前台超时限制):\n");
        for e in running.iter().take(5) {
            s.push_str(&format!(
                "- {}(已运行 {} 秒):{}\n",
                e.id,
                bm_contract::timestamp::unix_now_ms().saturating_sub(e.started_at_ms) / 1000,
                e.command.chars().take(120).collect::<String>()
            ));
        }
        if running.len() > 5 {
            s.push_str(&format!("- …另有 {} 个在跑作业\n", running.len() - 5));
        }
        s
    }

    /// 管理面台账(ADR-0046):复用后端具名 list(新→旧)。
    fn list(&self) -> Vec<Value> {
        JobTable::list(self)
    }
}

async fn pump_to_file(src: Option<impl tokio::io::AsyncRead + Unpin>, mut dst: std::fs::File) {
    use std::io::Write;
    let mut src = match src {
        Some(s) => s,
        None => return,
    };
    let mut buf = [0u8; 8192];
    loop {
        match src.read(&mut buf).await {
            Ok(0) => break,
            Err(e) => {
                tracing::warn!(error = %e, "后台作业日志泵读端失败,输出落盘中止");
                break;
            }
            Ok(n) => {
                if let Err(e) = dst.write_all(&buf[..n]) {
                    tracing::warn!(error = %e, "后台作业日志泵写端失败,输出落盘中止");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Arc<JobTable> {
        let dir = tempfile::tempdir().expect("临时目录");
        let t = JobTable::new(dir.path(), LimitsCell::with_default());
        // tempdir 在函数尾释放;测试进程内日志随目录消失无碍断言。
        std::mem::forget(dir);
        t
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn spawn_runs_to_completion_and_output_reports_terminal() {
        let t = table();
        let (id, log) = t.spawn("job1", "echo bm-job-ok", None).expect("拉起");
        assert_eq!(id, "job1");
        assert!(log.ends_with("job1.log"));
        let v = t.output("job1", 10_000).await;
        assert_eq!(v["status"], "succeeded", "{v}");
        assert_eq!(v["exit_code"], 0);
        assert!(v["output_tail"].as_str().unwrap().contains("bm-job-ok"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unknown_job_reports_unknown() {
        let t = table();
        let v = t.output("no-such-job", 0).await;
        assert_eq!(v["status"], "unknown");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn summary_empty_when_no_running() {
        let t = table();
        assert_eq!(t.summary(), "");
    }

    // P0-3():BOEN_* 内部命名空间不得随 exec 子进程外泄。
    #[test]
    fn strip_internal_env_drops_boen_namespace_only() {
        let vars = [
            ("BOEN_SECRET_MASTER_KEY".to_string(), "x".to_string()),
            ("BOEN_MODEL_API_KEY".to_string(), "y".to_string()),
            ("PATH".to_string(), "/bin".to_string()),
            ("HOME".to_string(), "/home/u".to_string()),
        ]
        .into_iter();
        let kept = strip_internal_env(vars);
        assert_eq!(
            kept,
            vec![
                ("PATH".to_string(), "/bin".to_string()),
                ("HOME".to_string(), "/home/u".to_string())
            ]
        );
    }
}
