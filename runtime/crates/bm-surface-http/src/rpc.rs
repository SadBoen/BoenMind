//! /rpc/{method} 端点:信封逐字节复用(M3 规格 §4)。
//! 业务结果(含信封内 error)恒 HTTP 200;仅 400/401/404/503 走传输状态码。

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use bm_contract::wire::{
    CancelParams, EventsPollParams, GetOperationParams, Method, RequestEnvelope, ResponseEnvelope,
    SendInputParams, SessionCloseParams, SessionCreateParams, SessionDeleteParams,
    SessionResumeParams,
};
use bm_core::CoreResult;
use serde_json::Value;

/// 应用层优雅停机(需鉴权):通知宿主排空(INV-12)后退出。
pub async fn shutdown_endpoint(State(state): State<AppState>) -> impl IntoResponse {
    state.shutdown.notify_waiters();
    Json(serde_json::json!({"ok": true, "draining": true}))
}

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "state": "running",
    }))
}

fn params<T: serde::de::DeserializeOwned>(req: &RequestEnvelope) -> CoreResult<T> {
    serde_json::from_value(req.params.clone())
        .map_err(|e| bm_core::CoreError::validation(format!("参数不合法: {e}")))
}

fn to_value<T: serde::Serialize>(r: CoreResult<T>) -> CoreResult<Value> {
    r.and_then(|v| serde_json::to_value(v).map_err(|_| bm_core::CoreError::Internal))
}

/// 内层分发:全部业务错误经 `?` 汇入 CoreResult。
async fn rpc_inner(state: &AppState, method: Method, req: &RequestEnvelope) -> CoreResult<Value> {
    match method {
        Method::SessionCreate => {
            let p: SessionCreateParams = params(req)?;
            to_value(state.handle.session_create(req.request_id.clone(), p).await)
        }
        Method::SessionResume => {
            let p: SessionResumeParams = params(req)?;
            to_value(state.handle.session_resume(req.request_id.clone(), p).await)
        }
        Method::SessionClose => {
            let p: SessionCloseParams = params(req)?;
            to_value(state.handle.session_close(req.request_id.clone(), p).await)
        }
        Method::SessionDelete => {
            let p: SessionDeleteParams = params(req)?;
            to_value(state.handle.session_delete(req.request_id.clone(), p).await)
        }
        Method::EventsPoll => {
            let p: EventsPollParams = params(req)?;
            to_value(state.handle.events_poll(p).await)
        }
        Method::AgentSendInput => {
            let p: SendInputParams = params(req)?;
            to_value(state.handle.send_input(req.request_id.clone(), p).await)
        }
        Method::CapabilityCancel => {
            let p: bm_contract::wire::CapabilityCancelParams = params(req)?;
            to_value(
                state
                    .handle
                    .capability_cancel(req.request_id.clone(), p)
                    .await,
            )
        }
        Method::AgentCancel => {
            let p: CancelParams = params(req)?;
            to_value(state.handle.agent_cancel(p).await)
        }
        Method::OperationsGet => {
            let p: GetOperationParams = params(req)?;
            to_value(state.handle.operations_get(p).await)
        }
        // M4:能力与审批方法(T3b 起 Runtime 已接线)
        Method::CapabilityCall => {
            let p: bm_contract::wire::CapabilityCallParams = params(req)?;
            to_value(
                state
                    .handle
                    .capability_call(req.request_id.clone(), p)
                    .await,
            )
        }
        Method::CapabilityList => {
            let p: bm_contract::wire::CapabilityListParams = params(req)?;
            to_value(state.handle.capability_list(p).await)
        }
        Method::ApprovalList => {
            let p: bm_contract::wire::ApprovalListParams = params(req)?;
            to_value(state.handle.approval_list(p).await)
        }
        Method::ApprovalRespond => {
            let p: bm_contract::wire::ApprovalRespondParams = params(req)?;
            to_value(
                state
                    .handle
                    .approval_respond(req.request_id.clone(), p)
                    .await,
            )
        }
        // M5:task 六方法(T2 起服务端实现)
        Method::TaskCreate => {
            let p: bm_contract::wire::TaskCreateParams = params(req)?;
            to_value(state.handle.task_create(req.request_id.clone(), p).await)
        }
        Method::TaskList => {
            let p: bm_contract::wire::TaskListParams = params(req)?;
            to_value(state.handle.task_list(p).await)
        }
        Method::TaskAutorun => {
            let p: bm_contract::wire::TaskAutorunParams = params(req)?;
            to_value(state.handle.task_autorun(req.request_id.clone(), p).await)
        }
        Method::TaskGet => {
            let p: bm_contract::wire::TaskGetParams = params(req)?;
            to_value(state.handle.task_get(p).await)
        }
        Method::TaskPause => {
            let p: bm_contract::wire::TaskLifecycleParams = params(req)?;
            to_value(state.handle.task_pause(req.request_id.clone(), p).await)
        }
        Method::TaskResume => {
            let p: bm_contract::wire::TaskLifecycleParams = params(req)?;
            to_value(state.handle.task_resume(req.request_id.clone(), p).await)
        }
        Method::TaskStop => {
            let p: bm_contract::wire::TaskLifecycleParams = params(req)?;
            to_value(state.handle.task_stop(req.request_id.clone(), p).await)
        }
    }
}

/// 统一 RPC 端点。未知方法 → 404;信封解析失败 → 400;其余 200 + 信封。
pub async fn rpc_endpoint(
    State(state): State<AppState>,
    Path(method_name): Path<String>,
    body: String,
) -> Response {
    let Some(method) = Method::from_wire(&method_name) else {
        return (StatusCode::NOT_FOUND, "未知方法").into_response();
    };
    let Ok(req) = serde_json::from_str::<RequestEnvelope>(&body) else {
        return (StatusCode::BAD_REQUEST, "请求信封非法").into_response();
    };
    let request_id = req.request_id.clone();
    let envelope = match rpc_inner(&state, method, &req).await {
        Ok(result) => ResponseEnvelope::Success { request_id, result },
        Err(e) => ResponseEnvelope::Failure {
            request_id,
            error: e.to_wire(),
        },
    };
    Json(&envelope).into_response()
}
