//! M11/ADR-0031 批次 1:task.share.* 内核内联执行。
//!
//! 公告栏 = Task 投影(share.published 事件即事实),无外部副作用;Broker
//! 裁决/审计照常,占位 Provider 不触达(fs.*「注册占位、执行体内置」同款,
//! 唯执行点在单写者循环内)。task_id 一律自 principal 推导,args 不可指定。

use super::*;
use crate::share::{SHARE_LIST, SHARE_PUBLISH, task_id_of_principal};

/// 拦截点:dispatch_capability 在 Broker prepare 通过后转入本函数;
/// 发布/读取均为 Task 投影上的低风险内联操作,不走异步执行器。
pub(crate) fn dispatch_share(
    w: &mut World,
    ctx: &CallContext,
    capability: &str,
    args: serde_json::Value,
    op_id: &BmId,
    prepared: crate::broker::PreparedCall,
) -> CallOutcome {
    let Some(task_id) = task_id_of_principal(&ctx.principal) else {
        return CallOutcome::InvalidArgs {
            message: "公告栏仅限 Task 成员(worker/coord)使用:当前主体无所属 Task".into(),
        };
    };
    let Ok(task_bmid) = BmId::parse(&task_id) else {
        return CallOutcome::InvalidArgs {
            message: "主体所属 Task 标识非法".into(),
        };
    };
    if w.task_board.entry(task_bmid.as_str()).is_none() {
        return CallOutcome::InvalidArgs {
            message: format!("Task 不存在: {task_id}"),
        };
    }
    match capability {
        SHARE_PUBLISH => {
            let Some(title) = args["title"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                return CallOutcome::InvalidArgs {
                    message: "title 必填(非空)".into(),
                };
            };
            let Some(content) = args["content"].as_str().filter(|s| !s.is_empty()) else {
                return CallOutcome::InvalidArgs {
                    message: "content 必填(非空)".into(),
                };
            };
            if title.chars().count() > 200 {
                return CallOutcome::InvalidArgs {
                    message: "title 超长(上限 200 字符)".into(),
                };
            }
            if content.chars().count() > 8000 {
                return CallOutcome::InvalidArgs {
                    message: "content 超长(上限 8000 字符)".into(),
                };
            }
            // 事件即事实:写穿持久 + 公告栏投影增量(emit 钩子同点维护)
            let env = w.emit(
                EventType::SharePublished,
                None,
                None,
                Some(op_id.clone()),
                serde_json::json!({
                    "task_id": task_id,
                    "principal": ctx.principal,
                    "title": title,
                    "content": content,
                }),
            );
            let result = serde_json::json!({
                "published": true,
                "task_id": task_id,
                "share_seq": env.event_seq,
            });
            complete_share_call(w, op_id, capability, &ctx.principal, &prepared, result)
        }
        SHARE_LIST => {
            let since = args["since_seq"].as_u64().unwrap_or(0);
            let shares = w.share_board.list(task_bmid.as_str(), since);
            let items: Vec<serde_json::Value> = shares
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "seq": s.seq,
                        "principal": s.principal,
                        "title": s.title,
                        "content": s.content,
                        "occurred_at": s.occurred_at,
                    })
                })
                .collect();
            let total = items.len();
            let result = serde_json::json!({
                "task_id": task_id,
                "total": total,
                "shares": items,
            });
            complete_share_call(w, op_id, capability, &ctx.principal, &prepared, result)
        }
        _ => CallOutcome::InvalidArgs {
            message: format!("未知 task.share 能力: {capability}"),
        },
    }
}

/// 两个 share 动作的公共收尾:审计事件 + Completed 收据组装。
fn complete_share_call(
    w: &mut World,
    op_id: &BmId,
    capability: &str,
    principal: &str,
    prepared: &crate::broker::PreparedCall,
    result: serde_json::Value,
) -> CallOutcome {
    emit_capability_invoked(
        w,
        op_id,
        capability,
        principal,
        Some(prepared.credential.binding_epoch),
        Some(&prepared.credential.provider_instance_id),
        "succeeded",
        None,
        None,
    );
    CallOutcome::Completed {
        call_id: prepared.credential.call_id.clone(),
        grant_id: prepared.grant_id.clone(),
        credential: prepared.credential.clone(),
        result,
    }
}
