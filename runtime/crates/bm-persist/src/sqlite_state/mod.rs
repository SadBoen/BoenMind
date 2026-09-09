//! SQLite 规范状态库:schema 版本化迁移(PRAGMA user_version,expand-contract)
//! + meta 表(含 CAS 写入门禁底座,ADR-0004 条件 3)。
//!
//! 行级 materialize(事件 → 行变更)自 T2 接入;本文件只负责打开/迁移/meta。

mod approvals;
mod backup;
mod capabilities;
mod evaluation;
mod grants;
mod idem;
mod memory;
mod ops;
mod outbox;
mod rows;
mod tasks;
#[cfg(test)]
mod tests;

pub use rows::{ApprovalRow, CapabilityRow, GrantRow, TaskRow};

use crate::error::{SqlResultExt, StoreError, StoreResult};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub const SCHEMA_VERSION: i64 = 12;

pub struct StateDb {
    pub(crate) conn: Mutex<Connection>,
}

impl StateDb {
    /// 打开并迁移到最新 schema。链式 expand-contract 迁移(ADR-0003 对偶:
    /// 只加列不删列,数据一致性不押注任何回滚)。
    pub fn open(path: &Path) -> StoreResult<Self> {
        let conn = Connection::open(path).sql()?;
        conn.pragma_update(None, "journal_mode", "WAL").sql()?;
        conn.pragma_update(None, "synchronous", "FULL").sql()?;
        // WAL checkpoint 策略定标 (M2-review §6-3 / M3-review §6-5 承兑):
        // 设置 wal_autocheckpoint 阈值为 1000 页 (约 4MB)，达到时自动触发 PASSIVE 检查点回写主库，
        // 杜绝 WAL 日志无限增长并兼顾写吞吐与崩溃恢复窗口。
        conn.pragma_update(None, "wal_autocheckpoint", 1000).sql()?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .sql()?;
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
        if version < 8 {
            Self::migrate_v7_to_v8(&conn)?;
        }
        if version < 9 {
            Self::migrate_v8_to_v9(&conn)?;
        }
        if version < 10 {
            Self::migrate_v9_to_v10(&conn)?;
        }
        if version < 11 {
            Self::migrate_v10_to_v11(&conn)?;
        }
        if version < 12 {
            Self::migrate_v11_to_v12(&conn)?;
        }
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .sql()?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// v11→v12(ADR-0030,expand 加列):sessions.permission_mode——会话
    /// 权限模式服务端化(前端 localStorage 权威废弃);存量行默认 'ask'
    /// (ADR-0030 决策 1:新会话默认审批)。变更经 session.mode.changed
    /// 事件物化(重放确定),本列随恢复装载进内存会话状态。
    fn migrate_v11_to_v12(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            ALTER TABLE sessions ADD COLUMN permission_mode TEXT NOT NULL DEFAULT 'ask';
            COMMIT;
            "#,
        )
        .sql()?;
        Ok(())
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
        )
        .sql()?;
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
        )
        .sql()?;
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
        )
        .sql()?;
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
        )
        .sql()?;
        // FTS5 索引失败不阻塞迁移(LIKE 兜底),但降级必须可观测
        if let Err(e) = conn
            .execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(content);")
        {
            tracing::warn!(error = %e, "FTS5 虚表创建失败,memory 检索退化为 LIKE 兜底");
        }
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
        )
        .sql()?;
        Ok(())
    }

    /// v10→v11(2026-09-08 会话目录服务端化,expand 加列):sessions.title/updated_at
    /// ——会话列表此前只存浏览器 localStorage(每设备各记各账,三端不一致的
    /// 根因);目录收归服务端单一权威(基线「访问端无状态」回归)。
    /// title = 首条用户消息截断(内容不在事件面,core 直写保护,同
    /// input_content 先例);updated_at = 最近回合边界(事件物化派生,重放确定)。
    fn migrate_v10_to_v11(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            ALTER TABLE sessions ADD COLUMN title TEXT;
            ALTER TABLE sessions ADD COLUMN updated_at TEXT;
            COMMIT;
            "#,
        )
        .sql()?;
        Ok(())
    }

    /// v9→v10(2026-09-06 重启续聊配套,expand 加列):sessions.workspace_id
    /// ——会话绑定工作目录跨重启持久(ADR-0018 决策 3 的修订:重启续聊
    /// 落地后绑定丢失已有用户可见害处——模型接着聊而文件根静默回落)。
    fn migrate_v9_to_v10(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            BEGIN;
            ALTER TABLE sessions ADD COLUMN workspace_id TEXT;
            COMMIT;
            "#,
        )
        .sql()?;
        Ok(())
    }

    /// v8→v9(2026-09-05 回看修复,expand:纯新增一表):op_cancel_marks——
    /// 取消意图持久标记。用户显式取消后、回合边界落定前若崩溃,恢复端凭此
    /// 走 Resuming→Stopped(turn_was_stopping)契约边,不再把已取消的回合
    /// 复活为 Running 重烧模型调用。
    fn migrate_v8_to_v9(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS op_cancel_marks (
                op_id     TEXT PRIMARY KEY,
                marked_at TEXT NOT NULL
            );
            "#,
        )
        .sql()?;
        Ok(())
    }

    /// v7→v8(M8.5/M8.7,expand:纯新增一表):evaluation_reports——
    /// 独立 Judge 的评估报告落库(报告 = 派生工件,不进事件日志)。
    fn migrate_v7_to_v8(conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS evaluation_reports (
                report_id  TEXT PRIMARY KEY,
                from_seq   INTEGER NOT NULL,
                to_seq     INTEGER NOT NULL,
                payload    TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            "#,
        )
        .sql()?;
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
        )
        .sql()?;
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
        )
        .sql()?;
        Ok(())
    }
}

impl StateDb {
    /// meta 读。
    pub fn meta_get(&self, key: &str) -> StoreResult<Option<String>> {
        let conn = self.conn.lock().expect("锁未中毒");
        let mut stmt = conn
            .prepare("SELECT value FROM meta WHERE key = ?1")
            .sql()?;
        let mut rows = stmt.query([key]).sql()?;
        if let Some(row) = rows.next().sql()? {
            Ok(Some(row.get(0).sql()?))
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
        )
        .sql()?;
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
            let mut stmt = conn
                .prepare("SELECT value FROM meta WHERE key = ?1")
                .sql()?;
            let mut rows = stmt.query([key]).sql()?;
            rows.next().sql()?.map(|r| r.get(0)).transpose().sql()?
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
        )
        .sql()?;
        Ok(())
    }
}
