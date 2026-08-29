//! Coordinator(M5.3/M5.4,基线 §11;ADR-0002 要点 2/3、条件 2 余项)。
//!
//! Coordinator 为受限 Agent 类型:权限 = Butler 可授协调权 ∩ Task 授权 ∩
//! 用户授权,默认拒绝、Task 结束即失效。三方交集在 Task 创建时**物化**为
//! task:<id> 作用域的 Grant(M4 预留枚举自此启用):
//! - Coordinator 自身的协调动词 Grant:parent 链回溯到 Butler 的 bootstrap
//!   Grant(上界不得超出);
//! - Worker 的能力 Grant:仅从 Task 授权中 capability.call 条目的资源谓词
//!   签发,parent 链回溯到 Coordinator 自身的 capability.call Grant,
//!   delegation_depth 恒 0(不可再转授)。
//!
//! 子树边界(M5 单 Task 演示命名空间):principal 采用固定
//! agent:coordinator / agent:worker;Task 终态时其 task:<id> Grant 全量
//! 撤销(「Task 结束即失效」的强制点);多 Team/多 Task 命名空间隔离随
//! M6 升级为 per-task principal(记录于 M5 规格 §8 解读条款)。

use bm_contract::capability::{Grant, GrantResource, GrantScope};
use bm_contract::ids::IdGen;
use bm_contract::timestamp::format_ts;
use bm_contract::wire::TaskAuthorizationEntry;
use chrono::DateTime;
use sha2::{Digest, Sha256};

/// Coordinator 的 M5 演示命名空间 principal(多 Team 隔离随 M6)。
pub const COORDINATOR_PRINCIPAL: &str = "agent:coordinator";
/// Worker 的 M5 演示命名空间 principal(GT-03 场景 A 同款)。
pub const WORKER_PRINCIPAL: &str = "agent:worker";

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let out = h.finalize();
    let mut hex = String::with_capacity(64);
    for b in out {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

/// 签发一个 task:<id> 作用域 Grant(delegation_depth 恒 0)。
/// #[allow]:参数与 Grant 冻结字段集一一对应,压缩反损可读性(emit 同款先例)。
#[allow(clippy::too_many_arguments)]
fn task_grant(
    ids: &dyn IdGen,
    task_id: &str,
    audience: &str,
    action: &str,
    resource: GrantResource,
    parent_hash: String,
    issued_by: &str,
    now: DateTime<chrono::Utc>,
) -> Grant {
    Grant {
        grant_id: ids.next_id("grant").to_string(),
        audience: audience.to_string(),
        action: action.to_string(),
        resource,
        scope: GrantScope::Task(task_id.to_string()),
        delegation_depth: 0,
        expires_at: None,
        revocation_version: 0,
        parent_grant_hash: parent_hash,
        issued_by: issued_by.to_string(),
        created_at: format_ts(now),
    }
}

/// 三方交集物化(Task 创建时执行一次):
/// 返回 (Coordinator 协调动词 Grant 集, Worker 能力 Grant 集)。
///
/// - Coordinator Grant:每个授权条目一枚,action = 动词,parent 哈希 =
///   Butler 同动词 bootstrap Grant(parent 查证返回 None = 上界已撤销,
///   该条目跳过——撤销后新建 Task 的 Coordinator 拿不到对应协调权);
/// - Worker Grant:仅 capability.call 条目按资源谓词逐枚签发,action =
///   谓词能力名,parent 哈希 = Coordinator 自身 capability.call Grant
///   的内容 SHA-256(授权链可上溯,逐级不超上界)。
///   无 capability.call 条目 = Worker 不获任何能力授权(默认拒绝上界)。
pub fn intersection_grants(
    ids: &dyn IdGen,
    task_id: &str,
    authorization: &[TaskAuthorizationEntry],
    now: DateTime<chrono::Utc>,
    mut butler_verb_grant: impl FnMut(&str) -> Option<Grant>,
) -> (Vec<Grant>, Vec<Grant>) {
    let mut coordinator_grants = Vec::new();
    let mut worker_grants = Vec::new();
    for entry in authorization {
        let Some(parent) = butler_verb_grant(&entry.verb) else {
            continue; // 上界缺失(Butler 撤销):该条目不物化
        };
        let resources: Vec<GrantResource> = match &entry.resources {
            Some(rs) => rs
                .as_array()
                .filter(|a| !a.is_empty())
                .map(|a| {
                    a.iter()
                        .map(|r| GrantResource {
                            capability: r["capability"].as_str().unwrap_or(&entry.verb).to_string(),
                            args_predicates: r["args_eq"].as_object().cloned().unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_else(|| {
                    vec![GrantResource {
                        capability: entry.verb.clone(),
                        args_predicates: Default::default(),
                    }]
                }),
            None => vec![GrantResource {
                capability: entry.verb.clone(),
                args_predicates: Default::default(),
            }],
        };
        for r in &resources {
            let g = task_grant(
                ids,
                task_id,
                COORDINATOR_PRINCIPAL,
                &entry.verb,
                r.clone(),
                sha256_hex(&serde_json::to_string(&parent).unwrap_or_default()),
                crate::butler::BUTLER_PRINCIPAL,
                now,
            );
            coordinator_grants.push(g);
        }
        // Worker 能力 Grant(仅 capability.call 谓词;父 = Coordinator 的
        // capability.call Grant 内容哈希)
        if entry.verb == "capability.call"
            && let Some(parent_call) = coordinator_grants
                .iter()
                .find(|g| g.action == "capability.call")
                .cloned()
        {
            for r in resources {
                let capability = r.capability.clone();
                let wg = task_grant(
                    ids,
                    task_id,
                    WORKER_PRINCIPAL,
                    &capability,
                    r,
                    sha256_hex(&serde_json::to_string(&parent_call).unwrap_or_default()),
                    COORDINATOR_PRINCIPAL,
                    now,
                );
                worker_grants.push(wg);
            }
        }
    }
    (coordinator_grants, worker_grants)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::butler::bootstrap_grant;
    use crate::clock::{Clock, MockClock};
    use bm_contract::ids::SeqIdGen;
    use serde_json::json;

    const BASE_MS: u128 = 1_788_000_000_000;

    fn auth(entries: serde_json::Value) -> Vec<TaskAuthorizationEntry> {
        serde_json::from_value(entries).unwrap()
    }

    fn butler_lookup(verb: &str) -> Option<Grant> {
        let ids = SeqIdGen::new();
        let clock = MockClock::at_ms(BASE_MS);
        Some(bootstrap_grant(&ids, verb, clock.now()))
    }

    #[test]
    fn intersection_materializes_task_scoped_grants_with_parent_chain() {
        let ids = SeqIdGen::new();
        let clock = MockClock::at_ms(BASE_MS);
        let authorization = auth(json!([
            {"verb": "task.collect", "klass": "safe"},
            {"verb": "agent.spawn", "klass": "mutation"},
            {"verb": "capability.call", "klass": "mutation",
             "resources": [{"capability": "system.notes.write"}]}
        ]));
        let (coord, worker) = intersection_grants(
            &ids,
            "task_01JAAAAAAAAAAAAAAAAAAAAAB2",
            &authorization,
            clock.now(),
            butler_lookup,
        );
        // Coordinator:3 枚(每条目一枚)task scope Grant
        assert_eq!(coord.len(), 3);
        assert!(coord.iter().all(|g| g.audience == COORDINATOR_PRINCIPAL
            && matches!(g.scope, GrantScope::Task(_))
            && g.issued_by == crate::butler::BUTLER_PRINCIPAL
            && g.delegation_depth == 0));
        // parent 哈希 = Butler bootstrap Grant 内容哈希(链可上溯)
        let spawn = coord.iter().find(|g| g.action == "agent.spawn").unwrap();
        let butler_spawn = butler_lookup("agent.spawn").unwrap();
        assert_eq!(
            spawn.parent_grant_hash,
            sha256_hex(&serde_json::to_string(&butler_spawn).unwrap())
        );
        // Worker:恰 1 枚,action = 谓词能力,parent = Coordinator 的
        // capability.call Grant 内容哈希
        assert_eq!(worker.len(), 1);
        assert_eq!(worker[0].audience, WORKER_PRINCIPAL);
        assert_eq!(worker[0].action, "system.notes.write");
        let parent_call = coord
            .iter()
            .find(|g| g.action == "capability.call")
            .unwrap();
        assert_eq!(
            worker[0].parent_grant_hash,
            sha256_hex(&serde_json::to_string(&parent_call).unwrap())
        );
        assert_eq!(worker[0].issued_by, COORDINATOR_PRINCIPAL);
    }

    #[test]
    fn no_capability_call_entry_means_worker_gets_nothing() {
        let ids = SeqIdGen::new();
        let clock = MockClock::at_ms(BASE_MS);
        let authorization = auth(json!([
            {"verb": "task.collect", "klass": "safe"}
        ]));
        let (coord, worker) =
            intersection_grants(&ids, "task_x", &authorization, clock.now(), butler_lookup);
        assert_eq!(coord.len(), 1);
        assert!(worker.is_empty(), "无 capability.call 授权 = Worker 零授权");
    }

    #[test]
    fn revoked_butler_upper_bound_skips_entry() {
        let ids = SeqIdGen::new();
        let clock = MockClock::at_ms(BASE_MS);
        let authorization = auth(json!([
            {"verb": "task.collect", "klass": "safe"},
            {"verb": "agent.spawn", "klass": "mutation"}
        ]));
        // agent.spawn 的上界已撤销(Butler 查证返回 None)
        let (coord, worker) =
            intersection_grants(&ids, "task_x", &authorization, clock.now(), |verb| {
                if verb == "agent.spawn" {
                    None
                } else {
                    butler_lookup(verb)
                }
            });
        assert_eq!(coord.len(), 1, "上界已撤销的动词不物化");
        assert_eq!(coord[0].action, "task.collect");
        assert!(worker.is_empty());
    }
}
