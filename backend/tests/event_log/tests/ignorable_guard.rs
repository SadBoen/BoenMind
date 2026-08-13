//! ignorable 守卫（D2）：未知事件 ignorable=true 跳过、
//! 缺省（false）拒绝重建——防旧版本静默读坏新日志。

use std::sync::Arc;

use bm_kernel::{EventLog, SurfaceIntent};
use bm_protocol::{BranchId, CoreEvent, ErrorCode, EventKind, SessionId};

/// 直接向 event_log 表插入"未来版本"的未知事件行，并同步分支头
/// （模拟未来版本内核写入的完整语义：head 会更新，故后续 append 不撞）。
async fn inject_unknown_row(
    conn: &turso::Connection,
    sid: &str,
    seq: i64,
    event_type: &str,
    ignorable: bool,
) {
    conn.execute(
        "INSERT INTO event_log (seq, session_id, branch_id, time, type, data, ignorable)
         VALUES (?1, ?2, 'main', 1, ?3, ?4, ?5)",
        (
            seq,
            sid,
            event_type,
            format!(
                r#"{{"seq":{seq},"session_id":"{sid}","branch_id":"main","time":1,"kind":"core","type":"{event_type}","ignorable":{ignorable}}}"#
            ),
            if ignorable { 1 } else { 0 },
        ),
    )
    .await
    .unwrap();
    conn.execute(
        "INSERT INTO branch_heads (session_id, branch_id, parent_branch, head_seq)
         VALUES (?1, 'main', NULL, ?2)
         ON CONFLICT(session_id, branch_id) DO UPDATE SET head_seq = excluded.head_seq",
        (sid, seq),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn unknown_required_event_blocks_replay() {
    let path = format!(
        "{}/bm_ignorable1_{}.db",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&path);
    let store = Arc::new(bm_storage_turso::TursoEventStore::open(&path).await.unwrap());
    let log = EventLog::new(store.clone());
    let sid = SessionId::new("sess_ig");
    let bid = BranchId::new("main");

    // 正常事件 + 未知必需事件（ignorable=false）
    log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnStart { turn: 1 }), SurfaceIntent::None).await.unwrap();
    {
        // 直接操作同一文件：打开第二个连接注入未知行（绕过内核的强类型层）
        let conn = {
            let db = turso::Builder::new_local(&path).build().await.unwrap();
            db.connect().unwrap()
        };
        inject_unknown_row(&conn, "sess_ig", 2, "future/critical", false).await;
    }

    // 重放必须拒绝（旧版本不认识但它是必需的）
    let err = log.replay(&sid, &bid).await.unwrap_err();
    assert_eq!(err.code(), ErrorCode::UnknownRequiredEvent);

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn unknown_ignorable_event_is_skipped() {
    let path = format!(
        "{}/bm_ignorable2_{}.db",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&path);
    let store = Arc::new(bm_storage_turso::TursoEventStore::open(&path).await.unwrap());
    let log = EventLog::new(store.clone());
    let sid = SessionId::new("sess_ig2");
    let bid = BranchId::new("main");

    log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnStart { turn: 1 }), SurfaceIntent::None).await.unwrap();
    {
        let conn = {
            let db = turso::Builder::new_local(&path).build().await.unwrap();
            db.connect().unwrap()
        };
        // 中间插入一个未知但可跳过的事件（ignorable=true，seq=2）
        inject_unknown_row(&conn, "sess_ig2", 2, "future/nice_to_have", true).await;
    }
    log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnEnd { turn: 1, reason: bm_protocol::TurnEndReason::Completed }), SurfaceIntent::None).await.unwrap();

    // 重放跳过未知事件，seq 连续由（被跳过的）行不参与重建——读层过滤，
    // 返回的流没有 seq 空洞冲突（read 返回 1、3）
    let evs = log.replay(&sid, &bid).await.unwrap();
    let seqs: Vec<u64> = evs.iter().map(|e| e.seq.as_u64()).collect();
    assert_eq!(seqs, vec![1, 3], "unknown ignorable event skipped");

    // 消息面不受影响
    let msgs = log.derive_messages(&sid, &bid).await.unwrap();
    assert_eq!(msgs.len(), 0);

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn custom_events_are_always_known() {
    // Custom（插件域）永远"已知"：内核透传，不触发守卫
    let store = Arc::new(bm_kernel::InMemoryEventStore::new());
    let log = EventLog::new(store);
    let sid = SessionId::new("sess_custom");
    let bid = BranchId::new("main");
    log.append(sid.clone(), bid.clone(), EventKind::Custom(bm_protocol::CustomEvent {
        event_type: "app.wiki.indexed".into(),
        data: serde_json::json!({"count": 3}),
    }), SurfaceIntent::None).await.unwrap();
    let evs = log.replay(&sid, &bid).await.unwrap();
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].kind.name(), "app.wiki.indexed");
}
