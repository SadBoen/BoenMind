//! workspace.* handler 领域子模块（api.rs 拆分）。
//! 内存注册表快照 + 建/改/删/排序/归档会话（目录/文件/日志不动）。

use serde_json::{json, Value};

use crate::api::AppState;
use crate::rpc::{err, err_with_details, ok};

/// workspace.list：内存注册表快照（createdAt/updatedAt 为 ISO-8601 string）+ 归档集。
pub(super) fn workspace_list(state: &AppState) -> Value {
    ok(state.workspace_snapshot())
}

/// workspace.create（未特权）：对已存在目录幂等注册。
/// 目录缺失/非目录 → `workspace-invalid-path`；已属某 workspace → 返回该 workspace（created:false）。
pub(super) fn workspace_create(state: &AppState, payload: Value) -> Value {
    use std::path::Path;

    let Some(path) = payload.get("path").and_then(Value::as_str) else {
        return err("bad-request", "missing path");
    };
    let p = Path::new(path);
    if !p.is_dir() {
        return err_with_details(
            "workspace-invalid-path",
            "path is not a directory",
            json!({ "path": path }),
        );
    }

    // 已注册同路径 → 幂等返回（created:false）。
    let mut ws = state.workspaces.lock().unwrap();
    for v in ws.values() {
        if v.get("path").and_then(Value::as_str) == Some(path) {
            return ok(json!({ "workspace": v, "created": false }));
        }
    }
    let now = chrono::Utc::now().to_rfc3339();
    let workspace = json!({
        "workspaceId": uuid::Uuid::new_v4().to_string(),
        "path": path,
        "title": p.file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string(),
        "sessionIds": [],
        "createdAt": now,
        "updatedAt": now,
    });
    let id = workspace["workspaceId"].as_str().unwrap().to_string();
    ws.insert(id, workspace.clone());
    drop(ws);
    state.broadcast_host(
        "host/workspace-changed",
        json!({ "workspace": workspace.clone() }),
    );
    ok(json!({ "workspace": workspace, "created": true }))
}

/// workspace.rename：title trim 后非空；未知 id → `workspace-not-found`；冲突 → `workspace-name-conflict`；
/// 改回原名 = 空操作成功。返回更新后的 workspace。
pub(super) fn workspace_rename(state: &AppState, payload: Value) -> Value {
    let Some(workspace_id) = payload.get("workspaceId").and_then(Value::as_str) else {
        return err("bad-request", "missing workspaceId");
    };
    let Some(title) = payload.get("title").and_then(Value::as_str).map(str::trim) else {
        return err("bad-request", "missing title");
    };
    if title.is_empty() {
        return err("bad-request", "title must be non-empty");
    }
    let mut ws = state.workspaces.lock().unwrap();
    // 标题冲突预检（不持可变借用）：另一 workspace 已用同名（改回原名除外）。
    for v in ws.values() {
        if v.get("title").and_then(Value::as_str) == Some(title)
            && v.get("workspaceId").and_then(Value::as_str) != Some(workspace_id)
        {
            return err_with_details(
                "workspace-name-conflict",
                "workspace title already in use",
                json!({ "name": title }),
            );
        }
    }
    let Some(view) = ws.get_mut(workspace_id) else {
        return err_with_details(
            "workspace-not-found",
            "workspace not found",
            json!({ "workspaceId": workspace_id }),
        );
    };
    view["title"] = json!(title);
    view["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let updated = view.clone();
    drop(ws);
    state.broadcast_host(
        "host/workspace-changed",
        json!({ "workspace": updated.clone() }),
    );
    ok(json!({ "workspace": updated }))
}

/// workspace.delete：仅删注册，目录/文件/日志不动；未知 id → `workspace-not-found`。
pub(super) fn workspace_delete(state: &AppState, payload: Value) -> Value {
    let Some(workspace_id) = payload.get("workspaceId").and_then(Value::as_str) else {
        return err("bad-request", "missing workspaceId");
    };
    let mut ws = state.workspaces.lock().unwrap();
    if ws.remove(workspace_id).is_none() {
        return err_with_details(
            "workspace-not-found",
            "workspace not found",
            json!({ "workspaceId": workspace_id }),
        );
    }
    drop(ws);
    // HostFrame：注册删除增量（台账 §3.1 host/workspace-removed；官方 delete
    // 语义 = 删除增量帧，前端据此移除列表项，无需全量快照）。
    state.broadcast_host(
        "host/workspace-removed",
        json!({ "workspaceId": workspace_id }),
    );
    ok(json!({ "deleted": true }))
}

/// workspace.insertBefore：DOM-insertBefore 语义；省略锚点 = 追加末尾。
/// 返回完整显示序。
pub(super) fn workspace_insert_before(state: &AppState, payload: Value) -> Value {
    let Some(workspace_id) = payload.get("workspaceId").and_then(Value::as_str) else {
        return err("bad-request", "missing workspaceId");
    };
    let before = payload.get("beforeWorkspaceId").and_then(Value::as_str);
    let ws = state.workspaces.lock().unwrap();
    let ids: Vec<String> = ws.keys().cloned().collect();
    if !ids.contains(&workspace_id.to_string()) {
        return err_with_details(
            "workspace-not-found",
            "workspace not found",
            json!({ "workspaceId": workspace_id }),
        );
    }
    if let Some(anchor) = before {
        if !ids.contains(&anchor.to_string()) {
            return err_with_details(
                "workspace-not-found",
                "anchor workspace not found",
                json!({ "workspaceId": anchor }),
            );
        }
    }
    // 重排：把 workspace_id 从当前位移除，插到锚点前（无锚点 = 末尾）。
    let mut order: Vec<String> = ids
        .into_iter()
        .filter(|id| id != workspace_id)
        .collect();
    match before {
        Some(anchor) => {
            if let Some(pos) = order.iter().position(|id| id == anchor) {
                order.insert(pos, workspace_id.to_string());
            } else {
                order.push(workspace_id.to_string());
            }
        }
        None => order.push(workspace_id.to_string()),
    }
    drop(ws);
    // HostFrame：重排后完整持久序（台账 §3.1 host/workspace-order-changed）。
    state.broadcast_host(
        "host/workspace-order-changed",
        json!({ "workspaceIds": order }),
    );
    ok(json!({ "workspaceIds": order }))
}

/// workspace.insertSessionBefore：把 sessionId 加进（或重排）workspace.sessionIds。
/// 未知 workspace → `workspace-not-found`；session/锚点不在账 → `workspace-move-invalid`；
/// 原位移动 = 空操作。返回更新后的 workspace。
pub(super) fn workspace_insert_session_before(state: &AppState, payload: Value) -> Value {
    let Some(workspace_id) = payload.get("workspaceId").and_then(Value::as_str) else {
        return err("bad-request", "missing workspaceId");
    };
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let before = payload.get("beforeSessionId").and_then(Value::as_str);
    let mut ws = state.workspaces.lock().unwrap();
    let Some(view) = ws.get_mut(workspace_id) else {
        return err_with_details(
            "workspace-not-found",
            "workspace not found",
            json!({ "workspaceId": workspace_id }),
        );
    };
    let mut session_ids: Vec<String> = view["sessionIds"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    // 会话须已在账（新会话由前端 create 后 insert 进来）。
    if !session_ids.contains(&session_id.to_string()) {
        return err_with_details(
            "workspace-move-invalid",
            "session is not in this workspace",
            json!({
                "workspaceId": workspace_id,
                "sessionId": session_id,
                "beforeSessionId": before,
            }),
        );
    }
    if let Some(anchor) = before {
        if !session_ids.contains(&anchor.to_string()) {
            return err_with_details(
                "workspace-move-invalid",
                "anchor session is not in this workspace",
                json!({ "workspaceId": workspace_id, "sessionId": session_id }),
            );
        }
    }
    // 原位移动 = 空操作成功。
    let already_in_place = before.is_none()
        || session_ids.last() == before.map(|b| b.to_string()).as_ref();
    if !already_in_place {
        session_ids.retain(|id| id != session_id);
        match before {
            Some(anchor) => {
                if let Some(pos) = session_ids.iter().position(|id| id == anchor) {
                    session_ids.insert(pos, session_id.to_string());
                } else {
                    session_ids.push(session_id.to_string());
                }
            }
            None => session_ids.push(session_id.to_string()),
        }
        view["sessionIds"] = json!(session_ids);
        view["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    }
    let updated = view.clone();
    drop(ws);
    state.broadcast_host(
        "host/workspace-changed",
        json!({ "workspace": updated.clone() }),
    );
    ok(json!({ "workspace": updated }))
}

/// workspace.archiveSession：把 sessionId 加入归档集（幂等）；会话既非 live 也不在持久化
/// → `session-not-found`。返回完整新归档集。
pub(super) async fn workspace_archive_session(state: &AppState, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    // 会话存在性：live 或持久化。
    let live = state.sessions.lock().unwrap().contains_key(session_id);
    let persisted = state
        .runtime
        .persist
        .list_sessions()
        .await
        .map(|ids| ids.contains(&session_id.to_string()))
        .unwrap_or(false);
    if !live && !persisted {
        return err_with_details(
            "session-not-found",
            "session not found",
            json!({ "sessionId": session_id }),
        );
    }
    let mut archived = state.archived_session_ids.lock().unwrap();
    if !archived.contains(&session_id.to_string()) {
        archived.push(session_id.to_string());
    }
    let new_set = archived.clone();
    drop(archived);
    // HostFrame：归档集每次持久化变更后全量（台账 §3.1 host/archived-sessions-changed）。
    state.broadcast_host(
        "host/archived-sessions-changed",
        json!({ "archivedSessionIds": new_set }),
    );
    ok(json!({ "archivedSessionIds": new_set }))
}
