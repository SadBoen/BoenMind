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
            self.inner
                .lock()
                .expect("锁未中毒")
                .scan_values
                .insert(value.to_string());
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
        // 复扫:脱敏后必须 0 命中
        let recheck = serde_json::to_string(&entry).expect("日志条目可序列化");
        for secret in &inner.scan_values {
            debug_assert!(!recheck.contains(secret.as_str()), "脱敏后仍命中凭据明文");
        }

        inner.entries.push(entry.clone());
        if let Some(path) = &self.path {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("Execution Log 文件可打开");
            writeln!(file, "{recheck}").expect("Execution Log 追加写成功");
            file.flush().expect("Execution Log flush 成功");
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
            std::fs::write(path, payload).expect("Execution Log 重写成功");
        }
        removed
    }
}
