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
}

/// 默认组合实现。
pub struct PersistStore {
    log: JsonlEventLog,
    state: StateDb,
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
        Ok(Self { log, state })
    }

    /// 状态库只读访问(恢复与测试断言用)。
    pub fn state(&self) -> &StateDb {
        &self.state
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
        self.mark_applied(event.event_seq)
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
        self.state
            .meta_compare_and_set(META_SNAPSHOT_SEQ, None, &applied.to_string())?;
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
