//! INV 断言库:每条不变量一个可复用的检查器 + 独立命名的测试入口在 tests/。

use bm_contract::events::{EventEnvelope, EventType};
use bm_contract::states::OperationState;
use bm_contract::wire::Receipt;

/// INV-3:seq 严格递增且连续(1..n 无空洞);INV-2:每条 operation.state.changed
/// 的 (from,to) 是迁移表中的边;INV-8:会话/Agent 创建事件先于该会话的回合事件。
pub fn assert_event_stream_wellformed(events: &[EventEnvelope]) {
    assert!(!events.is_empty(), "事件流为空");
    for (i, e) in events.iter().enumerate() {
        assert_eq!(
            e.event_seq,
            (i + 1) as u64,
            "INV-3:seq 连续性破坏 @{}",
            i + 1
        );
    }

    for e in events {
        if e.event_type == EventType::OperationStateChanged {
            let from = e.payload["from"].as_str().expect("from 是字符串");
            let to = e.payload["to"].as_str().expect("to 是字符串");
            let (from, to) = (
                OperationState::from_wire(from).expect("合法 from"),
                OperationState::from_wire(to).expect("合法 to"),
            );
            assert!(
                OperationState::can_transition(from, to),
                "INV-2:表外迁移 {from} -> {to}"
            );
        }
    }

    // INV-8:因果序——同会话内 session.created/agent.created 必须先于首个回合事件。
    let by_session: std::collections::HashMap<&str, Vec<&EventEnvelope>> = events
        .iter()
        .filter_map(|e| e.session_id.as_ref().map(|s| (s.as_str(), e)))
        .fold(Default::default(), |mut m, (s, e)| {
            m.entry(s).or_default().push(e);
            m
        });
    for (sess, evs) in &by_session {
        let created_seq = evs
            .iter()
            .find(|e| e.event_type == EventType::SessionCreated)
            .map(|e| e.event_seq);
        let turn_seq = evs
            .iter()
            .find(|e| e.event_type == EventType::AgentTurnStarted)
            .map(|e| e.event_seq);
        if let (Some(c), Some(t)) = (created_seq, turn_seq) {
            assert!(
                c < t,
                "INV-8:session.created({c}) 必须先于首个回合事件({t}), 会话 {sess}"
            );
        }
    }
}

/// INV-1(半句):同一 operation 在事件流中恰好出现一次终态迁移。
pub fn assert_single_terminal(events: &[EventEnvelope], operation_id: &str) {
    let terminals: Vec<&str> = events
        .iter()
        .filter(|e| e.event_type == EventType::OperationStateChanged)
        .filter(|e| e.payload["operation_id"].as_str() == Some(operation_id))
        .filter(|e| {
            OperationState::from_wire(e.payload["to"].as_str().unwrap_or(""))
                .map(|s| s.is_terminal())
                .unwrap_or(false)
        })
        .map(|e| e.payload["to"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        terminals.len(),
        1,
        "INV-1:operation {operation_id} 的终态迁移恰好一次,实际 {terminals:?}"
    );
}

/// INV-9:收据终态幂等——给定同一 operation 的多次收据,全部一致。
pub fn assert_receipts_idempotent(receipts: &[Receipt]) {
    assert!(receipts.len() >= 2, "至少两次查询");
    for r in receipts.iter().skip(1) {
        assert_eq!(r, &receipts[0], "INV-9:终态后收据必须一致");
    }
}

/// INV-5:泄漏扫描——对事件、收据、Execution Log 全文统计凭据明文命中数。
pub fn leak_scan(
    events: &[EventEnvelope],
    receipts: &[Receipt],
    exec_log_text: Option<&str>,
    secret_values: &[String],
) -> usize {
    let mut corpus = String::new();
    for e in events {
        corpus.push_str(&serde_json::to_string(e).expect("事件可序列化"));
    }
    for r in receipts {
        corpus.push_str(&serde_json::to_string(r).expect("收据可序列化"));
    }
    if let Some(t) = exec_log_text {
        corpus.push_str(t);
    }
    secret_values
        .iter()
        .filter(|s| corpus.contains(s.as_str()))
        .count()
}

/// 从执行日志文件读全文(UTF-8,逐行 JSONL)。
pub fn read_exec_log(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).expect("Execution Log 可读")
}
