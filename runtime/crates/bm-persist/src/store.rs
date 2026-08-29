//! EventStore 端口与默认实现:JSONL 日志(事实史)+ SQLite 状态(快路径)的组合。
//! 核心循环只依赖本端口;替换实现(M7 外置进程)调用方无感。

use crate::error::{StoreError, StoreResult};
use crate::event_log::JsonlEventLog;
use crate::sqlite_state::StateDb;
use bm_contract::events::EventEnvelope;
use std::path::Path;

pub const META_LAST_APPLIED: &str = "last_applied_seq";
pub const META_SNAPSHOT_SEQ: &str = "snapshot_seq";

pub trait EventStore: Send + Sync {
    /// 写穿组合入口(M2 规格 §5.1 写序):① 日志追加+flush → ② 事件物化进
    /// 规范状态 → ③ 位点推进。任一步失败即整体失败,调用方必须拒绝命令。
    fn record(&self, event: &EventEnvelope) -> StoreResult<()>;

    /// 启动恢复:修复位点之后的日志尾部(补物化),返回恢复报告。
    fn recover(&self) -> StoreResult<crate::recovery::RecoveryReport>;

    /// 未终态 operation 清点:(id, agent_id, state)。
    fn pending_operations(&self) -> StoreResult<Vec<(String, String, String)>>;

    /// 行装配(内存视图重建)。
    fn load_rows(&self) -> StoreResult<crate::recovery::WorldRows>;

    /// 单事件物化(恢复路径专用;写穿走 record)。
    fn materialize_event(&self, event: &EventEnvelope) -> StoreResult<()>;

    /// 保存回合输入原文(受保护存储;A4:原文不进事件/日志)。
    fn save_op_input(&self, operation_id: &str, content: &str) -> StoreResult<()>;

    /// 读回合输入原文(claim 续跑用)。
    fn op_input(&self, operation_id: &str) -> StoreResult<Option<String>>;

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
    fn save_approval(&self, row: crate::sqlite_state::ApprovalRow<'_>) -> StoreResult<()>;

    /// 恢复面:全部审批行(id, operation_id, state, payload)。
    fn list_approvals(&self) -> StoreResult<Vec<serde_json::Value>>;

    /// 写入/更新 Grant 行。
    fn save_grant(&self, row: crate::sqlite_state::GrantRow<'_>) -> StoreResult<()>;

    /// 恢复面:全部 Grant 行。
    fn list_grants(&self) -> StoreResult<Vec<serde_json::Value>>;

    /// 写入/更新 capability binding(epoch 持久计数)。
    fn save_capability_binding(
        &self,
        row: crate::sqlite_state::CapabilityRow<'_>,
    ) -> StoreResult<()>;

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

    // ---- M5:tasks / idempotency receipts(T1/T6c)----------------------------
    /// 写入/更新 Task 行(payload = task/task.v0.1 合同 JSON)。
    fn save_task(&self, row: crate::sqlite_state::TaskRow<'_>) -> StoreResult<()>;

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

/// 默认压实触发间隔(条);ADR-0004 条件 2:压实是强制义务,不是可选项。
pub const DEFAULT_COMPACTION_EVERY: u64 = 10_000;

/// 默认组合实现。
pub struct PersistStore {
    log: JsonlEventLog,
    state: StateDb,
    /// 每 N 条事件自动 快照+压实;None = 关闭(测试专用)。
    compaction_every: Option<u64>,
}

impl PersistStore {
    /// 打开目录下的 `events.jsonl` 与 `state.db`,并做互为校验:
    /// last_applied_seq ≤ last_log_seq,违反即判库损坏(拒绝服务,宁可拒开不可双写)。
    pub fn open(dir: &Path) -> StoreResult<Self> {
        let log = JsonlEventLog::open(dir.join("events.jsonl"))?;
        let state = StateDb::open(&dir.join("state.db"))?;
        let applied: u64 = state
            .meta_get(META_LAST_APPLIED)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let log_last = log.last_seq()?;
        if applied > log_last {
            return Err(StoreError::Corrupt {
                seq: applied,
                reason: format!(
                    "状态位点 {applied} 超前于日志末尾 {log_last}(违反先日志后状态写序)"
                ),
            });
        }
        Ok(Self {
            log,
            state,
            compaction_every: Some(DEFAULT_COMPACTION_EVERY),
        })
    }

    /// 韧性打开(混沌②,M2 规格 §6-T8):状态库损坏时将其隔离,
    /// 自事件日志重建投影——「投影重建只绑定事件日志」的终极验证。
    /// 边界:若日志已压实(前缀不在),前缀状态无法重建,拒绝并要求快照恢复。
    /// 返回 (store, 是否发生重建)。
    pub fn open_resilient(dir: &Path) -> StoreResult<(Self, bool)> {
        match Self::open(dir) {
            Ok(s) => Ok((s, false)),
            Err(StoreError::Sql(_)) | Err(StoreError::Corrupt { .. }) => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                for name in ["state.db", "state.db-wal", "state.db-shm"] {
                    let p = dir.join(name);
                    if p.exists() {
                        std::fs::rename(&p, dir.join(format!("{name}.corrupt-{stamp}")))?;
                    }
                }
                let s = Self::open(dir)?;
                let upto = s.last_log_seq()?;
                let rebuilt = crate::recovery::rebuild_projection(&s, upto, &s.state)?;
                tracing::warn!(rebuilt_seq = %rebuilt, "状态库损坏,已自事件日志重建投影");
                Ok((s, true))
            }
            Err(e) => Err(e),
        }
    }

    /// 以自定义压实间隔打开(测试小间隔;生产用默认)。
    pub fn with_compaction(dir: &Path, every_n: u64) -> StoreResult<Self> {
        let mut me = Self::open(dir)?;
        me.compaction_every = Some(every_n.max(1));
        Ok(me)
    }

    /// 关闭自动压实(测试专用)。
    pub fn without_compaction(mut self) -> Self {
        self.compaction_every = None;
        self
    }

    fn maybe_autocompact(&self, seq: u64) -> StoreResult<()> {
        let Some(every) = self.compaction_every else {
            return Ok(());
        };
        let snap: u64 = self
            .state
            .meta_get(META_SNAPSHOT_SEQ)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if seq.saturating_sub(snap) >= every {
            self.snapshot()?;
            self.compact(seq)?;
            tracing::info!(seq = %seq, snapshot = %seq, "自动压实完成");
        }
        Ok(())
    }

    /// 状态库只读访问(恢复与测试断言用)。
    pub fn state(&self) -> &StateDb {
        &self.state
    }

    /// 混沌④/CAS 门禁:带过期 expect 的写入被拒并落审计事件
    /// `store.write.rejected`(M4 epoch fencing 的底座行为)。返回是否发生拒绝。
    pub fn reject_and_audit(
        &self,
        audit_seq: u64,
        key: &str,
        stale_expect: &str,
        new: &str,
    ) -> StoreResult<bool> {
        match self
            .state
            .meta_compare_and_set(key, Some(stale_expect), new)
        {
            Ok(()) => Ok(false),
            Err(StoreError::CasMismatch { .. }) => {
                let event = EventEnvelope::new(
                    audit_seq,
                    bm_contract::events::EventType::StoreWriteRejected,
                    bm_contract::timestamp::now(),
                    None,
                    None,
                    None,
                    serde_json::json!({ "key": key, "reason": "stale_epoch" }),
                );
                self.record(&event)?;
                Ok(true)
            }
            Err(e) => Err(e),
        }
    }

    /// 当前快照位点(未快照过 = None)。
    pub fn snapshot_seq(&self) -> StoreResult<Option<u64>> {
        Ok(self
            .state
            .meta_get(META_SNAPSHOT_SEQ)?
            .and_then(|v| v.parse().ok()))
    }

    fn applied(&self) -> StoreResult<u64> {
        Ok(self
            .state
            .meta_get(META_LAST_APPLIED)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0))
    }
}

impl EventStore for PersistStore {
    fn record(&self, event: &EventEnvelope) -> StoreResult<()> {
        // ① 日志先行(必须先于状态,崩溃窗口单向)
        self.log.append(event, true)?;
        // ② 物化 + ③ 位点,同一状态侧顺序
        self.state.materialize(event)?;
        self.mark_applied(event.event_seq)?;
        // ④ 达到间隔则快照+压实(失败不阻断写路径:压实是优化,重试即可)
        if let Err(e) = self.maybe_autocompact(event.event_seq) {
            tracing::warn!(error = %e, seq = %event.event_seq, "自动压实失败(不影响写入)");
        }
        Ok(())
    }

    fn recover(&self) -> StoreResult<crate::recovery::RecoveryReport> {
        let replayed = crate::recovery::repair_tail(self)?;
        let interrupted_recovered = crate::recovery::pending_operations(&self.state)?.len();
        Ok(crate::recovery::RecoveryReport {
            last_applied_seq: self.applied()?,
            replayed,
            interrupted_recovered,
        })
    }

    fn pending_operations(&self) -> StoreResult<Vec<(String, String, String)>> {
        crate::recovery::pending_operations(&self.state)
    }

    fn load_rows(&self) -> StoreResult<crate::recovery::WorldRows> {
        crate::recovery::load_rows(&self.state)
    }

    fn materialize_event(&self, event: &EventEnvelope) -> StoreResult<()> {
        self.state.materialize(event)
    }

    fn save_op_input(&self, operation_id: &str, content: &str) -> StoreResult<()> {
        self.state.save_op_input(operation_id, content)
    }

    fn op_input(&self, operation_id: &str) -> StoreResult<Option<String>> {
        self.state.op_input(operation_id)
    }

    fn append(&self, event: &EventEnvelope) -> StoreResult<()> {
        self.log.append(event, true)
    }

    fn replay_since(&self, since_seq: u64) -> StoreResult<Vec<EventEnvelope>> {
        self.log.replay_since(since_seq)
    }

    fn last_log_seq(&self) -> StoreResult<u64> {
        self.log.last_seq()
    }

    fn last_applied_seq(&self) -> StoreResult<u64> {
        self.applied()
    }

    fn mark_applied(&self, seq: u64) -> StoreResult<()> {
        let current = self.applied()?;
        if seq <= current {
            return Err(StoreError::Corrupt {
                seq,
                reason: format!("位点必须单调推进(当前 {current})"),
            });
        }
        let expect = if current == 0 {
            None
        } else {
            Some(current.to_string())
        };
        self.state
            .meta_compare_and_set(META_LAST_APPLIED, expect.as_deref(), &seq.to_string())
    }

    fn snapshot(&self) -> StoreResult<u64> {
        let applied = self.applied()?;
        // 从上个快照位点单调推进(CAS);重复同位点为幂等快照
        let prev = self.snapshot_seq()?;
        if prev == Some(applied) {
            return Ok(applied);
        }
        let expect = prev.map(|v| v.to_string());
        self.state.meta_compare_and_set(
            META_SNAPSHOT_SEQ,
            expect.as_deref(),
            &applied.to_string(),
        )?;
        Ok(applied)
    }

    fn compact(&self, up_to_seq: u64) -> StoreResult<usize> {
        let snap: u64 = self
            .state
            .meta_get(META_SNAPSHOT_SEQ)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if up_to_seq > snap {
            return Err(StoreError::Corrupt {
                seq: up_to_seq,
                reason: format!("压实前缀 {up_to_seq} 超过快照位点 {snap}:重放将缺失前缀"),
            });
        }
        self.log.truncate_prefix(up_to_seq)
    }

    fn save_approval(&self, row: crate::sqlite_state::ApprovalRow<'_>) -> StoreResult<()> {
        self.state.save_approval(row)
    }

    fn list_approvals(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.state.list_approvals()
    }

    fn save_grant(&self, row: crate::sqlite_state::GrantRow<'_>) -> StoreResult<()> {
        self.state.save_grant(row)
    }

    fn list_grants(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.state.list_grants()
    }

    fn save_capability_binding(
        &self,
        row: crate::sqlite_state::CapabilityRow<'_>,
    ) -> StoreResult<()> {
        self.state.save_capability_binding(row)
    }

    fn list_capability_bindings(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.state.list_capability_bindings()
    }

    fn outbox_upsert(
        &self,
        operation_id: &str,
        kind: &str,
        state: &str,
        payload: &str,
        now: &str,
    ) -> StoreResult<()> {
        self.state
            .outbox_upsert(operation_id, kind, state, payload, now)
    }

    fn list_outbox_by_state(&self, state: &str) -> StoreResult<Vec<serde_json::Value>> {
        self.state.list_outbox_by_state(state)
    }

    fn save_task(&self, row: crate::sqlite_state::TaskRow<'_>) -> StoreResult<()> {
        self.state.save_task(row)
    }

    fn list_tasks(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.state.list_tasks()
    }

    fn save_idem_receipt(
        &self,
        key_hash: &str,
        payload: &str,
        created_at: &str,
    ) -> StoreResult<()> {
        self.state.save_idem_receipt(key_hash, payload, created_at)
    }

    fn list_idem_receipts(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.state.list_idem_receipts()
    }

    fn save_task_budget(
        &self,
        task_id: &str,
        agent_id: &str,
        used_tool_calls: u64,
        used_tokens: u64,
        now: &str,
    ) -> StoreResult<()> {
        self.state
            .save_task_budget(task_id, agent_id, used_tool_calls, used_tokens, now)
    }

    fn list_task_budget(&self) -> StoreResult<Vec<serde_json::Value>> {
        self.state.list_task_budget()
    }

    fn save_observation(
        &self,
        task_id: &str,
        verdict: &str,
        guard_state: &str,
        payload: &str,
        observed_at: &str,
    ) -> StoreResult<u64> {
        self.state
            .save_observation(task_id, verdict, guard_state, payload, observed_at)
    }

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
    ) -> StoreResult<()> {
        self.state.memory_put(
            entry_id,
            scope,
            content_ref,
            content_preview,
            source_trust,
            source_ref,
            correction_of,
            payload,
            created_at,
        )
    }

    fn memory_search(&self, scope: &str, query: &str) -> StoreResult<Vec<serde_json::Value>> {
        self.state.memory_search(scope, query)
    }

    fn memory_delete(&self, entry_id: &str) -> StoreResult<usize> {
        self.state.memory_delete(entry_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_contract::events::EventType;

    fn ev(seq: u64) -> EventEnvelope {
        EventEnvelope::new_unchecked(
            seq,
            EventType::RuntimeStarted,
            bm_contract::timestamp::now(),
            None,
            None,
            None,
            serde_json::json!({}),
        )
    }

    #[test]
    fn write_path_ordering_and_site_monotonic() {
        let dir = tempfile::tempdir().expect("临时目录");
        let store = PersistStore::open(dir.path()).expect("打开");

        // ① 日志先行
        store.append(&ev(1)).expect("追加 1");
        store.append(&ev(2)).expect("追加 2");
        assert_eq!(store.last_log_seq().expect("日志末尾"), 2);
        assert_eq!(store.last_applied_seq().expect("位点"), 0, "状态侧尚未推进");

        // ② 位点单调推进
        store.mark_applied(1).expect("推进到 1");
        store.mark_applied(2).expect("推进到 2");
        assert!(store.mark_applied(2).is_err(), "位点不可回退/重复");

        // 快照 + 压实
        let snap = store.snapshot().expect("快照");
        assert_eq!(snap, 2);
        assert_eq!(store.compact(2).expect("压实"), 2);
        assert!(store.compact(3).is_err(), "压实不可超过快照位点");
        assert_eq!(store.replay_since(0).expect("重放").len(), 0, "前缀已截断");
    }

    #[test]
    fn cross_check_rejects_state_ahead_of_log() {
        let dir = tempfile::tempdir().expect("临时目录");
        {
            let store = PersistStore::open(dir.path()).expect("打开");
            store.append(&ev(1)).expect("追加");
            store.mark_applied(1).expect("推进");
        }
        // 人为制造「状态超前于日志」的损坏:删掉日志尾部
        let log_path = dir.path().join("events.jsonl");
        std::fs::write(&log_path, "").expect("清空日志");
        assert!(
            PersistStore::open(dir.path()).is_err(),
            "状态超前于日志必须拒开(互为校验)"
        );
    }
}

#[cfg(test)]
mod t7_injection_tests {
    use super::*;
    use std::io::Write as _;

    /// T7 伪造/乱序注入(硬约束 3;ADR-0001 条件 3):绕过单写者直接写
    /// 持久日志的伪造事件,回放时必须可检出——未注册类型反序列化失败;
    /// 乱序 seq 的投影按 seq 排序后不变(INV-3)。
    #[test]
    fn forged_and_out_of_order_lines_are_detectable() {
        let dir = tempfile::tempdir().expect("临时目录");
        let path = dir.path().join("events.jsonl");
        // 合法事件一行(seq 1)
        let good = r#"{"event_seq":1,"type":"runtime.started","occurred_at":"2026-08-29T10:00:00.000Z","payload":{"pid":1,"version":"0.1.0","started_at":"2026-08-29T10:00:00.000Z"}}"#;
        // 伪造类型行(注册表外)
        let forged = r#"{"event_seq":2,"type":"attacker.command.executed","occurred_at":"2026-08-29T10:00:01.000Z","payload":{"requested_action":"mail.send"}}"#;
        // 乱序行(seq 9,跳号)
        let out_of_order = r#"{"event_seq":9,"type":"session.created","occurred_at":"2026-08-29T10:00:02.000Z","payload":{"session_id":"s"}}"#;
        {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .expect("打开日志");
            for line in [good, forged, out_of_order] {
                writeln!(f, "{line}").expect("写");
            }
            f.flush().expect("flush");
        }

        // 检出(ADR-0001 条件 3):伪造类型未注册 → 反序列化失败 → 打开即报
        // Corrupt 拒绝服务。磁盘被篡改意味着信任根已破(T-12),「宁可拒开」
        // 是正确的安全响应,而非静默跳过伪造行伪装无事。
        let err = match PersistStore::open(dir.path()) {
            Err(e) => e,
            Ok(_) => panic!("伪造行必须被检出,拒绝打开"),
        };
        assert!(matches!(err, StoreError::Corrupt { .. }), "{err:?}");
        assert!(
            format!("{err:?}").contains("attacker"),
            "错误信息指认伪造行"
        );
    }
}
