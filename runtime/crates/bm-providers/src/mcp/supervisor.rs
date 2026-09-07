//! MCP 装配 supervisor(F-07 收口):server 的启动装载与 webadmin 热重载
//! 此前各自持有一份「读配置 → 建传输 → hub.connect → 能力注册」逻辑(双写);
//! 本模块将其收口为单一入口,两处调用点同调。registrar 抽象由调用方注入
//! (启动=直接收集,热装载=capabilities_register/unregister + 快照写回)。

use crate::mcp::{HttpMcpTransport, McpHub, McpTransport, StdioMcpTransport, load_mcp_setups};
use bm_contract::capability::CapabilityManifest;
use bm_core::ports::SecretStore;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

/// 同步结果(与 webadmin /admin/mcp/reload 历史响应字段一致)。
#[derive(Debug, Default)]
pub struct SyncOutcome {
    pub registered: Vec<String>,
    pub updated: Vec<String>,
    pub uninstalled: Vec<String>,
    pub failed: Vec<Value>,
    /// 已装载快照(热装载写回 AdminConfig.mcp_servers 用;启动路径忽略)。
    pub note_loaded: Vec<Value>,
}

/// 能力注册/注销回调(启动=收集;热装载=转核心命令)。
#[async_trait::async_trait]
pub trait CapabilityRegistrar: Send + Sync {
    async fn register(
        &self,
        entries: Vec<(
            CapabilityManifest,
            Arc<dyn bm_core::registry::CapabilityProvider>,
        )>,
    ) -> Result<(), String>;
    async fn unregister(&self, names: Vec<String>) -> Result<(), String>;
}

/// 从配置读 server 清单(文件不存在 = 空清单)。
pub fn read_mcp_servers(path: &Path) -> Result<Vec<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let arr: Vec<Value> =
                serde_json::from_str(&text).map_err(|e| format!("MCP 配置不是 JSON 数组: {e}"))?;
            Ok(arr)
        }
        Err(_) => Ok(vec![]),
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

        let setup = match load_mcp_setups(mcp_config_path, secrets.as_ref()) {
            Ok(setups) => setups.into_iter().find(|s| s.name == name),
            Err(e) => {
                outcome
                    .failed
                    .push(json!({"name": name, "error": format!("配置解析失败: {e}")}));
                continue;
            }
        };
        let Some(setup) = setup else {
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
                _ => StdioMcpTransport::spawn(
                    &command,
                    &args,
                    &setup.env_resolved,
                    setup.restart_limit,
                )?
                .with_limits(limits.clone()),
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
