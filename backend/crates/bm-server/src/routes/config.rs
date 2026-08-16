//! 配置：读取 / 全量保存（含校验与 skills 目录同步）。

use axum::{Json, extract::State, http::StatusCode};
use bm_core::AppConfig;

use crate::{ApiResult, api_error};

pub async fn get_config(State(state): crate::SharedState) -> Json<AppConfig> {
    let mut config = state.config.read().expect("config poisoned").clone();
    for provider in &mut config.providers {
        if let Some(key) = provider.api_key.as_deref().filter(|s| !s.is_empty()) {
            provider.api_key = Some(bm_core::plugin_settings::mask_secret(key));
        }
    }
    Json(config)
}

pub async fn put_config(
    State(state): crate::SharedState,
    Json(mut config): Json<AppConfig>,
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
    if let Some(default_id) = &config.default_provider
        && !config.providers.iter().any(|p| &p.id == default_id) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                format!("默认提供商不存在: {default_id}"),
            ));
        }

    // GET 回传的是掩码：提交掩码或空串视为「未改」，保留服务端原值。
    {
        let current = state.config.read().expect("config poisoned");
        for provider in &mut config.providers {
            let Some(submitted) = provider.api_key.as_deref() else {
                continue;
            };
            let existing = current
                .providers
                .iter()
                .find(|p| p.id == provider.id)
                .and_then(|p| p.api_key.clone())
                .unwrap_or_default();
            if submitted.is_empty()
                || submitted == bm_core::plugin_settings::mask_secret(&existing)
            {
                provider.api_key = if existing.is_empty() {
                    None
                } else {
                    Some(existing)
                };
            }
        }
    }

    // 持久化 + 同步 skills 目录 + 更新内存
    if let Err(err) = bm_core::config::save(&config) {
        return Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("配置保存失败: {err}"),
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
    if let Some(compat) = &state.compat {
        compat.set_policy(crate::compat_engine::extension_policy_from_config(&config));
    }
    *state.config.write().expect("config poisoned") = config;
    crate::bm_engine::invalidate_loop_agents(&state).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
