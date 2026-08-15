//! 插件：列表 / 启停 / 安装 / 设置页与用量（quota 路径经 safe_join 校验）。

use axum::{Json, extract::State, http::StatusCode};
use bm_core::workspace;
use serde::Deserialize;
use serde_json::Value;

use crate::{ApiResult, api_error, api_error_from};

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
    {
        let mut config = state.config.write().await;
        bm_core::plugins::set_plugin_enabled(&mut config, &id, req.enabled)
            .map_err(api_error_from)?;
    }
    // 启用 → 增量加载插件并失效会话 agent（当前对话下一条消息即见新工具）；
    // 禁用无运行时卸载路径，工具面保留至服务重启（compat_engine.rs reload 注释）
    if req.enabled {
        if let Some(compat) = &state.compat {
            let config = state.config.read().await;
            compat.reload(&config).await;
        }
        crate::bm_engine::invalidate_loop_agents(&state).await;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn uninstall_plugin(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut config = state.config.write().await;
    bm_core::plugins::uninstall_plugin(&mut config, &id)
        .map_err(api_error_from)?;
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
        .map_err(api_error_from)?;
    // 安装后默认禁用，由用户在 UI 启用
    Ok(Json(info))
}

#[derive(Deserialize)]
pub struct InstallSourceRequest {
    /// 包源：npm:包名 / git:host/owner/repo[@ref] / 本地路径
    pub source: String,
}

/// POST /api/plugins/install-source — 按包源安装插件（复用上游包管理器，
/// 装到全局后把包内扩展资源复制进插件根目录）。npm 安装耗时较长，放阻塞线程执行。
pub async fn install_plugin_from_source(
    Json(req): Json<InstallSourceRequest>,
) -> ApiResult<Json<Vec<bm_core::plugins::PluginInfo>>> {
    let source = req.source;
    let infos = tokio::task::spawn_blocking(move || {
        bm_core::plugins::install_plugin_from_source(&source)
    })
    .await
    .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("安装任务失败: {err}")))?
    .map_err(api_error_from)?;
    // 安装后默认禁用，由用户在 UI 启用
    Ok(Json(infos))
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
    let entry = quota.get_mut(source)?;
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
        .map_err(api_error_from)?;
    let settings = bm_core::plugin_settings::read_settings_masked(&id, &schema);
    Ok(Json(serde_json::json!({ "ok": true, "settings": settings })))
}
