//! M8-T3:Judge 确定性评估——同区间两次评估报告逐字节一致;
//! 报告过合同 schema;报告落库 round-trip。

use bm_contract::events::{EventEnvelope, EventType};
use bm_contract::registries;
use bm_contract::schemas;
use bm_judge::evaluate;
use bm_persist::{EventStore, PersistStore};
use serde_json::json;

#[test]
fn t117_judge_deterministic_and_contract_shaped() {
    let dir = tempfile::tempdir().expect("临时目录");
    let store = PersistStore::open(dir.path()).expect("打开");

    // 区间 [1,4]:启动事实 + 一次副作用调用(intent→ok)+ 一次模型调用完成
    let op_a = "op_01JAAAAAAAAAAAAAAAAAAAAAA1";
    let store_records = vec![
        EventEnvelope::new(
            1,
            EventType::RuntimeStarted,
            "2026-08-30T12:00:00.000Z".into(),
            None,
            None,
            None,
            json!({"pid": 1, "version": "0.1.0-m8", "started_at": "2026-08-30T12:00:00.000Z"}),
        ),
        EventEnvelope::new(
            2,
            EventType::CapabilityInvoked,
            "2026-08-30T12:00:01.000Z".into(),
            None,
            None,
            Some(op_a.parse().unwrap()),
            json!({
                "call_id": "call_01JAAAAAAAAAAAAAAAAAAAAAA1",
                "operation_id": op_a,
                "capability": "mcp.wiki.page.write",
                "principal": "surface:user",
                "binding_epoch": 1,
                "provider_instance_id": "mcp.wiki@0.1.0",
                "outcome": "intent",
                "error_code": null,
                "idempotency_key_hash": "sha256:aa"
            }),
        ),
        EventEnvelope::new(
            3,
            EventType::CapabilityInvoked,
            "2026-08-30T12:00:02.000Z".into(),
            None,
            None,
            Some(op_a.parse().unwrap()),
            json!({
                "call_id": "call_01JAAAAAAAAAAAAAAAAAAAAAA1",
                "operation_id": op_a,
                "capability": "mcp.wiki.page.write",
                "principal": "surface:user",
                "binding_epoch": 1,
                "provider_instance_id": "mcp.wiki@0.1.0",
                "outcome": "ok",
                "error_code": null,
                "idempotency_key_hash": "sha256:aa"
            }),
        ),
        EventEnvelope::new(
            4,
            EventType::ModelInvocationCompleted,
            "2026-08-30T12:00:03.000Z".into(),
            None,
            None,
            Some(op_a.parse().unwrap()),
            json!({
                "operation_id": op_a, "agent_id": op_a,
                "model_id": "gpt-5.6-luna", "attempt": 1,
                "usage_in": 100, "usage_out": 10,
                "latency_ms": 1800, "stream_interrupted": false,
                "content": "seed", "content_truncated": false
            }),
        ),
    ];
    for e in &store_records {
        store.record(e).expect("记录");
    }
    store
        .outbox_upsert(
            op_a,
            "side_effect",
            "published",
            &json!({"capability": "mcp.wiki.page.write"}).to_string(),
            "2026-08-30T12:00:02.500Z",
        )
        .expect("outbox");

    // 两次评估 → 逐字节一致(确定性;generated_at 取区间内最大 occurred_at)
    let r1 = evaluate(&store, 1, 4).expect("评估1");
    let r2 = evaluate(&store, 1, 4).expect("评估2");
    assert_eq!(
        serde_json::to_string(&r1).unwrap(),
        serde_json::to_string(&r2).unwrap(),
        "同输入必须恒同报告"
    );

    // 报告过合同 schema
    schemas::validate(registries::EVALUATION_REPORT_SCHEMA, &r1)
        .expect("报告必须过 evaluation-report.v0_1");

    // 全部检查 pass(事件键集/单终态/副作用收据/延迟)
    assert_eq!(r1["summary"]["failed"], json!(0), "{r1}");
    assert_eq!(r1["summary"]["skipped"], json!(0), "{r1}");
    assert_eq!(r1["range"], json!({"from_seq": 1, "to_seq": 4}));

    // 报告落库 round-trip(v8 表)
    store
        .save_evaluation_report(
            r1["report_id"].as_str().unwrap(),
            1,
            4,
            &r1.to_string(),
            r1["generated_at"].as_str().unwrap(),
        )
        .expect("落库");
    let listed = store.list_evaluation_reports().expect("列表");
    assert_eq!(listed.len(), 1);
    let back: serde_json::Value =
        serde_json::from_str(listed[0]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(back, r1, "round-trip 后报告一致");
}

#[test]
fn t117b_judge_flags_unmatched_intent_and_multi_terminal() {
    let dir = tempfile::tempdir().expect("临时目录");
    let store = PersistStore::open(dir.path()).expect("打开");
    let op_b = "op_01JAAAAAAAAAAAAAAAAAAAAAA2";
    let records = vec![
        // intent 无任何完成、无 outbox 行 → receipt.side_effect fail
        EventEnvelope::new(
            1,
            EventType::CapabilityInvoked,
            "2026-08-30T12:00:00.000Z".into(),
            None,
            None,
            Some(op_b.parse().unwrap()),
            json!({
                "call_id": "call_01JAAAAAAAAAAAAAAAAAAAAAA2",
                "operation_id": op_b,
                "capability": "mcp.wiki.page.write",
                "principal": "surface:user",
                "binding_epoch": 1,
                "provider_instance_id": "mcp.wiki@0.1.0",
                "outcome": "intent",
                "error_code": null,
                "idempotency_key_hash": null
            }),
        ),
        // 同一 operation 两次终态 → inv.single_terminal fail
        EventEnvelope::new(
            2,
            EventType::OperationStateChanged,
            "2026-08-30T12:00:01.000Z".into(),
            None,
            None,
            Some(op_b.parse().unwrap()),
            json!({"operation_id": op_b, "from": "running", "to": "failed", "reason_code": "x"}),
        ),
        EventEnvelope::new(
            3,
            EventType::OperationStateChanged,
            "2026-08-30T12:00:02.000Z".into(),
            None,
            None,
            Some(op_b.parse().unwrap()),
            json!({"operation_id": op_b, "from": "failed", "to": "succeeded", "reason_code": "y"}),
        ),
    ];
    for e in &records {
        store.record(e).expect("记录");
    }
    let report = evaluate(&store, 1, 3).expect("评估");
    assert_eq!(report["summary"]["failed"], json!(2), "{report}");
    let ids: Vec<&str> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["verdict"] == "fail")
        .map(|c| c["check_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"receipt.side_effect"), "{ids:?}");
    assert!(ids.contains(&"inv.single_terminal"), "{ids:?}");
}
