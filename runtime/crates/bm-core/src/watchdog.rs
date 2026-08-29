//! Watchdog 与长期监护(M5.6,基线 §20;ADR-0004 条件 6)。
//!
//! 监护层「仅监督,不推断编排下一步」(M2 语义延续):扫描产出**事实事件**
//! 与监护观测,从不产生命令形状(G4 守护,常驻 CI)。编排重启的触发者之
//! 二 = Watchdog 自动触发——停滞判定成立后发布
//! watchdog.reorchestration.triggered 事实事件,由编排器消费后自行从最近
//! 一致的持久状态重新推理;Runtime 监督层不自行动步。
//!
//! 合同默认值(Task 级可配置随 M6;数值大白话见 PENDING D-M5-1):
//! stalled_after = 15 分钟 / stall_hard_limit = 24 小时 /
//! watchdog_tick = 60 秒 / repeat_threshold = 3 次。
//! waiting_approval 豁免:等的是人,不是机器——成员调用停在审批时刷新
//! 进度信号(不判停滞)。

use bm_contract::ids::BmId;
use chrono::DateTime;
use std::collections::HashMap;

pub const STALL_AFTER_MS: i64 = 15 * 60 * 1000;
pub const STALL_HARD_LIMIT_MS: i64 = 24 * 60 * 60 * 1000;
pub const WATCHDOG_TICK_MS: i64 = 60 * 1000;
pub const REPEAT_THRESHOLD: u32 = 3;

/// 单 Task 监护状态。
#[derive(Debug, Clone)]
pub struct TaskWatch {
    pub last_progress_at: DateTime<chrono::Utc>,
    pub last_progress_seq: u64,
    /// 最近一次成员调用签名(capability + args + outcome)的哈希。
    pub last_sig: Option<u64>,
    pub repeat_count: u32,
    /// 本次停滞episode是否已通告(进度刷新后复位)。
    pub stall_notified: bool,
    /// waiting_approval 豁免:成员调用停在审批(等人)时不判停滞/硬顶,
    /// 直到下一次非审批结果复位(基线 §20;ADR-0004 条件 6)。
    pub waiting_approval: bool,
}

/// 监护状态(Task → Watch)。
#[derive(Debug, Default)]
pub struct WatchdogState {
    pub watches: HashMap<String, TaskWatch>,
    pub next_scan_at: Option<DateTime<chrono::Utc>>,
}

/// 扫描判定(事实产出,不做状态变更——变更由运行时执行)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanDecision {
    /// 停滞超阈值:发 task.stalled + reorchestration 事实事件(每episode一次)。
    Stall,
    /// 累计超硬顶:Task 转 blocked(stall_hard_limit),不再自动重启。
    HardLimit,
}

impl WatchdogState {
    /// 进度信号:任何任务相关事实(状态迁移/成员/成功的成员调用)刷新。
    pub fn mark_progress(&mut self, task_id: &str, now: DateTime<chrono::Utc>, seq: u64) {
        let w = self
            .watches
            .entry(task_id.to_string())
            .or_insert(TaskWatch {
                last_progress_at: now,
                last_progress_seq: seq,
                last_sig: None,
                repeat_count: 0,
                stall_notified: false,
                waiting_approval: false,
            });
        w.last_progress_at = now;
        w.last_progress_seq = w.last_progress_seq.max(seq);
        w.stall_notified = false;
    }

    /// 成员调用签名记账:返回本次的连续重复次数(同 capability+args+outcome)。
    pub fn note_call(
        &mut self,
        task_id: &str,
        sig: u64,
        now: DateTime<chrono::Utc>,
        seq: u64,
    ) -> u32 {
        let w = self
            .watches
            .entry(task_id.to_string())
            .or_insert(TaskWatch {
                last_progress_at: now,
                last_progress_seq: seq,
                last_sig: None,
                repeat_count: 0,
                stall_notified: false,
                waiting_approval: false,
            });
        w.last_progress_at = now;
        w.last_progress_seq = w.last_progress_seq.max(seq);
        w.stall_notified = false;
        w.waiting_approval = false;
        if w.last_sig == Some(sig) {
            w.repeat_count += 1;
        } else {
            w.repeat_count = 1;
            w.last_sig = Some(sig);
        }
        w.repeat_count
    }

    /// 单 Task 扫描判定(仅 Running 态任务由调用方喂入)。
    /// 首次扫描即建档(通告标记需要稳定的状态载体)。
    pub fn decide(
        &mut self,
        task_id: &str,
        created_at: DateTime<chrono::Utc>,
        now: DateTime<chrono::Utc>,
    ) -> Option<ScanDecision> {
        let w = self
            .watches
            .entry(task_id.to_string())
            .or_insert(TaskWatch {
                last_progress_at: created_at,
                last_progress_seq: 0,
                last_sig: None,
                repeat_count: 0,
                stall_notified: false,
                waiting_approval: false,
            });
        // waiting_approval 豁免:等的是人,不是机器
        if w.waiting_approval {
            return None;
        }
        let elapsed_ms = (now - w.last_progress_at).num_milliseconds();
        if elapsed_ms > STALL_HARD_LIMIT_MS {
            return Some(ScanDecision::HardLimit);
        }
        if elapsed_ms > STALL_AFTER_MS && !w.stall_notified {
            return Some(ScanDecision::Stall);
        }
        None
    }

    /// 成员调用停在审批:置豁免位(等人不算停滞;下一次非审批结果复位)。
    pub fn mark_waiting(&mut self, task_id: &str, now: DateTime<chrono::Utc>, seq: u64) {
        let w = self
            .watches
            .entry(task_id.to_string())
            .or_insert(TaskWatch {
                last_progress_at: now,
                last_progress_seq: seq,
                last_sig: None,
                repeat_count: 0,
                stall_notified: false,
                waiting_approval: true,
            });
        w.last_progress_at = now;
        w.last_progress_seq = w.last_progress_seq.max(seq);
        w.stall_notified = false;
        w.waiting_approval = true;
    }

    /// 停滞已通告标记(事实事件发出后)。
    pub fn mark_stall_notified(&mut self, task_id: &str) {
        if let Some(w) = self.watches.get_mut(task_id) {
            w.stall_notified = true;
        }
    }

    /// 是否到达扫描时刻。
    pub fn due(&self, now: DateTime<chrono::Utc>) -> bool {
        match self.next_scan_at {
            Some(t) => now >= t,
            None => false,
        }
    }

    pub fn schedule_next(&mut self, now: DateTime<chrono::Utc>) {
        use chrono::Duration;
        self.next_scan_at = Some(now + Duration::milliseconds(WATCHDOG_TICK_MS));
    }

    /// 任务移除(终态清场)。
    pub fn forget(&mut self, task_id: &str) {
        self.watches.remove(task_id);
    }
}

/// G4 守护(常驻 CI 的结构断言):Watchdog 产出的载荷键恒为事实面——
/// 不出现命令语义形状(与 runtime validate_event_shape 的禁字清单同源)。
pub const WATCHDOG_EVENT_KEYS: [&str; 8] = [
    "task_id",
    "stalled_ms",
    "last_progress_seq",
    "agent_id",
    "capability",
    "repeat_count",
    "trigger",
    "reason",
];

pub fn is_fact_shaped(payload: &serde_json::Value) -> bool {
    const FORBIDDEN: [&str; 4] = [
        "requested_action",
        "instruction",
        "command",
        "please_execute",
    ];
    let Some(obj) = payload.as_object() else {
        return false;
    };
    obj.keys().all(|k| !FORBIDDEN.contains(&k.as_str()))
}

/// 生成成员调用签名(重复检测用;非加密哈希,仅判等)。
pub fn call_sig(capability: &str, args: &serde_json::Value, outcome: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    capability.hash(&mut h);
    args.to_string().hash(&mut h);
    outcome.hash(&mut h);
    h.finish()
}

/// 供测试与运行时共用的安全时间解析(失败按 created_at 兜底)。
pub fn parse_or(ts: &str, fallback: DateTime<chrono::Utc>) -> DateTime<chrono::Utc> {
    bm_contract::timestamp::parse_ts(ts).unwrap_or(fallback)
}

/// 观测:任务 id 引用(避免 runtime 之外的 BmId 构造)。
pub fn task_ref(id: &BmId) -> &str {
    id.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{Clock, MockClock};

    const BASE_MS: u128 = 1_788_000_000_000;

    #[test]
    fn stall_and_hard_limit_decisions_follow_windows() {
        let clock = MockClock::at_ms(BASE_MS);
        let mut wd = WatchdogState::default();
        let t0 = clock.now();
        // 无进度:15 分钟内不判停滞
        assert_eq!(
            wd.decide("t1", t0, t0 + chrono::Duration::minutes(14)),
            None
        );
        // 超 15 分钟 → Stall(首次)
        assert_eq!(
            wd.decide("t1", t0, t0 + chrono::Duration::minutes(16)),
            Some(ScanDecision::Stall)
        );
        // 通告后不再重复(直到进度刷新)
        wd.mark_stall_notified("t1");
        assert_eq!(
            wd.decide("t1", t0, t0 + chrono::Duration::minutes(30)),
            None,
            "同episode不重复通告"
        );
        // 超 24 小时 → HardLimit(硬顶:不再自动重启,转 blocked)
        assert_eq!(
            wd.decide("t1", t0, t0 + chrono::Duration::hours(25)),
            Some(ScanDecision::HardLimit)
        );
    }

    #[test]
    fn progress_refresh_resets_stall_episode() {
        let clock = MockClock::at_ms(BASE_MS);
        let mut wd = WatchdogState::default();
        let t0 = clock.now();
        wd.mark_progress("t1", t0, 5);
        let t1 = t0 + chrono::Duration::minutes(20);
        assert_eq!(wd.decide("t1", t0, t1), Some(ScanDecision::Stall));
        wd.mark_stall_notified("t1");
        // 进度刷新:episode 复位
        wd.mark_progress("t1", t0 + chrono::Duration::minutes(25), 9);
        assert_eq!(
            wd.decide("t1", t0, t0 + chrono::Duration::minutes(30)),
            None,
            "刷新后重新计时"
        );
    }

    #[test]
    fn repeat_count_accumulates_on_same_signature() {
        let clock = MockClock::at_ms(BASE_MS);
        let mut wd = WatchdogState::default();
        let now = clock.now();
        assert_eq!(wd.note_call("t1", 42, now, 1), 1);
        assert_eq!(wd.note_call("t1", 42, now, 2), 2);
        assert_eq!(wd.note_call("t1", 42, now, 3), 3, "达到 repeat_threshold");
        assert_eq!(wd.note_call("t1", 43, now, 4), 1, "签名变化即复位");
        assert_eq!(wd.note_call("t1", 42, now, 5), 1, "回到旧签名重新计数");
    }

    #[test]
    fn watchdog_payloads_are_fact_shaped() {
        assert!(is_fact_shaped(
            &serde_json::json!({"task_id": "task_x", "trigger": "watchdog"})
        ));
        assert!(!is_fact_shaped(
            &serde_json::json!({"requested_action": "task.cancel"})
        ));
        assert!(!is_fact_shaped(
            &serde_json::json!({"instruction": "spawn"})
        ));
        assert_eq!(WATCHDOG_EVENT_KEYS.len(), 8);
    }

    #[test]
    fn tick_scheduling_is_monotonic() {
        let clock = MockClock::at_ms(BASE_MS);
        let now = clock.now();
        let mut wd = WatchdogState::default();
        assert!(!wd.due(now), "未排程不触发");
        wd.schedule_next(now);
        assert!(!wd.due(now + chrono::Duration::milliseconds(WATCHDOG_TICK_MS - 1)));
        assert!(wd.due(now + chrono::Duration::milliseconds(WATCHDOG_TICK_MS)));
    }
}
