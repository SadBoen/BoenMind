//! 改进建议（refine-suggest）：列表 / 审批生效 / 拒绝 / 回滚。
//!
//! 代理只提交建议（pending）；用户批准后 bm-core::refine 才真正修改
//! SKILL.md 描述或追加系统提示词（改前备份可回滚）。审批权始终在宿主。

use axum::{Json, extract::{Query, State}, http::StatusCode};
use serde::Deserialize;

use crate::{ApiResult, api_error, api_error_from};

/// 列表（可选按状态过滤：pending / approved / rejected）。
#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<String>,
}

pub async fn list_refinement_suggestions(
    State(state): crate::SharedState,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Vec<bm_core::db::RefinementSuggestion>>> {
    state
        .db
        .list_refinement_suggestions(query.status.as_deref())
        .await
        .map(Json)
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

/// 批准：修改 SKILL.md 描述 / 追加系统提示词（改前备份），并把建议置为 approved。
pub async fn approve_suggestion(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let suggestion = state
        .db
        .get_refinement_suggestion(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "建议不存在"))?;
    if suggestion.status == "approved" {
        return Err(api_error(StatusCode::CONFLICT, "建议已批准，请勿重复操作"));
    }
    let backup = {
        let mut config = state.config.write().expect("config poisoned");
        bm_core::refine::apply_suggestion(&mut config, &suggestion).map_err(api_error_from)?
    };
    // 生效成功才改状态并记录备份路径
    state
        .db
        .set_refinement_suggestion_status(&id, "approved")
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if let Some(backup) = &backup {
        state
            .db
            .set_refinement_suggestion_backup(&id, backup)
            .await
            .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    }
    Ok(Json(serde_json::json!({ "ok": true, "backup": backup })))
}

pub async fn reject_suggestion(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let suggestion = state
        .db
        .get_refinement_suggestion(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "建议不存在"))?;
    if suggestion.status != "pending" {
        return Err(api_error(StatusCode::CONFLICT, "仅待审批的建议可拒绝"));
    }
    state
        .db
        .set_refinement_suggestion_status(&id, "rejected")
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// 回滚：从批准时备份恢复 SKILL.md，状态回到 pending（可重新审批）。
pub async fn rollback_suggestion(
    State(state): crate::SharedState,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let suggestion = state
        .db
        .get_refinement_suggestion(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "建议不存在"))?;
    if suggestion.status != "approved" {
        return Err(api_error(StatusCode::CONFLICT, "仅已批准的建议可回滚"));
    }
    let backup = suggestion
        .backup_path
        .as_deref()
        .ok_or_else(|| api_error(StatusCode::CONFLICT, "该建议无备份（system_prompt 类型追加不可回滚）"))?;
    {
        let config = state.config.read().expect("config poisoned");
        bm_core::refine::rollback_suggestion(&config, backup).map_err(api_error_from)?;
    }
    state
        .db
        .reset_refinement_suggestion(&id)
        .await
        .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
