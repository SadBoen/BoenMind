//! 会话/审批/操作/停止处理器(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

use super::*;

pub(crate) fn handle_session_create(
    w: &mut World,
    _request_id: BmId,
    params: SessionCreateParams,
) -> CoreResult<SessionCreateResult> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝新会话".into(),
        ));
    }
    let spec = &params.agent;
    if spec.name.is_empty() || spec.name.len() > 64 || spec.model_chain.is_empty() {
        return Err(CoreError::validation("agent 描述不完整"));
    }
    for m in &spec.model_chain {
        bm_contract::connector::validate_model_id(m).map_err(CoreError::validation)?;
    }

    let now = w.now_ts();
    let session_id = w.config.id_gen.next_id("sess");
    let agent_id = w.config.id_gen.next_id("agent");

    let mut session = Session {
        id: session_id.clone(),
        agent_id: agent_id.clone(),
        state: SessionState::Created,
        created_at: now.clone(),
    };
    // created→active(surface_attached):M1 进程内直调即视为已挂接。
    session.transition(SessionState::Active);
    w.sessions.insert(session_id.clone(), session);

    let budget = budget_from_spec(spec.budget.as_ref());
    w.agents.insert(
        agent_id.clone(),
        Agent {
            id: agent_id.clone(),
            session_id: session_id.clone(),
            name: spec.name.clone(),
            model_chain: spec.model_chain.clone(),
            state: AgentState::Created,
            budget,
            system_prompt: spec.system_prompt.clone(),
        },
    );
    // created→starting→running(agent_start + model_binding_ready):无事件(规格 §8.6)。
    {
        let agent = w.agents.get_mut(&agent_id).expect("已插入");
        agent.transition(AgentState::Starting);
        agent.transition(AgentState::Running);
    }

    w.emit(
        EventType::SessionCreated,
        Some(session_id.clone()),
        None,
        None,
        serde_json::json!({
            "session_id": session_id.as_str(),
            "agent_id": agent_id.as_str(),
        }),
    );
    let budget_limits = &w.agents[&agent_id].budget;
    w.emit(
        EventType::AgentCreated,
        Some(session_id.clone()),
        Some(agent_id.clone()),
        None,
        serde_json::json!({
            "agent_id": agent_id.as_str(),
            "session_id": session_id.as_str(),
            "model_chain": spec.model_chain,
            "budget": {"max_tokens": budget_limits.max_tokens, "max_turns": budget_limits.max_turns},
        }),
    );

    // M7 S1:模型调用权显式授权(Grant 台账;ADR-0006)——创建即授 Forever,
    // 可被 Butler revoke 收回;持久行保证重启后权利不丢。
    let mg =
        crate::butler::model_grant_for(&*w.config.id_gen, agent_id.as_str(), w.config.clock.now());
    w.grants.record(mg.clone());
    persist_grant(w, &mg.grant_id);
    w.emit(
        EventType::GrantCreated,
        None,
        None,
        None,
        serde_json::json!({
            "grant_id": mg.grant_id,
            "approval_id": null,
            "audience": mg.audience,
            "action": mg.action,
            "scope": mg.scope.to_wire(),
            "delegation_depth": mg.delegation_depth,
            "expires_at": null,
            "parent_hash": mg.parent_grant_hash,
            "resource": serde_json::to_value(&mg.resource).expect("resource 序列化"),
        }),
    );

    Ok(SessionCreateResult {
        session_id,
        agent_id,
        created_at: now,
        resume_cursor: Cursor {
            event_seq: w.bus.last_seq(),
        },
    })
}

pub(crate) fn handle_session_resume(
    w: &mut World,
    _request_id: BmId,
    params: SessionResumeParams,
) -> CoreResult<SessionResumeResult> {
    if w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "持久层故障,Runtime 拒写".into(),
        ));
    }
    let session = w
        .sessions
        .get(&params.session_id)
        .ok_or_else(|| CoreError::validation("session 不存在"))?
        .clone();
    if session.state == SessionState::Closed {
        return Err(CoreError::validation("session 已关闭,不可 resume"));
    }
    let since = params.since_seq.unwrap_or(0);
    let (events, _last, _) = w.events_for_session(&params.session_id, since, u32::MAX);
    let agent_state = w
        .agents
        .get(&session.agent_id)
        .map(|a| a.state)
        .unwrap_or(AgentState::Failed);

    w.emit(
        EventType::SessionResumed,
        Some(params.session_id.clone()),
        None,
        None,
        serde_json::json!({
            "session_id": params.session_id.as_str(),
            "since_seq": since,
            "replayed": events.len(),
        }),
    );

    Ok(SessionResumeResult {
        // M1 无 detached 路径(M3 Surface 断连引入);保持当前态。
        session_state: SessionState::Active,
        agent_state,
        last_event_seq: w.bus.last_seq(),
        events,
    })
}

pub(crate) fn handle_session_close(
    w: &mut World,
    _request_id: BmId,
    params: SessionCloseParams,
) -> CoreResult<SessionCloseResult> {
    if w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "持久层故障,Runtime 拒写".into(),
        ));
    }
    let agent_final_state;
    {
        let session = w
            .sessions
            .get_mut(&params.session_id)
            .ok_or_else(|| CoreError::validation("session 不存在"))?;
        if session.state == SessionState::Closed {
            return Err(CoreError::validation("session 已关闭"));
        }
        session.transition(SessionState::Closed);
        let agent = w.agents.get(&session.agent_id).expect("session 必有 agent");
        agent_final_state = agent.state.as_str().to_string();
    }
    // close 只关会话,不取消进行中的回合(INV-6);in_flight 不动。
    // W5:对话台账随会话关闭清退(历史回喂数据源,内存面随会话寿命)。
    w.session_chats.remove(&params.session_id);
    let reason = params.reason.unwrap_or_else(|| "user_request".into());
    w.emit(
        EventType::SessionClosed,
        Some(params.session_id.clone()),
        None,
        None,
        serde_json::json!({
            "session_id": params.session_id.as_str(),
            "reason": reason,
        }),
    );
    Ok(SessionCloseResult {
        closed_at: w.now_ts(),
        agent_final_state,
    })
}

pub(crate) fn handle_events_poll(
    w: &World,
    params: EventsPollParams,
) -> CoreResult<EventsPollResult> {
    let limit = params.limit.unwrap_or(100).clamp(1, 1000);
    // M5 增发:task_id 过滤(watch 观察面;task 事件不携带 session 关联,
    // 过滤在事件信封 payload.task_id 上执行,wire/session 合同语义)
    if let Some(task_id) = &params.task_id {
        let (events, last_seq, has_more) = w.events_for_task(task_id, params.since_seq, limit);
        return Ok(EventsPollResult {
            events,
            last_seq,
            has_more,
        });
    }
    let (events, last_seq, has_more) =
        w.events_for_session(&params.session_id, params.since_seq, limit);
    Ok(EventsPollResult {
        events,
        last_seq,
        has_more,
    })
}

// ---- 回合 ------------------------------------------------------------------

pub(crate) fn handle_send_input(
    w: &mut World,
    request_id: BmId,
    params: SendInputParams,
) -> CoreResult<Receipt> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障".into(),
        ));
    }
    if params.content.is_empty() || params.content.len() > 100_000 {
        return Err(CoreError::validation("content 长度越界(1..=100000 字节)"));
    }

    let session = w
        .sessions
        .get(&params.session_id)
        .ok_or_else(|| CoreError::validation("session 不存在"))?
        .clone();
    if session.state == SessionState::Closed {
        return Err(CoreError::validation("session 已关闭"));
    }
    let agent = w
        .agents
        .get(&params.agent_id)
        .ok_or_else(|| CoreError::validation("agent 不存在"))?
        .clone();
    if agent.session_id != session.id {
        return Err(CoreError::validation("agent 不属于该 session"));
    }
    if agent.state != AgentState::Running {
        return Err(CoreError::validation("agent 不在可接单状态"));
    }

    // 强制点①(规格 §8.2):预算拒绝不创建 operation。
    match agent.budget.check(true) {
        crate::budget::Verdict::ExceededTokens | crate::budget::Verdict::ExceededTurns => {
            let msg = match agent.budget.check(false) {
                crate::budget::Verdict::ExceededTokens => "剩余预算不足,回合不发起",
                _ => "回合数已用尽,回合不发起",
            };
            w.emit(
                EventType::BudgetExceeded,
                Some(session.id.clone()),
                Some(agent.id.clone()),
                None,
                serde_json::json!({
                    "agent_id": agent.id.as_str(),
                    "scope": BudgetScope::Agent.as_str(),
                    "used_tokens": agent.budget.used_tokens,
                    "limit_tokens": agent.budget.max_tokens,
                }),
            );
            return Err(CoreError::Semantic(ErrorCode::BudgetExceeded, msg.into()));
        }
        crate::budget::Verdict::Allow => {}
    }

    let now = w.now_ts();
    let operation_id = w.config.id_gen.next_id("op");
    let turn_index = agent.budget.turns_used + 1;

    // not_started→running(dispatch_accepted):由收据承载,不发事件(规格 §8.1)。
    let operation = Operation {
        id: operation_id.clone(),
        request_id: request_id.clone(),
        session_id: session.id.clone(),
        agent_id: agent.id.clone(),
        state: OperationState::NotStarted,
        turn_index,
        created_at: now.clone(),
        completed_at: None,
        action_summary: format!("Agent 回合进行中(第 {turn_index} 回)"),
        result_reference: None,
        error: None,
    }
    .dispatch();
    w.operations.insert(operation_id.clone(), operation);

    let model_id0 = agent.model_chain[0].clone();
    w.emit(
        EventType::AgentTurnStarted,
        Some(session.id.clone()),
        Some(agent.id.clone()),
        Some(operation_id.clone()),
        serde_json::json!({
            "agent_id": agent.id.as_str(),
            "operation_id": operation_id.as_str(),
            "turn_index": turn_index,
        }),
    );
    // Execution Log:agent.turn(输入只留摘要,基线 8.4;A4:载荷原文不入日志)
    {
        let digest = Sha256::digest(params.content.as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        w.exec_log.record(crate::exec_log::LogRecord {
            kind: LogKind::AgentTurn,
            session_id: session.id.clone(),
            agent_id: agent.id.clone(),
            operation_id: operation_id.clone(),
            request_id: Some(request_id.clone()),
            agent_state: AgentState::Running.as_str().to_string(),
            detail: serde_json::json!({
                "turn_index": turn_index,
                "input_digest": format!("sha256:{hex}"),
                "input_bytes": params.content.len(),
            }),
            ts: now.clone(),
        });
    }

    // 输入原文入受保护存储(A4:不进事件/日志),供崩溃后 claim 幂等续跑(M2.6)
    #[allow(clippy::collapsible_if)] // 与写穿主路径同构,保持三段式可读
    if let Some(store) = &w.store {
        if let Err(e) = store.save_op_input(operation_id.as_str(), &params.content) {
            tracing::error!(error = %e, op = %operation_id, "输入持久化失败,进入拒写态");
            w.persist_poisoned = true;
            return Err(CoreError::Semantic(
                ErrorCode::Internal,
                "输入持久化失败".into(),
            ));
        }
    }

    // running→waiting_model(model_invoke_issued)
    {
        let a = w.agents.get_mut(&agent.id).expect("存在");
        a.transition(AgentState::WaitingModel);
    }
    w.emit(
        EventType::AgentWaitingModel,
        Some(session.id.clone()),
        Some(agent.id.clone()),
        Some(operation_id.clone()),
        serde_json::json!({
            "agent_id": agent.id.as_str(),
            "operation_id": operation_id.as_str(),
            "model_id": model_id0,
        }),
    );

    // 强制点②(pre_invoke_check):M1 中与①同账本,防御性保留(基线 9.7)。
    if agent.budget.check(false) != crate::budget::Verdict::Allow {
        w.fail_turn(
            &operation_id,
            ErrorCode::BudgetExceeded,
            "模型调用前预算检查未通过".into(),
        );
        return Err(CoreError::Semantic(
            ErrorCode::BudgetExceeded,
            "模型调用前预算检查未通过".into(),
        ));
    }

    spawn_turn(
        w,
        &agent,
        &operation_id,
        params.content,
        params.model_override,
    );

    Ok(w.receipt_of(&w.operations[&operation_id]))
}

pub(crate) fn handle_approval_list(
    w: &mut World,
    params: wire::ApprovalListParams,
) -> CoreResult<serde_json::Value> {
    // A-11(审计台账):列表前置到期扫描。respond() 的就地过期检查只兜
    // 「有人来裁决」的路径;无人问津的滞留项在此收敛,保证待裁决队列
    // 不出现已过期仍可点项(响应路径本身的过期检查保持不变)。
    expire_due_approvals(w);
    // 缺省 = 待裁决队列(waiting_user):审批工作面只关心未决项;
    // 显式 --state 过滤任意状态(wire/capability 合同 description)。
    let state_filter = params
        .state_filter
        .as_deref()
        .unwrap_or(bm_contract::capability::ApprovalState::WaitingUser.as_str());
    let mut rows: Vec<&Approval> = w
        .approvals
        .values()
        .filter(|a| a.state.as_str() == state_filter)
        .collect();
    rows.sort_by(|a, b| a.requested_at.cmp(&b.requested_at));
    let mut approvals = Vec::new();
    for a in rows {
        approvals.push(serde_json::to_value(a).map_err(|_| CoreError::Internal)?);
    }
    Ok(serde_json::json!({ "approvals": approvals }))
}

/// 到期审批扫描(waiting_user 且过 deadline → expired)。返回本次翻转的
/// 审批 id。副作用与 respond() 的 Expired 分支逐项对齐:persist 行、
/// approval.expired 事件、关联 operation 取消(仅当仍处 waiting_approval,
/// 防表外迁移)、清 cap_pending。
pub(crate) fn expire_due_approvals(w: &mut World) -> Vec<BmId> {
    let mut expired: Vec<BmId> = Vec::new();
    {
        let mgr = ApprovalManager::new(&mut w.grants, &*w.config.clock, &*w.config.id_gen);
        for (id, approval) in w.approvals.iter_mut() {
            if mgr.expire_if_due(approval) {
                expired.push(id.clone());
            }
        }
    }
    for id in &expired {
        let op_row = w.cap_pending.get(id).map(|p| p.op_id.clone());
        if let (Some(a), Some(op_id)) = (w.approvals.get(id), op_row.as_ref()) {
            persist_approval(w, a, op_id, None);
        }
        w.emit(
            EventType::ApprovalExpired,
            None,
            None,
            op_row.clone(),
            serde_json::json!({
                "approval_id": id.as_str(),
                "operation_id": op_row.as_ref().map(|o| o.as_str()),
                "expired_at": w.now_ts(),
            }),
        );
        if let Some(op_id) = op_row {
            let still_waiting = w
                .operations
                .get(&op_id)
                .is_some_and(|o| o.state == OperationState::WaitingApproval);
            if still_waiting {
                w.settle_operation(&op_id, OperationState::Cancelled, None);
            }
            w.cap_pending.remove(id);
        }
    }
    expired
}

pub(crate) fn handle_approval_respond(
    w: &mut World,
    request_id: BmId,
    params: wire::ApprovalRespondParams,
) -> CoreResult<serde_json::Value> {
    let decision = match params.decision.as_str() {
        "approve" => RespondDecision::Approve,
        "deny" => RespondDecision::Deny,
        "withdraw" => RespondDecision::Withdraw,
        other => return Err(CoreError::validation(format!("非法 decision: {other}"))),
    };
    let scope = params
        .scope
        .as_deref()
        .map(|s| GrantScope::from_wire(s).ok_or_else(|| CoreError::validation("非法 scope")))
        .transpose()?;
    // M5 解读条款 4 兑现:task:<id> scope 自 Task 对象落地起启用;校验面
    // 仅拒绝引用不存在 Task 的情形(M4 期恒拒的过渡语义移除)
    if let Some(GrantScope::Task(task_id)) = &scope {
        let exists = BmId::parse(task_id.clone())
            .map(|id| w.tasks.contains_key(&id))
            .unwrap_or(false);
        if !exists {
            return Err(CoreError::validation(format!(
                "task scope 引用不存在的 Task: {task_id}"
            )));
        }
    }
    let pending = w.cap_pending.get(&params.approval_id).map(|p| {
        (
            p.op_id.clone(),
            p.capability.clone(),
            p.args.clone(),
            p.idempotency_key.clone(),
            p.principal.clone(),
            p.trust,
        )
    });
    let cap_for_resource = w
        .approvals
        .get(&params.approval_id)
        .map(|a| a.capability.clone())
        .ok_or_else(|| CoreError::validation("未知审批对象"))?;
    // M9 S1:memory.* 审批签发的 Grant 捕获抽屉谓词——批准只覆盖被批准的
    // 那个 scope(资源谓词命中步 4 的 Grant 查表),而非全抽屉能力。
    let mut predicates = serde_json::Map::new();
    if cap_for_resource.starts_with("memory.")
        && let Some(s) = pending
            .as_ref()
            .and_then(|(_, _, args, _, _, _)| args.get("scope"))
            .and_then(|v| v.as_str())
    {
        predicates.insert("scope".to_string(), serde_json::json!(s));
    }
    let resource = bm_contract::capability::GrantResource {
        capability: cap_for_resource,
        args_predicates: predicates,
    };
    let respond_result = {
        let approval = w
            .approvals
            .get_mut(&params.approval_id)
            .ok_or_else(|| CoreError::validation("未知审批对象"))?;
        let mut mgr = ApprovalManager::new(&mut w.grants, &*w.config.clock, &*w.config.id_gen);
        mgr.respond(approval, decision, scope, resource, CAPABILITY_CALLER)
    };
    // 裁决后同步审批行(非 waiting 态剥离重放载荷)
    let op_row_id = pending.as_ref().map(|(op_id, ..)| op_id.clone());
    if let (Some(a), Some(op_id)) = (w.approvals.get(&params.approval_id), op_row_id.as_ref()) {
        persist_approval(w, a, op_id, None);
    }
    let op = pending;
    match respond_result {
        Ok(Some(grant)) => {
            let op_key = op.as_ref().map(|(op_id, ..)| op_id.clone());
            w.emit(
                EventType::GrantCreated,
                None,
                None,
                op_key.clone(),
                serde_json::json!({
                    "grant_id": grant.grant_id,
                    "approval_id": params.approval_id.as_str(),
                    "audience": grant.audience,
                    "action": grant.action,
                    "scope": grant.scope.to_wire(),
                    "delegation_depth": grant.delegation_depth,
                    "expires_at": grant.expires_at,
                    "parent_hash": grant.parent_grant_hash,
                    "resource": serde_json::to_value(&grant.resource).expect("resource 序列化"),
                }),
            );
            // approval.resolved 键集:[approval_id, operation_id, outcome, scope, grant_id]
            w.emit(
                EventType::ApprovalResolved,
                None,
                None,
                op_key.clone(),
                serde_json::json!({
                    "approval_id": params.approval_id.as_str(),
                    "operation_id": op_key.as_ref().map(|o| o.as_str()),
                    "outcome": "approved",
                    "scope": grant.scope.to_wire(),
                    "grant_id": grant.grant_id,
                }),
            );
            // 批准:operation 续行(waiting_approval→running→统一执行助手)
            persist_grant(w, &grant.grant_id);
            if let Some((op_id, capability, args, idem, principal, trust)) = op {
                // P0(第四轮评审):重放前的纵深防护——操作已被取消(或其他
                // 路径终态)时拒绝重放,宁可报错也不踩表外迁移。
                let op_state = w.operations.get(&op_id).map(|o| o.state);
                if !matches!(op_state, Some(OperationState::WaitingApproval)) {
                    w.cap_pending.remove(&params.approval_id);
                    return Err(CoreError::validation(
                        "审批对应的操作已不在等待审批状态(可能已被取消),批准未重放",
                    ));
                }
                w.settle_operation(&op_id, OperationState::Running, None);
                // 重放按原始调用方身份归因(M5 双路径:surface / worker)
                let mut ctx = CallContext::content_chain(&principal, trust)
                    .unwrap_or_else(|_| CallContext::surface(CAPABILITY_CALLER));
                if let Some(k) = idem {
                    ctx = ctx.with_idempotency_key(k);
                }
                let outcome = dispatch_capability(w, &ctx, &capability, args, &op_id);
                match outcome {
                    CallOutcome::Completed { result, .. } => {
                        // W4b 对话内审批:同步批准执行的成果入 op_results,
                        // 供回合任务轮询取回喂模型
                        w.op_results.insert(op_id.clone(), result);
                        w.settle_operation(&op_id, OperationState::Succeeded, None);
                        persist_grant(w, &grant.grant_id);
                        w.cap_pending.remove(&params.approval_id);
                    }
                    CallOutcome::Suppressed { original_result } => {
                        w.op_results.insert(op_id.clone(), original_result);
                        w.settle_operation(&op_id, OperationState::Succeeded, None);
                        persist_grant(w, &grant.grant_id);
                        w.cap_pending.remove(&params.approval_id);
                    }
                    CallOutcome::DispatchedAsync => {
                        // M7 S4:异步执行中;完成经 Cmd::ProviderCall 落定
                        // (收据/Grant 消费态/outbox 均在完成处理器收口)
                        w.cap_pending.remove(&params.approval_id);
                    }
                    CallOutcome::ProviderUnavailable { message } => {
                        // M7 S5:重连超限在批准重放中同样快速失败(unavailable)
                        fail_capability_call(
                            w,
                            &op_id,
                            &capability,
                            &principal,
                            ErrorCode::Unavailable,
                            &message,
                        );
                        w.cap_pending.remove(&params.approval_id);
                    }
                    other => {
                        let code = match &other {
                            CallOutcome::InvalidArgs { .. } => ErrorCode::ValidationFailed,
                            CallOutcome::StaleBinding { .. } => ErrorCode::Unavailable,
                            _ => ErrorCode::Internal,
                        };
                        let message = match &other {
                            CallOutcome::InvalidArgs { message }
                            | CallOutcome::ProviderError { message }
                            | CallOutcome::InvalidOutput { message } => message.clone(),
                            CallOutcome::StaleBinding { .. } => "binding 已切换".into(),
                            _ => "批准后执行失败".into(),
                        };
                        w.settle_operation(
                            &op_id,
                            OperationState::Failed,
                            Some(WireError::new(code, message)),
                        );
                    }
                }
            }
            Ok(serde_json::json!({
                "approval_id": params.approval_id.as_str(),
                "state": "approved",
                "grant_id": grant.grant_id,
                "request_id": request_id.as_str(),
            }))
        }
        Ok(None) => {
            let outcome_str = match decision {
                RespondDecision::Deny => "denied",
                RespondDecision::Withdraw => "withdrawn",
                RespondDecision::Approve => unreachable!("approve 必然返回 Some/Err"),
            };
            let op_key = op.as_ref().map(|(op_id, ..)| op_id.clone());
            w.emit(
                EventType::ApprovalResolved,
                None,
                None,
                op_key.clone(),
                serde_json::json!({
                    "approval_id": params.approval_id.as_str(),
                    "operation_id": op_key.as_ref().map(|o| o.as_str()),
                    "outcome": outcome_str,
                    "scope": null,
                    "grant_id": null,
                }),
            );
            // denied/expired/withdrawn → operation cancelled(基线 §9.6)
            if let Some((op_id, ..)) = op {
                w.settle_operation(&op_id, OperationState::Cancelled, None);
                w.cap_pending.remove(&params.approval_id);
            }
            Ok(serde_json::json!({
                "approval_id": params.approval_id.as_str(),
                "state": outcome_str,
                "grant_id": null,
                "request_id": request_id.as_str(),
            }))
        }
        Err(ApprovalError::Expired) => {
            let op_key = op.as_ref().map(|(op_id, ..)| op_id.clone());
            w.emit(
                EventType::ApprovalExpired,
                None,
                None,
                op_key.clone(),
                serde_json::json!({
                    "approval_id": params.approval_id.as_str(),
                    "operation_id": op_key.as_ref().map(|o| o.as_str()),
                    "expired_at": w.now_ts(),
                }),
            );
            if let Some((op_id, ..)) = op {
                w.settle_operation(&op_id, OperationState::Cancelled, None);
                w.cap_pending.remove(&params.approval_id);
            }
            Err(CoreError::Semantic(
                ErrorCode::ApprovalDenied,
                "审批窗口已过期(等价拒绝)".into(),
            ))
        }
        Err(e) => Err(CoreError::validation(format!("审批裁决失败: {e:?}"))),
    }
}

pub(crate) fn handle_cancel(w: &World, params: CancelParams) -> CoreResult<CancelResult> {
    let op = w
        .operations
        .get(&params.operation_id)
        .ok_or_else(|| CoreError::validation("operation 不存在"))?;
    if op.session_id != params.session_id || op.agent_id != params.agent_id {
        return Err(CoreError::validation("operation 与 session/agent 不匹配"));
    }
    if op.is_terminal() {
        return Err(CoreError::validation("operation 已到终态,不可取消"));
    }
    // 触发取消令牌;真实落定在 TurnEvent::Cancelled(回合边界)。
    if let Some(token) = w.in_flight.get(&params.operation_id) {
        token.cancel();
    }
    Ok(CancelResult {
        accepted: true,
        operation_id: params.operation_id.clone(),
    })
}

pub(crate) fn handle_get_operation(w: &World, params: GetOperationParams) -> CoreResult<Receipt> {
    let op = w
        .operations
        .get(&params.operation_id)
        .ok_or_else(|| CoreError::validation("operation 不存在"))?;
    Ok(w.receipt_of(op))
}

pub(crate) async fn handle_stop(
    w: &mut World,
    rx: &mut mpsc::Receiver<Cmd>,
    reason: String,
    resp: oneshot::Sender<()>,
) {
    w.emit(
        EventType::RuntimeStopping,
        None,
        None,
        None,
        serde_json::json!({ "reason": reason }),
    );
    // 排空:不取消进行中回合(INV-12),等它们自然落定。
    w.draining = true;
    while !w.in_flight.is_empty() {
        match rx.recv().await {
            Some(Cmd::Turn(event)) => handle_turn_event(w, event),
            // W5:排空期回落中的台账回写照常应用(与 Turn 同口径)
            Some(Cmd::RememberTurn {
                session_id,
                user,
                assistant,
            }) => crate::runtime::turn::remember_turn(w, session_id, user, assistant),
            // W4b 排空期审批请求标记:照常透传(与核心循环同一形态)
            Some(Cmd::ApprovalRequested {
                approval_id,
                capability,
                args,
                operation_id,
            }) => {
                let marker = serde_json::json!({
                    "bm_approval_request": {
                        "approval_id": approval_id,
                        "capability": capability,
                        "args": args,
                        "operation_id": operation_id.as_str(),
                    }
                });
                let _ = w.tx.try_send(Cmd::ProviderDelta {
                    operation_id,
                    delta: format!("\n[BM_APPROVAL:{}]\n", marker),
                });
            }
            // 收据查询只读幂等,排空期照常应答(INV-6 精神)。
            Some(Cmd::GetOperation { params, resp }) => {
                let _ = resp.send(handle_get_operation(w, params));
            }
            Some(Cmd::EventsAll { resp }) => {
                let events = match &w.store {
                    Some(store) => store.replay_since(0).unwrap_or_default(),
                    None => w.bus.events().to_vec(),
                };
                let _ = resp.send(events);
            }
            Some(other) => reply_unavailable(other),
            None => break,
        }
    }
    let uptime_ms = w.started_instant.elapsed().as_millis() as u64;
    w.emit(
        EventType::RuntimeStopped,
        None,
        None,
        None,
        serde_json::json!({ "uptime_ms": uptime_ms }),
    );
    w.stopped = true;
    let _ = resp.send(());
}

/// W2 热装载:运行期追加注册能力(MCP 管理面重载)。
///
/// 语义收敛:只增——新 capability 逐条注册(binding 落持久,重启后随
/// --mcp-config 装载自然恢复);同名已存在/注册失败逐条记错,不拖垮批量;
/// 全部失败才整体报错。修改/删除仍走重启(v0 收敛,UI 明示)。
pub(crate) fn handle_capabilities_register(
    w: &mut World,
    entries: Vec<(
        bm_contract::capability::CapabilityManifest,
        std::sync::Arc<dyn crate::registry::CapabilityProvider>,
    )>,
) -> CoreResult<Vec<String>> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝能力注册".into(),
        ));
    }
    let mut registered: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (manifest, provider) in entries {
        let instance = format!("{}@{}", manifest.capability, manifest.version);
        let manifest_json = serde_json::to_string(&manifest).unwrap_or_default();
        let capability = manifest.capability.clone();
        match w.registry.register(manifest, &instance, provider) {
            Ok(_) => {
                if capability.starts_with("mcp.") {
                    w.registry.mark_async(&capability);
                }
                if let Some(store) = &w.store {
                    let _ =
                        store.save_capability_binding(bm_persist::sqlite_state::CapabilityRow {
                            capability: &capability,
                            provider_instance_id: &instance,
                            epoch: 1,
                            status: "active",
                            manifest: &manifest_json,
                            updated_at: &format_ts(w.started_at),
                        });
                }
                registered.push(capability);
            }
            Err(e) => errors.push(format!("{capability}: {e}")),
        }
    }
    if registered.is_empty() && !errors.is_empty() {
        return Err(CoreError::validation(format!(
            "能力注册全部失败: {}",
            errors.join("; ")
        )));
    }
    for e in &errors {
        tracing::warn!("capabilities_register 部分失败: {e}");
    }
    Ok(registered)
}

/// 热拔能力:从逻辑目录和持久层同步摘除。
pub(crate) fn handle_capabilities_unregister(
    w: &mut World,
    capabilities: Vec<String>,
) -> CoreResult<Vec<String>> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝能力注销".into(),
        ));
    }
    let mut removed: Vec<String> = Vec::new();
    for cap in capabilities {
        if w.registry.unregister(&cap) {
            if let Some(store) = &w.store {
                let _ = store.delete_capability_binding(&cap);
            }
            removed.push(cap);
        }
    }
    Ok(removed)
}
