//! 自 task_ops.rs 机械移入(内容零改动)。
use super::*;

pub(crate) fn handle_task_spawn_member(
    w: &mut World,
    task_id: BmId,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝成员追加".into(),
        ));
    }
    // 分阶段作用域:读任务与并发计数,门禁通过后即释放借用
    let (coord_aud, worker_aud, authorization) = {
        let Some(task) = w.tasks.get(&task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 不存在: {}", task_id.as_str()),
            ));
        };
        if task.state != bm_contract::states::TaskState::Running {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 状态 {} 不可追加成员", task.state.as_str()),
            ));
        }
        let alive_workers = task
            .members
            .iter()
            .filter(|m| m.role == crate::task::MemberRole::Worker)
            .count() as u64;
        // #30:Task 级并发上限覆盖(Budget 开放键),None = 合同默认 5
        let worker_cap = crate::team::max_concurrent_workers_of(task.budget.as_ref())
            .unwrap_or(crate::team::MAX_CONCURRENT_WORKERS);
        if alive_workers >= worker_cap {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("并发上限:存活 worker {} 已达 {}", alive_workers, worker_cap),
            ));
        }
        (
            crate::team::coord_principal(task_id.as_str()),
            crate::team::worker_principal(task_id.as_str()),
            task.authorization.clone(),
        )
    };
    let now = w.config.clock.now();
    let (_coord_grants, worker_grants) = {
        let mut butler_lookup = |verb: &str| {
            w.grants
                .active_for(crate::butler::BUTLER_PRINCIPAL, verb, now)
                .into_iter()
                .next()
        };
        crate::coordinator::intersection_grants(
            &*w.config.id_gen,
            task_id.as_str(),
            &coord_aud,
            &worker_aud,
            &authorization,
            now,
            &mut butler_lookup,
        )
    };
    for g in worker_grants.iter() {
        w.grants.record(g.clone());
        persist_grant(w, &g.grant_id);
        w.emit(
            EventType::GrantCreated,
            None,
            None,
            None,
            serde_json::json!({
                "grant_id": g.grant_id,
                "approval_id": null,
                "audience": g.audience,
                "action": g.action,
                "scope": g.scope.to_wire(),
                "delegation_depth": g.delegation_depth,
                "expires_at": null,
                "parent_hash": g.parent_grant_hash,
                "resource": serde_json::to_value(&g.resource).expect("resource 序列化"),
            }),
        );
    }
    let member_id = w.config.id_gen.next_id("agent");
    let grant_id = worker_grants.first().map(|g| g.grant_id.clone());
    let ev = w.emit(
        EventType::TaskMemberAdded,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task_id.as_str(),
            "agent_id": member_id.as_str(),
            "role": "worker",
            "grant_id": grant_id,
        }),
    );
    {
        let Some(task) = w.tasks.get_mut(&task_id) else {
            return Err(CoreError::Internal);
        };
        task.add_member(crate::task::TaskMember {
            agent_id: member_id.clone(),
            role: crate::task::MemberRole::Worker,
            grant_id: grant_id.clone(),
            joined_seq: ev.event_seq,
        });
    }
    let snapshot = w.tasks[&task_id].clone();
    persist_task(w, &snapshot);
    Ok(serde_json::json!({
        "task_id": task_id.as_str(),
        "agent_id": member_id.as_str(),
        "grant_id": grant_id,
    }))
}
pub(crate) fn handle_task_spawn_subtask(
    w: &mut World,
    params: SpawnSubtaskParams,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝委派".into(),
        ));
    }
    // 门禁(分阶段作用域:校验后即释放借用)
    let (parent_snapshot, coord_aud, _worker_aud, child_authorization) = {
        let Some(parent) = w.tasks.get(&params.parent_task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("父 Task 不存在: {}", params.parent_task_id.as_str()),
            ));
        };
        if !crate::team::depth_ok(parent.delegation_depth) {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!(
                    "委派深度超限:父深度 {} + 1 > {}",
                    parent.delegation_depth,
                    crate::team::MAX_DELEGATION_DEPTH
                ),
            ));
        }
        if !crate::team::authorization_subset(&params.authorization, &parent.authorization) {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                "委派授权必须为父授权的子集(成员权限只减不增)".into(),
            ));
        }
        let parent_max = crate::team::max_tool_calls_of(parent.budget.as_ref());
        let parent_used = *w.task_tool_calls.get(&params.parent_task_id).unwrap_or(&0);
        let child_max = crate::team::max_tool_calls_of(params.budget.as_ref());
        if !crate::team::budget_ok(child_max, parent_max, parent_used) {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                "子任务预算超父包络剩余(预算子分配门禁)".into(),
            ));
        }
        (
            parent.clone(),
            crate::team::coord_principal(params.parent_task_id.as_str()),
            crate::team::worker_principal(params.parent_task_id.as_str()),
            params.authorization.clone(),
        )
    };
    let now = w.config.clock.now();
    let parent_id_str = params.parent_task_id.as_str().to_string();
    // 子 Task 创建(wire 之外的内核委派路径;created_by = 父 Coordinator)
    let mut child = crate::task::Task::create(
        &*w.config.id_gen,
        params.title,
        params.goal,
        child_authorization,
        params.budget,
        None,
        Some(params.parent_task_id.clone()),
        parent_snapshot.delegation_depth + 1,
        now,
    );
    child.created_by = coord_aud.clone();
    w.emit(
        EventType::TaskCreated,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": child.id.as_str(),
            "title": child.title,
            "created_by": child.created_by,
            "parent_task_id": child.parent_task_id.as_ref().map(|p| p.as_str()),
        }),
    );
    let (from, to, guard) = child
        .transition(bm_contract::states::TaskState::Running, None, now)
        .expect("created→running 是迁移表边");
    w.emit(
        EventType::TaskStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": child.id.as_str(),
            "from": from.as_str(),
            "to": to.as_str(),
            "reason_code": guard,
            "task_epoch": child.task_epoch,
        }),
    );
    // 子任务协调链自举(per-child principal;Grant 链仍回溯 Butler 上界)
    let child_id_str = child.id.as_str().to_string();
    let child_coord_aud = crate::team::coord_principal(&child_id_str);
    let child_worker_aud = crate::team::worker_principal(&child_id_str);
    let (coord_grants, worker_grants) = {
        let mut butler_lookup = |verb: &str| {
            w.grants
                .active_for(crate::butler::BUTLER_PRINCIPAL, verb, now)
                .into_iter()
                .next()
        };
        crate::coordinator::intersection_grants(
            &*w.config.id_gen,
            &child_id_str,
            &child_coord_aud,
            &child_worker_aud,
            &child.authorization,
            now,
            &mut butler_lookup,
        )
    };
    for g in coord_grants.iter().chain(worker_grants.iter()) {
        w.grants.record(g.clone());
        persist_grant(w, &g.grant_id);
        w.emit(
            EventType::GrantCreated,
            None,
            None,
            None,
            serde_json::json!({
                "grant_id": g.grant_id,
                "approval_id": null,
                "audience": g.audience,
                "action": g.action,
                "scope": g.scope.to_wire(),
                "delegation_depth": g.delegation_depth,
                "expires_at": null,
                "parent_hash": g.parent_grant_hash,
                "resource": serde_json::to_value(&g.resource).expect("resource 序列化"),
            }),
        );
    }
    let coord_member_id = w.config.id_gen.next_id("agent");
    let coord_grant_id = coord_grants.first().map(|g| g.grant_id.clone());
    let ev = w.emit(
        EventType::TaskMemberAdded,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": child_id_str,
            "agent_id": coord_member_id.as_str(),
            "role": "coordinator",
            "grant_id": coord_grant_id,
        }),
    );
    child.add_member(crate::task::TaskMember {
        agent_id: coord_member_id,
        role: crate::task::MemberRole::Coordinator,
        grant_id: coord_grant_id,
        joined_seq: ev.event_seq,
    });
    if !worker_grants.is_empty() {
        let worker_member_id = w.config.id_gen.next_id("agent");
        let worker_grant_id = worker_grants[0].grant_id.clone();
        let ev = w.emit(
            EventType::TaskMemberAdded,
            None,
            None,
            None,
            serde_json::json!({
                "task_id": child_id_str,
                "agent_id": worker_member_id.as_str(),
                "role": "worker",
                "grant_id": worker_grant_id,
            }),
        );
        child.add_member(crate::task::TaskMember {
            agent_id: worker_member_id,
            role: crate::task::MemberRole::Worker,
            grant_id: Some(worker_grant_id),
            joined_seq: ev.event_seq,
        });
    }
    persist_task(w, &child);
    let result = serde_json::json!({
        "task_id": child.id.as_str(),
        "parent_task_id": parent_id_str,
        "delegation_depth": child.delegation_depth,
        "state": child.state.as_str(),
    });
    w.tasks.insert(child.id.clone(), child);
    Ok(result)
}
pub(crate) fn handle_task_remove_member(
    w: &mut World,
    params: RemoveMemberParams,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝成员移除".into(),
        ));
    }
    let removed = {
        let Some(task) = w.tasks.get_mut(&params.task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 不存在: {}", params.task_id.as_str()),
            ));
        };
        let before = task.members.len();
        task.members
            .retain(|m| m.agent_id.as_str() != params.agent_id.as_str());
        before != task.members.len()
    };
    if !removed {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("成员不存在: {}", params.agent_id.as_str()),
        ));
    }
    w.emit(
        EventType::TaskMemberRemoved,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": params.task_id.as_str(),
            "agent_id": params.agent_id.as_str(),
            "reason": params.reason,
        }),
    );
    let snapshot = w.tasks[&params.task_id].clone();
    persist_task(w, &snapshot);
    Ok(serde_json::json!({
        "task_id": params.task_id.as_str(),
        "agent_id": params.agent_id.as_str(),
        "removed": true,
    }))
}
pub(crate) fn handle_task_collect(w: &World, task_id: BmId) -> CoreResult<serde_json::Value> {
    let Some(task) = w.tasks.get(&task_id) else {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 不存在: {}", task_id.as_str()),
        ));
    };
    let results = w.task_results.get(&task_id).cloned().unwrap_or_default();
    let children: Vec<serde_json::Value> = w
        .tasks
        .values()
        .filter(|t| t.parent_task_id.as_ref() == Some(&task_id))
        .map(|t| {
            serde_json::json!({
                "task_id": t.id.as_str(),
                "title": t.title,
                "state": t.state.as_str(),
                "delegation_depth": t.delegation_depth,
            })
        })
        .collect();
    Ok(serde_json::json!({
        "task_id": task_id.as_str(),
        "state": task.state.as_str(),
        "results": results,
        "children": children,
    }))
}
pub(crate) fn handle_worker_call(
    w: &mut World,
    request_id: BmId,
    params: WorkerCallParams,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝成员调用".into(),
        ));
    }
    // 分阶段作用域:状态检查完成后即释放 task 借用
    let state = {
        let Some(task) = w.tasks.get(&params.task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 不存在: {}", params.task_id.as_str()),
            ));
        };
        task.state
    };
    match state {
        bm_contract::states::TaskState::Running => {}
        bm_contract::states::TaskState::Paused => {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                "Task 暂停中,成员调用挂起".into(),
            ));
        }
        bm_contract::states::TaskState::Blocked => {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                "Task blocked,等待用户裁定".into(),
            ));
        }
        other => {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 状态 {} 不可执行成员调用", other.as_str()),
            ));
        }
    }
    // M5-T6:Task 包络「工具调用前」强制点(Broker 路径唯一执行出口:
    // 绕过 Broker 无预算执行出口——G 断言的结构面)
    let max_tool_calls = w
        .tasks
        .get(&params.task_id)
        .and_then(|t| t.budget.as_ref())
        .and_then(|b| b.extra.get("max_tool_calls"))
        .and_then(|v| match v {
            bm_contract::budget::ExtraValue::Int(n) => u64::try_from(*n).ok(),
            bm_contract::budget::ExtraValue::Float(f) => u64::try_from(*f as i64).ok(),
            _ => None,
        });
    let used = *w.task_tool_calls.entry(params.task_id.clone()).or_insert(0);
    if let Some(max) = max_tool_calls {
        // 软限 80%:budget.warning(基线 §9.7;逐次逼近即告警)
        if (used + 1) as f64 >= 0.8 * max as f64 && used < max {
            w.emit(
                EventType::BudgetWarning,
                None,
                None,
                None,
                serde_json::json!({
                    "agent_id": w.system_agent.as_str(),
                    "scope": format!("task:{}", params.task_id.as_str()),
                    "used_tokens": used + 1,
                    "limit_tokens": max,
                    "ratio": bm_contract::budget::round_ratio((used + 1) as f64, max as f64),
                }),
            );
        }
        // 硬限:拒绝 + Task blocked(budget_exhausted)等待用户裁定
        if used + 1 > max {
            w.emit(
                EventType::BudgetExceeded,
                None,
                None,
                None,
                serde_json::json!({
                    "agent_id": w.system_agent.as_str(),
                    "scope": format!("task:{}", params.task_id.as_str()),
                    "used_tokens": used,
                    "limit_tokens": max,
                }),
            );
            let now = w.config.clock.now();
            let (from, to, epoch) = {
                let Some(task) = w.tasks.get_mut(&params.task_id) else {
                    return Err(CoreError::Internal);
                };
                let epoch = task.task_epoch;
                let (from, to, _g) = task
                    .transition(bm_contract::states::TaskState::Blocked, None, now)
                    .expect("running→blocked 是迁移表边");
                (from, to, epoch)
            };
            w.emit(
                EventType::TaskStateChanged,
                None,
                None,
                None,
                serde_json::json!({
                    "task_id": params.task_id.as_str(),
                    "from": from.as_str(),
                    "to": to.as_str(),
                    "reason_code": "budget_exhausted",
                    "task_epoch": epoch,
                }),
            );
            let snapshot = w.tasks[&params.task_id].clone();
            persist_task(w, &snapshot);
            return Err(CoreError::Semantic(
                ErrorCode::BudgetExceeded,
                format!(
                    "Task {} 预算包络已耗尽(max_tool_calls={max}),转 blocked 等待用户裁定",
                    params.task_id.as_str()
                ),
            ));
        }
    }
    // Agent 路径信任归因:worker 上下文 = agent-derived/untrusted(内容
    // 来源链随任务传递,不可自报降级);Grant 命中优先,无授权则 100% 升级。
    // M6:per-task principal(跨 Task 结构性隔离)
    let ctx = CallContext::content_chain(
        crate::team::worker_principal(params.task_id.as_str()).as_str(),
        DataTrust::Untrusted,
    )
    .map_err(|_| CoreError::Internal)?;
    let (call_op_id, outcome) = capability_call_inner(
        w,
        request_id,
        ctx,
        wire::CapabilityCallParams {
            capability: params.capability.clone(),
            args: params.args.clone(),
            idempotency_key: params.idempotency_key.clone(),
            deadline_ms: params.deadline_ms,
        },
    );
    // 「返回后记账」+ 重复检测 + 进度信号(waiting_approval 豁免:等人的
    // 时间不算停滞,进度随审批挂起刷新)
    let outcome_str = match &outcome {
        Ok(_) => "ok",
        // 2026-09-05 对齐审批错配根治:升级面为结构化 ApprovalNeeded
        // (wire 投影即 ApprovalRequired);等人的时间不算停滞
        Err(CoreError::ApprovalNeeded { .. }) => "approval",
        Err(_) => "error",
    };
    let now = w.config.clock.now();
    let sig = crate::watchdog::call_sig(&params.capability, &params.args, outcome_str);
    let repeat_count = if outcome_str == "approval" {
        w.watchdog.mark_waiting(params.task_id.as_str(), now, 0);
        0
    } else {
        w.watchdog.note_call(params.task_id.as_str(), sig, now, 0)
    };
    if outcome_str != "approval" {
        *w.task_tool_calls.entry(params.task_id.clone()).or_insert(0) += 1;
        let used_now = w.task_tool_calls[&params.task_id];
        if let Some(store) = w.store.clone()
            && let Err(e) =
                store.save_task_budget(params.task_id.as_str(), "", used_now, 0, &w.now_ts())
        {
            // 2026-09-05 口径统一:包络计数落库失败=重启后预算计数回退
            // (事实上的预算绕过),进入拒写态
            tracing::error!(error = %e, task = %params.task_id.as_str(), "Task 预算行落库失败,进入拒写态");
            w.persist_poisoned = true;
        }
        // M6.6:结果流水(来源/状态/关联 Operation;collect 聚合面)
        // 2026-09-05 回看修复:operation_id 必须是本次调用真实产物
        // (capability_call_inner 交还),此前取 op_capability 无序尾键=证据链错挂。
        let summary = match &outcome {
            Ok(r) => r["action_summary"].as_str().unwrap_or_default().to_string(),
            Err(_) => String::new(),
        };
        w.task_results
            .entry(params.task_id.clone())
            .or_default()
            .push(serde_json::json!({
                "agent_id": crate::team::worker_principal(params.task_id.as_str()),
                "operation_id": call_op_id.as_str(),
                "capability": params.capability,
                "state": if outcome.is_ok() { "succeeded" } else { "failed" },
                "action_summary": summary,
            }));
    }
    if repeat_count == crate::watchdog::REPEAT_THRESHOLD {
        w.emit(
            EventType::TaskRepeating,
            None,
            None,
            None,
            serde_json::json!({
                "task_id": params.task_id.as_str(),
                "agent_id": w.system_agent.as_str(),
                "capability": params.capability,
                "repeat_count": repeat_count,
            }),
        );
    }
    outcome
}
pub(crate) fn handle_butler_revoke(w: &mut World, reason: String) -> CoreResult<usize> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝撤销操作".into(),
        ));
    }
    let mut revoked = 0;
    for (verb, _) in crate::butler::COORDINATION_VERBS {
        // 分阶段作用域:收集后逐个撤销(避免跨字段借用)
        let gids: Vec<String> = w
            .grants
            .active_for(crate::butler::BUTLER_PRINCIPAL, verb, w.config.clock.now())
            .into_iter()
            .map(|g| g.grant_id)
            .collect();
        for gid in gids {
            let version = w.grants.revoke(&gid).map_err(|_| CoreError::Internal)?;
            w.emit(
                EventType::GrantRevoked,
                None,
                None,
                None,
                serde_json::json!({
                    "grant_id": gid,
                    "revocation_version": version,
                    "reason": reason,
                }),
            );
            persist_grant(w, &gid);
            revoked += 1;
        }
    }
    Ok(revoked)
}
