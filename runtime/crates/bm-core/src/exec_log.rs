//! Execution Log:JSONL 追加写,log_seq 由写者单调分配(合同:仅在本日志内排序)。
//! 每条落盘前执行脱敏扫描(INV-5):命中凭据明文的片段替换为 [REDACTED] 后
//! 才允许写盘,secret_scan 恒为 passed——未通过扫描的条目禁止落盘。

use bm_contract::BmTimestamp;
use bm_contract::exec_log::{LogEntry, SecretScan};
use bm_contract::ids::BmId;
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 一条待记录的日志(不含 log_seq/secret_scan,由写者分配/检查)。
pub struct LogRecord {
    pub kind: bm_contract::exec_log::LogKind,
    pub session_id: BmId,
    pub agent_id: BmId,
    pub operation_id: BmId,
    pub request_id: Option<BmId>,
    pub agent_state: String,
    pub detail: serde_json::Value,
    pub ts: BmTimestamp,
}

pub struct ExecutionLog {
    path: Option<PathBuf>,
    inner: Mutex<Inner>,
}

struct Inner {
    next_seq: u64,
    /// INV-5 扫描面:本进程经手的凭据明文(来自 Secret Store)。
    scan_values: BTreeSet<String>,
    /// 内存镜像(测试与泄漏扫描断言用)。
    entries: Vec<LogEntry>,
}

impl ExecutionLog {
    /// `dir = None` 时仅在内存中记账(纯事件流测试)。
    pub fn new(dir: Option<&Path>) -> Self {
        Self {
            path: dir.map(|d| d.join("execution-log.jsonl")),
            inner: Mutex::new(Inner {
                next_seq: 1,
                scan_values: BTreeSet::new(),
                entries: Vec::new(),
            }),
        }
    }

    /// 注册凭据明文进扫描面(Secret Store put/get 后调用)。
    pub fn register_scan_value(&self, value: &str) {
        if value.len() >= 6 {
            // 过短的值误报率高,不进扫描面
            let mut inner = self.inner.lock().expect("锁未中毒");
            inner.scan_values.insert(value.to_string());
            // P0(第四轮评审):序列化后凭据里的 "\|\" 等字符会转义,纯明文
            // contains 永不命中——同步注册 JSON 转义形态。
            if let Ok(esc) = serde_json::to_string(value) {
                let trimmed = esc.trim_matches('"').to_string();
                if trimmed != value {
                    inner.scan_values.insert(trimmed);
                }
            }
        }
    }

    /// 记录一条日志:扫描→脱敏→落盘。返回分配的 log_seq。
    pub fn record(&self, rec: LogRecord) -> u64 {
        let mut inner = self.inner.lock().expect("锁未中毒");
        let log_seq = inner.next_seq;
        inner.next_seq += 1;

        let mut entry = LogEntry {
            log_seq,
            ts: rec.ts,
            kind: rec.kind,
            session_id: rec.session_id,
            agent_id: rec.agent_id,
            operation_id: rec.operation_id,
            request_id: rec.request_id,
            state: rec.agent_state.to_string(),
            secret_scan: Some(SecretScan::Passed),
            detail: rec.detail,
        };

        // INV-5:对整条序列化结果做明文扫描,命中即脱敏。
        let mut serialized = serde_json::to_string(&entry).expect("日志条目可序列化");
        for secret in &inner.scan_values {
            if serialized.contains(secret.as_str()) {
                serialized = serialized.replace(secret.as_str(), "[REDACTED]");
                entry.detail = serde_json::from_str(&serialized).unwrap_or(entry.detail);
            }
        }
        // 复扫:脱敏后必须 0 命中。P0(第四轮评审)修复:原先 debug_assert
        // 在 release 不执行,fail-open;现改为 release 级 fail-closed——
        // 仍命中则整条降格为占位(禁止明文落盘),扫描态如实记 failed。
        let recheck = serde_json::to_string(&entry).expect("日志条目可序列化");
        let mut hit = false;
        for secret in &inner.scan_values {
            if recheck.contains(secret.as_str()) {
                hit = true;
                break;
            }
        }
        let line = if hit {
            entry.secret_scan = Some(SecretScan::Failed);
            entry.detail = serde_json::json!({
                "redaction_failed": true,
                "kind": entry.kind.as_str(),
                "note": "脱敏复扫未通过,原文禁止落盘(INV-5)"
            });
            serde_json::to_string(&entry).expect("占位条目可序列化")
        } else {
            recheck
        };

        inner.entries.push(entry.clone());
        if let Some(path) = &self.path {
            // F-01(审计台账)修复:磁盘满/权限/文件占用等外部条件不得 panic
            // 整个 runtime——写失败降级为 stderr 告警,内存镜像仍完整(诊断
            // 面少一行日志好过进程死亡);flush 错误同口径。
            match OpenOptions::new().create(true).append(true).open(path) {
                Ok(mut file) => {
                    if let Err(e) = writeln!(file, "{line}") {
                        eprintln!("[exec-log] 追加写失败(该条仅存内存镜像): {e}");
                    } else if let Err(e) = file.flush() {
                        eprintln!("[exec-log] flush 失败(该条仅存内存镜像): {e}");
                    }
                }
                Err(e) => {
                    eprintln!("[exec-log] 日志文件打开失败(该条仅存内存镜像): {e}");
                }
            }
        }
        log_seq
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        self.inner.lock().expect("锁未中毒").entries.clone()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("锁未中毒").entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ExecutionLog {
    /// M8.8:保留期修剪——删除 ts < cutoff 的条目并重写日志文件。
    /// cutoff 为 ISO-8601 UTC(合同形态,字典序即时间序);返回删除条数。
    /// 仅修剪 execution log(过程面);事件日志为审计本体,永不修剪。
    pub fn prune_before(&self, cutoff: &str) -> usize {
        let mut inner = self.inner.lock().expect("锁未中毒");
        let before = inner.entries.len();
        inner.entries.retain(|e| e.ts.as_str() >= cutoff);
        let removed = before - inner.entries.len();
        if removed > 0
            && let Some(path) = &self.path
        {
            let out = inner
                .entries
                .iter()
                .map(|e| serde_json::to_string(e).expect("日志条目可序列化"))
                .collect::<Vec<_>>()
                .join(
                    "
",
                );
            let payload = if out.is_empty() {
                String::new()
            } else {
                out + "
"
            };
            // F-01 同口径:重写失败不 panic(原文件保持未修剪态,下轮重试)
            if let Err(e) = std::fs::write(path, payload) {
                eprintln!("[exec-log] 修剪重写失败(内存镜像已修剪,文件待下轮): {e}");
            }
        }
        removed
    }
}

#[cfg(test)]
mod m9_review_tests {
    use super::*;
    use bm_contract::exec_log::LogKind;

    /// P0(第四轮评审)验收:含引号/反斜杠的凭据,其 JSON 转义形态也必须
    /// 被脱敏(此前纯明文 contains 对转义形态永不命中)。
    #[test]
    fn redaction_covers_json_escaped_secret_forms() {
        let log = ExecutionLog::new(None);
        let secret = "sk-t\"quote\\back-123456";
        log.register_scan_value(secret);
        log.record(LogRecord {
            kind: LogKind::Error,
            session_id: BmId::parse("sess_00000000000000000000000001").unwrap(),
            agent_id: BmId::parse("agent_00000000000000000000000002").unwrap(),
            operation_id: BmId::parse("op_00000000000000000000000003").unwrap(),
            request_id: Some(BmId::parse("req_00000000000000000000000004").unwrap()),
            agent_state: "running".into(),
            detail: serde_json::json!({ "note": format!("用户输入回显 {secret}") }),
            ts: "2026-08-30T00:00:00.000Z".into(),
        });
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        let serialized = serde_json::to_string(&entries[0]).unwrap();
        assert!(
            !serialized.contains("sk-t"),
            "凭据明文(含转义形态)禁止出现在日志:{serialized}"
        );
        assert!(serialized.contains("[REDACTED]"), "命中应替换为 REDACTED");
    }

    /// SecretScan::Failed 可序列化为合同值 "failed"(fail-closed 降格态)。
    #[test]
    fn secret_scan_failed_wire_value() {
        assert_eq!(
            serde_json::to_string(&SecretScan::Failed).unwrap(),
            "\"failed\""
        );
    }

    /// F-10(审计缺口)验收:复扫仍命中 → 整条降格为占位 + SecretScan::Failed
    /// (fail-closed 分支此前无测试)。触发构造:凭据恰为脱敏标记本身——
    /// 首轮替换是自替换(值不变),复扫必再命中,天然进入降格分支。
    #[test]
    fn recheck_hit_downgrades_entry_to_failed_placeholder() {
        let log = ExecutionLog::new(None);
        let marker_secret = "[REDACTED]";
        log.register_scan_value(marker_secret);
        log.record(LogRecord {
            kind: LogKind::Error,
            session_id: BmId::parse("sess_00000000000000000000000001").unwrap(),
            agent_id: BmId::parse("agent_00000000000000000000000002").unwrap(),
            operation_id: BmId::parse("op_00000000000000000000000003").unwrap(),
            request_id: None,
            agent_state: "running".into(),
            detail: serde_json::json!({ "leak": format!("包含凭据形标记 {marker_secret}") }),
            ts: "2026-08-30T00:00:00.000Z".into(),
        });
        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].secret_scan,
            Some(SecretScan::Failed),
            "复扫未通过必须降格为 failed"
        );
        let serialized = serde_json::to_string(&entries[0]).unwrap();
        assert!(
            !serialized.contains(marker_secret),
            "降格占位不得再含命中片段:{serialized}"
        );
        assert!(serialized.contains("redaction_failed"), "应有降格标记");
        // 内存镜像同样不含明文
        let mirrored = serde_json::to_string(&log.entries()[0].detail).unwrap();
        assert!(!mirrored.contains(marker_secret));
    }
}
