//! checkpoint 恢复：请求边界 fsync + 崩溃 interrupted 恢复。

use std::sync::Arc;

use bm_kernel::{EventLog, SurfaceIntent};
use bm_protocol::{BranchId, CoreEvent, EventKind, EventStorePort, SessionId};
use bm_storage_turso::{CheckpointState, CheckpointStore, TursoEventStore};

fn temp_db(name: &str) -> String {
    format!(
        "{}/{}_pid{}.db",
        std::env::temp_dir().display(),
        name,
        std::process::id()
    )
}

#[tokio::test]
async fn clean_checkpoint_no_recovery_needed() {
    let path = temp_db("bm_ckpt_clean");
    let _ = std::fs::remove_file(&path);
    let (ckpt, state) = CheckpointStore::open(&path).await.unwrap();
    assert_eq!(state, CheckpointState::Clean);
    ckpt.mark_clean(10).await.unwrap();
    let recovered = ckpt.recover(Some(10)).await.unwrap();
    assert_eq!(recovered, CheckpointState::Clean);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn interrupted_marker_recovers_to_clean() {
    let path = temp_db("bm_ckpt_interrupt");
    let _ = std::fs::remove_file(&path);
    let (ckpt, _) = CheckpointStore::open(&path).await.unwrap();
    ckpt.mark_interrupted().await.unwrap();
    assert_eq!(ckpt.read_state().await.unwrap(), CheckpointState::Interrupted);

    // 模拟崩溃后重启：事件日志实际头 seq=5（事务已提交），恢复归 clean
    let before = ckpt.recover(Some(5)).await.unwrap();
    assert_eq!(before, CheckpointState::Interrupted);
    assert_eq!(ckpt.read_state().await.unwrap(), CheckpointState::Clean);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn full_flow_data_survives_restart() {
    // 完整链路：写事件 → mark_clean → 模拟崩溃（不 mark）→ 重启后
    // recover 核对 → 数据完整可重放
    let path = temp_db("bm_ckpt_flow");
    let _ = std::fs::remove_file(&path);
    let sid = SessionId::new("sess_ckpt_flow");
    let bid = BranchId::new("main");

    {
        let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
        let log = EventLog::new(store.clone());
        let (ckpt, _) = CheckpointStore::open(&path).await.unwrap();

        for i in 1..=10u32 {
            ckpt.mark_interrupted().await.unwrap();
            log.append(
                sid.clone(),
                bid.clone(),
                EventKind::Core(CoreEvent::TurnStart { turn: i }),
                SurfaceIntent::None,
            )
            .await
            .unwrap();
        }
        ckpt.mark_clean(10).await.unwrap();
        // 模拟崩溃：不清 clean 标记直接"重启"
    }

    // 重启：打开存储 + checkpoint，recover 后数据完整
    let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
    let log = EventLog::new(store);
    let (ckpt, state) = CheckpointStore::open(&path).await.unwrap();
    assert_eq!(state, CheckpointState::Clean);
    ckpt.recover(log.head_seq(&sid, &bid).await.unwrap().map(|s| s.as_u64())).await.unwrap();

    let evs = log.replay(&sid, &bid).await.unwrap();
    assert_eq!(evs.len(), 10);
    assert_eq!(evs[9].seq.as_u64(), 10);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn restart_without_clean_marker_recovers() {
    // 最恶劣场景：写了一半"崩溃"（interrupted 未清），重启后恢复
    let path = temp_db("bm_ckpt_crash");
    let _ = std::fs::remove_file(&path);
    let sid = SessionId::new("sess_ckpt_crash");
    let bid = BranchId::new("main");

    {
        let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
        let log = EventLog::new(store);
        let (ckpt, _) = CheckpointStore::open(&path).await.unwrap();
        ckpt.mark_interrupted().await.unwrap();
        log.append(sid.clone(), bid.clone(), EventKind::Core(CoreEvent::TurnStart { turn: 1 }), SurfaceIntent::None).await.unwrap();
        // 崩溃：未 mark_clean
    }

    let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
    let log = EventLog::new(store);
    let (ckpt, state) = CheckpointStore::open(&path).await.unwrap();
    assert_eq!(state, CheckpointState::Interrupted);
    let head = log.head_seq(&sid, &bid).await.unwrap().map(|s| s.as_u64());
    // 事务原子性：写入完整（seq=1），恢复归 clean 并保留数据
    let before = ckpt.recover(head).await.unwrap();
    assert_eq!(before, CheckpointState::Interrupted);
    assert_eq!(ckpt.read_state().await.unwrap(), CheckpointState::Clean);
    let evs = log.replay(&sid, &bid).await.unwrap();
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].seq.as_u64(), 1);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn stale_branch_head_self_heals_on_open() {
    // 模拟历史版本崩溃窗口：branch_heads 落后于 event_log 实际 max(seq)
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
