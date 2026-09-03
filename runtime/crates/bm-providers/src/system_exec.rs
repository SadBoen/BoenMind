//! system.exec 内置命令执行能力(2026-09-03 用户令「按常规设计」):对标
//! pi/Claude Code 的 shell 工具,但每条命令走 Broker 审批卡(effect=
//! external-side-effect → needs_approval),适配服务器常驻形态与 ADR-0006。
//!
//! 形态 = 内置异步能力(provider id 以 `.async` 结尾 → registry 标异步),
//! 与 MCP 同管线(超时钳制/取消/单写者零阻塞/收据轮询+op_results 入表)。
//! 执行体 spawn 宿主 shell:Windows=cmd /C,其余=sh -c;输出合并截断 16K;
//! 超时杀进程(kill_on_drop)。

use bm_contract::capability::CapabilityManifest;
use bm_core::ports::{AsyncCallError, AsyncCapabilityExecutor};
use bm_core::registry::CapabilityProvider;
use serde_json::{Value, json};
use std::sync::Arc;

pub const EXEC_CAPABILITY: &str = "system.exec";

const OUTPUT_CAP_CHARS: usize = 16_000;

/// system.exec 的 manifest + 注册占位 provider(执行体在 ExecExecutor;
/// 同步面直调一律拒绝,防绕过 turn 语义——model.invoke 同款口径)。
pub fn exec_capability_entry() -> (CapabilityManifest, Arc<dyn CapabilityProvider>) {
    let manifest: CapabilityManifest = serde_json::from_value(json!({
        "capability": EXEC_CAPABILITY,
        "provider": "builtin.async",
        "version": "0.1.0",
        "input_schema": {
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "要执行的命令行(交由宿主 shell 解释)"},
                "timeout_ms": {"type": "integer", "description": "超时毫秒(可选,默认 60000,上限 300000)"}
            },
            "required": ["command"]
        },
        "output_schema": {"type": "object"},
        "effect": "external-side-effect",
        "idempotent": false,
        "cancellable": true,
        "timeout_ms": 60000,
        "approval": "required",
        "scopes": ["system.exec"]
    }))
    .expect("exec manifest 合法");
    (manifest, Arc::new(ExecPlaceholder))
}

struct ExecPlaceholder;
impl CapabilityProvider for ExecPlaceholder {
    fn invoke(&self, _args: Value) -> Result<Value, String> {
        Err("system.exec 仅限运行时 turn 循环经审批后调用".into())
    }
}

/// 异步执行体:spawn 宿主 shell 跑命令,超时/取消/输出上限内建。
pub struct ExecExecutor;

#[async_trait::async_trait]
impl AsyncCapabilityExecutor for ExecExecutor {
    async fn call(
        &self,
        _operation_id: &str,
        capability: &str,
        args: Value,
        deadline: std::time::Duration,
    ) -> Result<Value, AsyncCallError> {
        if capability != EXEC_CAPABILITY {
            return Err(AsyncCallError::Transport(format!(
                "exec 执行器不认识能力 {capability}"
            )));
        }
        let command = args["command"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or(AsyncCallError::Transport(
                "缺必填参数 command(字符串)".into(),
            ))?;
        let millis = args["timeout_ms"]
            .as_u64()
            .map(|m| m.clamp(1_000, 300_000))
            .unwrap_or(60_000)
            .min(deadline.as_millis() as u64);
        let dur = std::time::Duration::from_millis(millis);

        #[cfg(windows)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("cmd");
            c.arg("/C").arg(command);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = tokio::process::Command::new("sh");
            c.arg("-c").arg(command);
            c
        };
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = args["cwd"].as_str().filter(|s| !s.is_empty()) {
            cmd.current_dir(cwd);
        }
        let child = cmd
            .spawn()
            .map_err(|e| AsyncCallError::Transport(format!("进程启动失败: {e}")))?;
        let out = tokio::time::timeout(dur, child.wait_with_output())
            .await
            .map_err(|_| AsyncCallError::Timeout)?
            .map_err(|e| AsyncCallError::Transport(format!("等待退出失败: {e}")))?;
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        let err_text = String::from_utf8_lossy(&out.stderr);
        if !err_text.trim().is_empty() {
            text.push_str("\n[stderr]\n");
            text.push_str(&err_text);
        }
        let truncated = text.chars().count() > OUTPUT_CAP_CHARS;
        if truncated {
            text = text.chars().take(OUTPUT_CAP_CHARS).collect();
        }
        Ok(json!({
            "exit_code": out.status.code(),
            "output": text,
            "truncated": truncated,
        }))
    }
}

/// 组合执行器:system.exec 走内置执行体,其余回落(如 MCP hub)。
pub struct SplitExecutor {
    pub fallback: Arc<dyn AsyncCapabilityExecutor>,
}

#[async_trait::async_trait]
impl AsyncCapabilityExecutor for SplitExecutor {
    async fn call(
        &self,
        operation_id: &str,
        capability: &str,
        args: Value,
        deadline: std::time::Duration,
    ) -> Result<Value, AsyncCallError> {
        if capability == EXEC_CAPABILITY {
            ExecExecutor
                .call(operation_id, capability, args, deadline)
                .await
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

    #[tokio::test]
    async fn exec_runs_host_shell_and_captures_output() {
        let out = ExecExecutor
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

    #[tokio::test]
    async fn exec_rejects_missing_command_and_unknown_capability() {
        let err = ExecExecutor
            .call(
                "op",
                EXEC_CAPABILITY,
                json!({}),
                std::time::Duration::from_secs(5),
            )
            .await;
        assert!(matches!(err, Err(AsyncCallError::Transport(m)) if m.contains("command")));
        let err = ExecExecutor
            .call(
                "op",
                "system.echo",
                json!({"command": "x"}),
                std::time::Duration::from_secs(5),
            )
            .await;
        assert!(err.is_err());
    }

    #[test]
    fn manifest_is_approval_bearing_and_async_marked() {
        let (m, _) = exec_capability_entry();
        assert_eq!(m.effect.as_str(), "external-side-effect");
        assert!(m.provider.ends_with(".async"));
        assert_eq!(m.capability, EXEC_CAPABILITY);
    }
}
