//! Skills：列表 / 启停 / 安装（GitHub 与本地）/ 随机抽取。

use axum::{Json, extract::{Query, State}, http::StatusCode};
use serde::Deserialize;

use crate::{ApiResult, api_error, api_error_from};

// ---------------------------------------------------------------------------
// Skills
// ---------------------------------------------------------------------------

pub async fn list_skills(State(state): crate::SharedState) -> ApiResult<Json<Vec<bm_core::skills::SkillInfo>>> {
    let config = state.config.read().await;
    bm_core::skills::list_skills(&config)
        .map(Json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

#[derive(Deserialize)]
pub struct SetSkillRequest {
    pub enabled: bool,
}

pub async fn set_skill(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<SetSkillRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut config = state.config.write().await;
    bm_core::skills::set_skill_enabled(&mut config, &id, req.enabled)
        .map_err(api_error_from)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn uninstall_skill(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut config = state.config.write().await;
    bm_core::skills::uninstall_skill(&mut config, &id)
        .map_err(api_error_from)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct InstallSkillRequest {
    /// skills.sh / GitHub 来源：owner + repo + skill_id
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub skill_id: Option<String>,
    /// 本地路径来源（目录或 .md 文件）
    #[serde(default)]
    pub path: Option<String>,
}

pub async fn install_skill(
    State(state): crate::SharedState,
    Json(req): Json<InstallSkillRequest>,
) -> ApiResult<Json<bm_core::skills::SkillInfo>> {
    // 网络下载 + 解压为阻塞操作，放到阻塞线程池
    let owner = req.owner.clone();
    let repo = req.repo.clone();
    let skill_id = req.skill_id.clone();
    let path = req.path.clone();
    let result = tokio::task::spawn_blocking(move || {
        if let (Some(owner), Some(repo), Some(skill_id)) = (owner, repo, skill_id) {
            bm_core::skills::install_skill_from_github(&owner, &repo, &skill_id)
        } else if let Some(path) = path {
            bm_core::skills::install_skill_from_path(std::path::Path::new(&path))
        } else {
            Err(bm_core::AppError::invalid("需要提供 owner/repo/skill_id（skills.sh）或本地 path"))
        }
    })
    .await
    .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    let info = result.map_err(api_error_from)?;
    let _ = state; // 安装后默认禁用，由用户启用
    Ok(Json(info))
}

#[derive(Deserialize)]
pub struct RandomSkillsParams {
    /// 抽取数量，默认 5，上限 20
    #[serde(default = "default_random_count")]
    pub count: usize,
}

fn default_random_count() -> usize {
    5
}

pub async fn random_skills(
    Query(params): Query<RandomSkillsParams>,
) -> ApiResult<Json<Vec<bm_core::skills::SkillCandidate>>> {
    let candidates = tokio::task::spawn_blocking(move || bm_core::skills::random_skills(params.count))
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .map_err(api_error_from)?;
    Ok(Json(candidates))
}
