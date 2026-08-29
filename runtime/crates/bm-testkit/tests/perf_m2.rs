//! M2 性能定标:P-02(Session 恢复)/ P-04(事件追加吞吐)/ P-05(事件回放)/
//! P-07(磁盘增量)。默认 #[ignore];回填时
//! `cargo test --release -p bm-testkit --test perf_m2 -- --ignored --nocapture`。

use bm_contract::events::{EventEnvelope, EventType};
use bm_contract::ids::IdGen;
use bm_contract::timestamp::now;
use bm_contract::wire::SessionResumeParams;
use bm_persist::{EventStore, PersistStore};
use std::sync::Arc;
use std::time::Instant;

fn runtime_ev(seq: u64) -> EventEnvelope {
    EventEnvelope::new_unchecked(
        seq,
        EventType::RuntimeStarted,
        now(),
        None,
        None,
        None,
        serde_json::json!({"pid": 1, "version": "0.1.0-m1", "started_at": now()}),
    )
}

fn percentile(sorted: &mut [u128], p: f64) -> u128 {
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx]
}

/// 预置 1 万条持久事件(含一个真实会话,使 resume 有对象)。
fn seed_10k(dir: &std::path::Path) -> PersistStore {
    // P-02 前提 = 完整 1 万条日志:关闭压实(否则前缀被截,题设不成立)
    let store = PersistStore::open(dir).expect("打开").without_compaction();
    let ids = bm_contract::ids::SeqIdGen::new();
    let sess: bm_contract::ids::BmId = ids.next_id("sess");
    let agent: bm_contract::ids::BmId = ids.next_id("agent");
    let mut seq = 0u64;
    let mut rec = |e: EventEnvelope| {
        seq += 1;
        EventStore::record(&store, &e).expect("写穿");
    };
    rec(EventEnvelope::new_unchecked(
        1,
        EventType::SessionCreated,
        now(),
        Some(sess.clone()),
        None,
        None,
        serde_json::json!({"session_id": sess.as_str(), "agent_id": agent.as_str()}),
    ));
    // 余下 9999 条:周期性插入会话相关事件,保证 resume 有补发内容
    for i in 2..=10_000u64 {
        if i % 1000 == 0 {
            rec(EventEnvelope::new_unchecked(
                i,
                EventType::SessionResumed,
                now(),
                Some(sess.clone()),
                None,
                None,
                serde_json::json!({"session_id": sess.as_str(), "since_seq": 0, "replayed": 0}),
            ));
        } else {
            rec(runtime_ev(i));
        }
    }
    store
}

/// P-02 Session 恢复:含 1 万条持久事件时 session.resume 完成 p95(20 次)。
#[tokio::test]
#[ignore = "性能定标,手动运行"]
async fn p02_session_resume_with_10k_events() {
    let dir = tempfile::tempdir().expect("临时目录");
    let _store = seed_10k(dir.path());
    drop(_store);

    // 启动(含启动恢复)+ resume,计 resume 段
    let connector: Arc<dyn bm_core::ports::ModelConnector> =
        Arc::new(bm_providers::mock_model::MockConnector::new(vec![]));
    let handle = bm_core::runtime::RuntimeHandle::start(bm_core::runtime::RuntimeConfig {
        version: "0.1.0-m1".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(Arc::new(
            // P-02 题设 = 完整 1 万条日志:runtime 侧同样关闭自动压实
            PersistStore::open(dir.path())
                .expect("打开")
                .without_compaction(),
        )),
        connector,
        secret_store: Arc::new(bm_providers::secret::MemSecretStore::new()),
        id_gen: Arc::new(bm_contract::ids::SeqIdGen::new()),
        clock: Arc::new(bm_core::clock::SystemClock),
        turn_timeout_secs: bm_core::runtime::DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;

    // 会话 id 从恢复后事件流取
    let events = handle.events_all().await;
    let sess_id = events
        .iter()
        .find(|e| e.event_type == EventType::SessionCreated)
        .and_then(|e| e.payload["session_id"].as_str())
        .expect("会话存在")
        .to_string();
    let sess: bm_contract::ids::BmId = bm_contract::ids::BmId::parse(sess_id).expect("合法");

    let mut samples = Vec::new();
    for _ in 0..20 {
        let t0 = Instant::now();
        let r = handle
            .session_resume(
                bm_contract::ids::IdGen::next_id(&bm_contract::ids::SeqIdGen::new(), "req"),
                SessionResumeParams {
                    session_id: sess.clone(),
                    since_seq: Some(0),
                },
            )
            .await
            .expect("resume");
        let elapsed = t0.elapsed();
        assert!(!r.events.is_empty());
        samples.push(elapsed.as_micros());
    }
    handle.stop("perf").await;
    let p50 = percentile(&mut samples, 0.50) as f64 / 1000.0;
    let p95 = percentile(&mut samples, 0.95) as f64 / 1000.0;
    println!("P-02 resume_ms(10k events): p50={p50:.2} p95={p95:.2} (N=20)");
}

/// P-04 事件追加吞吐(含落盘,fsync 每条):批量 1 万事件计时。
#[test]
#[ignore = "性能定标,手动运行"]
fn p04_event_append_throughput() {
    let dir = tempfile::tempdir().expect("临时目录");
    let store = PersistStore::open(dir.path()).expect("打开");
    let n = 10_000u64;
    let t0 = Instant::now();
    for seq in 1..=n {
        EventStore::record(&store, &runtime_ev(seq)).expect("写穿");
    }
    let secs = t0.elapsed().as_secs_f64();
    println!(
        "P-04 append_events_per_sec(含 fsync+物化): {:.0} (N={n})",
        n as f64 / secs
    );
}

/// P-05 事件回放速率:自日志重放 1 万事件到全新状态库。
#[test]
#[ignore = "性能定标,手动运行"]
fn p05_event_replay_rate() {
    let dir = tempfile::tempdir().expect("临时目录");
    {
        let store = PersistStore::open(dir.path())
            .expect("打开")
            .without_compaction();
        for seq in 1..=10_000u64 {
            EventStore::record(&store, &runtime_ev(seq)).expect("写穿");
        }
    }
    // 全新库重放:source = 预置日志,dest = 全新状态库
    let seeded = PersistStore::open(dir.path()).expect("重开预置库");
    let dir2 = tempfile::tempdir().expect("临时目录");
    let fresh = PersistStore::open(dir2.path()).expect("新库");
    let t0 = Instant::now();
    let n = bm_persist::rebuild_projection(&seeded, 10_000, fresh.state()).expect("重放");
    let secs = t0.elapsed().as_secs_f64();
    println!("P-05 replay_events_per_sec: {:.0} (N={n})", n as f64 / secs);
}

/// P-07 磁盘增量:每千条事件的磁盘增量(状态库+事件日志)。
#[test]
#[ignore = "性能定标,手动运行"]
fn p07_disk_increment_per_1k_events() {
    let dir = tempfile::tempdir().expect("临时目录");
    let store = PersistStore::open(dir.path()).expect("打开");
    let dir_size = |d: &std::path::Path| -> u64 {
        std::fs::read_dir(d)
            .expect("列目录")
            .filter_map(|e| e.ok())
            .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
            .sum()
    };
    let before = dir_size(dir.path());
    for seq in 1..=1000u64 {
        EventStore::record(&store, &runtime_ev(seq)).expect("写穿");
    }
    let after = dir_size(dir.path());
    let kb = (after.saturating_sub(before)) as f64 / 1024.0;
    println!("P-07 disk_kb_per_1k_events(状态库+事件日志): {kb:.1} KB");
}
