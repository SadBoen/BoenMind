//! REST 路由（按领域拆分）：config / sessions / plugins / skills / providers / workspace / updates / steward / todos / terminal。
//! 共享类型 ApiResult / api_error / SharedState 在 crate 根（lib.rs）。

pub mod apps;
pub mod config;
pub mod experts;
pub mod mcp;
pub mod pdf_omni;
pub mod plugins;
pub mod providers;
pub mod refine;
pub mod sessions;
pub mod skills;
pub mod steward;
pub mod terminal;
pub mod todos;
pub mod updates;
pub mod workspace;

use axum::{Json, extract::State};

use crate::VERSION;

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
