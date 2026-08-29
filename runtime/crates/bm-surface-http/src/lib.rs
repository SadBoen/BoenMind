//! bm-surface-http:Surface Protocol 的 HTTP 绑定(M3.1)。
//!
//! 契约(合同库 surface/transport.v0_1):
//! - `POST /rpc/{method}`:body = RequestEnvelope,响应 = ResponseEnvelope,
//!   业务语义(含错误)全在信封,HTTP 恒 200(400/401/404/503 仅传输层);
//! - `GET /events/{session_id}?since_seq=N`:SSE 增量流,id = event_seq,
//!   服务端零订阅状态(断线重连 = resume cursor 语义);
//! - `GET /health`:无鉴权探针;
//! - 鉴权:除 /health 外一律 Bearer 令牌(合同库 surface/auth.v0_1)。

pub mod auth;
pub mod rpc;
pub mod sse;
pub mod token;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post};
use bm_core::runtime::RuntimeHandle;
use std::sync::Arc;

/// 共享应用状态。
#[derive(Clone)]
pub struct AppState {
    pub handle: RuntimeHandle,
    pub token: Arc<String>,
    pub store: Arc<dyn bm_persist::EventStore>,
    /// 应用层停机信号(M3.6:/shutdown 触发;服务宿主 await 它以退出)。
    pub shutdown: Arc<tokio::sync::Notify>,
}

/// 组装 Surface 路由。`token` 为已加载的访问令牌;/health 豁免鉴权,
/// /rpc 与 /events 受 Bearer 保护。
pub fn router(
    handle: RuntimeHandle,
    token: Arc<String>,
    store: Arc<dyn bm_persist::EventStore>,
    shutdown: Arc<tokio::sync::Notify>,
) -> Router {
    let state = AppState {
        handle,
        token,
        store,
        shutdown,
    };
    Router::new()
        .route("/rpc/{method}", post(rpc::rpc_endpoint))
        .route("/events/{session_id}", get(sse::events_sse))
        .route("/shutdown", post(rpc::shutdown_endpoint))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer,
        ))
        .route("/health", get(rpc::health))
        .with_state(state)
}
