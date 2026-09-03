//! fs.* 文件工具集内置能力(ADR-0021,2026-09-04 用户裁决「我希望内置」):
//! 查/读/改是 AI 基础手脚,自 code-tools 随包插件收编进内核——开箱即用、
//! 零进程损耗、免「扫描-批准」流程;沙箱根 = 工作区注册表(config/workspaces.json,
//! 注册表空时回落工作区浏览根),每调用重建(注册表增删免重启生效)。
//!
//! 形态 = 内置异步能力(provider id `builtin.async`,同 system.exec):
//! search 走 walkdir 全树遍历,不得占死单写者循环。审批分级不变:
//! fs.search / fs.read = read-only 直通;fs.write / fs.edit = 审批卡。
//!
//! 对话工具名:fs.search → fs__search(turn 侧 `.` → `__`)。

mod guard;
mod ops;

pub use guard::{Roots, display_path, normalize_lexical};

use bm_contract::capability::CapabilityManifest;
use bm_core::ports::{AsyncCallError, AsyncCapabilityExecutor};
use bm_core::registry::CapabilityProvider;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

pub const FS_SEARCH: &str = "fs.search";
pub const FS_READ: &str = "fs.read";
pub const FS_WRITE: &str = "fs.write";
pub const FS_EDIT: &str = "fs.edit";

/// 四件套 manifest + 注册占位 provider(执行体在 [`FsExecutor`];
/// 同步面直调一律拒绝,防绕过 turn 语义——system.exec 同款口径)。
pub fn fs_capability_entries() -> Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)> {
    vec![
        entry(
            FS_SEARCH,
            "read-only",
            "not-required",
            true,
            true,
            30_000,
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "搜索内容(默认按正则解释;纯文本加 fixed=true)"},
                    "fixed": {"type": "boolean", "description": "true=按字面文本搜索(不做正则解释),默认 false"},
                    "case_sensitive": {"type": "boolean", "description": "大小写敏感,默认 false(忽略大小写)"},
                    "max_results": {"type": "integer", "description": "命中上限(可选,默认 80,封顶 500)"}
                },
                "required": ["query"]
            }),
        ),
        entry(
            FS_READ,
            "read-only",
            "not-required",
            true,
            true,
            10_000,
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "文件路径(工作区内绝对路径,或相对第一根目录的相对路径)"},
                    "offset": {"type": "integer", "description": "起始行号(1 起,默认 1)"},
                    "limit": {"type": "integer", "description": "最多读多少行(默认 2000)"}
                },
                "required": ["path"]
            }),
        ),
        entry(
            FS_WRITE,
            "external-side-effect",
            "required",
            false,
            false,
            10_000,
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "目标文件路径(工作区内)"},
                    "content": {"type": "string", "description": "完整文件内容(UTF-8 文本)"}
                },
                "required": ["path", "content"]
            }),
        ),
        entry(
            FS_EDIT,
            "external-side-effect",
            "required",
            false,
            false,
            10_000,
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "目标文件路径(工作区内)"},
                    "old_string": {"type": "string", "description": "要被替换的精确原文(逐字一致含缩进)"},
                    "new_string": {"type": "string", "description": "替换后的内容(删内容传空串)"},
                    "replace_all": {"type": "boolean", "description": "多处命中时全部替换,默认 false"}
                },
                "required": ["path", "old_string", "new_string"]
            }),
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn entry(
    capability: &str,
    effect: &str,
    approval: &str,
    idempotent: bool,
    cancellable: bool,
    timeout_ms: u64,
    input_schema: Value,
) -> (CapabilityManifest, Arc<dyn CapabilityProvider>) {
    let manifest: CapabilityManifest = serde_json::from_value(json!({
        "capability": capability,
        "provider": "builtin.async",
        "version": "0.1.0",
        "input_schema": input_schema,
        "output_schema": {"type": "object"},
        "effect": effect,
        "idempotent": idempotent,
        "cancellable": cancellable,
        "timeout_ms": timeout_ms,
        "approval": approval,
        "scopes": ["domain:fs"]
    }))
    .expect("fs manifest 合法");
    (manifest, Arc::new(FsPlaceholder))
}

struct FsPlaceholder;
impl CapabilityProvider for FsPlaceholder {
    fn invoke(&self, _args: Value) -> Result<Value, String> {
        Err("fs.* 仅限运行时 turn 循环经异步路径调用".into())
    }
}

/// 异步执行体:每次调用从工作区注册表重建沙箱根(增删工作区免重启),
/// 注册表为空回落工作区浏览根(首次使用未开设置页也有合理默认)。
pub struct FsExecutor {
    pub data_dir: PathBuf,
    pub fallback_root: PathBuf,
}

impl FsExecutor {
    pub fn new(data_dir: impl Into<PathBuf>, fallback_root: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            fallback_root: fallback_root.into(),
        }
    }

    fn roots(&self) -> Roots {
        let mut raw: Vec<String> = bm_core::workspace::read_workspaces(&self.data_dir)
            .into_iter()
            .map(|w| w.path)
            .filter(|p| !p.is_empty())
            .collect();
        if raw.is_empty() {
            raw.push(self.fallback_root.display().to_string());
        }
        Roots::new(&raw)
    }
}

#[async_trait::async_trait]
impl AsyncCapabilityExecutor for FsExecutor {
    async fn call(
        &self,
        _operation_id: &str,
        capability: &str,
        args: Value,
        _deadline: std::time::Duration,
    ) -> Result<Value, AsyncCallError> {
        let roots = self.roots();
        let capability = capability.to_string();
        // 阻塞面(树遍历/磁盘 IO)挪出单写者循环
        let out = tokio::task::spawn_blocking(move || match capability.as_str() {
            FS_SEARCH => ops::search(&roots, &args),
            FS_READ => ops::read(&roots, &args),
            FS_WRITE => ops::write(&roots, &args),
            FS_EDIT => ops::edit(&roots, &args),
            other => json!({"ok": false, "error": format!("fs 执行器不认识能力 {other}")}),
        })
        .await
        .map_err(|e| AsyncCallError::Transport(format!("fs 任务失败: {e}")))?;
        if out["ok"].as_bool().unwrap_or(false) {
            Ok(out)
        } else {
            Err(AsyncCallError::Transport(
                out["error"].as_str().unwrap_or("fs 工具失败").to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_entries_with_approval_split_and_async_marker() {
        let set = fs_capability_entries();
        assert_eq!(set.len(), 4);
        for (m, _) in &set {
            assert_eq!(m.provider, "builtin.async", "fs.* 必须注册为异步");
        }
        let approval_of = |name: &str| {
            set.iter()
                .find(|(m, _)| m.capability == name)
                .expect("能力在册")
                .0
                .approval
        };
        assert_eq!(
            approval_of(FS_SEARCH),
            bm_contract::capability::ApprovalRequirement::NotRequired
        );
        assert_eq!(
            approval_of(FS_READ),
            bm_contract::capability::ApprovalRequirement::NotRequired
        );
        assert_eq!(
            approval_of(FS_WRITE),
            bm_contract::capability::ApprovalRequirement::Required
        );
        assert_eq!(
            approval_of(FS_EDIT),
            bm_contract::capability::ApprovalRequirement::Required
        );
    }

    #[test]
    fn placeholder_rejects_sync_invoke() {
        let (_, provider) = fs_capability_entries().remove(0);
        assert!(provider.invoke(json!({})).is_err());
    }

    #[tokio::test]
    async fn executor_reads_and_writes_within_registry_roots() {
        let dir = tempfile::tempdir().expect("tmp");
        let data = tempfile::tempdir().expect("data");
        let cfg = data.path().join("config");
        std::fs::create_dir_all(&cfg).expect("cfg");
        std::fs::write(
            cfg.join("workspaces.json"),
            json!({"workspaces": [{"id": "default", "name": "默", "path": dir.path().display().to_string()}]})
                .to_string(),
        )
        .expect("write");

        let exec = FsExecutor::new(data.path(), data.path().join("workspace"));
        let out = exec
            .call(
                "op",
                FS_WRITE,
                json!({"path": "a.txt", "content": "hi"}),
                std::time::Duration::from_secs(5),
            )
            .await
            .expect("write ok");
        assert_eq!(out["ok"], true);
        let out = exec
            .call(
                "op",
                FS_READ,
                json!({"path": "a.txt"}),
                std::time::Duration::from_secs(5),
            )
            .await
            .expect("read ok");
        assert!(out["content"].as_str().expect("content").contains("hi"));
    }

    #[tokio::test]
    async fn executor_errors_surface_as_transport_with_reason() {
        let data = tempfile::tempdir().expect("data");
        let exec = FsExecutor::new(data.path(), data.path().join("workspace"));
        let err = exec
            .call(
                "op",
                FS_READ,
                json!({"path": "../../x"}),
                std::time::Duration::from_secs(5),
            )
            .await;
        assert!(matches!(err, Err(AsyncCallError::Transport(m)) if !m.is_empty()));
    }

    #[tokio::test]
    async fn empty_registry_falls_back_to_workspace_root() {
        let data = tempfile::tempdir().expect("data");
        let ws = data.path().join("workspace");
        std::fs::create_dir_all(&ws).expect("ws");
        std::fs::write(ws.join("b.txt"), "fallback").expect("write");
        let exec = FsExecutor::new(data.path(), &ws);
        let out = exec
            .call(
                "op",
                FS_READ,
                json!({"path": "b.txt"}),
                std::time::Duration::from_secs(5),
            )
            .await
            .expect("read ok");
        assert!(
            out["content"]
                .as_str()
                .expect("content")
                .contains("fallback")
        );
    }
}
