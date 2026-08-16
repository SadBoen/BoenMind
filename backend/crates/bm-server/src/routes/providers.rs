//! 提供商工具：模型列表拉取 / 连接测试（SSRF 校验在 bm_core）/ 思考档位。

use axum::{Json, extract::{Query, State}, http::StatusCode};
use bm_core::config::{ProviderKind, ProviderShape};
use serde::Deserialize;

use crate::{ApiResult, api_error, api_error_from};

/// 请求体中的 kind 字符串解析为枚举：拼写/大小写错误在此显式 400，
/// 不会静默落入 custom 语义（旧签名收裸字符串时的问题）。
fn parse_kind(kind: &str) -> ApiResult<ProviderKind> {
    kind.parse()
        .map_err(|err: String| api_error(StatusCode::BAD_REQUEST, err))
}

/// 请求体中的协议形状（custom/未知厂商有效；内置厂商形状固定，
/// 传了也按内置形状处理——ProviderConfig::shape 是权威）。
fn parse_shape(shape: Option<&str>) -> ApiResult<Option<ProviderShape>> {
    match shape {
        None | Some("") => Ok(None),
        Some(s) => serde_json::from_value::<ProviderShape>(serde_json::Value::String(s.into()))
            .map(Some)
            .map_err(|_| api_error(StatusCode::BAD_REQUEST, format!("未知协议形状: {s}"))),
    }
}

/// 拉取模型列表请求体（表单中临时填写的端点与 key，不落盘）。
#[derive(Deserialize)]
pub struct ListModelsRequest {
    pub kind: String,
    /// 协议形状（custom 有效；内置厂商忽略）
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

pub async fn list_provider_models(
    Json(req): Json<ListModelsRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let kind = parse_kind(&req.kind)?;
    let shape = parse_shape(req.shape.as_deref())?.unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || {
        bm_core::providers::list_provider_models(kind, shape, &req.base_url, &req.api_key)
    })
    .await
    .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    match result {
        Ok(models) => Ok(Json(serde_json::json!({ "models": models }))),
        Err(err) => Err(api_error_from(err)),
    }
}

/// 测试连接请求体。`message` 为空 → 仅连通测试；非空 → 发送真实对话请求。
#[derive(Deserialize)]
pub struct TestProviderRequest {
    pub kind: String,
    /// 协议形状（custom 有效；内置厂商忽略）
    #[serde(default)]
    pub shape: Option<String>,
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
    let kind = parse_kind(&req.kind)?;
    let shape = parse_shape(req.shape.as_deref())?.unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || {
        bm_core::providers::test_provider_connection(
            kind,
            shape,
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
        Err(err) => Err(api_error_from(err)),
    }
}
// ---------------------------------------------------------------------------
// 官方端点表下发（GET /api/providers/presets）
// ---------------------------------------------------------------------------

/// 内置厂商官方端点表下发（前端设置页预填表单）。端点数据只在
/// bm_core::providers::official_base_url 维护一份（= ProviderPort 注册表
/// 数据源，同一张表），前端拉取后合并进本地预设（拉取失败用本地值兜底，
/// 见 provider-presets.tsx）。厂商精简后清单自动缩小（minimax/deepseek/custom）。
pub async fn presets() -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "presets": bm_core::providers::official_base_urls(),
    })))
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
/// 判定逻辑在 bm_core::thinking（按协议形状 + 模型名白名单）。
pub async fn thinking_levels(
    State(state): crate::SharedState,
    Query(params): Query<ThinkingLevelsParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let config = state.config.read().expect("config poisoned");
    let provider = config
        .providers
        .iter()
        .find(|p| p.id == params.provider)
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "provider 不存在"))?;
    let levels = bm_core::thinking::thinking_levels_for(
        provider.shape(),
        provider.kind,
        provider.base_url.as_deref(),
        &params.model,
    );
    Ok(Json(serde_json::json!({ "levels": levels })))
}
