//! 合同 ID:`<prefix>_<ULID26>`(wire/envelope.v0_1.schema.json #/definitions/id)。
//!
//! 前缀 `[a-z][a-z0-9_]{1,15}`;ULID 段为 26 位 Crockford Base32(无 I/L/O/U)。

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

pub const ULID_LEN: usize = 26;
pub const PREFIX_MAX: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum IdError {
    #[error(
        "ID 无效: {0:?}(须为 <prefix>_<ULID26>,前缀 [a-z][a-z0-9_]{{1,15}},ULID 26 位 Crockford)"
    )]
    Format(String),
    #[error("前缀无效: {0:?}")]
    Prefix(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BmId(String);

fn is_crockford(c: char) -> bool {
    c.is_ascii_digit() || c.is_ascii_uppercase() && !"ILOU".contains(c)
}

fn validate_prefix(prefix: &str) -> Result<(), IdError> {
    let mut chars = prefix.chars();
    let ok = matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && prefix.len() >= 2
        && prefix.len() <= PREFIX_MAX
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(IdError::Prefix(prefix.to_string()))
    }
}

fn validate(s: &str) -> Result<(), IdError> {
    let (prefix, ulid) = s
        .split_once('_')
        .ok_or_else(|| IdError::Format(s.to_string()))?;
    validate_prefix(prefix)?;
    if ulid.len() != ULID_LEN || !ulid.chars().all(is_crockford) {
        return Err(IdError::Format(s.to_string()));
    }
    Ok(())
}

impl BmId {
    /// 解析并校验一个完整 ID 字符串。
    pub fn parse(s: impl Into<String>) -> Result<Self, IdError> {
        let s = s.into();
        validate(&s)?;
        Ok(Self(s))
    }

    /// 由前缀 + ULID26 段拼装(段由 [`ulid26_for_counter`] 或 ulid crate 产出)。
    pub fn from_parts(prefix: &str, ulid26: &str) -> Result<Self, IdError> {
        validate_prefix(prefix)?;
        if ulid26.len() != ULID_LEN || !ulid26.chars().all(is_crockford) {
            return Err(IdError::Format(format!("{prefix}_{ulid26}")));
        }
        Ok(Self(format!("{prefix}_{ulid26}")))
    }

    /// 用真实 ULID 生成(生产路径)。前缀须先经 [`validate_prefix`] 校验。
    pub fn generate(prefix: &str) -> Self {
        Self::from_parts(prefix, &ulid::Ulid::new().to_string()).expect("前缀已由调用方校验")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn prefix(&self) -> &str {
        self.0.split_once('_').expect("构造时已校验").0
    }

    pub fn ulid_part(&self) -> &str {
        self.0.split_once('_').expect("构造时已校验").1
    }
}

impl fmt::Display for BmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for BmId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, IdError> {
        BmId::parse(s)
    }
}

impl Serialize for BmId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for BmId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        BmId::parse(s).map_err(serde::de::Error::custom)
    }
}

/// 计数器 → 合法 ULID26 段(十进制数字是 Crockford 字符集子集)。
/// 供测试/回放的确定性 ID 使用;真实 ULID 排序语义不适用于它。
pub fn ulid26_for_counter(n: u64) -> String {
    let s = n.to_string();
    let pad = ULID_LEN.saturating_sub(s.len());
    format!("{}{}", "0".repeat(pad), s)
}

/// ID 生成端口:生产用 [`UlidIdGen`],测试/回放用 [`SeqIdGen`] 保证确定性。
pub trait IdGen: Send + Sync {
    fn next_id(&self, prefix: &str) -> BmId;
}

pub struct UlidIdGen;

impl IdGen for UlidIdGen {
    fn next_id(&self, prefix: &str) -> BmId {
        BmId::generate(prefix)
    }
}

/// 确定性生成器:同一前缀下按 1,2,3,… 递增。测试断言可预先算出全部 ID。
pub struct SeqIdGen(AtomicU64);

impl SeqIdGen {
    pub fn new() -> Self {
        Self(AtomicU64::new(0))
    }
}

impl Default for SeqIdGen {
    fn default() -> Self {
        Self::new()
    }
}

impl SeqIdGen {
    /// 从指定计数器起生成(持久化恢复:跳过已用号段防撞)。
    pub fn starting_at(n: u64) -> Self {
        Self(AtomicU64::new(n))
    }
}

impl IdGen for SeqIdGen {
    fn next_id(&self, prefix: &str) -> BmId {
        let n = self.0.fetch_add(1, Ordering::Relaxed) + 1;
        BmId::from_parts(prefix, &ulid26_for_counter(n)).expect("前缀与计数器段均合法")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_ids() {
        for s in [
            "req_01J9Z8G3K2X7M4Q6B8WD5RNYVT",
            "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
            "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
        ] {
            let id = BmId::parse(s).expect("合法");
            assert_eq!(id.as_str(), s);
        }
    }

    #[test]
    fn rejects_bad_ids() {
        for s in [
            "",
            "req_",
            "Req_01J9Z8G3K2X7M4Q6B8WD5RNYVT",
            "req_01J9Z8G3K2X7M4Q6B8WD5RNYVU",
            "req_01J9Z8G3K2X7M4Q6B8WD5RNYVTX",
            "9req_01J9Z8G3K2X7M4Q6B8WD5RNYVT",
        ] {
            assert!(BmId::parse(s).is_err(), "{s} 应被拒绝");
        }
    }

    #[test]
    fn seq_gen_is_deterministic() {
        let g = SeqIdGen::new();
        assert_eq!(g.next_id("req").as_str(), "req_00000000000000000000000001");
        assert_eq!(g.next_id("req").as_str(), "req_00000000000000000000000002");
        assert_eq!(
            g.next_id("sess").as_str(),
            "sess_00000000000000000000000003"
        );
    }
}
