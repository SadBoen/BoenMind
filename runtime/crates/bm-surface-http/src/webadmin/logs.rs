//! 运行日志查看(2026-09-02 用户要求「设置里接入日志」)。

use super::AdminConfig;
use super::tail::read_tail;
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use std::path::Path;

/// 从文件尾部读最多 `max_bytes` 字节,返回最后 `n` 行(首行可能被截断则丢弃)。
/// P2:尾读逻辑与 context.rs 收口为 tail::read_tail。
fn tail_lines(path: &Path, max_bytes: u64, n: usize) -> Vec<String> {
    let lines = read_tail(path, max_bytes);
    let skip = lines.len().saturating_sub(n);
    lines.into_iter().skip(skip).collect()
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

/// GET /admin/debug/turns:Turn 调试日志状态 + 尾读(issue #14)。
/// 开关态来自核心句柄(进程内 AtomicBool),行 = turn-debug.jsonl 尾部直读。
pub async fn debug_turns_get(State(cfg): State<AdminConfig>) -> Response {
    let enabled = cfg.handle.turn_debug_enabled();
    let lines = super::tail::read_tail(&cfg.data_dir.join("turn-debug.jsonl"), 512 * 1024);
    let skip = lines.len().saturating_sub(200);
    Json(json!({
        "ok": true,
        "enabled": enabled,
        "lines": lines.into_iter().skip(skip).collect::<Vec<_>>(),
    }))
    .into_response()
}

/// POST /admin/debug/turns {enabled}:热开关(issue #14)。默认关,开启后
/// 新回合的模型响应原文/工具全量出入参/回合终态写入 turn-debug.jsonl。
pub async fn debug_turns_set(
    State(cfg): State<AdminConfig>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(on) = body["enabled"].as_bool() else {
        return super::bad_request("enabled 必须是布尔值");
    };
    cfg.handle.set_turn_debug(on);
    Json(json!({ "ok": true, "enabled": on })).into_response()
}
