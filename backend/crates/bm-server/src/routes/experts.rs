//! 专家预设管理 API（设置架构 §六）：列表 / 读取 / 写（创建+更新）/ 删除。
//! 专家与 subagent 角色同池（~/.boenmind/agents/*.md），预置专家禁删。

use axum::extract::Path;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::{ApiResult, api_error, api_error_from};

pub async fn list_experts() -> ApiResult<Json<Vec<bm_core::experts::ExpertDef>>> {
    bm_core::experts::list_experts()
        .map(Json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

pub async fn get_expert(
    Path(id): Path<String>,
) -> ApiResult<Json<bm_core::experts::ExpertDef>> {
    bm_core::experts::read_expert(&id)
        .map(Json)
        .map_err(api_error_from)
}

/// PUT /api/experts/{id} — 创建/更新专家预设（id = 文件名；未知 frontmatter 字段保留）。
/// 请求体的 name 与路径 id 不一致时以路径为准（前端编辑时名称锁定）。
#[derive(Deserialize)]
pub struct PutExpertRequest {
    pub description: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub extensions: Option<Vec<String>>,
    #[serde(default)]
    pub memory: Option<String>,
    #[serde(default)]
    pub system_prompt: String,
}

pub async fn put_expert(
    Path(id): Path<String>,
    Json(req): Json<PutExpertRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let def = bm_core::experts::ExpertDef {
        name: id,
        description: req.description,
        model: req.model,
        reasoning: req.reasoning,
        tools: req.tools,
        extensions: req.extensions,
        memory: req.memory,
        system_prompt: req.system_prompt,
        builtin: false, // 写时回填（read 时按名字判定）；此字段仅列表展示用
    };
    bm_core::experts::write_expert(&def).map_err(api_error_from)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_expert(
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    bm_core::experts::delete_expert(&id).map_err(api_error_from)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
