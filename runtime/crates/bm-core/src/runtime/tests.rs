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

    fn test_world(dir: &std::path::Path) -> World {
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
            model_call_audit: HashMap::new(),
            session_chats: HashMap::new(),
            session_turn_totals: HashMap::new(),
            ctx_log: Arc::new(crate::context_log::ContextLog::new(None)),
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
                turn_timeout_secs: 30,
                max_attempts: None,
                capabilities: vec![],
                async_executor: None,
                model_streaming: false,
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
}
