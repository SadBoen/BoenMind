//! 每软件 APP 专属配置 API（设置架构 §五）：单源 config.toml 的 `[apps.<id>]`
//! 段——专家绑定 / 记忆桶 / 工作区覆盖。底层 LLM 交互引擎所有 APP 共用一套，
//! 此处只是各 APP 的偏好配置。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::{ApiResult, api_error};

#[derive(Deserialize)]
pub struct PutAppProfileRequest {
    /// 该 APP 默认专家预设 id（None/空 = 解除绑定）
    #[serde(default)]
    pub expert: Option<String>,
    /// 记忆桶（None/空 = APP 默认）
    #[serde(default)]
    pub memory: Option<String>,
    /// 工作目录覆盖（None/空 = 全局配置/项目切换）
    #[serde(default)]
    pub working_dir: Option<String>,
}

/// PUT /api/apps/{id} — 更新某 APP 的专属配置（全量覆盖语义；空字段清空对应项）。
pub async fn put_app_profile(
    State(state): crate::SharedState,
    Path(app_id): Path<String>,
    Json(req): Json<PutAppProfileRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    if app_id.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "app id 不能为空"));
    }
    let mut config = state.config.write().expect("config poisoned");
    let profile = config.apps.entry(app_id.clone()).or_default();
    profile.expert = req.expert.filter(|s| !s.is_empty());
    profile.memory = req.memory.filter(|s| !s.is_empty());
    profile.working_dir = req.working_dir.filter(|s| !s.is_empty());
    if profile.expert.is_none() && profile.memory.is_none() && profile.working_dir.is_none() {
        config.apps.remove(&app_id);
    }
    bm_core::config::save(&config).map_err(|err| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("配置写入失败: {err}"),
        )
    })?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
