//! 【BoenMind 补丁 P11】vendored 版本刻意不带上游 `tests/` 目录，
//! 但 `src/session_index.rs` 的 `#[cfg(test)]` 模块以
//! `#[path = "../tests/common/mod.rs"]` 引用 `TestHarness`——
//! 缺失导致 workspace 全量 `cargo test` 编译失败。
//!
//! 本文件提供 TestHarness 的**最小桩实现**（仅覆盖 session_index.rs
//! 测试用到的 new / temp_path / log / record_artifact），目的是让
//! lib test 目标可编译；**不是**上游测试基座的功能替代。上游升级时
//! 若恢复自带 tests/，按台账 P11 删除本文件。
//!
//! 详见 backend/vendor/UPSTREAM_PATCHES.md P11。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static HARNESS_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 测试基座最小桩（见文件头说明）。
pub struct TestHarness {
    base: PathBuf,
}

/// info_ctx 闭包参数：字段收集容器（仅 push 元组）。
pub struct LogCtx {
    fields: Vec<(String, String)>,
}

impl LogCtx {
    pub fn push(&mut self, kv: (String, String)) {
        self.fields.push(kv);
    }
}

/// 桩日志器：info / info_ctx 均为 no-op（不收集、不落盘）。
pub struct HarnessLog;

impl HarnessLog {
    pub fn info(&self, _key: &str, _value: impl std::fmt::Display) {}
    pub fn info_ctx(
        &self,
        _key: &str,
        _value: impl std::fmt::Display,
        f: impl FnOnce(&mut LogCtx),
    ) {
        let mut ctx = LogCtx { fields: Vec::new() };
        f(&mut ctx);
    }
}

impl TestHarness {
    pub fn new(name: &str) -> Self {
        let n = HARNESS_COUNTER.fetch_add(1, Ordering::Relaxed);
        let sanitized: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let base = std::env::temp_dir().join(format!(
            "pi_test_harness_{sanitized}_{}_{n}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&base);
        Self { base }
    }

    /// 基目录下的相对路径（测试用工作路径，创建父目录）。
    pub fn temp_path(&self, rel: impl AsRef<Path>) -> PathBuf {
        let p = self.base.join(rel);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        p
    }

    pub fn log(&self) -> HarnessLog {
        HarnessLog
    }

    pub fn record_artifact(&self, _name: &str, _path: &Path) {}
}
