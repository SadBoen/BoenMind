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

/// 读技能库。缺文件 = 空库;JSON 损坏或缺 skills 数组 = 拒绝(2026-09-07
/// 复核批:此前静默回落空 Vec,盘上文件半损坏时下一次保存会把整库覆写清空,
/// 与 providers 同口径=损坏拒绝加载/覆写)。
fn read_skills(file: &std::path::Path) -> Result<Vec<Value>, String> {
    let v = match super::json_store::read_json_file(
        file,
        "读取技能库失败",
        "skills.json JSON 格式已损坏,拒绝加载/覆写",
    )? {
        super::json_store::JsonRead::Value(v) => v,
        super::json_store::JsonRead::Missing => return Ok(Vec::new()),
    };
    v["skills"]
        .as_array()
        .cloned()
        .ok_or_else(|| "skills.json 缺少合法的 skills 数组".to_string())
}

/// 写技能库(issue #38 收口:原 set/delete 两处内联样板,统一走原语)。
fn write_skills(file: &std::path::Path, skills: &[Value]) -> Result<(), String> {
    super::json_store::write_json_file(file, &json!({ "skills": skills }), "写入失败")
}

/// GET /admin/skills:技能库清单(角色页挂载勾选 + 展示)。
pub async fn skills_get(State(cfg): State<AdminConfig>) -> Response {
    let skills = match read_skills(&skills_file(&cfg)) {
        Ok(s) => s,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
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
    let mut skills = match read_skills(&file) {
        Ok(s) => s,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let id = body["skill_id"].as_str().unwrap_or_default().to_string();
    if let Some(slot) = skills
        .iter_mut()
        .find(|s| s["skill_id"].as_str() == Some(&id))
    {
        *slot = body.clone();
    } else {
        skills.push(body.clone());
    }
    if let Err(e) = write_skills(&file, &skills) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "技能已保存,下一回合起生效" })).into_response()
}

/// DELETE /admin/skills/{id}:删除技能(已挂载角色在组装时自动跳过缺失技能)。
pub async fn skills_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let file = skills_file(&cfg);
    let mut skills = match read_skills(&file) {
        Ok(s) => s,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let before = skills.len();
    skills.retain(|s| s["skill_id"].as_str() != Some(&id));
    if skills.len() == before {
        return admin_error(StatusCode::NOT_FOUND, "技能不存在");
    }
    if let Err(e) = write_skills(&file, &skills) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "技能已删除" })).into_response()
}
