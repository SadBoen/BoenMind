//! B6 — 插件权限决策记忆：`extension-permissions.json` 持久化
//! （"总是允许"/"总是拒绝"跨会话生效）。
//!
//! 语义吸收自 pi_agent_rust@44ddf80/src/permissions.rs（B6 决策记忆的参照
//! 实现），**文件格式与其完全兼容**——pi 引擎路径与 bm 引擎路径读写的
//! 是同一个 schema（`{version, decisions: {extension_id: [{capability,
//! allow, decided_at, expires_at?, version_range?}]}}`），双开对比期两路径
//! 行为一致、决策可互认。
//!
//! 自研差异（刻意简化）：文件锁用进程内互斥（bm-server 单进程持有两
//! 引擎，无跨进程争用）；时间戳用 unix 秒（`expires_at` 仍接受 RFC3339，
//! 过期判定按秒比较）；不依赖 fs4/chrono。

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 磁盘 schema 版本（与 legacy 一致）。
const CURRENT_VERSION: u32 = 1;

/// 一条持久化决策。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedDecision {
    pub capability: String,
    /// true = 允许；false = 拒绝。
    pub allow: bool,
    /// unix 秒时间戳（legacy 存 ISO-8601 字符串；读入时两格式都接受，
    /// 写出用 ISO-8601 保格式兼容）。
    pub decided_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_range: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PermissionsFile {
    version: u32,
    /// extension_id → 决策列表（legacy 格式为数组）。
    decisions: std::collections::BTreeMap<String, Vec<PersistedDecision>>,
}

/// 内存镜像 + 磁盘读写的决策存储。
#[derive(Debug)]
pub struct PermissionStore {
    path: PathBuf,
    /// extension_id → capability → 决策。
    decisions: HashMap<String, HashMap<String, PersistedDecision>>,
}

impl PermissionStore {
    /// 打开（或懒创建）指定路径的存储。文件损坏时报错（不静默清空——
    /// 用户既有决策是资产，宁可 fail-closed 让询问链兜底）。
    pub fn open(path: &Path) -> Result<Self, String> {
        let decisions = if path.exists() {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read permissions file {}: {e}", path.display()))?;
            let file: PermissionsFile = serde_json::from_str(&raw).map_err(|e| {
                format!("Failed to parse permissions file {}: {e}", path.display())
            })?;
            if file.version != CURRENT_VERSION {
                return Err(format!(
                    "Unsupported permissions file schema version {} in {} (expected {CURRENT_VERSION})",
                    file.version,
                    path.display(),
                ));
            }
            file.decisions
                .into_iter()
                .map(|(ext, decs)| {
                    let by_cap = decs.into_iter().map(|d| (d.capability.clone(), d)).collect();
                    (ext, by_cap)
                })
                .collect()
        } else {
            HashMap::new()
        };
        Ok(Self {
            path: path.to_path_buf(),
            decisions,
        })
    }

    /// 查 (extension_id, capability) 的已记忆决策。`None` = 无记忆（或已过期）→ 询问。
    pub fn lookup(&self, extension_id: &str, capability: &str) -> Option<bool> {
        let dec = self.decisions.get(extension_id)?.get(capability)?;
        if decision_is_active(dec) {
            Some(dec.allow)
        } else {
            None
        }
    }

    /// 记忆一次决策并持久化（写前重载磁盘、读-改-写、原子落盘）。
    /// 调用方以互斥锁保护 `&mut self`（bm-server 单进程内两引擎共用）。
    pub fn record(
        &mut self,
        extension_id: &str,
        capability: &str,
        allow: bool,
    ) -> Result<(), String> {
        // 写前重载磁盘（对齐 legacy update_persisted_decisions：多入口写入
        // 不互相覆盖；pi 路径的上游引擎也在写同一文件）
        let mut decisions = load_decisions(&self.path).unwrap_or_else(|err| {
            tracing::warn!(event = "bm.permission_store_reload_failed", error = %err, "权限记忆写前重载失败，以内存态续写");
            self.decisions.clone()
        });
        decisions
            .entry(extension_id.to_string())
            .or_default()
            .insert(
                capability.to_string(),
                PersistedDecision {
                    capability: capability.to_string(),
                    allow,
                    decided_at: now_iso8601(),
                    expires_at: None,
                    version_range: None,
                },
            );
        save_decisions(&self.path, &decisions)?;
        self.decisions = decisions;
        Ok(())
    }
}

fn load_decisions(
    path: &Path,
) -> Result<HashMap<String, HashMap<String, PersistedDecision>>, String> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read permissions file {}: {e}", path.display()))?;
    let file: PermissionsFile = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse permissions file {}: {e}", path.display()))?;
    Ok(file
        .decisions
        .into_iter()
        .map(|(ext, decs)| {
            let by_cap = decs.into_iter().map(|d| (d.capability.clone(), d)).collect();
            (ext, by_cap)
        })
        .collect())
}

/// 原子落盘：临时文件 + 同目录 rename；扩展名/能力名按序输出（稳定序列化，
/// 便于 diff 与审计）。
fn save_decisions(
    path: &Path,
    decisions: &HashMap<String, HashMap<String, PersistedDecision>>,
) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create permissions dir: {e}"))?;

    let mut ext_ids: Vec<&String> = decisions.keys().collect();
    ext_ids.sort();
    let file = PermissionsFile {
        version: CURRENT_VERSION,
        decisions: ext_ids
            .into_iter()
            .map(|ext| {
                let mut decs: Vec<PersistedDecision> =
                    decisions[ext].values().cloned().collect();
                decs.sort_by(|a, b| a.capability.cmp(&b.capability));
                (ext.clone(), decs)
            })
            .collect(),
    };
    let mut contents = serde_json::to_string_pretty(&file)
        .map_err(|e| format!("Serialize permissions: {e}"))?;
    contents.push('\n');

    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("Create permissions temp file: {e}"))?;
    tmp.write_all(contents.as_bytes())
        .map_err(|e| format!("Write permissions temp file: {e}"))?;
    tmp.persist(path)
        .map_err(|e| format!("Persist permissions file to {}: {}", path.display(), e.error))?;
    Ok(())
}

fn now_iso8601() -> String {
    // 与 legacy 同构的 YYYY-MM-DDThh:mm:ssZ（无 chrono 依赖）。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
        .as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// days-since-epoch → (year, month, day)（Howard Hinnant 算法，saturating 防溢出）。
const fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days.saturating_add(719_468);
    let era = z / 146_097;
    let doe = z.saturating_sub(era.saturating_mul(146_097));
    let yoe = doe
        .saturating_sub(doe / 1460)
        .saturating_add(doe / 36524)
        .saturating_sub(doe / 146_096)
        / 365;
    let y = yoe.saturating_add(era.saturating_mul(400));
    let doy = doe.saturating_sub(yoe.saturating_mul(365)).saturating_sub(yoe / 4)
        .saturating_add(yoe / 100);
    let mp = doy.saturating_mul(5).saturating_add(2) / 153;
    let d = doy.saturating_sub((mp.saturating_mul(153).saturating_add(2)) / 5).saturating_add(1);
    let m = if mp < 10 { mp.saturating_add(3) } else { mp.saturating_sub(9) };
    let y = if m <= 2 { y.saturating_add(1) } else { y };
    (y, m, d)
}

/// 过期判定：`expires_at` 为空 = 永不过期；否则按 RFC3339 解析与当前时刻比较。
fn decision_is_active(dec: &PersistedDecision) -> bool {
    let Some(expires_at) = dec.expires_at.as_deref() else {
        return true;
    };
    let Some(expiry_secs) = parse_rfc3339_to_secs(expires_at) else {
        // 无法解析的过期时间按"无记忆"处理（询问链兜底，对齐 legacy）
        return false;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
        .as_secs();
    expiry_secs > now
}

/// 极简 RFC3339 解析（支持时区偏移，如 `2026-01-01T00:30:00+01:00`）。
fn parse_rfc3339_to_secs(s: &str) -> Option<u64> {
    let bytes = s.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u64> {
        std::str::from_utf8(&bytes[r]).ok()?.parse().ok()
    };
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let days = ymd_to_days(year, month, day)?;
    let mut secs = days
        .checked_mul(86_400)?
        .checked_add(hour.checked_mul(3600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)?;
    // 时区后缀：Z 或 ±hh:mm（偏移从 UTC 减去）
    let rest = &bytes[19..];
    if rest != b"Z" && rest.len() >= 6 {
        let sign: i64 = if rest[0] == b'-' { 1 } else if rest[0] == b'+' { -1 } else { return None };
        let off_h: i64 = std::str::from_utf8(&rest[1..3]).ok()?.parse().ok()?;
        let off_m: i64 = std::str::from_utf8(&rest[4..6]).ok()?.parse().ok()?;
        secs = secs.checked_add_signed(sign * (off_h * 3600 + off_m * 60))?;
    }
    Some(secs)
}

/// (year, month, day) → days since epoch（不校验 1970 之前）。
fn ymd_to_days(year: u64, month: u64, day: u64) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1970 {
        return None;
    }
    let y = year - 1970;
    let mut days = y
        .checked_mul(365)?
        .checked_add((y + 1) / 4)?
        .checked_sub((y + 69) / 100)?
        .checked_add((y + 369) / 400)?;
    const CUM: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    days = days.checked_add(CUM[(month - 1) as usize])?.checked_add(day - 1)?;
    if month > 2 && is_leap(year) {
        days = days.checked_add(1)?;
    }
    Some(days)
}

const fn is_leap(year: u64) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(path: &Path) -> PermissionStore {
        PermissionStore::open(path).expect("open")
    }

    #[test]
    fn record_lookup_roundtrip_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extension-permissions.json");
        {
            let mut s = store(&path);
            assert_eq!(s.lookup("web-search", "http"), None);
            s.record("web-search", "http", true).unwrap();
            s.record("web-search", "exec", false).unwrap();
        }
        // 重开读回（跨"会话"生效）
        let s = store(&path);
        assert_eq!(s.lookup("web-search", "http"), Some(true));
        assert_eq!(s.lookup("web-search", "exec"), Some(false));
        assert_eq!(s.lookup("other", "http"), None);
    }

    #[test]
    fn overwrite_replaces_decision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extension-permissions.json");
        let mut s = store(&path);
        s.record("ext", "exec", true).unwrap();
        s.record("ext", "exec", false).unwrap();
        assert_eq!(s.lookup("ext", "exec"), Some(false));
    }

    #[test]
    fn file_format_matches_legacy_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extension-permissions.json");
        let mut s = store(&path);
        s.record("ext-a", "http", true).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["decisions"]["ext-a"][0]["capability"], "http");
        assert_eq!(json["decisions"]["ext-a"][0]["allow"], true);
        assert!(json["decisions"]["ext-a"][0]["decided_at"].is_string());
        // 无 expires_at/version_range 时不得序列化（对齐 legacy skip_serializing_if）
        assert!(json["decisions"]["ext-a"][0].get("expires_at").is_none());
    }

    #[test]
    fn legacy_written_file_is_readable() {
        // 手工构造 legacy 格式文件（ISO-8601 decided_at + 数组 decisions）
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extension-permissions.json");
        std::fs::write(
            &path,
            r#"{"version":1,"decisions":{"legacy-ext":[{"capability":"http","allow":true,"decided_at":"2026-08-14T10:00:00Z"}]}}"#,
        )
        .unwrap();
        let s = store(&path);
        assert_eq!(s.lookup("legacy-ext", "http"), Some(true));
    }

    #[test]
    fn expired_decision_treated_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extension-permissions.json");
        std::fs::write(
            &path,
            r#"{"version":1,"decisions":{"ext":[{"capability":"http","allow":true,"decided_at":"2020-01-01T00:00:00Z","expires_at":"2020-06-01T00:00:00Z"}]}}"#,
        )
        .unwrap();
        let s = store(&path);
        assert_eq!(s.lookup("ext", "http"), None, "过期决策应按无记忆处理");
    }

    #[test]
    fn corrupt_file_fails_open_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extension-permissions.json");
        std::fs::write(&path, "not json!!!").unwrap();
        assert!(PermissionStore::open(&path).is_err());
    }

    #[test]
    fn now_iso8601_is_lexicographic_timestamp() {
        let a = now_iso8601();
        let b = now_iso8601();
        assert_eq!(a.len(), 20);
        assert!(a.ends_with('Z'));
        assert!(b >= a);
    }

    #[test]
    fn days_to_ymd_known_dates() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        assert_eq!(days_to_ymd(10957), (2000, 1, 1));
        assert_eq!(days_to_ymd(11016), (2000, 2, 29), "2000-02-29");
    }

    #[test]
    fn rfc3339_parse_handles_offsets() {
        // 2026-01-01T00:30:00+01:00 == 2026-01-01T00:00:00Z + 30min == 前一日 23:30Z
        let a = parse_rfc3339_to_secs("2026-01-01T00:30:00+01:00").unwrap();
        let b = parse_rfc3339_to_secs("2025-12-31T23:30:00Z").unwrap();
        assert_eq!(a, b);
        assert!(parse_rfc3339_to_secs("not-a-timestamp").is_none());
    }
}
