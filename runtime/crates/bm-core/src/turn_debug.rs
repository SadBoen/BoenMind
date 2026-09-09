//! W5+#14 Turn 内调试日志:运行时开关(默认关)控制的 JSONL 追加写,
//! 落 `<数据目录>/turn-debug.jsonl`——**独立于 context-log.jsonl**(对话正文
//! 唯一落盘,A4 决策),携带比生产级日志更厚的回合细节(模型响应原文/
//! 工具全量出入参/回合终态),排查单次坏回合免临时插桩。
//! 形态与 context_log 同款(Arc 句柄 + Mutex + 内存镜像供测试);脱敏沿
//! INV-5 同一面:凭据明文两侧同注册,命中即 [REDACTED]。开关为进程内
//! AtomicBool(管理面热切换),关 = 记录零成本跳过;单条 payload 截断
//! 64K 字符(防失控回合拖爆磁盘)。

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// 单条 payload 序列化后的截断上限(字符)。
const PAYLOAD_CAP_CHARS: usize = 64_000;
/// 内存镜像上限(测试与 /admin/debug/turns 尾读备用面)。
const MIRROR_CAP: usize = 500;

pub struct TurnDebugLog {
    path: Option<PathBuf>,
    enabled: AtomicBool,
    inner: Mutex<TurnDebugInner>,
}

struct TurnDebugInner {
    file: Option<std::fs::File>,
    scan_values: BTreeSet<String>,
    mirror: Vec<serde_json::Value>,
}

impl TurnDebugLog {
    /// `dir = None` 时仅在内存镜像记账(纯测试);文件在首条记录时懒创建。
    pub fn new(dir: Option<&Path>) -> Self {
        Self {
            path: dir.map(|d| d.join("turn-debug.jsonl")),
            enabled: AtomicBool::new(false),
            inner: Mutex::new(TurnDebugInner {
                file: None,
                scan_values: BTreeSet::new(),
                mirror: Vec::new(),
            }),
        }
    }

    /// 管理面开关(热生效;spawn 侧每条记录前读)。
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// INV-5 扫描面注册(与 ContextLog/ExecutionLog 同批登记)。
    pub fn register_scan_value(&self, value: &str) {
        if value.len() >= 6 {
            let mut inner = self.inner.lock().expect("锁未中毒");
            inner.scan_values.insert(value.to_string());
            if let Ok(esc) = serde_json::to_string(value) {
                let trimmed = esc.trim_matches('"').to_string();
                if trimmed != value {
                    inner.scan_values.insert(trimmed);
                }
            }
        }
    }

    /// 记录一条调试事件:关 = 直接跳过;开 = 脱敏 → 文件追加 + 内存镜像。
    pub fn record(
        &self,
        kind: &str,
        session_id: &str,
        agent_id: &str,
        operation_id: &str,
        mut payload: serde_json::Value,
    ) {
        if !self.enabled() {
            return;
        }
        let line = serde_json::json!({
            "ts_ms": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            "kind": kind,
            "session_id": session_id,
            "agent_id": agent_id,
            "operation_id": operation_id,
            "payload": payload,
        });
        let mut text = serde_json::to_string(&line).unwrap_or_else(|_| "{}".to_string());
        if text.len() > PAYLOAD_CAP_CHARS {
            text.truncate(PAYLOAD_CAP_CHARS);
            text.push_str("\"}");
        }
        let mut inner = self.inner.lock().expect("锁未中毒");
        for secret in &inner.scan_values {
            if text.contains(secret.as_str()) {
                text = text.replace(secret.as_str(), "[REDACTED]");
            }
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            inner.mirror.push(v);
            if inner.mirror.len() > MIRROR_CAP {
                inner.mirror.remove(0);
            }
        }
        if let Some(path) = &self.path {
            let file = inner.file.get_or_insert_with(|| {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .expect("turn-debug.jsonl 打开失败")
            });
            let _ = writeln!(file, "{text}");
        }
        // 静默载荷标记:吞掉借用,避免调用方为 move 语义额外 clone
        let _ = &mut payload;
    }

    /// 尾读(旧→新):内存镜像(与文件内容一致)。
    pub fn tail(&self, n: usize) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("锁未中毒");
        let skip = inner.mirror.len().saturating_sub(n);
        inner.mirror.iter().skip(skip).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_by_default_and_records_nothing() {
        let log = TurnDebugLog::new(None);
        assert!(!log.enabled());
        log.record(
            "model_response",
            "s",
            "a",
            "op",
            serde_json::json!({"x": 1}),
        );
        assert!(log.tail(10).is_empty());
    }

    #[test]
    fn enabled_records_into_mirror_and_redacts_secrets() {
        let log = TurnDebugLog::new(None);
        log.set_enabled(true);
        log.register_scan_value("sk-super-secret-key");
        log.record(
            "tool_result",
            "sess1",
            "agent1",
            "op1",
            serde_json::json!({"result": "key=sk-super-secret-key end"}),
        );
        let tail = log.tail(10);
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0]["kind"], "tool_result");
        let text = tail[0].to_string();
        assert!(
            !text.contains("sk-super-secret-key"),
            "明文必须被脱敏: {text}"
        );
        assert!(text.contains("[REDACTED]"));
    }

    #[test]
    fn writes_file_when_dir_given() {
        let dir = tempfile::tempdir().expect("tmp");
        let log = TurnDebugLog::new(Some(dir.path()));
        log.set_enabled(true);
        log.record("turn_end", "s", "a", "op", serde_json::json!({"rounds": 2}));
        drop(log);
        let raw = std::fs::read_to_string(dir.path().join("turn-debug.jsonl")).unwrap();
        assert!(raw.contains("\"turn_end\""));
        assert!(raw.contains("\"rounds\":2"));
    }
}
