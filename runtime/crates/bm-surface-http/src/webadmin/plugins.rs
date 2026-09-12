//! 通用 wasm 插件管理面(ADR-0041):`config/plugins.json` 增删改查 + 热重载。
//!
//! 与 `skills.rs` 同构:落盘后即时热重载(摘旧→重编译→注册新),无需重启。
//! 差别仅在数据形状——技能是 `SkillDefinition`(scripts 数组),插件是"一项
//! 能力一条声明"(capability/provider/wasm/effect/timeout)。

use super::{AdminConfig, bad_request, internal, not_found, respond_or_fail};
use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

fn plugins_file(cfg: &AdminConfig) -> std::path::PathBuf {
 // 路径约定单源化(ADR-0053):不再手拼,与装载入口共用同一约定。
    bm_core::ports::skill_host::plugins_config_path(&cfg.data_dir)
}

/// 读插件声明(顶层数组)。缺文件 = 空;损坏即拒(与 skills 同口径:不静默
/// 回落空表,避免下次保存把整库覆写清空)。
fn read_plugins(file: &std::path::Path) -> Result<Vec<Value>, String> {
    match super::json_store::read_json_file(
        file,
        "读取插件声明失败",
        "plugins.json JSON 格式已损坏,拒绝加载/覆写",
    )? {
        super::json_store::JsonRead::Value(v) => v
            .as_array()
            .cloned()
            .ok_or_else(|| "plugins.json 必须是数组".to_string()),
        super::json_store::JsonRead::Missing => Ok(Vec::new()),
    }
}

fn write_plugins(file: &std::path::Path, items: &[Value]) -> Result<(), String> {
    super::json_store::write_json_file(file, &Value::Array(items.to_vec()), "写入失败")
}

/// 热重载:按当前 plugins.json 重建全部 wasm 插件能力(ADR-0041)。
/// 语义 = **整表重载**:先摘除宿主里所有非 `skill.` provider 的能力(技能由
/// skills.json 管,不在此面),再按最新声明重编译注册。简单且幂等——插件数量
/// 远小于能力数量,整表重建的代价可忽略,换来实现与心智的简单。
/// 未装配 wasm 宿主(`manager=None`)= 未启用,直接返回说明。
async fn reload_plugins(cfg: &AdminConfig) -> String {
    let Some(manager) = cfg.skills.clone() else {
        return "wasm 执行面未启用,插件未装载。".to_string();
    };
 // 1) 摘除全部通用插件来源的能力(ADR-0042:按装载来源判断,不按 provider
 // 名字前缀;技能由 skills.json 自己的热重载管理,不在此面触达)。
    let old = manager.unregister_all_generic();
    if !old.is_empty()
        && let Err(e) = cfg.handle.capabilities_unregister(old.clone()).await
    {
        return format!("插件已保存,但旧能力摘除失败: {e}");
    }
 // 2) 按最新声明重编译注册。
    let manifests = manager.load_plugins_file(&plugins_file(cfg));
    let entries = bm_core::ports::skill_host::placeholder_entries(manifests);
    if entries.is_empty() {
        return "插件声明已保存(当前无 wasm 插件或全部非法)。".to_string();
    }
    match cfg.handle.capabilities_register(entries).await {
        Ok(names) => format!("插件已热重载,{} 个能力即时生效。", names.len()),
        Err(e) => format!("插件已编译,但注册失败: {e}"),
    }
}

/// GET /admin/plugins:插件声明清单。
pub async fn plugins_get(State(cfg): State<AdminConfig>) -> Response {
    let items = respond_or_fail!(read_plugins(&plugins_file(&cfg)), internal);
    Json(json!({ "ok": true, "plugins": items })).into_response()
}

/// POST /admin/plugins:新建或更新一条插件声明(capability 同则覆盖)。
pub async fn plugins_set(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    if body["capability"].as_str().is_none() {
        return bad_request("插件声明缺 capability");
    }
    if body["wasm"].as_str().is_none() {
        return bad_request("插件声明缺 wasm");
    }
    let file = plugins_file(&cfg);
    let mut items = respond_or_fail!(read_plugins(&file), internal);
    let cap = body["capability"].as_str().unwrap_or_default().to_string();
    if let Some(slot) = items
        .iter_mut()
        .find(|p| p["capability"].as_str() == Some(&cap))
    {
        *slot = body.clone();
    } else {
        items.push(body.clone());
    }
    respond_or_fail!(write_plugins(&file, &items), internal);
    let note = reload_plugins(&cfg).await;
    Json(json!({ "ok": true, "note": note })).into_response()
}

/// DELETE /admin/plugins/{capability}:删除一条插件声明并热摘除其能力。
pub async fn plugins_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(capability): AxumPath<String>,
) -> Response {
    let file = plugins_file(&cfg);
    let mut items = respond_or_fail!(read_plugins(&file), internal);
    let before = items.len();
    items.retain(|p| p["capability"].as_str() != Some(&capability));
    if items.len() == before {
        return not_found("插件不存在");
    }
    respond_or_fail!(write_plugins(&file, &items), internal);
    let note = reload_plugins(&cfg).await;
    Json(json!({ "ok": true, "note": note })).into_response()
}

/// POST /admin/plugins/reload:按当前声明强制重载(手工保险,同 MCP reload)。
pub async fn plugins_reload(State(cfg): State<AdminConfig>) -> Response {
    let note = reload_plugins(&cfg).await;
    Json(json!({ "ok": true, "note": note })).into_response()
}
