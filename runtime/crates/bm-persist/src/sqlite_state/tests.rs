//! sqlite_state 测试(自 sqlite_state.rs 机械移入;内容零改动)。

use super::*;
use crate::error::StoreError;

#[test]
fn open_migrates_and_sets_version() {
    let dir = tempfile::tempdir().expect("临时目录");
    let db = StateDb::open(&dir.path().join("state.db")).expect("打开");
    assert_eq!(
        db.meta_get("last_applied_seq").expect("读"),
        None,
        "新库 meta 为空"
    );
}

#[test]
fn meta_cas() {
    let dir = tempfile::tempdir().expect("临时目录");
    let db = StateDb::open(&dir.path().join("state.db")).expect("打开");

    // 初始写入经 CAS(expect = None → 不存在时插入)
    db.meta_compare_and_set("last_applied_seq", None, "5")
        .expect("插入");
    assert_eq!(
        db.meta_get("last_applied_seq").expect("读"),
        Some("5".into())
    );

    // CAS 成功:expect 匹配
    db.meta_compare_and_set("last_applied_seq", Some("5"), "9")
        .expect("CAS 成功");
    assert_eq!(
        db.meta_get("last_applied_seq").expect("读"),
        Some("9".into())
    );

    // CAS 失败:expect 过期 → CasMismatch,值不变
    let err = db
        .meta_compare_and_set("last_applied_seq", Some("5"), "99")
        .expect_err("过期 expect 必须被拒");
    assert!(matches!(err, StoreError::CasMismatch { .. }));
    assert_eq!(
        db.meta_get("last_applied_seq").expect("读"),
        Some("9".into())
    );

    // CAS 对不存在的键:expect = None
    db.meta_compare_and_set("fresh", None, "1")
        .expect("absent 分支");
    assert_eq!(db.meta_get("fresh").expect("读"), Some("1".into()));
}

#[test]
fn v3_tables_roundtrip() {
    let dir = tempfile::tempdir().expect("临时目录");
    let db = StateDb::open(&dir.path().join("state.db")).expect("打开");
    let version: i64 = {
        let conn = db.conn.lock().expect("锁未中毒");
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(version, SCHEMA_VERSION);

    // approvals:upsert + 状态过滤
    db.save_approval(ApprovalRow {
        id: "appr_01JAAAAAAAAAAAAAAAAAAAAA04",
        operation_id: "op_01JAAAAAAAAAAAAAAAAAAAAA0A",
        capability: "system.danger.purge",
        principal: "surface:user",
        state: "waiting_user",
        payload: r#"{"approval_id":"appr_01JAAAAAAAAAAAAAAAAAAAAA04"}"#,
        created_at: "2026-08-29T10:00:00.220Z",
        resolved_at: None,
    })
    .expect("写 approval");
    db.save_approval(ApprovalRow {
        id: "appr_01JAAAAAAAAAAAAAAAAAAAAA04",
        operation_id: "op_01JAAAAAAAAAAAAAAAAAAAAA0A",
        capability: "system.danger.purge",
        principal: "surface:user",
        state: "denied",
        payload: r#"{"approval_id":"appr_01JAAAAAAAAAAAAAAAAAAAAA04","state":"denied"}"#,
        created_at: "2026-08-29T10:00:00.220Z",
        resolved_at: Some("2026-08-29T10:02:00.000Z"),
    })
    .expect("更新 approval");
    assert_eq!(
        db.list_approvals_by_state("waiting_user").unwrap().len(),
        0,
        "resolved 后不再处于 waiting"
    );
    let p = db
        .approval_payload("appr_01JAAAAAAAAAAAAAAAAAAAAA04")
        .unwrap()
        .expect("payload 在");
    assert!(p.contains("denied"));

    // grants:写 + 撤销 + 恢复面(T6c 起消费计数随行持久)
    db.save_grant(GrantRow {
        id: "grant_01JAAAAAAAAAAAAAAAAAAAAA0C",
        audience: "agent:note_bot",
        action: "system.notes.write",
        revocation_version: 0,
        revoked: false,
        used_count: 0,
        payload: r#"{"grant_id":"grant_01JAAAAAAAAAAAAAAAAAAAAA0C"}"#,
        created_at: "2026-08-29T10:02:09.500Z",
    })
    .expect("写 grant");
    db.save_grant(GrantRow {
        id: "grant_01JAAAAAAAAAAAAAAAAAAAAA0C",
        audience: "agent:note_bot",
        action: "system.notes.write",
        revocation_version: 1,
        revoked: true,
        used_count: 3,
        payload: r#"{"grant_id":"grant_01JAAAAAAAAAAAAAAAAAAAAA0C","revocation_version":1}"#,
        created_at: "2026-08-29T10:02:09.500Z",
    })
    .expect("撤销 grant");
    let grants = db.list_grants().unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0]["revoked"], serde_json::json!(1));
    assert_eq!(grants[0]["revocation_version"], serde_json::json!(1));
    assert_eq!(
        grants[0]["used_count"],
        serde_json::json!(3),
        "T6c:消费余量持久"
    );

    // capabilities:epoch 持久计数
    db.save_capability_binding(CapabilityRow {
        capability: "system.echo",
        provider_instance_id: "system.echo@0.1.0",
        epoch: 7,
        status: "active",
        manifest: r#"{"capability":"system.echo"}"#,
        updated_at: "2026-08-29T10:00:00.100Z",
    })
    .expect("写 binding");
    db.save_capability_binding(CapabilityRow {
        capability: "system.echo",
        provider_instance_id: "system.echo@0.2.0",
        epoch: 8,
        status: "active",
        manifest: r#"{"capability":"system.echo"}"#,
        updated_at: "2026-08-29T10:05:00.100Z",
    })
    .expect("切 binding");
    let caps = db.list_capability_bindings().unwrap();
    assert_eq!(caps.len(), 1);
    assert_eq!(caps[0]["epoch"], serde_json::json!(8));

    // outbox:upsert + 状态列表(T6 对账底座)
    db.outbox_upsert(
        "op_01JAAAAAAAAAAAAAAAAAAAAA0A",
        "side_effect",
        "pending",
        r#"{"n":1}"#,
        "2026-08-29T10:06:00.000Z",
    )
    .expect("upsert outbox");
    db.outbox_upsert(
        "op_01JAAAAAAAAAAAAAAAAAAAAA0A",
        "side_effect",
        "verified",
        r#"{"n":2}"#,
        "2026-08-29T10:07:00.000Z",
    )
    .expect("推进 outbox");
    assert_eq!(db.list_outbox_by_state("pending").unwrap().len(), 0);
    let verified = db.list_outbox_by_state("verified").unwrap();
    assert_eq!(verified.len(), 1);
    assert_eq!(verified[0]["payload"], serde_json::json!(r#"{"n":2}"#));
}

/// expand-contract:v2 库打开自动升 v3,既有行不受影响(ADR-0003 对偶)。
#[test]
fn v2_database_upgrades_to_v3_keeping_rows() {
    let dir = tempfile::tempdir().expect("临时目录");
    let path = dir.path().join("state.db");
    {
        let conn = rusqlite::Connection::open(&path).expect("建 v2 库");
        conn.execute_batch(
            r#"
            CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY, state TEXT NOT NULL,
                agent_id TEXT NOT NULL, created_at TEXT NOT NULL);
            INSERT INTO sessions VALUES('sess_01JAAAAAAAAAAAAAAAAAAAAA0B',
                'active', 'agent_01JAAAAAAAAAAAAAAAAAAAAA0C',
                '2026-08-28T10:00:00.000Z');
            PRAGMA user_version = 2;
            "#,
        )
        .expect("v2 schema");
    }
    let db = StateDb::open(&path).expect("打开 v2 库(自动迁移)");
    let rows = db
        .query_rows("SELECT id FROM sessions", &[])
        .expect("读旧表");
    assert_eq!(rows.len(), 1, "v2 既有行保留");
    assert_eq!(
        db.list_capability_bindings().unwrap().len(),
        0,
        "v3 新表为空"
    );
}

/// expand-contract:v3 库打开自动升 v4,既有 grants 行的 used_count 取默认 0。
#[test]
fn v3_database_upgrades_to_v4_keeping_rows() {
    let dir = tempfile::tempdir().expect("临时目录");
    let path = dir.path().join("state.db");
    {
        let conn = rusqlite::Connection::open(&path).expect("建 v3 库");
        conn.execute_batch(
            r#"
            CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY, state TEXT NOT NULL,
                agent_id TEXT NOT NULL, created_at TEXT NOT NULL);
            CREATE TABLE grants (
                id TEXT PRIMARY KEY, audience TEXT NOT NULL, action TEXT NOT NULL,
                revocation_version INTEGER NOT NULL DEFAULT 0,
                revoked INTEGER NOT NULL DEFAULT 0,
                payload TEXT NOT NULL, created_at TEXT NOT NULL);
            INSERT INTO grants VALUES('grant_01JAAAAAAAAAAAAAAAAAAAAA0C',
                'agent:note_bot', 'system.notes.write', 0, 0, '{}',
                '2026-08-29T10:02:09.500Z');
            PRAGMA user_version = 3;
            "#,
        )
        .expect("v3 schema");
    }
    let db = StateDb::open(&path).expect("打开 v3 库(自动迁移)");
    let grants = db.list_grants().unwrap();
    assert_eq!(grants.len(), 1, "v3 既有行保留");
    assert_eq!(grants[0]["used_count"], serde_json::json!(0), "新列默认 0");
    assert_eq!(db.list_tasks().unwrap().len(), 0, "v4 新表为空");
}

/// v4:tasks 行往返 + task_epoch 单调推进 + 幂等收据(T6c)。
#[test]
fn v4_tasks_and_idem_receipts_roundtrip() {
    let dir = tempfile::tempdir().expect("临时目录");
    let db = StateDb::open(&dir.path().join("state.db")).expect("打开");

    let payload =
        r#"{"task_id":"task_01JAAAAAAAAAAAAAAAAAAAAAB2","state":"running","task_epoch":1}"#;
    db.save_task(TaskRow {
        id: "task_01JAAAAAAAAAAAAAAAAAAAAAB2",
        title: "整理读书笔记",
        state: "created",
        parent_task_id: None,
        delegation_depth: 0,
        created_by: "butler:system",
        task_epoch: 1,
        payload,
        created_at: "2026-08-29T11:00:01.000Z",
        updated_at: "2026-08-29T11:00:01.000Z",
    })
    .expect("写 task");
    db.save_task(TaskRow {
        id: "task_01JAAAAAAAAAAAAAAAAAAAAAB2",
        title: "整理读书笔记",
        state: "paused",
        parent_task_id: None,
        delegation_depth: 2,
        created_by: "butler:system",
        task_epoch: 2,
        payload,
        created_at: "2026-08-29T11:00:01.000Z",
        updated_at: "2026-08-29T11:05:00.000Z",
    })
    .expect("推进 task");
    let tasks = db.list_tasks().unwrap();
    assert_eq!(tasks.len(), 1, "upsert 不重复建行");
    assert_eq!(tasks[0]["state"], serde_json::json!("paused"));
    assert_eq!(
        tasks[0]["task_epoch"],
        serde_json::json!(2),
        "epoch 随行持久"
    );

    // 幂等收据:首写落行,冲突写忽略(原收据不被覆盖)
    let receipt = r#"{"operation_id":"op_01JAAAAAAAAAAAAAAAAAAAAAB8","state":"succeeded"}"#;
    db.save_idem_receipt("sha256:1a2b", receipt, "2026-08-29T11:00:03.200Z")
        .expect("写收据");
    db.save_idem_receipt(
        "sha256:1a2b",
        r#"{"tampered":true}"#,
        "2026-08-29T11:00:09.000Z",
    )
    .expect("重复写为 no-op");
    assert_eq!(
        db.idem_receipt("sha256:1a2b").unwrap().as_deref(),
        Some(receipt),
        "原收据不被覆盖"
    );
    assert_eq!(db.list_idem_receipts().unwrap().len(), 1);
    assert_eq!(db.idem_receipt("sha256:absent").unwrap(), None);
}

/// v10→v11(2026-09-08 会话目录服务端化):存量 v10 库打开自动加
/// title/updated_at 两列(NULL),既有行保留;backfill_session_meta 只填空
/// 不覆盖(COALESCE 幂等,重入不改已有值)。
#[test]
fn v10_database_upgrades_to_v11_and_backfill_is_fill_if_null() {
    let dir = tempfile::tempdir().expect("临时目录");
    let path = dir.path().join("state.db");
    {
        let conn = rusqlite::Connection::open(&path).expect("建 v10 库");
        conn.execute_batch(
            r#"
            CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY, state TEXT NOT NULL,
                agent_id TEXT NOT NULL, created_at TEXT NOT NULL,
                workspace_id TEXT);
            INSERT INTO sessions VALUES('sess_01JAAAAAAAAAAAAAAAAAAAAA0B',
                'active', 'agent_01JAAAAAAAAAAAAAAAAAAAAA0C',
                '2026-09-08T10:00:00.000Z', NULL);
            PRAGMA user_version = 10;
            "#,
        )
        .expect("v10 schema");
    }
    let db = StateDb::open(&path).expect("打开 v10 库(自动迁移)");
    let rows = db
        .query_rows(
            "SELECT title, updated_at FROM sessions WHERE id = 'sess_01JAAAAAAAAAAAAAAAAAAAAA0B'",
            &[],
        )
        .expect("读迁移后行");
    assert_eq!(rows.len(), 1, "存量行保留");
    assert!(rows[0]["title"].is_null(), "title 新列为 NULL");
    assert!(rows[0]["updated_at"].is_null(), "updated_at 新列为 NULL");

    // 回填:只填空
    db.backfill_session_meta(
        "sess_01JAAAAAAAAAAAAAAAAAAAAA0B",
        Some("帮我总结这份文档"),
        Some("2026-09-08T10:05:00.000Z"),
    )
    .expect("回填目录");
    // 重入:已有值不被覆盖(None 参数同样保持原值)
    db.backfill_session_meta(
        "sess_01JAAAAAAAAAAAAAAAAAAAAA0B",
        Some("后来的一条消息"),
        None,
    )
    .expect("重入回填为 no-op");
    let rows = db
        .query_rows(
            "SELECT title, updated_at FROM sessions WHERE id = 'sess_01JAAAAAAAAAAAAAAAAAAAAA0B'",
            &[],
        )
        .expect("读回填后行");
    assert_eq!(
        rows[0]["title"],
        serde_json::json!("帮我总结这份文档"),
        "已有标题不被重入覆盖"
    );
    assert_eq!(
        rows[0]["updated_at"],
        serde_json::json!("2026-09-08T10:05:00.000Z"),
        "updated_at 已填平"
    );
    // 不存在的会话 = 空 no-op 不报错
    db.backfill_session_meta("sess_absent", Some("x"), Some("y"))
        .expect("不存在的会话 no-op");
}
