//! 运行日志查看(2026-09-02 用户要求「设置里接入日志」)。

use super::AdminConfig;
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use std::path::Path;

/// 从文件尾部读最多 `max_bytes` 字节,返回最后 `n` 行(首行可能被截断则丢弃)。
fn tail_lines(path: &Path, max_bytes: u64, n: usize) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return vec![];
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        return vec![];
    }
    let mut lines: Vec<&str> = buf.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // 截断边界上的半行不可信
    }
    let skip = lines.len().saturating_sub(n);
    lines.into_iter().skip(skip).map(String::from).collect()
}

/// GET /admin/logs:数据目录三份日志的尾部直读(各取最后 200 行,单文件
/// 最多回读 512KB)——execution-log.jsonl(回合/工具调用明细)、
/// events.jsonl(事件流,含 capability.invoked 的 intent/result/error)与
/// context-log.jsonl(W5 上下文快照原文,调试用),供诊断「工具调用卡死」
/// 一类运行期问题。
pub async fn logs_tail(State(cfg): State<AdminConfig>) -> Response {
    let dir = cfg.data_dir;
    Json(json!({
        "ok": true,
        "exec": tail_lines(&dir.join("execution-log.jsonl"), 512 * 1024, 200),
        "events": tail_lines(&dir.join("events.jsonl"), 512 * 1024, 200),
        "context": tail_lines(&dir.join("context-log.jsonl"), 512 * 1024, 200),
    }))
    .into_response()
}
