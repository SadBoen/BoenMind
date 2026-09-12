//! runtime 内嵌测试(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

#[cfg(test)]
mod t7_event_shape_tests {
    use super::super::*;

 /// T7 硬约束 3:命令语义形状在持久化前拒绝(G1 Bus 不得当 RPC)。
 #[test]
    fn command_semantic_payloads_are_rejected_before_persist() {
        let ty = EventType::SessionCreated;
        for bad_key in [
            "requested_action",
            "instruction",
            "command",
            "please_execute",
        ] {
            let payload = serde_json::json!({ bad_key: {"op": "mail.send"} });
            assert!(
                validate_event_shape(&ty, &payload).is_err(),
                "{bad_key} 形状必须被拒"
            );
        }
 // 正常事实载荷照常通过
        assert!(validate_event_shape(&ty, &serde_json::json!({"session_id": "x"})).is_ok());
    }
}

#[cfg(test)]
mod r2_tombstone_tests {
    use super::super::*;
    use bm_contract::ids::{IdGen, UlidIdGen};

 // ---- 最小 stub 集(bm-core 不依赖 providers/testkit,自备确定性件)----

    struct StubConnector;
 #[async_trait::async_trait]
    impl ModelConnector for StubConnector {
        fn provider(&self) -> &'static str {
            "stub"
        }

        async fn invoke(
            &self,
            _req: InvokeRequest,
            _c: tokio_util::sync::CancellationToken,
        ) -> InvokeResponse {
            InvokeResponse::Failed {
                error_code: ErrorCode::Internal,
                retryable: false,
                attempt: 1,
                detail_ref: None,
                detail: None,
            }
        }
        async fn invoke_stream(
            &self,
            req: InvokeRequest,
            cancel: tokio_util::sync::CancellationToken,
            _on_delta: Box<dyn for<'a> FnMut(&'a str) + Send + 'static>,
        ) -> InvokeResponse {
            self.invoke(req, cancel).await
        }
    }

    struct StubSecrets;
    impl crate::ports::SecretStore for StubSecrets {
        fn get(&self, _secret_ref: &str) -> Result<String, crate::ports::SecretError> {
            Err(crate::ports::SecretError::NotFound("stub".into()))
        }
        fn put(&self, _secret_ref: &str, _value: &str) -> Result<(), crate::ports::SecretError> {
            Ok(())
        }
        fn delete(&self, _secret_ref: &str) -> Result<(), crate::ports::SecretError> {
            Ok(())
        }
        fn expose_for_scan(&self) -> Vec<String> {
            Vec::new()
        }
    }

    struct FixedClock;
    impl crate::clock::Clock for FixedClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::DateTime::from_timestamp(1_787_952_900, 0).unwrap()
        }
    }

    pub(super) fn test_world(dir: &std::path::Path) -> World {
        let (tx, _rx) = mpsc::channel::<Cmd>(64);
 // F-12:内存桩(理由同 memory.rs 测试注释)
        let store: Arc<dyn EventStore> =
            Arc::new(crate::ports::persist::test_support::MemEventStore::new());
        World {
            bus: EventBus::new(),
            exec_log: Arc::new(ExecutionLog::new(None)),
            in_flight: HashMap::new(),
            sessions: HashMap::new(),
            agents: HashMap::new(),
            operations: HashMap::new(),
            started_at: FixedClock.now(),
            started_instant: std::time::Instant::now(),
            draining: false,
            stopped: false,
            persist_poisoned: false,
            registry: CapabilityRegistry::new(),
            grants: GrantLedger::new(),
            approvals: HashMap::new(),
            cap_pending: HashMap::new(),
            idem_results: HashMap::new(),
            system_session: UlidIdGen.next_id("sess"),
            system_agent: UlidIdGen.next_id("agent"),
            tasks: HashMap::new(),
            task_board: crate::task::TaskBoard::default(),
            share_board: crate::share::TaskShareBoard::default(),
            task_tool_calls: HashMap::new(),
            watchdog: crate::watchdog::WatchdogState::default(),
            op_capability: HashMap::new(),
            task_results: HashMap::new(),
            model_delta_seq: HashMap::new(),
            autorun: HashMap::new(),
            op_async_meta: HashMap::new(),
            op_results: HashMap::new(),
            provider_health: HashMap::new(),
            cap_in_flight: HashMap::new(),
            draining_caps: HashMap::new(),
            model_call_audit: HashMap::new(),
            session_chats: HashMap::new(),
            session_turn_totals: HashMap::new(),
            ctx_log: Arc::new(crate::context_log::ContextLog::new(None)),
            turn_debug: Arc::new(crate::turn_debug::TurnDebugLog::new(None)),
            tx,
            store: Some(store.clone()),
            config: RuntimeConfig {
                version: "test".into(),
                data_dir: Some(dir.to_path_buf()),
                store: Some(store),
                connector: Arc::new(StubConnector),
                secret_store: Arc::new(StubSecrets),
                id_gen: Arc::new(UlidIdGen),
                clock: Arc::new(FixedClock),
                capabilities: vec![],
                async_executor: None,
                model_streaming: false,
                limits: LimitsCell::with_default(),
                job_board: None,
            },
        }
    }

 /// R2(INV-3):坏形状事件以 StoreWriteRejected tombstone 占住原 seq 槽
 /// (持久+总线),此后正常事件 seq 连续——存储侧重放无跳号。
 #[test]
    fn bad_shape_event_tombstone_keeps_store_seqs_contiguous() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut world = test_world(dir.path());

        let good1 = world.emit(
            EventType::RuntimeStarted,
            None,
            None,
            None,
            serde_json::json!({"pid": 1, "version": "test", "started_at": "2026-09-06T00:00:00Z"}),
        );
        let bad = world.emit(
            EventType::SessionCreated,
            None,
            None,
            None,
            serde_json::json!({"command": {"op": "mail.send"}}),
        );
        let good2 = world.emit(
            EventType::RuntimeStarted,
            None,
            None,
            None,
            serde_json::json!({"pid": 1, "version": "test-2", "started_at": "2026-09-06T00:00:00Z"}),
        );

 // 三次发射 seq 连续(坏事件不跳号)
        assert_eq!(
            good2.event_seq - good1.event_seq,
            2,
            "正常事件 seq 必须连续推进"
        );
 // 坏事件返回的即 tombstone:类型与占位一致
        assert_eq!(bad.event_type, EventType::StoreWriteRejected);
        assert_eq!(bad.event_seq, good1.event_seq + 1);

 // 持久侧重放:seq 无孔洞,且 tombstone 真实落盘
        let store = world.store.as_ref().unwrap();
        let persisted = store.replay_since(0).expect("重放成功");
        let seqs: Vec<u64> = persisted.iter().map(|e| e.event_seq).collect();
        assert!(seqs.len() >= 3, "至少三条落盘: {seqs:?}");
        for w in seqs.windows(2) {
            assert_eq!(w[1], w[0] + 1, "INV-3 连续性破损: {seqs:?}");
        }
        assert!(
            persisted
                .iter()
                .any(|e| e.event_type == EventType::StoreWriteRejected
                    && e.event_seq == good1.event_seq + 1),
            "tombstone 必须落在坏事件的原 seq 槽"
        );
    }

 /// agents 行已随会话删除,内存遗留即幽灵 Agent(常驻增长,且按 agent_id
 /// 查询会误判其活跃)。
 #[test]
    fn session_delete_also_removes_agent_from_memory() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut world = test_world(dir.path());
 // 纯内存口径:store/data_dir 置空跳过持久侧效(MemEventStore 桩未覆盖
 // erase 路径),被测对象=删除的内存台账清理本身
        world.store = None;
        world.config.data_dir = None;

 // 直接装配 Session+Agent(被测对象是删除清理;handle_session_create
 // 会持久化 Grant,MemEventStore 桩未覆盖该路径)
        let sid = world.config.id_gen.next_id("sess");
        let aid = world.config.id_gen.next_id("agent");
        let mut session = Session {
            id: sid.clone(),
            agent_id: aid.clone(),
            state: SessionState::Created,
            created_at: world.now_ts(),
            workspace_id: None,
            title: None,
            updated_at: None,
            permission_mode: bm_contract::wire::PermissionMode::Ask,
        };
        session.transition(SessionState::Active);
        world.sessions.insert(sid.clone(), session);
        world.agents.insert(
            aid.clone(),
            Agent {
                id: aid.clone(),
                session_id: sid.clone(),
                name: "回归".into(),
                model_chain: vec!["stub.model".into()],
                state: AgentState::Created,
                budget: crate::state::budget_from_spec(None),
                system_prompt: None,
                allowed_tools: None,
            },
        );
        assert!(world.agents.contains_key(&aid), "前置:agent 已在内存台账");

        let req = world.config.id_gen.next_id("req");
        handle_session_delete(
            &mut world,
            req,
            SessionDeleteParams {
                session_id: sid.clone(),
            },
        )
        .expect("删会话成功");

        assert!(
            !world.agents.contains_key(&aid),
            "删会话后内存不得遗留幽灵 Agent"
        );
        assert!(
            !world.sessions.contains_key(&sid),
            "会话本身必须已从内存移除"
        );
    }

 // ---- 会话目录()----

    fn catalog_session(world: &World, updated_at: Option<String>) -> Session {
        Session {
            id: world.config.id_gen.next_id("sess"),
            agent_id: world.config.id_gen.next_id("agent"),
            state: SessionState::Active,
            created_at: "2026-09-08T08:00:00.000Z".into(),
            workspace_id: None,
            title: None,
            updated_at,
            permission_mode: bm_contract::wire::PermissionMode::Ask,
        }
    }

 #[test]
    fn session_list_orders_by_recent_activity() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut world = test_world(dir.path());
        world.store = None;
        world.config.data_dir = None;

        let mut early = catalog_session(&world, Some("2026-09-08T09:00:00.000Z".into()));
        early.title = Some("早".into());
        let late = catalog_session(&world, Some("2026-09-08T11:00:00.000Z".into()));
 // updated_at 缺失(存量旧行):读模型回落 created_at
        let legacy = catalog_session(&world, None);
        for s in [early, late, legacy] {
            world.sessions.insert(s.id.clone(), s);
        }

        let items = handle_session_list(&world);
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0].updated_at.as_deref(),
            Some("2026-09-08T11:00:00.000Z"),
            "最近活跃在前"
        );
        assert_eq!(items[1].title.as_deref(), Some("早"));
        assert_eq!(
            items[2].updated_at.as_deref(),
            Some("2026-09-08T08:00:00.000Z"),
            "缺 updated_at 的旧行回落 created_at 排末位"
        );
        assert_eq!(items[2].title, None, "未命名会话标题为 null 交前端回落");
    }
}

#[cfg(test)]
mod m11_share_tests {
    use super::super::*;
    use super::r2_tombstone_tests::test_world;
    use crate::broker::CallContext;
    use bm_contract::capability::{DataTrust, Grant, GrantScope};
    use bm_contract::ids::{IdGen, UlidIdGen};

    const T1: &str = "task_01JAAAAAAAAAAAAAAAAAAAAA0C";
    const T2: &str = "task_01JAAAAAAAAAAAAAAAAAAAAA0D";

    fn share_world() -> World {
        let dir = tempfile::tempdir().expect("tmp");
        let mut w = test_world(dir.path());
        for (m, p) in crate::share::share_capability_entries() {
            w.registry.register(m, "builtin.core", p).expect("注册");
        }
 // 投影存在性 = Task 存在口径(dispatch_share 查 task_board)
        w.task_board.restore_row(T1, "任务一", "running", 1);
        w.task_board.restore_row(T2, "任务二", "running", 1);
        w
    }

    fn worker_ctx(task: &str) -> CallContext {
        CallContext::content_chain(
            crate::team::worker_principal(task).as_str(),
            DataTrust::Untrusted,
        )
        .expect("worker ctx")
    }

    fn seed_grant(w: &mut World, audience: &str, action: &str, task: &str) {
        let scope = serde_json::to_value(GrantScope::Task(task.to_string())).expect("scope");
        let grant: Grant = serde_json::from_value(serde_json::json!({
            "grant_id": if action == crate::share::SHARE_PUBLISH {
                "grant_01JAAAAAAAAAAAAAAAAAAAAA0C"
            } else {
                "grant_01JAAAAAAAAAAAAAAAAAAAAA0D"
            },
            "audience": audience,
            "action": action,
            "resource": {"capability": action, "args_predicates": {}},
            "scope": scope,
            "delegation_depth": 0,
            "expires_at": null,
            "revocation_version": 0,
            "parent_grant_hash": "9b1dec3f2a6c47d5b8e0f1a2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a",
            "issued_by": "surface:user",
            "created_at": "2026-09-10T00:00:00.000Z"
        }))
        .expect("grant 合法");
        w.grants.record(grant);
    }

    fn call(
        w: &mut World,
        ctx: CallContext,
        capability: &str,
        args: serde_json::Value,
    ) -> crate::CoreResult<serde_json::Value> {
        let req = UlidIdGen.next_id("req");
        let (_, r) = capability_call_inner(
            w,
            req,
            ctx,
            wire::CapabilityCallParams {
                capability: capability.into(),
                args,
                idempotency_key: None,
                deadline_ms: None,
            },
        );
        r
    }

 /// 验收门 1/3/5:A 发布 → B 可见;审计双落盘;增量投影 == 重放重建。
 #[test]
    fn publish_then_cross_member_list_with_audit() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut w = test_world(dir.path());
        for (m, p) in crate::share::share_capability_entries() {
            w.registry.register(m, "builtin.core", p).expect("注册");
        }
        w.task_board.restore_row(T1, "任务一", "running", 1);
        let wa = worker_ctx(T1);
        let wb = worker_ctx(T1);
        seed_grant(
            &mut w,
            wa.principal.as_str(),
            crate::share::SHARE_PUBLISH,
            T1,
        );
        seed_grant(&mut w, wb.principal.as_str(), crate::share::SHARE_LIST, T1);

        let r = call(
            &mut w,
            wa,
            crate::share::SHARE_PUBLISH,
            serde_json::json!({"title": "wiki 查到 X", "content": "X 的出处已核实"}),
        );
        let res = r.expect("publish 应成功");
        assert_eq!(res["result"]["published"], true);
        let seq = res["result"]["share_seq"].as_u64().expect("share_seq");

        let r = call(&mut w, wb, crate::share::SHARE_LIST, serde_json::json!({}));
        let res = r.expect("list 应成功");
        assert_eq!(res["result"]["total"], 1);
        assert_eq!(res["result"]["shares"][0]["title"], "wiki 查到 X");
        assert_eq!(res["result"]["shares"][0]["seq"], seq);

 // 审计双落盘:share.published 事实 + capability.invoked 收据
        let events = w
            .store
            .as_ref()
            .expect("store")
            .replay_since(0)
            .expect("replay");
        assert!(
            events
                .iter()
                .any(|e| matches!(e.event_type, EventType::SharePublished))
        );
        assert!(events.iter().any(|e| {
            matches!(e.event_type, EventType::CapabilityInvoked)
                && e.payload["outcome"] == "succeeded"
                && e.payload["capability"] == crate::share::SHARE_PUBLISH
        }));
 // 增量投影 == 重放重建(ADR-0004 条件 1)
        assert_eq!(
            crate::share::TaskShareBoard::rebuild(&events),
            w.share_board
        );
    }

 /// 验收门 2:跨 Task 结构性隔离;args 里的 task_id 不可伪造归属。
 #[test]
    fn cross_task_isolation_and_args_spoof_ignored() {
        let mut w = share_world();
        let w2 = worker_ctx(T2);
        seed_grant(
            &mut w,
            w2.principal.as_str(),
            crate::share::SHARE_PUBLISH,
            T2,
        );
        seed_grant(&mut w, w2.principal.as_str(), crate::share::SHARE_LIST, T2);

 // args 里指定 task_id = T1:归属仍按 principal 落 T2,不落 T1
        let r = call(
            &mut w,
            w2.clone(),
            crate::share::SHARE_PUBLISH,
            serde_json::json!({"task_id": T1, "title": "越界?", "content": "c"}),
        );
        r.expect("publish 应成功");
        assert_eq!(w.share_board.list(T1, 0).len(), 0, "T1 不得被越界写入");
        assert_eq!(w.share_board.list(T2, 0).len(), 1);

 // list 同理:只回自己的公告栏
        let r = call(
            &mut w,
            w2,
            crate::share::SHARE_LIST,
            serde_json::json!({"task_id": T1}),
        );
        let res = r.expect("list 应成功");
        assert_eq!(res["result"]["task_id"], T2);
        assert_eq!(res["result"]["total"], 1);
    }

 /// 验收门 4:非 Task 域主体无公告栏(直通裁决通过、内核执行层拒绝——纵深)。
 #[test]
    fn non_task_principal_is_rejected() {
        let mut w = share_world();
        let r = call(
            &mut w,
            CallContext::surface("surface:user"),
            crate::share::SHARE_PUBLISH,
            serde_json::json!({"title": "t", "content": "c"}),
        );
        match r {
            Err(crate::CoreError::Semantic(code, _)) => {
                assert_eq!(code, ErrorCode::ValidationFailed);
            }
            other => panic!("应 ValidationFailed,实际 {other:?}"),
        }
    }

 /// 权限三层:无 Grant 的 untrusted publish 升级审批(Reversible 生效);
 /// list 默认拒绝(ADR-0006);不存在的 Task 拒绝。
 #[test]
    fn ungranted_worker_escalates_and_unknown_task_rejected() {
        let mut w = share_world();
        let r = call(
            &mut w,
            worker_ctx(T1),
            crate::share::SHARE_PUBLISH,
            serde_json::json!({"title": "t", "content": "c"}),
        );
        assert!(
            matches!(r, Err(crate::CoreError::ApprovalNeeded { .. })),
            "无 Grant publish 应升级审批"
        );
        let r = call(
            &mut w,
            worker_ctx(T1),
            crate::share::SHARE_LIST,
            serde_json::json!({}),
        );
        assert!(r.is_err(), "无 Grant list 应默认拒绝");

 // Task 不存在(投影无此任务)→ 拒绝
        const T3: &str = "task_01JAAAAAAAAAAAAAAAAAAAAA0E";
        let w3 = worker_ctx(T3);
        seed_grant(
            &mut w,
            w3.principal.as_str(),
            crate::share::SHARE_PUBLISH,
            T3,
        );
        let r = call(
            &mut w,
            w3,
            crate::share::SHARE_PUBLISH,
            serde_json::json!({"title": "t", "content": "c"}),
        );
        assert!(r.is_err(), "不存在的 Task 应拒绝");
    }
}
