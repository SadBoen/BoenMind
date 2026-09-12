//! 会话/审批/操作/停止处理器(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

use super::*;

/// Provider 熔断健康快照(issue #12):内存视图投影,按 provider 名排序
/// (稳定输出);仅含已发生调用失败的 provider(健康态不建条目)。
/// 管理面读模型,不入合同。
pub(crate) fn handle_provider_health(w: &World) -> Vec<(String, super::ProviderHealth)> {
    let mut items: Vec<(String, super::ProviderHealth)> = w
        .provider_health
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items
}

/// 会话目录列表():内存视图投影,最近活跃在前;
/// updated_at 缺失时回落创建时间。管理面读模型,不入合同。
pub(crate) fn handle_session_list(w: &World) -> Vec<crate::state::SessionSummary> {
    let mut items: Vec<crate::state::SessionSummary> = w
        .sessions
        .values()
        .map(|s| crate::state::SessionSummary {
            id: s.id.as_str().to_string(),
            state: s.state.as_str().to_string(),
            title: s.title.clone(),
            created_at: s.created_at.clone(),
            updated_at: Some(s.updated_at.clone().unwrap_or_else(|| s.created_at.clone())),
            permission_mode: s.permission_mode.as_str().to_string(),
        })
        .collect();
    items.sort_unstable_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    items
}

/// 会话权限模式变更(ADR-0030 决策 1/2):更新服务端会话状态并落
/// session.mode.changed 事实事件(物化投影持久,重启装载)。会话不存在
/// 或模式非法由调用方/解析层拒绝;单写者通道内执行。
pub(crate) fn handle_session_set_mode(
    w: &mut World,
    session_id: BmId,
    mode: PermissionMode,
) -> CoreResult<serde_json::Value> {
    let session = w
        .sessions
        .get_mut(&session_id)
        .ok_or_else(|| CoreError::validation(format!("未知会话: {}", session_id.as_str())))?;
    let from = session.permission_mode;
    if from == mode {
        // 幂等:同值设置不发事件不落审计噪音,直接确认
        return Ok(serde_json::json!({
            "session_id": session_id.as_str(),
            "permission_mode": mode.as_str(),
            "changed": false,
        }));
    }
    session.permission_mode = mode;
    w.emit(
        EventType::SessionModeChanged,
        Some(session_id.clone()),
        None,
        None,
        serde_json::json!({
            "session_id": session_id.as_str(),
            "from": from.as_str(),
            "to": mode.as_str(),
        }),
    );
    Ok(serde_json::json!({
        "session_id": session_id.as_str(),
        "permission_mode": mode.as_str(),
        "changed": true,
    }))
}

pub(crate) fn handle_session_create(
    w: &mut World,
    _request_id: BmId,
    params: SessionCreateParams,
) -> CoreResult<SessionCreateResult> {
    w.gate_writes("新会话")?;
    let spec = &params.agent;
    if spec.name.is_empty() || spec.name.len() > 64 || spec.model_chain.is_empty() {
        return Err(CoreError::validation("agent 描述不完整"));
    }
    for m in &spec.model_chain {
        bm_contract::connector::validate_model_id(m).map_err(CoreError::validation)?;
    }
    // W8(ADR-0018):会话绑定工作区必须已登记(注册表 = config/workspaces.json,
    // 管理面写盘、核心只读)。未配置 data_dir(纯内存测试态)时登记表恒空,
    // 显式绑定一律拒绝——绑定必须真实可解析,不做「看起来能选」的假接受。
    if let Some(wid) = &spec.workspace_id {
        w.validate_workspace(wid)?;
    }

    let now = w.now_ts();
    let session_id = w.config.id_gen.next_id("sess");
    let agent_id = w.config.id_gen.next_id("agent");

    let mut session = Session {
        id: session_id.clone(),
        agent_id: agent_id.clone(),
        state: SessionState::Created,
        created_at: now.clone(),
        workspace_id: spec.workspace_id.clone(),
        // 会话目录():初值 updated_at = created_at;
        // 标题待首条用户消息回填(内容不在事件面,core 直写)
        title: None,
        updated_at: Some(now.clone()),
        // ADR-0030 决策 1:新会话默认 ask(变更前确认),服务端权威
        permission_mode: PermissionMode::Ask,
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
            allowed_tools: spec.allowed_tools.clone(),
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

    // 重启续聊配套():创建即绑定的工作目录落持久行
    // (SessionCreated 事件载荷不含绑定,投影由本处直写)。
    if w.sessions[&session_id].workspace_id.is_some()
        && let Some(store) = w.store.clone()
    {
        let wid = w.sessions[&session_id].workspace_id.clone();
        if let Err(e) = store.save_session_workspace(session_id.as_str(), wid.as_deref()) {
            tracing::error!(error = %e, session = %session_id.as_str(), "创建期工作区绑定落库失败,进入拒写态");
            w.persist_poisoned = true;
        }
    }
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
    w.emit_grant_created(&mg, None, None);

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
    let (events, _last, _) = w.events_for_session(&params.session_id, since, u32::MAX)?;
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
        agent_id: session.agent_id.clone(),
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
    w.session_turn_totals.remove(&params.session_id);
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

/// session.delete():会话删除 = 墓碑 + 原文擦除。
/// ①内存台账清场(sessions/agents/chats/totals;live 会话直接移除,不设中间态);
/// ②持久侧单事务:墓碑(防事件重放复活)+ operations.input_content 擦除
/// (用户原文不留,操作元数据行保留供审计,对齐 A4 精神);
/// ③context-log 流式过滤该会话行(临时文件+fsync+rename,内存 O(1))。
/// events.jsonl 不动(仅元数据,seq 连续不变量);不可恢复。
pub(crate) fn handle_session_delete(
    w: &mut World,
    request_id: BmId,
    params: wire::SessionDeleteParams,
) -> CoreResult<wire::SessionDeleteResult> {
    w.gate_writes("删除")?;
    let session_id = params.session_id.clone();
    let Some(session) = w.sessions.remove(&session_id) else {
        return Err(CoreError::validation("session 不存在"));
    };
    let _ = request_id;
    // 会话内未终态 operation 一并清场(其收据随会话消失)
    let session_ops: Vec<BmId> = w
        .operations
        .values()
        .filter(|o| o.session_id == session_id)
        .map(|o| o.id.clone())
        .collect();
    for op_id in &session_ops {
        w.operations.remove(op_id);
        w.op_capability.remove(op_id);
        w.op_results.remove(op_id);
        if let Some(token) = w.in_flight.remove(op_id) {
            token.cancel(); // 在途回合:令牌取消,回合边界自会因载体消失而中止
        }
    }
    w.session_chats.remove(&session_id);
    w.session_turn_totals.remove(&session_id);
    // Agent 实体一并清场():SQLite 侧 agents 行已随会话
    // 删除,内存遗留即幽灵 Agent——常驻增长,且按 agent_id 查询会误判其活跃。
    w.agents.remove(&session.agent_id);

    let now = w.now_ts();
    // ①墓碑 + ②原文清空(单事务;失败入拒写态,防半删状态)
    if let Some(store) = w.store.clone()
        && let Err(e) = store.erase_session_contents(session_id.as_str(), &now)
    {
        tracing::error!(error = %e, session = %session_id.as_str(), "会话删除持久侧效失败,进入拒写态");
        w.persist_poisoned = true;
        return Err(CoreError::Semantic(
            ErrorCode::Internal,
            "会话删除持久侧效失败".into(),
        ));
    }
    // ③context-log 过滤(会话已从内存移除)
    let purged = match &w.config.data_dir {
        Some(dir) => {
            crate::ports::persist::filter_lines_atomic(&dir.join("context-log.jsonl"), |line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .map(|v| {
                        v.get("session_id").and_then(|s| s.as_str()) == Some(session_id.as_str())
                    })
                    .unwrap_or(false)
            })
            .map_err(|e| {
                tracing::error!(error = %e, session = %session_id.as_str(), "context-log 擦除失败");
                w.persist_poisoned = true;
                CoreError::Semantic(ErrorCode::Internal, "context-log 擦除失败".into())
            })?
        }
        None => 0,
    };
    // 持久层 sessions/agents 行删除(墓碑已在,事件重放亦不复活;若会话行仍在持久层则 DELETE)
    if let Some(store) = w.store.clone()
        && let Err(e) = store.delete_session_rows(session_id.as_str())
    {
        // 墓碑已在事务1落定,残留行重启不复活;此处失败须留观测点不可静默
        tracing::warn!(error = %e, session = %session_id.as_str(), "会话行删除失败(墓碑在,重启不复活)");
    }
    tracing::info!(session = %session_id.as_str(), purged, "会话已删除(墓碑+原文擦除)");
    Ok(wire::SessionDeleteResult {
        deleted_at: now,
        purged_lines: purged as u64,
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
        let (events, last_seq, has_more) = w.events_for_task(task_id, params.since_seq, limit)?;
        return Ok(EventsPollResult {
            events,
            last_seq,
            has_more,
        });
    }
    let (events, last_seq, has_more) =
        w.events_for_session(&params.session_id, params.since_seq, limit)?;
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
        // 失败自愈(合同增发 failed→running,resend_after_failure):回合失败
        // ≠agent 死亡,同会话再次发消息即恢复接单;发 agent.resumed 同步投影。
        // 其余状态(取消/停止/进行中)照旧拒绝。
        if agent.state == AgentState::Failed {
            if let Some(a) = w.agents.get_mut(&params.agent_id) {
                a.transition(AgentState::Running);
            }
            w.emit(
                EventType::AgentResumed,
                Some(session.id.clone()),
                Some(agent.id.clone()),
                None,
                serde_json::json!({
                    "agent_id": agent.id.as_str(),
                    "operation_id": serde_json::Value::Null,
                }),
            );
        } else {
            return Err(CoreError::validation("agent 不在可接单状态"));
        }
    }

    // W8(ADR-0018):本回合工作区覆盖(对话级热切换,model_override 同款)。
    // 校验通过即更新会话绑定;未登记 id 拒绝,不静默沿用旧值。
    if let Some(wid) = &params.workspace_override {
        w.validate_workspace(wid)?;
        if session.workspace_id.as_deref() != Some(wid.as_str()) {
            if let Some(s) = w.sessions.get_mut(&params.session_id) {
                s.workspace_id = Some(wid.clone());
            }
            // 重启续聊配套():绑定落持久行,失败入拒写态
            if let Some(store) = w.store.clone()
                && let Err(e) =
                    store.save_session_workspace(params.session_id.as_str(), Some(wid.as_str()))
            {
                tracing::error!(error = %e, session = %params.session_id.as_str(), "工作区绑定落库失败,进入拒写态");
                w.persist_poisoned = true;
            }
        }
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
        let digest_hex = bm_contract::hash::sha256_hex(params.content.as_bytes());
        w.exec_log.record(crate::exec_log::LogRecord {
            kind: LogKind::AgentTurn,
            session_id: session.id.clone(),
            agent_id: agent.id.clone(),
            operation_id: operation_id.clone(),
            request_id: Some(request_id.clone()),
            agent_state: AgentState::Running.as_str().to_string(),
            detail: serde_json::json!({
                "turn_index": turn_index,
                "input_digest": format!("sha256:{digest_hex}"),
                "input_bytes": params.content.len(),
            }),
            ts: now.clone(),
        });
    }
    // 会话历史回放():用户消息逐条入上下文日志(kind=user_message,
    // 与 assistant_final 同流),供 /admin/sessions/{id}/messages 按 seq 重放。
    // A4 口径不变:事件面仍只留摘要;诊断日志面按快照口径 16K 截断。
    {
        let truncated = crate::runtime::turn::content_trunc_with(
            &params.content,
            w.config.limits.get().audit_entry_max_chars,
        );
        w.ctx_log.record_event(
            session.id.as_str(),
            operation_id.as_str(),
            turn_index,
            "user_message",
            &now,
            serde_json::json!({
                "content": truncated,
                "content_truncated": truncated != params.content,
            }),
        );
    }

    // 会话目录标题回填():首条用户消息即标题(截断),
    // 只在尚无标题时生效(内存守卫 + SQL COALESCE 双重幂等);失败入拒写态
    //(sessions 表 = 规范状态,静默内存-库漂移即破坏投影纪律)。
    if session.title.is_none() {
        let title = crate::runtime::turn::session_title_from(&params.content);
        if let Some(s) = w.sessions.get_mut(&params.session_id) {
            s.title = Some(title.clone());
        }
        if let Some(store) = w.store.clone()
            && let Err(e) = store.backfill_session_meta(session.id.as_str(), Some(&title), None)
        {
            tracing::error!(error = %e, session = %session.id.as_str(), "会话标题回填落库失败,进入拒写态");
            w.persist_poisoned = true;
        }
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
        // P1-19():边守卫——表外迁移记日志不 panic。
        if AgentState::can_transition(a.state, AgentState::WaitingModel) {
            a.transition(AgentState::WaitingModel);
        } else {
            tracing::warn!(agent = %agent.id.as_str(), state = ?a.state, "发模型前 agent 状态异常,未迁移 waiting_model");
        }
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

pub(crate) fn handle_capability_list(
    w: &World,
    params: wire::CapabilityListParams,
) -> CoreResult<wire::CapabilityListResult> {
    let mut discovered = w.registry.discover();
    if let Some(ref provider) = params.provider {
        discovered.retain(|c| &c.provider == provider);
    }
    let capabilities = discovered
        .into_iter()
        .map(|c| serde_json::to_value(c).unwrap_or(serde_json::Value::Null))
        .collect();
    Ok(wire::CapabilityListResult { capabilities })
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
    source: crate::approval::ResolvedSource,
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
    // ADR-0038:抽屉式授权的 Grant 捕获 scope 谓词——批准只覆盖被批准的
    // 那个 scope(资源谓词命中步 4 的 Grant 查表),而非全抽屉能力。
    // 判据 = manifest 声明了 `authorization.drawer`(能力族由合同表达,不再硬编码
    // 能力名前缀——原 `starts_with("memory.")` 属族感知残留,ADR-0038 已裁
    // 规则本体的真源是 manifest)。
    let declares_drawer = w
        .registry
        .manifest_of(&cap_for_resource)
        .and_then(|m| m.authorization.as_ref())
        .and_then(|a| a.drawer.as_ref())
        .is_some();
    let mut predicates = serde_json::Map::new();
    if declares_drawer
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
    // ADR-0030:裁决来源落审计(对象字段 + resolved 事件 source 键)
    if let Some(a) = w.approvals.get_mut(&params.approval_id) {
        a.resolved_source = Some(source.as_str().to_string());
    }
    // 裁决后同步审批行(非 waiting 态剥离重放载荷)
    let op_row_id = pending.as_ref().map(|(op_id, ..)| op_id.clone());
    if let (Some(a), Some(op_id)) = (w.approvals.get(&params.approval_id), op_row_id.as_ref()) {
        persist_approval(w, a, op_id, None);
    }
    let op = pending;
    match respond_result {
        Ok(Some(grant)) => {
            let op_key = op.as_ref().map(|(op_id, ..)| op_id.clone());
            w.emit_grant_created(&grant, Some(params.approval_id.as_str()), op_key.clone());
            // approval.resolved 键集:[approval_id, operation_id, outcome, scope, grant_id, source]
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
                    "source": source.as_str(),
                }),
            );
            // 批准:operation 续行(waiting_approval→running→统一执行助手)
            persist_grant(w, &grant.grant_id);
            if let Some((op_id, capability, args, idem, principal, trust)) = op {
                // P0():重放前的纵深防护——操作已被取消(或其他
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
                    "source": source.as_str(),
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

/// 排空超时的强制收尾:取消全部在途回合并落 Cancelled(`why` 进告警日志)。
fn force_cancel_in_flight(w: &mut World, why: &str) {
    tracing::warn!(in_flight_count = w.in_flight.len(), "{why}");
    let in_flight_items: Vec<_> = w.in_flight.drain().collect();
    for (op_id, token) in in_flight_items {
        token.cancel();
        w.settle_operation(
            &op_id,
            OperationState::Cancelled,
            Some(WireError::new(
                ErrorCode::Cancelled,
                "Runtime 停机排空超时,强制取消",
            )),
        );
    }
}

pub(crate) fn handle_cancel(w: &mut World, params: CancelParams) -> CoreResult<CancelResult> {
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
    // 取消意图持久化():显式取消若在回合边界落定前
    // 遇到崩溃,恢复端凭标记走 Resuming→Stopped(turn_was_stopping)边,
    // 不把已取消的回合复活重跑。写失败 = 拒写态(与 save_op_input 同纪律)。
    if let Some(store) = &w.store
        && let Err(e) = store.mark_op_cancelled(params.operation_id.as_str(), &w.now_ts())
    {
        tracing::error!(error = %e, op = %params.operation_id.as_str(), "取消标记持久化失败,进入拒写态");
        w.persist_poisoned = true;
        return Err(CoreError::Semantic(
            ErrorCode::Internal,
            "取消标记持久化失败".into(),
        ));
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

pub(crate) fn handle_operation_cancel(
    w: &mut World,
    operation_id: BmId,
) -> CoreResult<CancelResult> {
    let op = w
        .operations
        .get(&operation_id)
        .ok_or_else(|| CoreError::validation("operation 不存在"))?;
    let params = CancelParams {
        session_id: op.session_id.clone(),
        agent_id: op.agent_id.clone(),
        operation_id,
    };
    handle_cancel(w, params)
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
    // 排空:等在途回合自然落定,但设硬顶超时(默认10s),防坏任务永久挂死停机/升级回路
    w.draining = true;
    let drain_deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !w.in_flight.is_empty() {
        let remaining_time = drain_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining_time.is_zero() {
            force_cancel_in_flight(
                w,
                "Runtime handle_stop 排空超时(10s),强制取消在途回合并结束",
            );
            break;
        }

        let cmd_opt = match tokio::time::timeout(remaining_time, rx.recv()).await {
            Ok(cmd) => cmd,
            Err(_) => {
                force_cancel_in_flight(w, "Runtime handle_stop 排空等待超时,强制清理剩余在途回合");
                break;
            }
        };

        match cmd_opt {
            Some(Cmd::Turn(event)) => handle_turn_event(w, event),
            // W5:排空期回落中的台账回写照常应用(与 Turn 同口径)
            Some(Cmd::RememberTurn {
                session_id,
                user,
                assistant,
            }) => crate::runtime::turn::remember_turn(w, session_id, user, assistant),
            // 收据查询只读幂等,排空期照常应答(INV-6 精神)。
            Some(Cmd::GetOperation { params, resp }) => {
                let _ = resp.send(handle_get_operation(w, params));
            }
            Some(Cmd::EventsAll { resp }) => {
                let events = match &w.store {
                    Some(store) => match store.replay_since(0) {
                        Ok(events) => events,
                        Err(e) => {
                            tracing::warn!(error = %e, "事件流重放失败,轨迹视图降级为空");
                            Vec::new()
                        }
                    },
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
/// 语义收敛:只增——新 capability 逐条注册(binding 落持久,重启后随
/// --mcp-config 装载自然恢复);同名已存在/注册失败逐条记错,不拖垮批量;
/// 全部失败才整体报错。修改/删除仍走重启(v0 收敛,UI 明示)。
/// binding_epoch 连续性():重新注册以「持久行 max+1」
/// 抬升代际——注销侧只留墓碑行,重载前后 (epoch, instance) 可对账。
pub(crate) fn handle_capabilities_register(
    w: &mut World,
    entries: Vec<(
        bm_contract::capability::CapabilityManifest,
        std::sync::Arc<dyn crate::registry::CapabilityProvider>,
    )>,
) -> CoreResult<Vec<String>> {
    w.gate_writes("能力注册")?;
    // 持久 epoch 快照先行:逐条查库是 N+1,且落库后的行会污染后续代际基线。
    // 读取失败不得静默继续——会把代际重置回 1(与落库失败同口径置毒拒绝)。
    let persisted_epochs: std::collections::HashMap<String, u64> = match &w.store {
        Some(store) => match store.list_capability_bindings() {
            Ok(rows) => rows
                .iter()
                .filter_map(|row| {
                    Some((
                        row["capability"].as_str()?.to_string(),
                        row["epoch"].as_u64()?,
                    ))
                })
                .collect(),
            Err(e) => {
                tracing::error!(error = %e, "capability binding 快照读取失败,进入拒写态");
                w.persist_poisoned = true;
                return Err(CoreError::Internal);
            }
        },
        None => Default::default(),
    };
    let mut registered: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (manifest, provider) in entries {
        let instance = format!("{}@{}", manifest.capability, manifest.version);
        let manifest_json = serde_json::to_string(&manifest).unwrap_or_default();
        let capability = manifest.capability.clone();
        let provider_id = manifest.provider.clone();
        match w
            .registry
            .register(manifest.clone(), &instance, provider.clone())
        {
            Ok(_) => {
                // 代际抬升:已有持久行(含注销墓碑)= max+1;全新能力 = 1。
                let target = persisted_epochs
                    .get(&capability)
                    .map(|e| e.saturating_add(1))
                    .unwrap_or(1);
                let effective = w.registry.restore_binding(
                    manifest,
                    &instance,
                    target,
                    crate::registry::BindingStatus::Active,
                );
                // restore_binding 按「可丢失缓存」语义清空句柄,重新 attach。
                let _ = w.registry.attach_handle(&capability, provider);
                // 异步分道判定与启动注册同源:以 manifest.execution_mode 声明为唯一真源
                // (ADR-0036/0054,内核不认识 provider 命名前缀)。
                w.registry.mark_async_for(&capability, &provider_id);
                if let Some(store) = w.store.clone()
                    && let Err(e) =
                        store.save_capability_binding(crate::ports::persist::CapabilityRow {
                            capability: &capability,
                            provider_instance_id: &instance,
                            epoch: effective,
                            status: "active",
                            manifest: &manifest_json,
                            updated_at: &format_ts(w.started_at),
                        })
                {
                    tracing::error!(error = %e, capability = %capability, "能力 binding 落库失败,进入拒写态");
                    w.persist_poisoned = true;
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

/// 热拔能力:内存面摘除;持久行墓碑化(status=unavailable)而非物理删除——
/// 行是 binding_epoch 代际连续性的唯一跨重启载体(
pub(crate) fn handle_capabilities_unregister(
    w: &mut World,
    capabilities: Vec<String>,
) -> CoreResult<Vec<String>> {
    w.gate_writes("能力注销")?;
    let mut removed: Vec<String> = Vec::new();
    for cap in capabilities {
        // ADR-0037:有在途异步调用 -> 进排空(拒新调用,待全部落定后摘除),
        // 不在在途调用中途拔路由;无在途 -> 直接摘除(现状)。
        let in_flight: std::collections::HashSet<BmId> = w
            .op_async_meta
            .iter()
            .filter(|(_, m)| m.capability == cap)
            .map(|(id, _)| id.clone())
            .collect();
        if !in_flight.is_empty() {
            if w.registry.begin_drain(&cap).is_ok() {
                w.draining_caps.insert(cap.clone(), in_flight);
                removed.push(cap);
            }
            continue;
        }
        if w.registry.binding_of(&cap).is_some() {
            crate::runtime::turn::remove_capability_binding(w, &cap);
            removed.push(cap);
        }
    }
    Ok(removed)
}
