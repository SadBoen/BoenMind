//! # kernel-session
//!
//! 会话领域实现：append-only SessionEvent 日志（唯一事实源）+ 投影。
//!
//! 分层纪律：`Session` 是进程内会话核心，内部维护 seq 单调递增的事件日志；
//! 每次 `append` 同步发布到 `EventBus`（观察者可借此驱动投影/持久化）。
//! `derive_messages` 按日志顺序投影出发给模型的完整历史消息
//! （sessions/messages/tool_calls 均为日志的派生视图）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use kernel_contracts::{
    text_message, ContentBlock, EventBus, LlmMessage, Role, SessionEvent, SessionHeader,
    SessionId, SessionRecord,
};
use parking_lot::RwLock;
use thiserror::Error;

/// 会话操作错误。
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session log is empty")]
    EmptyLog,
    #[error("session log seq is not consecutive: expected {expected}, found {found}")]
    SeqNotConsecutive { expected: u64, found: u64 },
    #[error("first event in session log is not SessionStarted")]
    MissingSessionStarted,
    #[error("session log header does not match the provided header")]
    HeaderMismatch,
}

/// 会话：append-only 事件日志（唯一事实源）+ 投影。
pub struct Session {
    header: SessionHeader,
    log: RwLock<Vec<SessionRecord>>,
    next_seq: AtomicU64,
    bus: EventBus,
}

impl Session {
    /// 构造新会话：自动 append 一条 `SessionStarted` 事件并发布到总线。
    pub fn new(header: SessionHeader, bus: EventBus) -> Self {
        let header_for_event = header.clone();
        let session = Self {
            header,
            log: RwLock::new(Vec::new()),
            next_seq: AtomicU64::new(1),
            bus,
        };
        session.append(SessionEvent::SessionStarted {
            header: header_for_event,
        });
        session
    }

    /// 从已持久化的事件日志恢复会话。
    ///
    /// 校验：seq 从 1 开始且连续、首条必须是 `SessionStarted`、其 header 与传入 header 匹配。
    pub fn from_log(
        header: SessionHeader,
        records: Vec<SessionRecord>,
        bus: EventBus,
    ) -> Result<Self, SessionError> {
        if records.is_empty() {
            return Err(SessionError::EmptyLog);
        }
        for (i, record) in records.iter().enumerate() {
            let expected = i as u64 + 1;
            if record.seq != expected {
                return Err(SessionError::SeqNotConsecutive {
                    expected,
                    found: record.seq,
                });
            }
        }
        let started_header = match &records[0].event {
            SessionEvent::SessionStarted { header } => header,
            _ => return Err(SessionError::MissingSessionStarted),
        };
        if started_header != &header {
            return Err(SessionError::HeaderMismatch);
        }
        let next_seq = records.len() as u64 + 1;
        Ok(Self {
            header,
            log: RwLock::new(records),
            next_seq: AtomicU64::new(next_seq),
            bus,
        })
    }

    pub fn id(&self) -> &SessionId {
        &self.header.id
    }

    pub fn header(&self) -> &SessionHeader {
        &self.header
    }

    /// append 一条事件：seq 自增、时间戳 = `Utc::now()`、push 进日志、发布到总线。
    pub fn append(&self, event: SessionEvent) -> SessionRecord {
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let record = SessionRecord::new(seq, event);
        self.log.write().push(record.clone());
        self.bus.emit(&record);
        record
    }

    /// 完整事件日志副本（按 seq 升序）。
    pub fn events(&self) -> Vec<SessionRecord> {
        self.log.read().clone()
    }

    /// 日志内最大 seq；空日志返回 0。
    pub fn last_seq(&self) -> u64 {
        self.log.read().last().map(|r| r.seq).unwrap_or(0)
    }

    /// 投影：按日志顺序生成发给模型的完整历史消息。
    ///
    /// - `UserMessage{text}` → user 文本消息
    /// - `AssistantMessage{content}` → assistant 消息（content 原样，含工具调用块）
    /// - `ToolResult{result}` → tool 消息（`call_id` + `output` 回填）
    /// - 其余事件（Turn/Step/AssistantChunk/SessionStarted/Ended/ToolCall）跳过
    pub fn derive_messages(&self) -> Vec<LlmMessage> {
        let log = self.log.read();
        let mut messages = Vec::new();
        for record in log.iter() {
            match &record.event {
                SessionEvent::UserMessage { text } => {
                    messages.push(text_message(Role::User, text.clone()));
                }
                SessionEvent::AssistantMessage { content } => {
                    messages.push(LlmMessage {
                        role: Role::Assistant,
                        content: content.clone(),
                    });
                }
                SessionEvent::ToolResult { result } => {
                    messages.push(LlmMessage {
                        role: Role::Tool,
                        content: vec![ContentBlock::ToolResult(result.clone())],
                    });
                }
                _ => {}
            }
        }
        messages
    }
}

/// 进程内会话存储：`SessionId → Arc<Session>`。
#[derive(Default)]
pub struct SessionStore {
    sessions: RwLock<HashMap<SessionId, Arc<Session>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建新会话并登记到存储。
    pub fn create(&self, header: SessionHeader, bus: EventBus) -> Arc<Session> {
        let session = Arc::new(Session::new(header.clone(), bus));
        self.sessions.write().insert(header.id, Arc::clone(&session));
        session
    }

    /// 从事件日志恢复会话并登记到存储。
    pub fn restore(
        &self,
        header: SessionHeader,
        records: Vec<SessionRecord>,
        bus: EventBus,
    ) -> Result<Arc<Session>, SessionError> {
        let session = Arc::new(Session::from_log(header.clone(), records, bus)?);
        self.sessions.write().insert(header.id, Arc::clone(&session));
        Ok(session)
    }

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions
            .read()
            .get(&SessionId(id.to_string()))
            .cloned()
    }

    /// 全部会话 id（按字典序稳定输出）。
    pub fn list(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.sessions.read().keys().map(|k| k.0.clone()).collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kernel_contracts::{ToolCall, ToolCallResult};

    fn header(id: &str) -> SessionHeader {
        SessionHeader {
            id: SessionId(id.to_string()),
            app: "test".to_string(),
            profile: "test".to_string(),
            workspace: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn append_is_monotonic_and_projection_is_correct() {
        let session = Session::new(header("s1"), EventBus::new());

        // 构造即 append SessionStarted{seq=1}
        let events = session.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].seq, 1);
        assert!(matches!(events[0].event, SessionEvent::SessionStarted { .. }));

        let r2 = session.append(SessionEvent::UserMessage {
            text: "hello".to_string(),
        });
        let r3 = session.append(SessionEvent::AssistantMessage {
            content: vec![ContentBlock::Text("hi".to_string())],
        });
        let r4 = session.append(SessionEvent::ToolResult {
            result: ToolCallResult {
                call_id: "call_1".to_string(),
                output: "42".to_string(),
                is_error: false,
            },
        });
        assert_eq!(r2.seq, 2);
        assert_eq!(r3.seq, 3);
        assert_eq!(r4.seq, 4);
        assert_eq!(session.last_seq(), 4);

        let seqs: Vec<u64> = session.events().iter().map(|r| r.seq).collect();
        assert_eq!(seqs, vec![1, 2, 3, 4]);

        // 投影：user / assistant / tool 顺序正确，ToolResult 回填 call_id + output
        let messages = session.derive_messages();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(
            messages[0].content,
            vec![ContentBlock::Text("hello".to_string())]
        );
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(
            messages[1].content,
            vec![ContentBlock::Text("hi".to_string())]
        );
        assert_eq!(messages[2].role, Role::Tool);
        assert_eq!(
            messages[2].content,
            vec![ContentBlock::ToolResult(ToolCallResult {
                call_id: "call_1".to_string(),
                output: "42".to_string(),
                is_error: false,
            })]
        );
    }

    #[test]
    fn append_publishes_to_event_bus() {
        let bus = EventBus::new();
        let received: Arc<parking_lot::Mutex<Vec<u64>>> = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let probe = Arc::clone(&received);
        let _disposer = bus.on_event(move |record| {
            probe.lock().push(record.seq);
        });

        let session = Session::new(header("bus"), bus);
        session.append(SessionEvent::UserMessage {
            text: "a".to_string(),
        });
        session.append(SessionEvent::UserMessage {
            text: "b".to_string(),
        });

        let seqs = received.lock().clone();
        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[test]
    fn from_log_rejects_non_consecutive_seq() {
        let records = vec![
            SessionRecord::new(1, SessionEvent::SessionStarted { header: header("x") }),
            SessionRecord::new(3, SessionEvent::UserMessage { text: "a".to_string() }),
        ];
        let err = Session::from_log(header("x"), records, EventBus::new())
            .err()
            .unwrap();
        assert!(matches!(
            err,
            SessionError::SeqNotConsecutive { expected: 2, found: 3 }
        ));
    }

    #[test]
    fn from_log_rejects_missing_started_and_header_mismatch() {
        // 首条不是 SessionStarted
        let records = vec![SessionRecord::new(
            1,
            SessionEvent::UserMessage { text: "a".to_string() },
        )];
        let err = Session::from_log(header("x"), records, EventBus::new())
            .err()
            .unwrap();
        assert!(matches!(err, SessionError::MissingSessionStarted));

        // header 不匹配
        let records = vec![SessionRecord::new(
            1,
            SessionEvent::SessionStarted { header: header("other") },
        )];
        let err = Session::from_log(header("x"), records, EventBus::new())
            .err()
            .unwrap();
        assert!(matches!(err, SessionError::HeaderMismatch));

        // 空日志
        let err = Session::from_log(header("x"), Vec::new(), EventBus::new())
            .err()
            .unwrap();
        assert!(matches!(err, SessionError::EmptyLog));
    }

    #[test]
    fn store_create_restore_and_get() {
        let store = SessionStore::new();
        let h = header("s1");
        let created = store.create(h.clone(), EventBus::new());
        assert_eq!(store.list(), vec!["s1".to_string()]);
        assert_eq!(created.last_seq(), 1);

        created.append(SessionEvent::UserMessage {
            text: "hello".to_string(),
        });
        assert_eq!(created.last_seq(), 2);

        // 从日志恢复（必须使用与日志内 SessionStarted 相同的 header）
        let records = created.events();
        let restored = store
            .restore(created.header().clone(), records, EventBus::new())
            .expect("restore from valid log");
        assert_eq!(restored.last_seq(), 2);
        assert_eq!(restored.id(), &SessionId("s1".to_string()));

        // get 命中且为恢复后的实例
        let got = store.get("s1").expect("session present in store");
        assert_eq!(got.last_seq(), 2);
        assert_eq!(store.list().len(), 1);
        assert!(store.get("nope").is_none());
    }

    #[test]
    fn store_restore_rejects_invalid_log() {
        let store = SessionStore::new();
        let records = vec![
            SessionRecord::new(1, SessionEvent::SessionStarted { header: header("s2") }),
            SessionRecord::new(2, SessionEvent::UserMessage { text: "a".to_string() }),
            SessionRecord::new(4, SessionEvent::UserMessage { text: "b".to_string() }),
        ];
        assert!(store.restore(header("s2"), records, EventBus::new()).is_err());
        assert!(store.get("s2").is_none());
    }

    #[test]
    fn derive_messages_skips_non_message_events() {
        let session = Session::new(header("s3"), EventBus::new());
        session.append(SessionEvent::UserMessage {
            text: "u".to_string(),
        });
        session.append(SessionEvent::Turn(kernel_contracts::TurnEvent::Started { turn: 1 }));
        session.append(SessionEvent::Step {
            turn: 1,
            step: 1,
            phase: kernel_contracts::StepPhase::Started,
        });
        session.append(SessionEvent::AssistantChunk {
            text: "chunk".to_string(),
        });
        session.append(SessionEvent::ToolCall {
            call: ToolCall {
                id: "c1".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({ "text": "hi" }),
            },
        });
        session.append(SessionEvent::SessionEnded {
            reason: "done".to_string(),
        });

        // 只投影出 UserMessage 一条
        let messages = session.derive_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
    }
}
