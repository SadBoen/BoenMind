//! T5 自动压实策略 + T6 重放确定性(混沌③,ADR-0004 条件 1/8)。

use bm_contract::events::{EventEnvelope, EventType};
use bm_contract::ids::IdGen;
use bm_contract::timestamp::now;
use bm_persist::{EventStore, PersistStore, StateDb, dump_all, rebuild_projection};
use serde_json::json;

fn ev(seq: u64, kind: &str) -> EventEnvelope {
    let ty = match kind {
        "session" => EventType::SessionCreated,
        _ => EventType::RuntimeStarted,
    };
    let payload = match kind {
        "session" => json!({
            "session_id": format!("sess_{seq:0>26}"),
            "agent_id": format!("agent_{seq:0>26}"),
        }),
        _ => json!({"pid": 1, "version": "0.1.0-m1", "started_at": now()}),
    };
    EventEnvelope::new_unchecked(seq, ty, now(), None, None, None, payload)
}

#[test]
fn t23_auto_compaction_triggers_and_keeps_suffix() {
    let dir = tempfile::tempdir().expect("临时目录");
    let store = PersistStore::with_compaction(dir.path(), 10).expect("打开");

    // 25 条事件:应在 seq=10、20 两次触发快照+压实
    for seq in 1..=25 {
        let kind = if seq % 5 == 0 { "session" } else { "runtime" };
        EventStore::record(&store, &ev(seq, kind)).expect("写穿");
    }

    assert_eq!(EventStore::last_applied_seq(&store).expect("位点"), 25);
    let snap: u64 = store.snapshot_seq().expect("快照位点").unwrap_or(0);
    assert_eq!(snap, 20, "最后一次自动压实快照位点");

    // 日志前缀已截断:仅保留 seq > 20 的 5 条
    let kept = EventStore::replay_since(&store, 0).expect("重放");
    assert_eq!(kept.len(), 5, "前缀已压实");
    assert_eq!(kept[0].event_seq, 21);

    // 重开自洽
    drop(store);
    let store2 = PersistStore::open(dir.path()).expect("重开");
    assert_eq!(EventStore::last_applied_seq(&store2).expect("位点"), 25);
    assert_eq!(EventStore::last_log_seq(&store2).expect("日志末尾"), 25);
}

#[test]
fn t24_rebuild_projection_is_deterministic_chaos3() {
    let dir = tempfile::tempdir().expect("临时目录");
    let store = PersistStore::open(dir.path()).expect("打开");

    // 混合真实事件流(两个会话,各自一个成功回合)
    let ids = bm_contract::ids::SeqIdGen::new();
    let mut seq = 0u64;
    let mut push = |ty: EventType,
                    session: Option<bm_contract::ids::BmId>,
                    agent: Option<bm_contract::ids::BmId>,
                    op: Option<bm_contract::ids::BmId>,
                    payload: serde_json::Value| {
        seq += 1;
        let e = EventEnvelope::new(seq, ty, now(), session, agent, op, payload);
        EventStore::record(&store, &e).expect("写穿");
    };

    for _ in 0..2 {
        let sess: bm_contract::ids::BmId = ids.next_id("sess");
        let agent: bm_contract::ids::BmId = ids.next_id("agent");
        let op: bm_contract::ids::BmId = ids.next_id("op");
        let s = sess.as_str().to_string();
        let a = agent.as_str().to_string();
        let o = op.as_str().to_string();
        push(
            EventType::SessionCreated,
            Some(sess.clone()),
            None,
            None,
            json!({"session_id": s, "agent_id": a}),
        );
        push(
            EventType::AgentCreated,
            Some(sess.clone()),
            Some(agent.clone()),
            None,
            json!({"agent_id": a, "session_id": s, "model_chain": ["zhipu.glm-4-flash"],
                    "budget": {"max_tokens": 1000, "max_turns": 5}}),
        );
        push(
            EventType::AgentTurnStarted,
            Some(sess.clone()),
            Some(agent.clone()),
            Some(op.clone()),
            json!({"agent_id": a, "operation_id": o, "turn_index": 1}),
        );
        push(
            EventType::AgentWaitingModel,
            Some(sess.clone()),
            Some(agent.clone()),
            Some(op.clone()),
            json!({"agent_id": a, "operation_id": o, "model_id": "zhipu.glm-4-flash"}),
        );
        push(
            EventType::ModelInvocationCompleted,
            Some(sess.clone()),
            Some(agent.clone()),
            Some(op.clone()),
            json!({"operation_id": o, "agent_id": a, "model_id": "zhipu.glm-4-flash",
                    "attempt": 1, "usage_in": 412, "usage_out": 58,
                    "latency_ms": 1873, "stream_interrupted": false,
                    "content": "seed", "content_truncated": false}),
        );
        push(
            EventType::OperationStateChanged,
            Some(sess.clone()),
            Some(agent.clone()),
            Some(op.clone()),
            json!({"operation_id": o, "from": "running", "to": "succeeded",
                    "reason_code": "result_recorded"}),
        );
        push(
            EventType::AgentCompleted,
            Some(sess.clone()),
            Some(agent.clone()),
            Some(op.clone()),
            json!({"agent_id": a, "operation_id": o, "turn_index": 1,
                   "content": "chaos3 回答"}),
        );
    }

    // 混沌③:同一前缀两次重建,结果逐字段一致
    let dest1 = StateDb::open(&dir.path().join("d1.db")).expect("dest1");
    let dest2 = StateDb::open(&dir.path().join("d2.db")).expect("dest2");
    let last1 = rebuild_projection(&store, seq, &dest1).expect("重建1");
    let last2 = rebuild_projection(&store, seq, &dest2).expect("重建2");
    assert_eq!(last1, last2);
    assert_eq!(last1, seq);
    assert_eq!(
        dump_all(&dest1).expect("导出1"),
        dump_all(&dest2).expect("导出2"),
        "混沌③:同前缀两次重建必须一致"
    );

    // 部分前缀重建(≤7)同样确定,且不包含 seq=8 之后的事实
    let p1 = StateDb::open(&dir.path().join("p1.db")).expect("p1");
    let p2 = StateDb::open(&dir.path().join("p2.db")).expect("p2");
    rebuild_projection(&store, 7, &p1).expect("前缀重建1");
    rebuild_projection(&store, 7, &p2).expect("前缀重建2");
    assert_eq!(
        dump_all(&p1).expect("导出p1"),
        dump_all(&p2).expect("导出p2")
    );
    let d1 = dump_all(&p1).expect("dump");
    assert_eq!(
        d1["operations"].as_array().expect("数组").len(),
        1,
        "前缀 7 只含第一回合"
    );
    assert_eq!(d1["meta"][0]["value"], "7", "重建位点=前缀末尾");
}

#[test]
fn t27_schema_v1_to_v2_expand_migration() {
    let dir = tempfile::tempdir().expect("临时目录");
    let db_path = dir.path().join("state.db");

    let conn = rusqlite::Connection::open(&db_path).expect("建 v1 库");
    conn.execute_batch(
        r#"
        CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE sessions (id TEXT PRIMARY KEY, state TEXT, agent_id TEXT, created_at TEXT);
        CREATE TABLE agents (id TEXT PRIMARY KEY);
        CREATE TABLE operations (
            id TEXT PRIMARY KEY, session_id TEXT, agent_id TEXT, request_id TEXT,
            state TEXT, turn_index INTEGER, created_at TEXT, completed_at TEXT,
            action_summary TEXT, result_ref TEXT, error_code TEXT, error_message TEXT
        );
        CREATE TABLE tombstones (kind TEXT, id TEXT, at TEXT, PRIMARY KEY (kind, id));
        PRAGMA user_version = 1;
        "#,
    )
    .expect("v1 schema");
    conn.execute(
        "INSERT INTO operations(id, session_id, agent_id, state, turn_index, created_at)
         VALUES('op_x', 's', 'a', 'running', 1, '2026-08-29T00:00:00.000Z')",
        [],
    )
    .expect("v1 数据");
    drop(conn);

    let db = StateDb::open(&db_path).expect("v2 打开");
    let rows = db
        .query_rows(
            "SELECT id, state, input_content FROM operations WHERE id='op_x'",
            &[],
        )
        .expect("查询");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["state"], "running");
    assert!(rows[0]["input_content"].is_null(), "旧数据该列为 NULL");
}
