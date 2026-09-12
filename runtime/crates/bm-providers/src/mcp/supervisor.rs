//! MCP 装配 supervisor(F-07 收口):server 的启动装载与 webadmin 热重载
//! 此前各自持有一份「读配置 → 建传输 → hub.connect → 能力注册」逻辑(双写);
//! 本模块将其收口为单一入口,两处调用点同调。registrar 抽象由调用方注入
//! (启动=直接收集,热装载=capabilities_register/unregister + 快照写回)。

use crate::mcp::{HttpMcpTransport, McpHub, McpTransport, StdioMcpTransport, load_mcp_setups};
use bm_core::ports::SecretStore;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

/// 同步结果(与 webadmin /admin/mcp/reload 历史响应字段一致)。
// ADR-0046:同步结果 / 注册回调 / 配置读取上移 core(`ports::mcp_admin`),
// 此处 re-export 保持既有公共路径不变。
pub use bm_core::ports::mcp_admin::{CapabilityRegistrar, SyncOutcome, read_mcp_servers};

/// ADR-0035 §4:把 limits 的 MB/秒/进程数折算为 [`bm_sandbox::SandboxLimits`]。
/// 独立成函数以便单测(避免 spawn 真实子进程)。
pub fn sandbox_from_limits(l: &bm_core::limits::Limits) -> bm_sandbox::SandboxLimits {
    bm_sandbox::SandboxLimits {
        memory_bytes: l.mcp_subprocess_memory_mb.saturating_mul(1024 * 1024),
        cpu_seconds: l.mcp_subprocess_cpu_secs,
        active_processes: l.mcp_subprocess_max_procs,
    }
}

/// 以 mcp.json 为准把 hub 同步到目标态:卸载已移除的、新增/更新/失败分通道。
/// `loaded_names` = 当前已装载名单(快照,由调用方从 hub 外的状态取)。
pub async fn sync_from_config(
    hub: &Arc<McpHub>,
    mcp_config_path: &Path,
    secrets: Arc<dyn SecretStore>,
    loaded_names: Vec<String>,
    registrar: &dyn CapabilityRegistrar,
    limits: &bm_core::limits::LimitsCell,
) -> SyncOutcome {
    let mut outcome = SyncOutcome::default();

    let servers = match read_mcp_servers(mcp_config_path) {
        Ok(servers) => servers,
        Err(e) => {
            outcome
                .failed
                .push(json!({"name": Value::Null, "error": e}));
            return outcome;
        }
    };

    let target_names: Vec<String> = servers
        .iter()
        .filter_map(|s| s["name"].as_str().map(|n| n.to_string()))
        .filter(|n| !n.is_empty())
        .collect();

    // 1. 处理需要移除的 server(在 loaded 中但不在 target 中)
    for name in &loaded_names {
        if !target_names.contains(name) {
            let removed_caps = hub.disconnect_server(name).await;
            if !removed_caps.is_empty()
                && let Err(e) = registrar.unregister(removed_caps).await
            {
                outcome.failed.push(json!({"name": name, "error": e}));
            } else {
                outcome.uninstalled.push(name.clone());
            }
        }
    }

    // 解析一次全量 setup(2026-09-12 N+1 修复:此前在下方 per-server 循环体内
    // 调 load_mcp_setups,每个 server 都把整个 mcp.json 重读重析+密钥解析
    // 一遍,启动与热重载各放大 O(n) 倍)。解析失败与逐条失败同形:每个目标
    // server 一条失败记录;摘除步已先行完成,与既有语义一致。
    let setups = match load_mcp_setups(
        mcp_config_path,
        secrets.as_ref(),
        limits.get().mcp_restart_limit,
    ) {
        Ok(setups) => setups,
        Err(e) => {
            for name in &target_names {
                outcome
                    .failed
                    .push(json!({"name": name, "error": format!("配置解析失败: {e}")}));
            }
            return outcome;
        }
    };

    // 2. 重连 target 中的每一个 server(支持新增与修改更新)
    for item in &servers {
        let name = item["name"].as_str().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let command = item["command"].as_str().unwrap_or("").to_string();
        let args: Vec<String> = item["args"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        // W10:缺省工具超时走 limits(mcp.json 条目级 tool_timeout_ms 仍优先)。
        let timeout = item["tool_timeout_ms"]
            .as_u64()
            .unwrap_or(limits.get().mcp_default_tool_timeout_ms);

        // 已存在的 server:先摘除旧路由与注销旧能力(修改更新语义)
        if loaded_names.contains(&name) {
            let removed_caps = hub.disconnect_server(&name).await;
            if !removed_caps.is_empty()
                && let Err(e) = registrar.unregister(removed_caps).await
            {
                outcome.failed.push(json!({"name": name, "error": e}));
                continue;
            }
        }

        let Some(setup) = setups.iter().find(|s| s.name == name) else {
            continue;
        };

        let loaded = async {
            let transport: Arc<dyn McpTransport> = match setup.transport.as_str() {
                "http" | "sse" | "streamable-http" => {
                    let url = setup
                        .url
                        .as_deref()
                        .ok_or_else(|| "远程 MCP 缺少 url 字段".to_string())?;
                    HttpMcpTransport::new(url, setup.bearer_token.clone())
                        .with_limits(limits.clone())
                }
                _ => {
                    // ADR-0035 §4:从 limits 折算 OS 级资源上限,随 spawn 施加。
                    let sandbox = sandbox_from_limits(&limits.get());
                    StdioMcpTransport::spawn_with_sandbox(
                        &command,
                        &args,
                        &setup.env_resolved,
                        setup.restart_limit,
                        sandbox,
                    )?
                    .with_limits(limits.clone())
                }
            };
            hub.connect(&name, transport, timeout)
                .await
                .map_err(|e| e.to_string())
        }
        .await;

        match loaded {
            Ok(manifests) => {
                let count = manifests.len();
                let entries = McpHub::capability_entries(manifests);
                match registrar.register(entries).await {
                    Ok(()) => {
                        if loaded_names.contains(&name) {
                            outcome.updated.push(name.clone());
                        } else {
                            outcome.registered.push(name.clone());
                        }
                        outcome
                            .note_loaded
                            .push(json!({"name": name, "tools": count}));
                    }
                    Err(e) => outcome.failed.push(json!({"name": name, "error": e})),
                }
            }
            Err(e) => outcome.failed.push(json!({"name": name, "error": e})),
        }
    }
    outcome
}

#[cfg(test)]
mod sandbox_tests {
    use super::sandbox_from_limits;

    /// ADR-0035 §4:limits(MB/秒/进程数)折算为 OS 上限(字节)。
    #[test]
    fn limits_map_to_sandbox_bytes() {
        let l = bm_core::limits::Limits {
            mcp_subprocess_memory_mb: 512,
            mcp_subprocess_cpu_secs: 30,
            mcp_subprocess_max_procs: 8,
            ..Default::default()
        };
        let s = sandbox_from_limits(&l);
        assert_eq!(s.memory_bytes, 512 * 1024 * 1024);
        assert_eq!(s.cpu_seconds, 30);
        assert_eq!(s.active_processes, 8);
        assert!(!s.is_noop());

        // 全零 = 空操作(不施加)
        let z = bm_core::limits::Limits {
            mcp_subprocess_memory_mb: 0,
            mcp_subprocess_cpu_secs: 0,
            mcp_subprocess_max_procs: 0,
            ..Default::default()
        };
        assert!(sandbox_from_limits(&z).is_noop());
    }
}
