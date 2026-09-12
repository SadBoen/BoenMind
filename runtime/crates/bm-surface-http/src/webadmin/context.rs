//! 上下文透视/检索与会话历史回放/删除(W5 透视 + W9 检索 +
//! 会话管理批;数据源 context-log.jsonl)。

use super::{AdminConfig, bad_request, not_found, respond_or_fail};
use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use bm_contract::ids::{IdGen, UlidIdGen};
use serde_json::{Value, json};
use std::path::Path;

/// GET /admin/context:context-log.jsonl 尾部解析为结构化数组(W5 上下文
/// 透视页直用)。每行 = 一次模型调用的请求快照(messages/tools)+ 结果
/// (status/usage/耗时);坏行跳过;最多回读 2MB、默认 120 条(新→旧即
/// 最旧在前,与文件时序一致)。
pub async fn context_tail(State(cfg): State<AdminConfig>) -> Response {
    // W10:尾读字节/条数上限走 limits。
    let lim = cfg.limits.get();
    let steps = read_context_tail(
        &cfg.data_dir.join("context-log.jsonl"),
        lim.context_tail_max_bytes,
        lim.context_tail_entries,
    );
    Json(json!({ "ok": true, "steps": steps })).into_response()
}

/// GET /admin/context/search?q=&limit=:跨会话全文检索(W9 二期)。
/// 个人单机数据量下行级扫描(context-log.jsonl 任一行含 q 即命中,
/// 大小写不敏感);数据量上来再换 FTS5 索引(规格 W9 二期备注)。
/// P1-12():BufReader 流式逐行——不再整文件载入内存
/// (context-log 无轮转机制,长会话可达 GB 级);滑动窗口只留最新 limit 条
/// 命中,输出序仍为新→旧。
pub async fn context_search(
    State(cfg): State<AdminConfig>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let q = params.get("q").cloned().unwrap_or_default();
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .clamp(1, cfg.limits.get().context_search_max_limit);
    if q.trim().is_empty() {
        return bad_request("缺少 q");
    }
    let path = cfg.data_dir.join("context-log.jsonl");
    let needle = q.to_lowercase();
    let mut window: std::collections::VecDeque<Value> = std::collections::VecDeque::new();
    if let Ok(f) = std::fs::File::open(&path) {
        use std::io::BufRead;
        for line in std::io::BufReader::new(f).lines() {
            let Ok(line) = line else { break };
            if line.to_lowercase().contains(&needle)
                && let Ok(v) = serde_json::from_str::<Value>(&line)
            {
                window.push_back(v);
                if window.len() > limit {
                    window.pop_front();
                }
            }
        }
    }
    let hits: Vec<Value> = window.into_iter().rev().collect();
    Json(json!({ "ok": true, "q": q, "hits": hits, "total": hits.len() })).into_response()
}

/// GET /admin/sessions?limit=&skip=:会话目录()。
/// 三端列表不一致的根因);目录收归服务端单一权威,前端启动即拉本端点
/// (管理面不入合同)。按最近活跃倒序,内存视图投影(经核心单写者)。
/// 分页(issue #15):limit 默认 500 硬顶 1000,skip = 从最新跳过条数
/// (与 session_messages 同款游标口径);响应增 total/limit/skip/truncated
/// 增量字段。核心仍全量投影 SessionSummary(行小、单写者内存视图),
/// 裁剪在 HTTP 面——载荷与前端渲染有界;核心侧分页待真实规模需要再做。
pub(crate) async fn session_list(
    State(cfg): State<AdminConfig>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let sessions = cfg.handle.session_list().await;
    let total = sessions.len();
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(500)
        .clamp(1, 1000);
    let skip: usize = params.get("skip").and_then(|v| v.parse().ok()).unwrap_or(0);
    let page: Vec<_> = sessions.iter().skip(skip).take(limit).collect();
    let truncated = skip + page.len() < total;
    Json(json!({
        "ok": true,
        "total": total,
        "limit": limit,
        "skip": skip,
        "truncated": truncated,
        "sessions": page,
    }))
    .into_response()
}

/// DELETE /admin/sessions/{session_id}:会话删除()。
/// 经核心单写者执行:墓碑 + operations 原文擦除 + context-log 流式过滤;
/// events.jsonl 仅元数据不动(A4/审计口径)。不可恢复。
pub(crate) async fn session_delete(
    State(cfg): State<AdminConfig>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Response {
    match cfg
        .handle
        .session_delete(
            UlidIdGen.next_id("req"),
            bm_contract::wire::SessionDeleteParams {
                session_id: respond_or_fail!(bm_contract::ids::BmId::parse(&session_id), |_| {
                    bad_request("非法会话 id")
                }),
            },
        )
        .await
    {
        Ok(r) => Json(json!({
            "ok": true,
            "session_id": session_id,
            "deleted_at": r.deleted_at.as_str(),
            "purged_lines": r.purged_lines,
        }))
        .into_response(),
        Err(e) => bad_request(e.to_wire().message),
    }
}

/// GET /admin/sessions/{session_id}/mode:会话权限模式读取(ADR-0030)。
/// 服务端权威,前端仅为选择器显示面;经核心单写者读模型(session_list)。
pub(crate) async fn session_mode_get(
    State(cfg): State<AdminConfig>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Response {
    let sessions = cfg.handle.session_list().await;
    match sessions.iter().find(|s| s.id == session_id) {
        Some(s) => Json(json!({
            "ok": true,
            "session_id": session_id,
            "permission_mode": s.permission_mode,
        }))
        .into_response(),
        None => not_found("未知会话"),
    }
}

/// POST /admin/sessions/{session_id}/mode body: {"mode":"ask"|"plan"|"yolo"}
/// 会话权限模式变更(ADR-0030):经核心单写者通道更新服务端会话状态,
/// 落 session.mode.changed 事实事件(物化投影持久,重启装载)。模式在
/// 裁决点读取——变更不影响已开出的等待中审批单(那些仍走人工)。
pub(crate) async fn session_mode_set(
    State(cfg): State<AdminConfig>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let mode = respond_or_fail!(
        bm_contract::wire::PermissionMode::from_wire(body["mode"].as_str().unwrap_or(""))
            .ok_or_else(|| bad_request("非法 mode:必须为 ask|plan|yolo"))
    );
    let sid = respond_or_fail!(bm_contract::ids::BmId::parse(&session_id), |_| bad_request(
        "非法会话 id"
    ));
    match cfg.handle.session_set_mode(sid, mode).await {
        Ok(_) => Json(json!({
            "ok": true,
            "session_id": session_id,
            "permission_mode": mode.as_str(),
        }))
        .into_response(),
        Err(e) => bad_request(e.to_wire().message),
    }
}

/// POST /admin/operations/{operation_id}/cancel
/// P1-5: 服务端管理面取消在途 operation 端点
pub(crate) async fn operation_cancel(
    State(cfg): State<AdminConfig>,
    axum::extract::Path(operation_id): axum::extract::Path<String>,
) -> Response {
    let op_id = respond_or_fail!(bm_contract::ids::BmId::parse(&operation_id), |_| {
        bad_request("非法 operation_id")
    });
    match cfg.handle.operation_cancel(op_id).await {
        Ok(r) => Json(json!({
            "ok": true,
            "accepted": r.accepted,
            "operation_id": r.operation_id.as_str(),
        }))
        .into_response(),
        Err(e) => bad_request(e.to_wire().message),
    }
}

/// GET /admin/sessions/{session_id}/messages?limit=&skip=:
/// 会话历史回放(
/// 界面)。从 context-log.jsonl 过滤 kind ∈ {user_message, assistant_final},
/// 按文件序(=真实时序)返回第 skip 条之前的最近 limit 条;limit 默认 50
/// 上限 200;has_more 指示是否还有更早。**分页游标用「从末尾跳过的条数」
/// 而非 seq**——历史文件里 seq 曾跨重启重数,seq 游标在存量数据上必错页。
/// BufReader 逐行流式,内存只留单页窗口(不整文件载入)。
pub async fn session_messages(
    State(cfg): State<AdminConfig>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    // P1-13():分页页大小用独立旋钮,不再借用检索
    // 上限(context_search_max_limit)——语义解耦,默认同为 200 行为零变化。
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .clamp(1, cfg.limits.get().session_messages_max_limit);
    let skip: usize = params.get("skip").and_then(|v| v.parse().ok()).unwrap_or(0);

    use std::collections::VecDeque;
    use std::io::BufRead;
    // 双端队列只留最近 skip+limit 条匹配;文件序 = 落盘序 = 真实时序
    let mut window: VecDeque<Value> = VecDeque::new();
    let mut matched: usize = 0;
    if let Ok(f) = std::fs::File::open(cfg.data_dir.join("context-log.jsonl")) {
        for line in std::io::BufReader::new(f).lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue; // 坏行跳过
            };
            if v.get("session_id").and_then(|s| s.as_str()) != Some(session_id.as_str()) {
                continue;
            }
            let role = match v.get("kind").and_then(|k| k.as_str()).unwrap_or("") {
                "user_message" => "user",
                "assistant_final" => "assistant",
                _ => continue,
            };
            let content = v["data"]["content"].as_str().unwrap_or_default();
            if content.is_empty() {
                continue;
            }
            matched += 1;
            window.push_back(json!({
                "ts": v.get("ts").cloned().unwrap_or(Value::Null),
                "role": role,
                "content": content,
            }));
            if window.len() > skip + limit {
                window.pop_front();
            }
        }
    }
    // 丢弃末尾 skip 条(那些已在前面的页面里),剩下的即本页(最旧在前)
    let take = window.len().saturating_sub(skip);
    let messages: Vec<Value> = window.into_iter().take(take).collect();
    let has_more = matched > skip + messages.len();
    Json(json!({
        "ok": true,
        "session_id": session_id,
        "messages": messages,
        "has_more": has_more,
    }))
    .into_response()
}

/// context-log 尾部读取+逐行解析(只读诊断面;任何失败静默为空)。
/// P2():尾读逻辑与 logs.rs 收口为 tail::read_tail。
fn read_context_tail(path: &Path, max_bytes: u64, limit: usize) -> Vec<Value> {
    let lines = super::tail::read_tail(path, max_bytes);
    let mut steps: Vec<Value> = lines
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if steps.len() > limit {
        steps = steps.split_off(steps.len() - limit);
    }
    steps
}
