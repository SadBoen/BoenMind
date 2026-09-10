//! 对话内审批裁决(W4b;与 /rpc/approval.respond 同一执行体,
//! 走 /admin 免鉴权口径——W1 同款已登记欠账;前端审批卡片无令牌可带)。

use super::{AdminConfig, bad_request, internal, respond_or_fail};
use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::response::{IntoResponse, Response};
use bm_contract::ids::{IdGen, UlidIdGen};
use serde_json::Value;

/// POST /admin/approvals/{id}/respond  body: {decision: "approve"|"deny",
/// scope?: "once"(approve 必带,走 once 单次口径)}
pub async fn approval_respond(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let decision = body["decision"].as_str().unwrap_or("").to_string();
    let scope = body["scope"].as_str().map(|s| s.to_string());
    let request_id = UlidIdGen.next_id("req");
    let appr_id = respond_or_fail!(bm_contract::ids::BmId::parse(&id), |_| bad_request(
        "非法审批单 id"
    ));
    match cfg
        .handle
        .approval_respond(
            request_id,
            bm_contract::wire::ApprovalRespondParams {
                approval_id: appr_id,
                decision,
                scope,
            },
        )
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => bad_request(format!("审批裁决失败: {e}")),
    }
}

/// GET /admin/approvals:待裁决审批单列表(前端轮询面)。2026-09-07 批准
/// 可达性修复:审批标记此前仅随回合 /v1 流下发,流到期或后台续跑回合
/// 无主流时,审批单永远无人可批,任务一律卡死在审批轮询。本查询同时
/// 触发到期清扫,滞留单不占待决队列。
pub async fn approvals_list(State(cfg): State<AdminConfig>) -> Response {
    match cfg
        .handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e.to_wire().message),
    }
}
