//! 自 task_ops.rs 机械移入(内容零改动)。
use super::*;

pub(crate) fn handle_task_lifecycle(
    w: &mut World,
    _request_id: BmId,
    action: TaskAction,
    params: wire::TaskLifecycleParams,
) -> CoreResult<wire::TaskStateResult> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝 Task 生命周期命令".into(),
        ));
    }
    let Some(task) = w.tasks.get_mut(&params.task_id) else {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 不存在: {}", params.task_id.as_str()),
        ));
    };
    // epoch 门禁:wire 路径出示当前 epoch(接管权变更后旧命令在核心面拒绝)
    task.require_epoch(task.task_epoch)
        .map_err(|e| task_error_to_core(&params.task_id, e))?;
    let now = w.config.clock.now();
    let to = match action {
        TaskAction::Pause => bm_contract::states::TaskState::Paused,
        TaskAction::Resume => bm_contract::states::TaskState::Running,
        TaskAction::Stop => bm_contract::states::TaskState::Cancelled,
    };
    // 分阶段作用域(跨字段借用):迁移与取值完成后即释放 task 借用
    let (transition, state_result, task_snapshot) = {
        let (from, to, guard) = task
            .transition(to, None, now)
            .map_err(|e| task_error_to_core(&params.task_id, e))?;
        let result = wire::TaskStateResult {
            task_id: task.id.clone(),
            state: task.state,
        };
        (Some((from, to, guard)), result, task.clone())
    };
    let (from, to, guard) = transition.expect("迁移已成功");
    w.emit(
        EventType::TaskStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task_snapshot.id.as_str(),
            "from": from.as_str(),
            "to": to.as_str(),
            "reason_code": guard,
            "task_epoch": task_snapshot.task_epoch,
        }),
    );
    persist_task(w, &task_snapshot);
    // Task 结束即失效(ADR-0002 要点 1):终态时该 Task 的全部 task:<id>
    // Grant 撤销(审计 grant.revoked,持久行同步,重启不复活)
    if to == bm_contract::states::TaskState::Cancelled {
        let gids: Vec<String> = w
            .grants
            .grants_scoped_to(task_snapshot.id.as_str())
            .into_iter()
            .filter(|g| {
                w.grants
                    .entry_state(&g.grant_id)
                    .map(|(_, revoked)| !revoked)
                    .unwrap_or(false)
            })
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
                    "reason": "task_cancelled",
                }),
            );
            persist_grant(w, &gid);
        }
    }
    Ok(state_result)
}
pub(crate) fn watchdog_scan_run(w: &mut World) -> usize {
    // P1-4: 看门狗每拍同步扫描并清理已过期审批,杜绝滞留单与惰性过期脱节
    crate::runtime::handlers::expire_due_approvals(w);

    let now = w.config.clock.now();
    let mut events = 0;
    // 分阶段作用域:先收集 Running 任务与判定,再逐个变更
    let mut decisions: Vec<(BmId, crate::watchdog::ScanDecision)> = Vec::new();
    for t in w.tasks.values() {
        if t.state != bm_contract::states::TaskState::Running {
            continue;
        }
        let created = crate::watchdog::parse_or(t.created_at.as_str(), now);
        // #30:Task 级停滞窗口/硬顶覆盖(Budget 开放键),None = 全局 limits
        let budget = t.budget.as_ref();
        let stall_override = crate::team::stall_after_ms_of(budget);
        let hard_override = crate::team::stall_hard_limit_ms_of(budget);
        if let Some(d) =
            w.watchdog
                .decide_with(t.id.as_str(), created, now, stall_override, hard_override)
        {
            decisions.push((t.id.clone(), d));
        }
    }
    for (tid, d) in decisions {
        match d {
            crate::watchdog::ScanDecision::Stall => {
                let elapsed_ms = {
                    let watch = w.watchdog.watches.get(tid.as_str());
                    watch
                        .map(|x| (now - x.last_progress_at).num_milliseconds())
                        .unwrap_or(0)
                };
                let last_seq = w
                    .watchdog
                    .watches
                    .get(tid.as_str())
                    .map(|x| x.last_progress_seq)
                    .unwrap_or(0);
                w.watchdog.mark_stall_notified(tid.as_str());
                w.emit(
                    EventType::TaskStalled,
                    None,
                    None,
                    None,
                    serde_json::json!({
                        "task_id": tid.as_str(),
                        "stalled_ms": elapsed_ms,
                        "last_progress_seq": last_seq,
                    }),
                );
                w.emit(
                    EventType::WatchdogReorchestrationTriggered,
                    None,
                    None,
                    None,
                    serde_json::json!({
                        "task_id": tid.as_str(),
                        "trigger": "watchdog",
                        "reason": "stalled_after_default_window",
                    }),
                );
                events += 2;
            }
            crate::watchdog::ScanDecision::HardLimit => {
                // 分阶段作用域:迁移完成后即释放 task 借用
                let Some((from, to, guard, snapshot)) = (|| {
                    let task = w.tasks.get_mut(&tid)?;
                    let (from, to, guard) = task
                        .transition(bm_contract::states::TaskState::Blocked, None, now)
                        .ok()?;
                    Some((from, to, guard, task.clone()))
                })() else {
                    continue;
                };
                w.emit(
                    EventType::TaskStateChanged,
                    None,
                    None,
                    None,
                    serde_json::json!({
                        "task_id": tid.as_str(),
                        "from": from.as_str(),
                        "to": to.as_str(),
                        "reason_code": guard,
                        "task_epoch": snapshot.task_epoch,
                    }),
                );
                persist_task(w, &snapshot);
                events += 1;
            }
        }
    }
    events
}
pub(crate) fn handle_task_report_completion(
    w: &mut World,
    task_id: BmId,
    claim_summary: String,
    operation_id: Option<BmId>,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝完成报告".into(),
        ));
    }
    let Some(task) = w.tasks.get(&task_id) else {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 不存在: {}", task_id.as_str()),
        ));
    };
    if !matches!(
        task.state,
        bm_contract::states::TaskState::Running | bm_contract::states::TaskState::Paused
    ) {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 状态 {} 不可受理完成报告", task.state.as_str()),
        ));
    }
    // 核验证据:声称所涉 Operation 的能力 verification 钩子
    let mut evidence: Vec<(String, String)> = Vec::new();
    let mut verdict = "unverified";
    if let Some(op_id) = &operation_id {
        evidence.push(("receipt".into(), op_id.as_str().to_string()));
        let capability = w.op_capability.get(op_id).cloned();
        if let Some(cap) = capability
            && let Some(manifest) = w.registry.manifest_of(&cap)
            && let Some(hook) = &manifest.verification
        {
            let query = hook["query"].as_str().unwrap_or_default().to_string();
            let expect = hook["expect"].as_str().unwrap_or("exists").to_string();
            if !query.is_empty() {
                // 观测查询:observation principal × read-only trusted → 直通
                let req = w.config.id_gen.next_id("req");
                let ctx = CallContext::surface("system:observation");
                let outcome = capability_call_inner(
                    w,
                    req,
                    ctx,
                    wire::CapabilityCallParams {
                        capability: query.clone(),
                        args: serde_json::json!({"subject": task_id.as_str()}),
                        idempotency_key: None,
                        deadline_ms: Some(2000),
                    },
                )
                .1;
                match outcome {
                    Ok(result) => {
                        let satisfied = crate::observation::expect_satisfied(&result, &expect);
                        evidence.push(("state_check".into(), format!("{query} expect={expect}")));
                        verdict = match satisfied {
                            Some(true) => "verified",
                            _ => "unverified",
                        };
                    }
                    Err(_) => {
                        evidence.push(("state_check".into(), format!("{query} 不可得")));
                        verdict = "unverified";
                    }
                }
            }
        }
    }
    let now = w.config.clock.now();
    let now_ts = format_ts(now);
    // 状态机终局(门禁在 Task::transition:verified=false 不得 completed)
    let (from, to, guard, verified_flag) = {
        let Some(task) = w.tasks.get_mut(&task_id) else {
            return Err(CoreError::Internal);
        };
        if verdict == "verified" {
            let r = task
                .transition(bm_contract::states::TaskState::Completed, Some(true), now)
                .expect("verified → completed 是迁移表边");
            (r.0, r.1, r.2, true)
        } else {
            let r = task
                .transition(bm_contract::states::TaskState::Blocked, None, now)
                .map_err(|e| task_error_to_core(&task_id, e))?;
            (r.0, r.1, r.2, false)
        }
    };
    let guard_state = if verdict == "verified" {
        "completed"
    } else {
        "outcome_unknown"
    };
    // Observation Log 行 + observation.recorded 事件
    let entry = crate::observation::ObservationEntry {
        log_seq: 0,
        task_id: task_id.as_str().to_string(),
        agent_id: None,
        operation_id: operation_id.as_ref().map(|o| o.as_str().to_string()),
        claim_summary: claim_summary.clone(),
        evidence: evidence.clone(),
        verdict: if verdict == "verified" {
            "verified"
        } else {
            "unverified"
        },
        guard_state,
        observed_at: now_ts.clone(),
    };
    if let Some(store) = &w.store {
        let seq = store
            .save_observation(
                task_id.as_str(),
                entry.verdict,
                guard_state,
                &entry.to_contract_json(),
                &now_ts,
            )
            .unwrap_or(0);
        w.emit(
            EventType::ObservationRecorded,
            None,
            None,
            None,
            serde_json::json!({
                "task_id": task_id.as_str(),
                "log_seq": seq,
                "verdict": entry.verdict,
                "guard_state": guard_state,
            }),
        );
    }
    w.emit(
        EventType::TaskStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task_id.as_str(),
            "from": from.as_str(),
            "to": to.as_str(),
            "reason_code": guard,
            "task_epoch": w.tasks[&task_id].task_epoch,
        }),
    );
    let snapshot = w.tasks[&task_id].clone();
    persist_task(w, &snapshot);
    Ok(serde_json::json!({
        "task_id": task_id.as_str(),
        "verdict": entry.verdict,
        "verified": verified_flag,
        "state": snapshot.state.as_str(),
        "claim_digest": crate::observation::claim_digest(&claim_summary),
    }))
}
pub(crate) fn handle_task_budget_increase(
    w: &mut World,
    task_id: BmId,
    max_tool_calls: u64,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝扩容".into(),
        ));
    }
    // 分阶段作用域:包络更新与迁移完成后即释放 task 借用
    let (old_limit, snapshot, transition) = {
        let Some(task) = w.tasks.get_mut(&task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 不存在: {}", task_id.as_str()),
            ));
        };
        if task.is_terminal() {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                "终态 Task 不可扩容".into(),
            ));
        }
        let old_limit = task
            .budget
            .as_ref()
            .and_then(|b| b.extra.get("max_tool_calls"))
            .and_then(|v| match v {
                bm_contract::budget::ExtraValue::Int(n) => u64::try_from(*n).ok(),
                _ => None,
            })
            .unwrap_or(0);
        let budget = task
            .budget
            .get_or_insert_with(|| bm_contract::budget::Budget {
                max_tokens: u64::MAX,
                max_turns: u32::MAX,
                extra: Default::default(),
            });
        budget.extra.insert(
            "max_tool_calls".into(),
            bm_contract::budget::ExtraValue::Int(max_tool_calls as i64),
        );
        // blocked(budget_exhausted)的任务:扩容即用户裁定 → 恢复运行
        let mut transition = None;
        if task.state == bm_contract::states::TaskState::Blocked {
            let now = w.config.clock.now();
            let (from, to, guard) = task
                .transition(bm_contract::states::TaskState::Running, None, now)
                .expect("blocked→running 是迁移表边(user_resolved)");
            transition = Some((from, to, guard));
        }
        (old_limit, task.clone(), transition)
    };
    w.emit(
        EventType::TaskBudgetIncreased,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task_id.as_str(),
            "key": "max_tool_calls",
            "old_limit": old_limit,
            "new_limit": max_tool_calls,
            "approval_id": null,
        }),
    );
    if let Some((from, to, guard)) = transition {
        w.emit(
            EventType::TaskStateChanged,
            None,
            None,
            None,
            serde_json::json!({
                "task_id": task_id.as_str(),
                "from": from.as_str(),
                "to": to.as_str(),
                "reason_code": guard,
                "task_epoch": snapshot.task_epoch,
            }),
        );
    }
    persist_task(w, &snapshot);
    let state_after = snapshot.state;
    Ok(serde_json::json!({
        "task_id": task_id.as_str(),
        "max_tool_calls": max_tool_calls,
        "state": state_after.as_str(),
    }))
}
