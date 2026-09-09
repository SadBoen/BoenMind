//! Team 编队与委派(M6,基线 §11;M6 规格 §5)。
//!
//! 机制进内核、策略留外围(§2.3 六问):成员关系/委派深度/并发/预算子分配
//! 是 L2 规范约束(强制点在本模块 + spawn 命令),编队策略是 Coordinator
//! 的可替换行为。Team 不设独立合同对象——Task.members + 子任务树即 Team
//! (规格 §8-1)。委派 = 子任务:Grant delegation_depth 恒 0 语义不变
//! (转授禁止的是能力授权),深度上限约束的是任务分解层级。
//!
//! per-task principal(M5 遗留承接):跨 Task 访问在 Grant 查表层结构性
//! 不命中——子树裁剪从「构造性」升级为「结构性」。

use bm_contract::budget::{Budget, ExtraValue};
use bm_contract::wire::TaskAuthorizationEntry;

/// 委派深度上限(根 Task = 0;M6.5 合同默认,Task 级配置随 M7)。
pub const MAX_DELEGATION_DEPTH: u64 = 3;
/// 单 Task 存活 Worker 并发上限(M6.5 合同默认)。
pub const MAX_CONCURRENT_WORKERS: u64 = 5;

/// Coordinator 的 per-task principal(跨 Task 结构性隔离)。
pub fn coord_principal(task_id: &str) -> String {
    format!("agent:coord:{task_id}")
}

/// Worker 的 per-task principal。
pub fn worker_principal(task_id: &str) -> String {
    format!("agent:worker:{task_id}")
}

/// 读取预算的 max_tool_calls(开放键值;None = 不限)。
pub fn max_tool_calls_of(budget: Option<&Budget>) -> Option<u64> {
    budget
        .and_then(|b| b.extra.get("max_tool_calls"))
        .and_then(|v| match v {
            ExtraValue::Int(n) => u64::try_from(*n).ok(),
            ExtraValue::Float(f) => u64::try_from(*f as i64).ok(),
            _ => None,
        })
}

/// 读取预算开放键的通用整数投影(#30;非正数/类型不符 = None,按未配置
/// 处理——非法值不放大限制也不放开门禁,回退全局默认)。
fn extra_positive_int_of(budget: Option<&Budget>, key: &str) -> Option<i64> {
    budget
        .and_then(|b| b.extra.get(key))
        .and_then(|v| match v {
            ExtraValue::Int(n) => Some(*n),
            ExtraValue::Float(f) => Some(*f as i64),
            _ => None,
        })
        .filter(|n| *n > 0)
}

/// Task 级停滞窗口覆盖(#30;None = 用全局 limits/合同默认 15min)。
pub fn stall_after_ms_of(budget: Option<&Budget>) -> Option<i64> {
    extra_positive_int_of(budget, "stall_after_ms")
}

/// Task 级停滞硬顶覆盖(#30;None = 用全局 limits/合同默认 24h)。
pub fn stall_hard_limit_ms_of(budget: Option<&Budget>) -> Option<i64> {
    extra_positive_int_of(budget, "stall_hard_limit_ms")
}

/// Task 级存活 Worker 并发上限覆盖(#30;None = 用合同默认 MAX_CONCURRENT_WORKERS=5)。
pub fn max_concurrent_workers_of(budget: Option<&Budget>) -> Option<u64> {
    extra_positive_int_of(budget, "max_concurrent_workers").map(|v| v as u64)
}

/// 授权子集校验(M6.5「成员权限只减不增」):child 的动词集 ⊆ parent,
/// 且同动词下 child 的资源谓词能力集 ⊆ parent 的能力集。
/// - child 不得引入 parent 没有的动词(含 safe:默认继承的查询面同样取子集);
/// - 资源谓词为空 = 全参(宽于任何具体谓词):child 谓词为空而 parent 具体
///   时视为放宽,拒绝;child 具体而 parent 为空 = 收窄,允许(能力名仍须 ⊆)。
pub fn authorization_subset(
    child: &[TaskAuthorizationEntry],
    parent: &[TaskAuthorizationEntry],
) -> bool {
    let parent_verbs: std::collections::BTreeSet<&str> =
        parent.iter().map(|e| e.verb.as_str()).collect();
    // child 每个动词必须在 parent 中,且能力资源 ⊆
    for c in child {
        if !parent_verbs.contains(c.verb.as_str()) {
            return false;
        }
        let caps = |e: &TaskAuthorizationEntry| -> Option<std::collections::BTreeSet<String>> {
            e.resources.as_ref().and_then(|r| r.as_array()).map(|a| {
                a.iter()
                    .filter_map(|x| x["capability"].as_str().map(|s| s.to_string()))
                    .collect()
            })
        };
        match (
            caps(c),
            parent.iter().find(|p| p.verb == c.verb).and_then(caps),
        ) {
            (Some(cc), Some(pc)) => {
                if !cc.is_subset(&pc) {
                    return false;
                }
            }
            // child 谓词为空 = 动词级全参,parent 具体 → 视为放宽,拒绝
            // (2026-09-05 回看收紧:与注释/M6「只减不增」承诺对齐,原实现
            //  此臂放行 = 子任务可凭空谓词越出父授权的谓词集)
            (None, Some(_)) => return false,
            // child 具体而 parent 动词级全参 = 收窄,允许
            (Some(_), None) => {}
            // 双方均动词级全参 = 同参,允许
            (None, None) => {}
        }
    }
    // parent 的 mutation 动词可不在 child 中(只减不增);safe 同理
    true
}

/// 委派深度门禁。
pub fn depth_ok(parent_depth: u64) -> bool {
    parent_depth < MAX_DELEGATION_DEPTH
}

/// 预算子分配门禁:child.max_tool_calls ≤ parent 剩余(不做 reservation,
/// spawn 时点校验;M6 规格 §9)。
pub fn budget_ok(child_max: Option<u64>, parent_max: Option<u64>, parent_used: u64) -> bool {
    match (child_max, parent_max) {
        // 子不限而父有限 = 事实上的无限子分配,拒绝
        (None, Some(_)) => false,
        (Some(c), Some(p)) => c + parent_used <= p,
        // 父不限(或双方均不限)= 放行
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn auth(v: serde_json::Value) -> Vec<TaskAuthorizationEntry> {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn per_task_principals_are_distinct_across_tasks() {
        let a = coord_principal("task_A");
        let b = coord_principal("task_B");
        assert_ne!(a, b);
        assert!(a.starts_with("agent:coord:task_"));
        assert!(worker_principal("task_A").starts_with("agent:worker:task_"));
    }

    #[test]
    fn authorization_subset_enforces_only_reduce() {
        let parent = auth(json!([
            {"verb": "capability.call", "klass": "mutation",
             "resources": [{"capability": "system.notes.write"},
                           {"capability": "system.mail.mock_send"}]},
            {"verb": "agent.spawn", "klass": "mutation"}
        ]));
        // 真子集:动词在、能力收窄
        assert!(authorization_subset(
            &auth(json!([
                {"verb": "capability.call", "klass": "mutation",
                 "resources": [{"capability": "system.notes.write"}]}
            ])),
            &parent
        ));
        // 引入 parent 没有的动词 → 拒
        assert!(!authorization_subset(
            &auth(json!([{"verb": "team.create", "klass": "mutation"}])),
            &parent
        ));
        // 能力越出谓词集 → 拒
        assert!(!authorization_subset(
            &auth(json!([
                {"verb": "capability.call", "klass": "mutation",
                 "resources": [{"capability": "system.echo"}]}
            ])),
            &parent
        ));
        // 收窄为无资源(动词级,能力集空 ⊆)→ 允许
        assert!(authorization_subset(
            &auth(json!([{"verb": "agent.spawn", "klass": "mutation"}])),
            &parent
        ));
    }

    #[test]
    fn authorization_subset_rejects_predicate_widening() {
        // 2026-09-05 回看收紧:child 谓词为空(动词级全参)而 parent 具体
        // = 放宽,必须拒绝(与 41-45 行文档承诺对齐;原实现放行 = 越权面)
        let parent = auth(json!([
            {"verb": "capability.call", "klass": "mutation",
             "resources": [{"capability": "system.notes.write"}]}
        ]));
        assert!(!authorization_subset(
            &auth(json!([{"verb": "capability.call", "klass": "mutation"}])),
            &parent
        ));
        // 反向:child 具体、parent 动词级全参 = 收窄,允许
        let broad_parent = auth(json!([
            {"verb": "capability.call", "klass": "mutation"}
        ]));
        assert!(authorization_subset(
            &auth(json!([
                {"verb": "capability.call", "klass": "mutation",
                 "resources": [{"capability": "system.notes.write"}]}
            ])),
            &broad_parent
        ));
        // 双方均动词级 = 同参,允许
        assert!(authorization_subset(
            &auth(json!([{"verb": "capability.call", "klass": "mutation"}])),
            &broad_parent
        ));
    }

    #[test]
    fn depth_gate_allows_three_levels() {
        assert!(depth_ok(0), "根 → 子(深度1)");
        assert!(depth_ok(1));
        assert!(depth_ok(2), "孙(深度3)恰达上限");
        assert!(!depth_ok(3), "曾孙(深度4)超限");
    }

    fn budget_with(extra: serde_json::Value) -> Option<Budget> {
        let mut b = Budget {
            max_tokens: 1000,
            max_turns: 10,
            extra: Default::default(),
        };
        let m = extra.as_object().expect("obj");
        for (k, v) in m {
            b.extra.insert(
                k.clone(),
                serde_json::from_value(v.clone()).expect("ExtraValue"),
            );
        }
        Some(b)
    }

    #[test]
    fn task_level_override_keys_follow_budget_extra() {
        // #30:三个开放键的读取与非法值回退
        let b = budget_with(json!({
            "stall_after_ms": 60_000,
            "stall_hard_limit_ms": 3_600_000,
            "max_concurrent_workers": 2
        }));
        assert_eq!(stall_after_ms_of(b.as_ref()), Some(60_000));
        assert_eq!(stall_hard_limit_ms_of(b.as_ref()), Some(3_600_000));
        assert_eq!(max_concurrent_workers_of(b.as_ref()), Some(2));
        // 未配置 = None(回退全局)
        assert_eq!(stall_after_ms_of(None), None);
        assert_eq!(max_concurrent_workers_of(None), None);
        // 非法值(0/负数/字符串)= 忽略回退
        let bad = budget_with(json!({"stall_after_ms": 0, "max_concurrent_workers": -3}));
        assert_eq!(stall_after_ms_of(bad.as_ref()), None);
        assert_eq!(max_concurrent_workers_of(bad.as_ref()), None);
        let junk = budget_with(json!({"max_concurrent_workers": "many"}));
        assert_eq!(max_concurrent_workers_of(junk.as_ref()), None);
    }

    #[test]
    fn budget_gate_requires_child_within_parent_remaining() {
        // 子 10 ≤ 父剩余 48-0
        assert!(budget_ok(Some(10), Some(48), 0));
        // 父已用 40,剩 8 < 子 10 → 拒
        assert!(!budget_ok(Some(10), Some(48), 40));
        // 子不限而父有限 → 拒(事实上的无限子分配)
        assert!(!budget_ok(None, Some(48), 0));
        // 父不限 → 放行
        assert!(budget_ok(Some(10), None, 0));
        assert!(budget_ok(None, None, 0));
    }
}
