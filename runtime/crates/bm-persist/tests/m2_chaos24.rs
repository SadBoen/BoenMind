//! T8 混沌④/②:过期 CAS 写入拒绝并留审计事件;状态库损坏自日志重建、
//! 行为无差异(ADR-0004 条件 1/8 的 M2 适配映射)。

use bm_contract::events::{EventEnvelope, EventType};
use bm_contract::ids::{BmId, IdGen, SeqIdGen};
use bm_contract::timestamp::now;
use bm_persist::{EventStore, PersistStore, dump_all};
use serde_json::json;

#[test]
fn t28_stale_cas_rejected_and_audited_chaos4() {
    let dir = tempfile::tempdir().expect("临时目录");
    let store = PersistStore::open(dir.path()).expect("打开");

    // 建立位点:推进到 seq 5 并快照
    for seq in 1..=5 {
        let e = EventEnvelope::new_unchecked(
            seq,
            EventType::RuntimeStarted,
            now(),
            None,
            None,
            None,
            json!({"pid": 1, "version": "0.1.0-m1", "started_at": now()}),
        );
        EventStore::record(&store, &e).expect("写穿");
    }
    store.snapshot().expect("快照");
    assert_eq!(store.snapshot_seq().expect("读快照"), Some(5));

    // 过期写入:expect=3(实际 5)→ CasMismatch → 审计事件落盘
    let rejected = store
        .reject_and_audit(6, "snapshot_seq", "3", "99")
        .expect("审计路径");
    assert!(rejected, "过期写入必须被拒");

    // 审计事件在日志里(键集过注册表),且被拒的值没有写入
    let log = EventStore::replay_since(&store, 0).expect("重放");
    let audit = log
        .iter()
        .find(|e| e.event_type == EventType::StoreWriteRejected)
        .expect("存在审计事件");
    assert_eq!(audit.event_seq, 6);
    assert_eq!(audit.payload["key"], "snapshot_seq");
    assert_eq!(audit.payload["reason"], "stale_epoch");
    assert_eq!(
        store.snapshot_seq().expect("读快照"),
        Some(5),
        "被拒的值不得生效"
    );

    // 合法 CAS(带正确 expect)仍可用
    let ok = store
        .reject_and_audit(7, "snapshot_seq", "5", "5")
        .expect("审计路径");
    assert!(!ok, "正确 expect 不产生拒绝");
}

#[test]
fn t29_corrupt_state_db_rebuilt_from_log_chaos2() {
    let dir = tempfile::tempdir().expect("临时目录");
    let ids = SeqIdGen::new();

    // ① 用真实运行时产出一个含已完成回合的目录
    let store = PersistStore::open(dir.path()).expect("打开");
    let sess: BmId = ids.next_id("sess");
    let agent: BmId = ids.next_id("agent");
    let op: BmId = ids.next_id("op");
    let s = sess.as_str().to_string();
    let a = agent.as_str().to_string();
    let o = op.as_str().to_string();
    let mut seq = 0u64;
    let mut push = |ty: EventType,
                    payload: serde_json::Value,
                    session: Option<BmId>,
                    agentv: Option<BmId>,
                    opv: Option<BmId>| {
        seq += 1;
        let e = EventEnvelope::new(seq, ty, now(), session, agentv, opv, payload);
        EventStore::record(&store, &e).expect("写穿");
    };
    push(
        EventType::SessionCreated,
        json!({"session_id": s, "agent_id": a}),
        Some(sess.clone()),
        None,
        None,
    );
    push(
        EventType::AgentCreated,
        json!({"agent_id": a, "session_id": s, "model_chain": ["zhipu.glm-4-flash"],
                "budget": {"max_tokens": 1000, "max_turns": 5}}),
        Some(sess.clone()),
        Some(agent.clone()),
        None,
    );
    push(
        EventType::AgentTurnStarted,
        json!({"agent_id": a, "operation_id": o, "turn_index": 1}),
        Some(sess.clone()),
        Some(agent.clone()),
        Some(op.clone()),
    );
    push(
        EventType::AgentWaitingModel,
        json!({"agent_id": a, "operation_id": o, "model_id": "zhipu.glm-4-flash"}),
        Some(sess.clone()),
        Some(agent.clone()),
        Some(op.clone()),
    );
    push(
        EventType::ModelInvocationCompleted,
        json!({"operation_id": o, "agent_id": a, "model_id": "zhipu.glm-4-flash",
                "attempt": 1, "usage_in": 412, "usage_out": 58,
                "latency_ms": 1873, "stream_interrupted": false}),
        Some(sess.clone()),
        Some(agent.clone()),
        Some(op.clone()),
    );
    push(
        EventType::OperationStateChanged,
        json!({"operation_id": o, "from": "running", "to": "succeeded",
                "reason_code": "result_recorded"}),
        Some(sess.clone()),
        Some(agent.clone()),
        Some(op.clone()),
    );
    push(
        EventType::AgentCompleted,
        json!({"agent_id": a, "operation_id": o, "turn_index": 1,
               "content": "重建场景的回答"}),
        Some(sess.clone()),
        Some(agent.clone()),
        Some(op.clone()),
    );

    let expected = dump_all(store.state()).expect("损坏前导出");
    drop(store);

    // ② 损坏状态库(投影类本地库受损;事件日志完好)
    std::fs::write(dir.path().join("state.db"), b"THIS IS NOT SQLITE").expect("损坏");

    // ③ 韧性打开:自日志重建,行为无差异
    let (store2, rebuilt) = PersistStore::open_resilient(dir.path()).expect("韧性打开");
    assert!(rebuilt, "必须发生重建");

    // 行为无差异:全部行与损坏前一致
    let after = dump_all(store2.state()).expect("重建后导出");
    assert_eq!(after["sessions"], expected["sessions"]);
    assert_eq!(after["agents"], expected["agents"]);
    assert_eq!(after["operations"], expected["operations"]);
    assert_eq!(after["operations"][0]["state"], "succeeded");
    assert_eq!(after["agents"][0]["budget_used_tokens"], 470);

    // 事件日志完好:全量重放条数一致
    assert_eq!(EventStore::replay_since(&store2, 0).expect("重放").len(), 7);
}
