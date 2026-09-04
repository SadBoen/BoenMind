//! W5 上下文透视:每次模型调用的请求快照(messages+tools)与结果(usage/
//! 耗时/成败)追加写 `<数据目录>/context-log.jsonl`,供 /admin/context 与
//! 前端「上下文」页直读——回答「这次到底发给了模型什么」的诊断刚需。
//! 形态与 ExecutionLog 同款(Arc 句柄 + Mutex + 内存镜像供测试);脱敏沿
//! INV-5 同一面:凭据明文两侧同注册,命中即 [REDACTED]。管理面端点暂不
//! 入冻结合同(webadmin.rs 模块注释同口径)。单条消息内容截断 16K 字符
//! (与 model.content.completed 事件截断口径一致,防快照文件膨胀)。

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 单条消息内容入快照的截断上限(字符;与事件面 16KB 口径同量级)。
const SNAPSHOT_CONTENT_CAP_CHARS: usize = 16_000;

/// 一次模型调用的快照记录(调用结束落一行;status/error/usage 为结果侧)。
pub struct ContextRecord {
    pub session_id: String,
    pub agent_id: String,
    pub operation_id: String,
    /// 回合序号(operation.turn_index;同回合多步共享)。
    pub turn_index: u32,
    /// 本回合内第几次模型调用(1 起;工具轮结果回喂后重调即 +1)。
    pub step: u32,
    /// 降级链尝试序号(1 起;区分重试间的同序号步骤)。
    pub attempt: u32,
    pub model_id: String,
    pub streaming: bool,
    /// 请求消息序列([{role, content, content_truncated}];快照时点原样)。
    pub messages: Vec<serde_json::Value>,
    /// OpenAI function 工具定义(随请求原样)。
    pub tools: Vec<serde_json::Value>,
    /// ok | error | cancelled
    pub status: &'static str,
    pub error_code: Option<String>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    /// 推理思考消耗(提供商如实上报;不报 = None,前端显示「未上报」)。
    pub tokens_reasoning: Option<u64>,
    /// 提示词缓存命中(提供商如实上报;不报 = None)。
    pub tokens_cached: Option<u64>,
    /// 流式首包延迟(请求发出→首个增量到达;非流式调用无从测量 = None)。
    pub ttft_ms: Option<u64>,
    /// 组装本次请求时已被台账双上限丢弃的历史轮数(0 = 无遗忘)。
    pub evicted_turns: Option<u64>,
    pub latency_ms: Option<u64>,
    pub ts: String,
}

pub struct ContextLog {
    path: Option<PathBuf>,
    inner: Mutex<Inner>,
}

struct Inner {
    next_seq: u64,
    /// INV-5 扫描面:本进程经手的凭据明文(与 ExecutionLog 同批登记)。
    scan_values: BTreeSet<String>,
    /// 内存镜像(测试断言用)。
    entries: Vec<serde_json::Value>,
}

/// 单条消息内容截断(字符口径;截断标记如实)。
fn snap_content(s: &str) -> (String, bool) {
    if s.chars().count() > SNAPSHOT_CONTENT_CAP_CHARS {
        (s.chars().take(SNAPSHOT_CONTENT_CAP_CHARS).collect(), true)
    } else {
        (s.to_string(), false)
    }
}

/// 消息序列入快照形状([{role, content, content_truncated}])。
pub fn snapshot_messages(messages: &[bm_contract::connector::Message]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|m| {
            let (content, truncated) = snap_content(&m.content);
            serde_json::json!({
                "role": m.role.as_str(),
                "content": content,
                "content_truncated": truncated,
            })
        })
        .collect()
}

impl ContextLog {
    /// `dir = None` 时仅内存记账(纯事件流测试)。
    pub fn new(dir: Option<&Path>) -> Self {
        Self {
            path: dir.map(|d| d.join("context-log.jsonl")),
            inner: Mutex::new(Inner {
                next_seq: 1,
                scan_values: BTreeSet::new(),
                entries: Vec::new(),
            }),
        }
    }

    /// 注册凭据明文进扫描面(Secret Store put/get 后调用,与执行日志同批)。
    pub fn register_scan_value(&self, value: &str) {
        if value.len() >= 6 {
            // 过短的值误报率高,不进扫描面(ExecutionLog 同口径)
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

    /// 记录一次模型调用快照:扫描→脱敏→落盘。失败静默降级(诊断面
    /// 不反压业务回合);返回分配的 seq。
    pub fn record(&self, rec: ContextRecord) -> u64 {
        let mut inner = self.inner.lock().expect("锁未中毒");
        let seq = inner.next_seq;
        inner.next_seq += 1;
        let value = serde_json::json!({
            "seq": seq,
            "ts": rec.ts,
            "session_id": rec.session_id,
            "agent_id": rec.agent_id,
            "operation_id": rec.operation_id,
            "turn_index": rec.turn_index,
            "step": rec.step,
            "attempt": rec.attempt,
            "model_id": rec.model_id,
            "streaming": rec.streaming,
            "messages": rec.messages,
            "tools": rec.tools,
            "status": rec.status,
            "error_code": rec.error_code,
            "tokens_in": rec.tokens_in,
            "tokens_out": rec.tokens_out,
            "tokens_reasoning": rec.tokens_reasoning,
            "tokens_cached": rec.tokens_cached,
            "ttft_ms": rec.ttft_ms,
            "evicted_turns": rec.evicted_turns,
            "latency_ms": rec.latency_ms,
        });
        // INV-5 同面:对整条序列化结果做明文扫描,命中即替换(写脱敏后的串)
        let mut serialized = serde_json::to_string(&value).unwrap_or_default();
        for secret in &inner.scan_values {
            if serialized.contains(secret.as_str()) {
                serialized = serialized.replace(secret.as_str(), "[REDACTED]");
            }
        }
        if let Some(p) = &self.path
            && let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p)
        {
            let _ = writeln!(f, "{serialized}");
            let _ = f.flush();
        }
        inner
            .entries
            .push(serde_json::from_str(&serialized).unwrap_or(value));
        seq
    }

    /// W9 逐轮事件(tool_call/tool_result/assistant_final/turn_end,规格
    /// milestones/W9-context-trajectory-spec.md):与模型调用快照同一 jsonl
    /// 流,`kind` 字段区分(快照行无 kind,既有读取面不受影响)。脱敏与
    /// 静默降级同 record。返回分配的 seq。
    pub fn record_event(
        &self,
        session_id: &str,
        operation_id: &str,
        turn_index: u32,
        kind: &str,
        ts: &str,
        mut data: serde_json::Value,
    ) -> u64 {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("kind".into(), serde_json::Value::String(kind.to_string()));
        }
        let mut inner = self.inner.lock().expect("锁未中毒");
        let seq = inner.next_seq;
        inner.next_seq += 1;
        let value = serde_json::json!({
            "seq": seq,
            "ts": ts,
            "session_id": session_id,
            "operation_id": operation_id,
            "turn_index": turn_index,
            "kind": kind,
            "data": data,
        });
        let mut serialized = serde_json::to_string(&value).unwrap_or_default();
        for secret in &inner.scan_values {
            if serialized.contains(secret.as_str()) {
                serialized = serialized.replace(secret.as_str(), "[REDACTED]");
            }
        }
        if let Some(p) = &self.path
            && let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p)
        {
            let _ = writeln!(f, "{serialized}");
            let _ = f.flush();
        }
        inner
            .entries
            .push(serde_json::from_str(&serialized).unwrap_or(value));
        seq
    }

    /// 内存镜像尾部(测试断言用;新→旧次序与文件一致,即最旧在前)。
    pub fn tail(&self, n: usize) -> Vec<serde_json::Value> {
        let inner = self.inner.lock().expect("锁未中毒");
        let start = inner.entries.len().saturating_sub(n);
        inner.entries[start..].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_contract::connector::{Message, Role};

    fn rec(messages: Vec<serde_json::Value>) -> ContextRecord {
        ContextRecord {
            session_id: "s".into(),
            agent_id: "a".into(),
            operation_id: "op".into(),
            turn_index: 0,
            step: 1,
            model_id: "mock.model".into(),
            streaming: false,
            messages,
            tools: vec![],
            status: "ok",
            error_code: None,
            tokens_in: Some(412),
            tokens_out: Some(58),
            tokens_reasoning: Some(20),
            tokens_cached: Some(300),
            ttft_ms: Some(320),
            evicted_turns: Some(0),
            latency_ms: Some(1873),
            attempt: 1,
            ts: "2026-09-02T00:00:00Z".into(),
        }
    }

    #[test]
    fn snapshot_truncates_long_content_with_flag() {
        let long = "x".repeat(SNAPSHOT_CONTENT_CAP_CHARS + 10);
        let msgs = vec![Message {
            role: Role::User,
            content: long,
        }];
        let snap = snapshot_messages(&msgs);
        assert_eq!(snap[0]["content_truncated"], serde_json::json!(true));
        assert_eq!(
            snap[0]["content"].as_str().unwrap().chars().count(),
            SNAPSHOT_CONTENT_CAP_CHARS
        );
        let short = vec![Message {
            role: Role::User,
            content: "你好".into(),
        }];
        let snap = snapshot_messages(&short);
        assert_eq!(snap[0]["content_truncated"], serde_json::json!(false));
        assert_eq!(snap[0]["role"], serde_json::json!("user"));
    }

    #[test]
    fn memory_only_dir_none_and_redaction_hits() {
        let log = ContextLog::new(None);
        log.register_scan_value("sk-very-secret-value");
        let msgs = vec![Message {
            role: Role::User,
            content: "我的 key 是 sk-very-secret-value 别外传".into(),
        }];
        log.record(rec(snapshot_messages(&msgs)));
        let tail = log.tail(10);
        let raw = serde_json::to_string(&tail).unwrap();
        assert!(!raw.contains("sk-very-secret-value"), "明文不得落快照");
        assert!(raw.contains("[REDACTED]"));
        assert_eq!(tail[0]["tokens_in"], serde_json::json!(412));
        assert_eq!(tail[0]["tokens_reasoning"], serde_json::json!(20));
        assert_eq!(tail[0]["tokens_cached"], serde_json::json!(300));
        assert_eq!(tail[0]["ttft_ms"], serde_json::json!(320));
        assert_eq!(tail[0]["evicted_turns"], serde_json::json!(0));
        assert_eq!(tail[0]["status"], serde_json::json!("ok"));
    }

    #[test]
    fn writes_appends_to_file() {
        let dir = std::env::temp_dir().join(format!("bm-ctx-log-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = ContextLog::new(Some(&dir));
        log.record(rec(vec![]));
        log.record(rec(vec![]));
        let raw = std::fs::read_to_string(dir.join("context-log.jsonl")).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["seq"], serde_json::json!(1));
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["seq"], serde_json::json!(2));
        std::fs::remove_dir_all(&dir).ok();
    }
}
