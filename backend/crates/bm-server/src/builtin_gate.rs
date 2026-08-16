//! 内置工具权限门（审查 P0-2）——模型工具面中高权限内置工具
//! （bash/subagent）执行前经决策记忆 + 询问链裁决，修复"沙箱插件严格
//! 设闸、内置 bash 零闸"的威胁方向倒置（架构 §5.4 把关链对主路径生效）。
//! 低权限内置工具（ls/find/grep/read/write/edit…，工作区 safe_join 圈禁
//! 内）不打扰；permissive/yolo 档位直放（与插件引擎同一档位来源，
//! 见 compat_engine::extension_policy_from_config）。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use bm_core::agent::AgentStreamEvent;
use bm_core::AppConfig;
use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};

use crate::compat_engine::ask_capability;
use crate::permission_store::PermissionStore;
use crate::PermissionDecision;

/// 高权限内置工具（需过门）：bash = 任意命令执行；subagent = 派生子
/// 进程。其余内置工具在工作区 safe_join 圈禁内，不打扰用户。
const HIGH_RISK_TOOLS: &[&str] = &["bash", "subagent"];

/// 内置工具权限门。全服务共享一个实例（决策记忆/询问通道全局）。
pub struct BuiltinGate {
    store: Arc<std::sync::Mutex<PermissionStore>>,
    streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>>,
    pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
    /// 运行时配置（与 AppState/kernel 同一把锁）。每次 check 读当前档位，
    /// 避免启动快照让 yolo→safe 热切换失效（审查 2026-08-17 P1）。
    config: Arc<RwLock<AppConfig>>,
}

impl BuiltinGate {
    pub fn new(
        store: Arc<std::sync::Mutex<PermissionStore>>,
        streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>>,
        pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
        config: Arc<RwLock<AppConfig>>,
    ) -> Self {
        Self {
            store,
            streams,
            pending,
            config,
        }
    }

    fn ask_high_risk(&self) -> bool {
        let config = self.config.read().expect("config poisoned");
        config.extension_policy.as_deref() != Some("permissive")
    }

    /// 工具执行前裁决。Ok = 放行；Err = 拒绝原因（模型可见文案，
    /// 帮助模型收敛重试策略——审查 BUG-004 的"莫名失败"体验）。
    pub async fn check(&self, session_id: &str, tool: &str) -> Result<(), String> {
        if !self.ask_high_risk() || !HIGH_RISK_TOOLS.contains(&tool) {
            return Ok(());
        }
        let message = match tool {
            "bash" => "内置工具请求：bash（执行任意命令）",
            "subagent" => "内置工具请求：subagent（派生子代理执行任务）",
            _ => unreachable!("check 仅对高权限工具调用"),
        };
        let allow = ask_capability(
            &self.store,
            &self.streams,
            &self.pending,
            session_id,
            "builtin",
            tool,
            message,
        )
        .await;
        if allow {
            Ok(())
        } else {
            Err(format!(
                "工具 {tool} 未获权限（询问被拒或无响应超时）——请用户授权后重试，或改用其他方式"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn test_config(permissive: bool) -> Arc<RwLock<AppConfig>> {
        let mut cfg = AppConfig::default();
        cfg.extension_policy = Some(if permissive { "permissive" } else { "safe" }.into());
        Arc::new(RwLock::new(cfg))
    }

    fn test_gate(ask_high_risk: bool) -> (BuiltinGate, Arc<std::sync::Mutex<PermissionStore>>) {
        let store = Arc::new(std::sync::Mutex::new(PermissionStore::ephemeral()));
        let streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let gate = BuiltinGate::new(store.clone(), streams, pending, test_config(!ask_high_risk));
        (gate, store)
    }

    #[tokio::test]
    async fn low_risk_tools_never_ask() {
        let (gate, _) = test_gate(true);
        assert!(gate.check("s1", "read").await.is_ok());
        assert!(gate.check("s1", "grep").await.is_ok());
        assert!(gate.check("s1", "todo").await.is_ok());
    }

    #[tokio::test]
    async fn permissive_mode_passes_high_risk() {
        let (gate, _) = test_gate(false);
        assert!(gate.check("s1", "bash").await.is_ok());
        assert!(gate.check("s1", "subagent").await.is_ok());
    }

    #[tokio::test]
    async fn memory_hit_bypasses_ask() {
        let (gate, store) = test_gate(true);
        store
            .lock()
            .unwrap()
            .record("builtin", "bash", true)
            .unwrap();
        assert!(gate.check("s1", "bash").await.is_ok());

        store
            .lock()
            .unwrap()
            .record("builtin", "bash", false)
            .unwrap();
        assert!(gate.check("s1", "bash").await.is_err());
    }

    #[tokio::test]
    async fn ask_chain_allow_records_always_decision() {
        let (_, store) = test_gate(true);
        // 响应者：询问注册后回 allow+always（模拟前端 respond_permission）
        let pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        // 手动放一个与 gate 共享的 pending——上面 test_gate 的 pending 不暴露，
        // 这里构造完整实例
        let streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let p = pending.clone();
        let responder = tokio::spawn(async move {
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let mut map = p.lock().await;
                if let Some((_, tx)) = map.drain().next() {
                    let _ = tx.send(PermissionDecision {
                        allow: true,
                        always: true,
                    });
                    return;
                }
            }
            panic!("询问未在 1s 内注册");
        });
        let gate = BuiltinGate::new(store.clone(), streams, pending, test_config(false));
        assert!(gate.check("s1", "bash").await.is_ok());
        responder.await.unwrap();
        // always 决策已回写：二次调用命中记忆，无响应者也不询问
        assert!(gate.check("s1", "bash").await.is_ok());
    }

    #[tokio::test]
    async fn ask_chain_denied_decision_rejects() {
        // 用户点"拒绝"→ 立即拒绝（fail-closed 的拒绝分支；超时分支由
        // PERMISSION_TIMEOUT 语义保证——timeout→None→false，60s 人工验证）
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
        let gate = BuiltinGate::new(store.clone(), streams, pending, test_config(false));
        let r = gate.check("s1", "bash").await;
        responder.await.unwrap();
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("未获权限"));
    }

    #[tokio::test]
    async fn live_policy_switch_tightens_without_rebuild() {
        let store = Arc::new(std::sync::Mutex::new(PermissionStore::ephemeral()));
        let streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>> =
            Arc::new(TokioMutex::new(HashMap::new()));
        let config = test_config(true);
        let gate = BuiltinGate::new(store.clone(), streams, pending, config.clone());
        store
            .lock()
            .unwrap()
            .record("builtin", "bash", false)
            .unwrap();
        // permissive 直放，不读记忆
        assert!(gate.check("s1", "bash").await.is_ok());
        config.write().unwrap().extension_policy = Some("safe".into());
        // 切回 safe 后同一扇门立即读记忆拒绝，无需重建
        assert!(gate.check("s1", "bash").await.is_err());
    }
}
