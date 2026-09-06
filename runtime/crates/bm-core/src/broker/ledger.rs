//! Grant 台账(M4 内存版;「调用方×目标 Capability」O(1) 查表;自 broker.rs 机械移入)。
use super::types::LedgerError;
use bm_contract::capability::{Grant, GrantScope};
use bm_contract::timestamp::parse_ts;
use std::collections::HashMap;

struct LedgerEntry {
    grant: Grant,
    used_count: u64,
    revoked: bool,
}

impl std::fmt::Debug for LedgerEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LedgerEntry")
            .field("grant_id", &self.grant.grant_id)
            .field("used_count", &self.used_count)
            .field("revoked", &self.revoked)
            .finish()
    }
}

/// Grant 台账(M4 内存版;T3 由 SQLite grants 表承载同一索引语义)。
/// `index`(audience × action → grant_ids)即「调用方×目标 Capability」
/// 查表:签发/撤销时增量重编译并递增 policy_version。
#[derive(Debug, Default)]
pub struct GrantLedger {
    entries: HashMap<String, LedgerEntry>,
    index: HashMap<(String, String), Vec<String>>,
    policy_version: u64,
}

impl GrantLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// 策略表版本:lease 准入比对项(签发后策略变更 → 旧 lease 失效)。
    pub fn policy_version(&self) -> u64 {
        self.policy_version
    }

    /// 物化/签发一条 Grant(审批 approved 的下游;T3 接线)。
    pub fn record(&mut self, grant: Grant) {
        let key = (grant.audience.clone(), grant.action.clone());
        let id = grant.grant_id.clone();
        self.entries.insert(
            id.clone(),
            LedgerEntry {
                grant,
                used_count: 0,
                revoked: false,
            },
        );
        self.index.entry(key).or_default().push(id);
        self.policy_version += 1;
    }

    /// 撤销:revocation_version 单调 +1,旧副本即刻失效(可撤销,基线 §11.3)。
    pub fn revoke(&mut self, grant_id: &str) -> Result<u64, LedgerError> {
        let entry = self
            .entries
            .get_mut(grant_id)
            .ok_or(LedgerError::UnknownGrant)?;
        entry.revoked = true;
        entry.grant.revocation_version += 1;
        self.policy_version += 1;
        Ok(entry.grant.revocation_version)
    }

    /// O(1) 索引命中 + 常量级有效性校验(撤销/过期/计数)。返回克隆快照,
    /// 避免借用跨越决策。
    pub fn active_for(
        &self,
        audience: &str,
        action: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<Grant> {
        let Some(ids) = self.index.get(&(audience.to_string(), action.to_string())) else {
            return Vec::new();
        };
        ids.iter()
            .filter_map(|id| self.entries.get(id))
            .filter(|e| Self::entry_is_active(e, now))
            .map(|e| e.grant.clone())
            .collect()
    }

    /// 执行前预扣一次授权(Once/Count;Ttl/Forever/Task 不计数)。
    pub fn consume(
        &mut self,
        grant_id: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), LedgerError> {
        let now_clamped = now;
        let (active, scope) = {
            let entry = self
                .entries
                .get(grant_id)
                .ok_or(LedgerError::UnknownGrant)?;
            (
                Self::entry_is_active(entry, now_clamped),
                entry.grant.scope.clone(),
            )
        };
        if !active {
            return Err(LedgerError::GrantExhausted);
        }
        let entry = self
            .entries
            .get_mut(grant_id)
            .ok_or(LedgerError::UnknownGrant)?;
        match scope {
            GrantScope::Once | GrantScope::Count(_) => {
                entry.used_count += 1;
                if matches!(scope, GrantScope::Once) {
                    entry.revoked = true; // Once:首次消费即失效
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 有效性判定(纯函数,不借台账:撤销/过期/计数常量校验)。
    fn entry_is_active(entry: &LedgerEntry, now: chrono::DateTime<chrono::Utc>) -> bool {
        if entry.revoked {
            return false;
        }
        if let Some(expires_at) = &entry.grant.expires_at {
            match parse_ts(expires_at) {
                Some(t) if t > now => {}
                _ => return false,
            }
        }
        match entry.grant.scope {
            GrantScope::Once => entry.used_count == 0,
            GrantScope::Count(n) => entry.used_count < n,
            _ => true,
        }
    }

    pub fn get(&self, grant_id: &str) -> Option<&Grant> {
        self.entries.get(grant_id).map(|e| &e.grant)
    }

    /// 按作用域查 Grant(M5:task:<id> 作用域的「Task 结束即失效」撤销面)。
    /// 返回该作用域的全部 Grant(含已撤销;调用方按需过滤)。
    pub fn grants_scoped_to(&self, task_id: &str) -> Vec<Grant> {
        self.entries
            .values()
            .map(|e| &e.grant)
            .filter(|g| matches!(&g.scope, GrantScope::Task(t) if t == task_id))
            .cloned()
            .collect()
    }

    /// 持久化视图:条目的 (used_count, revoked),供恢复/落库同步。
    pub fn entry_state(&self, grant_id: &str) -> Option<(u64, bool)> {
        self.entries
            .get(grant_id)
            .map(|e| (e.used_count, e.revoked))
    }

    /// 恢复:按持久行重建条目(used/revoked 原样装载)。
    pub fn restore(&mut self, grant: Grant, used_count: u64, revoked: bool) {
        let key = (grant.audience.clone(), grant.action.clone());
        let id = grant.grant_id.clone();
        self.entries.insert(
            id.clone(),
            LedgerEntry {
                grant,
                used_count,
                revoked,
            },
        );
        self.index.entry(key).or_default().push(id);
        self.policy_version += 1;
    }
}
