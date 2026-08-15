//! 工作文件夹：目录枚举与文件读取（文本 / 二进制 base64）。

use axum::{Json, extract::{Query, State}, http::StatusCode};
use bm_core::workspace;
use serde::Deserialize;

use crate::{ApiResult, api_error};

// ---------------------------------------------------------------------------
// 工作文件夹
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListWorkspaceParams {
    #[serde(default)]
    pub dir: String,
}

pub async fn list_workspace(
    State(state): crate::SharedState,
    Query(params): Query<ListWorkspaceParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().await;
    let root = config.working_dir.clone();
    drop(config);
    match workspace::list_dir(&root, &params.dir) {
        Ok(entries) => Ok(Json(serde_json::json!({
            "dir": params.dir,
            "entries": entries,
        }))),
        Err(workspace::WorkspaceError::OutsideRoot(msg)) => {
            Err(api_error(StatusCode::BAD_REQUEST, format!("路径越界: {msg}")))
        }
        Err(err) => Err(api_error(StatusCode::BAD_REQUEST, err.to_string())),
    }
}

#[derive(Deserialize)]
pub struct ReadFileParams {
    pub path: String,
}

/// 读取工作文件夹内文件。文本文件返回 UTF-8 内容，二进制（图片/PDF）返回 base64。
pub async fn read_workspace_file(
    State(state): crate::SharedState,
    Query(params): Query<ReadFileParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().await;
    let root = config.working_dir.clone();
    drop(config);

    let bytes = match workspace::read_file(&root, &params.path) {
        Ok(b) => b,
        Err(workspace::WorkspaceError::OutsideRoot(msg)) => {
            return Err(api_error(StatusCode::BAD_REQUEST, format!("路径越界: {msg}")));
        }
        Err(err) => {
            return Err(api_error(StatusCode::BAD_REQUEST, err.to_string()));
        }
    };
    let name = params
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&params.path)
        .to_string();
    let mime = workspace::mime_for(&name);

    if workspace::is_text(mime) {
        match String::from_utf8(bytes) {
            Ok(content) => Ok(Json(serde_json::json!({
                "name": name,
                "path": params.path,
                "mime": mime,
                "kind": "text",
                "content": content,
                "size": content.len(),
            }))),
            Err(_) => Err(api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "文件不是合法的 UTF-8 文本",
            )),
        }
    } else {
        Ok(Json(serde_json::json!({
            "name": name,
            "path": params.path,
            "mime": mime,
            "kind": "binary",
            "content": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes),
            "size": bytes.len(),
        })))
    }
}

#[derive(Deserialize)]
pub struct WriteFileParams {
    /// 相对工作文件夹的路径（正斜杠分隔）
    pub path: String,
    /// 文本内容（整体覆盖）
    pub content: String,
}

/// 写文本文件（M2 编辑器保存；父目录须存在，越界校验同读路径）。
pub async fn write_workspace_file(
    State(state): crate::SharedState,
    Json(params): Json<WriteFileParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().await;
    let root = config.working_dir.clone();
    drop(config);
    workspace::write_file(&root, &params.path, &params.content)
        .map_err(|err| crate::api_error_bad_request(err.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true, "path": params.path })))
}

/// Git 仓库状态（M2 分支图数据源）：工作目录是 git 仓库时返回当前分支、
/// 最近提交（含 parents 拓扑边）、本地分支指针与工作区变更；
/// 不是仓库 → `{ "repo": false }`（优雅降级）。
pub async fn git_info(
    State(state): crate::SharedState,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().await;
    let root = config.working_dir.clone();
    drop(config);
    Ok(Json(git_info_inner(&root)))
}

/// git 探测实现（纯函数便于单测）：四条只读命令，各自失败独立降级。
fn git_info_inner(root: &std::path::Path) -> serde_json::Value {
    use std::process::Command;
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .ok()
    };
    // 非仓库（或 git 不可用）→ 整组降级
    let branch = match run(&["rev-parse", "--abbrev-ref", "HEAD"]) {
        Some(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => return serde_json::json!({ "repo": false }),
    };
    // 提交拓扑（DAG）：%p = 父提交短 hash（空格分隔；merge 多个）。--branches
    // 覆盖全部本地分支（不拉远程），按拓扑序输出（新 → 旧）。
    let commits = run(&["log", "--branches", "-15", "--pretty=format:%h|%s|%p"])
        .map(|out| {
            if out.status.success() {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter_map(|l| {
                        let mut parts = l.splitn(3, '|');
                        let hash = parts.next()?.to_string();
                        let subject = parts.next().unwrap_or("").to_string();
                        let parents = parts
                            .next()
                            .unwrap_or("")
                            .split_whitespace()
                            .map(str::to_string)
                            .collect::<Vec<_>>();
                        Some(serde_json::json!({ "hash": hash, "subject": subject, "parents": parents }))
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new() // HEAD 无提交（空仓库）
            }
        })
        .unwrap_or_default();
    // 本地分支指针（tip 提交短 hash；分支图标签的锚点）
    let branches = run(&[
        "for-each-ref",
        "refs/heads",
        "--format=%(refname:short)|%(objectname:short)",
    ])
    .map(|out| {
        if out.status.success() {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| {
                    l.split_once('|').map(|(name, tip)| {
                        serde_json::json!({ "name": name, "tip": tip })
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    })
    .unwrap_or_default();
    let status = run(&["status", "--porcelain"])
        .map(|out| {
            if out.status.success() {
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        })
        .unwrap_or_default();
    serde_json::json!({
        "repo": true,
        "branch": branch,
        "commits": commits,
        "branches": branches,
        "status": status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_info_gracefully_reports_non_repo() {
        // 工作目录不是 git 仓库 → repo:false（不 panic）
        let v = git_info_inner(std::path::Path::new("C:\\"));
        assert_eq!(v["repo"], false);
    }
}
