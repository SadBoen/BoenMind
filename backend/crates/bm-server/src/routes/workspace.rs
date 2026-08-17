//! 工作文件夹：目录枚举与文件读取（文本 / 二进制 base64）。

use axum::{Json, extract::{Query, State}, http::StatusCode};
use bm_core::workspace;
use serde::Deserialize;

use crate::{ApiResult, api_error};

// ---------------------------------------------------------------------------
// 工作文件夹
// ---------------------------------------------------------------------------

/// 解析项目根：请求显式 root（项目切换，编程壳传当前项目根）优先，
/// 缺省 = 全局配置工作目录。显式 root 必须落在已登记白名单内。
fn resolve_root(
    config: &bm_core::AppConfig,
    root: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    match root {
        Some(r) if !r.trim().is_empty() => {
            let candidate = std::path::PathBuf::from(r.trim());
            let allowed = workspace::trusted_roots(config);
            if workspace::path_under_any(&candidate, &allowed) {
                Ok(candidate)
            } else {
                Err(format!(
                    "项目根未登记：{}（请先在设置中加入 trusted_project_roots）",
                    candidate.display()
                ))
            }
        }
        _ => Ok(config.working_dir.clone()),
    }
}

/// 允许作为新建项目父目录的信任根（白名单 + 配置工作目录）。
/// 新建项目必须在这些根之下，避免把任意系统目录登记进白名单。
fn project_parent_roots(config: &bm_core::AppConfig) -> Vec<std::path::PathBuf> {
    let mut roots = workspace::trusted_roots(config);
    // 新项目默认建在全局工作目录下
    roots.push(config.working_dir.clone());
    roots
}

#[derive(Deserialize)]
pub struct ListWorkspaceParams {
    #[serde(default)]
    pub dir: String,
    /// 项目根（绝对路径）；缺省 = 配置工作目录
    #[serde(default)]
    pub root: Option<String>,
}

pub async fn list_workspace(
    State(state): crate::SharedState,
    Query(params): Query<ListWorkspaceParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let root = resolve_root(&config, params.root.as_deref()).map_err(|msg| {
        api_error(StatusCode::BAD_REQUEST, msg)
    })?;
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
    /// 项目根（绝对路径）；缺省 = 配置工作目录
    #[serde(default)]
    pub root: Option<String>,
}

/// 读取工作文件夹内文件。文本文件返回 UTF-8 内容，二进制（图片/PDF）返回 base64。
pub async fn read_workspace_file(
    State(state): crate::SharedState,
    Query(params): Query<ReadFileParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let root = resolve_root(&config, params.root.as_deref()).map_err(|msg| {
        api_error(StatusCode::BAD_REQUEST, msg)
    })?;
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
    /// 项目根（绝对路径）；缺省 = 配置工作目录
    #[serde(default)]
    pub root: Option<String>,
}

/// 写文本文件（M2 编辑器保存；父目录须存在，越界校验同读路径）。
pub async fn write_workspace_file(
    State(state): crate::SharedState,
    Json(params): Json<WriteFileParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let root = resolve_root(&config, params.root.as_deref()).map_err(|msg| {
        api_error(StatusCode::BAD_REQUEST, msg)
    })?;
    drop(config);
    workspace::write_file(&root, &params.path, &params.content)
        .map_err(|err| crate::api_error_bad_request(err.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true, "path": params.path })))
}

#[derive(Deserialize)]
pub struct GitInfoParams {
    /// 项目根（绝对路径）；缺省 = 配置工作目录
    #[serde(default)]
    pub root: Option<String>,
}

/// Git 仓库状态（M2 分支图数据源）：工作目录是 git 仓库时返回当前分支、
/// 最近提交（含 parents 拓扑边）、本地分支指针与工作区变更；
/// 不是仓库 → `{ "repo": false }`（优雅降级）。
pub async fn git_info(
    State(state): crate::SharedState,
    Query(params): Query<GitInfoParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let root = resolve_root(&config, params.root.as_deref()).map_err(|msg| {
        api_error(StatusCode::BAD_REQUEST, msg)
    })?;
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

#[derive(Deserialize)]
pub struct BrowseParams {
    /// 要浏览的绝对目录；空 = 系统根（Windows 盘符 / Unix /）
    #[serde(default)]
    pub path: String,
}

/// GET /api/workspace/browse — 目录选择器：浏览任意绝对目录（只读）。
/// 不校验白名单——浏览本身不产生任何变更；新建项目时才在信任根下创建。
pub async fn browse_workspace(
    Query(params): Query<BrowseParams>,
) -> ApiResult<Json<serde_json::Value>> {
    match workspace::browse_dir(&params.path) {
        Ok(result) => Ok(Json(serde_json::to_value(result).unwrap_or(serde_json::json!({})))),
        Err(err) => Err(api_error(StatusCode::BAD_REQUEST, err.to_string())),
    }
}

#[derive(Deserialize)]
pub struct NewProjectParams {
    /// 父目录（必须位于配置工作目录或白名单根之下）
    pub parent: String,
    /// 项目名称（不含路径分隔符；同时是目录名）
    pub name: String,
    /// 是否 git init -b main
    #[serde(default)]
    pub git_init: bool,
}

/// POST /api/workspace/projects/new — 新建项目：建目录（可选 git init）+
/// 自动登记 trusted_project_roots + 持久化配置。前端调用后可直接切换新项目。
pub async fn new_project(
    State(state): crate::SharedState,
    Json(body): Json<NewProjectParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let parent = std::path::PathBuf::from(body.parent.trim());
    if !parent.is_dir() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("父目录不存在或不可读: {}", parent.display()),
        ));
    }
    let parent_canon = parent.canonicalize().unwrap_or(parent.clone());
    let config = state.config.read().expect("config poisoned");
    let allowed = project_parent_roots(&config);
    if !workspace::path_under_any(&parent_canon, &allowed) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("项目父目录未在信任范围内（请选择工作目录或已登记项目根下的位置）: {}", parent_canon.display()),
        ));
    }
    // 提取当前白名单，create 成功后写回
    let roots = config.trusted_project_roots.clone();
    drop(config);

    let (project, new_roots, git_ok) = workspace::create_project(&parent_canon, &body.name, body.git_init, &roots)
        .map_err(|err| crate::api_error_bad_request(err.to_string()))?;

    // 白名单有新增才写配置（create_project 已去重）
    if new_roots.len() != roots.len() {
        let mut config = state.config.write().expect("config poisoned");
        config.trusted_project_roots = new_roots;
        if let Err(err) = bm_core::config::save(&config) {
            // 目录已建、白名单登记失败 → 回滚登记并报错（目录保留，用户可手动加白名单）
            return Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("项目目录已创建，但白名单登记失败（请手动在设置中添加）: {err}"),
            ));
        }
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "name": body.name,
        "root": project.display().to_string(),
        "git_init_ok": git_ok,
    })))
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

    #[test]
    fn resolve_root_prefers_explicit_root() {
        let mut config = bm_core::AppConfig::default();
        config.working_dir = std::path::PathBuf::from("D:\\default");
        config.trusted_project_roots = vec![std::path::PathBuf::from("D:\\projects\\my-app")];
        assert_eq!(
            resolve_root(&config, Some("D:\\projects\\my-app")).unwrap(),
            std::path::PathBuf::from("D:\\projects\\my-app")
        );
        assert_eq!(resolve_root(&config, None).unwrap(), config.working_dir);
        assert_eq!(resolve_root(&config, Some("  ")).unwrap(), config.working_dir);
        assert!(resolve_root(&config, Some("D:\\evil")).is_err());
    }
}
