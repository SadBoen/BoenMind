//! Skills：列表 / 启停 / 安装（GitHub 与本地）/ 随机抽取。

use axum::{Json, extract::{Query, State}, http::StatusCode};
use serde::Deserialize;

use crate::{ApiResult, api_error, api_error_from};

// ---------------------------------------------------------------------------
// Skills
// ---------------------------------------------------------------------------

pub async fn list_skills(State(state): crate::SharedState) -> ApiResult<Json<Vec<bm_core::skills::SkillInfo>>> {
    // 技能面（SERVICE_FACES #11）：kernel 可用时经服务；退化直调
    if let Some(kernel) = &state.kernel
        && let Ok(port) = kernel.port::<dyn bm_protocol::SkillPort>("skill")
    {
        let list = port
            .list()
            .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        let mut infos: Vec<bm_core::skills::SkillInfo> = serde_json::from_value(list)
            .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        // 服务面返回不含设置 schema（kernel 不感知扩展设置），统一在此补齐
        for info in &mut infos {
            if info.settings_schema.is_none() {
                info.settings_schema = bm_core::skills::skill_settings_schema(&info.id);
            }
        }
        return Ok(Json(infos));
    }
    let config = state.config.read().await;
    bm_core::skills::list_skills(&config)
        .map(Json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

/// GET /api/skills/{id}/settings — 读取 skill 设置（schema + 掩码值回显）。
/// skill 无 settings.json 声明 → 404（前端据此不显示"设置"入口）。
pub async fn get_skill_settings(
    State(_state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let schema = bm_core::skills::skill_settings_schema(&id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "skill 未安装或无设置声明"))?;
    let settings = bm_core::skills::read_skill_settings_masked(&id, &schema);
    Ok(Json(serde_json::json!({ "settings": settings })))
}

#[derive(serde::Deserialize)]
pub struct PutSkillSettingsRequest {
    /// 扁平 {key: value}；secret 字段提交掩码/空 = 保留原值
    pub values: serde_json::Value,
}

#[derive(serde::Deserialize)]
pub struct PutSkillScopeRequest {
    /// 生效 APP 列表（空 = 公共；含 "*" = 公共；["chat"] = 仅聊天）
    pub scopes: Vec<String>,
}

/// PUT /api/skills/{id}/scope — 设置 skill 作用域（config.toml skill_scopes 覆盖；
/// 注入面 = system prompt 的 available_skills 块按 session.app 过滤）。
pub async fn put_skill_scope(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<PutSkillScopeRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    {
        let config = state.config.read().await;
        bm_core::skills::list_skills(&config)
            .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "skill 未安装"))?;
    }
    let scopes: Vec<String> = req
        .scopes
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "*")
        .collect();
    let mut config = state.config.write().await;
    if scopes.is_empty() {
        config.skill_scopes.remove(&id);
    } else {
        config.skill_scopes.insert(id, scopes);
    }
    bm_core::config::save(&config).map_err(|err| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("配置写入失败: {err}"),
        )
    })?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// PUT /api/skills/{id}/settings — 保存 skill 设置（语义同插件设置）。
pub async fn put_skill_settings(
    State(_state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<PutSkillSettingsRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let schema = bm_core::skills::skill_settings_schema(&id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "skill 未安装或无设置声明"))?;
    let saved = bm_core::skills::save_skill_settings(&id, &schema, &req.values).map_err(api_error_from)?;
    Ok(Json(serde_json::json!({ "ok": true, "settings": saved })))
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
    if let Some(kernel) = &state.kernel
        && let Ok(port) = kernel.port::<dyn bm_protocol::SkillPort>("skill")
    {
        port.set_enabled(&id, req.enabled)
            .map_err(|err| api_error(StatusCode::NOT_FOUND, err.to_string()))?;
    } else {
        let mut config = state.config.write().await;
        bm_core::skills::set_skill_enabled(&mut config, &id, req.enabled).map_err(api_error_from)?;
    }
    // 启停改变 skill 注入面：失效会话 agent，当前对话下一条消息即按新配置重建
    crate::bm_engine::invalidate_loop_agents(&state).await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn uninstall_skill(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    if let Some(kernel) = &state.kernel
        && let Ok(port) = kernel.port::<dyn bm_protocol::SkillPort>("skill")
    {
        port.uninstall(&id)
            .map_err(|err| api_error(StatusCode::NOT_FOUND, err.to_string()))?;
    } else {
        let mut config = state.config.write().await;
        bm_core::skills::uninstall_skill(&mut config, &id).map_err(api_error_from)?;
    }
    crate::bm_engine::invalidate_loop_agents(&state).await;
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
    State(_state): crate::SharedState,
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
    // 安装后默认禁用，由用户启用；启用走 set_skill 失效重建（此处只卸载/安装
    // 不重建——禁用状态不改变注入面）
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
