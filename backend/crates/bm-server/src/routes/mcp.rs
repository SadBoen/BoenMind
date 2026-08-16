//! MCP 管理 API（bm-mcp 官方插件设置面）：已连接 server 状态查询、
//! 运行时连接/断开（操作即时生效 + 持久化到 config.toml `mcp` 数组，
//! 重启后仍生效）。

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::{ApiResult, api_error};

#[derive(Serialize)]
pub struct McpServerStatus {
    pub name: String,
    pub transport: String,
    /// 协商成功的协议版本（如 "2026-07-28" / "2025-11-25"）
    pub protocol_version: String,
    /// 该 server 暴露的工具数（模型工具面可见）
    pub tool_count: usize,
}

/// 已连接 MCP server 状态列表。
pub async fn list_servers(State(state): crate::SharedState) -> Json<Vec<McpServerStatus>> {
    let Some(mcp) = &state.mcp else {
        return Json(Vec::new());
    };
    let tools = mcp.tools();
    let servers = mcp.servers();
    let mut out = Vec::new();
    for s in servers {
        let count = tools.iter().filter(|t| t.server_name == s.name).count();
        out.push(McpServerStatus {
            name: s.name,
            transport: format!("{:?}", s.transport).to_lowercase(),
            protocol_version: s.protocol_version,
            tool_count: count,
        });
    }
    Json(out)
}

/// 运行时连接一个 MCP server（stdio / streamable HTTP），并持久化配置。
pub async fn connect_server(
    State(state): crate::SharedState,
    Json(config): Json<bm_mcp::McpServerConfig>,
) -> ApiResult<Json<serde_json::Value>> {
    let Some(mcp) = &state.mcp else {
        return Err(api_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "MCP 插件未启用（bm-server 未配置 mcp）",
        ));
    };
    mcp.connect_server(config.clone())
        .await
        .map_err(|err| api_error(axum::http::StatusCode::BAD_REQUEST, err))?;

    // 持久化：合并进 config.mcp 数组（同名覆盖）后写盘
    persist_servers(&state, config).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// 断开一个 MCP server（即时生效；配置保留——重连可用）。
pub async fn disconnect_server(
    State(state): crate::SharedState,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<Json<serde_json::Value>> {
    let name = body
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| api_error(axum::http::StatusCode::BAD_REQUEST, "缺少 name"))?;
    let Some(mcp) = &state.mcp else {
        return Err(api_error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "MCP 插件未启用",
        ));
    };
    mcp.disconnect_server(name)
        .await
        .map_err(|err| api_error(axum::http::StatusCode::BAD_REQUEST, err))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// 把 server 配置合并进 config.toml 的 `mcp` 数组（同名覆盖）并写盘。
async fn persist_servers(
    state: &crate::AppState,
    config: bm_mcp::McpServerConfig,
) -> ApiResult<()> {
    let mut app_config = state.config.write().await;
    let mut servers: Vec<bm_mcp::McpServerConfig> = app_config
        .mcp
        .clone()
        .and_then(|v| serde_json::from_value::<Vec<bm_mcp::McpServerConfig>>(v).ok())
        .unwrap_or_default();
    servers.retain(|s| s.name != config.name);
    servers.push(config);
    app_config.mcp = Some(serde_json::to_value(servers).map_err(|err| {
        api_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("配置序列化失败: {err}"),
        )
    })?);
    bm_core::config::save(&app_config).map_err(|err| {
        api_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("配置写入失败: {err}"),
        )
    })
}
