//! /rpc/{method} 端点:信封逐字节复用(M3 规格 §4)。
//! 业务结果(含信封内 error)恒 HTTP 200;仅 400/401/404/503 走传输状态码。

use crate::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use bm_contract::wire::{
    CancelParams, EventsPollParams, GetOperationParams, Method, RequestEnvelope, ResponseEnvelope,
    SendInputParams, SessionCloseParams, SessionCreateParams, SessionResumeParams,
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
        // M4:能力与审批三方法(T3b 起 Runtime 已接线)
        Method::CapabilityCall => {
            let p: bm_contract::wire::CapabilityCallParams = params(req)?;
            to_value(
                state
                    .handle
                    .capability_call(req.request_id.clone(), p)
                    .await,
            )
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

/// M8 审计工具:网页 CLI 终端后端(BOEN_CLI_WEB=1 显式开启;仅本机形态)。
/// 把命令行转发给与服务器同目录的 boenmind CLI 子进程,自动注入
/// --url/--token(令牌不回传页面)。非 shell 执行:参数按引号切分。
pub async fn cli_endpoint(State(state): State<AppState>, body: String) -> Response {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;

    if std::env::var("BOEN_CLI_WEB").as_deref() != Ok("1") {
        return (StatusCode::NOT_FOUND, "CLI 终端未启用").into_response();
    }
    #[derive(serde::Deserialize)]
    struct CliBody {
        cmd: String,
    }
    let Ok(parsed) = serde_json::from_str::<CliBody>(&body) else {
        return (StatusCode::BAD_REQUEST, "body 须为 {\"cmd\": \"...\"}").into_response();
    };
    let cmd = parsed.cmd.trim().to_string();
    if cmd.is_empty() || cmd.len() > 500 {
        return (StatusCode::BAD_REQUEST, "cmd 为空或过长").into_response();
    }
    // 简单分词:双引号内保留空格;禁止分号/管道/反引号(不做 shell)
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for ch in cmd.chars() {
        match ch {
            '"' => in_quote = !in_quote,
            ';' | '|' | '`' => {
                return (StatusCode::BAD_REQUEST, "禁止 shell 元字符").into_response();
            }
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    if args.is_empty() {
        return (StatusCode::BAD_REQUEST, "空命令").into_response();
    }
    // CLI 可执行文件:优先与服务器同目录,否则回退 PATH
    let cli_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("boenmind.exe")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("boenmind"));
    let url =
        std::env::var("BOEN_CLI_WEB_URL").unwrap_or_else(|_| "http://127.0.0.1:7531".to_string());
    let mut full: Vec<String> = vec![
        "--url".into(),
        url,
        "--token".into(),
        state.token.as_str().to_string(),
    ];
    full.extend(args);
    let child = tokio::process::Command::new(&cli_path)
        .args(&full)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            let payload = serde_json::json!({
                "exit_code": -1, "stdout": "",
                "stderr": format!("CLI 启动失败: {e}"),
            });
            return (StatusCode::OK, Json(payload)).into_response();
        }
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout).await;
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr).await;
    }
    let exit_code =
        match tokio::time::timeout(std::time::Duration::from_secs(60), child.wait()).await {
            Ok(Ok(st)) => st.code().unwrap_or(-1),
            Ok(Err(e)) => {
                stderr.push_str(&format!("wait 失败: {e}"));
                -1
            }
            Err(_) => {
                let _ = child.kill().await;
                stderr.push_str("执行超时(60s),已终止");
                -1
            }
        };
    let payload = serde_json::json!({
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
    });
    (StatusCode::OK, Json(payload)).into_response()
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
