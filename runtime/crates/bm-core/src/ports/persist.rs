//! 持久化端口层(F-12 依赖倒置):EventStore 端口与行 DTO 的**所有权**归内核 bm-core;bm-persist 作为实现方反向依赖内核,并 re-export 保持旧路径。
use bm_contract::events::EventEnvelope;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("事件日志 IO 失败: {0}")]
    Io(#[from] std::io::Error),
    /// SQLite 后端错误(F-12 依赖倒置:内核端口层不再依赖 rusqlite,
    /// 实现方 bm-persist 把 rusqlite::Error 转成携带消息的本变体)。
    #[error("SQLite 失败: {0}")]
    Sql(String),
    #[error("事件日志损坏于 seq {seq}: {reason}")]
    Corrupt { seq: u64, reason: String },
    #[error("CAS 不匹配: key={key} expect={expect}")]
    CasMismatch { key: String, expect: String },
    #[error("目录未初始化: {0}")]
    NotOpen(String),
}

pub type StoreResult<T> = Result<T, StoreError>;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryReport {
    /// 恢复完成后的状态位点。
    pub last_applied_seq: u64,
    /// 修复窗口内重放(补物化)的事件数。
    pub replayed: usize,
    /// 被标记 interrupted 的未终态 operation 数。
    pub interrupted_recovered: usize,
}

/// 规范状态行(装配内存视图的载体)。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionRow {
    pub id: String,
    pub state: String,
    pub agent_id: String,
    pub created_at: String,
    /// 重启续聊配套(2026-09-06):会话绑定工作目录(未绑定 = None)。
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentRow {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub model_chain: String,
    pub state: String,
    pub budget_max_tokens: Option<i64>,
    pub budget_max_turns: Option<i64>,
    pub budget_used_tokens: i64,
    pub budget_turns_used: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OperationRow {
    pub id: String,
    pub session_id: String,
    pub agent_id: String,
    pub request_id: Option<String>,
    pub state: String,
    pub turn_index: i64,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub action_summary: Option<String>,
    pub result_reference: Option<String>,
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub input_content: Option<String>,
}

/// Task 规范状态行(M5-T1;payload = task/task.v0.1 合同 JSON)。
#[derive(Debug, Clone, Deserialize)]
pub struct TaskStateRow {
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
}

#[derive(Debug, Clone, Default)]
pub struct WorldRows {
    pub sessions: Vec<SessionRow>,
    pub agents: Vec<AgentRow>,
    pub operations: Vec<OperationRow>,
    pub tasks: Vec<TaskStateRow>,
}

pub trait EventStore: Send + Sync {
    /// 写穿组合入口(M2 规格 §5.1 写序):① 日志追加+flush → ② 事件物化进
    /// 规范状态 → ③ 位点推进。任一步失败即整体失败,调用方必须拒绝命令。
    fn record(&self, event: &EventEnvelope) -> StoreResult<()>;

    /// 启动恢复:修复位点之后的日志尾部(补物化),返回恢复报告。
    fn recover(&self) -> StoreResult<RecoveryReport>;

    /// 未终态 operation 清点:(id, agent_id, state)。
    fn pending_operations(&self) -> StoreResult<Vec<(String, String, String)>>;

    /// 行装配(内存视图重建)。
    fn load_rows(&self) -> StoreResult<WorldRows>;

    /// 单事件物化(恢复路径专用;写穿走 record)。
    fn materialize_event(&self, event: &EventEnvelope) -> StoreResult<()>;

    /// 保存回合输入原文(受保护存储;A4:原文不进事件/日志)。
    fn save_op_input(&self, operation_id: &str, content: &str) -> StoreResult<()>;

    /// 读回合输入原文(claim 续跑用)。
    fn op_input(&self, operation_id: &str) -> StoreResult<Option<String>>;

    /// 持久化取消意图标记(显式取消后、回合边界前崩溃时,恢复端据此走
    /// Resuming→Stopped(turn_was_stopping)边,不复活已取消回合)。
    fn mark_op_cancelled(&self, operation_id: &str, marked_at: &str) -> StoreResult<()>;

    /// 查询取消意图标记(恢复端判定 turn_was_stopping)。
    fn op_cancel_requested(&self, operation_id: &str) -> StoreResult<bool>;

    /// 会话绑定工作目录持久化(重启续聊配套;None = 解绑)。
    fn save_session_workspace(
        &self,
        session_id: &str,
        workspace_id: Option<&str>,
    ) -> StoreResult<()>;

    /// 会话删除侧效(2026-09-06 A+B):墓碑 + operations.input_content 擦除
    /// (单事务);context-log 文件过滤由调用方配 filter_lines_atomic 做。
    fn erase_session_contents(&self, session_id: &str, at: &str) -> StoreResult<()>;

    /// 会话行与其 agent 行删除(墓碑已在;事件重放侧由 load_rows 跳墓碑兜底)。
    fn delete_session_rows(&self, session_id: &str) -> StoreResult<()>;

    /// ① 日志先行:追加事件并 flush。失败 = 本次命令失败(核心循环须拒绝,不可静默)。
    fn append(&self, event: &EventEnvelope) -> StoreResult<()>;

    /// 投影重建的唯一合法依据(ADR-0004 条件 1):重放 seq > since 的事件。
    fn replay_since(&self, since_seq: u64) -> StoreResult<Vec<EventEnvelope>>;

    /// 日志末尾 seq(空 = 0)。
    fn last_log_seq(&self) -> StoreResult<u64>;

    /// 状态侧位点:SQLite 已应用到的事件 seq。
    fn last_applied_seq(&self) -> StoreResult<u64>;

    /// ② 状态侧位点推进(CAS 单调);由核心循环在状态物化提交后调用。
    fn mark_applied(&self, seq: u64) -> StoreResult<()>;

    /// 快照:记录 snapshot_seq(M2 中 SQLite 即活状态,快照 = 位点声明)。
    fn snapshot(&self) -> StoreResult<u64>;

    /// 压实:截断 seq ≤ up_to 的日志前缀(仅可在快照位点 ≥ up_to 后调用)。
    fn compact(&self, up_to_seq: u64) -> StoreResult<usize>;

    // ---- M4:approvals / grants / capabilities(审批中断恢复面)----------------
    /// 写入/更新审批对象(payload = 包装 JSON:approval 合同形态 + 未决时的
    /// 重放执行载荷)。
    fn save_approval(&self, row: ApprovalRow<'_>) -> StoreResult<()>;

    /// 恢复面:全部审批行(id, operation_id, state, payload)。
    fn list_approvals(&self) -> StoreResult<Vec<serde_json::Value>>;

    /// 写入/更新 Grant 行。
    fn save_grant(&self, row: GrantRow<'_>) -> StoreResult<()>;

    /// 恢复面:全部 Grant 行。
    fn list_grants(&self) -> StoreResult<Vec<serde_json::Value>>;

    /// 写入/更新 capability binding(epoch 持久计数)。
    fn save_capability_binding(&self, row: CapabilityRow<'_>) -> StoreResult<()>;

    /// 删除 capability binding。
    fn delete_capability_binding(&self, capability: &str) -> StoreResult<()>;

    /// 恢复面:全部 binding 行。
    fn list_capability_bindings(&self) -> StoreResult<Vec<serde_json::Value>>;

    /// outbox 记录 upsert(副作用对账底座;pending→published→verified)。
    fn outbox_upsert(
        &self,
        operation_id: &str,
        kind: &str,
        state: &str,
        payload: &str,
        now: &str,
    ) -> StoreResult<()>;

    /// 恢复面:指定状态的 outbox 行(如 pending = intent 无结果)。
    fn list_outbox_by_state(&self, state: &str) -> StoreResult<Vec<serde_json::Value>>;

    // ---- M8:评估报告(独立 Judge 产出;派生工件不进事件日志)----------------
    /// 写入评估报告(同 report_id 覆盖)。
    fn save_evaluation_report(
        &self,
        report_id: &str,
        from_seq: u64,
        to_seq: u64,
        payload: &str,
        created_at: &str,
    ) -> StoreResult<()>;
    /// 评估报告列表(按创建时间)。
    fn list_evaluation_reports(&self) -> StoreResult<Vec<serde_json::Value>>;

    // ---- M5:tasks / idempotency receipts(T1/T6c)----------------------------
    /// 写入/更新 Task 行(payload = task/task.v0.1 合同 JSON)。
    fn save_task(&self, row: TaskRow<'_>) -> StoreResult<()>;

    /// 恢复面:全部 Task 行。
    fn list_tasks(&self) -> StoreResult<Vec<serde_json::Value>>;

    /// 幂等收据落表(T6c):恢复期抑制判定不依赖内存。
    fn save_idem_receipt(&self, key_hash: &str, payload: &str, created_at: &str)
    -> StoreResult<()>;

    /// 恢复面:全部幂等收据行。
    fn list_idem_receipts(&self) -> StoreResult<Vec<serde_json::Value>>;

    /// Task 预算账本行 upsert(M5-T6;agent_id = "" 为 Task 级聚合)。
    fn save_task_budget(
        &self,
        task_id: &str,
        agent_id: &str,
        used_tool_calls: u64,
        used_tokens: u64,
        now: &str,
    ) -> StoreResult<()>;

    /// 恢复面:全部预算账本行。
    fn list_task_budget(&self) -> StoreResult<Vec<serde_json::Value>>;

    /// Observation Log 条目落表(M5-T8),返回 log_seq。
    fn save_observation(
        &self,
        task_id: &str,
        verdict: &str,
        guard_state: &str,
        payload: &str,
        observed_at: &str,
    ) -> StoreResult<u64>;

    /// 记忆写入(M5-T7;correction_of 即时墓碑化被纠正条目)。
    #[allow(clippy::too_many_arguments)]
    fn memory_put(
        &self,
        entry_id: &str,
        scope: &str,
        content_ref: &str,
        content_preview: Option<&str>,
        source_trust: &str,
        source_ref: Option<&str>,
        correction_of: Option<&str>,
        payload: &str,
        created_at: &str,
    ) -> StoreResult<()>;

    /// 记忆检索(scope 内非墓碑;FTS5 优先 LIKE 兜底)。
    fn memory_search(&self, scope: &str, query: &str) -> StoreResult<Vec<serde_json::Value>>;

    /// 记忆删除(墓碑 + 来源级联),返回级联数。
    fn memory_delete(&self, entry_id: &str) -> StoreResult<usize>;
}

pub fn filter_lines_atomic<F>(path: &Path, drop_line: F) -> std::io::Result<usize>
where
    F: Fn(&str) -> bool,
{
    use std::io::{BufRead, BufReader, Write};
    use std::sync::atomic::{AtomicU64, Ordering};
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let Ok(reader) = std::fs::File::open(path) else {
        return Ok(0); // 文件不存在 = 无可擦
    };
    // 序号唯一化临时名(P1-15 同族:防并发写互踩同一 tmp)。
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(format!(
        ".purge.tmp{}",
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = PathBuf::from(tmp_name);
    let mut out = std::fs::File::create(&tmp)?;
    let mut dropped = 0usize;
    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { break };
        if drop_line(&line) {
            dropped += 1;
            continue;
        }
        out.write_all(line.as_bytes())?;
        out.write_all(
            b"
",
        )?;
    }
    out.flush()?;
    out.sync_all()?;
    drop(out);
    std::fs::rename(&tmp, path)?;
    Ok(dropped)
}

/// 测试支撑(仅 bm-core 自身测试;真实实现见 bm-persist::PersistStore)。
/// 内存版 EventStore:record/append/replay_since/last_log_seq/last_applied_seq
/// 真实可用,其余方法按需补齐或显式未实现(测试不会触达即 panic 可见)。
#[cfg(test)]
pub mod test_support {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MemEventStore {
        events: Mutex<Vec<EventEnvelope>>,
    }

    impl MemEventStore {
        pub fn new() -> Self {
            Self::default()
        }
    }

    impl EventStore for MemEventStore {
        fn recover(&self) -> StoreResult<RecoveryReport> {
            unimplemented!("MemEventStore: recover 未在测试中触达")
        }
        fn pending_operations(&self) -> StoreResult<Vec<(String, String, String)>> {
            unimplemented!("MemEventStore: pending_operations 未在测试中触达")
        }
        fn load_rows(&self) -> StoreResult<WorldRows> {
            unimplemented!("MemEventStore: load_rows 未在测试中触达")
        }
        fn materialize_event(&self, _event: &EventEnvelope) -> StoreResult<()> {
            unimplemented!("MemEventStore: materialize_event 未在测试中触达")
        }
        fn save_op_input(&self, _operation_id: &str, _content: &str) -> StoreResult<()> {
            unimplemented!("MemEventStore: save_op_input 未在测试中触达")
        }
        fn op_input(&self, _operation_id: &str) -> StoreResult<Option<String>> {
            unimplemented!("MemEventStore: op_input 未在测试中触达")
        }
        fn mark_op_cancelled(&self, _operation_id: &str, _marked_at: &str) -> StoreResult<()> {
            unimplemented!("MemEventStore: mark_op_cancelled 未在测试中触达")
        }
        fn op_cancel_requested(&self, _operation_id: &str) -> StoreResult<bool> {
            unimplemented!("MemEventStore: op_cancel_requested 未在测试中触达")
        }
        fn save_session_workspace(
            &self,
            _session_id: &str,
            _workspace_id: Option<&str>,
        ) -> StoreResult<()> {
            unimplemented!("MemEventStore: save_session_workspace 未在测试中触达")
        }
        fn erase_session_contents(&self, _session_id: &str, _at: &str) -> StoreResult<()> {
            unimplemented!("MemEventStore: erase_session_contents 未在测试中触达")
        }
        fn delete_session_rows(&self, _session_id: &str) -> StoreResult<()> {
            unimplemented!("MemEventStore: delete_session_rows 未在测试中触达")
        }
        fn mark_applied(&self, _seq: u64) -> StoreResult<()> {
            unimplemented!("MemEventStore: mark_applied 未在测试中触达")
        }
        fn snapshot(&self) -> StoreResult<u64> {
            unimplemented!("MemEventStore: snapshot 未在测试中触达")
        }
        fn compact(&self, _up_to_seq: u64) -> StoreResult<usize> {
            unimplemented!("MemEventStore: compact 未在测试中触达")
        }
        fn save_approval(&self, _row: ApprovalRow<'_>) -> StoreResult<()> {
            unimplemented!("MemEventStore: save_approval 未在测试中触达")
        }
        fn list_approvals(&self) -> StoreResult<Vec<serde_json::Value>> {
            unimplemented!("MemEventStore: list_approvals 未在测试中触达")
        }
        fn save_grant(&self, _row: GrantRow<'_>) -> StoreResult<()> {
            unimplemented!("MemEventStore: save_grant 未在测试中触达")
        }
        fn list_grants(&self) -> StoreResult<Vec<serde_json::Value>> {
            unimplemented!("MemEventStore: list_grants 未在测试中触达")
        }
        fn save_capability_binding(&self, _row: CapabilityRow<'_>) -> StoreResult<()> {
            unimplemented!("MemEventStore: save_capability_binding 未在测试中触达")
        }
        fn delete_capability_binding(&self, _capability: &str) -> StoreResult<()> {
            unimplemented!("MemEventStore: delete_capability_binding 未在测试中触达")
        }
        fn list_capability_bindings(&self) -> StoreResult<Vec<serde_json::Value>> {
            unimplemented!("MemEventStore: list_capability_bindings 未在测试中触达")
        }
        fn outbox_upsert(
            &self,
            _operation_id: &str,
            _kind: &str,
            _state: &str,
            _payload: &str,
            _now: &str,
        ) -> StoreResult<()> {
            unimplemented!("MemEventStore: outbox_upsert 未在测试中触达")
        }
        fn list_outbox_by_state(&self, _state: &str) -> StoreResult<Vec<serde_json::Value>> {
            unimplemented!("MemEventStore: list_outbox_by_state 未在测试中触达")
        }
        fn save_evaluation_report(
            &self,
            _report_id: &str,
            _from_seq: u64,
            _to_seq: u64,
            _payload: &str,
            _created_at: &str,
        ) -> StoreResult<()> {
            unimplemented!("MemEventStore: save_evaluation_report 未在测试中触达")
        }
        fn list_evaluation_reports(&self) -> StoreResult<Vec<serde_json::Value>> {
            unimplemented!("MemEventStore: list_evaluation_reports 未在测试中触达")
        }
        fn save_task(&self, _row: TaskRow<'_>) -> StoreResult<()> {
            unimplemented!("MemEventStore: save_task 未在测试中触达")
        }
        fn list_tasks(&self) -> StoreResult<Vec<serde_json::Value>> {
            unimplemented!("MemEventStore: list_tasks 未在测试中触达")
        }
        fn save_idem_receipt(
            &self,
            _key_hash: &str,
            _payload: &str,
            _created_at: &str,
        ) -> StoreResult<()> {
            unimplemented!("MemEventStore: save_idem_receipt 未在测试中触达")
        }
        fn list_idem_receipts(&self) -> StoreResult<Vec<serde_json::Value>> {
            unimplemented!("MemEventStore: list_idem_receipts 未在测试中触达")
        }
        fn save_task_budget(
            &self,
            _task_id: &str,
            _agent_id: &str,
            _used_tool_calls: u64,
            _used_tokens: u64,
            _now: &str,
        ) -> StoreResult<()> {
            unimplemented!("MemEventStore: save_task_budget 未在测试中触达")
        }
        fn list_task_budget(&self) -> StoreResult<Vec<serde_json::Value>> {
            unimplemented!("MemEventStore: list_task_budget 未在测试中触达")
        }
        fn save_observation(
            &self,
            _task_id: &str,
            _verdict: &str,
            _guard_state: &str,
            _payload: &str,
            _observed_at: &str,
        ) -> StoreResult<u64> {
            unimplemented!("MemEventStore: save_observation 未在测试中触达")
        }
        fn memory_put(
            &self,
            _entry_id: &str,
            _scope: &str,
            _content_ref: &str,
            _content_preview: Option<&str>,
            _source_trust: &str,
            _source_ref: Option<&str>,
            _correction_of: Option<&str>,
            _payload: &str,
            _created_at: &str,
        ) -> StoreResult<()> {
            unimplemented!("MemEventStore: memory_put 未在测试中触达")
        }
        fn memory_search(&self, _scope: &str, _query: &str) -> StoreResult<Vec<serde_json::Value>> {
            unimplemented!("MemEventStore: memory_search 未在测试中触达")
        }
        fn memory_delete(&self, _entry_id: &str) -> StoreResult<usize> {
            unimplemented!("MemEventStore: memory_delete 未在测试中触达")
        }
        fn record(&self, event: &EventEnvelope) -> StoreResult<()> {
            self.events.lock().expect("锁").push(event.clone());
            Ok(())
        }
        fn append(&self, event: &EventEnvelope) -> StoreResult<()> {
            self.events.lock().expect("锁").push(event.clone());
            Ok(())
        }
        fn replay_since(&self, since_seq: u64) -> StoreResult<Vec<EventEnvelope>> {
            Ok(self
                .events
                .lock()
                .expect("锁")
                .iter()
                .filter(|e| e.event_seq > since_seq)
                .cloned()
                .collect())
        }
        fn last_log_seq(&self) -> StoreResult<u64> {
            Ok(self
                .events
                .lock()
                .expect("锁")
                .last()
                .map(|e| e.event_seq)
                .unwrap_or(0))
        }
        fn last_applied_seq(&self) -> StoreResult<u64> {
            self.last_log_seq()
        }
    }
}
