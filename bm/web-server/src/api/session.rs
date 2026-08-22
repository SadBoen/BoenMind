//! session.* handler 领域子模块（api.rs 拆分）。
//! 会话生命周期/历史/搜索/分叉/提示/模型选择/取消/重命名（共享 model helper 在主文件）。

use std::sync::Arc;

use kernel_contracts::session::{SessionEvent, SessionHeader, SessionId};
use serde_json::{json, Value};

use crate::api::{current_model, model_groups, AppState, SessionHandle};
use crate::events::translate_events;
use crate::rpc::{err, err_with_details, ok};

pub(super) async fn session_list(state: &Arc<AppState>) -> Value {
    let ids: Vec<String> = state
        .runtime
        .persist
        .list_sessions()
        .await
        .unwrap_or_default();
    let items = {
        let sessions = state.sessions.lock().unwrap();
        ids.into_iter()
            .map(|id| {
                let h = sessions.get(&id);
                json!({
                    "sessionId": id,
                    "updatedAt": "1970-01-01T00:00:00.000Z",
                    "running": h.map(|s| s.running).unwrap_or(false),
                    "blank": h.map(|s| s.blank).unwrap_or(true),
                    "cwd": h
                        .and_then(|s| s.agent.session().header().workspace.clone())
                        .unwrap_or_default(),
                })
            })
            .collect::<Vec<_>>()
    };
    ok(json!({ "items": items }))
}

/// 幂等采用或 cwd 冲突（对齐上游 ensureSession：同 cwd 采用现有会话，
/// 不同 cwd → `session-conflict{sessionId, requestedCwd, existingCwd?}`。
/// 替代旧自造码 session-exists——上游 RpcErrorDetailsMap 无此码，前端
/// 按 session-conflict 分支处理（rpc.schema.ts 逐字形状）。
pub(super) fn session_create_existing(
    session_id: &str,
    existing_cwd: Option<String>,
    cwd: &Option<String>,
) -> Value {
    if existing_cwd == *cwd {
        // 幂等采用现有（对齐上游 ensureSession 的 live/persisted 采用路径；
        // 前端把 create 当 commit，随后经 history/list 拉数据）。
        ok(json!({ "sessionId": session_id, "agentPreset": "standard" }))
    } else {
        let mut details = json!({
            "sessionId": session_id,
            "requestedCwd": cwd.clone().unwrap_or_default(),
        });
        if let Some(ex) = &existing_cwd {
            details["existingCwd"] = json!(ex);
        }
        err_with_details(
            "session-conflict",
            format!("session {session_id} already exists in a different directory"),
            details,
        )
    }
}

pub(super) async fn session_create(state: &Arc<AppState>, payload: Value) -> Value {
    // schema 校验：workspaceId 显式 null 拒绝（Node 实测 bad-request）。
    if let Some(ws) = payload.get("workspaceId") {
        if ws.is_null() {
            return err("bad-request", "workspaceId must be a string or omitted");
        }
    }
    let session_id = payload
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // workspace 前置（对齐上游 create：先查 workspace 再有任何创建副作用——
    // 未知 workspace → workspace-not-found，绝不留半建 session）。
    let workspace_path: Option<String> = match payload.get("workspaceId").and_then(Value::as_str) {
        Some(ws_id) => {
            let ws = state.workspaces.lock().unwrap();
            let Some(view) = ws.get(ws_id) else {
                return err_with_details(
                    "workspace-not-found",
                    format!("workspace \"{ws_id}\" not found"),
                    json!({ "workspaceId": ws_id }),
                );
            };
            view.get("path").and_then(Value::as_str).map(str::to_string)
        }
        None => None,
    };
    // cwd = workspace.path ?? payload.cwd（对齐上游 cwd 解析顺序）。
    let cwd = workspace_path.or_else(|| {
        payload.get("cwd").and_then(Value::as_str).map(str::to_string)
    });

    // 幂等/冲突判定（对齐上游 ensureSession 顺序：live 优先，其次持久化）。
    // 旧实现静默拒绝（BUG-007）；新语义：同 cwd 幂等采用，不同 cwd → session-conflict。
    if let Some(h) = state.sessions.lock().unwrap().get(&session_id) {
        let existing = h.agent.session().header().workspace.clone();
        return session_create_existing(&session_id, existing, &cwd);
    }
    if state
        .runtime
        .persist
        .list_sessions()
        .await
        .unwrap_or_default()
        .contains(&session_id)
    {
        // 持久化已存在（运行中未恢复）：读日志 SessionStarted header 的 workspace 比较。
        let existing = match state.runtime.persist.load_events(&session_id).await {
            Ok(Some(records)) => records.first().and_then(|r| match &r.event {
                SessionEvent::SessionStarted { header } => header.workspace.clone(),
                _ => None,
            }),
            _ => None,
        };
        return session_create_existing(&session_id, existing, &cwd);
    }

    let header = SessionHeader {
        id: SessionId(session_id.clone()),
        app: "web".into(),
        profile: "web".into(),
        workspace: cwd.clone(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    match state.runtime.create_session(header).await {
        Ok(agent) => {
            // workspaceId 语义（台账 §2 session.create）：新会话 attach 进该 workspace，
            // prepend 到 sessionIds（活动时显示序首）。前置已保证 workspace 存在。
            if let Some(ws_id) = payload.get("workspaceId").and_then(Value::as_str) {
                let mut ws = state.workspaces.lock().unwrap();
                let Some(view) = ws.get_mut(ws_id) else {
                    // 竞态防御：前置后 workspace 被并发删（理论不可达）。
                    return err_with_details(
                        "workspace-attach-failed",
                        format!("session \"{session_id}\" was created but could not attach to workspace \"{ws_id}\""),
                        json!({ "sessionId": session_id, "workspaceId": ws_id }),
                    );
                };
                let mut session_ids: Vec<String> = view["sessionIds"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                if !session_ids.contains(&session_id) {
                    session_ids.insert(0, session_id.clone());
                    view["sessionIds"] = json!(session_ids);
                    view["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
                }
            }
            let mut sessions = state.sessions.lock().unwrap();
            sessions.insert(
                session_id.clone(),
                SessionHandle {
                    agent,
                    running: false,
                    blank: true,
                    title: None,
                    selected: None,
                },
            );
            drop(sessions);
            // HostFrame：新会话广播（blank 恒 true，首个 running 时翻转——由 prompt 侧翻转）。
            state.broadcast_host(
                "host/session-added",
                json!({ "sessionId": session_id, "blank": true, "cwd": cwd }),
            );
            // workspace attach 后广播该工作区（HostFrame 单 workspace 形状）。
            if let Some(ws_id) = payload.get("workspaceId").and_then(Value::as_str) {
                let ws = state.workspaces.lock().unwrap();
                if let Some(view) = ws.get(ws_id) {
                    state.broadcast_host(
                        "host/workspace-changed",
                        json!({ "workspace": view.clone() }),
                    );
                }
            }
            ok(json!({ "sessionId": session_id, "agentPreset": "standard" }))
        }
        Err(e) => err("internal", format!("session create failed: {e}")),
    }
}

pub(super) async fn session_history(state: &Arc<AppState>, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let records = match state.runtime.persist.load_events(session_id).await {
        Ok(Some(r)) => r,
        Ok(None) => return err("session-not-found", format!("session {session_id} not found")),
        Err(e) => return err("internal", format!("history failed: {e}")),
    };
    let events: Vec<SessionEvent> = records.into_iter().map(|r| r.event).collect();
    let wire = translate_events(&events);
    let items: Vec<Value> = wire
        .iter()
        .map(|ev| json!({ "event": ev }))
        .collect();
    // projections：tail 页才有（当前无分页 = 恒 tail）。asOfSeq = wire 长度 - 1
    // （空日志 -1，对齐 session/subscribed 的 lastSeq 约定）。
    let as_of_seq = wire.len() as i64 - 1;
    let projections = state.projection_snapshot();
    ok(json!({
        "events": items,
        "hasMore": false,
        "projections": { "asOfSeq": as_of_seq, "values": projections }
    }))
}

/// 搜索结果限制（台账 §2 session.search：`SESSION_SEARCH_RESULT_LIMIT=20`、
/// `SESSION_SEARCH_SNIPPET_MAX_CODE_POINTS=240`）。
pub(super) const SESSION_SEARCH_RESULT_LIMIT: usize = 20;
pub(super) const SESSION_SEARCH_SNIPPET_MAX_CODE_POINTS: usize = 240;

/// session.search：query trim 后 1-500 字符、禁 NUL；扫全部会话日志找文本匹配，
/// snippet 取匹配点附近窗口（≤240 code points），结果 ≤20 条。
pub(super) async fn session_search(state: &Arc<AppState>, payload: Value) -> Value {
    let Some(query) = payload.get("query").and_then(Value::as_str) else {
        return err("bad-request", "missing query");
    };
    let query = query.trim();
    if query.is_empty() {
        return err("bad-request", "query must be at least 1 character");
    }
    if query.chars().count() > 500 {
        return err("bad-request", "query must be at most 500 characters");
    }
    if query.contains('\0') {
        return err("bad-request", "query must not contain NUL");
    }

    let session_ids = match state.runtime.persist.list_sessions().await {
        Ok(ids) => ids,
        Err(e) => return err("internal", format!("search failed: {e}")),
    };
    let mut items: Vec<Value> = Vec::new();
    for sid in session_ids {
        if items.len() >= SESSION_SEARCH_RESULT_LIMIT {
            break;
        }
        let records = match state.runtime.persist.load_events(&sid).await {
            Ok(Some(r)) => r,
            _ => continue,
        };
        let events: Vec<SessionEvent> = records.into_iter().map(|r| r.event).collect();
        // 只扫表面文本事件（user/message、assistant/message）。
        let mut snippet: Option<String> = None;
        for ev in &events {
            match ev {
                SessionEvent::UserMessage { text } => {
                    if let Some(snip) = make_snippet(text, query) {
                        snippet = Some(snip);
                        break;
                    }
                }
                SessionEvent::AssistantMessage { content, .. } => {
                    let t: String = content
                        .iter()
                        .filter_map(|b| match b {
                            kernel_contracts::ContentBlock::Text(t) => Some(t.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !t.is_empty() {
                        if let Some(snip) = make_snippet(&t, query) {
                            snippet = Some(snip);
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(snip) = snippet {
            items.push(json!({ "sessionId": sid, "snippet": snip }));
        }
    }
    ok(json!({ "items": items, "hasMore": false }))
}

/// 从文本中截取包含 query 首匹配的 snippet 窗口（≤240 code points）。
/// 无匹配 → None。
/// astral 边界纪律：所有切分都走 `chars()`（Unicode scalar value），
/// 绝不劈 surrogate pair；`find` 的字节偏移先换算成 char 位置再取窗口
/// （否则 query 前的多字节字符会把窗口起点算偏）。
pub(super) fn make_snippet(text: &str, query: &str) -> Option<String> {
    let byte_pos = text.find(query)?;
    let max = SESSION_SEARCH_SNIPPET_MAX_CODE_POINTS;
    // 匹配点为中心：匹配前留 100，匹配后留到 240（不足则向前补）。
    let total_chars = text.chars().count();
    let char_pos = text[..byte_pos].chars().count();
    let lead = 100usize;
    let start_char = char_pos.saturating_sub(lead).min(total_chars.saturating_sub(max));
    let mut out: String = text.chars().skip(start_char).take(max).collect();
    if out.chars().count() >= max {
        out = out.chars().take(max.saturating_sub(1)).collect::<String>() + "…";
    }
    // 折叠连续空白为单空格，trim。
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(collapsed)
}
pub(super) async fn session_fork(state: &Arc<AppState>, payload: Value) -> Value {
    use kernel_contracts::session::TurnEvent;

    let Some(source_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let at_seq = payload.get("atSeq").and_then(Value::as_u64);
    let records = match state.runtime.persist.load_events(source_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return err("session-not-found", format!("session {source_id} not found"))
        }
        Err(e) => return err("internal", format!("fork failed: {e}")),
    };
    let events: Vec<SessionEvent> = records.into_iter().map(|r| r.event).collect();

    // 锚点：所有完成 turn 的（事件 seq，含 SessionStarted 的 seq=1 偏移）。
    // 事件 Vec 下标 0 = SessionStarted（seq 1），事件下标 i 对应持久化 seq i+1。
    let turn_ends: Vec<(usize, u64)> = events
        .iter()
        .enumerate()
        .filter_map(|(i, ev)| match ev {
            SessionEvent::Turn(TurnEvent::Ended { .. }) => Some((i, i as u64 + 1)),
            _ => None,
        })
        .collect();
    let anchor = match at_seq {
        // 锚定第一个 ≥atSeq 的 turn/end。
        Some(at) => turn_ends.iter().find(|(_, seq)| *seq >= at).map(|(i, _)| *i),
        None => turn_ends.last().map(|(i, _)| *i),
    };
    let Some(anchor_idx) = anchor else {
        // in-log 锚点 turn 未闭（或日志无完成 turn）。
        return err_with_details(
            "fork-unavailable",
            "no completed turn to fork from",
            json!({ "sessionId": source_id }),
        );
    };

    // 复制 [1..=anchor_idx]（下标 1 起跳过 SessionStarted）的事件到新会话。
    let mut header = match &events[0] {
        SessionEvent::SessionStarted { header } => header.clone(),
        _ => return err("internal", "source log has no SessionStarted"),
    };
    let new_id = uuid::Uuid::new_v4().to_string();
    header.id = SessionId(new_id.clone());
    header.created_at = chrono::Utc::now();
    header.updated_at = chrono::Utc::now();
    let fork_cwd = header.workspace.clone();

    let agent = match state.runtime.create_session(header).await {
        Ok(a) => a,
        Err(e) => return err("internal", format!("fork create failed: {e}")),
    };
    for ev in events.iter().take(anchor_idx + 1).skip(1) {
        let rec = agent.session().append(ev.clone());
        if let Err(e) = state
            .runtime
            .persist
            .append_events(&new_id, std::slice::from_ref(&rec.event))
            .await
        {
            // 清理孤儿半会话（内存 + 磁盘；fork 失败不留 residue——ARCH-005）。
            let _ = state.runtime.persist.delete_session(&new_id).await;
            state.sessions.lock().unwrap().remove(&new_id);
            return err("internal", format!("fork persist failed: {e}"));
        }
    }

    let mut sessions = state.sessions.lock().unwrap();
    sessions.insert(
        new_id.clone(),
        SessionHandle {
            agent,
            running: false,
            blank: false,
            title: None,
            selected: None,
        },
    );
    drop(sessions);
    state.broadcast_host(
        "host/session-added",
        json!({ "sessionId": new_id, "blank": false, "cwd": fork_cwd }),
    );
    ok(json!({ "sessionId": new_id }))
}

pub(super) async fn session_prompt(state: &Arc<AppState>, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let content = payload.get("content");
    let Some(content) = content.and_then(Value::as_array) else {
        return err("bad-request", "missing content array");
    };
    // 单 text 块前导 '/' = slash 命令（台账：session.prompt 语义）。
    let text = content
        .iter()
        .find_map(|b| b.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if text.trim_start().starts_with('/') {
        return err("unknown-command", "unknown command");
    }
    if text.trim().is_empty() {
        return err("bad-request", "empty prompt");
    }

    let agent = {
        let mut sessions = state.sessions.lock().unwrap();
        let Some(h) = sessions.get_mut(session_id) else {
            return err("session-not-found", format!("session {session_id} not found"));
        };
        if h.running {
            // 单活跃回合：排队语义 M1 简化为拒绝（台账：session.prompt 可 queue/steer）。
            return err("agent-busy", "session already running");
        }
        h.running = true;
        h.blank = false;
        // per-session 模型选择同步给 agent（session.selectModel 语义）。
        if let Some((provider, model)) = h.selected.clone() {
            h.agent.set_model_override(provider, model);
        }
        Arc::clone(&h.agent)
    };
    let state2 = Arc::clone(state);
    let sid = session_id.to_string();
    state2.broadcast_host(
        "host/session-status",
        json!({ "sessionId": sid, "running": true }),
    );
    tokio::spawn(async move {
        let _ = agent.run_turn(Some(&text)).await;
        if let Some(h) = state2.sessions.lock().unwrap().get_mut(&sid) {
            h.running = false;
        }
        // goal-round-driver：回合完成后，若该 session 有 active + 有额度目标，
        // 注入 <goal_round> 续跑下一轮（自动续跑到目标完成/暂停/额度耗尽）。
        // 从 Mutex clone 出 Arc（不跨 await 持 guard）。
        let driver = state2.goal_driver.lock().unwrap().clone();
        if let Some(driver) = driver {
            let _ = driver.maybe_continue(&sid).await;
        }
        state2.broadcast_host(
            "host/session-status",
            json!({ "sessionId": sid, "running": false }),
        );
    });
    ok(json!({ "accepted": true }))
}

pub(super) fn session_models(state: &AppState, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let (provider, model) = current_model(state, session_id);
    // 可路由：真 provider 模式下当前 provider 必须已装配；mock 模式恒 true。
    let routable = if state.providers.is_empty() {
        true
    } else {
        state.providers.iter().any(|p| p.id == provider)
    };
    ok(json!({
        "current": { "provider": provider, "model": model },
        "routable": routable,
        "groups": model_groups(state),
        "failures": [],
    }))
}

/// session.selectModel：目录成员关系仅 advisory，直接接受任何 provider/model；
/// 写入会话级选择（prompt 时生效）。mock 模式下仍可接受（advisory）。
pub(super) fn session_select_model(state: &AppState, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let Some(provider) = payload.get("provider").and_then(Value::as_str) else {
        return err("bad-request", "missing provider");
    };
    let Some(model) = payload.get("model").and_then(Value::as_str) else {
        return err("bad-request", "missing model");
    };
    let mut sessions = state.sessions.lock().unwrap();
    let Some(h) = sessions.get_mut(session_id) else {
        return err("session-not-found", format!("session {session_id} not found"));
    };
    h.selected = Some((provider.to_string(), model.to_string()));
    // 立即同步给 agent（若本会话已开跑，下一回合生效）。
    h.agent
        .set_model_override(provider.to_string(), model.to_string());
    let mut selected = json!({ "provider": provider, "model": model });
    if let Some(re) = payload.get("reasoningEffort") {
        selected["reasoningEffort"] = re.clone();
    }
    ok(json!({ "selected": selected }))
}

pub(super) fn session_cancel(state: &Arc<AppState>, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    // M4：ReactLoopAgent.abort() 触发活跃回合的取消信号 → 流以
    // finish{kind:'aborted', code:'ABORTED'} 收尾（对齐 DSH session.cancel 语义）。
    let sessions = state.sessions.lock().unwrap();
    let Some(h) = sessions.get(session_id) else {
        return err("session-not-found", format!("session {session_id} not found"));
    };
    h.agent.abort();
    ok(json!({ "accepted": true }))
}

pub(super) fn session_rename(state: &Arc<AppState>, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let Some(title) = payload.get("title").and_then(Value::as_str) else {
        return err("bad-request", "missing title");
    };
    let title = title.trim();
    if title.is_empty() {
        return err("title-invalid", format!("session {session_id}"));
    }
    let mut sessions = state.sessions.lock().unwrap();
    let Some(h) = sessions.get_mut(session_id) else {
        return err("session-not-found", format!("session {session_id} not found"));
    };
    h.title = Some(title.to_string());
    ok(json!({ "title": title, "seq": 1i64 }))
}

/// session.delete：删除会话（持久化日志 + live 表）。空会话/título 会话也允许删除；
/// 未知会话 → bad-request（幂等删除由前端确认弹窗兜底，不静默吞错）。
/// 运行中的会话拒绝删除（agent-busy——先取消再删）。
pub(super) async fn session_delete(state: &Arc<AppState>, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    // 运行中会话拒绝（先 cancel 再删；避免悬空 running 状态）。
    if let Some(h) = state.sessions.lock().unwrap().get(session_id) {
        if h.running {
            return err("agent-busy", "session is running; cancel before deleting");
        }
    }
    match state.runtime.persist.delete_session(session_id).await {
        Ok(()) => {
            state.sessions.lock().unwrap().remove(session_id);
            // 广播删除（前端会话列表据此移除）。
            state.broadcast_host(
                "host/session-removed",
                json!({ "sessionId": session_id }),
            );
            ok(json!({ "deleted": true }))
        }
        Err(e) => err("internal", format!("delete failed: {e}")),
    }
}

