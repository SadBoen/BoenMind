//! 自更新端点：检查 / 应用 / 重启。
//!
//! 全部由用户手动触发（About 页点按钮），无任何自动检查或提示。
//! - check：查 GitHub Releases 最新版（bm-core 负责版本/资产/白名单逻辑）
//! - apply：下载 → 验签 → 落盘（managed：runtime 目录；standalone：替换自身）
//! - restart：standalone 延迟 300ms exec 自身（先返回响应，进程被替换）
//!   managed 不提供——桌面版由前端调壳的 backend_restart 命令重启

use axum::{Json, extract::State, http::StatusCode};
use bm_core::AppError;

use crate::{ApiResult, SharedState, api_error, api_error_from};

/// 检查更新（用户手动触发）
pub async fn check_update() -> ApiResult<Json<serde_json::Value>> {
    let result = tokio::task::spawn_blocking(bm_core::updates::check_update)
        .await
        .map_err(|e| api_error_from(AppError::internal(format!("检查更新线程异常: {e}"))))?;
    match result {
        Ok(check) => Ok(Json(serde_json::to_value(check).unwrap_or(serde_json::json!({})))),
        Err(err) => Err(api_error_from(err)),
    }
}

/// 应用更新：下载 → 验签 → 落盘/替换。
/// 有运行中的任务时拒绝（进程重启会丢内存中的 agent 任务，提示先停止）。
pub async fn apply_update(State(state): SharedState) -> ApiResult<Json<serde_json::Value>> {
    if state.db.has_running_tasks().await.unwrap_or(false) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "有正在运行的任务，请等待完成或停止后再升级",
        ));
    }
    let result = tokio::task::spawn_blocking(bm_core::updates::apply_update)
        .await
        .map_err(|e| api_error_from(AppError::internal(format!("升级线程异常: {e}"))))?;
    match result {
        Ok(outcome) => Ok(Json(serde_json::to_value(outcome).unwrap_or(serde_json::json!({})))),
        Err(err) => Err(api_error_from(err)),
    }
}

/// 重启（standalone）：延迟 300ms 后 exec 自身（PID 不变 → systemd 无感知）。
/// 先返回响应，进程随后被新版本替换；managed（桌面版）由壳负责，拒绝此调用。
pub async fn restart_update() -> ApiResult<Json<serde_json::Value>> {
    if bm_core::updates::is_managed() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "桌面版由应用壳重启，无需调用此端点",
        ));
    }
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Err(err) = crate::exec_self() {
            eprintln!("[bm-server] 自更新 exec 失败: {err}");
        }
    });
    Ok(Json(serde_json::json!({ "status": "restarting" })))
}
