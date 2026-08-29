//! 时间戳格式:合同一律 ISO-8601 UTC,毫秒精度、`Z` 后缀(基线 8.3)。

use chrono::{DateTime, SecondsFormat, Utc};

/// 由 `DateTime<Utc>` 格式化为合同形态(`2026-08-29T09:30:00.100Z`)。
pub fn format_ts(t: DateTime<Utc>) -> super::BmTimestamp {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn now() -> super::BmTimestamp {
    format_ts(Utc::now())
}

/// 宽松解析(供测试断言排序用)。
pub fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_matches_contract_shape() {
        let t = DateTime::parse_from_rfc3339("2026-08-29T09:30:00.100Z").unwrap();
        assert_eq!(format_ts(t.with_timezone(&Utc)), "2026-08-29T09:30:00.100Z");
    }
}

/// 距给定时间戳的剩余时长;不可解析返回 None,已过期为 Some(0)。
/// M7 起供连接器把合同 deadline 折算成 HTTP 超时预算(bm-providers 无 chrono)。
pub fn remaining_until(ts: &str) -> Option<std::time::Duration> {
    let dl = parse_ts(ts)?;
    let remaining = dl - chrono::Utc::now();
    if remaining <= chrono::Duration::zero() {
        return Some(std::time::Duration::ZERO);
    }
    remaining.to_std().ok()
}
