//! REST 路由：健康检查、配置、会话 CRUD、工作文件夹。

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use bm_core::{AppConfig, workspace};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{ApiResult, VERSION, api_error};

// ---------------------------------------------------------------------------
// 健康检查
// ---------------------------------------------------------------------------

pub async fn health(State(state): crate::SharedState) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    Json(serde_json::json!({
        "status": "ok",
        "version": VERSION,
        "workingDir": config.working_dir,
        "providers": config.providers.len(),
        "theme": config.theme,
        "lang": config.lang,
    }))
}

// ---------------------------------------------------------------------------
// 配置
// ---------------------------------------------------------------------------

pub async fn get_config(State(state): crate::SharedState) -> Json<AppConfig> {
    Json(state.config.read().await.clone())
}

pub async fn put_config(
    State(state): crate::SharedState,
    Json(config): Json<AppConfig>,
) -> ApiResult<Json<serde_json::Value>> {
    // 基本校验：提供商 id 唯一
    let mut seen = std::collections::HashSet::new();
    for p in &config.providers {
        if p.id.trim().is_empty() {
            return Err(api_error(StatusCode::BAD_REQUEST, "提供商 id 不能为空"));
        }
        if !seen.insert(p.id.clone()) {
            return Err(api_error(StatusCode::BAD_REQUEST, format!("提供商 id 重复: {}", p.id)));
        }
    }
    if let Some(default_id) = &config.default_provider {
        if !config.providers.iter().any(|p| &p.id == default_id) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                format!("默认提供商不存在: {default_id}"),
            ));
        }
    }

    // 持久化 + 同步 pi models.json + 更新内存
    if let Err(err) = bm_core::config::save(&config) {
        return Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("配置保存失败: {err}"),
        ));
    }
    if let Err(err) = bm_core::config::sync_pi_models_json(&config) {
        return Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("pi models.json 同步失败: {err}"),
        ));
    }
    // 直接替换 enabled_skills 的场景（如前端设置页全量保存）也要同步 pi/skills
    // 目录，否则注入提示与实际加载源漂移，agent 读不到 skill
    if let Err(err) = bm_core::skills::sync_skills_to_pi(&config) {
        return Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("skills 目录同步失败: {err}"),
        ));
    }
    let _ = bm_core::config::ensure_working_dir(&config);
    *state.config.write().await = config;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// 会话
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

pub async fn create_session(
    State(state): crate::SharedState,
    Json(req): Json<CreateSessionRequest>,
) -> ApiResult<Json<bm_core::db::Session>> {
    let id = Uuid::new_v4().to_string();
    let session = state
        .db
        .create_session(&id, req.provider_id.as_deref(), req.model.as_deref())
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let mut session = session;
    if let Some(title) = req.title {
        if !title.trim().is_empty() {
            state
                .db
                .rename_session(&session.id, title.trim())
                .await
                .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
            session.title = title.trim().to_string();
        }
    }
    Ok(Json(session))
}

pub async fn list_sessions(State(state): crate::SharedState) -> ApiResult<Json<Vec<bm_core::db::Session>>> {
    state
        .db
        .list_sessions()
        .await
        .map(Json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

pub async fn get_session(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let session = state
        .db
        .get_session(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, format!("会话不存在: {id}")))?;
    let messages = state
        .db
        .list_messages(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(serde_json::json!({
        "session": session,
        "messages": messages,
    })))
}

#[derive(Deserialize)]
pub struct RenameSessionRequest {
    pub title: String,
}

pub async fn rename_session(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<RenameSessionRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let title = req.title.trim().to_string();
    if title.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "标题不能为空"));
    }
    state
        .db
        .rename_session(&id, &title)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_session(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let rows = state
        .db
        .delete_session(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if rows == 0 {
        return Err(api_error(StatusCode::NOT_FOUND, format!("会话不存在: {id}")));
    }
    // 清理对应的 agent 会话句柄
    state.agents.lock().await.remove(&id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// 插件
// ---------------------------------------------------------------------------

pub async fn list_plugins(State(state): crate::SharedState) -> ApiResult<Json<Vec<bm_core::plugins::PluginInfo>>> {
    let config = state.config.read().await;
    bm_core::plugins::list_plugins(&config)
        .map(Json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

#[derive(Deserialize)]
pub struct SetPluginRequest {
    pub enabled: bool,
}

pub async fn set_plugin(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<SetPluginRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut config = state.config.write().await;
    bm_core::plugins::set_plugin_enabled(&mut config, &id, req.enabled)
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, err))?;
    // 插件启停影响 agent 会话创建，需同步 models.json 之外的配置（无需重启）
    drop(config);
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn uninstall_plugin(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut config = state.config.write().await;
    bm_core::plugins::uninstall_plugin(&mut config, &id)
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, err))?;
    drop(config);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct InstallPluginRequest {
    pub path: String,
}

pub async fn install_plugin(
    Json(req): Json<InstallPluginRequest>,
) -> ApiResult<Json<bm_core::plugins::PluginInfo>> {
    let info = bm_core::plugins::install_plugin(std::path::Path::new(&req.path))
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, err))?;
    // 安装后默认禁用，由用户在 UI 启用
    Ok(Json(info))
}

/// 查找插件信息（manifest 的 schema/quota/testSources 均已解析）。
/// 插件不存在 → Ok(None)。
async fn plugin_info(
    state: &crate::AppState,
    id: &str,
) -> Result<Option<bm_core::plugins::PluginInfo>, (StatusCode, axum::Json<serde_json::Value>)> {
    let config = state.config.read().await;
    let plugins = bm_core::plugins::list_plugins(&config)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(plugins.into_iter().find(|p| p.id == id))
}

/// 查找插件的 settings schema（插件不存在或无设置页 → None）。
async fn plugin_settings_schema(
    state: &crate::AppState,
    id: &str,
) -> Result<Option<Vec<bm_core::plugin_settings::SettingField>>, (StatusCode, axum::Json<serde_json::Value>)> {
    Ok(plugin_info(state, id).await?.and_then(|p| p.settings_schema))
}

/// GET /api/plugins/{id}/settings — 读取插件设置（secret 字段掩码回显）。
/// 附加字段 `quota`：若插件 manifest 声明了用量文件（如 web-search 的 quota.json），
/// 一并返回供设置页展示（可选，无声明时为空）。
pub async fn get_plugin_settings(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let info = plugin_info(&state, &id)
        .await?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "插件不存在或无设置页"))?;
    let schema = info
        .settings_schema
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "插件不存在或无设置页"))?;
    let settings = bm_core::plugin_settings::read_settings_masked(&id, &schema);
    let quota = read_plugin_quota(&state, info.quota.as_ref()).await;
    Ok(Json(serde_json::json!({ "settings": settings, "quota": quota })))
}

/// 读取插件用量文件（路径由 manifest `quota.path` 声明，相对工作文件夹）。
/// 由插件自身写入（沙箱 pi.tool("write") 限制在 workspace 内）；读取失败返回 null。
/// 路径经 safe_join 校验：恶意 manifest 声明的 `..`/绝对路径越界被拒绝。
async fn read_plugin_quota(
    state: &crate::AppState,
    quota: Option<&bm_core::plugin_settings::QuotaDecl>,
) -> Option<serde_json::Value> {
    let working_dir = state.config.read().await.working_dir.clone();
    let file = workspace::safe_join(&working_dir, quota?.path.as_str()).ok()?;
    let text = std::fs::read_to_string(&file).ok()?;
    serde_json::from_str::<serde_json::Value>(&text).ok()
}

#[derive(Deserialize)]
pub struct PutPluginSettingsRequest {
    /// 扁平 {key: value}；secret 字段提交掩码/空 = 保留原值
    pub values: serde_json::Value,
}

#[derive(Deserialize)]
pub struct TestSourceRequest {
    /// 源标识：jina / tavily / exa / serper / firecrawl / custom1 …
    pub source: String,
    /// 表单当前值（可选）：测试按钮点击时把未保存的修改一并传给探测逻辑；
    /// secret 字段提交空/掩码 = 用已存原值（与保存语义一致）
    #[serde(default)]
    pub values: Option<serde_json::Value>,
}

/// POST /api/plugins/{id}/test-source — 探测单个搜索源连通性（验证 API Key），
/// 供设置页「测试」按钮使用。用插件设置的明文值发轻量探测请求；
/// 请求体带 values 时优先用表单当前值（secret 掩码/空 = 已存原值）。
/// 测试成功会真实消耗免费额度（如 Tavily 1 次调用），故同步计入用量文件
/// 并随响应返回最新 quota，设置页用量进度条即时刷新。
pub async fn test_plugin_source(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<TestSourceRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let info = plugin_info(&state, &id)
        .await?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "插件不存在或无设置页"))?;
    let schema = info
        .settings_schema
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "插件不存在或无设置页"))?;
    let test_sources = info.test_sources.unwrap_or_default();
    // 明文读取（含密钥，仅服务端内部使用，不回传）
    let mut settings = bm_core::plugin_settings::read_settings(&id, &schema);
    // 用表单当前值覆盖（仅 schema 声明的 key；secret 空/掩码 = 保留原值）
    if let Some(values) = req.values.as_ref().and_then(|v| v.as_object()) {
        for (k, v) in values {
            if settings.get(k).is_none() {
                continue;
            }
            if let Some(s) = v.as_str()
                && let Some(cur) = settings.get(k).and_then(Value::as_str)
                && (s.is_empty() || s == bm_core::plugin_settings::mask_secret(cur))
            {
                continue; // 未修改：用已存原值
            }
            settings[k] = v.clone();
        }
    }
    let source = req.source.clone();
    let result = tokio::task::spawn_blocking(move || {
        bm_core::plugin_test::test_source(&source, &settings, &test_sources)
    })
    .await
    .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    // 测试成功 = 真实消耗 1 次免费额度，计入用量文件（manifest 声明的按次计费源）
    let quota = if result.ok {
        bump_plugin_quota(&state, &req.source, info.quota.as_ref()).await
    } else {
        None
    };
    Ok(Json(serde_json::json!({
        "ok": result.ok,
        "latencyMs": result.latency_ms,
        "detail": result.detail,
        "quota": quota,
    })))
}

/// 用量文件里给某源 +1 次调用（仅 manifest `quota.countOnTest` 声明的按次计费源；
/// 其余源无统计）。返回更新后的完整用量（无声明/无文件时返回 None）。
async fn bump_plugin_quota(
    state: &crate::AppState,
    source: &str,
    quota_decl: Option<&bm_core::plugin_settings::QuotaDecl>,
) -> Option<serde_json::Value> {
    let decl = quota_decl?;
    if !decl.count_on_test.iter().any(|s| s == source) {
        return None;
    }
    let working_dir = state.config.read().await.working_dir.clone();
    let file = workspace::safe_join(&working_dir, decl.path.as_str()).ok()?;
    let mut quota: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).ok()?).ok()?;
    let Some(entry) = quota.get_mut(source) else { return None };
    let is_calls = entry.get("unit").and_then(Value::as_str) == Some("calls");
    let total = entry.get("total").and_then(Value::as_i64).unwrap_or(0);
    let used = entry.get("used").and_then(Value::as_i64).unwrap_or(0);
    if is_calls && total > 0 && used < total {
        entry["used"] = Value::Number((used + 1).into());
    }
    let today = entry.get("callsToday").and_then(Value::as_i64).unwrap_or(0);
    entry["callsToday"] = Value::Number((today + 1).into());
    entry["today"] = Value::String(today_str());
    std::fs::write(&file, serde_json::to_string_pretty(&quota).ok()?).ok()?;
    Some(quota)
}

fn today_str() -> String {
    // UTC 日期键（与插件内 todayStr() 一致）：1970-01-01 起的天数 → ISO 日期
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = now / 86_400;
    // Howard Hinnant civil_from_days：天数 → (y, m, d)
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// PUT /api/plugins/{id}/settings — 保存插件设置（类型校验 + 密钥掩码保留），
/// 返回合并后的掩码版设置供前端刷新。
pub async fn put_plugin_settings(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<PutPluginSettingsRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let schema = plugin_settings_schema(&state, &id)
        .await?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "插件不存在或无设置页"))?;
    bm_core::plugin_settings::save_settings(&id, &schema, &req.values)
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, err))?;
    let settings = bm_core::plugin_settings::read_settings_masked(&id, &schema);
    Ok(Json(serde_json::json!({ "ok": true, "settings": settings })))
}

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
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, err))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn uninstall_skill(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut config = state.config.write().await;
    bm_core::skills::uninstall_skill(&mut config, &id)
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, err))?;
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
            Err("需要提供 owner/repo/skill_id（skills.sh）或本地 path".to_string())
        }
    })
    .await
    .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    let info = result.map_err(|err| api_error(StatusCode::BAD_REQUEST, err))?;
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
        .map_err(|err| api_error(StatusCode::BAD_GATEWAY, err))?;
    Ok(Json(candidates))
}

// ---------------------------------------------------------------------------
// 提供商工具（模型列表拉取 / 连接测试）
// ---------------------------------------------------------------------------

/// 拉取模型列表请求体（表单中临时填写的端点与 key，不落盘）。
#[derive(Deserialize)]
pub struct ListModelsRequest {
    pub kind: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

pub async fn list_provider_models(
    Json(req): Json<ListModelsRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let result = tokio::task::spawn_blocking(move || {
        bm_core::providers::list_provider_models(&req.kind, &req.base_url, &req.api_key)
    })
    .await
    .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    match result {
        Ok(models) => Ok(Json(serde_json::json!({ "models": models }))),
        Err(msg) => Err(api_error(StatusCode::BAD_GATEWAY, msg)),
    }
}

/// 测试连接请求体。`message` 为空 → 仅连通测试；非空 → 发送真实对话请求。
#[derive(Deserialize)]
pub struct TestProviderRequest {
    pub kind: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub message: String,
}

pub async fn test_provider(
    Json(req): Json<TestProviderRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let result = tokio::task::spawn_blocking(move || {
        bm_core::providers::test_provider_connection(
            &req.kind,
            &req.base_url,
            &req.api_key,
            &req.model,
            &req.message,
        )
    })
    .await
    .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    match result {
        Ok(detail) => Ok(Json(serde_json::json!({ "ok": true, "detail": detail }))),
        Err(msg) => Err(api_error(StatusCode::BAD_GATEWAY, msg)),
    }
}

// ---------------------------------------------------------------------------
// 工作文件夹
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListWorkspaceParams {
    #[serde(default)]
    pub dir: String,
}

pub async fn list_workspace(
    State(state): crate::SharedState,
    Query(params): Query<ListWorkspaceParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().await;
    let root = config.working_dir.clone();
    drop(config);
    match workspace::list_dir(&root, &params.dir) {
        Ok(entries) => Ok(Json(serde_json::json!({
            "dir": params.dir,
            "entries": entries,
        }))),
        Err(workspace::WorkspaceError::OutsideRoot(msg)) => {
            Err(api_error(StatusCode::BAD_REQUEST, format!("路径越界: {msg}")))
        }
        Err(err) => Err(api_error(StatusCode::BAD_REQUEST, err.to_string())),
    }
}

#[derive(Deserialize)]
pub struct ReadFileParams {
    pub path: String,
}

/// 读取工作文件夹内文件。文本文件返回 UTF-8 内容，二进制（图片/PDF）返回 base64。
pub async fn read_workspace_file(
    State(state): crate::SharedState,
    Query(params): Query<ReadFileParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().await;
    let root = config.working_dir.clone();
    drop(config);

    let bytes = match workspace::read_file(&root, &params.path) {
        Ok(b) => b,
        Err(workspace::WorkspaceError::OutsideRoot(msg)) => {
            return Err(api_error(StatusCode::BAD_REQUEST, format!("路径越界: {msg}")));
        }
        Err(err) => {
            return Err(api_error(StatusCode::BAD_REQUEST, err.to_string()));
        }
    };
    let name = params
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&params.path)
        .to_string();
    let mime = workspace::mime_for(&name);

    if workspace::is_text(mime) {
        match String::from_utf8(bytes) {
            Ok(content) => Ok(Json(serde_json::json!({
                "name": name,
                "path": params.path,
                "mime": mime,
                "kind": "text",
                "content": content,
                "size": content.len(),
            }))),
            Err(_) => Err(api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "文件不是合法的 UTF-8 文本",
            )),
        }
    } else {
        Ok(Json(serde_json::json!({
            "name": name,
            "path": params.path,
            "mime": mime,
            "kind": "binary",
            "content": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes),
            "size": bytes.len(),
        })))
    }
}

// ---------------------------------------------------------------------------
// 思考档位
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ThinkingLevelsParams {
    /// 提供商 id（config.providers[].id）
    pub provider: String,
    pub model: String,
}

/// 查询某模型的思考档位（GET /api/thinking-levels?provider=&model=）。
/// 判定逻辑在 bm_core::thinking（复刻 pi 运行时白名单），pi 仍是最终权威。
pub async fn thinking_levels(
    State(state): crate::SharedState,
    Query(params): Query<ThinkingLevelsParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().await;
    let provider = config
        .providers
        .iter()
        .find(|p| p.id == params.provider)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "provider 不存在"))?;
    let levels = bm_core::thinking::thinking_levels_for(
        provider.kind,
        provider.base_url.as_deref(),
        &params.model,
    );
    Ok(Json(serde_json::json!({ "levels": levels })))
}
