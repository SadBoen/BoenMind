//! W10 后台作业列表(ADR-0025):长命令后台转轨的台账查询面。

use super::AdminConfig;
use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// GET /admin/jobs(W10/ADR-0025):后台作业列表(新→旧)。
pub async fn jobs_list(State(cfg): State<AdminConfig>) -> Response {
    let jobs = cfg.jobs.as_ref().map(|j| j.list()).unwrap_or_default();
    Json(json!({ "ok": true, "jobs": jobs })).into_response()
}
