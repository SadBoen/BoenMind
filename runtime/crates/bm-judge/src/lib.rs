//! bm-judge:独立评估器(M8.7)。
//!
//! 只读事件日志与收据(outbox),不依赖运行时内存态;对给定事件区间
//! 产出确定性评估报告(evaluation/evaluation-report.v0_1)——同输入
//! 恒同报告,可复跑、可对比。LLM 定性注解为可选层,不进报告必填字段。

use bm_contract::events::EventType;
use bm_contract::ids::ulid26_for_counter;
use bm_persist::EventStore;
use serde_json::{Value, json};

pub const JUDGE_VERSION: &str = "0.1.0";

/// 终态 operation 状态(operation.state.changed 的 to 值)。
const TERMINAL_OP_STATES: [&str; 4] = ["succeeded", "failed", "cancelled", "timeout"];

#[derive(Debug, thiserror::Error)]
pub enum JudgeError {
    #[error("区间非法: from={from} > to={to}")]
    BadRange { from: u64, to: u64 },
    #[error("存储故障: {0}")]
    Store(String),
}

struct Check {
    check_id: &'static str,
    verdict: &'static str, // pass | fail | skipped
    evidence: String,
}

/// 评估 [from_seq, to_seq] 闭区间,产出合同形态报告 JSON。
pub fn evaluate(store: &dyn EventStore, from_seq: u64, to_seq: u64) -> Result<Value, JudgeError> {
    if from_seq > to_seq || from_seq == 0 {
        return Err(JudgeError::BadRange {
            from: from_seq,
            to: to_seq,
        });
    }
    // replay_since(s) 返回 seq > s 的事件;闭区间须从 from_seq-1 起取
    let events: Vec<_> = store
        .replay_since(from_seq - 1)
        .map_err(|e| JudgeError::Store(e.to_string()))?
        .into_iter()
        .filter(|e| e.event_seq <= to_seq)
        .collect();

    let checks = vec![
        check_seq_contiguous(&events, from_seq, to_seq),
        check_event_shape(&events),
        check_single_terminal(&events),
        check_side_effect_receipts(store, &events)?,
        check_latency(&events),
    ];

    let passed = checks.iter().filter(|c| c.verdict == "pass").count() as u64;
    let failed = checks.iter().filter(|c| c.verdict == "fail").count() as u64;
    let skipped = checks.iter().filter(|c| c.verdict == "skipped").count() as u64;

    // 确定性:generated_at 取区间内最大 occurred_at(不引入墙钟)
    let generated_at = events
        .iter()
        .map(|e| e.occurred_at.clone())
        .max()
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into());

    Ok(json!({
        "report_id": format!("rep_{}", ulid26_for_counter(from_seq)),
        "range": {"from_seq": from_seq, "to_seq": to_seq},
        "checks": checks.into_iter().map(|c| json!({
            "check_id": c.check_id,
            "verdict": c.verdict,
            "evidence": c.evidence,
        })).collect::<Vec<_>>(),
        "summary": {"passed": passed, "failed": failed, "skipped": skipped},
        "judge_version": JUDGE_VERSION,
        "generated_at": generated_at,
    }))
}

fn check_seq_contiguous(
    events: &[bm_contract::events::EventEnvelope],
    from: u64,
    to: u64,
) -> Check {
    if events.is_empty() {
        return Check {
            check_id: "seq.contiguous",
            verdict: "skipped",
            evidence: format!("区间 [{from},{to}] 无事件"),
        };
    }
    let mut gaps = 0usize;
    for w in events.windows(2) {
        if w[1].event_seq != w[0].event_seq + 1 {
            gaps += 1;
        }
    }
    if events[0].event_seq != from {
        gaps += 1;
    }
    Check {
        check_id: "seq.contiguous",
        verdict: if gaps == 0 { "pass" } else { "fail" },
        evidence: format!("n={} from={} to={} gaps={}", events.len(), from, to, gaps),
    }
}

fn check_event_shape(events: &[bm_contract::events::EventEnvelope]) -> Check {
    let mut bad = Vec::new();
    for e in events {
        match EventType::from_wire(e.event_type.as_str()) {
            Some(ty) => {
                let mut expected: Vec<&str> = ty.payload_keys().to_vec();
                expected.sort_unstable();
                let mut actual: Vec<&str> = e
                    .payload
                    .as_object()
                    .map(|o| o.keys().map(|s| s.as_str()).collect())
                    .unwrap_or_default();
                actual.sort_unstable();
                if expected != actual {
                    bad.push(e.event_seq);
                }
            }
            None => bad.push(e.event_seq),
        }
    }
    Check {
        check_id: "event.registry_keys",
        verdict: if bad.is_empty() { "pass" } else { "fail" },
        evidence: if bad.is_empty() {
            format!("n={} 全部命中注册表键集", events.len())
        } else {
            format!("键集漂移 seq={bad:?}")
        },
    }
}

fn check_single_terminal(events: &[bm_contract::events::EventEnvelope]) -> Check {
    use std::collections::HashMap;
    let mut per_op: HashMap<String, u32> = HashMap::new();
    for e in events {
        if e.event_type.as_str() == "operation.state.changed"
            && TERMINAL_OP_STATES.contains(&e.payload["to"].as_str().unwrap_or(""))
        {
            let op = e.payload["operation_id"].as_str().unwrap_or("").to_string();
            *per_op.entry(op).or_insert(0) += 1;
        }
    }
    let multi: Vec<&String> = per_op
        .iter()
        .filter(|(_, c)| **c > 1)
        .map(|(o, _)| o)
        .collect();
    Check {
        check_id: "inv.single_terminal",
        verdict: if multi.is_empty() { "pass" } else { "fail" },
        evidence: format!("ops={} multi_terminal={}", per_op.len(), multi.len()),
    }
}

fn check_side_effect_receipts(
    store: &dyn EventStore,
    events: &[bm_contract::events::EventEnvelope],
) -> Result<Check, JudgeError> {
    use std::collections::HashMap;
    // op → (intent 数, 完成数 ok/error/suppressed)
    let mut intents: HashMap<String, u32> = HashMap::new();
    let mut settled: HashMap<String, bool> = HashMap::new();
    for e in events {
        if e.event_type.as_str() != "capability.invoked" {
            continue;
        }
        let op = e.payload["operation_id"].as_str().unwrap_or("").to_string();
        match e.payload["outcome"].as_str().unwrap_or("") {
            "intent" => *intents.entry(op).or_insert(0) += 1,
            "ok" | "error" | "suppressed" => {
                settled.insert(op, true);
            }
            _ => {}
        }
    }
    // outbox 对账行也算完成证据(崩溃/超时窗口的对账底座)
    let outbox_published = store
        .list_outbox_by_state("published")
        .map_err(|e| JudgeError::Store(e.to_string()))?;
    let mut unmatched = 0u32;
    for op in intents.keys() {
        let has_outbox = outbox_published.iter().any(|r| {
            r.get("operation_id").and_then(|v| v.as_str()) == Some(op.as_str())
                || r["payload"]
                    .as_str()
                    .map(|p| p.contains(op.as_str()))
                    .unwrap_or(false)
        });
        if !settled.get(op).copied().unwrap_or(false) && !has_outbox {
            unmatched += 1;
        }
    }
    Ok(Check {
        check_id: "receipt.side_effect",
        verdict: if unmatched == 0 { "pass" } else { "fail" },
        evidence: format!("intents={} unmatched={}", intents.len(), unmatched),
    })
}

fn check_latency(events: &[bm_contract::events::EventEnvelope]) -> Check {
    // 2026-09-05 回看修复:延迟门与回合超时同源(BOEN_TURN_TIMEOUT_SECS,
    // 默认 120s)——原固定 30s 会把「合法但慢」的回合误判 fail
    let gate_ms: u64 = std::env::var("BOEN_TURN_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(120)
        .saturating_mul(1000);
    let mut lat: Vec<u64> = events
        .iter()
        .filter(|e| e.event_type.as_str() == "model.invocation.completed")
        .filter_map(|e| e.payload["latency_ms"].as_u64())
        .collect();
    if lat.is_empty() {
        return Check {
            check_id: "latency.bucket",
            verdict: "skipped",
            evidence: "区间内无模型调用".into(),
        };
    }
    lat.sort_unstable();
    let max = lat[lat.len() - 1];
    let p50 = lat[lat.len() / 2];
    let ok = max <= gate_ms;
    Check {
        check_id: "latency.bucket",
        verdict: if ok { "pass" } else { "fail" },
        evidence: format!(
            "n={} p50={}ms max={}ms(门 {}ms)",
            lat.len(),
            p50,
            max,
            gate_ms
        ),
    }
}
