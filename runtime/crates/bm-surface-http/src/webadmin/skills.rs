//! 技能库(W4b;config/skills.json;合同 capability/skill.v0_1)。

use super::{AdminConfig, bad_request, internal, not_found, respond_or_fail};
use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

fn skills_file(cfg: &AdminConfig) -> std::path::PathBuf {
    // 路径约定单源化(ADR-0053):不再手拼,与装载入口共用同一约定。
    bm_core::ports::skill_host::skills_config_path(&cfg.data_dir)
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

/// 取一条技能定义(读最新盘上 skills.json)。
///
/// 注意与 ADR-0053 的边界:此处是**管理面 CRUD**读取,需返回原始 JSON 供前端
/// 表单回显,故保留 Raw Value 形态;「声明 → 能力」的**装载**解析在
/// `bm_core::ports::skill_host`(启动与整表重载单入口)。
fn skill_def_by_id(file: &std::path::Path, id: &str) -> Option<Value> {
    read_skills(file)
        .ok()?
        .into_iter()
        .find(|s| s["skill_id"].as_str() == Some(id))
}

/// 热重载一个技能的脚本能力(ADR-0033):摘旧 → 按最新定义重编译注册。
///
/// - 旧能力经 `handle.capabilities_unregister` 墓碑化(status=unavailable,
///   epoch 不回退,复用 ADR-0032 代际机制);
/// - 新能力经 `handle.capabilities_register` 注册,异步分道由 provider
///   命名约定(`skill.*`)自动归属(ADR-0033)。
///
/// 未装配脚本执行面(`skills=None`)= 生产未启用 wasm 脚本,跳过(纯知识包)。
/// 返回面向用户的结果说明(替代此前笼统的「下一回合起生效」)。
async fn reload_skill(cfg: &AdminConfig, skill_id: &str) -> String {
    let Some(manager) = cfg.skills.clone() else {
        return "技能已保存(纯知识包)。".to_string();
    };
    // 1) 摘旧:编译缓存 + 核心注册表(墓碑化)。
    let old = manager.unregister_skill(skill_id);
    if !old.is_empty()
        && let Err(e) = cfg.handle.capabilities_unregister(old).await
    {
        return format!("技能已保存,但旧脚本能力摘除失败: {e}");
    }
    // 2) 按最新定义重编译(已删除/无 scripts = 到此为止,新面为空)。
    let Some(def_value) = skill_def_by_id(&skills_file(cfg), skill_id) else {
        return "技能脚本能力已即时摘除。".to_string();
    };
    if def_value.get("scripts").is_none() {
        return "技能已保存(纯知识包,无脚本)。".to_string();
    }
    let Ok(def) = serde_json::from_value::<bm_contract::skill::SkillDefinition>(def_value) else {
        return "技能已保存,但 scripts 载荷非法,脚本未装载。".to_string();
    };
    let root = cfg.data_dir.join("skills").join(skill_id);
    let manifests = match manager.register_skill(skill_id, &def, &root) {
        Ok(m) => m,
        Err(e) => return format!("技能已保存,但脚本编译失败(未装载): {e}"),
    };
    let entries = bm_core::ports::skill_host::placeholder_entries(manifests);
    match cfg.handle.capabilities_register(entries).await {
        Ok(names) => format!("技能已热重载,{} 个脚本能力即时生效。", names.len()),
        Err(e) => format!("技能脚本已编译,但注册失败: {e}"),
    }
}

/// GET /admin/skills:技能库清单(角色页挂载勾选 + 展示)。
pub async fn skills_get(State(cfg): State<AdminConfig>) -> Response {
    let skills = respond_or_fail!(read_skills(&skills_file(&cfg)), internal);
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
        return bad_request(format!("技能不合规: {e}"));
    }
    let file = skills_file(&cfg);
    let mut skills = respond_or_fail!(read_skills(&file), internal);
    let id = body["skill_id"].as_str().unwrap_or_default().to_string();
    if let Some(slot) = skills
        .iter_mut()
        .find(|s| s["skill_id"].as_str() == Some(&id))
    {
        *slot = body.clone();
    } else {
        skills.push(body.clone());
    }
    respond_or_fail!(write_skills(&file, &skills), internal);
    let note = reload_skill(&cfg, &id).await;
    Json(json!({ "ok": true, "note": note })).into_response()
}

/// DELETE /admin/skills/{id}:删除技能(已挂载角色在组装时自动跳过缺失技能)。
pub async fn skills_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let file = skills_file(&cfg);
    let mut skills = respond_or_fail!(read_skills(&file), internal);
    let before = skills.len();
    skills.retain(|s| s["skill_id"].as_str() != Some(&id));
    if skills.len() == before {
        return not_found("技能不存在");
    }
    respond_or_fail!(write_skills(&file, &skills), internal);
    let note = reload_skill(&cfg, &id).await;
    Json(json!({ "ok": true, "note": note })).into_response()
}
