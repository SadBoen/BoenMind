//! 崩溃恢复语义验证（现行机制）：
//! - `repair_heads` 启动自愈——branch_heads 落后于 event_log 实际 max(seq) 时对齐；
//! - `recover_interrupted_turns`（A4）——TurnStart 后无 TurnEnd 的未闭合回合，
//!   启动补写 TurnEnd{reason: Interrupted}，且幂等。
//!
//! 历史：checkpoint 策略（checkpoint_state 表 interrupted/clean 标记）已于
//! 2026-08-15 删除——单事务 append 保证无半写，其崩溃恢复职责由
//! repair_heads + recover_interrupted_turns 承接（见 5c6451b）。

use std::sync::Arc;

use bm_kernel::{EventLog, SurfaceIntent};
use bm_protocol::{BranchId, CoreEvent, EventKind, EventStorePort, SessionId, TurnEndReason};
use bm_storage_turso::{TursoEventStore, recover_interrupted_turns};

fn temp_db(name: &str) -> String {
    format!(
        "{}/{}_pid{}.db",
        std::env::temp_dir().display(),
        name,
        std::process::id()
    )
}

#[tokio::test]
async fn stale_branch_head_self_heals_on_open() {    // 模拟历史版本崩溃窗口：branch_heads 落后于 event_log 实际 max(seq)
    // （单条 append 曾是两步非事务），重启 open 的 repair_heads 自愈对齐，
    // 之后 append 不再撞 UNIQUE
    let path = temp_db("bm_repair_head");
    let _ = std::fs::remove_file(&path);
    let sid = SessionId::new("sess_repair");
    let bid = BranchId::new("main");

    {
        let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
        let log = EventLog::new(store);
        log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnStart { turn: 1 }), SurfaceIntent::None).await.unwrap();
        log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnEnd { turn: 1, reason: bm_protocol::TurnEndReason::Completed }), SurfaceIntent::None).await.unwrap();
        // 人为制造落后 head（独立连接直接改表，模拟旧版崩溃窗口）
        let db = turso::Builder::new_local(&path).build().await.unwrap();
        let conn = db.connect().unwrap();
        conn.execute(
            "UPDATE branch_heads SET head_seq = 1 WHERE session_id = ?1 AND branch_id = ?2",
            (sid.as_str(), bid.as_str()),
        )
        .await
        .unwrap();
    }

    // 重启：repair_heads 把 head 对齐到 max(seq)=2
    let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
    let head = store.head_seq(&sid, &bid).await.unwrap().map(|s| s.as_u64());
    assert_eq!(head, Some(2));
    // 继续 append 不撞 UNIQUE
    let log = EventLog::new(store);
    let seq = log
        .append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnStart { turn: 2 }), SurfaceIntent::None)
        .await
        .unwrap();
    assert_eq!(seq.as_u64(), 3);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn interrupted_turn_backfilled_on_recover() {
    // 崩溃场景：TurnStart 后无 TurnEnd（chunk 写到一半进程没了）→
    // 启动恢复补写 TurnEnd{reason: Interrupted}，且幂等
    let path = temp_db("bm_turn_interrupt");
    let _ = std::fs::remove_file(&path);
    let sid = SessionId::new("sess_int1");
    let bid = BranchId::new("main");

    {
        let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
        let log = EventLog::new(store);
        log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnStart { turn: 3 }), SurfaceIntent::None).await.unwrap();
        log.append(
            sid.clone(),
            bid.clone(),
            EventKind::Core(CoreEvent::AssistantChunk {
                turn: 3,
                step: 1,
                chunk: bm_protocol::StreamChunk { text: "半截".into() },
            }),
            SurfaceIntent::Append,
        )
        .await
        .unwrap();
        // 崩溃：TurnEnd 未落
    }

    let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
    let log = EventLog::new(store.clone());
    let n = recover_interrupted_turns(store.as_ref(), &log).await.unwrap();
    assert_eq!(n, 1);

    let evs = log.replay(&sid, &bid).await.unwrap();
    let last = evs.last().unwrap();
    match &last.kind {
        EventKind::Core(CoreEvent::TurnEnd { turn, reason }) => {
            assert_eq!(*turn, 3);
            assert_eq!(*reason, TurnEndReason::Interrupted);
        }
        other => panic!("最后一条应为 TurnEnd(Interrupted)，得到 {other:?}"),
    }

    // 幂等：已闭合回合不再补写
    let n2 = recover_interrupted_turns(store.as_ref(), &log).await.unwrap();
    assert_eq!(n2, 0);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn closed_turns_and_multi_session_untouched() {
    // 已闭合回合 / 不同会话互不干扰；只有真正未闭合的会话被补写
    let path = temp_db("bm_turn_multi");
    let _ = std::fs::remove_file(&path);
    let closed = SessionId::new("sess_closed");
    let open = SessionId::new("sess_open");
    let bid = BranchId::new("main");

    {
        let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
        let log = EventLog::new(store);
        // 会话 A：完整闭合
        log.append(closed.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnStart { turn: 1 }), SurfaceIntent::None).await.unwrap();
        log.append(
            closed.clone(),
            bid.clone(),
            EventKind::Core(CoreEvent::TurnEnd { turn: 1, reason: TurnEndReason::Completed }),
            SurfaceIntent::None,
        )
        .await
        .unwrap();
        // 会话 B：两回合，第二回合未闭合
        log.append(open.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnStart { turn: 1 }), SurfaceIntent::None).await.unwrap();
        log.append(
            open.clone(),
            bid.clone(),
            EventKind::Core(CoreEvent::TurnEnd { turn: 1, reason: TurnEndReason::Completed }),
            SurfaceIntent::None,
        )
        .await
        .unwrap();
        log.append(open.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnStart { turn: 2 }), SurfaceIntent::None).await.unwrap();
    }

    let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
    let log = EventLog::new(store.clone());
    let n = recover_interrupted_turns(store.as_ref(), &log).await.unwrap();
    assert_eq!(n, 1, "只补写真正未闭合的回合");

    // 闭合会话不受影响；未闭合会话补的是 turn 2
    let closed_evs = log.replay(&closed, &bid).await.unwrap();
    assert_eq!(closed_evs.len(), 2);
    let open_evs = log.replay(&open, &bid).await.unwrap();
    assert_eq!(open_evs.len(), 4);
    match &open_evs.last().unwrap().kind {
        EventKind::Core(CoreEvent::TurnEnd { turn, reason }) => {
            assert_eq!(*turn, 2);
            assert_eq!(*reason, TurnEndReason::Interrupted);
        }
        other => panic!("应为 TurnEnd(Interrupted)，得到 {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}
