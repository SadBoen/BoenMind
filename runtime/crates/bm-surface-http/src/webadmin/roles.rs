//! 多角色管理(W4b;config/roles.json,ADR-0012 口径)。

use super::{AdminConfig, admin_error};
use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize, Clone)]
pub struct RoleConfigDoc {
    pub active_id: String,
    pub roles: Vec<RoleItem>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RoleItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub system_prompt: String,
    /// W4b:挂载的技能 skill_id 列表(只是数据;加载不改变权限)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
}

/// 读角色库文档。缺文件 = 默认单角色;旧版单 system_prompt 形态照旧迁移;
/// 空 roles 数组沿用旧默认回退。JSON 损坏/无 roles 数组 = 拒绝(2026-09-07
/// 复核批:此前一律静默回退默认文档,盘上文件损坏时下一次保存会把用户角色
/// 整库覆写,与 providers/skills 同口径=损坏拒绝加载/覆写)。
pub fn read_roles_doc(file: &std::path::Path) -> Result<RoleConfigDoc, String> {
    let raw = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(default_doc()),
        Err(e) => return Err(format!("读取角色库失败: {e}")),
    };
    let v: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("roles.json JSON 格式已损坏,拒绝加载/覆写: {e}"))?;
    if let Some(roles_arr) = v["roles"].as_array() {
        let active_id = v["active_id"].as_str().unwrap_or("assistant").to_string();
        let roles: Vec<RoleItem> = roles_arr
            .iter()
            .filter_map(|r| serde_json::from_value(r.clone()).ok())
            .collect();
        if roles.is_empty() {
            return Ok(default_doc());
        }
        return Ok(RoleConfigDoc { active_id, roles });
    } else if let Some(sp) = v["system_prompt"].as_str() {
        let name = v["name"].as_str().unwrap_or("assistant").to_string();
        return Ok(RoleConfigDoc {
            active_id: "assistant".into(),
            roles: vec![RoleItem {
                id: "assistant".into(),
                name,
                description: Some("默认通用助理".into()),
                system_prompt: sp.to_string(),
                skills: None,
            }],
        });
    }
    Err("roles.json 缺少合法的 roles 数组,拒绝加载/覆写".to_string())
}

fn default_doc() -> RoleConfigDoc {
    RoleConfigDoc {
        active_id: "assistant".into(),
        roles: vec![RoleItem {
            id: "assistant".into(),
            name: "assistant".into(),
            description: Some("默认通用助理".into()),
            system_prompt: "".into(),
            skills: None,
        }],
    }
}

pub fn write_roles_doc(file: &std::path::Path, doc: &RoleConfigDoc) -> Result<(), String> {
    if let Some(dir) = file.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return Err(format!("目录创建失败: {e}"));
    }
    let text = match serde_json::to_string_pretty(doc) {
        Ok(t) => crate::config_store::crlf(t),
        Err(e) => return Err(format!("序列化失败: {e}")),
    };
    bm_persist::atomic_write(file, text.as_bytes()).map_err(|e| format!("写入失败: {e}"))
}

/// 读全部角色与激活角色 id(设置页与聊天页下拉)。
pub async fn roles_get(State(cfg): State<AdminConfig>) -> Response {
    let file = roles_file(&cfg);
    let doc = match read_roles_doc(&file) {
        Ok(d) => d,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    Json(json!({
        "ok": true,
        "active_id": doc.active_id,
        "roles": doc.roles,
    }))
    .into_response()
}

/// 保存单角色(创建或更新,向后兼容 roles_set 以及多角色编辑)。
pub async fn roles_set(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let file = roles_file(&cfg);
    let mut doc = match read_roles_doc(&file) {
        Ok(d) => d,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };

    // 如果传递了全量 roles 数组，则全量更新
    if let Some(roles_arr) = body["roles"].as_array() {
        let roles: Vec<RoleItem> = roles_arr
            .iter()
            .filter_map(|r| serde_json::from_value(r.clone()).ok())
            .collect();
        if roles.is_empty() {
            return admin_error(StatusCode::BAD_REQUEST, "角色列表不能为空");
        }
        let active_id = body["active_id"]
            .as_str()
            .unwrap_or(&doc.active_id)
            .to_string();
        doc.roles = roles;
        doc.active_id = active_id;
    } else {
        // 单角色增改形态
        let id = body["id"]
            .as_str()
            .unwrap_or(body["name"].as_str().unwrap_or("assistant"))
            .to_string();
        let name = body["name"].as_str().unwrap_or(&id).to_string();
        let description = body["description"].as_str().map(|s| s.to_string());
        let system_prompt = body["system_prompt"].as_str().unwrap_or("").to_string();

        if let Some(existing) = doc.roles.iter_mut().find(|r| r.id == id) {
            existing.name = name;
            existing.description = description;
            existing.system_prompt = system_prompt;
            // W4b:技能挂载(传了才覆盖,保持既有挂载不丢)
            if let Some(sk) = body["skills"].as_array() {
                existing.skills = Some(
                    sk.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect(),
                );
            }
        } else {
            doc.roles.push(RoleItem {
                id: id.clone(),
                name,
                description,
                system_prompt,
                skills: body["skills"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                }),
            });
        }
        if body["set_active"].as_bool().unwrap_or(false) {
            doc.active_id = id;
        }
    }

    if let Err(e) = write_roles_doc(&file, &doc) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "已保存,下一回合起生效", "active_id": doc.active_id }))
        .into_response()
}

/// 删除指定角色
pub async fn roles_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let file = roles_file(&cfg);
    let mut doc = match read_roles_doc(&file) {
        Ok(d) => d,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    if doc.roles.len() <= 1 {
        return admin_error(StatusCode::BAD_REQUEST, "至少需要保留一个角色");
    }
    let orig_len = doc.roles.len();
    doc.roles.retain(|r| r.id != id);
    if doc.roles.len() == orig_len {
        return admin_error(StatusCode::NOT_FOUND, "指定角色不存在");
    }
    if doc.active_id == id {
        doc.active_id = doc.roles[0].id.clone();
    }
    if let Err(e) = write_roles_doc(&file, &doc) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "角色已删除", "active_id": doc.active_id })).into_response()
}

/// 设置默认激活角色
pub async fn roles_set_active(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let file = roles_file(&cfg);
    let mut doc = match read_roles_doc(&file) {
        Ok(d) => d,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    if !doc.roles.iter().any(|r| r.id == id) {
        return admin_error(StatusCode::NOT_FOUND, "指定角色不存在");
    }
    doc.active_id = id;
    if let Err(e) = write_roles_doc(&file, &doc) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "已设为默认角色", "active_id": doc.active_id }))
        .into_response()
}

fn roles_file(cfg: &AdminConfig) -> std::path::PathBuf {
    cfg.data_dir.join("config").join("roles.json")
}
