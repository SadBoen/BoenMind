//! system.exec 内置命令执行能力(2026-09-03 用户令「按常规设计」):对标
//! pi/Claude Code 的 shell 工具,但每条命令走 Broker 审批卡(effect=
//! external-side-effect → needs_approval),适配服务器常驻形态与 ADR-0006。
//!
//! 形态 = 内置异步能力(provider id 以 `.async` 结尾 → registry 标异步),
//! 与 MCP 同管线(超时钳制/取消/单写者零阻塞/收据轮询+op_results 入表)。
//! 执行体 spawn 宿主 shell(2026-09-06 对齐 DSH/Pi):Windows=PowerShell
//! (-NoProfile -NonInteractive,原生命令失败退出码经 $LASTEXITCODE 透传),
//! 其余=bash -c;输出合并截断(limits 可调);超时杀进程(kill_on_drop)。
//!
//! W10(ADR-0024/0025):默认/上限/截断走 limits 热生效;timeout_ms 超前台
//! 上限或显式 run_in_background → 转后台作业(jobs 台账),经
//! system.job_output 轮询收取(对标 Hermes 自动转轨/DSH job_output)。

use crate::fs_tools;
use crate::jobs::{self, JobTable};
use bm_contract::capability::CapabilityManifest;
use bm_core::limits::LimitsCell;
use bm_core::ports::{AsyncCallError, AsyncCapabilityExecutor};
use bm_core::registry::CapabilityProvider;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

pub const EXEC_CAPABILITY: &str = "system.exec";
pub const JOB_OUTPUT_CAPABILITY: &str = "system.job_output";

/// system.exec 的 manifest + 注册占位 provider(执行体在 ExecExecutor;
/// 同步面直调一律拒绝,防绕过 turn 语义——model.invoke 同款口径)。
///
/// manifest timeout_ms = 前台硬顶(600s,与核心钳制天花板一致):决定
/// capability 层 deadline;逐次生效的默认/上限在执行体内读 limits——
/// 管理面改值下一条命令即生效,无需重建 registry(ADR-0024 §2)。
pub fn exec_capability_entry() -> (CapabilityManifest, Arc<dyn CapabilityProvider>) {
    let manifest: CapabilityManifest = serde_json::from_value(json!({
        "capability": EXEC_CAPABILITY,
        "provider": "builtin.async",
        "version": "0.2.0",
        "description": "在宿主 shell 执行命令:Windows 以 PowerShell(-NoProfile -NonInteractive)执行,其余以 bash -c 执行;返回 exit_code 与合并后的 stdout/stderr(超限截断)。适合跑构建/测试/进程管理等动态操作。可传 timeout_ms 毫秒(默认约 120 秒,前台最长 10 分钟,管理端「限制与超时」可调)。长任务(下载/clone/冷编译)传 run_in_background=true 立即返回作业号,或 timeout_ms 超前台上限时自动转后台执行;之后用 system.job_output 按作业号收取结果。可传 cwd 指定工作目录(限已登记工作区内,越界拒绝)。调用需用户批准后执行。",
        "input_schema": {
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "要执行的命令行(交由宿主 shell 解释)"},
                "cwd": {"type": "string", "description": "工作目录(可选;须在已登记工作区白名单内,越界拒绝;缺省=服务进程工作目录)"},
                "timeout_ms": {"type": "integer", "description": "前台超时毫秒(可选,默认约 120000,上限 600000;超上限自动转后台)"},
                "run_in_background": {"type": "boolean", "description": "true=转后台执行:立即返回 job_id 不等完成,稍后用 system.job_output 收取(适合下载、clone、长构建)"}
            },
            "required": ["command"]
        },
        "output_schema": {"type": "object"},
        "effect": "external-side-effect",
        "idempotent": false,
        "cancellable": true,
        "timeout_ms": 600_000,
        "approval": "required",
        "scopes": ["system.exec"],
        "execution_mode": "async"
    }))
    .expect("exec manifest 合法");
    (manifest, Arc::new(ExecPlaceholder))
}

/// system.job_output 的 manifest + 占位 provider:后台作业收取面(读语义,
/// 免审批;同样走异步管线防阻塞单写者循环)。
pub fn job_output_capability_entry() -> (CapabilityManifest, Arc<dyn CapabilityProvider>) {
    let manifest: CapabilityManifest = serde_json::from_value(json!({
        "capability": JOB_OUTPUT_CAPABILITY,
        "provider": "builtin.async",
        "version": "0.1.0",
        "description": "查询后台作业(system.exec 转后台返回的 job_id)的状态与输出尾部。可传 wait_ms 等待其终态(默认 10000,上限 60000);status=running 时可再次调用继续等。全部历史输出见返回的 log_path。",
        "input_schema": {
            "type": "object",
            "properties": {
                "job_id": {"type": "string", "description": "后台作业号(system.exec 转后台回执中的 job_id)"},
                "wait_ms": {"type": "integer", "description": "最多等待其终态的毫秒数(可选,默认 10000,上限 60000;0=立即返回当前状态)"}
            },
            "required": ["job_id"]
        },
        "output_schema": {"type": "object"},
        "effect": "read-only",
        "idempotent": true,
        "cancellable": true,
        "timeout_ms": 70_000,
        "approval": "not-required",
        "scopes": ["system.exec"],
        "execution_mode": "async"
    }))
    .expect("job_output manifest 合法");
    (manifest, Arc::new(ExecPlaceholder))
}

struct ExecPlaceholder;
impl CapabilityProvider for ExecPlaceholder {
    fn invoke(&self, _args: Value) -> Result<Value, String> {
        Err("system.exec/job_output 仅限运行时 turn 循环经审批后调用".into())
    }
    /// ADR-0041:声明插件身份(执行体异步在 ExecExecutor,此处为同步占位)。
    fn plugin_meta(&self) -> Option<bm_contract::plugin::PluginMeta> {
        Some(bm_contract::plugin::PluginMeta::new(
            "kernel.exec",
            env!("CARGO_PKG_VERSION"),
            bm_contract::plugin::PluginKind::Tool,
        ))
    }
}

/// 异步执行体:spawn 宿主 shell 跑命令(前台)+ 后台作业收取(system.job_output)。
pub struct ExecExecutor {
    pub limits: LimitsCell,
    pub jobs: Arc<JobTable>,
    /// cwd 白名单数据源(2026-09-08 审计修复,与 fs.* 同源沙箱)。
    pub data_dir: std::path::PathBuf,
    pub fallback_root: std::path::PathBuf,
}

impl ExecExecutor {
    pub fn new(
        limits: LimitsCell,
        jobs: Arc<JobTable>,
        data_dir: impl Into<std::path::PathBuf>,
        fallback_root: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            limits,
            jobs,
            data_dir: data_dir.into(),
            fallback_root: fallback_root.into(),
        }
    }

    fn roots(&self) -> fs_tools::Roots {
        fs_tools::workspace_roots(&self.data_dir, &self.fallback_root)
    }

    async fn exec_command(
        &self,
        operation_id: &str,
        args: Value,
        deadline: Duration,
    ) -> Result<Value, AsyncCallError> {
        let command = args["command"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or(AsyncCallError::Transport(
                "缺必填参数 command(字符串)".into(),
            ))?
            .to_string();
        // cwd 沙箱(2026-09-08 审计修复):显式 cwd 经 fs.* 同款工作区白名单
        // 解析(组件级前缀比对,防 .. 逃逸/同名前缀混淆),越界拒绝;未传保持
        // 既有语义(继承宿主进程工作目录,由装配方决定)。校验在转轨分支之前,
        // 前台与后台作业一次覆盖。
        let cwd_raw = args["cwd"].as_str().unwrap_or_default().trim().to_string();
        let cwd = if cwd_raw.is_empty() {
            String::new()
        } else {
            self.roots()
                .resolve(&cwd_raw)
                .map_err(AsyncCallError::Transport)?
                .display()
                .to_string()
        };
        let limits = self.limits.get();
        let requested = args["timeout_ms"].as_u64();
        let wants_background = args["run_in_background"].as_bool().unwrap_or(false);

        // ADR-0025:显式后台或 timeout_ms 超前台上限 → 自动转轨(拒绝避免,
        // Hermes 式);立即回执,进程入 jobs 台账独立执行。
        if wants_background || requested.is_some_and(|m| m > limits.exec_max_ms) {
            let (job_id, log_path) = self
                .jobs
                .spawn(operation_id, &command, Some(&cwd))
                .map_err(AsyncCallError::Transport)?;
            return Ok(json!({
                "backgrounded": true,
                "job_id": job_id,
                "log_path": log_path.display().to_string(),
                "note": "已转后台执行(不受前台超时限制)。用 system.job_output(job_id=…) 轮询收取;完成前勿盲目重跑同一命令。",
            }));
        }

        // 前台:默认/上限走 limits(热生效);deadline=manifest 硬顶。
        let millis = requested
            .unwrap_or(limits.exec_default_ms)
            .clamp(1_000, limits.exec_max_ms)
            .min(deadline.as_millis() as u64);
        let dur = Duration::from_millis(millis);

        let mut cmd = jobs::platform_shell(&command);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if !cwd.is_empty() {
            cmd.current_dir(&cwd);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| AsyncCallError::Transport(format!("进程启动失败: {e}")))?;

        // P1-14: 流式截断读取,避免巨量输出(GB级)在 wait_with_output 中先打爆内存
        let max_bytes = (limits.exec_output_max_chars.saturating_mul(4)).max(64 * 1024);
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();

        async fn read_pipe_capped<R: tokio::io::AsyncRead + Unpin>(
            pipe: Option<R>,
            cap: usize,
        ) -> Vec<u8> {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            if let Some(p) = pipe {
                // 至多 cap 字节(超出不读);读错误按 EOF 处理(保留部分结果)
                let _ = p.take(cap as u64).read_to_end(&mut buf).await;
            }
            buf
        }

        let read_task = async {
            let out_bytes = read_pipe_capped(stdout.as_mut(), max_bytes).await;
            let err_bytes = read_pipe_capped(stderr.as_mut(), max_bytes).await;
            let status = child.wait().await;
            (status, out_bytes, err_bytes)
        };

        let (status, stdout_bytes, stderr_bytes) = tokio::time::timeout(dur, read_task)
            .await
            .map_err(|_| AsyncCallError::Timeout)?;
        let status = status.map_err(|e| AsyncCallError::Transport(format!("等待退出失败: {e}")))?;

        let mut text = String::from_utf8_lossy(&stdout_bytes).to_string();
        let err_text = String::from_utf8_lossy(&stderr_bytes);
        if !err_text.trim().is_empty() {
            text.push_str("\n[stderr]\n");
            text.push_str(&err_text);
        }
        let truncated = text.chars().count() > limits.exec_output_max_chars;
        if truncated {
            text = text.chars().take(limits.exec_output_max_chars).collect();
        }
        Ok(json!({
            "exit_code": status.code(),
            "output": text,
            "truncated": truncated,
        }))
    }
}

#[async_trait::async_trait]
impl AsyncCapabilityExecutor for ExecExecutor {
    async fn call(
        &self,
        operation_id: &str,
        capability: &str,
        args: Value,
        deadline: Duration,
    ) -> Result<Value, AsyncCallError> {
        match capability {
            EXEC_CAPABILITY => self.exec_command(operation_id, args, deadline).await,
            JOB_OUTPUT_CAPABILITY => {
                let job_id = args["job_id"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or(AsyncCallError::Transport(
                        "缺必填参数 job_id(字符串;来自转后台回执)".into(),
                    ))?
                    .to_string();
                let wait_ms = args["wait_ms"].as_u64().unwrap_or(10_000);
                Ok(self.jobs.output(&job_id, wait_ms).await)
            }
            other => Err(AsyncCallError::Transport(format!(
                "exec 执行器不认识能力 {other}"
            ))),
        }
    }
}

/// 组合执行器:system.exec / system.job_output / fs.* / skill.* 走内置执行体,
/// 其余回落(如 MCP hub)。
pub struct SplitExecutor {
    /// system.exec + system.job_output(持 limits 与后台作业台账)。
    pub exec: Arc<ExecExecutor>,
    pub fs: fs_tools::FsExecutor,
    /// Skill v0.2(ADR-0016 第二步):wasmtime 技能脚本执行面。
    pub skills: Option<Arc<crate::skill_wasm::SkillScriptManager>>,
    pub fallback: Arc<dyn AsyncCapabilityExecutor>,
}

#[async_trait::async_trait]
impl AsyncCapabilityExecutor for SplitExecutor {
    async fn call(
        &self,
        operation_id: &str,
        capability: &str,
        args: Value,
        deadline: Duration,
    ) -> Result<Value, AsyncCallError> {
        if capability == EXEC_CAPABILITY || capability == JOB_OUTPUT_CAPABILITY {
            self.exec
                .call(operation_id, capability, args, deadline)
                .await
        } else if fs_tools::FsExecutor::handles(capability) {
            // ADR-0042:按 fs 执行器**声明的能力集**分道,不再 `starts_with("fs.")`。
            self.fs.call(operation_id, capability, args, deadline).await
        } else if self
            .skills
            .as_ref()
            .is_some_and(|m| m.has_capability(capability))
        {
            // ADR-0041:按**归属**分道而非名字前缀——wasm 宿主编译表里有的
            // capability(技能脚本或通用 wasm 插件)都归 wasm 执行面。
            match &self.skills {
                Some(m) => m.call(operation_id, capability, args, deadline).await,
                None => Err(AsyncCallError::Transport("wasm 执行面未启用".to_string())),
            }
        } else {
            self.fallback
                .call(operation_id, capability, args, deadline)
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_core::limits::{Limits, LimitsCell};

    fn executor() -> (ExecExecutor, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("临时目录");
        let cell = LimitsCell::with_default();
        let jobs = JobTable::new(dir.path(), cell.clone());
        // 注册表空 → 回落根 = 同一临时目录:界内 cwd 须放行,界外须拒绝。
        (ExecExecutor::new(cell, jobs, dir.path(), dir.path()), dir)
    }

    #[tokio::test]
    async fn exec_runs_host_shell_and_captures_output() {
        let (exec, _dir) = executor();
        let out = exec
            .call(
                "op",
                EXEC_CAPABILITY,
                json!({"command": "echo bm-exec-ok"}),
                std::time::Duration::from_secs(30),
            )
            .await
            .expect("执行成功");
        assert!(out["output"].as_str().unwrap().contains("bm-exec-ok"));
        assert_eq!(out["truncated"], json!(false));
    }

    // ADR-0022 后续批:平台 shell 换装(Windows=PowerShell/其余=bash)后,
    // 原生命令失败退出码必须穿透 shell 到收据,不被吞成 0。
    #[tokio::test]
    async fn exec_propagates_native_exit_code() {
        let (exec, _dir) = executor();
        #[cfg(windows)]
        let command = json!({"command": "cmd /c exit 3"});
        #[cfg(not(windows))]
        let command = json!({"command": "exit 3"});
        let out = exec
            .call(
                "op",
                EXEC_CAPABILITY,
                command,
                std::time::Duration::from_secs(30),
            )
            .await
            .expect("执行成功");
        assert_eq!(out["exit_code"], json!(3), "{out}");
    }

    #[tokio::test]
    async fn exec_rejects_missing_command_and_unknown_capability() {
        let (exec, _dir) = executor();
        let err = exec
            .call(
                "op",
                EXEC_CAPABILITY,
                json!({}),
                std::time::Duration::from_secs(5),
            )
            .await;
        assert!(matches!(err, Err(AsyncCallError::Transport(m)) if m.contains("command")));
        let err = exec
            .call(
                "op",
                "system.echo",
                json!({"command": "x"}),
                std::time::Duration::from_secs(5),
            )
            .await;
        assert!(err.is_err());
    }

    // ADR-0024:前台超时按 limits 生效(默认 120s;传小值即刻生效)。
    #[tokio::test]
    async fn exec_times_out_per_requested_timeout() {
        let (exec, _dir) = executor();
        #[cfg(windows)]
        let args = json!({"command": "Start-Sleep -Seconds 30", "timeout_ms": 1500});
        #[cfg(not(windows))]
        let args = json!({"command": "sleep 30", "timeout_ms": 1500});
        let err = exec
            .call(
                "op",
                EXEC_CAPABILITY,
                args,
                std::time::Duration::from_secs(600),
            )
            .await;
        assert!(matches!(err, Err(AsyncCallError::Timeout)), "{err:?}");
    }

    // ADR-0025:显式后台 → 立即回执作业号;job_output 收取到终态。
    #[tokio::test(flavor = "multi_thread")]
    async fn background_promotion_returns_receipt_and_output_collects() {
        let (exec, _dir) = executor();
        #[cfg(windows)]
        let args = json!({"command": "echo bm-bg-done", "run_in_background": true});
        #[cfg(not(windows))]
        let args = json!({"command": "echo bm-bg-done", "run_in_background": true});
        let receipt = exec
            .call(
                "op-bg",
                EXEC_CAPABILITY,
                args,
                std::time::Duration::from_secs(30),
            )
            .await
            .expect("转后台回执");
        assert_eq!(receipt["backgrounded"], json!(true), "{receipt}");
        let job_id = receipt["job_id"].as_str().expect("job_id").to_string();
        let out = exec
            .call(
                "op-collect",
                JOB_OUTPUT_CAPABILITY,
                json!({"job_id": job_id, "wait_ms": 10_000}),
                std::time::Duration::from_secs(30),
            )
            .await
            .expect("收取成功");
        assert_eq!(out["status"], "succeeded", "{out}");
        assert!(out["output_tail"].as_str().unwrap().contains("bm-bg-done"));
    }

    // ADR-0025:timeout_ms 超前台上限 → 自动转轨(Hermes 式拒绝避免)。
    #[tokio::test(flavor = "multi_thread")]
    async fn over_limit_timeout_auto_promotes_to_background() {
        let (exec, _dir) = executor();
        let args = json!({"command": "echo bm-promoted", "timeout_ms": 999_999});
        let receipt = exec
            .call(
                "op-auto",
                EXEC_CAPABILITY,
                args,
                std::time::Duration::from_secs(30),
            )
            .await
            .expect("自动转轨回执");
        assert_eq!(receipt["backgrounded"], json!(true), "{receipt}");
    }

    // 2026-09-08 审计修复:显式 cwd 必须过 fs.* 同源工作区白名单——
    // 越界目录(哪怕真实存在)拒绝,白名单内(含相对路径)放行;转后台同受此闸。
    #[tokio::test]
    async fn exec_cwd_whitelist_rejects_outside_and_allows_inside() {
        let (exec, dir) = executor();
        let outside = tempfile::tempdir().expect("外部目录");
        // 越界绝对路径 → 拒绝
        let err = exec
            .call(
                "op",
                EXEC_CAPABILITY,
                json!({"command": "echo x", "cwd": outside.path().display().to_string()}),
                std::time::Duration::from_secs(30),
            )
            .await;
        assert!(
            matches!(err, Err(AsyncCallError::Transport(ref m)) if m.contains("白名单") || m.contains("注册表")),
            "越界 cwd 必须被拒:{err:?}"
        );
        // 转后台路径同受闸
        let err = exec
            .call(
                "op-bg",
                EXEC_CAPABILITY,
                json!({"command": "echo x", "cwd": outside.path().display().to_string(), "run_in_background": true}),
                std::time::Duration::from_secs(30),
            )
            .await;
        assert!(err.is_err(), "转后台 cwd 越界同样必须被拒:{err:?}");
        // 白名单内相对路径 → 放行,且实际工作目录即回落根(canonical 形)
        // (guard.rs 的 Roots 会剥 `\\?\` verbatim 前缀,比对前同步剥除)
        let root = dir.path().canonicalize().expect("canonical");
        let root_s = root
            .display()
            .to_string()
            .to_lowercase()
            .trim_start_matches(r"\\?\")
            .to_string();
        #[cfg(windows)]
        let cmd_str = "(Get-Location).Path";
        #[cfg(not(windows))]
        let cmd_str = "pwd";
        let out = exec
            .call(
                "op",
                EXEC_CAPABILITY,
                json!({"command": cmd_str, "cwd": "."}),
                std::time::Duration::from_secs(30),
            )
            .await
            .expect("界内 cwd 应放行");
        let out_l = out["output"].as_str().unwrap_or("").to_lowercase();
        assert!(
            out_l.contains(&root_s),
            "实际 cwd 应为工作区根 {root_s}:{out}"
        );
    }

    // ADR-0024:limits 热生效——改 Cell 后下一条命令截断上限随之变化。
    // (钳制下限 1000,故产出 1500 字符再以 1000 截断。)
    #[tokio::test]
    async fn limits_hot_reload_changes_truncation() {
        let (exec, dir) = executor();
        let cell = exec.limits.clone();
        let l = Limits {
            exec_output_max_chars: 1000,
            ..Limits::default()
        };
        cell.set(l);
        #[cfg(windows)]
        let args = json!({"command": "Write-Output ('a' * 1500)"});
        #[cfg(not(windows))]
        let args = json!({"command": "printf 'a%.0s' $(seq 1 1500)"});
        let out = exec
            .call(
                "op",
                EXEC_CAPABILITY,
                args,
                std::time::Duration::from_secs(30),
            )
            .await
            .expect("执行成功");
        assert_eq!(out["truncated"], json!(true), "1500 字符须被截断:{out}");
        assert_eq!(
            out["output"].as_str().unwrap().chars().count(),
            1000,
            "截断后应恰为 limits 上限"
        );
        drop(dir);
    }

    #[tokio::test]
    async fn job_output_requires_job_id() {
        let (exec, _dir) = executor();
        let err = exec
            .call(
                "op",
                JOB_OUTPUT_CAPABILITY,
                json!({}),
                std::time::Duration::from_secs(5),
            )
            .await;
        assert!(matches!(err, Err(AsyncCallError::Transport(m)) if m.contains("job_id")));
    }

    #[test]
    fn manifest_is_approval_bearing_and_async_marked() {
        let (m, _) = exec_capability_entry();
        assert_eq!(m.effect.as_str(), "external-side-effect");
        assert!(m.provider.ends_with(".async"));
        assert_eq!(m.capability, EXEC_CAPABILITY);
        // ADR-0025:manifest 硬顶=前台 600s(与核心钳制天花板一致);
        // 默认值走 limits,不随 manifest 改。
        assert_eq!(m.timeout_ms, 600_000);
        let (jo, _) = job_output_capability_entry();
        assert_eq!(jo.capability, JOB_OUTPUT_CAPABILITY);
        assert_eq!(jo.effect.as_str(), "read-only");
        assert!(jo.provider.ends_with(".async"));
    }
}
