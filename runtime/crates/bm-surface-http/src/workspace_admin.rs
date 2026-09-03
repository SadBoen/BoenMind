//! W8(ADR-0018):「常规」设置后端——工作区注册表 CRUD + 运行环境探针。
//! 壳子私用 REST(W2 口径,不入冻结合同)。注册表读取实现唯一入口 =
//! `bm_core::workspace`(核心校验与这里写盘共用同一形状)。

use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::webadmin::{AdminConfig, admin_error};

/// 工作区注册表文件(config/workspaces.json)。
fn workspaces_file(cfg: &AdminConfig) -> std::path::PathBuf {
    cfg.data_dir.join("config").join("workspaces.json")
}

fn read_registry(cfg: &AdminConfig) -> Vec<Value> {
    std::fs::read_to_string(workspaces_file(cfg))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["workspaces"].as_array().cloned())
        .unwrap_or_default()
}

fn write_registry(cfg: &AdminConfig, list: &[Value]) -> Result<(), String> {
    let dir = cfg.data_dir.join("config");
    std::fs::create_dir_all(&dir).map_err(|e| format!("config 目录创建失败: {e}"))?;
    let text = serde_json::to_string_pretty(&json!({ "workspaces": list }))
        .map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(workspaces_file(cfg), text).map_err(|e| format!("写盘失败: {e}"))
}

fn new_workspace_id() -> String {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes).expect("系统熵源不可用");
    let mut s = String::from("ws_");
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 路径校验:存在、是目录、canonicalize(消解 .. 与符号链接目标)。
/// 返回规范化路径文本;错误为用户可读消息。
fn validate_dir(path_text: &str) -> Result<String, String> {
    let p = std::path::Path::new(path_text.trim());
    if p.as_os_str().is_empty() {
        return Err("路径不能为空".into());
    }
    if !p.exists() {
        return Err(format!("路径不存在: {}", p.display()));
    }
    if !p.is_dir() {
        return Err(format!("路径不是目录: {}", p.display()));
    }
    let canon = std::fs::canonicalize(p).map_err(|e| format!("路径解析失败: {e}"))?;
    Ok(pretty_normalized(canon.display().to_string()))
}

/// Windows canonicalize 产出 `\\?\D:\...` 扩展前缀;常规路径剥掉,
/// 入库/显示/模型注入都用人话形态。
fn pretty_normalized(path: String) -> String {
    path.strip_prefix(r"\\?\")
        .map(str::to_string)
        .unwrap_or(path)
}

fn entry_with_status(entry: &Value) -> Value {
    let path = entry["path"].as_str().unwrap_or("");
    let exists = std::path::Path::new(path).is_dir();
    json!({
        "id": entry["id"],
        "name": entry["name"],
        "path": path,
        "exists": exists,
        "isDefault": entry["id"] == bm_core::workspace::DEFAULT_WORKSPACE_ID,
    })
}

/// 首次读取播种:注册表为空时以现役文件浏览根建 default 条目
/// (旧文件树/旧用法零破坏;ADR-0018 决策 1)。
fn ensure_seeded(cfg: &AdminConfig) {
    if !read_registry(cfg).is_empty() {
        return;
    }
    let _ = write_registry(
        cfg,
        &[json!({
            "id": bm_core::workspace::DEFAULT_WORKSPACE_ID,
            "name": "默认工作区",
            "path": cfg.workspace_root.display().to_string(),
        })],
    );
}

/// GET /admin/workspaces
pub async fn workspaces_list(State(cfg): State<AdminConfig>) -> Response {
    ensure_seeded(&cfg);
    let list: Vec<Value> = read_registry(&cfg).iter().map(entry_with_status).collect();
    Json(json!({ "workspaces": list })).into_response()
}

/// POST /admin/workspaces {name, path}
pub async fn workspaces_create(
    State(cfg): State<AdminConfig>,
    Json(body): Json<Value>,
) -> Response {
    ensure_seeded(&cfg);
    let name = body["name"].as_str().unwrap_or("").trim().to_string();
    if name.is_empty() || name.len() > 100 {
        return admin_error(StatusCode::BAD_REQUEST, "名称必填且 ≤100 字符");
    }
    let path = match validate_dir(body["path"].as_str().unwrap_or("")) {
        Ok(p) => p,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    let mut list = read_registry(&cfg);
    if list
        .iter()
        .any(|e| e["path"].as_str() == Some(path.as_str()))
    {
        return admin_error(StatusCode::CONFLICT, "该路径已登记");
    }
    let entry = json!({ "id": new_workspace_id(), "name": name, "path": path });
    list.push(entry.clone());
    if let Err(e) = write_registry(&cfg, &list) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "workspace": entry_with_status(&entry) })).into_response()
}

/// PUT /admin/workspaces/{id} {name?, path?}(default 条目可改名改路径)
pub async fn workspaces_update(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let mut list = read_registry(&cfg);
    let Some(pos) = list.iter().position(|e| e["id"] == json!(id)) else {
        return admin_error(StatusCode::NOT_FOUND, format!("工作区「{id}」不存在"));
    };
    let mut entry = list[pos].clone();
    if let Some(n) = body["name"].as_str() {
        let n = n.trim();
        if n.is_empty() || n.len() > 100 {
            return admin_error(StatusCode::BAD_REQUEST, "名称必填且 ≤100 字符");
        }
        entry["name"] = json!(n);
    }
    if let Some(p) = body["path"].as_str() {
        let canon = match validate_dir(p) {
            Ok(c) => c,
            Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
        };
        if list
            .iter()
            .enumerate()
            .any(|(i, e)| i != pos && e["path"].as_str() == Some(canon.as_str()))
        {
            return admin_error(StatusCode::CONFLICT, "该路径已登记");
        }
        entry["path"] = json!(canon);
    }
    list[pos] = entry.clone();
    if let Err(e) = write_registry(&cfg, &list) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "workspace": entry_with_status(&entry) })).into_response()
}

/// DELETE /admin/workspaces/{id}(default 拒删:它承载旧文件树根)
pub async fn workspaces_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if id == bm_core::workspace::DEFAULT_WORKSPACE_ID {
        return admin_error(StatusCode::BAD_REQUEST, "默认工作区不可删除");
    }
    let mut list = read_registry(&cfg);
    let before = list.len();
    list.retain(|e| e["id"] != json!(id));
    if list.len() == before {
        return admin_error(StatusCode::NOT_FOUND, format!("工作区「{id}」不存在"));
    }
    if let Err(e) = write_registry(&cfg, &list) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true })).into_response()
}

/// POST /admin/workspaces/{id}/check:重新探测目录可用性
/// (不可用时回传错误;不自动改写登记路径)。
pub async fn workspaces_check(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let list = read_registry(&cfg);
    let Some(entry) = list.iter().find(|e| e["id"] == json!(id)) else {
        return admin_error(StatusCode::NOT_FOUND, format!("工作区「{id}」不存在"));
    };
    match validate_dir(entry["path"].as_str().unwrap_or("")) {
        Ok(canon) => Json(json!({ "ok": true, "path": canon })).into_response(),
        Err(e) => Json(json!({ "ok": false, "error": e })).into_response(),
    }
}

// ---- 运行环境探针(Python / Node.js;ADR-0018 决策 5)----------------------

/// 依次尝试候选命令,首个成功者胜出;5 秒超时;无 shell,无注入面。
async fn probe_first(candidates: &[(&str, &[&str])]) -> Value {
    for (program, args) in candidates {
        let Ok(out) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::process::Command::new(program)
                .args(*args)
                .stdin(std::process::Stdio::null())
                .output(),
        )
        .await
        else {
            continue; // 超时 = 视为不可用,试下一个候选
        };
        let Ok(out) = out else { continue };
        if !out.status.success() {
            continue;
        }
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let version = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string();
        if version.is_empty() {
            continue;
        }
        let args_json: Vec<Value> = args.iter().map(|a| json!(a)).collect();
        return json!({
            "installed": true,
            "version": version,
            "program": if args.is_empty() {
                json!(program.to_string())
            } else {
                json!(format!("{} {}", program, args.join(" ")))
            },
            "argv": args_json,
            "error": Value::Null,
        });
    }
    json!({
        "installed": false,
        "version": Value::Null,
        "program": Value::Null,
        "argv": [],
        "error": "未检测到可用的命令(或全部候选超时/失败)",
    })
}

/// GET /admin/runtime/env:Python / Node.js 安装情况(只回传版本与命令,
/// 无敏感信息;探测 = 固定候选命令,不做 PATH 之外的事)。
pub async fn runtime_env() -> Response {
    let python = probe_first(&[
        ("python3", &["--version"]),
        ("python", &["--version"]),
        ("py", &["-3", "--version"]),
    ])
    .await;
    let node = probe_first(&[("node", &["--version"])]).await;
    Json(json!({ "python": python, "node": node })).into_response()
}
