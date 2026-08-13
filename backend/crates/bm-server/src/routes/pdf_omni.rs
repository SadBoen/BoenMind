//! pdf-omni 插件宿主端点：`POST /api/plugins/pdf-omni/parse`。
//!
//! TS 薄壳插件（QuickJS 沙箱）经 loopback 调用本端点，把全部重活（流式上传、
//! 轮询、PDF 操作、级联、验证）交给 Rust 核心 `crate::pdf_omni`。
//! - API keys 从插件设置文件 `~/.boenmind/extensions/pdf-omni/settings.json` 读取
//!   （设置页 secret 字段，单源；不随请求在 loopback 上传）
//! - 本地文件路径用 `bm_core::workspace::safe_join` 校验（拒绝越界/..）
//! - 鉴权由全局 auth_middleware 覆盖（BOENMIND_TOKEN）

use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;

use crate::{ApiResult, api_error};

/// 设置文件中 API key 的字段名（与 TS 插件 extension.json settings 声明对齐；
/// sources. 前缀与设置页 testSources 的 {sources.xxx.apiKey} 占位引用一致）。
const KEY_MINERU: &str = "sources.mineru.apiKey";
const KEY_LLAMAPARSE: &str = "sources.llamaparse.apiKey";
const KEY_LLAMAPARSE2: &str = "sources.llamaparse.apiKey2";

#[derive(Debug, Deserialize)]
pub struct ParsePdfBody {
    #[serde(flatten)]
    pub req: crate::pdf_omni::ParsePdfRequest,
}

/// POST /api/plugins/pdf-omni/parse — 统一解析入口（工具约定：错误也返回 JSON，HTTP 200）。
pub async fn parse_pdf(
    State(state): crate::SharedState,
    Json(body): Json<ParsePdfBody>,
) -> ApiResult<Json<crate::pdf_omni::ParsePdfResult>> {
    let config = state.config.read().await;
    let workspace_root = config.working_dir.clone();
    drop(config);

    // 从插件设置文件读 API keys（单源：设置页写入处）
    let keys = read_plugin_keys();
    let result = crate::pdf_omni::parse_pdf_any(&body.req, &keys, &workspace_root).await;
    Ok(Json(result))
}

/// 读取插件设置文件中的 API keys（平铺 JSON；无设置文件 → 空）。
fn read_plugin_keys() -> crate::pdf_omni::EngineKeys {
    let mut keys = crate::pdf_omni::EngineKeys::default();
    let settings_path = bm_core::config::app_dir()
        .join("extensions")
        .join("pdf-omni")
        .join("settings.json");
    let Ok(text) = std::fs::read_to_string(settings_path) else {
        return keys;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return keys;
    };
    keys.mineru = v.get(KEY_MINERU).and_then(|s| s.as_str()).unwrap_or("").to_string();
    let k1 = v.get(KEY_LLAMAPARSE).and_then(|s| s.as_str()).unwrap_or("").to_string();
    let k2 = v.get(KEY_LLAMAPARSE2).and_then(|s| s.as_str()).unwrap_or("").to_string();
    keys.llamaparse = [k1, k2].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(",");
    keys
}

/// 探测响应（设置页"测试按钮"辅助：确认 keys 已配置且可连到端点）。
pub async fn probe(
    State(state): crate::SharedState,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let keys = read_plugin_keys();
    let _ = state;
    let mineru_ok = !keys.mineru.is_empty();
    let llamaparse_ok = !keys.llamaparse.is_empty();
    if !mineru_ok && !llamaparse_ok {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "未配置任何 API key（插件设置页）",
        ));
    }
    Ok(Json(serde_json::json!({
        "ok": true,
        "mineru_configured": mineru_ok,
        "llamaparse_configured": llamaparse_ok,
        "note": "仅确认 key 已配置；真实连通性以 parse_pdf 调用为准（设置页测试走 testSources 探测）",
    })))
}
