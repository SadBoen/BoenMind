//! 管家（Steward）路由：OS 层主动汇报通道 + 状态查询（架构 §14.1 三件套 ②）。

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::AppState;

/// OS 层汇报请求：`message` 事件内容；`wake_after_seconds` 可选，
/// 汇报同时登记下次唤醒（治理层夹 [pacing-min, pacing-max]）。
#[derive(Debug, Deserialize)]
pub struct InjectRequest {
    pub message: String,
    #[serde(default)]
    pub wake_after_seconds: Option<i64>,
}

/// 事件 → 管家回合（Inject 源）：立即投喂一个回合，可顺带自调节奏。
pub async fn inject(
    State(state): State<AppState>,
    Json(req): Json<InjectRequest>,
) -> Response {
    let message = req.message.trim().to_string();
    if message.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "message 不能为空".to_string(),
        )
            .into_response();
    }
    let Some(store) = state.steward.clone() else {
        return (
            StatusCode::BAD_REQUEST,
            "管家未启用（BM_STEWARD_SESSION 未设置）".to_string(),
        )
            .into_response();
    };
    match crate::bm_engine::steward_inject(state, store, message, req.wake_after_seconds).await {
        Ok(()) => Json(serde_json::json!({ "status": "ok" })).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, err).into_response(),
    }
}

/// 管家状态（调试 / 前端展示：谁在当管家、下次唤醒时间、治理区间）。
pub async fn status(State(state): State<AppState>) -> Response {
    let Some(store) = state.steward.clone() else {
        return Json(serde_json::json!({
            "enabled": false,
        }))
        .into_response();
    };
    let snap = store.snapshot().await;
    let (min_s, max_s) = store.pacing().await;
    Json(serde_json::json!({
        "enabled": true,
        "sessionId": snap.session_id,
        "nextWakeAtMs": snap.next_wake_at_ms,
        "lastWakeAtMs": snap.last_wake_at_ms,
        "lastReason": snap.last_reason,
        "inFlight": snap.in_flight,
        "pacingMinS": min_s,
        "pacingMaxS": max_s,
    }))
    .into_response()
}
