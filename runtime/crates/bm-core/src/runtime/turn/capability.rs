//! 自 turn.rs 机械移入(内容零改动)。
use super::*;

pub(crate) fn persist_grant(w: &mut World, grant_id: &str) {
    let Some(store) = w.store.clone() else {
        return;
    };
    let Some(grant) = w.grants.get(grant_id).cloned() else {
        return;
    };
    let (used, revoked) = w.grants.entry_state(grant_id).unwrap_or((0, false));
    if let Err(e) = store.save_grant(crate::ports::persist::GrantRow {
        id: grant.grant_id.as_str(),
        audience: grant.audience.as_str(),
        action: grant.action.as_str(),
        revocation_version: grant.revocation_version,
        revoked: revoked || used >= 1 && matches!(grant.scope, GrantScope::Once),
        used_count: used,
        payload: &serde_json::to_string(&grant).unwrap_or_default(),
        created_at: grant.created_at.as_str(),
    }) {
        tracing::error!(error = %e, grant = %grant_id, "Grant 行落库失败,进入拒写态");
        w.persist_poisoned = true;
    }
}
pub(crate) fn capability_scope_choices() -> Vec<GrantScope> {
    vec![
        GrantScope::Once,
        GrantScope::Count(5),
        GrantScope::Ttl(3_600_000),
    ]
}
pub(crate) fn handle_capability_call(
    w: &mut World,
    request_id: BmId,
    params: wire::CapabilityCallParams,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝能力调用".into(),
        ));
    }
    // 直路径(Wire Surface):trusted 直调;幂等键随合同参数面挂链
    // (M7-T3 修复:此前仅 worker 路径挂键,Wire 直调的 idempotency_key 被忽略)
    let mut ctx = CallContext::surface(CAPABILITY_CALLER);
    if let Some(k) = &params.idempotency_key {
        ctx = ctx.with_idempotency_key(k);
    }
    capability_call_inner(w, request_id, ctx, params).1
}
pub(crate) fn capability_call_inner(
    w: &mut World,
    request_id: BmId,
    ctx: CallContext,
    params: wire::CapabilityCallParams,
) -> (BmId, CoreResult<serde_json::Value>) {
    // 步 1-4:查表裁决(Broker 为字段级临时借用,用后即还)
    let decision = {
        let broker = Broker::new(
            &w.registry,
            &mut w.grants,
            &*w.config.clock,
            &*w.config.id_gen,
        );
        broker.decide(&ctx, &params.capability, &params.args)
    };
    // operation 载体:系统容器上的内存操作(M4 能力调用不依赖 Session/Agent;
    // 规范状态由 approvals/grants 承载,operations 表不落行——回看复核项)
    let op_id = w.config.id_gen.next_id("op");
    w.op_capability
        .insert(op_id.clone(), params.capability.clone());
    let created_at = w.now_ts();
    let operation = Operation {
        id: op_id.clone(),
        request_id: request_id.clone(),
        session_id: w.system_session.clone(),
        agent_id: w.system_agent.clone(),
        state: bm_contract::states::OperationState::NotStarted,
        turn_index: 0,
        created_at: created_at.clone(),
        completed_at: None,
        action_summary: format!("能力调用 {}", params.capability),
        result_reference: None,
        error: None,
    };
    w.operations.insert(op_id.clone(), operation.dispatch());

    let outcome = match decision {
        Decision::Allowed { grant_id } => {
            // 统一执行助手:副作用前门禁(intent)+ 幂等抑制 + 结果事件
            let outcome =
                dispatch_capability(w, &ctx, &params.capability, params.args.clone(), &op_id);
            match outcome {
                CallOutcome::Completed {
                    call_id,
                    credential,
                    result,
                    ..
                } => {
                    let completed_at = w.now_ts();
                    w.settle_operation(&op_id, OperationState::Succeeded, None);
                    // Grant 消费态落行(Once 消费即 revoked,重启后不复活)
                    if let Some(gid) = &grant_id {
                        persist_grant(w, gid);
                    }
                    let _ = (call_id, credential);
                    Ok(serde_json::json!({
                        "operation_id": op_id.as_str(),
                        "request_id": request_id.as_str(),
                        "principal": ctx.principal.clone(),
                        "capability": params.capability,
                        "state": "succeeded",
                        "created_at": created_at,
                        "completed_at": completed_at,
                        "action_summary": format!("能力 {} 执行完成", params.capability),
                        "result_reference": null,
                        "error": null,
                        "grant_used": grant_id,
                        "result": result,
                    }))
                }
                CallOutcome::InvalidArgs { message } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::ValidationFailed,
                        &message,
                    );
                    Err(CoreError::Semantic(ErrorCode::ValidationFailed, message))
                }
                CallOutcome::StaleBinding { expected_epoch, .. } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::Unavailable,
                        &format!("binding 已切换(凭证 epoch {expected_epoch}),请重试"),
                    );
                    Err(CoreError::Semantic(
                        ErrorCode::Unavailable,
                        "Provider binding 已切换,请重试".into(),
                    ))
                }
                CallOutcome::ProviderError { message } | CallOutcome::InvalidOutput { message } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::Internal,
                        &message,
                    );
                    Err(CoreError::Internal)
                }
                CallOutcome::ProviderUnavailable { message } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::Unavailable,
                        &message,
                    );
                    Err(CoreError::Semantic(ErrorCode::Unavailable, message))
                }
                CallOutcome::Suppressed { original_result } => {
                    // 幂等抑制:不重复执行,返回原收据(审计已由助手落
                    // outcome=suppressed;ADR-0002 条件 6)
                    let completed_at = w.now_ts();
                    w.settle_operation(&op_id, OperationState::Succeeded, None);
                    if let Some(gid) = &grant_id {
                        persist_grant(w, gid);
                    }
                    Ok(serde_json::json!({
                        "operation_id": op_id.as_str(),
                        "request_id": request_id.as_str(),
                        "principal": ctx.principal.clone(),
                        "capability": params.capability,
                        "state": "succeeded",
                        "created_at": created_at,
                        "completed_at": completed_at,
                        "action_summary": "幂等抑制:等价请求返回原收据",
                        "result_reference": null,
                        "error": null,
                        "grant_used": grant_id,
                        "result": original_result,
                    }))
                }
                CallOutcome::DispatchedAsync => {
                    // M7 S4:已派发异步执行;调用方经 operations.get 轮询终态
                    Ok(serde_json::json!({
                        "operation_id": op_id.as_str(),
                        "request_id": request_id.as_str(),
                        "principal": ctx.principal.clone(),
                        "capability": params.capability,
                        "state": "running",
                        "created_at": created_at,
                        "completed_at": null,
                        "action_summary": format!("能力 {} 异步执行中", params.capability),
                        "result_reference": null,
                        "error": null,
                        "grant_used": grant_id,
                        "result": null,
                    }))
                }
                CallOutcome::Rejected { .. } => {
                    unreachable!("Allowed 分支不会再被拒绝")
                }
            }
        }
        Decision::RequireApproval {
            risk_class,
            effective_risk,
        } => {
            let mut mgr = ApprovalManager::new(&mut w.grants, &*w.config.clock, &*w.config.id_gen);
            let mut approval = mgr.open(OpenApproval {
                capability: &params.capability,
                principal: &ctx.principal,
                risk_class,
                effective_risk,
                input_trust: ctx.trust,
                args: &params.args,
                args_summary: &format!("能力 {} 调用", params.capability),
                scope_choices: capability_scope_choices(),
                ttl_ms: 300_000,
            });
            let approval_id = BmId::parse(approval.approval_id.clone()).expect("appr_ 前缀合法");
            w.settle_operation(&op_id, OperationState::WaitingApproval, None);
            w.emit(
                EventType::ApprovalRequested,
                None,
                None,
                Some(op_id.clone()),
                serde_json::json!({
                    "approval_id": approval.approval_id,
                    "operation_id": op_id.as_str(),
                    "capability": params.capability,
                    "principal": ctx.principal.clone(),
                    "risk_class": risk_class.as_str(),
                    "effective_risk": effective_risk.as_str(),
                    "input_trust": approval.input_trust.as_str(),
                    "expires_at": approval.expires_at,
                }),
            );
            approval.grant_id = None;
            w.approvals.insert(approval_id.clone(), approval.clone());
            persist_approval(
                w,
                &approval,
                &op_id,
                Some((
                    &params.capability,
                    &params.args,
                    params.idempotency_key.as_deref(),
                    ctx.principal.as_str(),
                    ctx.trust,
                )),
            );
            w.cap_pending.insert(
                approval_id.clone(),
                PendingCapabilityCall {
                    op_id: op_id.clone(),
                    capability: params.capability.clone(),
                    args: params.args.clone(),
                    idempotency_key: params.idempotency_key.clone(),
                    principal: ctx.principal.clone(),
                    trust: ctx.trust,
                },
            );
            // GT-02 场景 A2 形态:approval_required 错误信封;operation 停在
            // waiting_approval,由 approval.respond 续行(基线 §9.6)。
            // 结构化携带两 ID:回合管线免反查,凭此精确绑定审批卡片。
            Err(CoreError::ApprovalNeeded {
                message: format!("能力 {} 需要用户审批", params.capability),
                approval_id: approval_id.as_str().to_string(),
                operation_id: op_id.as_str().to_string(),
            })
        }
        Decision::Denied { reason } => {
            let (msg, call_id) = match reason {
                DenyReason::UnknownCapability => (
                    "未知能力,且审批不能补授权(默认拒绝)",
                    w.config.id_gen.next_id("call"),
                ),
                DenyReason::NoGrant => ("无有效授权(默认拒绝)", w.config.id_gen.next_id("call")),
            };
            let reason_code = match reason {
                DenyReason::UnknownCapability => "unknown_capability",
                DenyReason::NoGrant => "no_grant",
            };
            w.settle_operation(
                &op_id,
                OperationState::Failed,
                Some(WireError::new(ErrorCode::PermissionDenied, msg.to_string())),
            );
            w.emit(
                EventType::CapabilityDenied,
                None,
                None,
                Some(op_id.clone()),
                serde_json::json!({
                    "call_id": call_id.as_str(),
                    "capability": params.capability,
                    "principal": ctx.principal.clone(),
                    "input_trust": ctx.trust.as_str(),
                    "reason_code": reason_code,
                }),
            );
            Err(CoreError::Semantic(
                ErrorCode::PermissionDenied,
                msg.to_string(),
            ))
        }
    };
    // 2026-09-05 回看修复:随收据/错误一并交还本次调用的真实 operation_id,
    // 调用方(任务结果流水等)不得再从无序容器反查(H2 证据链错挂根因)。
    (op_id, outcome)
}
pub(crate) fn handle_provider_call(
    w: &mut World,
    operation_id: BmId,
    result: Result<serde_json::Value, crate::ports::AsyncCallError>,
) {
    use crate::ports::AsyncCallError;
    let Some(meta) = w.op_async_meta.remove(&operation_id) else {
        return;
    };
    w.cap_in_flight.remove(&operation_id);
    if !w.operations.contains_key(&operation_id) {
        return; // 停机清场后回流的迟到完成:无载体,丢弃(事件已在日志)
    }
    match result {
        Ok(value) => {
            if let Err(e) = bm_contract::schemas::validate(&meta.output_schema, &value) {
                fail_capability_call(
                    w,
                    &operation_id,
                    &meta.capability,
                    &meta.principal,
                    ErrorCode::Internal,
                    &format!("异步结果出参校验失败: {e}"),
                );
                return;
            }
            w.settle_operation(&operation_id, OperationState::Succeeded, None);
            if let (Some(h), true) = (&meta.key_hash, meta.is_side_effect) {
                w.idem_results.insert(h.clone(), value.clone());
                if let Some(store) = &w.store {
                    // F-02(审计台账):投影写失败必须留痕——静默失败会使重启后
                    // 幂等抑制失效(副作用可能重放)
                    if let Err(e) = store.save_idem_receipt(h, &value.to_string(), &w.now_ts()) {
                        eprintln!("[persist] 幂等收据落表失败 key={h}: {e:?}");
                    }
                    if let Err(e) = store.outbox_upsert(
                        operation_id.as_str(),
                        "side_effect",
                        "published",
                        &serde_json::json!({
                            "capability": meta.capability,
                            "key_hash": meta.key_hash,
                        })
                        .to_string(),
                        &w.now_ts(),
                    ) {
                        tracing::error!(error = %e, op = %operation_id.as_str(), "outbox published 落库失败(T6 收紧前仅告警)");
                    }
                }
            }
            if let Some(gid) = &meta.grant_id {
                persist_grant(w, gid);
            }
            w.op_results.insert(operation_id.clone(), value.clone());
            // M7 S5:成功 -> 恢复 healthy(重连成功/清探针计数)
            note_provider_success(w, &mcp_provider_of(&meta.capability), "重连握手成功");
            emit_capability_invoked_with(
                w,
                &meta.call_id,
                &operation_id,
                &meta.capability,
                &meta.principal,
                Some(meta.epoch),
                Some(&meta.instance_id),
                "ok",
                None,
                meta.key_hash.as_deref(),
            );
        }
        Err(e) => {
            // M7 S5:传输故障 -> MCP unavailable 立即;unavailable 期间的调用
            // 即重连探针(到上限后由 dispatch 门快速失败)
            if matches!(e, AsyncCallError::Transport(_)) {
                let provider = mcp_provider_of(&meta.capability);
                let was = w
                    .provider_health
                    .get(&provider)
                    .map(|h| h.status)
                    .unwrap_or("healthy");
                let entry = w.provider_health.entry(provider.clone()).or_default();
                entry.status = "unavailable";
                if was == "unavailable" {
                    entry.reconnect_attempts += 1;
                }
                if was != "unavailable" {
                    emit_provider_health(w, &provider, "healthy", "unavailable", "子进程/通道故障");
                }
            }
            let (code, msg) = match e {
                AsyncCallError::Timeout => (
                    ErrorCode::Timeout,
                    "异步调用超时(结果未知,对账由 outbox 承载)",
                ),
                AsyncCallError::Transport(_) => (ErrorCode::Unavailable, "Provider 传输故障"),
                AsyncCallError::ToolError => (ErrorCode::Internal, "工具报告执行失败"),
            };
            fail_capability_call(
                w,
                &operation_id,
                &meta.capability,
                &meta.principal,
                code,
                msg,
            );
            if let Some(gid) = &meta.grant_id {
                persist_grant(w, gid);
            }
        }
    }
}
pub(crate) fn handle_capability_cancel(
    w: &mut World,
    params: wire::CapabilityCancelParams,
) -> CoreResult<wire::CapabilityCancelResult> {
    let op = w
        .operations
        .get(&params.operation_id)
        .ok_or_else(|| CoreError::validation("operation 不存在"))?;
    if !matches!(
        op.state,
        OperationState::Running | OperationState::WaitingApproval
    ) {
        return Err(CoreError::validation("operation 不在可取消状态"));
    }
    if let Some(token) = w.cap_in_flight.remove(&params.operation_id) {
        token.cancel();
    }
    // M8.3:语义取消的传输层贯彻(notifications/cancelled;尽力终止)
    if let Some(ex) = &w.config.async_executor {
        ex.cancel_op(params.operation_id.as_str());
    }
    if let Some(meta) = w.op_async_meta.remove(&params.operation_id)
        && let Some(gid) = &meta.grant_id
    {
        persist_grant(w, gid);
    }
    w.settle_operation(
        &params.operation_id,
        OperationState::Cancelled,
        Some(WireError::new(
            ErrorCode::Cancelled,
            params.reason.unwrap_or_else(|| "用户显式取消".into()),
        )),
    );
    // P0(第四轮评审):取消等待审批的操作必须连带撤审。否则用户随后批准
    // 会触发 Cancelled→Running 的表外迁移,状态机 panic,单写者死亡。
    let affected: Vec<BmId> = w
        .cap_pending
        .iter()
        .filter(|(_, p)| p.op_id == params.operation_id)
        .map(|(aid, _)| aid.clone())
        .collect();
    for aid in affected {
        w.cap_pending.remove(&aid);
        if let Some(mut approval) = w.approvals.get(&aid).cloned() {
            let resource = bm_contract::capability::GrantResource {
                capability: approval.capability.clone(),
                args_predicates: Default::default(),
            };
            let respond = {
                let mut mgr = crate::approval::ApprovalManager::new(
                    &mut w.grants,
                    &*w.config.clock,
                    &*w.config.id_gen,
                );
                mgr.respond(
                    &mut approval,
                    crate::approval::RespondDecision::Withdraw,
                    None,
                    resource,
                    "system:cancel",
                )
            };
            if let Ok(None) = respond {
                w.approvals.insert(aid.clone(), approval);
                persist_approval(
                    w,
                    w.approvals.get(&aid).expect("存在"),
                    &params.operation_id,
                    None,
                );
            }
        }
    }
    emit_capability_invoked(
        w,
        &params.operation_id,
        w.op_capability
            .get(&params.operation_id)
            .cloned()
            .unwrap_or_default()
            .as_str(),
        "user:cancel",
        None,
        None,
        "error",
        Some(ErrorCode::Cancelled),
        None,
    );
    Ok(wire::CapabilityCancelResult {
        operation_id: params.operation_id,
        state: "cancelled".into(),
    })
}
pub(crate) fn handle_provider_progress(
    w: &mut World,
    operation_id: String,
    progress: u64,
    total: Option<u64>,
    message: Option<String>,
) {
    let Ok(op_id) = BmId::parse(&operation_id) else {
        return;
    };
    if !w.operations.contains_key(&op_id) {
        return;
    }
    let capability = w.op_capability.get(&op_id).cloned().unwrap_or_default();
    w.emit(
        EventType::CapabilityProgress,
        None,
        None,
        Some(op_id.clone()),
        serde_json::json!({
            "call_id": w.config.id_gen.next_id("call").as_str(),
            "operation_id": op_id.as_str(),
            "capability": capability,
            "progress": progress,
            "total": total,
            "message": message,
        }),
    );
}
pub(crate) fn fail_capability_call(
    w: &mut World,
    op_id: &BmId,
    capability: &str,
    principal: &str,
    code: ErrorCode,
    message: &str,
) {
    w.settle_operation(
        op_id,
        OperationState::Failed,
        Some(WireError::new(code, message.to_string())),
    );
    w.emit(
        EventType::CapabilityInvoked,
        None,
        None,
        Some(op_id.clone()),
        serde_json::json!({
            "call_id": w.config.id_gen.next_id("call").as_str(),
            "operation_id": op_id.as_str(),
            "capability": capability,
            "principal": principal,
            "binding_epoch": 0,
            "provider_instance_id": "n/a",
            "outcome": "error",
            "error_code": code.as_str(),
            "idempotency_key_hash": null,
        }),
    );
}
pub(crate) fn dispatch_capability(
    w: &mut World,
    ctx: &CallContext,
    capability: &str,
    args: serde_json::Value,
    op_id: &BmId,
) -> CallOutcome {
    let prepared = {
        let mut broker = Broker::new(
            &w.registry,
            &mut w.grants,
            &*w.config.clock,
            &*w.config.id_gen,
        );
        match broker.prepare(ctx, capability, args.clone()) {
            Ok(p) => p,
            Err(outcome) => {
                emit_capability_invoked(
                    w,
                    op_id,
                    capability,
                    &ctx.principal,
                    None,
                    None,
                    "error",
                    Some(error_code_of(&outcome)),
                    None,
                );
                return outcome;
            }
        }
    };
    let key_hash: Option<String> = ctx.idempotency_key.as_ref().map(|k| {
        sha256_hex(&format!(
            "{k}:{}",
            serde_json::to_string(&args).unwrap_or_default()
        ))
    });
    if prepared.is_side_effect {
        // 幂等抑制:等价请求返回原收据,Provider 不再执行
        if let Some(h) = &key_hash
            && let Some(original) = w.idem_results.get(h).cloned()
        {
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "suppressed",
                None,
                Some(h),
            );
            return CallOutcome::Suppressed {
                original_result: original,
            };
        }
        // 前门禁:intent 落盘后方执行(崩溃窗口 = intent 在而结果不在 →
        // 恢复期以 Provider 幂等查询对账,T6b)。outbox pending 行与 intent
        // 事件同批落盘,是恢复扫描的对账底座。
        emit_capability_invoked(
            w,
            op_id,
            capability,
            &ctx.principal,
            Some(prepared.credential.binding_epoch),
            Some(&prepared.credential.provider_instance_id),
            "intent",
            None,
            key_hash.as_deref(),
        );
        if let Some(store) = &w.store
            && let Err(e) = store.outbox_upsert(
                op_id.as_str(),
                "side_effect",
                "pending",
                &serde_json::json!({
                    "capability": capability,
                    "key_hash": key_hash,
                })
                .to_string(),
                &w.now_ts(),
            )
        {
            tracing::error!(error = %e, op = %op_id.as_str(), "outbox pending 落库失败(T6 收紧前仅告警)");
        }
    }
    // M7 S4:异步 Provider 路径——决策/校验/预扣/intent 门已过,执行交
    // 异步执行器,完成经 Cmd::ProviderCall 回单写者回路落定。
    if w.registry.is_async(capability) {
        // M7 S5:MCP 重连超限 -> 快速失败(不再触执行器,直至重装)
        let provider = mcp_provider_of(capability);
        let blocked = w
            .provider_health
            .get(&provider)
            .map(|h| {
                h.status == "unavailable"
                    && h.reconnect_attempts >= w.config.limits.get().mcp_reconnect_limit
            })
            .unwrap_or(false);
        if blocked {
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "error",
                Some(ErrorCode::Unavailable),
                key_hash.as_deref(),
            );
            return CallOutcome::ProviderUnavailable {
                message: "异步 Provider 重连超限,保持 unavailable 直至重装".into(),
            };
        }
        let Some(executor) = w.config.async_executor.clone() else {
            return CallOutcome::ProviderError {
                message: "异步执行器未装配".into(),
            };
        };
        let meta = AsyncCallMeta {
            capability: capability.to_string(),
            principal: ctx.principal.clone(),
            call_id: BmId::parse(&prepared.credential.call_id)
                .unwrap_or_else(|_| w.config.id_gen.next_id("call")),
            epoch: prepared.credential.binding_epoch,
            instance_id: prepared.credential.provider_instance_id.clone(),
            key_hash: key_hash.clone(),
            is_side_effect: prepared.is_side_effect,
            output_schema: prepared.manifest.output_schema.to_string(),
            grant_id: prepared.grant_id.clone(),
        };
        w.op_async_meta.insert(op_id.clone(), meta);
        // Grant 消费态随 spawn 落行(count 类重启不回满)
        if let Some(gid) = &prepared.grant_id {
            persist_grant(w, gid);
        }
        let deadline_ms = prepared.manifest.timeout_ms.clamp(100, 600_000);
        let cancel = CancellationToken::new();
        w.cap_in_flight.insert(op_id.clone(), cancel.clone());
        let tx = w.tx.clone();
        let op = op_id.clone();
        let cap = capability.to_string();
        let exec_args = args.clone();
        tokio::spawn(async move {
            let result = executor
                .call(
                    op.as_str(),
                    &cap,
                    exec_args,
                    std::time::Duration::from_millis(deadline_ms),
                )
                .await;
            let _ = tx
                .send(Cmd::ProviderCall {
                    operation_id: op,
                    result,
                })
                .await;
        });
        return CallOutcome::DispatchedAsync;
    }
    let outcome = {
        let broker = Broker::new(
            &w.registry,
            &mut w.grants,
            &*w.config.clock,
            &*w.config.id_gen,
        );
        broker.execute(&prepared, args)
    };
    match &outcome {
        CallOutcome::Completed { result, .. } => {
            if let (Some(h), true) = (&key_hash, prepared.is_side_effect) {
                w.idem_results.insert(h.clone(), result.clone());
                // T6c 收紧(M5-T1):幂等收据落表,恢复期抑制判定不依赖内存
                // F-02(审计台账):落表失败必须留痕,不得静默
                if let Some(store) = &w.store
                    && let Err(e) = store.save_idem_receipt(h, &result.to_string(), &w.now_ts())
                {
                    eprintln!("[persist] 幂等收据落表失败 key={h}: {e:?}");
                }
            }
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "ok",
                None,
                key_hash.as_deref(),
            );
            if prepared.is_side_effect
                && let Some(store) = &w.store
                && let Err(e) = store.outbox_upsert(
                    op_id.as_str(),
                    "side_effect",
                    "published",
                    &serde_json::json!({
                        "capability": capability,
                        "key_hash": key_hash,
                    })
                    .to_string(),
                    &w.now_ts(),
                )
            {
                tracing::error!(error = %e, op = %op_id.as_str(), "outbox published 落库失败(T6 收紧前仅告警)");
            }
        }
        CallOutcome::Suppressed { .. } => unreachable!("抑制发生在 execute 前"),
        other => {
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "error",
                Some(error_code_of(other)),
                key_hash.as_deref(),
            );
        }
    }
    outcome
}
