//! 内存环形日志缓冲：tracing Layer 收集最近 N 条日志，设置中心「日志」页经
//! `/api/logs` 查看与筛选。
//!
//! 设计：只保留最近若干条（默认 5000），进程内查询；不做落盘——便携版形态
//! 下 bm-server 可能以子进程/内嵌线程运行，落盘位置与轮转归日志文件策略
//! （本轮不引入）。级别过滤依赖调用方 EnvFilter（main.rs 已有），本缓冲
//! 只收集已通过过滤的事件。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{Event, Subscriber, field::Visit};
use tracing_subscriber::Layer;

/// 一条日志（/api/logs 的返回单元）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    /// Unix 毫秒时间戳
    pub ts_ms: i64,
    pub level: String,
    /// 来源模块（tracing target）
    pub target: String,
    /// 格式化消息（message 字段 + 其余字段 key=value 拼接）
    pub message: String,
}

/// 环形缓冲（容量上限，旧日志从头弹出）。
pub struct LogBuffer {
    inner: Mutex<VecDeque<LogEntry>>,
    capacity: usize,
}

/// 进程级单例：main.rs 初始化 tracing 时安装；/api/logs 从这里读。
/// 无缓冲也可用（内嵌壳模式未初始化 tracing → 返回空列表，不报错）。
static LOG_BUFFER: OnceLock<Arc<LogBuffer>> = OnceLock::new();

/// 默认容量
const DEFAULT_CAPACITY: usize = 5000;

impl LogBuffer {
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
        })
    }

    /// 获取/创建进程级单例（供 main.rs 与路由共用）。
    pub fn install() -> Arc<Self> {
        LOG_BUFFER
            .get_or_init(|| Self::new(DEFAULT_CAPACITY))
            .clone()
    }

    /// 尝试取单例（未初始化返回 None——路由侧兜底，不强制安装）。
    pub fn global() -> Option<Arc<Self>> {
        LOG_BUFFER.get().cloned()
    }

    pub fn push(&self, entry: LogEntry) {
        let mut inner = self.inner.lock().unwrap();
        if inner.len() >= self.capacity {
            inner.pop_front();
        }
        inner.push_back(entry);
    }

    /// 查询：level = 最低级别（info → info/warn/error 都要）；q = 关键字
    /// （target/message 包含，大小写不敏感）；返回最新在前（按时间倒序）。
    pub fn query(
        &self,
        min_level: Option<&str>,
        q: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Vec<LogEntry> {
        let min = min_level.and_then(level_rank).unwrap_or(0);
        let needle = q.map(|s| s.to_lowercase());
        let inner = self.inner.lock().unwrap();
        inner
            .iter()
            .rev()
            .filter(|e| {
                level_rank(&e.level).unwrap_or(0) >= min
                    && needle.as_ref().is_none_or(|n| {
                        e.message.to_lowercase().contains(n) || e.target.to_lowercase().contains(n)
                    })
            })
            .skip(offset)
            .take(limit)
            .cloned()
            .collect()
    }
}

/// 级别数值（过滤比较用）：TRACE=0 … ERROR=4。未知级别按 0（不误杀）。
/// 大小写不敏感（查询参数来自前端小写，缓冲内是 tracing 大写）。
fn level_rank(level: &str) -> Option<u8> {
    match level.to_uppercase().as_str() {
        "TRACE" => Some(0),
        "DEBUG" => Some(1),
        "INFO" => Some(2),
        "WARN" => Some(3),
        "ERROR" => Some(4),
        _ => None,
    }
}

/// tracing Layer：on_event 时把事件收进缓冲。
pub struct BufferLayer {
    buffer: Arc<LogBuffer>,
}

impl BufferLayer {
    pub fn new(buffer: Arc<LogBuffer>) -> Self {
        Self { buffer }
    }
}

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let level = event.metadata().level().to_string();
        let target = event.metadata().target().to_string();
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let mut visitor = FieldFormatter::default();
        event.record(&mut visitor);
        self.buffer.push(LogEntry {
            ts_ms,
            level,
            target,
            message: visitor.render(),
        });
    }
}

/// 字段收集：message 字段 + 其余字段（`key=value`，值带引号）。
#[derive(Default)]
struct FieldFormatter {
    message: Option<String>,
    fields: Vec<String>,
}

impl FieldFormatter {
    fn render(&self) -> String {
        match &self.message {
            Some(m) if self.fields.is_empty() => m.clone(),
            Some(m) => format!("{m} {}", self.fields.join(" ")),
            None => self.fields.join(" "),
        }
    }
}

impl Visit for FieldFormatter {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.push(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.push(field, value.to_string());
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.push(field, value.to_string());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.push(field, value.to_string());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.push(field, value.to_string());
    }

    fn record_error(&mut self, field: &tracing::field::Field, value: &(dyn std::error::Error + 'static)) {
        self.push(field, format!("{value}"));
    }
}

impl FieldFormatter {
    fn push(&mut self, field: &tracing::field::Field, value: String) {
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            let name = field.name();
            let rendered = if name.is_empty() {
                value
            } else if value.contains(char::is_whitespace) {
                format!("{name}={value:?}")
            } else {
                format!("{name}={value}")
            };
            self.fields.push(rendered);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_caps_and_evicts() {
        let buf = LogBuffer::new(3);
        for i in 0..5 {
            buf.push(LogEntry {
                ts_ms: i,
                level: "INFO".into(),
                target: "test".into(),
                message: format!("msg {i}"),
            });
        }
        // 容量 3：最新 3 条（2,3,4），倒序
        let entries = buf.query(None, None, 10, 0);
        let msgs: Vec<_> = entries.iter().map(|e| e.message.clone()).collect();
        assert_eq!(msgs, vec!["msg 4", "msg 3", "msg 2"]);
    }

    #[test]
    fn query_filters_level_and_keyword() {
        let buf = LogBuffer::new(10);
        for (i, lvl) in ["INFO", "WARN", "ERROR", "DEBUG"].iter().enumerate() {
            buf.push(LogEntry {
                ts_ms: i as i64,
                level: (*lvl).into(),
                target: "t.module".into(),
                message: format!("hello {lvl}"),
            });
        }
        // 最低 WARN → 只剩 WARN/ERROR
        let entries = buf.query(Some("warn"), None, 10, 0);
        let levels: Vec<_> = entries.iter().map(|e| e.level.clone()).collect();
        assert_eq!(levels, vec!["ERROR", "WARN"]);
        // 关键字 hello + debug 级别（含 DEBUG）
        let entries = buf.query(Some("debug"), Some("hello"), 10, 0);
        assert_eq!(entries.len(), 4);
        // 关键字不存在的级别组合
        let entries = buf.query(Some("error"), Some("warn"), 10, 0);
        assert!(entries.is_empty());
        // 大小写不敏感
        let entries = buf.query(None, Some("HELLO"), 10, 0);
        assert_eq!(entries.len(), 4);
    }

    #[test]
    fn offset_and_limit() {
        let buf = LogBuffer::new(10);
        for i in 0..5 {
            buf.push(LogEntry {
                ts_ms: i,
                level: "INFO".into(),
                target: "t".into(),
                message: format!("m{i}"),
            });
        }
        let page1 = buf.query(None, None, 2, 0);
        let page2 = buf.query(None, None, 2, 2);
        assert_eq!(page1.len(), 2);
        assert_eq!(page2.len(), 2);
        assert_ne!(page1[0].ts_ms, page2[0].ts_ms);
    }
}
