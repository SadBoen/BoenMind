//! Task 族处理器与参数(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

use super::*;

/// 追加成员入参(M6.3)。
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnMemberParams {
    pub task_id: BmId,
}

/// 子任务委派入参(M6.3/M6.5;门禁:深度/子集/预算——并发门禁未实现,
// 本地 worker 不占异步并发位,如需并发上限随后续里程碑增补)。
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnSubtaskParams {
    pub parent_task_id: BmId,
    pub title: String,
    pub goal: String,
    pub authorization: Vec<wire::TaskAuthorizationEntry>,
    pub budget: Option<bm_contract::budget::Budget>,
}

/// 成员移除入参(替换/退出留痕;墓碑语义)。
#[derive(Debug, Clone, PartialEq)]
pub struct RemoveMemberParams {
    pub task_id: BmId,
    pub agent_id: BmId,
    pub reason: String,
}

/// Worker 能力调用入参(Agent 路径;非 Wire 面——GT-03 场景 A3,worker 以
/// task:<id> Grant 直通;worker agent 的自主回合循环随 M7 真实 Provider)。
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerCallParams {
    pub task_id: BmId,
    pub capability: String,
    pub args: serde_json::Value,
    pub idempotency_key: Option<String>,
    pub deadline_ms: Option<u64>,
}

/// Task 行同步(M5-T1):完整合同载荷落 tasks 表;先于事件物化(与
/// persist_approval 同款顺序,事件 INSERT OR IGNORE 不覆盖完整行)。
/// 2026-09-05 口径统一:落库失败=重启后 Task 投影缺失(事件流声称存在),
/// 进入拒写态,不再静默吞错。
pub(crate) fn persist_task(w: &mut World, task: &crate::task::Task) {
    let Some(store) = w.store.clone() else {
        return;
    };
    let payload = task_contract_json(task);
    if let Err(e) = store.save_task(bm_persist::sqlite_state::TaskRow {
        id: task.id.as_str(),
        title: &task.title,
        state: task.state.as_str(),
        created_by: &task.created_by,
        task_epoch: task.task_epoch,
        payload: &payload,
        created_at: task.created_at.as_str(),
        updated_at: task.updated_at.as_str(),
        parent_task_id: task.parent_task_id.as_ref().map(|p| p.as_str()),
        delegation_depth: task.delegation_depth,
    }) {
        tracing::error!(error = %e, task = %task.id.as_str(), "Task 行落库失败,进入拒写态");
        w.persist_poisoned = true;
    }
}

/// Task 合同形态 JSON(task/task.v0.1;members 在行级表 + 事件承载,不入载荷)。
/// budget None = 运行时默认包络(落为显式对象,合同 budget 必为对象)。
pub(crate) fn task_contract_json(task: &crate::task::Task) -> String {
    const DEFAULT_MAX_TOKENS: i64 = 1_000_000;
    const DEFAULT_MAX_TURNS: i64 = 1_000;
    let budget_json = match &task.budget {
        Some(b) => serde_json::to_value(b).unwrap_or(serde_json::json!({})),
        None => serde_json::json!({
            "max_tokens": DEFAULT_MAX_TOKENS, "max_turns": DEFAULT_MAX_TURNS
        }),
    };
    serde_json::json!({
        "task_id": task.id.as_str(),
        "title": task.title,
        "goal": task.goal,
        "state": task.state.as_str(),
        "created_by": task.created_by,
        "task_epoch": task.task_epoch,
        "authorization": task.authorization,
        "budget": budget_json,
        "deadline": task.deadline.as_deref(),
        "members": [],
        "parent_task_id": task.parent_task_id.as_ref().map(|p| p.as_str()),
        "delegation_depth": task.delegation_depth,
        "created_at": task.created_at.as_str(),
        "updated_at": task.updated_at.as_str(),
    })
    .to_string()
}

/// task.create:Task 对象入 L2(tasks 表 + task.created 事件)并即启动
/// (created→running,GT-03 场景 A1 语义:Butler 接单即开跑)。
/// request_id 预留审计(M5 规格归因链随 T3 Butler 接线启用)。
pub(crate) fn handle_task_create(
    w: &mut World,
    _request_id: BmId,
    params: wire::TaskCreateParams,
) -> CoreResult<wire::TaskCreateResult> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝创建 Task".into(),
        ));
    }
    // 协调权门禁(M5.1):task.create 是 Butler 的 mutation 协调动词——
    // bootstrap Grant 被撤销后此命令拒绝(重授走审批,撤销不影响既有 Task)
    if w.grants
        .active_for(
            crate::butler::BUTLER_PRINCIPAL,
            "task.create",
            w.config.clock.now(),
        )
        .is_empty()
    {
        return Err(CoreError::Semantic(
            ErrorCode::PermissionDenied,
            "Butler 协调权(task.create)已被撤销,重授需用户批准".into(),
        ));
    }
    // Task 授权校验:动词 ⊆ Butler 协调清单(上界);mutation 动词必须显式
    // 标记 klass=mutation(ADR-0002 §11.2 二分;领域动词不可授权)
    let authorization = params.authorization.unwrap_or_default();
    for entry in &authorization {
        let Some(class) = crate::butler::verb_class(&entry.verb) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("非协调动词不可授权: {}", entry.verb),
            ));
        };
        let ok = match class {
            crate::butler::CoordinationClass::Mutation => {
                entry.klass.as_deref() == Some("mutation")
            }
            crate::butler::CoordinationClass::Safe => {
                matches!(entry.klass.as_deref(), None | Some("safe"))
            }
        };
        if !ok {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!(
                    "授权分级与动词默认分级不一致: {}(mutation 动词必须显式 klass=mutation)",
                    entry.verb
                ),
            ));
        }
    }
    let now = w.config.clock.now();
    // wire task.create 恒为根 Task(委派走 spawn_subtask 内核 API,M6 规格 §8-4)
    let mut task = crate::task::Task::create(
        &*w.config.id_gen,
        params.title,
        params.goal,
        authorization,
        params.budget,
        params.deadline,
        None,
        0,
        now,
    );
    // task.created(事实)+ created→running(task_started)
    w.emit(
        EventType::TaskCreated,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task.id.as_str(),
            "title": task.title,
            "created_by": task.created_by,
            "parent_task_id": null,
        }),
    );
    let (from, to, guard) = task
        .transition(bm_contract::states::TaskState::Running, None, now)
        .expect("created→running 是迁移表边");
    w.emit(
        EventType::TaskStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task.id.as_str(),
            "from": from.as_str(),
            "to": to.as_str(),
            "reason_code": guard,
            "task_epoch": task.task_epoch,
        }),
    );

    // M5-T4/T5:协调链自举——三方交集物化为 task:<id> Grant(ADR-0002 §11.3)
    // + Coordinator/单 Worker 成员事实(GT-03 场景 A2 形态)
    {
        let task_id_str = task.id.as_str().to_string();
        // M6:per-task principal 命名空间(跨 Task 访问在 Grant 查表层结构性不命中)
        let coord_aud = crate::team::coord_principal(&task_id_str);
        let worker_aud = crate::team::worker_principal(&task_id_str);
        // 分阶段作用域:butler 上界查证闭包借用 w.grants,产出后即释放
        let (coord_grants, worker_grants) = {
            let mut butler_lookup = |verb: &str| {
                w.grants
                    .active_for(crate::butler::BUTLER_PRINCIPAL, verb, now)
                    .into_iter()
                    .next()
            };
            crate::coordinator::intersection_grants(
                &*w.config.id_gen,
                &task_id_str,
                &coord_aud,
                &worker_aud,
                &task.authorization,
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
        // 成员事实:Coordinator(必有)+ Worker(仅当任务声明了能力资源)
        let member_event = |w: &mut World, agent_id: &str, role: &str, grant_id: Option<&str>| {
            w.emit(
                EventType::TaskMemberAdded,
                None,
                None,
                None,
                serde_json::json!({
                    "task_id": task_id_str,
                    "agent_id": agent_id,
                    "role": role,
                    "grant_id": grant_id,
                }),
            )
        };
        let coord_member_id = w.config.id_gen.next_id("agent");
        let coord_grant_id = coord_grants
            .iter()
            .find(|g| g.action == "agent.spawn")
            .or_else(|| coord_grants.first())
            .map(|g| g.grant_id.clone());
        let ev = member_event(
            w,
            coord_member_id.as_str(),
            "coordinator",
            coord_grant_id.as_deref(),
        );
        task.add_member(crate::task::TaskMember {
            agent_id: coord_member_id,
            role: crate::task::MemberRole::Coordinator,
            grant_id: coord_grant_id,
            joined_seq: ev.event_seq,
        });
        if !worker_grants.is_empty() {
            let worker_member_id = w.config.id_gen.next_id("agent");
            let worker_grant_id = worker_grants[0].grant_id.clone();
            let ev = member_event(
                w,
                worker_member_id.as_str(),
                "worker",
                Some(worker_grant_id.as_str()),
            );
            task.add_member(crate::task::TaskMember {
                agent_id: worker_member_id,
                role: crate::task::MemberRole::Worker,
                grant_id: Some(worker_grant_id),
                joined_seq: ev.event_seq,
            });
        }
    }
    persist_task(w, &task);
    let result = wire::TaskCreateResult {
        task_id: task.id.clone(),
        state: task.state,
        created_at: task.created_at.clone(),
    };
    w.tasks.insert(task.id.clone(), task);
    Ok(result)
}

/// task.pause / task.resume / task.stop:生命周期命令(表内边 + epoch 门禁 +
/// 事实事件 + 行同步)。wire 面不暴露 epoch 参数(与 input_trust 同款收权):
/// wire 命令恒以当前 epoch 出示;stale 语义由编排器内部路径与测试行使。
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

/// Watchdog 扫描(运行时执行面):判定 → 事实事件/状态变更。
/// 仅监督,不推断编排下一步(G4);blocked 后不再自动重启(ADR-0004 条件 6)。
pub(crate) fn watchdog_scan_run(w: &mut World) -> usize {
    let now = w.config.clock.now();
    let mut events = 0;
    // 分阶段作用域:先收集 Running 任务与判定,再逐个变更
    let mut decisions: Vec<(BmId, crate::watchdog::ScanDecision)> = Vec::new();
    for t in w.tasks.values() {
        if t.state != bm_contract::states::TaskState::Running {
            continue;
        }
        let created = crate::watchdog::parse_or(t.created_at.as_str(), now);
        if let Some(d) = w.watchdog.decide(t.id.as_str(), created, now) {
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

impl World {
    /// 到期自动扫描(核心循环节拍)。
    pub(crate) fn maybe_watchdog_scan(&mut self) {
        let now = self.config.clock.now();
        if !self.watchdog.due(now) {
            return;
        }
        watchdog_scan_run(self);
        self.watchdog.schedule_next(now);
    }

    /// 手动扫描(诊断/测试入口):忽略节拍,直接执行并重排下次。
    pub(crate) fn watchdog_scan_now(&mut self) -> usize {
        let now = self.config.clock.now();
        let n = watchdog_scan_run(self);
        self.watchdog.schedule_next(now);
        n
    }
}

/// Worker 声称完成 → Observation 核验 → 状态机终局(完成判定门禁):
/// - 声称所涉 Operation 的能力声明了 verification 钩子 → 执行 query 能力
///   (observation principal,read-only trusted 直通)做确定性核验;
/// - verified → completed(verified_completion);证据不足/无核验钩子 →
///   unverified → blocked(outcome_unknown_pending)等用户裁定;
/// - 观测写 Observation Log(observation.recorded 事件镜像)。
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

/// Task 预算扩容:更新包络 + 事实事件 + blocked 恢复(用户批准面)。
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

// ---- M6:Team 编队与委派(T2)----------------------------------------------

/// 追加 Worker 成员(M6.3):并发门禁(存活 worker ≤ 5)→ 授权链签发
/// (与自举同构,per-task principal)→ member.added。
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
        if alive_workers >= crate::team::MAX_CONCURRENT_WORKERS {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!(
                    "并发上限:存活 worker {} 已达 {}",
                    alive_workers,
                    crate::team::MAX_CONCURRENT_WORKERS
                ),
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

/// 委派子任务(M6.3/M6.5):四门禁(深度/授权子集/预算/并发)→ 子 Task
/// 创建 + 协调链自举。委派 = 子任务(规格 §5.2);权限只减不增。
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

/// 成员移除(M6.3):member.removed 留痕 + 成员列表移除(墓碑语义)。
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

/// 结果收集(M6.6):来源/状态/关联 Operation 三要素 + 子任务概览。
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

/// Worker 能力调用(Agent 路径):Task 必须在运行态;worker principal +
/// untrusted 上下文走统一执行体(Grant 命中直通 / 无授权 100% 升级审批)。
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

/// Butler 协调权撤销:撤销 bootstrap Grant 集(审计 grant.revoked),持久行
/// 同步(重启后不复活——materialize 尊重已撤销行)。
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

/// task.list:全量任务投影为合同对象;(created_at, task_id) 字典序确定性。
/// state_filter 未识别的状态串 = 空列表(宽松过滤,合同未约束枚举面)。
pub(crate) fn handle_task_list(
    w: &World,
    params: wire::TaskListParams,
) -> CoreResult<wire::TaskListResult> {
    let mut tasks: Vec<&crate::task::Task> = w
        .tasks
        .values()
        .filter(|t| match &params.state_filter {
            Some(f) => t.state.as_str() == f,
            None => true,
        })
        .collect();
    tasks.sort_by(|a, b| (&a.created_at, a.id.as_str()).cmp(&(&b.created_at, b.id.as_str())));
    Ok(wire::TaskListResult {
        tasks: tasks
            .iter()
            .map(|t| serde_json::from_str(&task_contract_json(t)).unwrap_or_default())
            .collect(),
    })
}

/// task.get:规范对象 + 监护态投影(guard_states 随 T7 Watchdog 填充)。
pub(crate) fn handle_task_get(
    w: &World,
    params: wire::TaskGetParams,
) -> CoreResult<wire::TaskGetResult> {
    let Some(task) = w.tasks.get(&params.task_id) else {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 不存在: {}", params.task_id.as_str()),
        ));
    };
    Ok(wire::TaskGetResult {
        task: serde_json::from_str(&task_contract_json(task)).unwrap_or_default(),
        guard_states: None,
    })
}

/// TaskError → 统一错误信封(脱敏;epoch 拒绝带 reason_code 语义)。
pub(crate) fn task_error_to_core(task_id: &BmId, e: crate::task::TaskError) -> CoreError {
    let msg = match e {
        crate::task::TaskError::IllegalTransition { from, to } => format!(
            "Task {} 表外迁移: {} -> {}",
            task_id.as_str(),
            from.as_str(),
            to.as_str()
        ),
        crate::task::TaskError::UnverifiedCompletion => {
            format!("Task {} 无核验结论不得终局(完成判定门禁)", task_id.as_str())
        }
        crate::task::TaskError::StaleEpoch { current, presented } => format!(
            "Task {} 命令携带过期 epoch({presented},当前 {current}),Stale 拒绝",
            task_id.as_str()
        ),
    };
    CoreError::Semantic(ErrorCode::ValidationFailed, msg)
}

// ---- M9-S3:worker 自主环 v0 -----------------------------------------------
// 事件驱动(单写者内逐步推进):task.autorun 受理 → 专属会话回合 →
// TurnEvent 回账 → pump 裁决(继续/完成/停滞/超限/外部暂停)→ 下一回合。
// v0 完成哨兵 = 模型回复以 [[AUTORUN_DONE]] 开头(无工具调用的模型面,
// 哨兵即完成声明;真工具闭环随模型 tools 合同演进)。

pub(crate) const AUTORUN_DONE_MARK: &str = "[[AUTORUN_DONE]]";

pub(crate) struct AutorunState {
    pub(crate) session_id: BmId,
    pub(crate) agent_id: BmId,
    pub(crate) turn: u64,
    pub(crate) max_turns: u64,
    pub(crate) in_flight_op: Option<BmId>,
    pub(crate) last_content: Option<String>,
    pub(crate) prev_content: Option<String>,
    pub(crate) repeats: u64,
}

fn emit_autorun(w: &mut World, task_id: &BmId, phase: &str, turn: u64, reason: Option<&str>) {
    w.emit(
        EventType::TaskAutorunStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task_id.as_str(),
            "phase": phase,
            "turn": turn,
            "reason": reason,
        }),
    );
}

fn autorun_instruction(goal: &str, last: Option<&str>) -> String {
    let mut s = format!("任务目标:{goal}");
    s.push_str("\n请自主推进一步。");
    if let Some(l) = last {
        s.push_str(&format!("\n上一步输出:{l}"));
    }
    s.push_str("\n若任务已全部完成,本条回复以 [[AUTORUN_DONE]] 开头,其后接结果摘要。");
    s
}

pub(crate) fn handle_task_autorun_start(
    w: &mut World,
    request_id: BmId,
    params: bm_contract::wire::TaskAutorunParams,
) -> CoreResult<bm_contract::wire::TaskAutorunResult> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝自主环受理".into(),
        ));
    }
    let task_state = {
        let Some(task) = w.tasks.get(&params.task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 不存在: {}", params.task_id.as_str()),
            ));
        };
        task.state
    };
    if !matches!(task_state, bm_contract::states::TaskState::Running) {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 非运行态,拒绝自主环: {task_state:?}"),
        ));
    }
    if w.autorun.contains_key(&params.task_id) {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            "该 Task 已在自主推进中".into(),
        ));
    }
    // 工作链来源:系统主体(实际部署)/ 任一既有代理 / 兜底(v0;回看裁决)
    let model_chain = w
        .agents
        .get(&w.system_agent)
        .map(|a| a.model_chain.clone())
        .or_else(|| w.agents.values().next().map(|a| a.model_chain.clone()))
        .unwrap_or_else(|| vec!["model.default".to_string()]);
    let created = handle_session_create(
        w,
        request_id,
        bm_contract::wire::SessionCreateParams {
            agent: bm_contract::wire::AgentSpec {
                name: format!("autorun-{}", params.task_id.as_str()),
                model_chain,
                budget: None,
                system_prompt: None,
                workspace_id: None,
            },
        },
    )?;
    let max_turns = params.max_turns.unwrap_or(6).clamp(1, 50);
    w.autorun.insert(
        params.task_id.clone(),
        AutorunState {
            session_id: created.session_id.clone(),
            agent_id: created.agent_id.clone(),
            turn: 0,
            max_turns,
            in_flight_op: None,
            last_content: None,
            prev_content: None,
            repeats: 0,
        },
    );
    emit_autorun(w, &params.task_id, "started", 0, None);
    let goal = w.tasks[&params.task_id].goal.clone();
    let (session_id, agent_id) = {
        let st = &w.autorun[&params.task_id];
        (st.session_id.clone(), st.agent_id.clone())
    };
    let sent = handle_send_input(
        w,
        w.config.id_gen.next_id("req"),
        bm_contract::wire::SendInputParams {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            content: autorun_instruction(&goal, None),
            model_override: None,
            workspace_override: None,
            input_trust: bm_contract::wire::InputTrust::Trusted,
        },
    )?;
    // 防御化(原 expect):send_input 期间 autorun 态被并发清场的健壮性兜底
    let Some(st) = w.autorun.get_mut(&params.task_id) else {
        return Ok(bm_contract::wire::TaskAutorunResult {
            session_id,
            agent_id,
            accepted: false,
        });
    };
    st.in_flight_op = Some(sent.operation_id);
    Ok(bm_contract::wire::TaskAutorunResult {
        session_id,
        agent_id,
        accepted: true,
    })
}

/// 完成臂回账:Completed 事件携带的正文记入状态(pump 用)。
pub(crate) fn autorun_note_completed(w: &mut World, op_id: &BmId, content: &str) {
    for st in w.autorun.values_mut() {
        if st.in_flight_op.as_ref() == Some(op_id) {
            st.last_content = Some(content.to_string());
            st.turn += 1;
        }
    }
}

/// 外部终局:任务转 blocked(自主环出口专用)。
fn autorun_block(w: &mut World, task_id: &BmId, reason: &str) {
    let now = w.config.clock.now();
    let Some(task) = w.tasks.get_mut(task_id) else {
        return;
    };
    let epoch = task.task_epoch;
    if let Ok((from, to, _g)) = task.transition(bm_contract::states::TaskState::Blocked, None, now)
    {
        w.emit(
            EventType::TaskStateChanged,
            None,
            None,
            None,
            serde_json::json!({
                "task_id": task_id.as_str(),
                "from": from.as_str(),
                "to": to.as_str(),
                "reason_code": reason,
                "task_epoch": epoch,
            }),
        );
        let snapshot = w.tasks[task_id].clone();
        persist_task(w, &snapshot);
    }
}

/// 回合终局后的推进裁决(handle_turn_event 末尾调用)。
pub(crate) fn autorun_pump(w: &mut World, op_id: &BmId) {
    let Some(task_id) = w
        .autorun
        .iter()
        .find(|(_, s)| s.in_flight_op.as_ref() == Some(op_id))
        .map(|(k, _)| k.clone())
    else {
        return;
    };
    let receipt_state = w.operations.get(op_id).map(|o| o.state);
    match receipt_state {
        Some(bm_contract::states::OperationState::Succeeded) => {}
        Some(bm_contract::states::OperationState::Cancelled) => {
            emit_autorun(w, &task_id, "finished", 0, Some("cancelled"));
            w.autorun.remove(&task_id);
            return;
        }
        Some(_) => {
            emit_autorun(w, &task_id, "finished", 0, Some("model_failed"));
            w.autorun.remove(&task_id);
            return;
        }
        None => return,
    }
    let (turn, content) = {
        let st = &w.autorun[&task_id];
        (st.turn, st.last_content.clone().unwrap_or_default())
    };
    // 完成哨兵
    if content.trim_start().starts_with(AUTORUN_DONE_MARK) {
        let summary = content.trim_start()[AUTORUN_DONE_MARK.len()..]
            .trim()
            .to_string();
        let _ = handle_task_report_completion(w, task_id.clone(), summary, Some(op_id.clone()));
        emit_autorun(w, &task_id, "finished", turn, Some("done"));
        w.autorun.remove(&task_id);
        return;
    }
    // 停滞:连续两轮完全相同输出(防御化(原 expect):条目被并发清场则放弃推进)
    let Some(st) = w.autorun.get_mut(&task_id) else {
        return;
    };
    let same = st.prev_content.as_deref() == Some(content.as_str());
    if same {
        st.repeats += 1;
    } else {
        st.repeats = 1;
        st.prev_content = Some(content.clone());
    }
    if w.autorun[&task_id].repeats >= 2 {
        autorun_block(w, &task_id, "stalled");
        emit_autorun(w, &task_id, "finished", turn, Some("stalled"));
        w.autorun.remove(&task_id);
        return;
    }
    // 轮数上限
    if turn >= w.autorun[&task_id].max_turns {
        autorun_block(w, &task_id, "max_turns");
        emit_autorun(w, &task_id, "finished", turn, Some("max_turns"));
        w.autorun.remove(&task_id);
        return;
    }
    // 外部状态复检(暂停/停止即时生效)
    let task_state = w.tasks[&task_id].state;
    if !matches!(task_state, bm_contract::states::TaskState::Running) {
        emit_autorun(w, &task_id, "finished", turn, Some(task_state.as_str()));
        w.autorun.remove(&task_id);
        return;
    }
    // 继续下一步
    let (goal, session_id, agent_id) = {
        let st = &w.autorun[&task_id];
        (
            w.tasks[&task_id].goal.clone(),
            st.session_id.clone(),
            st.agent_id.clone(),
        )
    };
    let sent = handle_send_input(
        w,
        w.config.id_gen.next_id("req"),
        bm_contract::wire::SendInputParams {
            session_id,
            agent_id,
            content: autorun_instruction(&goal, Some(&content)),
            model_override: None,
            workspace_override: None,
            input_trust: bm_contract::wire::InputTrust::Trusted,
        },
    );
    match sent {
        Ok(r) => {
            emit_autorun(w, &task_id, "turn_completed", turn, None);
            // 防御化(原 expect):send_input 期间条目被并发清场则放弃回填
            if let Some(st) = w.autorun.get_mut(&task_id) {
                st.in_flight_op = Some(r.operation_id);
            }
        }
        Err(_) => {
            emit_autorun(w, &task_id, "finished", turn, Some("send_failed"));
            w.autorun.remove(&task_id);
        }
    }
}
