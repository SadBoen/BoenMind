//! 技能库(W4b;config/skills.json;合同 capability/skill.v0_1)。

use super::{AdminConfig, admin_error};
use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

fn skills_file(cfg: &AdminConfig) -> std::path::PathBuf {
    cfg.data_dir.join("config").join("skills.json")
}

/// 读技能库(缺文件 = 空库)。
fn read_skills(file: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["skills"].as_array().cloned())
        .unwrap_or_default()
}

/// GET /admin/skills:技能库清单(角色页挂载勾选 + 展示)。
pub async fn skills_get(State(cfg): State<AdminConfig>) -> Response {
    let skills = read_skills(&skills_file(&cfg));
    Json(json!({ "ok": true, "skills": skills })).into_response()
}

/// POST /admin/skills:新建或更新技能(skill_id 同则覆盖)。
/// 校验走合同 skill.v0_1——Skill 只是数据,加载不改变权限。
pub async fn skills_set(State(cfg): State<AdminConfig>, Json(mut body): Json<Value>) -> Response {
    if body["description"].is_null() {
        body["description"] = json!(null);
    }
    if body["allowed_capabilities"].is_null() {
        body["allowed_capabilities"] = json!([]);
    }
    if let Err(e) = bm_contract::schemas::validate(bm_contract::registries::SKILL_SCHEMA, &body) {
        return admin_error(StatusCode::BAD_REQUEST, format!("技能不合规: {e}"));
    }
    let file = skills_file(&cfg);
    let mut skills = read_skills(&file);
    let id = body["skill_id"].as_str().unwrap_or_default().to_string();
    if let Some(slot) = skills
        .iter_mut()
        .find(|s| s["skill_id"].as_str() == Some(&id))
    {
        *slot = body.clone();
    } else {
        skills.push(body.clone());
    }
    if let Some(dir) = file.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("目录创建失败: {e}"),
        );
    }
    let text = match serde_json::to_string_pretty(&json!({ "skills": skills })) {
        Ok(t) => crate::config_store::crlf(t),
        Err(_) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, "序列化失败"),
    };
    if let Err(e) = bm_persist::atomic_write(&file, text.as_bytes()) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("写入失败: {e}"));
    }
    Json(json!({ "ok": true, "note": "技能已保存,下一回合起生效" })).into_response()
}

/// DELETE /admin/skills/{id}:删除技能(已挂载角色在组装时自动跳过缺失技能)。
pub async fn skills_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let file = skills_file(&cfg);
    let mut skills = read_skills(&file);
    let before = skills.len();
    skills.retain(|s| s["skill_id"].as_str() != Some(&id));
    if skills.len() == before {
        return admin_error(StatusCode::NOT_FOUND, "技能不存在");
    }
    let text = match serde_json::to_string_pretty(&json!({ "skills": skills })) {
        Ok(t) => crate::config_store::crlf(t),
        Err(_) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, "序列化失败"),
    };
    if let Err(e) = bm_persist::atomic_write(&file, text.as_bytes()) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("写入失败: {e}"));
    }
    Json(json!({ "ok": true, "note": "技能已删除" })).into_response()
}
