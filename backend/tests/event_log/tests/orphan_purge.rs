//! C1 回收站超期清除：孤儿会话（sessions 表已删）的超期事件物理删除。
//! 语义：删除会话 = 入回收站（事件仍留 event_log）；超期（time < before）
//! 才物理删除；在册会话的事件永不删。

use std::sync::Arc;

use bm_kernel::{EventLog, SurfaceIntent};
use bm_protocol::{BranchId, CoreEvent, EventKind, SessionId};
use bm_storage_turso::TursoEventStore;

fn turn(n: u32) -> EventKind {
    EventKind::Core(CoreEvent::TurnStart { turn: n })
}

/// 建最小 sessions 表（purge 只读引用其 id 列；完整 schema 属 bm-core）。
async fn create_sessions_table(path: &str) {
    let db = turso::Builder::new_local(path).build().await.unwrap();
    let conn = db.connect().unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY)",
    )
    .await
    .unwrap();
}

/// 直接把某会话全部事件的时间改成指定值（超期场景需要老时间戳）。
async fn set_session_time(path: &str, session: &str, time_ms: i64) {
    let db = turso::Builder::new_local(path).build().await.unwrap();
    let conn = db.connect().unwrap();
    conn.execute(
        "UPDATE event_log SET time = ?1 WHERE session_id = ?2",
        (time_ms, session),
    )
    .await
    .unwrap();
}

fn temp_db(name: &str) -> String {
    format!(
        "{}/{}_pid{}.db",
        std::env::temp_dir().display(),
        name,
        std::process::id()
    )
}

#[tokio::test]
async fn purge_removes_only_expired_orphan_events() {
    let path = temp_db("bm_orphan_purge");
    let _ = std::fs::remove_file(&path);
    create_sessions_table(&path).await;
    let bid = BranchId::new("main");
    let now = 1_760_000_000_000i64;
    let old = now - 100 * 86_400_000; // 100 天前
    let recent = now - 10 * 86_400_000; // 10 天前

    let live = SessionId::new("sess_live");
    let orphan_old = SessionId::new("sess_orphan_old");
    let orphan_recent = SessionId::new("sess_orphan_recent");

    {
        let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
        let log = EventLog::new(store);
        for (sid, turns) in [
            (&live, vec![1, 2]),
            (&orphan_old, vec![1, 2, 3]),
            (&orphan_recent, vec![1]),
        ] {
            for t in turns {
                log.append(sid.clone(), bid.clone(), turn(t), SurfaceIntent::None)
                    .await
                    .unwrap();
            }
        }
    }
    // 在册会话（live）在 sessions 表有行：直接插入；孤儿不插（已删）
    {
        let db = turso::Builder::new_local(&path).build().await.unwrap();
        let conn = db.connect().unwrap();
        conn.execute("INSERT INTO sessions (id) VALUES (?1)", [live.as_str()])
            .await
            .unwrap();
    }
    // 老孤儿变老、新孤儿保持新
    set_session_time(&path, orphan_old.as_str(), old).await;
    set_session_time(&path, orphan_recent.as_str(), recent).await;
    set_session_time(&path, live.as_str(), old).await; // 在册会话再老也不删

    let store = Arc::new(TursoEventStore::open(&path).await.unwrap());
    let removed = store.purge_orphaned_events(now - 90 * 86_400_000).await.unwrap();
    assert_eq!(removed, 3, "只删超期孤儿（orphan_old 的 3 条）");

    let log = EventLog::new(store.clone());
    assert_eq!(log.replay(&live, &bid).await.unwrap().len(), 2, "在册会话不动");
    assert_eq!(log.replay(&orphan_old, &bid).await.unwrap().len(), 0);
    assert_eq!(
        log.replay(&orphan_recent, &bid).await.unwrap().len(),
        1,
        "未超期的孤儿仍在回收站"
    );
    // orphan_old 的分支头随之清理，orphan_recent 的头保留
    let heads = log.branch_heads(&orphan_old).await.unwrap();
    assert!(heads.is_empty(), "事件清空的孤儿分支头一并删除");
    let heads = log.branch_heads(&orphan_recent).await.unwrap();
    assert_eq!(heads.len(), 1);
    let _ = std::fs::remove_file(&path);
}
