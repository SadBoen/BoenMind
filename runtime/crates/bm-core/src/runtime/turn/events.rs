//! 自 turn.rs 机械移入(内容零改动)。
use super::*;

pub(crate) fn handle_turn_event(w: &mut World, event: TurnEvent) {
    // M9-S3:先捕获回执 ID(事件随即被消费),处理完做自主环裁决
    let autorun_op = turn_event_op(&event).cloned();
    match event {
        TurnEvent::AttemptFailed {
            operation_id,
            model_id,
            attempt,
            error_code,
        } => {
            let (session_id, agent_id, request_id, agent_state) = {
                let Some(op) = w.operations.get(&operation_id) else {
                    tracing::warn!(operation = %operation_id.as_str(), "AttemptFailed: operation 已不存在,忽略事件");
                    w.in_flight.remove(&operation_id);
                    return;
                };
                let Some(a) = w.agents.get(&op.agent_id) else {
                    tracing::warn!(operation = %operation_id.as_str(), agent = %op.agent_id.as_str(), "AttemptFailed: agent 已不存在,忽略事件");
                    w.in_flight.remove(&operation_id);
                    return;
                };
                (
                    op.session_id.clone(),
                    op.agent_id.clone(),
                    op.request_id.clone(),
                    a.state.as_str().to_string(),
                )
            };
            w.emit(
                EventType::ModelInvocationFailed,
                Some(session_id.clone()),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "operation_id": operation_id.as_str(),
                    "agent_id": agent_id.as_str(),
                    "model_id": model_id,
                    "attempt": attempt,
                    "error_code": error_code.as_str(),
                }),
            );
            w.exec_log.record(crate::exec_log::LogRecord {
                kind: LogKind::ModelInvocation,
                session_id,
                agent_id,
                operation_id,
                request_id: Some(request_id),
                agent_state,
                detail: serde_json::json!({
                    "model_id": model_id,
                    "attempt": attempt,
                    "error_code": error_code.as_str(),
                    "stream_interrupted": false,
                }),
                ts: w.now_ts(),
            });
            // M7 S5:失败回账(>=3 连续失败 -> 熔断开闸)
            // P1(第四轮评审):仅故障类(Unavailable)计熔断;鉴权/参数错
            // (PermissionDenied/ValidationFailed)是配置错,不烧熔断器。
            if error_code == ErrorCode::Unavailable {
                note_provider_failure(w, w.config.connector.provider(), "模型调用连续失败");
            }
        }
        TurnEvent::ChainExhausted {
            operation_id,
            error_code,
            detail,
        } => {
            emit_model_call_error_audit(w, &operation_id, error_code);
            // ADR-0029:真实死因进用户可见消息(message 上限 500,截断合规)
            let mut message = format!("模型降级链耗尽({error_code})");
            if let Some(d) = detail.as_deref().filter(|s| !s.trim().is_empty()) {
                message.push_str(": ");
                let remain = 480usize.saturating_sub(message.chars().count());
                message.push_str(&d.chars().take(remain).collect::<String>());
            }
            w.fail_turn(&operation_id, error_code, message);
            w.in_flight.remove(&operation_id);
        }
        TurnEvent::Cancelled { operation_id } => {
            emit_model_call_error_audit(w, &operation_id, ErrorCode::Cancelled);
            // operation: running→cancelled(唯一合法入口 = 显式取消,INV-12)
            let (session_id, agent_id) = {
                let Some(op) = w.operations.get(&operation_id) else {
                    tracing::warn!(operation = %operation_id.as_str(), "Cancelled: operation 已不存在,忽略事件");
                    w.in_flight.remove(&operation_id);
                    return;
                };
                (op.session_id.clone(), op.agent_id.clone())
            };
            {
                if let Some(a) = w.agents.get_mut(&agent_id) {
                    // waiting_model→stopping(explicit_cancel)→stopped(turn_boundary_reached)
                    // P1-19(2026-09-07 架构评审):先查边再迁移——迟到取消不得
                    // assert 打崩进程(恢复/并发边界由 handle.rs 同款守卫兜底)。
                    if AgentState::can_transition(a.state, AgentState::Stopping) {
                        a.transition(AgentState::Stopping);
                        a.transition(AgentState::Stopped);
                    }
                }
            }
            w.settle_operation(
                &operation_id,
                OperationState::Cancelled,
                Some(WireError::new(
                    ErrorCode::Cancelled,
                    "用户显式取消".to_string(),
                )),
            );
            w.emit(
                EventType::AgentCancelled,
                Some(session_id),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": operation_id.as_str(),
                }),
            );
            w.in_flight.remove(&operation_id);
        }
        TurnEvent::Completed {
            operation_id,
            model_id,
            attempt,
            content,
            usage_in,
            usage_out,
            latency_ms,
            stream_interrupted,
        } => {
            autorun_note_completed(w, &operation_id, &content);
            let (session_id, agent_id, request_id, agent_state) = {
                let Some(op) = w.operations.get(&operation_id) else {
                    tracing::warn!(operation = %operation_id.as_str(), "Completed: operation 已不存在,忽略事件");
                    w.in_flight.remove(&operation_id);
                    return;
                };
                let Some(a) = w.agents.get(&op.agent_id) else {
                    tracing::warn!(operation = %operation_id.as_str(), agent = %op.agent_id.as_str(), "Completed: agent 已不存在,忽略事件");
                    w.in_flight.remove(&operation_id);
                    return;
                };
                (
                    op.session_id.clone(),
                    op.agent_id.clone(),
                    op.request_id.clone(),
                    a.state.as_str().to_string(),
                )
            };
            w.emit(
                EventType::ModelInvocationCompleted,
                Some(session_id.clone()),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "operation_id": operation_id.as_str(),
                    "agent_id": agent_id.as_str(),
                    "model_id": model_id,
                    "attempt": attempt,
                    "usage_in": usage_in,
                    "usage_out": usage_out,
                    "latency_ms": latency_ms,
                    "stream_interrupted": stream_interrupted,
                    // M8.1 修复:回答正文入事件(截断走 limits,防日志膨胀;
                    // 截断标记如实)——正文此前无处落地,用户面不可见
                    "content": content_trunc_with(
                        &content,
                        w.config.limits.get().audit_entry_max_chars,
                    ),
                    "content_truncated": content.len()
                        > w.config.limits.get().audit_entry_max_chars,
                }),
            );
            // M7 S5:成功回账(清计数/半开恢复 healthy)
            note_provider_success(w, w.config.connector.provider(), "模型调用成功");
            // M7 S1:模型调用审计(Broker 路径与普通能力调用同享 capability.invoked 面)
            if let Some(a) = w.model_call_audit.remove(&operation_id) {
                emit_capability_invoked_with(
                    w,
                    &a.call_id,
                    &operation_id,
                    "model.invoke",
                    &a.principal,
                    Some(a.epoch),
                    Some(&a.instance_id),
                    "ok",
                    None,
                    None,
                );
            }
            w.exec_log.record(crate::exec_log::LogRecord {
                kind: LogKind::ModelInvocation,
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                operation_id: operation_id.clone(),
                request_id: Some(request_id),
                agent_state,
                detail: serde_json::json!({
                    "model_id": model_id,
                    "attempt": attempt,
                    "usage": {"tokens_in": usage_in, "tokens_out": usage_out},
                    "latency_ms": latency_ms,
                    "stream_interrupted": stream_interrupted,
                }),
                ts: w.now_ts(),
            });

            // waiting_model→running(model_response_ok)
            {
                if let Some(a) = w.agents.get_mut(&agent_id) {
                    // P1-19:边守卫(终止态回 Running 的迟到事件只记日志不崩进程)
                    if AgentState::can_transition(a.state, AgentState::Running) {
                        a.transition(AgentState::Running);
                    } else {
                        tracing::warn!(agent = %agent_id.as_str(), state = ?a.state, "回合完成事件迟到,agent 状态未迁移");
                    }
                }
            }

            // 强制点③(post_invoke_accounting)
            let turn_index = match w.operations.get(&operation_id) {
                Some(op) => op.turn_index,
                None => {
                    w.in_flight.remove(&operation_id);
                    return;
                }
            };
            let (ratio, warn, exceeded) = {
                let Some(a) = w.agents.get_mut(&agent_id) else {
                    w.in_flight.remove(&operation_id);
                    return;
                };
                a.budget.account(usage_in.saturating_add(usage_out))
            };
            let (used, limit) = match w.agents.get(&agent_id) {
                Some(a) => (a.budget.used_tokens, a.budget.max_tokens),
                None => (0, 0),
            };
            w.exec_log.record(crate::exec_log::LogRecord {
                kind: LogKind::BudgetCheck,
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                operation_id: operation_id.clone(),
                request_id: None,
                agent_state: AgentState::Running.as_str().to_string(),
                detail: serde_json::json!({
                    "scope": BudgetScope::Agent.as_str(),
                    "used_tokens": used,
                    "limit_tokens": limit,
                    "ratio": ratio,
                }),
                ts: w.now_ts(),
            });
            if warn {
                w.emit(
                    EventType::BudgetWarning,
                    Some(session_id.clone()),
                    Some(agent_id.clone()),
                    None,
                    serde_json::json!({
                        "agent_id": agent_id.as_str(),
                        "scope": BudgetScope::Agent.as_str(),
                        "used_tokens": used,
                        "limit_tokens": limit,
                        "ratio": ratio,
                    }),
                );
            }
            if exceeded {
                w.emit(
                    EventType::BudgetExceeded,
                    Some(session_id.clone()),
                    Some(agent_id.clone()),
                    None,
                    serde_json::json!({
                        "agent_id": agent_id.as_str(),
                        "scope": BudgetScope::Agent.as_str(),
                        "used_tokens": used,
                        "limit_tokens": limit,
                    }),
                );
            }

            // running→succeeded(result_recorded)+ agent.completed
            {
                let now = w.now_ts();
                if let Some(op) = w.operations.get_mut(&operation_id) {
                    op.action_summary =
                        format!("回合 {turn_index} 完成({usage_in} 入 / {usage_out} 出 token)");
                    op.result_reference = Some(wire::ResultReference {
                        kind: wire::ResultRefKind::ExecutionLog,
                        r#ref: format!("log:{operation_id}"),
                    });
                }
                let _ = now;
            }
            w.settle_operation(&operation_id, OperationState::Succeeded, None);
            w.emit(
                EventType::AgentCompleted,
                Some(session_id),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": operation_id.as_str(),
                    "turn_index": turn_index,
                    "content": content,
                }),
            );
            w.in_flight.remove(&operation_id);
        }
    }
    if let Some(op) = autorun_op {
        autorun_pump(w, &op);
    }
    /// M9-S3:TurnEvent → 回执 ID(自主环裁决入口用)。
    fn turn_event_op(e: &TurnEvent) -> Option<&BmId> {
        match e {
            TurnEvent::Completed { operation_id, .. }
            | TurnEvent::Cancelled { operation_id }
            | TurnEvent::AttemptFailed { operation_id, .. }
            | TurnEvent::ChainExhausted { operation_id, .. } => Some(operation_id),
        }
    }
}
