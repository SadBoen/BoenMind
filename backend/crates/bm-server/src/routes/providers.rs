//! 提供商工具：模型列表拉取 / 连接测试（SSRF 校验在 bm_core）/ 思考档位。

use axum::{Json, extract::{Query, State}, http::StatusCode};
use serde::Deserialize;

use crate::{ApiResult, api_error};

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
