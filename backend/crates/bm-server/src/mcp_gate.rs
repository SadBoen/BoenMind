//! MCP 工具权限门（bm-mcp 官方插件配套）：外部 MCP server 提供的工具
//! 一律先裁决——所有 `mcp__` 工具走决策记忆 + 询问链（外部代码不可信，
//! 与内置高权限工具同闸）；permissive/yolo 档位直放（全自动档位语义
//! 与插件引擎/BuiltinGate 一致）。

use std::collections::HashMap;
use std::sync::Arc;

use bm_core::agent::AgentStreamEvent;
use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};

use crate::compat_engine::ask_capability;
use crate::permission_store::PermissionStore;
use crate::PermissionDecision;

/// MCP 工具权限门。全服务共享一个实例（决策记忆/询问通道全局）。
pub struct McpGate {
    store: Arc<std::sync::Mutex<PermissionStore>>,
    streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>>,
    pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
    /// true = MCP 工具走询问链（safe/balanced/default）；
    /// false = 直放（permissive/yolo——用户已选全自动档位）。
    ask: bool,
}

impl McpGate {
    pub fn new(
        store: Arc<std::sync::Mutex<PermissionStore>>,
        streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>>,
        pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
        ask: bool,
    ) -> Self {
        Self {
            store,
            streams,
            pending,
            ask,
        }
    }

    /// 工具执行前裁决。Ok = 放行；Err = 拒绝原因（模型可见文案）。
    pub async fn check(&self, session_id: &str, tool: &str) -> Result<(), String> {
        if !self.ask {
            return Ok(());
        }
        // MCP 工具按工具名粒度记忆（qualified_name = mcp__server__tool），
        // 首次调用询问，用户选 always 后同工具免问。
        let message = format!("MCP 工具请求：{tool}（来自外部 MCP server）");
        let allow = ask_capability(
            &self.store,
            &self.streams,
            &self.pending,
            session_id,
            "mcp",
            tool,
            &message,
        )
        .await;
        if allow {
            Ok(())
        } else {
            Err(format!(
                "工具 {tool} 未获权限（询问被拒或无响应超时）——请用户授权后重试，或禁用该 MCP server"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn test_gate(ask: bool) -> (McpGate, Arc<std::sync::Mutex<PermissionStore>>) {
        let store = Arc::new(std::sync::Mutex::new(PermissionStore::ephemeral()));
        let streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let gate = McpGate::new(store.clone(), streams, pending, ask);
        (gate, store)
    }

    #[tokio::test]
    async fn permissive_mode_passes_all() {
        let (gate, _) = test_gate(false);
        assert!(gate.check("s1", "mcp__fs__read_file").await.is_ok());
    }

    #[tokio::test]
    async fn memory_hit_bypasses_ask() {
        let (gate, store) = test_gate(true);
        store
            .lock()
            .unwrap()
            .record("mcp", "mcp__fs__read_file", true)
            .unwrap();
        assert!(gate.check("s1", "mcp__fs__read_file").await.is_ok());

        store
            .lock()
            .unwrap()
            .record("mcp", "mcp__fs__read_file", false)
            .unwrap();
        assert!(gate.check("s1", "mcp__fs__read_file").await.is_err());
    }

    #[tokio::test]
    async fn ask_chain_denied_rejects() {
        let store = Arc::new(std::sync::Mutex::new(PermissionStore::ephemeral()));
        let streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let p = pending.clone();
        let responder = tokio::spawn(async move {
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let mut map = p.lock().await;
                if let Some((_, tx)) = map.drain().next() {
                    let _ = tx.send(PermissionDecision {
                        allow: false,
                        always: false,
                    });
                    return;
                }
            }
            panic!("询问未在 1s 内注册");
        });
        let gate = McpGate::new(store.clone(), streams, pending, true);
        let r = gate.check("s1", "mcp__fs__read_file").await;
        responder.await.unwrap();
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("未获权限"));
    }
}
