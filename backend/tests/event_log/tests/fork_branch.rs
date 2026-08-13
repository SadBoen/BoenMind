//! fork 分支（三维寻址）：分支独立计数、parent 记录、超头/重复拒绝。

use std::sync::Arc;

use bm_kernel::{EventLog, InMemoryEventStore, SurfaceIntent};
use bm_protocol::{BranchId, CoreEvent, ErrorCode, EventKind, SessionId};

fn turn(n: u32) -> EventKind {
    EventKind::Core(CoreEvent::TurnStart { turn: n })
}

#[tokio::test]
async fn fork_creates_independent_branch() {
    let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
    let sid = SessionId::new("sess_fork");
    let main = BranchId::new("main");

    log.append(sid.clone(), main.clone(), turn(1), SurfaceIntent::None).await.unwrap();
    log.append(sid.clone(), main.clone(), turn(2), SurfaceIntent::None).await.unwrap();

    // fork 出分支
    let br = log.fork(&sid, &main).await.unwrap();
    assert!(br.as_str().starts_with("br_"));

    // 新分支从 seq 1 起独立计数
    log.append(sid.clone(), br.clone(), turn(3), SurfaceIntent::None).await.unwrap();
    let br_evs = log.replay(&sid, &br).await.unwrap();
    assert_eq!(br_evs.len(), 1);
    assert_eq!(br_evs[0].seq.as_u64(), 1);

    // main 分支不受影响（head 仍是 2）
    let main_head = log.head_seq(&sid, &main).await.unwrap().unwrap();
    assert_eq!(main_head.as_u64(), 2);
}

#[tokio::test]
async fn fork_from_unknown_branch_rejected() {
    let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
    let sid = SessionId::new("sess_fork2");
    let err = log.fork(&sid, &BranchId::new("br_nonexistent")).await.unwrap_err();
    assert_eq!(err.code(), ErrorCode::ForkConflict);
}

#[tokio::test]
async fn branch_heads_track_parent() {
    let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
    let sid = SessionId::new("sess_fork3");
    let main = BranchId::new("main");
    log.append(sid.clone(), main.clone(), turn(1), SurfaceIntent::None).await.unwrap();
    let br = log.fork(&sid, &main).await.unwrap();
    let heads = log.branch_heads(&sid).await.unwrap();
    assert_eq!(heads.len(), 2);
    let br_head = heads.iter().find(|h| h.branch_id == br).unwrap();
    assert_eq!(br_head.parent_branch.as_deref(), Some("main"));
    assert_eq!(br_head.head_seq.as_u64(), 0); // 空分支 head 0
}

#[tokio::test]
async fn turso_fork_persists_and_reopens() {
    let path = format!(
        "{}/bm_fork_turso_{}.db",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&path);
    let sid = SessionId::new("sess_fork_turso");
    let main = BranchId::new("main");

    let br: BranchId;
    {
        let store = Arc::new(bm_storage_turso::TursoEventStore::open(&path).await.unwrap());
        let log = EventLog::new(store);
        log.append(sid.clone(), main.clone(), turn(1), SurfaceIntent::None).await.unwrap();
        br = log.fork(&sid, &main).await.unwrap();
        log.append(sid.clone(), br.clone(), turn(2), SurfaceIntent::None).await.unwrap();
    }
    // 重新打开：分支关系与数据持久
    {
        let store = Arc::new(bm_storage_turso::TursoEventStore::open(&path).await.unwrap());
        let log = EventLog::new(store);
        let heads = log.branch_heads(&sid).await.unwrap();
        assert_eq!(heads.len(), 2);
        let br_head = heads.iter().find(|h| h.branch_id == br).unwrap();
        assert_eq!(br_head.parent_branch.as_deref(), Some("main"));
        let br_evs = log.replay(&sid, &br).await.unwrap();
        assert_eq!(br_evs.len(), 1);
        assert_eq!(br_evs[0].seq.as_u64(), 1);
    }
    let _ = std::fs::remove_file(&path);
}
