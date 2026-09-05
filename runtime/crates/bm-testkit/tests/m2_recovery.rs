//! M2.3/M2.4/M2.7 启动恢复与跨进程 resume(任务 T3):
//! - 同目录重启:会话/Agent/Operation/预算账本全部自持久状态就位;
//! - resume 补发历史事件(事件日志为唯一重建依据,ADR-0004 条件 1);
//! - 崩溃遗留的非终态 operation 走 interrupted→resumed 恢复迁移并留审计事件;
//! - runtime.recovered 事件报告恢复面。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{BmId, IdGen, SeqIdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::{GetOperationParams, SessionResumeParams};
use bm_persist::{EventStore, PersistStore};
use bm_providers::mock_model::Step;
use bm_testkit::invariants::assert_event_stream_wellformed;
use bm_testkit::replay::{MODEL_A, rig_on};

fn now_env() -> bm_contract::BmTimestamp {
    bm_contract::timestamp::now()
}

/// 手工写入「崩溃现场」:会话/Agent/回合已创建且进行中,但无终态事件。
fn craft_crash_scene(store: &PersistStore, sess: &BmId, agent: &BmId, op: &BmId) {
    use bm_contract::events::{EventEnvelope, EventType};
    use serde_json::json;
    let ids = SeqIdGen::new();
    // M7 S1:真实运行时在会话创建时已持久 agent 的 model.invoke Grant;
    // 崩溃场景必须如实包含,否则恢复后回合被 Broker 默认拒绝(ADR-0006)。
    let grant = bm_core::butler::model_grant_for(&ids, agent.as_str(), chrono::Utc::now());
    let _ = store.save_grant(bm_persist::sqlite_state::GrantRow {
        id: grant.grant_id.as_str(),
        audience: grant.audience.as_str(),
        action: grant.action.as_str(),
        revocation_version: 0,
        revoked: false,
        used_count: 0,
        payload: &serde_json::to_string(&grant).expect("Grant 可序列化"),
        created_at: grant.created_at.as_str(),
    });
    let mut seq = store.last_log_seq().expect("日志末尾");
    let mut push = |ty: EventType,
                    session: Option<BmId>,
                    agentv: Option<BmId>,
                    opv: Option<BmId>,
                    payload: serde_json::Value| {
        seq += 1;
        let e = EventEnvelope::new(seq, ty, now_env(), session, agentv, opv, payload);
        store.record(&e).expect("手工落盘成功");
    };

    push(
        EventType::SessionCreated,
        Some(sess.clone()),
        None,
        None,
        json!({"session_id": sess.as_str(), "agent_id": agent.as_str()}),
    );
    push(
        EventType::AgentCreated,
        Some(sess.clone()),
        Some(agent.clone()),
        None,
        json!({
            "agent_id": agent.as_str(),
            "session_id": sess.as_str(),
            "model_chain": [MODEL_A],
            "budget": {"max_tokens": 50000, "max_turns": 10},
        }),
    );
    push(
        EventType::AgentTurnStarted,
        Some(sess.clone()),
        Some(agent.clone()),
        Some(op.clone()),
        json!({
            "agent_id": agent.as_str(),
            "operation_id": op.as_str(),
            "turn_index": 1,
        }),
    );
    push(
        EventType::AgentWaitingModel,
        Some(sess.clone()),
        Some(agent.clone()),
        Some(op.clone()),
        json!({
            "agent_id": agent.as_str(),
            "operation_id": op.as_str(),
            "model_id": MODEL_A,
        }),
    );
    let _ = ids;
}

#[tokio::test]
async fn t20_cross_process_resume_and_continue() {
    let dir = tempfile::tempdir().expect("临时目录");
    let rig1 = rig_on(dir.path(), vec![Step::ok("第一答", 412, 58)]).await;
    let (sess, agent) = rig1.create_session().await.expect("会话创建");
    let receipt = rig1.send(&sess, &agent, "第一问").await.expect("回合发起");
    // 等终态
    loop {
        let r = rig1
            .handle
            .operations_get(GetOperationParams {
                operation_id: receipt.operation_id.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let rig1_events = rig1.all_events().await;
    let first_log_len = rig1_events.len();
    // 无会话关联的事件数(runtime.started + M5 起的 12 条 bootstrap Grant 等):
    // resume 补发流只含会话关联事件(合同 events.poll 语义)
    let uncorrelated = rig1_events
        .iter()
        .filter(|e| e.session_id.is_none())
        .count();
    rig1.handle.stop("restart").await;

    // 同目录启动第二台 Runtime:恢复 + 跨进程 resume
    let rig2 = rig_on(dir.path(), vec![Step::ok("第二答", 100, 50)]).await;

    let resumed = rig2
        .handle
        .session_resume(
            rig2.ids.next_id("req"),
            SessionResumeParams {
                session_id: sess.clone(),
                since_seq: Some(0),
            },
        )
        .await
        .expect("跨进程 resume 成功");
    assert_eq!(
        resumed.session_state,
        bm_contract::states::SessionState::Active
    );
    assert_eq!(
        resumed.agent_state,
        bm_contract::states::AgentState::Running
    );
    // resume 补发的是【会话过滤】后的事件:无会话关联的事件
    // (runtime.started + bootstrap Grant 等)不入补发流(合同 events.poll 语义)
    assert_eq!(
        resumed.events.len(),
        first_log_len - uncorrelated,
        "resume 自日志补发全部会话历史"
    );

    // 预算账本恢复:turns_used=1 → 新回合 turn_index=2(INV-7 跨重启)
    let receipt2 = rig2
        .send(&sess, &agent, "第二问")
        .await
        .expect("恢复后可接单");
    let r2 = loop {
        let r = rig2
            .handle
            .operations_get(GetOperationParams {
                operation_id: receipt2.operation_id.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            break r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };
    assert_eq!(r2.state, OperationState::Succeeded);

    let events = rig2.all_events().await;
    assert_event_stream_wellformed(&events);
    // INV-3 跨重启连续:seq 无断档
    let recovered = events
        .iter()
        .find(|e| e.event_type == EventType::RuntimeRecovered)
        .expect("存在 runtime.recovered");
    assert_eq!(recovered.payload["interrupted_recovered"], 0);
    assert_eq!(recovered.payload["replayed"], 0, "正常停机无修复窗口");
    // 第二回合 turn_index = 2
    let started: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::AgentTurnStarted)
        .collect();
    assert_eq!(started.len(), 2);
    assert_eq!(
        started[1].payload["turn_index"], 2,
        "预算账本恢复使回合计数连续"
    );

    rig2.handle.stop("test_done").await;
}

#[tokio::test]
async fn t21_interrupted_operation_recovered_with_audit() {
    let dir = tempfile::tempdir().expect("临时目录");
    let ids = SeqIdGen::new();
    let sess: BmId = ids.next_id("sess");
    let agent: BmId = ids.next_id("agent");
    let op: BmId = ids.next_id("op");

    {
        let store = PersistStore::open(dir.path()).expect("打开");
        craft_crash_scene(&store, &sess, &agent, &op);
        assert_eq!(store.last_log_seq().expect("4 条"), 4);
        // drop = 模拟进程在此刻死亡(operation 永远停在 running)
    }

    // 重启:恢复流程必须把 running 落为 interrupted 并留审计事件
    let rig = rig_on(dir.path(), vec![Step::ok("续答", 10, 5)]).await;

    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);

    let interrupted = events
        .iter()
        .find(|e| e.event_type == EventType::AgentInterrupted)
        .expect("存在 agent.interrupted");
    assert_eq!(interrupted.payload["operation_id"], op.as_str());

    let op_change = events
        .iter()
        .find(|e| {
            e.event_type == EventType::OperationStateChanged
                && e.payload["operation_id"].as_str() == Some(op.as_str())
        })
        .expect("存在该 op 的状态迁移");
    assert_eq!(op_change.payload["from"], "running");
    assert_eq!(op_change.payload["to"], "interrupted");
    assert_eq!(
        op_change.payload["reason_code"],
        "runtime_crash_before_terminal"
    );

    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::AgentResumed)
    );
    let recovered = events
        .iter()
        .find(|e| e.event_type == EventType::RuntimeRecovered)
        .expect("存在 runtime.recovered");
    assert_eq!(recovered.payload["interrupted_recovered"], 1);
    assert_eq!(
        recovered.payload["last_applied_seq"], 4,
        "修复窗口 = 崩溃前全部 4 条"
    );
    // 验证 bus.resumed 发射
    let bus_resumed = events
        .iter()
        .find(|e| e.event_type == EventType::BusResumed)
        .expect("存在 bus.resumed");
    assert_eq!(bus_resumed.payload["component"], "event_bus");

    // 恢复后:同一 agent 可接新单(claim 语义,M2.6 前半),收据仍可查询(INV-6)
    let old_receipt = rig
        .handle
        .operations_get(GetOperationParams {
            operation_id: op.clone(),
        })
        .await
        .expect("旧收据可查询");
    assert_eq!(old_receipt.state, OperationState::Interrupted);

    let receipt2 = rig
        .send(&sess, &agent, "新问题")
        .await
        .expect("恢复后 agent 可接单");
    assert_eq!(receipt2.state, OperationState::Running);

    rig.handle.stop("test_done").await;
}

#[tokio::test]
async fn t25_claim_with_input_content_redrives_turn() {
    // 与 t21 同场景,但崩溃前保存了输入原文 → 恢复必须自动 claim 续跑至终态
    let dir = tempfile::tempdir().expect("临时目录");
    let ids = SeqIdGen::new();
    let sess: BmId = ids.next_id("sess");
    let agent: BmId = ids.next_id("agent");
    let op: BmId = ids.next_id("op");

    {
        let store = PersistStore::open(dir.path()).expect("打开");
        craft_crash_scene(&store, &sess, &agent, &op);
        store
            .save_op_input(op.as_str(), "崩溃时的原始问题")
            .expect("保存输入");
    }

    let rig = rig_on(dir.path(), vec![Step::ok("claim 续答", 30, 15)]).await;

    let events = rig.all_events().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::AgentInterrupted)
    );
    let to_running = events
        .iter()
        .find(|e| {
            e.event_type == EventType::OperationStateChanged
                && e.payload["operation_id"].as_str() == Some(op.as_str())
                && e.payload["to"].as_str() == Some("running")
        })
        .expect("claim 落到 running");
    assert_eq!(to_running.payload["reason_code"], "recovery_replay_ok");

    let receipt = rig
        .handle
        .operations_get(GetOperationParams {
            operation_id: op.clone(),
        })
        .await
        .expect("查询");
    assert_eq!(receipt.state, OperationState::Succeeded, "claim 续跑完成");

    rig.handle.stop("test_done").await;
}

#[tokio::test]
async fn t26_outcome_unknown_requires_ruling_not_retry() {
    // INV-10/11:outcome_unknown 只能经核验/裁定结束;普通重试不得触碰
    let dir = tempfile::tempdir().expect("临时目录");
    let ids = SeqIdGen::new();
    let sess: BmId = ids.next_id("sess");
    let agent: BmId = ids.next_id("agent");
    let op: BmId = ids.next_id("op");

    {
        let store = PersistStore::open(dir.path()).expect("打开");
        craft_crash_scene(&store, &sess, &agent, &op);
        let e = bm_contract::events::EventEnvelope::new(
            store.last_log_seq().expect("末尾") + 1,
            EventType::OperationStateChanged,
            now_env(),
            Some(sess.clone()),
            Some(agent.clone()),
            Some(op.clone()),
            serde_json::json!({
                "operation_id": op.as_str(),
                "from": "running",
                "to": "outcome_unknown",
                "reason_code": "(deadline_exceeded OR crash OR cancel) AND effect_class IN [reversible-command, external-side-effect, high-risk-command]",
            }),
        );
        store.record(&e).expect("落盘");
    }

    let rig = rig_on(dir.path(), vec![Step::ok("续答", 10, 5)]).await;

    let receipt = rig
        .handle
        .operations_get(GetOperationParams {
            operation_id: op.clone(),
        })
        .await
        .expect("查询");
    assert_eq!(
        receipt.state,
        OperationState::OutcomeUnknown,
        "恢复不得自动收口 outcome_unknown"
    );

    let err = rig
        .handle
        .recovery_settle(op.clone(), bm_core::runtime::RecoveryVerdict::Cancelled)
        .await
        .expect_err("非法裁定拒绝");
    assert!(matches!(
        err,
        bm_core::CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));

    let settled = rig
        .handle
        .recovery_settle(op.clone(), bm_core::runtime::RecoveryVerdict::Failed)
        .await
        .expect("裁定成功");
    assert_eq!(settled.state, OperationState::Failed);
    assert_eq!(
        settled.error.as_ref().expect("带错误").code.get(),
        ErrorCode::OutcomeUnknown
    );

    let err2 = rig
        .handle
        .recovery_settle(op.clone(), bm_core::runtime::RecoveryVerdict::Succeeded)
        .await
        .expect_err("终态不可再裁定");
    assert!(matches!(
        err2,
        bm_core::CoreError::Semantic(ErrorCode::ValidationFailed, _)
    ));

    let receipt2 = rig
        .send(&sess, &agent, "新回合")
        .await
        .expect("agent 已恢复可接单");
    assert_eq!(receipt2.state, OperationState::Running);

    rig.handle.stop("test_done").await;
}

#[tokio::test]
async fn t27_cancel_marked_before_crash_restores_stopped_not_running() {
    // 2026-09-05 回看修复回归:用户显式取消后、回合边界落定前崩溃 →
    // 恢复必须尊重取消意图(Resuming→Stopped,turn_was_stopping 契约边),
    // 不得复活接单、不得凭输入原文重驱回合烧模型调用(修复前无条件 Running)。
    let dir = tempfile::tempdir().expect("临时目录");
    let ids = SeqIdGen::new();
    let sess: BmId = ids.next_id("sess");
    let agent: BmId = ids.next_id("agent");
    let op: BmId = ids.next_id("op");

    {
        let store = PersistStore::open(dir.path()).expect("打开");
        craft_crash_scene(&store, &sess, &agent, &op);
        // 输入原文在场(若无视取消标记,claim 会自动重驱——正是要堵的路径)
        store
            .save_op_input(op.as_str(), "崩溃前用户已取消的问题")
            .expect("保存输入");
        // 用户取消请求已落标记(崩溃发生在回合边界落定前)
        store
            .mark_op_cancelled(op.as_str(), "2026-09-05T00:00:00Z")
            .expect("写取消标记");
        // drop = 模拟进程在此刻死亡
    }

    let rig = rig_on(dir.path(), vec![Step::ok("不该被调用", 30, 15)]).await;

    let events = rig.all_events().await;
    assert_event_stream_wellformed(&events);

    // operation:running→interrupted(崩溃语义)→ cancelled(user_ruling)
    let to_cancelled = events
        .iter()
        .find(|e| {
            e.event_type == EventType::OperationStateChanged
                && e.payload["operation_id"].as_str() == Some(op.as_str())
                && e.payload["to"].as_str() == Some("cancelled")
        })
        .expect("取消标记使 op 落 cancelled");
    assert_eq!(to_cancelled.payload["reason_code"], "user_ruling");

    // agent:agent.cancelled 在案,且此后无 agent.resumed(不复活)
    let cancelled = events
        .iter()
        .find(|e| e.event_type == EventType::AgentCancelled)
        .expect("存在 agent.cancelled");
    assert_eq!(cancelled.payload["operation_id"], op.as_str());
    assert!(
        !events
            .iter()
            .any(|e| e.event_type == EventType::AgentResumed),
        "已取消的 agent 不得复活接单"
    );

    // 恢复后 agent 为 stopped:同会话发消息被拒(停止不可接单,取消是唯一入口语义)
    let err = rig
        .send(&sess, &agent, "取消后不应接单")
        .await
        .expect_err("stopped agent 不得接单");
    assert!(matches!(err, bm_core::CoreError::Semantic(_, _)));

    // 旧收据:终态 cancelled
    let old_receipt = rig
        .handle
        .operations_get(GetOperationParams {
            operation_id: op.clone(),
        })
        .await
        .expect("旧收据可查询");
    assert_eq!(old_receipt.state, OperationState::Cancelled);

    rig.handle.stop("test_done").await;
}

#[tokio::test]
async fn t28_session_chat_ledger_rebuilt_after_restart() {
    // 重启续聊(2026-09-06):重启后同会话再发消息,模型请求必须带上
    // 重建的历史台账(修复前 session_chats 纯内存,重启即失忆)。
    let dir = tempfile::tempdir().expect("临时目录");
    let mut last_receipt = None;
    {
        let rig1 = rig_on(
            dir.path(),
            vec![Step::ok("回答一", 30, 15), Step::ok("回答二", 30, 15)],
        )
        .await;
        let (sess, agent) = rig1.create_session().await.expect("会话创建");
        for q in ["问题一", "问题二"] {
            let receipt = rig1.send(&sess, &agent, q).await.expect("发送");
            loop {
                let r = rig1
                    .handle
                    .operations_get(GetOperationParams {
                        operation_id: receipt.operation_id.clone(),
                    })
                    .await
                    .expect("查询");
                if r.state.is_terminal() {
                    assert_eq!(r.state, OperationState::Succeeded, "{q} 应成功");
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            last_receipt = Some((sess.clone(), agent.clone()));
        }
        rig1.handle.stop("restart").await;
    }
    let (sess, agent) = last_receipt.expect("会话信息");

    // 重启:同目录,同会话继续
    let rig2 = rig_on(dir.path(), vec![Step::ok("回答三", 30, 15)]).await;
    let receipt = rig2
        .send(&sess, &agent, "问题三")
        .await
        .expect("重启后同会话可继续(会话自持久层装载)");
    loop {
        let r = rig2
            .handle
            .operations_get(GetOperationParams {
                operation_id: receipt.operation_id.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            assert_eq!(r.state, OperationState::Succeeded);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    // 断言:第三轮的模型调用快照(无 kind 的行)里,请求 messages 含
    // 重建出的前两轮历史
    let text = std::fs::read_to_string(dir.path().join("context-log.jsonl")).expect("读日志");
    let last_snapshot = text
        .lines()
        .rev()
        .find(|l| {
            serde_json::from_str::<serde_json::Value>(l)
                .map(|v| v.get("kind").is_none())
                .unwrap_or(false)
        })
        .expect("存在第三轮模型调用快照");
    let v: serde_json::Value = serde_json::from_str(last_snapshot).expect("快照合法");
    let msgs = v["messages"].as_array().expect("messages 数组");
    let all: String = msgs
        .iter()
        .map(|m| m["content"].as_str().unwrap_or_default())
        .collect();
    assert!(
        all.contains("问题一") && all.contains("回答一") && all.contains("问题二"),
        "重启后模型请求必须重建历史,实际: {all}"
    );
    assert!(all.contains("问题三"), "当前轮用户消息在场");
    rig2.handle.stop("test_done").await;
}

#[tokio::test]
async fn t29_session_delete_tombstone_and_erase() {
    // 会话删除 A+B(2026-09-06):墓碑 + 对话原文擦除,一次落地。
    let dir = tempfile::tempdir().expect("临时目录");
    let rig1 = rig_on(
        dir.path(),
        vec![Step::ok("回答一", 30, 15), Step::ok("回答二", 30, 15)],
    )
    .await;
    let (sess, agent) = rig1.create_session().await.expect("会话创建");
    for q in ["要被删除的问题一", "要被删除的问题二"] {
        let receipt = rig1.send(&sess, &agent, q).await.expect("发送");
        loop {
            let r = rig1
                .handle
                .operations_get(GetOperationParams {
                    operation_id: receipt.operation_id.clone(),
                })
                .await
                .expect("查询");
            if r.state.is_terminal() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }
    // 前置:context-log 里有该会话的行
    let log_path = dir.path().join("context-log.jsonl");
    let before = std::fs::read_to_string(&log_path).expect("读日志");
    assert!(before.contains(sess.as_str()) && before.contains("要被删除的问题一"));

    // 删除
    let result = rig1
        .handle
        .session_delete(
            SeqIdGen::new().next_id("req"),
            bm_contract::wire::SessionDeleteParams {
                session_id: sess.clone(),
            },
        )
        .await
        .expect("删除成功");
    assert!(
        result.purged_lines >= 4,
        "至少擦除两轮的 4 行,实际 {}",
        result.purged_lines
    );

    // ① context-log 无该会话行
    let after = std::fs::read_to_string(&log_path).expect("读日志");
    assert!(!after.contains(sess.as_str()), "context-log 不得残留会话行");
    // ② 持久侧:墓碑在场;input_content 全空;session 行已删
    {
        let store = bm_persist::PersistStore::open(dir.path()).expect("重开");
        let tomb = store
            .state()
            .query_rows("SELECT id FROM tombstones WHERE kind='session'", &[])
            .expect("读墓碑");
        assert!(
            tomb.iter().any(|v| v["id"].as_str() == Some(sess.as_str())),
            "墓碑在场"
        );
        let ops = store
            .state()
            .query_rows(
                "SELECT input_content FROM operations WHERE session_id = ?1",
                &[&sess.as_str()],
            )
            .expect("读操作行");
        assert!(ops.iter().all(|v| v["input_content"].is_null()), "原文已擦");
        let sess_rows = store
            .state()
            .query_rows("SELECT id FROM sessions WHERE id = ?1", &[&sess.as_str()])
            .expect("读会话行");
        assert!(sess_rows.is_empty(), "会话行已删");
    }
    // ③ resume 拒绝(不可继续聊已删会话)
    let err = rig1
        .handle
        .session_resume(
            SeqIdGen::new().next_id("req"),
            bm_contract::wire::SessionResumeParams {
                session_id: sess.clone(),
                since_seq: Some(u64::MAX),
            },
        )
        .await
        .expect_err("已删会话 resume 必须失败");
    assert!(matches!(err, bm_core::CoreError::Semantic(_, _)));
    // ④ 再发消息拒绝
    assert!(rig1.send(&sess, &agent, "还收吗").await.is_err());
    rig1.handle.stop("test_done").await;

    // ⑤ 重启后:台账重建跳过墓碑(会话行已删,不会复活),回放端点无残留
    let rig2 = rig_on(dir.path(), vec![Step::ok("新答", 10, 5)]).await;
    let log_after_restart = std::fs::read_to_string(&log_path).expect("读日志");
    assert!(!log_after_restart.contains(sess.as_str()));
    rig2.handle.stop("test_done").await;
}
