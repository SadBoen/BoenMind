//! auth.* handler 领域子模块（api.rs 拆分）。
//! 认证门控放行方法 + AuthPort 登录/登出/状态/改密（--auth 装配；未装配诚实失败）。

use serde_json::{json, Value};

use crate::api::AppState;
use crate::rpc::{err, ok};

pub(super) fn auth_methods() -> &'static [&'static str] {
    &[
        "auth.status",
        "auth.login",
        "auth.logout",
        "auth.changePassword",
        "host.describe", // 前端启动先查状态，放行（不含敏感数据）
    ]
}
/// 认证面：登录/登出/状态/改密（AuthPort；--auth 装配；未装配诚实失败）。
pub(super) fn auth_port(state: &AppState) -> Result<&dyn kernel_contracts::AuthPort, Value> {
    state
        .runtime
        .auth
        .as_deref()
        .ok_or_else(|| err("auth-not-available", "auth plugin not installed (pass --auth)"))
}

/// auth.status：当前会话是否有效。
pub(super) async fn auth_status(state: &AppState, token: Option<&str>) -> Value {
    match auth_port(state) {
        Ok(auth) => ok(json!({ "authenticated": auth.is_authenticated(token.unwrap_or("")) })),
        Err(e) => e,
    }
}

/// auth.login：只密码登录，成功签发会话 token。
pub(super) async fn auth_login(state: &AppState, payload: Value) -> Value {
    let auth = match auth_port(state) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let Some(password) = payload.get("password").and_then(Value::as_str) else {
        return err("bad-request", "missing password");
    };
    match auth.login(password).await {
        Ok(r) if r.ok => ok(json!({ "token": r.token })),
        Ok(_) => err("wrong-password", "wrong password"),
        Err(e) => err("auth-error", format!("{e:?}")),
    }
}

/// auth.logout：作废当前会话（幂等）。
pub(super) async fn auth_logout(state: &AppState, token: Option<&str>) -> Value {
    match auth_port(state) {
        Ok(auth) => {
            if let Some(t) = token {
                auth.logout(t);
            }
            ok(Value::Null)
        }
        Err(e) => e,
    }
}

/// auth.changePassword：改密（需会话有效 + 当前密码正确）。
pub(super) async fn auth_change_password(state: &AppState, payload: Value, token: Option<&str>) -> Value {
    let auth = match auth_port(state) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let current = payload.get("currentPassword").and_then(Value::as_str).unwrap_or("");
    let new = payload.get("newPassword").and_then(Value::as_str).unwrap_or("");
    match auth
        .change_password(token.unwrap_or(""), current, new)
        .await
    {
        Ok(r) if r.ok => ok(Value::Null),
        Ok(r) => err(&r.error, format!("auth failed: {}", r.error)),
        Err(e) => err("auth-error", format!("{e:?}")),
    }
}
