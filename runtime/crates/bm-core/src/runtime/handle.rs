//! RuntimeHandle 对外句柄(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

use super::*;

/// 运行句柄:进程内 Wire API(M1 方法集合)。
#[derive(Clone)]
pub struct RuntimeHandle {
    tx: mpsc::Sender<Cmd>,
}

impl RuntimeHandle {
    /// 启动 Runtime:恢复持久状态(如配置了 store)→ runtime.started →
    /// 中断清点的审计事件 → runtime.recovered,随后进入核心循环。
    pub async fn start(config: RuntimeConfig) -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>(1024);
        let exec_log = Arc::new(ExecutionLog::new(config.data_dir.as_deref()));
        let started_at = config.clock.now();
        // capability 操作的系统容器(内存合成;不与任何 Session/Agent 关联)
        let system_session = config.id_gen.next_id("sess");
        let system_agent = config.id_gen.next_id("agent");
        let config = config;
        let mut world = World {
            bus: EventBus::new(),
            exec_log,
            in_flight: HashMap::new(),
            sessions: HashMap::new(),
            agents: HashMap::new(),
            operations: HashMap::new(),
            started_at,
            started_instant: Instant::now(),
            draining: false,
            stopped: false,
            persist_poisoned: false,
            registry: CapabilityRegistry::new(),
            grants: GrantLedger::new(),
            approvals: HashMap::new(),
            cap_pending: HashMap::new(),
            idem_results: HashMap::new(),
            system_session,
            system_agent,
            tasks: HashMap::new(),
            task_board: crate::task::TaskBoard::default(),
            task_tool_calls: HashMap::new(),
            watchdog: crate::watchdog::WatchdogState::default(),
            op_capability: HashMap::new(),
            task_results: HashMap::new(),
            model_delta_seq: HashMap::new(),
        op_async_meta: HashMap::new(),
            op_results: HashMap::new(),
            provider_health: HashMap::new(),
            cap_in_flight: HashMap::new(),
            model_call_audit: HashMap::new(),
            tx: tx.clone(),
            store: config.store.clone(),
            config,
        };

        // T3 启动恢复:修复窗口 → 行装配 → 中断清点。恢复失败 = 拒绝启动
        // (宁可拒开,不带残缺状态服务)。
        let mut pending_interrupts: Vec<(BmId, BmId, String)> = Vec::new();
        let mut agents_to_resume: Vec<(BmId, Option<BmId>)> = Vec::new();
        let mut report = None;
        if let Some(store) = world.store.clone() {
            let r = store.recover().expect("启动恢复失败,拒绝启动(宁可拒开)");
            let rows = store.load_rows().expect("规范状态行装配失败,拒绝启动");
            world.load_world_rows(rows, &mut pending_interrupts, &mut agents_to_resume);
            // seq 分配器重同步到持久日志末尾之后:跨重启 seq 连续(INV-3)
            let log_last = r.last_applied_seq.max(store.last_log_seq().unwrap_or(0));
            world.bus.resync_to(log_last + 1);
            // 空库首启 = 新启动,不是恢复:不产生 runtime.recovered 噪音事件
            if log_last > 0 {
                report = Some(r);
            }
        }
        // M4:内置能力注册(启动面)+ 持久 binding/审批/授权恢复
        let caps = std::mem::take(&mut world.config.capabilities);
        let mut registered: Vec<(String, Arc<dyn CapabilityProvider>)> = Vec::new();
        for (manifest, provider) in caps {
            let instance = format!("{}@{}", manifest.capability, manifest.version);
            let manifest_json = serde_json::to_string(&manifest).unwrap_or_default();
            let capability = manifest.capability.clone();
            let is_async = manifest.provider.starts_with("mcp.");
            world
                .registry
                .register(manifest, &instance, provider.clone())
                .expect("内置能力首次注册不得冲突");
            if is_async {
                world.registry.mark_async(&capability);
            }
            registered.push((capability.clone(), provider));
            if let Some(store) = &world.store {
                let _ = store.save_capability_binding(bm_persist::sqlite_state::CapabilityRow {
                    capability: &capability,
                    provider_instance_id: &instance,
                    epoch: 1,
                    status: "active",
                    manifest: &manifest_json,
                    updated_at: &format_ts(world.started_at),
                });
            }
        }
        // M4 启动恢复:binding epoch 取持久 max(不回退,ADR-0001 条件 2);
        // Grant 台账与审批对象重建——「审批中断后可以恢复」(基线 M4 通过条件)。
        if let Some(store) = &world.store {
            for row in store.list_capability_bindings().unwrap_or_default() {
                let cap = row["capability"].as_str().unwrap_or("");
                let epoch = row["epoch"].as_u64().unwrap_or(0);
                let instance = row["provider_instance_id"].as_str().unwrap_or("");
                if let Some(m) = world.registry.manifest_of(cap).cloned() {
                    world.registry.restore_binding(m, instance, epoch);
                }
            }
            // restore_binding 清空可丢失运行时缓存(T1 语义):句柄由注册流程
            // 重新 attach——否则恢复后执行入口拿不到 Provider。
            for (cap, provider) in &registered {
                let _ = world.registry.attach_handle(cap, provider.clone());
            }
            for row in store.list_grants().unwrap_or_default() {
                let payload = row["payload"].as_str().unwrap_or("null");
                let Ok(grant) = serde_json::from_str::<bm_contract::capability::Grant>(payload)
                else {
                    continue;
                };
                let revoked = row["revoked"].as_i64().unwrap_or(0) == 1;
                // T6c 收紧(M5-T1):消费计数随行恢复,count 类余量重启不回满
                let used = row["used_count"].as_u64().unwrap_or(0);
                world.grants.restore(grant, used, revoked);
            }
            // T6c 收紧(M5-T1):幂等收据仓自持久层装载
            for row in store.list_idem_receipts().unwrap_or_default() {
                if let (Some(h), Some(payload)) =
                    (row["key_hash"].as_str(), row["payload"].as_str())
                    && let Ok(v) = serde_json::from_str::<serde_json::Value>(payload)
                {
                    world.idem_results.insert(h.to_string(), v);
                }
            }
            // M5-T6:Task 包络记账恢复(聚合行 agent_id = "")
            for row in store.list_task_budget().unwrap_or_default() {
                if row["agent_id"].as_str() == Some("")
                    && let (Some(tid), Some(used)) =
                        (row["task_id"].as_str(), row["used_tool_calls"].as_u64())
                    && let Ok(id) = BmId::parse(tid.to_string())
                {
                    world.task_tool_calls.insert(id, used);
                }
            }
            // M5.4:Task Board 投影启动重建——重放事件日志折叠(ADR-0004 条件 1);
            // 已被压实的前缀自 L2 行补齐(行即同一事件流的快照态,键列确定性)
            let events = store.replay_since(0).unwrap_or_default();
            world.task_board = crate::task::TaskBoard::rebuild(&events);
            for row in store.list_tasks().unwrap_or_default() {
                let id = row["id"].as_str().unwrap_or_default().to_string();
                if world.task_board.entry(&id).is_none() {
                    world.task_board.restore_row(
                        &id,
                        row["title"].as_str().unwrap_or_default(),
                        row["state"].as_str().unwrap_or("created"),
                        row["task_epoch"].as_u64().unwrap_or(1),
                    );
                }
            }
            for row in store.list_approvals().unwrap_or_default() {
                let payload = row["payload"].as_str().unwrap_or("null");
                let Ok(wrap) = serde_json::from_str::<serde_json::Value>(payload) else {
                    continue;
                };
                let Ok(approval) = serde_json::from_value::<Approval>(wrap["approval"].clone())
                else {
                    continue;
                };
                let state = approval.state;
                let approval_id =
                    BmId::parse(approval.approval_id.clone()).expect("库内 approval id 合法");
                let Ok(op_id) = BmId::parse(row["operation_id"].as_str().unwrap_or("")) else {
                    continue;
                };
                if state == bm_contract::capability::ApprovalState::WaitingUser {
                    // waiting_approval 的 operation 内存重建(从未离开该状态)
                    let request_id =
                        BmId::from_parts("req", op_id.ulid_part()).expect("同段合成合法");
                    let created_at = approval.requested_at.clone();
                    world.operations.insert(
                        op_id.clone(),
                        Operation {
                            id: op_id.clone(),
                            request_id,
                            session_id: world.system_session.clone(),
                            agent_id: world.system_agent.clone(),
                            state: OperationState::WaitingApproval,
                            turn_index: 0,
                            created_at,
                            completed_at: None,
                            action_summary: "能力调用(恢复)".to_string(),
                            result_reference: None,
                            error: None,
                        },
                    );
                    if let Some(call) = wrap.get("call") {
                        let capability = call["capability"].as_str().unwrap_or("").to_string();
                        let args = call["args"].clone();
                        if !capability.is_empty() {
                            world.cap_pending.insert(
                                approval_id.clone(),
                                PendingCapabilityCall {
                                    op_id,
                                    capability,
                                    args,
                                    idempotency_key: call["idempotency_key"]
                                        .as_str()
                                        .map(|s| s.to_string()),
                                    principal: call["principal"]
                                        .as_str()
                                        .unwrap_or(CAPABILITY_CALLER)
                                        .to_string(),
                                    trust: call["trust"]
                                        .as_str()
                                        .and_then(DataTrust::from_wire)
                                        .unwrap_or(DataTrust::Trusted),
                                },
                            );
                        }
                    }
                }
                world.approvals.insert(approval_id, approval);
            }
            // T6b 恢复三路:pending outbox = intent 在而结果不在。Provider
            // 幂等查询属 M7(外部核验);M4 落地 = operation 重建为
            // outcome_unknown,等待裁定入口(recovery_settle:external_
            // verification OR user_ruling),禁止自动重放(ADR-0004)。
            for row in store.list_outbox_by_state("pending").unwrap_or_default() {
                let Ok(op_id) = BmId::parse(row["operation_id"].as_str().unwrap_or("")) else {
                    continue;
                };
                if world.operations.contains_key(&op_id) {
                    continue;
                }
                let request_id = BmId::from_parts("req", op_id.ulid_part()).expect("同段合成合法");
                world.operations.insert(
                    op_id.clone(),
                    Operation {
                        id: op_id.clone(),
                        request_id,
                        session_id: world.system_session.clone(),
                        agent_id: world.system_agent.clone(),
                        state: OperationState::OutcomeUnknown,
                        turn_index: 0,
                        created_at: world.now_ts(),
                        completed_at: None,
                        action_summary: "外部副作用结果未知(恢复)".to_string(),
                        result_reference: None,
                        error: None,
                    },
                );
                tracing::warn!(
                    op = %op_id,
                    "outbox pending:外部副作用结果未知,等待裁定(recovery_settle)"
                );
            }
        }

        // M5-T3:Butler bootstrap 协调权物化(基线 §10.1 全集;幂等——已有
        // (含已撤销)不重发;撤销是持久事实,重授走审批随 M8 UI)
        {
            let mut existing_pairs: Vec<(String, String)> = Vec::new();
            if let Some(store) = &world.store {
                for row in store.list_grants().unwrap_or_default() {
                    if row["audience"].as_str() == Some(crate::butler::BUTLER_PRINCIPAL)
                        && let Some(action) = row["action"].as_str()
                    {
                        existing_pairs.push((
                            crate::butler::BUTLER_PRINCIPAL.to_string(),
                            action.to_string(),
                        ));
                    }
                }
            }
            let issued = crate::butler::materialize_missing(
                &mut world.grants,
                &existing_pairs,
                &*world.config.id_gen,
                &*world.config.clock,
            );
            for g in issued {
                persist_grant(&world, &g.grant_id);
                world.emit(
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
        }

        // M7.5:异步执行器进度回注(回路外 → Cmd 回流单写者)
        if let Some(ex) = world.config.async_executor.clone() {
            let tx = world.tx.clone();
            ex.set_progress_sink(Box::new(move |n| {
                let _ = tx.try_send(Cmd::ProviderProgress {
                    operation_id: n.operation_id,
                    progress: n.progress,
                    total: n.total,
                    message: n.message,
                });
            }));
        }

        world.watchdog.schedule_next(world.config.clock.now());
        world.emit(
            EventType::RuntimeStarted,
            None,
            None,
            None,
            serde_json::json!({
                "pid": std::process::id(),
                "version": world.config.version,
                "started_at": format_ts(world.started_at),
            }),
        );

        // 中断清点(留审计事件,ADR-0004 恢复语义):崩溃时未终态的 operation
        // 走 running→interrupted(runtime_crash_before_terminal);agent 走
        // waiting_model/running→interrupted→resuming→running。
        // T7 claim(M2.6):NoEffect 域且输入原文在受保护存储中时,自动
        // interrupted→running(recovery_replay_ok)并重驱回合 → 幂等续跑;
        // 无输入上下文或 outcome_unknown 者,留给裁定入口(recovery_settle)。
        let mut claim_redrive: Vec<(BmId, BmId, String)> = Vec::new();
        for (op_id, agent_id, op_state) in pending_interrupts.drain(..) {
            if op_state == "running" {
                world.settle_operation(&op_id, OperationState::Interrupted, None);
            }
            world.emit(
                EventType::AgentInterrupted,
                None,
                Some(agent_id.clone()),
                Some(op_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": op_id.as_str(),
                    "reason": "runtime_recovery",
                }),
            );
            {
                let a = world.agents.get_mut(&agent_id).expect("恢复行必有 agent");
                a.transition(AgentState::Interrupted);
                a.transition(AgentState::Resuming);
                a.transition(AgentState::Running);
            }
            world.emit(
                EventType::AgentResumed,
                None,
                Some(agent_id.clone()),
                Some(op_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": op_id.as_str(),
                }),
            );
            // claim 判定:有输入原文才可续跑
            let content = world
                .store
                .as_ref()
                .and_then(|s| s.op_input(op_id.as_str()).ok())
                .flatten();
            if let Some(content) = content {
                claim_redrive.push((op_id, agent_id, content));
            }
        }

        // 无 running op 但停在中间态的 agent:同样走中断恢复(outcome_unknown
        // 场景:op 留给裁定,agent 恢复可接单)
        for (agent_id, latest_op) in agents_to_resume.drain(..) {
            // op 流可能已恢复该 agent:先查内存态,避免重复中断事件
            if world.agents.get(&agent_id).map(|a| a.state) == Some(AgentState::Running) {
                continue;
            }
            let op_hint = latest_op.or_else(|| {
                world
                    .operations
                    .values()
                    .filter(|o| o.agent_id == agent_id)
                    .map(|o| o.id.clone())
                    .next()
            });
            world.emit(
                EventType::AgentInterrupted,
                None,
                Some(agent_id.clone()),
                op_hint.clone(),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": op_hint.as_ref().map(|o| o.as_str()),
                    "reason": "runtime_recovery",
                }),
            );
            {
                let a = world.agents.get_mut(&agent_id).expect("恢复行必有 agent");
                if AgentState::can_transition(a.state, AgentState::Interrupted) {
                    a.transition(AgentState::Interrupted);
                }
                a.transition(AgentState::Resuming);
                a.transition(AgentState::Running);
            }
            world.emit(
                EventType::AgentResumed,
                None,
                Some(agent_id.clone()),
                op_hint.clone(),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": op_hint.as_ref().map(|o| o.as_str()),
                }),
            );
        }

        // claim 续跑:interrupted→running(recovery_replay_ok)后重驱回合
        for (op_id, agent_id, content) in claim_redrive {
            world.settle_operation(&op_id, OperationState::Running, None);
            let agent = world
                .agents
                .get(&agent_id)
                .cloned()
                .expect("恢复行必有 agent");
            // 重驱即再发模型调用:running→waiting_model(与 send_input 同构)
            {
                let a = world.agents.get_mut(&agent_id).expect("存在");
                a.transition(AgentState::WaitingModel);
            }
            world.emit(
                EventType::AgentResumed,
                None,
                Some(agent_id.clone()),
                Some(op_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": op_id.as_str(),
                }),
            );
            world
                .in_flight
                .insert(op_id.clone(), CancellationToken::new());
            spawn_turn(&mut world, &agent, &op_id, content);
        }

        if let Some(r) = report {
            world.emit(
                EventType::RuntimeRecovered,
                None,
                None,
                None,
                serde_json::json!({
                    "last_applied_seq": r.last_applied_seq,
                    "replayed": r.replayed,
                    "interrupted_recovered": r.interrupted_recovered,
                }),
            );
        }

        tokio::spawn(core_loop(world, rx));
        Self { tx }
    }

    pub async fn session_create(
        &self,
        request_id: BmId,
        params: SessionCreateParams,
    ) -> CoreResult<SessionCreateResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::SessionCreate {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn session_resume(
        &self,
        request_id: BmId,
        params: SessionResumeParams,
    ) -> CoreResult<SessionResumeResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::SessionResume {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn session_close(
        &self,
        request_id: BmId,
        params: SessionCloseParams,
    ) -> CoreResult<SessionCloseResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::SessionClose {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn events_poll(&self, params: EventsPollParams) -> CoreResult<EventsPollResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::EventsPoll { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn send_input(
        &self,
        request_id: BmId,
        params: SendInputParams,
    ) -> CoreResult<Receipt> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::SendInput {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn agent_cancel(&self, params: CancelParams) -> CoreResult<CancelResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Cancel { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 收据查询幂等(INV-9):终态后任意多次调用结果一致。
    pub async fn operations_get(&self, params: GetOperationParams) -> CoreResult<Receipt> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::GetOperation { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 全量事件流(诊断端口,M1 供回放器使用;M3 由 watch/cursor 取代)。
    #[doc(hidden)]
    pub async fn events_all(&self) -> Vec<EventEnvelope> {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Cmd::EventsAll { resp: tx }).await.is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    /// 恢复裁定(M2.6 内部入口;INV-10:普通重试不得触碰 outcome_unknown)。
    /// 迁移表合法边:outcome_unknown→succeeded/failed(external_verification OR
    /// user_ruling);interrupted→running(claim)/cancelled(user_ruling)。
    #[doc(hidden)]
    pub async fn recovery_settle(
        &self,
        operation_id: BmId,
        verdict: RecoveryVerdict,
    ) -> CoreResult<Receipt> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::RecoverySettle {
                operation_id,
                verdict,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// capability.call(M4.2):统一入口;需审批时返回 approval_required 错误,
    /// operation 停在 waiting_approval,经 approval.respond 续行。
    pub async fn capability_call(
        &self,
        request_id: BmId,
        params: wire::CapabilityCallParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::CapabilityCall {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// approval.list(M4.7):审批对象列表(默认全部,waiting_user 在前)。
    pub async fn approval_list(
        &self,
        params: wire::ApprovalListParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::ApprovalList { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// approval.respond(M4.7):批准(物化 Grant 并重放执行)/拒绝/取消。
    /// M8.1:异步能力调用结果(核心诊断端口;wire 面随按需增发)。
    pub async fn operation_result(
        &self,
        operation_id: BmId,
    ) -> CoreResult<Option<serde_json::Value>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::GetOpResult {
                operation_id,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// M8.3:能力调用语义取消(在途异步;迟到完成丢弃)。
    pub async fn capability_cancel(
        &self,
        request_id: BmId,
        params: wire::CapabilityCancelParams,
    ) -> CoreResult<wire::CapabilityCancelResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::CapabilityCancel {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn approval_respond(
        &self,
        request_id: BmId,
        params: wire::ApprovalRespondParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::ApprovalRespond {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// task.create(M5.2):Task 创建并启动(L2 规范状态;wire/task.v0.1)。
    pub async fn task_create(
        &self,
        request_id: BmId,
        params: wire::TaskCreateParams,
    ) -> CoreResult<wire::TaskCreateResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskCreate {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// task.pause(M5.5):暂停 Task 及其成员编排推进。
    pub async fn task_pause(
        &self,
        request_id: BmId,
        params: wire::TaskLifecycleParams,
    ) -> CoreResult<wire::TaskStateResult> {
        self.task_lifecycle(request_id, TaskAction::Pause, params)
            .await
    }

    /// task.resume(M5.5):恢复 = 编排重启触发者之一(ADR-0004 条件 6)。
    pub async fn task_resume(
        &self,
        request_id: BmId,
        params: wire::TaskLifecycleParams,
    ) -> CoreResult<wire::TaskStateResult> {
        self.task_lifecycle(request_id, TaskAction::Resume, params)
            .await
    }

    /// task.stop(M5.5):取消 Task(进行中副作用按 §9.5 收敛,成员级联停止)。
    pub async fn task_stop(
        &self,
        request_id: BmId,
        params: wire::TaskLifecycleParams,
    ) -> CoreResult<wire::TaskStateResult> {
        self.task_lifecycle(request_id, TaskAction::Stop, params)
            .await
    }

    /// task.list(M5.4):任务列表(L2 规范状态投影为合同对象;确定性序)。
    pub async fn task_list(
        &self,
        params: wire::TaskListParams,
    ) -> CoreResult<wire::TaskListResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskList { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 追加 Worker 成员(M6.3):并发门禁内再签发一枚 Worker(授权链同自举)。
    pub async fn task_spawn_member(&self, task_id: BmId) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskSpawnMember { task_id, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 委派子任务(M6.3/M6.5):深度/授权子集/预算/并发四门禁后建子 Task
    /// 并完成其协调链自举(委派 = 子任务,规格 §5.2)。
    pub async fn task_spawn_subtask(
        &self,
        params: SpawnSubtaskParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskSpawnSubtask { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 成员移除(M6.3):替换/退出留痕(task.member.removed,墓碑语义)。
    pub async fn task_remove_member(
        &self,
        params: RemoveMemberParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskRemoveMember { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 结果收集(M6.6):聚合成员结果(来源/状态/关联 Operation)+ 子任务概览。
    pub async fn task_collect(&self, task_id: BmId) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskCollect { task_id, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// Worker 能力调用(M5 Agent 路径):worker 以 task:<id> Grant 经
    /// Broker 统一裁决(Grant 命中直通,无授权 untrusted 100% 升级审批)。
    pub async fn worker_capability_call(
        &self,
        request_id: BmId,
        params: WorkerCallParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::WorkerCall {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// Task 预算扩容(M5-T6):仅用户可发起(Broker/Coordinator 不可扩容,
    /// ADR-0002 要点 5);blocked(budget_exhausted)的任务扩容后恢复运行。
    pub async fn task_budget_increase(
        &self,
        task_id: BmId,
        max_tool_calls: u64,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskBudgetIncrease {
                task_id,
                max_tool_calls,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// Worker 声称任务完成(M5-T8):Observation 消费 verification 钩子做
    /// 确定性核验——verified → completed;unverified → blocked(outcome_
    /// unknown_pending)等用户裁定,禁止自动标成功(完成判定门禁)。
    pub async fn task_report_completion(
        &self,
        task_id: BmId,
        claim_summary: impl Into<String>,
        operation_id: Option<BmId>,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskReportCompletion {
                task_id,
                claim_summary: claim_summary.into(),
                operation_id,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// Watchdog 手动扫描(诊断入口):返回本次产生的事件数。
    pub async fn watchdog_scan(&self) -> CoreResult<usize> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::WatchdogScan { resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// Butler 协调权撤销(M5.1):撤销 bootstrap Grant 集(全部 mutation+
    /// safe 动词),撤销后仅剩只读查询面;重授走审批(交互随 M8)。
    /// 返回撤销的 Grant 数。
    pub async fn butler_revoke(&self, reason: impl Into<String>) -> CoreResult<usize> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::ButlerRevoke {
                reason: reason.into(),
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// task.get(M5.2/M5.4):规范对象 + 监护态(guard_states 随 T7 填充)。
    pub async fn task_get(&self, params: wire::TaskGetParams) -> CoreResult<wire::TaskGetResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskGet { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    async fn task_lifecycle(
        &self,
        request_id: BmId,
        action: TaskAction,
        params: wire::TaskLifecycleParams,
    ) -> CoreResult<wire::TaskStateResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskLifecycle {
                request_id,
                action,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 停机:排空进行中回合(不取消,INV-12),发 stopping/stopped 事件。
    pub async fn stop(&self, reason: impl Into<String>) {
        let (tx, rx) = oneshot::channel();
        if self
            .tx
            .send(Cmd::Stop {
                reason: reason.into(),
                resp: tx,
            })
            .await
            .is_err()
        {
            return;
        }
        let _ = rx.await;
    }
}
