//! SQLite 规范状态库:schema 版本化迁移(PRAGMA user_version,expand-contract)
//! + meta 表(含 CAS 写入门禁底座,ADR-0004 条件 3)。
//!
//! 行级 materialize(事件 → 行变更)自 T2 接入;本文件只负责打开/迁移/meta。

use crate::error::{StoreError, StoreResult};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub const SCHEMA_VERSION: i64 = 7;

/// approvals 表行(载荷列 = Approval 合同 JSON 文本)。
pub struct ApprovalRow<'a> {
    pub id: &'a str,
    pub operation_id: &'a str,
    pub capability: &'a str,
    pub principal: &'a str,
    pub state: &'a str,
    pub payload: &'a str,
    pub created_at: &'a str,
    pub resolved_at: Option<&'a str>,
}

/// grants 表行(载荷列 = Grant 合同 JSON 文本)。
pub struct GrantRow<'a> {
    pub id: &'a str,
    pub audience: &'a str,
    pub action: &'a str,
    pub revocation_version: u64,
    pub revoked: bool,
    /// T6c 收紧(M5-T1):count 类 Grant 消费余量持久化,重启不再回满。
    pub used_count: u64,
    pub payload: &'a str,
    pub created_at: &'a str,
}

/// tasks 表行(载荷列 = task/task.v0.1 合同 JSON 文本)。
pub struct TaskRow<'a> {
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
}

/// capabilities 表行(manifest 列 = Capability Manifest 合同 JSON 文本)。
pub struct CapabilityRow<'a> {
    pub capability: &'a str,
    pub provider_instance_id: &'a str,
    pub epoch: u64,
    pub status: &'a str,
    pub manifest: &'a str,
    pub updated_at: &'a str,
}

pub struct StateDb {
    pub(crate) conn: Mutex<Connection>,
}

impl StateDb {
    /// 打开并迁移到最新 schema。链式 expand-contract 迁移(ADR-0003 对偶:
    /// 只加列不删列,数据一致性不押注任何回滚)。
    pub fn open(path: &Path) -> StoreResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::Corrupt {
                seq: 0,
                reason: format!("未知 schema 版本 {version}(库来自更新的实现?)"),
            });
        }
        if version < 1 {
            Self::migrate_v0_to_v1(&conn)?;
        }
        if version < 2 {
            Self::migrate_v1_to_v2(&conn)?;
        }
        if version < 3 {
            Self::migrate_v2_to_v3(&conn)?;
        }
        if version < 4 {
            Self::migrate_v3_to_v4(&conn)?;
        }
        if version < 5 {
            Self::migrate_v4_to_v5(&conn)?;
        }
        if version < 6 {
            Self::migrate_v5_to_v6(&conn)?;
        }
        if version < 7 {
            Self::migrate_v6_to_v7(&conn)?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// v2→v3(M4-T3,expand:纯新增四表,不动既有行):
    /// approvals(审批持久对象)/ grants(Broker 授权台账)/
    /// capabilities(Provider binding 逻辑目录,epoch 持久计数)/
    /// outbox(副作用对账底座,T6 启用)。
    /// 载荷列存合同形态 JSON(载荷合同 = capability/*.schema.json),
    /// 行级键列供索引与审计查询。
    fn migrate_v2_to_v3(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            CREATE TABLE approvals (
                id           TEXT PRIMARY KEY,
                operation_id TEXT NOT NULL,
                capability   TEXT NOT NULL,
                principal    TEXT NOT NULL,
                state        TEXT NOT NULL,
                payload      TEXT NOT NULL,
                created_at   TEXT NOT NULL,
                resolved_at  TEXT
            );
            CREATE INDEX idx_approvals_state ON approvals(state);
            CREATE TABLE grants (
                id                 TEXT PRIMARY KEY,
                audience           TEXT NOT NULL,
                action             TEXT NOT NULL,
                revocation_version INTEGER NOT NULL DEFAULT 0,
                revoked            INTEGER NOT NULL DEFAULT 0,
                payload            TEXT NOT NULL,
                created_at         TEXT NOT NULL
            );
            CREATE INDEX idx_grants_audience_action ON grants(audience, action);
            CREATE TABLE capabilities (
                capability           TEXT PRIMARY KEY,
                provider_instance_id TEXT NOT NULL,
                epoch                INTEGER NOT NULL,
                status               TEXT NOT NULL,
                manifest             TEXT NOT NULL,
                updated_at           TEXT NOT NULL
            );
            CREATE TABLE outbox (
                operation_id TEXT NOT NULL,
                kind         TEXT NOT NULL,
                state        TEXT NOT NULL,
                payload      TEXT NOT NULL,
                created_at   TEXT NOT NULL,
                updated_at   TEXT NOT NULL,
                PRIMARY KEY (operation_id, kind)
            );
            COMMIT;
            "#,
        )?;
        Ok(())
    }

    /// v3→v4(M5-T1,expand:纯新增 + 加列,不动既有行):
    /// tasks(Task 规范状态,L2 唯一持有,ADR-0004)/ task_members(成员事实,
    /// 纯事件物化)/ task_budget_ledger(两级账本,T6 启用)/ observations
    /// (Observation Log,T8 启用)/ memories(memory.* 底座,T8 启用)/
    /// T6c 收紧两项:grants.used_count 列(count 消费余量持久化,重启不回满)
    /// 与 idempotency_receipts 表(幂等收据落表,恢复期抑制判定不再依赖内存)。
    fn migrate_v3_to_v4(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            CREATE TABLE tasks (
                id         TEXT PRIMARY KEY,
                title      TEXT NOT NULL,
                state      TEXT NOT NULL,
                created_by TEXT NOT NULL,
                task_epoch INTEGER NOT NULL,
                payload    TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX idx_tasks_state ON tasks(state);
            CREATE TABLE task_members (
                task_id    TEXT NOT NULL,
                agent_id   TEXT NOT NULL,
                role       TEXT NOT NULL,
                grant_id   TEXT,
                joined_seq INTEGER NOT NULL,
                PRIMARY KEY (task_id, agent_id)
            );
            CREATE TABLE task_budget_ledger (
                task_id     TEXT NOT NULL,
                agent_id    TEXT NOT NULL,
                used_tokens INTEGER NOT NULL DEFAULT 0,
                updated_at  TEXT NOT NULL,
                PRIMARY KEY (task_id, agent_id)
            );
            CREATE TABLE observations (
                log_seq     INTEGER PRIMARY KEY,
                task_id     TEXT NOT NULL,
                verdict     TEXT NOT NULL,
                guard_state TEXT NOT NULL,
                payload     TEXT NOT NULL,
                observed_at TEXT NOT NULL
            );
            CREATE TABLE memories (
                id         TEXT PRIMARY KEY,
                scope      TEXT NOT NULL,
                tombstoned INTEGER NOT NULL DEFAULT 0,
                payload    TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX idx_memories_scope ON memories(scope);
            ALTER TABLE grants ADD COLUMN used_count INTEGER NOT NULL DEFAULT 0;
            CREATE TABLE idempotency_receipts (
                key_hash   TEXT PRIMARY KEY,
                payload    TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            COMMIT;
            "#,
        )?;
        Ok(())
    }

    /// v4→v5(M5-T6,expand 加列):task_budget_ledger 增 used_tool_calls
    /// (Task 包络的工具调用维度记账;token 维度留给模型调用聚合)。
    fn migrate_v4_to_v5(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            ALTER TABLE task_budget_ledger ADD COLUMN used_tool_calls INTEGER NOT NULL DEFAULT 0;
            COMMIT;
            "#,
        )?;
        Ok(())
    }

    /// v5→v6(M5-T8,expand 加列):memories 检索面列 + FTS5 全文索引
    /// (FTS5 编译特性缺失时静默跳过索引,检索走 LIKE 兜底——接口可替换)。
    fn migrate_v5_to_v6(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            ALTER TABLE memories ADD COLUMN content_preview TEXT;
            ALTER TABLE memories ADD COLUMN source_ref TEXT;
            ALTER TABLE memories ADD COLUMN correction_of TEXT;
            COMMIT;
            "#,
        )?;
        // FTS5 索引失败不阻塞迁移(LIKE 兜底)
        let _ = conn
            .execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(content);");
        Ok(())
    }

    /// v6→v7(M6-T1,expand 加列):tasks.parent_task_id/delegation_depth
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

    /// v1→v2(M2.6):operations 增 input_content 列(受保护存储)——
    /// 输入原文只存规范状态库、不进事件/日志(A4),供 claim 幂等续跑。
    fn migrate_v1_to_v2(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            ALTER TABLE operations ADD COLUMN input_content TEXT;
            COMMIT;
            "#,
        )?;
        Ok(())
    }

    fn migrate_v0_to_v1(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            CREATE TABLE meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE sessions (
                id         TEXT PRIMARY KEY,
                state      TEXT NOT NULL,
                agent_id   TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE agents (
                id                  TEXT PRIMARY KEY,
                session_id          TEXT NOT NULL,
                name                TEXT NOT NULL,
                model_chain         TEXT NOT NULL,
                state               TEXT NOT NULL,
                budget_max_tokens   INTEGER,
                budget_max_turns    INTEGER,
                budget_used_tokens  INTEGER NOT NULL DEFAULT 0,
                budget_turns_used   INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE operations (
                id             TEXT PRIMARY KEY,
                session_id     TEXT NOT NULL,
                agent_id       TEXT NOT NULL,
                request_id     TEXT,
                state          TEXT NOT NULL,
                turn_index     INTEGER NOT NULL,
                created_at     TEXT NOT NULL,
                completed_at   TEXT,
                action_summary TEXT,
                result_ref     TEXT,
                error_code     TEXT,
                error_message  TEXT
            );
            CREATE TABLE tombstones (
                kind TEXT NOT NULL,
                id   TEXT NOT NULL,
                at   TEXT NOT NULL,
                PRIMARY KEY (kind, id)
            );
            COMMIT;
            "#,
        )?;
        Ok(())
    }

    /// 保存回合输入原文(受保护存储;A4:原文不进事件/日志)。
    pub fn save_op_input(&self, operation_id: &str, content: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "UPDATE operations SET input_content = ?2 WHERE id = ?1",
            rusqlite::params![operation_id, content],
        )?;
        Ok(())
    }

    /// 读回合输入原文(claim 续跑用)。
    pub fn op_input(&self, operation_id: &str) -> StoreResult<Option<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn.prepare("SELECT input_content FROM operations WHERE id = ?1")?;
        let mut rows = stmt.query([operation_id])?;
        if let Some(row) = rows.next()? {
            Ok(row.get(0)?)
        } else {
            Ok(None)
        }
    }

    /// 通用行查询(恢复与测试读取用;返回按列名的 JSON 对象数组)。
    pub fn query_rows(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> StoreResult<Vec<serde_json::Value>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn.prepare(sql)?;
        let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
        let mut rows = stmt.query(params)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut obj = serde_json::Map::new();
            for (i, name) in names.iter().enumerate() {
                let v: serde_json::Value = match row.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => serde_json::Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => n.into(),
                    rusqlite::types::ValueRef::Real(f) => f.into(),
                    rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into(),
                    rusqlite::types::ValueRef::Blob(_) => serde_json::Value::Null,
                };
                obj.insert(name.clone(), v);
            }
            out.push(serde_json::Value::Object(obj));
        }
        Ok(out)
    }

    /// meta 读。
    pub fn meta_get(&self, key: &str) -> StoreResult<Option<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// meta 无条件写。
    pub fn meta_set(&self, key: &str, value: &str) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// meta CAS:仅当现值等于 expect 时写入。返回是否成功;
    /// 不匹配返回 CasMismatch(调用方据此产生 store.write.rejected 审计事件)。
    pub fn meta_compare_and_set(
        &self,
        key: &str,
        expect: Option<&str>,
        new: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        let current: Option<String> = {
            let mut stmt = conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
            let mut rows = stmt.query([key])?;
            rows.next()?.map(|r| r.get(0)).transpose()?
        };
        if current.as_deref() != expect {
            return Err(StoreError::CasMismatch {
                key: key.to_string(),
                expect: expect.unwrap_or("<absent>").to_string(),
            });
        }
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, new],
        )?;
        Ok(())
    }

    // ---- v3:approvals / grants / capabilities / outbox(M4)------------------
    // 载荷列 = 合同形态 JSON 文本(capability/*.schema.json);行级键列供索引。

    /// 写入/更新审批对象(upsert)。
    pub fn save_approval(&self, row: ApprovalRow<'_>) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO approvals(id, operation_id, capability, principal, state, payload,
                                   created_at, resolved_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state,
                 payload = excluded.payload, resolved_at = excluded.resolved_at",
            rusqlite::params![
                row.id,
                row.operation_id,
                row.capability,
                row.principal,
                row.state,
                row.payload,
                row.created_at,
                row.resolved_at
            ],
        )?;
        Ok(())
    }

    pub fn approval_payload(&self, id: &str) -> StoreResult<Option<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn.prepare("SELECT payload FROM approvals WHERE id = ?1")?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_approvals_by_state(&self, state: &str) -> StoreResult<Vec<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt =
            conn.prepare("SELECT payload FROM approvals WHERE state = ?1 ORDER BY created_at")?;
        let mut rows = stmt.query([state])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row.get(0)?);
        }
        Ok(out)
    }

    /// 恢复面:全部审批行(id, operation_id, state, payload)。
    pub fn list_approvals(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT id, operation_id, state, payload FROM approvals ORDER BY created_at",
            &[],
        )
    }

    /// 写入/更新 Grant(revoked 标志/版本/消费计数随推进;T6c 起消费余量持久)。
    pub fn save_grant(&self, row: GrantRow<'_>) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO grants(id, audience, action, revocation_version, revoked, used_count,
                                payload, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET revocation_version = excluded.revocation_version,
                 revoked = excluded.revoked, used_count = excluded.used_count,
                 payload = excluded.payload",
            rusqlite::params![
                row.id,
                row.audience,
                row.action,
                row.revocation_version as i64,
                row.revoked as i64,
                row.used_count as i64,
                row.payload,
                row.created_at
            ],
        )?;
        Ok(())
    }

    /// 恢复面:全部 Grant 行(id, audience, action, revocation_version, revoked,
    /// used_count, payload)。
    pub fn list_grants(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT id, audience, action, revocation_version, revoked, used_count, payload
             FROM grants ORDER BY created_at",
            &[],
        )
    }

    /// 写入/更新 capability binding(epoch 单调由调用方保证,恢复时取 max)。
    pub fn save_capability_binding(&self, row: CapabilityRow<'_>) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO capabilities(capability, provider_instance_id, epoch, status,
                                      manifest, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(capability) DO UPDATE SET
                 provider_instance_id = excluded.provider_instance_id,
                 epoch = excluded.epoch, status = excluded.status,
                 manifest = excluded.manifest, updated_at = excluded.updated_at",
            rusqlite::params![
                row.capability,
                row.provider_instance_id,
                row.epoch as i64,
                row.status,
                row.manifest,
                row.updated_at
            ],
        )?;
        Ok(())
    }

    /// 恢复面:全部 binding 行。
    pub fn list_capability_bindings(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT capability, provider_instance_id, epoch, status, manifest
             FROM capabilities ORDER BY capability",
            &[],
        )
    }

    /// outbox 记录 upsert(T6 副作用对账底座;状态 pending→published→verified)。
    pub fn outbox_upsert(
        &self,
        operation_id: &str,
        kind: &str,
        state: &str,
        payload: &str,
        now: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO outbox(operation_id, kind, state, payload, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(operation_id, kind) DO UPDATE SET state = excluded.state,
                 payload = excluded.payload, updated_at = excluded.updated_at",
            rusqlite::params![operation_id, kind, state, payload, now],
        )?;
        Ok(())
    }

    /// 恢复面:指定状态的 outbox 行。
    pub fn list_outbox_by_state(&self, state: &str) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT operation_id, kind, state, payload FROM outbox WHERE state = ?1",
            rusqlite::params![state],
        )
    }

    // ---- v4:tasks / task_members / idempotency_receipts(M5-T1)--------------
    // 载荷列 = task/task.v0.1 合同形态 JSON;行级键列供索引与恢复。

    /// 写入/更新 Task(upsert;task_epoch 单调由调用方保证,恢复时取 max)。
    pub fn save_task(&self, row: TaskRow<'_>) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO tasks(id, title, state, created_by, task_epoch, payload,
                               created_at, updated_at, parent_task_id, delegation_depth)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title,
                 state = excluded.state, task_epoch = excluded.task_epoch,
                 payload = excluded.payload, updated_at = excluded.updated_at,
                 parent_task_id = excluded.parent_task_id,
                 delegation_depth = excluded.delegation_depth",
            rusqlite::params![
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
            ],
        )?;
        Ok(())
    }

    /// 恢复面:全部 Task 行。
    pub fn list_tasks(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT id, title, state, created_by, task_epoch, payload, created_at,
                    updated_at, parent_task_id, delegation_depth
             FROM tasks ORDER BY created_at",
            &[],
        )
    }

    /// Task 预算账本行 upsert(agent_id = "" 为 Task 级聚合行)。
    pub fn save_task_budget(
        &self,
        task_id: &str,
        agent_id: &str,
        used_tool_calls: u64,
        used_tokens: u64,
        now: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO task_budget_ledger(task_id, agent_id, used_tokens, used_tool_calls,
                                          updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(task_id, agent_id) DO UPDATE SET
                 used_tokens = excluded.used_tokens,
                 used_tool_calls = excluded.used_tool_calls,
                 updated_at = excluded.updated_at",
            rusqlite::params![
                task_id,
                agent_id,
                used_tokens as i64,
                used_tool_calls as i64,
                now
            ],
        )?;
        Ok(())
    }

    /// 恢复面:全部预算账本行。
    pub fn list_task_budget(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT task_id, agent_id, used_tokens, used_tool_calls, updated_at
             FROM task_budget_ledger",
            &[],
        )
    }

    /// Observation Log 条目落表(log_seq 自 MAX+1 单调分配),返回 seq。
    pub fn save_observation(
        &self,
        task_id: &str,
        verdict: &str,
        guard_state: &str,
        payload: &str,
        observed_at: &str,
    ) -> StoreResult<u64> {
        let conn = self.conn.lock().expect("锁未中毒");
        let next: i64 = conn.query_row(
            "SELECT COALESCE(MAX(log_seq), 0) + 1 FROM observations",
            [],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO observations(log_seq, task_id, verdict, guard_state, payload,
                                      observed_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![next, task_id, verdict, guard_state, payload, observed_at],
        )?;
        Ok(next as u64)
    }

    /// 记忆写入(墓碑语义见 delete),返回 entry_id(id 由调用方给定)。
    /// #[allow]:参数与 memory-entry 合同字段一一对应(压缩反损可读性)。
    #[allow(clippy::too_many_arguments)]
    pub fn memory_put(
        &self,
        entry_id: &str,
        scope: &str,
        _content_ref: &str,
        content_preview: Option<&str>,
        _source_trust: &str,
        source_ref: Option<&str>,
        correction_of: Option<&str>,
        payload: &str,
        created_at: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO memories(id, scope, tombstoned, content_preview, source_ref,
                                  correction_of, payload, created_at)
             VALUES(?1, ?2, 0, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            rusqlite::params![
                entry_id,
                scope,
                content_preview,
                source_ref,
                correction_of,
                payload,
                created_at
            ],
        )?;
        // 用户纠正:被纠正条目立即墓碑化(覆盖而非追加,基线 §4.1)
        if let Some(target) = correction_of {
            let _ = conn.execute(
                "UPDATE memories SET tombstoned = 1 WHERE id = ?1",
                rusqlite::params![target],
            );
        }
        // FTS5 索引(失败静默:LIKE 兜底)
        if let Some(preview) = content_preview {
            let _ = conn.execute(
                "INSERT INTO memories_fts(rowid, content)
                 VALUES((SELECT rowid FROM memories WHERE id = ?1), ?2)",
                rusqlite::params![entry_id, preview],
            );
        }
        Ok(())
    }

    /// 记忆检索:scope 内非墓碑条目(FTS5 MATCH 优先,LIKE 兜底)。
    pub fn memory_search(&self, scope: &str, query: &str) -> StoreResult<Vec<serde_json::Value>> {
        let rows = self.query_rows(
            "SELECT m.id, m.scope, m.content_preview, m.source_ref, m.payload
             FROM memories m JOIN memories_fts f ON m.rowid = f.rowid
             WHERE m.scope = ?1 AND m.tombstoned = 0 AND memories_fts MATCH ?2",
            rusqlite::params![scope, format!("\"{query}\"")],
        );
        match rows {
            Ok(r) => Ok(r),
            Err(_) => self.query_rows(
                "SELECT id, scope, content_preview, source_ref, payload FROM memories
                 WHERE scope = ?1 AND tombstoned = 0
                   AND content_preview LIKE ('%' || ?2 || '%')",
                rusqlite::params![scope, query],
            ),
        }
    }

    /// 记忆删除:墓碑 + 来源级联失效。返回级联数。
    pub fn memory_delete(&self, entry_id: &str) -> StoreResult<usize> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "UPDATE memories SET tombstoned = 1 WHERE id = ?1",
            [entry_id],
        )?;
        let cascaded = conn.execute(
            "UPDATE memories SET tombstoned = 1
             WHERE source_ref = ?1 AND tombstoned = 0",
            [entry_id],
        )?;
        Ok(cascaded)
    }

    /// 幂等收据落表(T6c):key_hash → 原收据;恢复后抑制判定不依赖内存。
    pub fn save_idem_receipt(
        &self,
        key_hash: &str,
        payload: &str,
        created_at: &str,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute(
            "INSERT INTO idempotency_receipts(key_hash, payload, created_at)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(key_hash) DO NOTHING",
            rusqlite::params![key_hash, payload, created_at],
        )?;
        Ok(())
    }

    /// 读幂等收据(T6c:恢复期判定「外部是否已执行」)。
    pub fn idem_receipt(&self, key_hash: &str) -> StoreResult<Option<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt =
            conn.prepare("SELECT payload FROM idempotency_receipts WHERE key_hash = ?1")?;
        let mut rows = stmt.query([key_hash])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// 恢复面:全部幂等收据行。
    pub fn list_idem_receipts(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.query_rows(
            "SELECT key_hash, payload, created_at FROM idempotency_receipts ORDER BY created_at",
            &[],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn meta_set_and_cas() {
        let dir = tempfile::tempdir().expect("临时目录");
        let db = StateDb::open(&dir.path().join("state.db")).expect("打开");

        db.meta_set("last_applied_seq", "5").expect("写");
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
        assert_eq!(version, 7);

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
}
