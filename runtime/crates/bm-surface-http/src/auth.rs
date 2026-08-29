//! Bearer 鉴权(合同库 surface/auth.v0_1)。
//! 全部 /rpc 与 /events 请求须 `Authorization: Bearer <token>`;/health 豁免。

use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// 校验 Bearer 令牌;失败返回 401(transport 合同:unauthorized)。
pub async fn require_bearer(
    State(app): State<crate::AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let expected = app.token;
    let ok = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|given| constant_time_eq(given.as_bytes(), expected.as_bytes()))
        .unwrap_or(false);
    if ok {
        next.run(request).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

/// 常数时间比较(避免时序侧信道;令牌为高熵随机值,此处为纵深防御)。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}
