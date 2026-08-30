//! M8-T4:数据面质量——在线备份/恢复(v118)、v7→v8 迁移演练、
//! 损坏隔离(S5)、用户删除墓碑回放(M8.8)、执行日志保留期修剪。

use bm_contract::ids::{BmId, IdGen, SeqIdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::{CapabilityCallParams, GetOperationParams};
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_providers::mock_model::{MockConnector, Step};
use bm_testkit::replay::rig_on;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// t118:在线备份 → 恢复 → 历史会话不损坏(resume + 新回合成功 +
/// 旧收据保持 succeeded)。运行中取快照(VACUUM INTO + 事件日志拷贝)。
#[tokio::test]
async fn t118_backup_restore_history_intact() {
    let dir = tempfile::tempdir().expect("临时目录");
    let rig1 = rig_on(dir.path(), vec![Step::ok("第一答", 100, 40)]).await;
    let (sess, agent) = rig1.create_session().await.expect("会话创建");
    let r1 = rig1.send(&sess, &agent, "第一问").await.expect("回合发起");
    wait_done(&rig1.handle, &r1.operation_id).await;
    let old_receipt = rig1
        .handle
        .operations_get(GetOperationParams {
            operation_id: r1.operation_id.clone(),
        })
        .await
        .expect("旧收据");
    assert_eq!(old_receipt.state, OperationState::Succeeded);
    rig1.handle.stop("backup").await;

    // 在线备份:同位点原子快照(state.db + events.jsonl + manifest)
    let backup = tempfile::tempdir().expect("备份目录");
    {
        let store = bm_persist::PersistStore::open(dir.path()).expect("打开");
        store.backup_into(backup.path()).expect("备份");
        bm_persist::PersistStore::verify_backup(backup.path()).expect("备份校验必须通过");
        assert!(
            backup.path().join("manifest.json").exists(),
            "位点清单必须存在"
        );
    }
    // 篡改检测:改坏 state.db → verify 必须拒绝(X-04 验收 3)
    {
        let tampered = backup.path().join("state.db");
        let mut data = std::fs::read(&tampered).expect("读");
        let mid = data.len() / 2;
        data[mid] ^= 0xFF;
        std::fs::write(&tampered, &data).expect("写");
        assert!(
            bm_persist::PersistStore::verify_backup(backup.path()).is_err(),
            "篡改后的备份必须被拒绝"
        );
        let store = bm_persist::PersistStore::open(dir.path()).expect("打开");
        store.backup_into(backup.path()).expect("重新备份");
        bm_persist::PersistStore::verify_backup(backup.path()).expect("重新校验");
    }

    // 恢复:副本打开 → 历史会话 resume → 新回合成功;旧收据不变
    let rig2 = rig_on(backup.path(), vec![Step::ok("恢复后新答", 80, 30)]).await;
    let resumed = rig2
        .handle
        .session_resume(
            rig2.ids.next_id("req"),
            bm_contract::wire::SessionResumeParams {
                session_id: sess.clone(),
                since_seq: Some(0),
            },
        )
        .await
        .expect("历史会话必须可 resume(基线 M8 通过条件)");
    assert_ne!(resumed.session_state.as_str(), "closed", "会话状态不损坏");

    let old_after = rig2
        .handle
        .operations_get(GetOperationParams {
            operation_id: r1.operation_id.clone(),
        })
        .await
        .expect("旧收据可查");
    assert_eq!(old_after.state, OperationState::Succeeded, "历史收据不损坏");

    let r2 = rig2
        .send(&sess, &agent, "恢复后的新问题")
        .await
        .expect("新回合");
    wait_done(&rig2.handle, &r2.operation_id).await;
    let receipt2 = rig2
        .handle
        .operations_get(GetOperationParams {
            operation_id: r2.operation_id.clone(),
        })
        .await
        .expect("新收据");
    assert_eq!(receipt2.state, OperationState::Succeeded);
    rig2.handle.stop("done").await;
}

/// t119a:v7→v8 迁移演练——旧版本库打开后自动迁移,既有数据零丢失,
/// 新表(评估报告)可用。
#[tokio::test]
async fn t119a_migration_v7_to_v8_data_intact() {
    let dir = tempfile::tempdir().expect("临时目录");
    // v8 现行代码建库 + 写入 Grant
    {
        let store = bm_persist::PersistStore::open(dir.path()).expect("建库");
        let ids = SeqIdGen::new();
        let grant = bm_core::butler::model_grant_for(
            &ids,
            "agent_01JAAAAAAAAAAAAAAAAAAAAAA1",
            chrono_now(),
        );
        bm_persist::EventStore::save_grant(
            &store,
            bm_persist::sqlite_state::GrantRow {
                id: grant.grant_id.as_str(),
                audience: grant.audience.as_str(),
                action: grant.action.as_str(),
                revocation_version: 0,
                revoked: false,
                used_count: 0,
                payload: &serde_json::to_string(&grant).unwrap(),
                created_at: grant.created_at.as_str(),
            },
        )
        .expect("写 grant");
    }
    // 降形模拟旧库:user_version=7 + 删新表
    {
        let conn = rusqlite::Connection::open(dir.path().join("state.db")).expect("打开");
        conn.execute_batch("DROP TABLE IF EXISTS evaluation_reports; PRAGMA user_version = 7;")
            .expect("降形");
    }
    // 重开:自动迁移回 v8,既有行不丢
    let store = bm_persist::PersistStore::open(dir.path()).expect("迁移重开");
    let grants = bm_persist::EventStore::list_grants(&store).expect("grant 行存活");
    assert!(
        grants
            .iter()
            .any(|g| g["action"].as_str() == Some("model.invoke")),
        "迁移后既有数据零丢失:{grants:?}"
    );
    // 新表可用
    bm_persist::EventStore::save_evaluation_report(
        &store,
        "rep_01JAAAAAAAAAAAAAAAAAAAAAA1",
        1,
        9,
        "{\"ok\":true}",
        "2026-08-30T12:00:00.000Z",
    )
    .expect("评估报告写入(v8 表已建)");
    assert_eq!(
        bm_persist::EventStore::list_evaluation_reports(&store)
            .expect("列表")
            .len(),
        1
    );
}

/// t119b:状态库损坏 → open_resilient 隔离坏文件 + 自事件日志重建投影(S5)。
#[tokio::test]
async fn t119b_corrupt_state_quarantined_and_rebuilt() {
    let dir = tempfile::tempdir().expect("临时目录");
    let rig = rig_on(dir.path(), vec![Step::ok("答", 50, 20)]).await;
    let (sess, _agent) = rig.create_session().await.expect("会话创建");
    rig.handle.stop("done").await;

    // 写坏状态库:拷贝到独立目录再破坏(原目录句柄已由上一代持有)
    let corrupt_dir = tempfile::tempdir().expect("破坏目录");
    for name in ["state.db", "events.jsonl"] {
        let src = dir.path().join(name);
        if src.exists() {
            std::fs::copy(&src, corrupt_dir.path().join(name)).expect("拷贝");
        }
    }
    let _ = std::fs::remove_file(corrupt_dir.path().join("state.db-wal"));
    std::fs::write(
        corrupt_dir.path().join("state.db"),
        b"not a sqlite database at all",
    )
    .expect("写坏");

    // 常规打开报损坏;resilient 打开 = 隔离 + 重建
    assert!(bm_persist::PersistStore::open(corrupt_dir.path()).is_err());
    let (store, rebuilt) =
        bm_persist::PersistStore::open_resilient(corrupt_dir.path()).expect("resilient");
    assert!(rebuilt, "必须标记重建");
    let rows = bm_persist::EventStore::load_rows(&store).expect("投影行");
    assert!(
        rows.sessions.iter().any(|s| s.id.as_str() == sess.as_str()),
        "会话投影自事件日志重建"
    );
    // 隔离文件留存(取证面)
    let quarantined: Vec<_> = std::fs::read_dir(corrupt_dir.path())
        .expect("列目录")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
        .collect();
    assert!(!quarantined.is_empty(), "损坏文件必须留档隔离");
}

/// t119c:用户删除墓碑回放——memory.delete 后重启(事件日志重放),
/// 已删条目不复活;未删条目可检索(M8.8)。
#[tokio::test]
async fn t119c_user_deletion_tombstone_survives_replay() {
    let dir = tempfile::tempdir().expect("临时目录");
    let dir_path = dir.path().to_path_buf();
    std::mem::forget(dir); // 目录跨两代 Runtime 存续

    let ids = Arc::new(SeqIdGen::new());
    let scope = "memory:user".to_string(); // 合同形态:裸 user 域(scope_ok)

    // 第一代:写 A/B,删 A
    {
        let store: Arc<dyn bm_persist::EventStore> =
            Arc::new(bm_persist::PersistStore::open(&dir_path).expect("打开"));
        let connector = Arc::new(MockConnector::new(vec![]));
        let mut caps = Vec::new();
        caps.extend(bm_core::memory::memory_capabilities(
            store.clone(),
            ids.clone(),
        ));
        let handle = RuntimeHandle::start(RuntimeConfig {
            capabilities: caps,
            async_executor: None,
            version: "0.1.0-m8".into(),
            data_dir: Some(dir_path.clone()),
            store: Some(store),
            connector,
            secret_store: Arc::new(bm_providers::secret::MemSecretStore::with(
                "secret:model.x",
                "sk",
            )),
            id_gen: ids.clone(),
            clock: Arc::new(bm_core::clock::SystemClock),
            turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
            max_attempts: None,
        })
        .await;
        let call = |r: BmId, cap: &str, args: serde_json::Value| {
            let h = handle.clone();
            let cap = cap.to_string();
            async move {
                h.capability_call(
                    r,
                    CapabilityCallParams {
                        capability: cap,
                        args,
                        idempotency_key: None,
                        deadline_ms: Some(5_000),
                    },
                )
                .await
            }
        };
        let a = call(
            ids.next_id("req"),
            "memory.write",
            json!({"scope": scope, "content_ref": "protected://mem/a",
                   "content_preview": "用户偏好:深色主题"}),
        )
        .await;
        let a = a.expect("写 A");
        let entry_a = a["result"]["entry_id"].as_str().unwrap().to_string();
        call(
            ids.next_id("req"),
            "memory.write",
            json!({"scope": scope, "content_ref": "protected://mem/b",
                   "content_preview": "普通备注"}),
        )
        .await
        .expect("写 B");
        // 删 A(reversible → 审批 → once 批准 → 重放执行)
        let err = call(
            ids.next_id("req"),
            "memory.delete",
            json!({"entry_id": entry_a}),
        )
        .await
        .expect_err("删除需审批");
        assert!(matches!(
            err,
            bm_core::CoreError::Semantic(bm_contract::error_codes::ErrorCode::ApprovalRequired, _)
        ));
        let list = handle
            .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
            .await
            .unwrap();
        let del_appr = list["approvals"][0]["approval_id"]
            .as_str()
            .unwrap()
            .to_string();
        handle
            .approval_respond(
                ids.next_id("req"),
                bm_contract::wire::ApprovalRespondParams {
                    approval_id: BmId::parse(&del_appr).unwrap(),
                    decision: "approve".into(),
                    scope: Some("once".into()),
                },
            )
            .await
            .expect("批准删除");
        handle.stop("gen1_done").await;
    }

    // 第二代(重放):已删不复活;未删可检索
    {
        let store: Arc<dyn bm_persist::EventStore> =
            Arc::new(bm_persist::PersistStore::open(&dir_path).expect("重开"));
        let connector = Arc::new(MockConnector::new(vec![]));
        let mut caps = Vec::new();
        caps.extend(bm_core::memory::memory_capabilities(
            store.clone(),
            ids.clone(),
        ));
        let handle = RuntimeHandle::start(RuntimeConfig {
            capabilities: caps,
            async_executor: None,
            version: "0.1.0-m8".into(),
            data_dir: Some(dir_path.clone()),
            store: Some(store),
            connector,
            secret_store: Arc::new(bm_providers::secret::MemSecretStore::with(
                "secret:model.x",
                "sk",
            )),
            id_gen: ids.clone(),
            clock: Arc::new(bm_core::clock::SystemClock),
            turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
            max_attempts: None,
        })
        .await;
        let call = |r: BmId, cap: &str, args: serde_json::Value| {
            let h = handle.clone();
            let cap = cap.to_string();
            async move {
                h.capability_call(
                    r,
                    CapabilityCallParams {
                        capability: cap,
                        args,
                        idempotency_key: None,
                        deadline_ms: Some(5_000),
                    },
                )
                .await
            }
        };
        let found_deleted = call(
            ids.next_id("req"),
            "memory.search",
            json!({"scope": scope, "query": "深色主题"}),
        )
        .await
        .expect("检索已删");
        assert_eq!(
            found_deleted["result"]["count"],
            json!(0),
            "重放后已删条目不得复活(墓碑回放)"
        );
        let found_kept = call(
            ids.next_id("req"),
            "memory.search",
            json!({"scope": scope, "query": "普通备注"}),
        )
        .await
        .expect("检索保留");
        assert_eq!(
            found_kept["result"]["count"],
            json!(1),
            "未删条目照常可检索"
        );
        handle.stop("gen2_done").await;
    }
}

/// t119d:执行日志保留期修剪——旧条目清除、新条目保留;事件日志不受影响。
#[tokio::test]
async fn t119d_exec_log_retention_prune() {
    let dir = tempfile::tempdir().expect("临时目录");
    let log = bm_core::exec_log::ExecutionLog::new(Some(dir.path()));
    let id = || BmId::generate("op");
    let mk_record = |ts: &str| bm_core::exec_log::LogRecord {
        kind: bm_contract::exec_log::LogKind::AgentTurn,
        session_id: id(),
        agent_id: id(),
        operation_id: id(),
        request_id: None,
        agent_state: "running".into(),
        detail: json!({"note": "x"}),
        ts: ts.into(),
    };
    log.record(mk_record("2026-01-01T00:00:00.000Z"));
    log.record(mk_record("2026-08-30T00:00:00.000Z"));
    assert_eq!(log.len(), 2);

    // 保留 2026-03-01 之后 → 旧条目被清除
    let removed = log.prune_before("2026-03-01T00:00:00.000Z");
    assert_eq!(removed, 1);
    assert_eq!(log.len(), 1);
    assert_eq!(log.entries()[0].ts, "2026-08-30T00:00:00.000Z");

    // 全保留边界:cutoff 早于全部 → 0 删除
    assert_eq!(log.prune_before("2020-01-01T00:00:00.000Z"), 0);
}

// ---- 本地辅助 ---------------------------------------------------------------

async fn wait_done(handle: &RuntimeHandle, op: &BmId) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        assert!(tokio::time::Instant::now() < deadline, "10s 未终态");
        let r = handle
            .operations_get(GetOperationParams {
                operation_id: op.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn chrono_now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

/// t119e(外部审计 X-03 验收):先压实日志前缀、再损坏状态库 →
/// open_resilient 必须 fail-closed 拒绝自动重建(而非静默生成不完整状态)。
#[tokio::test]
async fn t119e_compacted_corrupt_recovery_fails_closed() {
    let dir = tempfile::tempdir().expect("临时目录");
    let rig = rig_on(
        dir.path(),
        vec![Step::ok("答1", 30, 10), Step::ok("答2", 20, 8)],
    )
    .await;
    let (sess, agent) = rig.create_session().await.expect("会话创建");
    let r = rig.send(&sess, &agent, "问题").await.expect("回合发起");
    wait_done(&rig.handle, &r.operation_id).await;
    rig.handle.stop("done").await;

    // 压实前缀:快照位点推进到中段再截断
    {
        let store = bm_persist::PersistStore::open(dir.path()).expect("打开");
        let last = bm_persist::EventStore::last_log_seq(&store).expect("末尾");
        let mid = last / 2;
        bm_persist::EventStore::snapshot(&store).expect("快照");
        bm_persist::EventStore::compact(&store, mid).expect("压实");
    }
    // 损坏状态库:拷贝到独立目录后破坏副本(原目录句柄由上一代持有)
    let corrupt_dir = tempfile::tempdir().expect("破坏目录");
    for name in ["state.db", "events.jsonl"] {
        let src = dir.path().join(name);
        if src.exists() {
            std::fs::copy(&src, corrupt_dir.path().join(name)).expect("拷贝");
        }
    }
    let _ = std::fs::remove_file(corrupt_dir.path().join("state.db-wal"));
    std::fs::write(corrupt_dir.path().join("state.db"), b"garbage-state-db").expect("写坏");

    // fail-closed:必须拒绝自动重建,理由指向快照恢复
    let res = bm_persist::PersistStore::open_resilient(corrupt_dir.path());
    assert!(res.is_err(), "压实 + 损坏必须拒绝自动重建");
    let msg = format!("{:?}", res.err().unwrap());
    assert!(
        msg.contains("压实") || msg.contains("快照"),
        "拒绝理由必须指向快照恢复:{msg}"
    );
}
