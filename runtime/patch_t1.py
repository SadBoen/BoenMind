from pathlib import Path

# 1. bm-contract:事件 +1、payload 键集
p = Path("crates/bm-contract/src/events.rs")
s = p.read_text(encoding="utf-8")
old = '''    WatchdogReorchestrationTriggered => "watchdog.reorchestration.triggered",
    ObservationRecorded => "observation.recorded",
});'''
new = '''    WatchdogReorchestrationTriggered => "watchdog.reorchestration.triggered",
    ObservationRecorded => "observation.recorded",
    // M6 增发(2026-08-30,Minor:纯追加,M6 规格 §4-2)
    TaskMemberRemoved => "task.member.removed",
});'''
assert s.count(old) == 1
s = s.replace(old, new)
old = '''//! 运行时事件注册表镜像(registry/runtime-events.v0_1.json,40 类,封闭集合:
//! M1 20 + M2 增发 2 + M4 增发 10 + M5 增发 8)。'''
new = '''//! 运行时事件注册表镜像(registry/runtime-events.v0_1.json,41 类,封闭集合:
//! M1 20 + M2 增发 2 + M4 增发 10 + M5 增发 8 + M6 增发 1)。'''
assert s.count(old) == 1
s = s.replace(old, new)
old = '''            EventType::TaskCreated => &["task_id", "title", "created_by"],'''
new = '''            EventType::TaskCreated => &["task_id", "title", "created_by", "parent_task_id"],'''
assert s.count(old) == 1
s = s.replace(old, new)
old = '''            EventType::WatchdogReorchestrationTriggered => &["task_id", "trigger", "reason"],'''
new = '''            EventType::WatchdogReorchestrationTriggered => &["task_id", "trigger", "reason"],
            EventType::TaskMemberRemoved => &["task_id", "agent_id", "reason"],'''
assert s.count(old) == 1
s = s.replace(old, new)
p.write_text(s, encoding="utf-8")

# 2. sync.rs
p = Path("crates/bm-contract/tests/sync.rs")
s = p.read_text(encoding="utf-8")
old = '''    assert_eq!(
        registry.len(),
        40,
        "注册表事件数漂移(M1 20 + M2 增发 2 + M4 增发 10 + M5 增发 8)"
    );'''
new = '''    assert_eq!(
        registry.len(),
        41,
        "注册表事件数漂移(M1 20 + M2 增发 2 + M4 增发 10 + M5 增发 8 + M6 增发 1)"
    );'''
assert s.count(old) == 1
s = s.replace(old, new)
old = '''    // parent_task_id 恒 null(M6 预留,M5 规格 §4-1)
    let mut bad = task;
    bad["parent_task_id"] = json!("task_01JAAAAAAAAAAAAAAAAAAAAAB1");
    assert!(
        validate(registries::TASK_SCHEMA, &bad).is_err(),
        "parent_task_id M5 恒 null"
    );'''
new = '''    // M6 启用:parent_task_id 放宽为 task 引用;delegation_depth 必填且 ≤3
    let mut with_parent = task.clone();
    with_parent["parent_task_id"] = json!("task_01JAAAAAAAAAAAAAAAAAAAAAB1");
    with_parent["delegation_depth"] = json!(1);
    assert!(
        validate(registries::TASK_SCHEMA, &with_parent).is_ok(),
        "M6:parent_task_id + delegation_depth 合法"
    );
    let mut bad = task;
    bad["delegation_depth"] = json!(4);
    assert!(
        validate(registries::TASK_SCHEMA, &bad).is_err(),
        "委派深度上限 3(M6.5)"
    );'''
assert s.count(old) == 1
s = s.replace(old, new)
p.write_text(s, encoding="utf-8")

# 3. bm-persist v7
p = Path("crates/bm-persist/src/sqlite_state.rs")
s = p.read_text(encoding="utf-8")
s = s.replace("pub const SCHEMA_VERSION: i64 = 6;", "pub const SCHEMA_VERSION: i64 = 7;")
old = '''        if version < 6 {
            Self::migrate_v5_to_v6(&conn)?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;'''
new = '''        if version < 6 {
            Self::migrate_v5_to_v6(&conn)?;
        }
        if version < 7 {
            Self::migrate_v6_to_v7(&conn)?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;'''
assert s.count(old) == 1
s = s.replace(old, new)
old = '''    /// v1→v2(M2.6):operations 增 input_content 列(受保护存储)——'''
new = '''    /// v6→v7(M6-T1,expand 加列):tasks.parent_task_id/delegation_depth
    /// (委派链;事件物化与直接落行双路写)。
    fn migrate_v6_to_v7(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            ALTER TABLE tasks ADD COLUMN parent_task_id TEXT;
            ALTER TABLE tasks ADD COLUMN delegation_depth INTEGER NOT NULL DEFAULT 0;
            COMMIT;
            "#,
        )?;
        Ok(())
    }

    /// v1→v2(M2.6):operations 增 input_content 列(受保护存储)——'''
assert s.count(old) == 1
s = s.replace(old, new)
old = '''            "INSERT INTO tasks(id, title, state, created_by, task_epoch, payload,
                               created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title,
                 state = excluded.state, task_epoch = excluded.task_epoch,
                 payload = excluded.payload, updated_at = excluded.updated_at",'''
new = '''            "INSERT INTO tasks(id, title, state, created_by, task_epoch, payload,
                               created_at, updated_at, parent_task_id, delegation_depth)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title,
                 state = excluded.state, task_epoch = excluded.task_epoch,
                 payload = excluded.payload, updated_at = excluded.updated_at,
                 parent_task_id = excluded.parent_task_id,
                 delegation_depth = excluded.delegation_depth",'''
assert s.count(old) == 1
s = s.replace(old, new)
old = '''            rusqlite::params![
                row.id,
                row.title,
                row.state,
                row.created_by,
                row.task_epoch as i64,
                row.payload,
                row.created_at,
                row.updated_at
            ],'''
new = '''            rusqlite::params![
                row.id,
                row.title,
                row.state,
                row.created_by,
                row.task_epoch as i64,
                row.payload,
                row.created_at,
                row.updated_at,
                row.parent_task_id,
                row.delegation_depth as i64,
            ],'''
assert s.count(old) == 1
s = s.replace(old, new)
old = '''pub struct TaskRow<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub created_by: &'a str,
    pub task_epoch: u64,
    pub payload: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
}'''
new = '''pub struct TaskRow<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub created_by: &'a str,
    pub task_epoch: u64,
    pub payload: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub parent_task_id: Option<&'a str>,
    pub delegation_depth: u64,
}'''
assert s.count(old) == 1
s = s.replace(old, new)
old = '''            "SELECT id, title, state, created_by, task_epoch, payload, created_at, updated_at
             FROM tasks ORDER BY created_at",'''
new = '''            "SELECT id, title, state, created_by, task_epoch, payload, created_at,
                    updated_at, parent_task_id, delegation_depth
             FROM tasks ORDER BY created_at",'''
assert s.count(old) == 1
s = s.replace(old, new)
p.write_text(s, encoding="utf-8")

# 4. recovery.rs
p = Path("crates/bm-persist/src/recovery.rs")
s = p.read_text(encoding="utf-8")
old = '''pub struct TaskStateRow {
    pub id: String,
    pub title: String,
    pub state: String,
    pub created_by: String,
    pub task_epoch: i64,
    pub payload: String,
    pub created_at: String,
    pub updated_at: String,
}'''
new = '''pub struct TaskStateRow {
    pub id: String,
    pub title: String,
    pub state: String,
    pub created_by: String,
    pub task_epoch: i64,
    pub payload: String,
    pub created_at: String,
    pub updated_at: String,
    pub parent_task_id: Option<String>,
    pub delegation_depth: i64,
}'''
assert s.count(old) == 1
s = s.replace(old, new)
old = '''            "SELECT id, title, state, created_by, task_epoch, payload, created_at, updated_at
             FROM tasks",'''
new = '''            "SELECT id, title, state, created_by, task_epoch, payload, created_at,
                    updated_at, parent_task_id, delegation_depth
             FROM tasks",'''
assert s.count(old) == 1
s = s.replace(old, new)
p.write_text(s, encoding="utf-8")

# 5. materialize:TaskCreated 写 parent
p = Path("crates/bm-persist/src/materialize.rs")
s = p.read_text(encoding="utf-8")
old = '''                EventType::TaskCreated => {
                    // INSERT OR IGNORE:完整载荷行已由核心先落(直接落行先于
                    // 事件物化),此处仅兜底事件重建路径(重建载荷为键字段形态)。
                    conn.execute(
                        "INSERT OR IGNORE INTO tasks(id, title, state, created_by, task_epoch,
                                                    payload, created_at, updated_at)
                         VALUES(?1, ?2, 'created', ?3, 1, ?4, ?5, ?5)",
                        rusqlite::params![
                            str_field(p, "task_id")?,
                            str_field(p, "title")?,
                            str_field(p, "created_by")?,
                            format!(r#"{{"task_id":"{}"}}"#, str_field(p, "task_id")?),
                            ts,
                        ],
                    )?;
                    Ok(1)
                }'''
new = '''                EventType::TaskCreated => {
                    // INSERT OR IGNORE:完整载荷行已由核心先落(直接落行先于
                    // 事件物化),此处仅兜底事件重建路径(重建载荷为键字段形态)。
                    let parent = opt_str_field(p, "parent_task_id")?;
                    conn.execute(
                        "INSERT OR IGNORE INTO tasks(id, title, state, created_by, task_epoch,
                                                    payload, created_at, updated_at,
                                                    parent_task_id, delegation_depth)
                         VALUES(?1, ?2, 'created', ?3, 1, ?4, ?5, ?5, ?6, ?7)",
                        rusqlite::params![
                            str_field(p, "task_id")?,
                            str_field(p, "title")?,
                            str_field(p, "created_by")?,
                            format!(r#"{{"task_id":"{}"}}"#, str_field(p, "task_id")?),
                            ts,
                            parent,
                            match &parent {
                                Some(_) => 1i64,
                                None => 0i64,
                            },
                        ],
                    )?;
                    Ok(1)
                }'''
assert s.count(old) == 1
s = s.replace(old, new)
p.write_text(s, encoding="utf-8")
print("T1 done")
