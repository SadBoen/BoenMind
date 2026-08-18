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
use kernel_contracts::session::{
    SessionEvent, SessionHeader, StepPhase, TurnEndReason, TurnEvent,
};
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
    /// 单回合最大 step 数（数值可配置；装配默认 [`kernel_loop::DEFAULT_MAX_STEPS`]）。
    pub max_steps: u64,
}

impl Runtime {
    /// 创建一个新的运行时（headless profile：内存 store + sqlite 持久化 + mock LLM）。
    pub fn headless(sqlite_path: PathBuf) -> Result<Self, AssemblyError> {
        Self::headless_with_max_steps(sqlite_path, kernel_loop::DEFAULT_MAX_STEPS)
    }

    /// 带 max_steps 的 headless 装配（web-server 经 `--max-steps` 覆盖）。
    pub fn headless_with_max_steps(
        sqlite_path: PathBuf,
        max_steps: u64,
    ) -> Result<Self, AssemblyError> {
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
            max_steps,
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
            max_steps: self.max_steps,
        })
    }
}

/// interrupted-turn 修复：闭合崩溃孤儿回合（对齐 DSH session-persistence 恢复语义）。
///
/// kill -9 可能发生在回合中途（Step Started 已落、Ended 未落，或 Turn Started 已落、
/// Ended 未落）。**不删除任何已落盘事件**——只扫描配对，发现未闭合的 Step/Turn
/// 就在日志尾部追加 closers（Step Ended + `Turn Ended{Interrupted}`），把回合闭合。
/// 这样事件日志作为唯一事实源完整保留（含已闭合错误回合的 requestId 审计事实），
/// 且不越过 Turn Ended 截断——后续已闭合回合的历史永不丢失。
fn repair_interrupted_turn(events: Vec<SessionEvent>) -> Vec<SessionEvent> {
    let mut repaired = events;
    // 正向扫描配对深度：depth>0 说明有未配对 Started（本回合或嵌套残留）。
    let mut step_open: u64 = 0;
    let mut turn_open: u64 = 0;
    let mut last_open_step: Option<(u64, u64)> = None;
    let mut last_open_turn: Option<u64> = None;
    for ev in &repaired {
        match ev {
            SessionEvent::Step {
                turn,
                step,
                phase: StepPhase::Started,
            } => {
                step_open += 1;
                last_open_step = Some((*turn, *step));
            }
            SessionEvent::Step {
                phase: StepPhase::Ended,
                ..
            } => {
                step_open = step_open.saturating_sub(1);
                if step_open == 0 {
                    last_open_step = None;
                }
            }
            SessionEvent::Turn(TurnEvent::Started { turn }) => {
                turn_open += 1;
                last_open_turn = Some(*turn);
            }
            SessionEvent::Turn(TurnEvent::Ended { .. }) => {
                turn_open = turn_open.saturating_sub(1);
                if turn_open == 0 {
                    last_open_turn = None;
                }
            }
            _ => {}
        }
    }
    if let Some((turn, step)) = last_open_step {
        repaired.push(SessionEvent::Step {
            turn,
            step,
            phase: StepPhase::Ended,
        });
    }
    if let Some(turn) = last_open_turn {
        repaired.push(SessionEvent::Turn(TurnEvent::Ended {
            turn,
            reason: TurnEndReason::Interrupted,
        }));
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
        // 空脚本 LLM：SessionStarted + User + Turn Started + Step/S + AssistantChunk(Finish)
        // + AssistantMessage(空) + Step/E + TurnE
        assert_eq!(events.len(), 8);
        // 恢复后的会话可继续跑（turn 编号接续，不重复）。
        let outcome = restored.run_turn(Some("again")).await.unwrap();
        assert!(outcome.steps >= 1);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn repair_closes_open_step_and_turn_tail() {
        // 未配对 Step Started（kill-9 于 step 中）：追加 closers 闭合，
        // 历史事件（SessionStarted/UserMessage/Turn Started）全部保留。
        let v = vec![
            SessionEvent::SessionStarted {
                header: header("x"),
            },
            SessionEvent::UserMessage { text: "hi".into() },
            SessionEvent::Turn(TurnEvent::Started { turn: 1 }),
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Started,
            },
        ];
        let r = repair_interrupted_turn(v);
        assert_eq!(r.len(), 6);
        assert!(matches!(
            r[4],
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Ended
            }
        ));
        assert!(matches!(
            r[5],
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Interrupted
            })
        ));
    }

    #[test]
    fn repair_keeps_closed_history_and_closes_only_tail() {
        // 完整闭合的 turn 1 + kill-9 于 turn 2 中途：只闭合 turn 2 尾部，
        // turn 1 的 Turn Ended（含审计事实）原样保留——不越过 Turn Ended 截断。
        let v = vec![
            SessionEvent::SessionStarted {
                header: header("x"),
            },
            SessionEvent::UserMessage { text: "hi".into() },
            SessionEvent::Turn(TurnEvent::Started { turn: 1 }),
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Started,
            },
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Ended,
            },
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Error {
                    message: "boom".into(),
                    code: "E".into(),
                    request_id: Some("req-1".into()),
                },
            }),
            SessionEvent::UserMessage { text: "again".into() },
            SessionEvent::Turn(TurnEvent::Started { turn: 2 }),
            SessionEvent::Step {
                turn: 2,
                step: 1,
                phase: StepPhase::Started,
            },
        ];
        let r = repair_interrupted_turn(v);
        assert_eq!(r.len(), 11);
        // turn 1 的闭合回合（含 requestId）保留。
        assert!(matches!(
            &r[5],
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Error {
                    code,
                    request_id: Some(rid),
                    ..
                }
            }) if code == "E" && rid == "req-1"
        ));
        // 尾部 = closers（Step Ended + Turn Ended{Interrupted}）。
        assert!(matches!(
            r[9],
            SessionEvent::Step {
                turn: 2,
                step: 1,
                phase: StepPhase::Ended
            }
        ));
        assert!(matches!(
            r[10],
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 2,
                reason: TurnEndReason::Interrupted
            })
        ));
    }

    /// 取消/报错回合（已闭合，含 requestId）→ 用户再发消息 → kill-9 于新回合中途 →
    /// restore：错误回合历史完整保留，中断回合被 closers 闭合（回归 P1-2/P1-3：
    /// 旧实现会在未配对 Step Started 处整段截断，删掉错误回合及后续全部历史）。
    #[tokio::test]
    async fn restore_after_closed_error_turn_preserves_history() {
        let db = tmp_db("cancel-history");
        let rt = Runtime::headless(db.clone()).unwrap();
        let agent = rt.create_session(header("s1")).await.unwrap();
        // 手工构造：turn 1 错误闭合（Step 配对 + Turn Ended Error 带 requestId），
        // turn 2 中断于 Step Started（模拟 kill-9 于取消后新回合中途）。
        let seq = [
            SessionEvent::Turn(TurnEvent::Started { turn: 1 }),
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Started,
            },
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Ended,
            },
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Error {
                    message: "boom".into(),
                    code: "E".into(),
                    request_id: Some("req-1".into()),
                },
            }),
            SessionEvent::Turn(TurnEvent::Started { turn: 2 }),
            SessionEvent::Step {
                turn: 2,
                step: 1,
                phase: StepPhase::Started,
            },
        ];
        for e in seq {
            let rec = agent.session().append(e);
            rt.persist
                .append_events("s1", std::slice::from_ref(&rec.event))
                .await
                .unwrap();
        }
        drop(rt);

        let rt2 = Runtime::headless(db.clone()).unwrap();
        let restored = rt2.restore_session("s1").await.unwrap();
        let events: Vec<kernel_contracts::session::SessionEvent> = restored
            .session()
            .events()
            .into_iter()
            .map(|r| r.event)
            .collect();
        // 错误回合的审计事实（requestId）不丢。
        assert!(events.iter().any(|e| matches!(
            e,
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Error {
                    code,
                    request_id: Some(rid),
                    ..
                }
            }) if code == "E" && rid == "req-1"
        )));
        // 中断回合被 closers 闭合。
        assert!(events.iter().any(|e| matches!(
            e,
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 2,
                reason: TurnEndReason::Interrupted
            })
        )));
        // 恢复后 turn 编号接续（下一次 run_turn 应开 turn 3）。
        let outcome = restored.run_turn(Some("again")).await.unwrap();
        assert!(outcome.steps >= 1);
        let starts: Vec<u64> = restored
            .session()
            .events()
            .into_iter()
            .filter_map(|r| match r.event {
                SessionEvent::Turn(TurnEvent::Started { turn }) => Some(turn),
                _ => None,
            })
            .collect();
        assert_eq!(starts, vec![1, 2, 3]);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
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
