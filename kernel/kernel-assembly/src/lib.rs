//! # kernel-assembly
//!
//! 组合根：装配微内核各端口为运行时，并提供会话创建/恢复的完整闭环
//! （含 interrupted-turn 修复：kill -9 后重载日志，把未完成回合的尾部
//! 未配对事件修剪掉，保证恢复后的日志没有 torn 状态）。

use std::path::PathBuf;
use std::sync::Arc;

use kernel_contracts::bus::EventBus;
use kernel_contracts::llm::LlmPort;
use kernel_contracts::ports::{
    PluginRuntimeAvailability, PluginRuntimePort, SessionPersistPort,
};
use kernel_contracts::session::{SessionEvent, SessionHeader, StepPhase, TurnEvent};
use kernel_contracts::PortResult;
use kernel_loop::{LoopRuntime, ReactLoopAgent};
use kernel_session::SessionStore;
use kernel_storage::SqlitePersist;
use kernel_tools::{ToolGate, ToolRegistry};

/// 装配错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssemblyError {
    #[error("persist error: {0}")]
    Persist(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("invalid session log: {0}")]
    InvalidLog(String),
    #[error("plugin runtime unavailable: {0}")]
    PluginUnavailable(String),
}

/// 微内核组合根：所有端口经此装配。
pub struct Runtime {
    pub llm: Arc<dyn LlmPort>,
    pub store: Arc<SessionStore>,
    pub tools: Arc<ToolRegistry>,
    pub gate: Arc<ToolGate>,
    pub persist: Arc<dyn SessionPersistPort>,
    pub plugin_runtime: Arc<dyn PluginRuntimePort>,
    pub provider: String,
    pub model: String,
    pub bus: EventBus,
}

impl Runtime {
    /// 创建一个新的运行时（headless profile：内存 store + sqlite 持久化 + mock LLM）。
    pub fn headless(sqlite_path: PathBuf) -> Result<Self, AssemblyError> {
        let persist = Arc::new(
            SqlitePersist::open(&sqlite_path).map_err(|e| AssemblyError::Persist(e.to_string()))?,
        );
        let llm = Arc::new(kernel_llm::ScriptLlm::new(
            "mock".to_string(),
            "mock-1".to_string(),
            vec![],
        ));
        let bus = EventBus::new();
        let store = Arc::new(SessionStore::new());
        let tools = Arc::new(ToolRegistry::new());
        let gate = Arc::new(ToolGate::new());
        Ok(Self {
            llm,
            store,
            tools,
            gate,
            persist,
            plugin_runtime: Arc::new(kernel_contracts::UnavailablePluginRuntime),
            provider: "mock".to_string(),
            model: "mock-1".to_string(),
            bus,
        })
    }

    /// 创建一个新会话（写入 header 索引 + 首条 SessionStarted），返回代理。
    pub async fn create_session(
        &self,
        header: SessionHeader,
    ) -> Result<ReactLoopAgent, AssemblyError> {
        let session = self.store.create(header, self.bus.clone());
        self.persist
            .create_session(session.header())
            .await
            .map_err(|e| AssemblyError::Persist(e.to_string()))?;
        let rt = self.loop_runtime();
        Ok(ReactLoopAgent::new(rt, session))
    }

    /// 从持久化日志恢复会话（kill -9 后重载），自动做 interrupted-turn 修复。
    pub async fn restore_session(
        &self,
        session_id: &str,
    ) -> Result<ReactLoopAgent, AssemblyError> {
        let Some(events) = self
            .persist
            .load_events(session_id)
            .await
            .map_err(|e| AssemblyError::Persist(e.to_string()))?
        else {
            return Err(AssemblyError::SessionNotFound(session_id.to_string()));
        };
        if events.is_empty() {
            return Err(AssemblyError::InvalidLog("empty event log".into()));
        }
        // 首条必须是 SessionStarted，从中取回 header。
        let first = events
            .first()
            .ok_or_else(|| AssemblyError::InvalidLog("empty log".into()))?;
        let header = match first {
            SessionEvent::SessionStarted { header } => header.clone(),
            _ => {
                return Err(AssemblyError::InvalidLog(
                    "first event not SessionStarted".into(),
                ))
            }
        };

        let original_len = events.len();
        // interrupted-turn 修复：把尾部未配对的 Step/Turn Started 修剪掉。
        let repaired = repair_interrupted_turn(events);
        // 修复必须落盘：磁盘与内存一致，torn-tail 是磁盘层不变量。
        if repaired.len() != original_len {
            self.persist
                .rewrite_events(session_id, &repaired)
                .await
                .map_err(|e| AssemblyError::Persist(e.to_string()))?;
        }

        let records: Vec<kernel_contracts::SessionRecord> = repaired
            .iter()
            .enumerate()
            .map(|(i, e)| {
                kernel_contracts::SessionRecord::new((i + 1) as u64, session_id, e.clone())
            })
            .collect();
        let session = self
            .store
            .restore(header, records, self.bus.clone())
            .map_err(|e| AssemblyError::InvalidLog(e.to_string()))?;
        let rt = self.loop_runtime();
        Ok(ReactLoopAgent::new(rt, session))
    }

    /// 列出已持久化的会话 id。
    pub async fn list_sessions(&self) -> PortResult<Vec<String>> {
        self.persist.list_sessions().await
    }

    /// 探测插件运行时（fail-loud：未装配必须显式处理）。
    pub fn plugin_availability(&self) -> PluginRuntimeAvailability {
        self.plugin_runtime.availability()
    }

    fn loop_runtime(&self) -> Arc<LoopRuntime> {
        Arc::new(LoopRuntime {
            llm: Arc::clone(&self.llm),
            store: Arc::clone(&self.store),
            tools: Arc::clone(&self.tools),
            gate: Arc::clone(&self.gate),
            persist: Arc::clone(&self.persist),
            provider: self.provider.clone(),
            model: self.model.clone(),
        })
    }
}

/// interrupted-turn 修复：修剪未完成回合的尾部未配对事件。
///
/// kill -9 可能发生在回合中途（Step Started 已落、Ended 未落，或 Turn Started 已落、
/// Ended 未落）。恢复时这些"悬空"事件会污染投影（例如 Step Started 后没有消息），
/// 因此按配对规则修剪：从尾部往回，遇到未配对的 Started 就丢弃。
fn repair_interrupted_turn(events: Vec<SessionEvent>) -> Vec<SessionEvent> {
    let mut repaired = events;
    let mut step_open = 0u64;
    let mut turn_open = false;
    let mut cut = repaired.len();
    for (idx, ev) in repaired.iter().enumerate().rev() {
        match ev {
            SessionEvent::Step {
                phase: StepPhase::Ended,
                ..
            } => {
                step_open += 1;
            }
            SessionEvent::Step {
                phase: StepPhase::Started,
                ..
            } => {
                if step_open > 0 {
                    step_open -= 1;
                } else if cut == repaired.len() {
                    cut = idx;
                }
            }
            SessionEvent::Turn(TurnEvent::Ended { .. }) => {
                turn_open = true;
            }
            SessionEvent::Turn(TurnEvent::Started { .. }) => {
                if turn_open {
                    turn_open = false;
                } else if cut == repaired.len() {
                    cut = idx;
                }
            }
            _ => {}
        }
    }
    if cut < repaired.len() {
        repaired.truncate(cut);
    }
    repaired
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kernel_contracts::session::SessionId;

    fn header(id: &str) -> SessionHeader {
        SessionHeader {
            id: SessionId(id.to_string()),
            app: "test".into(),
            profile: "headless".into(),
            workspace: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn tmp_db(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bm-kernel-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("test.db")
    }

    #[tokio::test]
    async fn create_restore_roundtrip() {
        let db = tmp_db("roundtrip");
        let rt = Runtime::headless(db.clone()).unwrap();
        let agent = rt.create_session(header("s1")).await.unwrap();
        agent.run_turn(Some("hi")).await.unwrap();
        drop(rt);

        // 重开运行时，从持久化恢复。
        let rt2 = Runtime::headless(db.clone()).unwrap();
        let restored = rt2.restore_session("s1").await.unwrap();
        let events = restored.session().events();
        // 空脚本 LLM：SessionStarted + User + Step/S + AssistantMessage(空) + Step/E + TurnE
        assert_eq!(events.len(), 6);
        // 恢复后的会话可继续跑（turn 编号接续，不重复）。
        let outcome = restored.run_turn(Some("again")).await.unwrap();
        assert!(outcome.steps >= 1);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn repair_drops_open_step_tail() {
        let mut v = vec![
            SessionEvent::SessionStarted {
                header: header("x"),
            },
            SessionEvent::UserMessage { text: "hi".into() },
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Started,
            },
        ];
        // 无配对 Ended：尾巴应被修剪。
        let r = repair_interrupted_turn(v.clone());
        assert_eq!(r.len(), 2);

        v.push(SessionEvent::Step {
            turn: 1,
            step: 1,
            phase: StepPhase::Ended,
        });
        v.push(SessionEvent::Turn(TurnEvent::Started { turn: 1 }));
        let r = repair_interrupted_turn(v);
        // 完整的 step 保留，未闭合的 Turn Started 修剪。
        assert_eq!(r.len(), 4);
    }

    #[tokio::test]
    async fn plugin_runtime_fails_loud() {
        let rt = Runtime::headless(tmp_db("plugin")).unwrap();
        assert_eq!(
            rt.plugin_availability(),
            PluginRuntimeAvailability::Unavailable {
                reason: "plugin runtime is not registered in this delivery profile".into()
            }
        );
    }
}
