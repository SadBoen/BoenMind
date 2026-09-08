//! Bearer 鉴权(合同库 surface/auth.v0_1)。
//! 全部 /rpc 与 /events 请求须 `Authorization: Bearer <token>`;/health 豁免。
//! /admin 与 /v1 走 [`require_api_auth`](issue #10 断链补立,口径见函数注释)。

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

/// /admin 与 /v1 的统一鉴权(issue #10 断链补立):
/// ① 带 `Authorization: Bearer <t>` → 必须与本机令牌一致,错误即 401
///   (严格失败:持错误凭据绝不因「墙未开」而放行);
/// ② 无 Authorization 头 → 门户会话 Cookie 有效即放行(已登录浏览器);
/// ③ 门户未配置且绑定回环 → 放行(本地开发/Playground 未登录可用,
///   decisions.md #5 口径);其余(公网裸绑/已设墙未认证)401。
/// 合同面 /rpc、/events 仍走 [`require_bearer`] 严格 Bearer,不认 Cookie。
pub async fn require_api_auth(
    State(app): State<crate::AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    if let Some(given) = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        if constant_time_eq(given.as_bytes(), app.token.as_bytes()) {
            return next.run(request).await;
        }
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if crate::portal::cookie_authed(&app, &headers)
        || (!app.portal.configured() && !app.public_bind)
    {
        next.run(request).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

/// 常数时间比较(避免时序侧信道;令牌为高熵随机值,此处为纵深防御)。
/// P2(2026-09-07 架构评审):portal.rs 同款实现已收口到本函数。
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}
