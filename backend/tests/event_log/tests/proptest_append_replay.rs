//! proptest 承诺（实现方案 §6）：事件日志 append/replay 一致性属性测试。
//!
//! 对任意 append/append_batch/fork/clear 操作序列验证不变量：
//! 1. 重放两次字节一致（确定性——实现方案 §5-6 的核心承诺）；
//! 2. 每分支 replay 的 seq 严格递增（批内连续、跨操作不跳号不重复）；
//! 3. append/append_batch 返回的 seq 与 replay 尾部一致；
//! 4. clear 后分支从 seq 1 重新起。
//!
//! 用 InMemoryEventStore（事件流语义与 turso 实现同构；turso 路径的
//! 原子性由 checkpoint/ignorable/fork 集成测试覆盖）。

use std::sync::Arc;

use bm_kernel::{EventLog, InMemoryEventStore, SurfaceIntent};
use bm_protocol::{BranchId, CoreEvent, EventKind, SessionId};
use proptest::prelude::*;

fn turn(n: u32) -> EventKind {
    EventKind::Core(CoreEvent::TurnStart { turn: n })
}

/// 操作模型：单条 append / 批量 append（1..=8 条）/ fork / clear。
#[derive(Debug, Clone, Copy)]
enum Op {
    Append,
    AppendBatch(u8),
    Fork,
    Clear,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => Just(Op::Append),
        3 => (1u8..=8).prop_map(Op::AppendBatch),
        1 => Just(Op::Fork),
        1 => Just(Op::Clear),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(60))]

    #[test]
    fn arbitrary_operation_sequences_keep_replay_invariants(
        ops in prop::collection::vec(op_strategy(), 0..40),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let log = EventLog::new(Arc::new(InMemoryEventStore::new()));
            let sid = SessionId::new("sess_prop");
            let main = BranchId::new("main");
            // 模型状态：每分支期望的下一 seq + 已落事件数
            let mut branches: Vec<BranchId> = vec![main.clone()];
            let mut expected_len: std::collections::HashMap<String, u64> =
                std::collections::HashMap::new();
            let mut expected_next: std::collections::HashMap<String, u64> =
                std::collections::HashMap::new();
            expected_len.insert(main.to_string(), 0);
            expected_next.insert(main.to_string(), 1);
            let mut turn_no: u32 = 0;
            let mut round: usize = 0;

            for op in ops {
                // 轮转选分支（确定性伪随机；单一分支概率由 ops 生成器覆盖）
                let bid = branches[round % branches.len()].clone();
                round += 1;
                let key = bid.to_string();
                match op {
                    Op::Append => {
                        turn_no += 1;
                        let seq = log
                            .append(sid.clone(), bid.clone(), turn(turn_no), SurfaceIntent::None)
                            .await
                            .unwrap();
                        assert_eq!(seq.as_u64(), expected_next[&key]);
                        *expected_next.get_mut(&key).unwrap() += 1;
                        *expected_len.get_mut(&key).unwrap() += 1;
                    }
                    Op::AppendBatch(n) => {
                        let events: Vec<(EventKind, SurfaceIntent, bool, Option<Vec<bm_protocol::SeqNo>>)> =
                            (0..n)
                                .map(|_| {
                                    turn_no += 1;
                                    (turn(turn_no), SurfaceIntent::None, false, None)
                                })
                                .collect();
                        let seqs = log.append_batch(sid.clone(), bid.clone(), events).await.unwrap();
                        let start = expected_next[&key];
                        let want: Vec<bm_protocol::SeqNo> = (start..start + n as u64)
                            .map(bm_protocol::SeqNo::new)
                            .collect();
                        assert_eq!(seqs, want, "append_batch 返回的 seq 连续");
                        *expected_next.get_mut(&key).unwrap() += n as u64;
                        *expected_len.get_mut(&key).unwrap() += n as u64;
                    }
                    Op::Fork => {
                        // 设计约束（超头拒绝）：源分支须有头行——main 须先有
                        // 事件；fork 出来的空分支已有头行（head 0）可直接再 fork。
                        // 模型遵守该约束：不满足则跳过本 op。
                        let can_fork = expected_len[&key] > 0 || key != main.to_string();
                        if !can_fork {
                            continue;
                        }
                        let new = log.fork(&sid, &bid).await.unwrap();
                        branches.push(new.clone());
                        expected_next.insert(new.to_string(), 1);
                        expected_len.insert(new.to_string(), 0);
                    }
                    Op::Clear => {
                        let removed = log.clear_session(&sid).await.unwrap();
                        // 清除所有分支的事件
                        let total: u64 = expected_len.values().sum();
                        assert_eq!(removed, total);
                        for v in expected_len.values_mut() {
                            *v = 0;
                        }
                        for v in expected_next.values_mut() {
                            *v = 1;
                        }
                        branches = vec![main.clone()];
                    }
                }

                // 不变量 3：每分支 replay 与模型一致（严格递增、条数一致）
                for b in &branches {
                    let evs = log.replay(&sid, b).await.unwrap();
                    assert_eq!(evs.len() as u64, expected_len[&b.to_string()]);
                    for (i, ev) in evs.iter().enumerate() {
                        assert_eq!(ev.seq.as_u64(), i as u64 + 1, "分支内 seq 连续从 1 起");
                    }
                    // 不变量 1：重放两次字节一致
                    let again = log.replay(&sid, b).await.unwrap();
                    assert_eq!(
                        serde_json::to_string(&evs).unwrap(),
                        serde_json::to_string(&again).unwrap(),
                        "重放两次必须字节一致"
                    );
                }
            }
        });
    }
}
