//! 日志尾读公共函数(P2,2026-09-07 架构评审:此前 logs.rs 与 context.rs
//! 各持一份 open→seek→去半行 逻辑,口径漂移风险)。

/// 从文件尾部读最多 `max_bytes` 字节,返回按文件序的行(截断边界上的半行
/// 丢弃)。文件不存在/不可读 = 空表(诊断面静默降级)。
pub(crate) fn read_tail(path: &std::path::Path, max_bytes: u64) -> Vec<String> {
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
    lines.into_iter().map(String::from).collect()
}
