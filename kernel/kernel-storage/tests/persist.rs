//! kernel-storage 集成测试：以公共 API（`SqlitePersist` + `SessionPersistPort`）
//! 验证持久层契约。每条测试用 `std::env::temp_dir()` + uuid 子目录，测完（含 panic）
//! 由 RAII guard 清理。

use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use kernel_contracts::{
    PortError, PortErrorKind, SessionEvent, SessionHeader, SessionId, SessionPersistPort, TurnEvent,
};
use kernel_storage::SqlitePersist;

/// 临时目录：uuid 子目录，Drop 时清理（panic 展开也会执行）。
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "kernel-storage-test-{}",
            uuid::Uuid::new_v4()
        )))
    }

    fn db(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn header(id: &str, created_at: DateTime<Utc>) -> SessionHeader {
    SessionHeader {
        id: SessionId(id.to_string()),
        app: "test".to_string(),
        profile: "unit".to_string(),
        workspace: Some("/tmp/ws".to_string()),
        created_at,
        updated_at: created_at,
    }
}

#[tokio::test]
async fn create_then_load_round_trips_header_and_started_event() {
    let dir = TempDir::new();
    let db_path = dir.db("roundtrip.db");
    let store = SqlitePersist::open(&db_path).expect("open should create the database");
    assert_eq!(store.path(), db_path.as_path());
    assert!(db_path.exists(), "database file must exist after open");

    let now = Utc::now();
    let mut h = header("s1", now);
    // 故意给一个不同于 created_at 的 updated_at：create 必须归一化为 created_at。
    h.updated_at = now + Duration::hours(1);

    store.create_session(&h).await.expect("create_session");
    assert_ne!(h.updated_at, now, "caller's header must not be mutated");

    let events = store
        .load_events("s1")
        .await
        .expect("load_events")
        .expect("session should exist");
    assert_eq!(events.len(), 1, "fresh session holds exactly the SessionStarted event");
    match &events[0] {
        SessionEvent::SessionStarted { header } => {
            assert_eq!(header.id.as_str(), "s1");
            assert_eq!(header.app, "test");
            assert_eq!(header.profile, "unit");
            assert_eq!(header.workspace.as_deref(), Some("/tmp/ws"));
            assert_eq!(header.created_at, now);
            assert_eq!(header.updated_at, now, "stored updated_at must equal created_at");
        }
        other => panic!("expected SessionStarted, got {other:?}"),
    }

    // 底层核对：首条事件 seq 必须为 1（顺带验证 WAL 下多连接可读）。
    let raw = rusqlite::Connection::open(&db_path).expect("open raw connection");
    let (seq, event_json): (i64, String) = raw
        .query_row(
            "SELECT seq, event_json FROM events WHERE session_id='s1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read first event row");
    assert_eq!(seq, 1);
    let ev: SessionEvent = serde_json::from_str(&event_json).expect("deserialize event_json");
    assert!(matches!(ev, SessionEvent::SessionStarted { .. }));
}

#[tokio::test]
async fn append_batches_keep_order_and_contiguous_seq() {
    let dir = TempDir::new();
    let db_path = dir.db("append.db");
    let store = SqlitePersist::open(&db_path).expect("open");
    let now = Utc::now();
    store.create_session(&header("s2", now)).await.expect("create_session");

    store
        .append_events(
            "s2",
            &[
                SessionEvent::UserMessage {
                    text: "hello".to_string(),
                },
                SessionEvent::Turn(TurnEvent::Started { turn: 1 }),
            ],
        )
        .await
        .expect("append batch 1");

    store
        .append_events(
            "s2",
            &[
                SessionEvent::AssistantChunk {
                    chunk: kernel_contracts::StreamChunk::TextDelta {
                        index: 0,
                        text: "world".to_string(),
                    },
                },
                SessionEvent::SessionEnded {
                    reason: "done".to_string(),
                },
            ],
        )
        .await
        .expect("append batch 2");

    let events = store.load_events("s2").await.expect("load_events").expect("exists");
    assert_eq!(events.len(), 5, "started + 2 + 2");
    assert!(matches!(&events[0], SessionEvent::SessionStarted { .. }));
    assert!(
        matches!(&events[1], SessionEvent::UserMessage { text } if text == "hello"),
        "batch 1 order must be preserved"
    );
    assert!(matches!(&events[2], SessionEvent::Turn(TurnEvent::Started { turn: 1 })));
    assert!(
        matches!(&events[3], SessionEvent::AssistantChunk { chunk: kernel_contracts::StreamChunk::TextDelta { text, .. } } if text == "world"),
        "batch 2 must follow batch 1 in order"
    );
    assert!(matches!(&events[4], SessionEvent::SessionEnded { reason } if reason == "done"));

    // seq 必须连续：1..=5。
    let raw = rusqlite::Connection::open(&db_path).expect("raw connection");
    let seqs: Vec<i64> = raw
        .prepare("SELECT seq FROM events WHERE session_id='s2' ORDER BY seq")
        .expect("prepare seq query")
        .query_map([], |row| row.get(0))
        .expect("run seq query")
        .collect::<Result<_, _>>()
        .expect("collect seqs");
    assert_eq!(seqs, vec![1, 2, 3, 4, 5], "seq must be contiguous across batches");

    // 空批次是无操作。
    store.append_events("s2", &[]).await.expect("empty append is a no-op");
    let after = store.load_events("s2").await.expect("load after empty append").expect("exists");
    assert_eq!(after.len(), 5);
}

#[tokio::test]
async fn append_is_one_transaction_no_torn_tail() {
    let dir = TempDir::new();
    let db_path = dir.db("atomic.db");
    let store = SqlitePersist::open(&db_path).expect("open");
    let now = Utc::now();
    store.create_session(&header("s3", now)).await.expect("create_session");

    // 用第二条原生连接装一个触发器：往 events 插入 seq>=3 时立刻 FAIL——
    // 构造"批内第二条 INSERT 成功、第三条失败"的场景（kill -9 落在事务中段的等价物）。
    let raw = rusqlite::Connection::open(&db_path).expect("raw connection");
    raw.execute_batch(
        "CREATE TRIGGER fail_third BEFORE INSERT ON events \
         WHEN NEW.seq >= 3 BEGIN SELECT RAISE(FAIL, 'boom'); END;",
    )
    .expect("create failing trigger");

    // 批次 [seq2, seq3]：seq2 插入成功、seq3 被触发器拒绝 → 整个事务必须回滚。
    let res = store
        .append_events(
            "s3",
            &[
                SessionEvent::UserMessage {
                    text: "first".to_string(),
                },
                SessionEvent::UserMessage {
                    text: "second".to_string(),
                },
            ],
        )
        .await;
    assert!(res.is_err(), "a batch with a failing insert must error");

    // 原子性证明：seq2 绝不能残留（无 torn-tail）。
    let events = store.load_events("s3").await.expect("load_events").expect("exists");
    assert_eq!(
        events.len(),
        1,
        "rolled-back batch must leave zero trace (SessionStarted only)"
    );

    // 摘除触发器后同一批次完整落盘。
    raw.execute_batch("DROP TRIGGER fail_third;").expect("drop trigger");
    store
        .append_events(
            "s3",
            &[
                SessionEvent::UserMessage {
                    text: "first".to_string(),
                },
                SessionEvent::UserMessage {
                    text: "second".to_string(),
                },
            ],
        )
        .await
        .expect("append after trigger removed");

    let events = store.load_events("s3").await.expect("load after retry").expect("exists");
    assert_eq!(events.len(), 3);
    assert_eq!(events[1], SessionEvent::UserMessage { text: "first".to_string() });
    assert_eq!(events[2], SessionEvent::UserMessage { text: "second".to_string() });
}

#[tokio::test]
async fn append_to_missing_session_returns_not_found_and_leaves_no_residue() {
    let dir = TempDir::new();
    let db_path = dir.db("missing.db");
    let store = SqlitePersist::open(&db_path).expect("open");

    let err = store
        .append_events("ghost", &[SessionEvent::UserMessage { text: "x".to_string() }])
        .await
        .expect_err("append to a missing session must fail");
    match err {
        PortError {
            kind: PortErrorKind::NotFound,
            ..
        } => {}
        other => panic!("expected NotFound, got {other:?}"),
    }

    // 后续 create + append：事件从 seq=1 干净开始，无失败批次残留。
    let now = Utc::now();
    store.create_session(&header("ghost", now)).await.expect("create_session");
    store
        .append_events("ghost", &[SessionEvent::UserMessage { text: "y".to_string() }])
        .await
        .expect("append");

    let events = store.load_events("ghost").await.expect("load_events").expect("exists");
    assert_eq!(events.len(), 2, "started + 1 user message, no residue");
    assert!(matches!(&events[1], SessionEvent::UserMessage { text } if text == "y"));

    // seq 必须恰好是 1 和 2。
    let raw = rusqlite::Connection::open(&db_path).expect("raw connection");
    let seqs: Vec<i64> = raw
        .prepare("SELECT seq FROM events WHERE session_id='ghost' ORDER BY seq")
        .expect("prepare seq query")
        .query_map([], |row| row.get(0))
        .expect("run seq query")
        .collect::<Result<_, _>>()
        .expect("collect seqs");
    assert_eq!(seqs, vec![1, 2]);
}

#[tokio::test]
async fn delete_session_removes_header_and_events() {
    let dir = TempDir::new();
    let db_path = dir.db("delete.db");
    let store = SqlitePersist::open(&db_path).expect("open");
    let now = Utc::now();
    store.create_session(&header("s4", now)).await.expect("create_session");
    store
        .append_events("s4", &[SessionEvent::UserMessage { text: "a".to_string() }])
        .await
        .expect("append");

    assert!(store.load_events("s4").await.expect("load before delete").is_some());

    store.delete_session("s4").await.expect("delete_session");
    assert!(store.load_events("s4").await.expect("load after delete").is_none());

    // events 表也必须清干净（手动双删的验证）。
    let raw = rusqlite::Connection::open(&db_path).expect("raw connection");
    let n: i64 = raw
        .query_row(
            "SELECT COUNT(*) FROM events WHERE session_id='s4'",
            [],
            |row| row.get(0),
        )
        .expect("count remaining events");
    assert_eq!(n, 0, "events table must be cleaned by delete_session too");

    // 幂等：再删不存在的会话也返回 Ok。
    store.delete_session("s4").await.expect("idempotent delete of missing session");
}

#[tokio::test]
async fn list_sessions_sorts_by_most_recent_updated_at() {
    let dir = TempDir::new();
    let store = SqlitePersist::open(&dir.db("list.db")).expect("open");
    let base = Utc::now();
    // created_at 全部设在过去，且彼此不同 → 排序确定且与墙上时钟无关。
    store.create_session(&header("old", base - Duration::seconds(3))).await.expect("create old");
    store.create_session(&header("mid", base - Duration::seconds(2))).await.expect("create mid");
    store.create_session(&header("new", base - Duration::seconds(1))).await.expect("create new");

    let list = store.list_sessions().await.expect("list_sessions");
    assert_eq!(list, vec!["new", "mid", "old"], "must sort by updated_at desc");

    // 向 old 追加事件会刷新 updated_at（= now，晚于所有 created_at）→ 排到最前。
    store
        .append_events("old", &[SessionEvent::UserMessage { text: "bump".to_string() }])
        .await
        .expect("append to bump updated_at");
    let list = store.list_sessions().await.expect("list after bump");
    assert_eq!(list, vec!["old", "new", "mid"], "recently appended session must move to front");
}
