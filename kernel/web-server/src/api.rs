//! RPC 方法分派（契约台账 §2）：52 方法表中的聊天闭环子集。
//! 当前实现子集：host.describe / session.{list,create,history,prompt,cancel,rename} /
//! llm.{providers,models} / workspace.list。其余方法返回 `bad-request`（not implemented 语义
//! 由错误码承载），随 conformance 轮逐步补齐。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use kernel_assembly::Runtime;
use kernel_contracts::llm::LlmModelInfo;
use kernel_contracts::session::{SessionEvent, SessionHeader, SessionId};
use kernel_loop::ReactLoopAgent;
use serde_json::{json, Value};

use crate::events::translate_events;
use crate::rpc::{err, err_with_details, ok};

/// 活跃会话句柄。
pub struct SessionHandle {
    pub agent: Arc<ReactLoopAgent>,
    pub running: bool,
    pub blank: bool,
    pub title: Option<String>,
    /// per-session 模型选择（session.selectModel 写入；prompt 时同步给 agent）。
    pub selected: Option<(String, String)>,
}

/// 真实 provider 运行时（M3）：静态模型清单 + 流式适配器（None = mock 模式）。
pub struct ProviderRuntime {
    pub id: String,
    pub display_name: String,
    pub settings_ns: String,
    pub base_url: String,
    pub models: Vec<LlmModelInfo>,
    pub adapter: Option<Arc<kernel_llm::OpenAICompatLlm>>,
}

impl ProviderRuntime {
    pub fn settings_path(&self) -> Vec<String> {
        vec!["llm".to_string(), self.id.clone()]
    }
}

/// 兼容层应用状态。
pub struct AppState {
    pub runtime: Runtime,
    pub sessions: Mutex<HashMap<String, SessionHandle>>,
    pub version: String,
    pub host_cwd: String,
    /// 实时 wire 事件广播（bus → WS/SSE 下行）：payload 已是 WireSessionEvent JSON。
    pub events_tx: tokio::sync::broadcast::Sender<Value>,
    /// host 流事件广播（HostFrame 下行）：(method, payload) 对，host_loop 包帧发送。
    pub host_events_tx: tokio::sync::broadcast::Sender<(String, Value)>,
    /// 信任栅栏的 trustedHosts（部署时 --trusted-host 传入）。
    pub trusted_hosts: Vec<String>,
    /// 设置命名空间内存视图（M2.5：settings.* 写面的最小存储；持久化后置）。
    pub settings: Mutex<HashMap<String, serde_json::Map<String, Value>>>,
    /// 工作区注册表（M2.5 内存态）：workspaceId → WorkspaceView。
    pub workspaces: Mutex<HashMap<String, Value>>,
    /// 归档会话集（workspace.archiveSession 持久集）。
    pub archived_session_ids: Mutex<Vec<String>>,
    /// 凭据存储（credentials.set/unset 写面；内存态，值永不出域，只报 configured）。
    pub credentials: Mutex<HashMap<String, String>>,
    /// 真实 provider 运行时（M3；空 = mock 单 provider 模式）。
    pub providers: Vec<ProviderRuntime>,
}

impl AppState {
    pub fn new(runtime: Runtime) -> Self {
        Self::with_trusted_hosts(runtime, vec![])
    }

    pub fn with_trusted_hosts(runtime: Runtime, trusted_hosts: Vec<String>) -> Self {
        Self::assemble(runtime, trusted_hosts, vec![])
    }

    pub fn assemble(
        runtime: Runtime,
        trusted_hosts: Vec<String>,
        providers: Vec<ProviderRuntime>,
    ) -> Self {
        let host_cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let (events_tx, _rx) = tokio::sync::broadcast::channel(256);
        let (host_events_tx, _hrx) = tokio::sync::broadcast::channel(256);
        Self {
            runtime,
            sessions: Mutex::new(HashMap::new()),
            version: "0.1.0".to_string(),
            host_cwd,
            events_tx,
            host_events_tx,
            trusted_hosts,
            settings: Mutex::new(HashMap::new()),
            workspaces: Mutex::new(HashMap::new()),
            archived_session_ids: Mutex::new(Vec::new()),
            credentials: Mutex::new(HashMap::new()),
            providers,
        }
    }

    /// 广播一帧 host 事件（HostFrame 下行）。调用方自行构造 payload 全形。
    pub fn broadcast_host(&self, method: impl Into<String>, payload: Value) {
        let _ = self.host_events_tx.send((method.into(), payload));
    }

    /// 全量 workspace 快照（workspace-changed 帧的 value 形态）。
    pub fn workspace_snapshot(&self) -> Value {
        let mut items: Vec<Value> = self.workspaces.lock().unwrap().values().cloned().collect();
        items.sort_by_key(|w| {
            w.get("createdAt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        });
        let archived = self.archived_session_ids.lock().unwrap().clone();
        json!({ "items": items, "archivedSessionIds": archived })
    }

    /// 把 kernel 事件总线接到实时下行通道（幂等：仅调用一次）。
    /// bus listener 是同步闭包：按会话维护翻译游标 + wire seq 累计（每会话从 0 连续，
    /// 与 translate_events 一致；SessionStarted 不翻译但占 record.seq，故不能用 record.seq-1）。
    /// 返回的 Disposer 必须持有（drop 即注销），调用方负责保活到进程结束。
    pub fn attach_event_bus(&self) -> kernel_contracts::bus::Disposer {
        let tx = self.events_tx.clone();
        let per_session: std::sync::Mutex<
            HashMap<String, (crate::events::EventTranslator, i64)>,
        > = std::sync::Mutex::new(HashMap::new());
        let listener = move |record: &kernel_contracts::SessionRecord| {
            let mut table = per_session.lock().unwrap();
            let (trans, seq) = table
                .entry(record.session_id.as_str().to_string())
                .or_insert_with(|| (crate::events::EventTranslator::new(), 0));
            if let Some(mut wire) = trans.translate_one(&record.event) {
                wire.seq = *seq;
                *seq += 1;
                wire.time = record.timestamp.timestamp_millis();
                let payload = json!({
                    "sessionId": record.session_id.as_str(),
                    "event": wire,
                });
                let _ = tx.send(payload);
            }
        };
        self.runtime.bus.on_event(listener)
    }
}

/// 分派入口：method 与路径端点不一致时由调用方先判 bad-request（fetch/handler 语义）。
pub async fn dispatch(state: &Arc<AppState>, method: &str, payload: Value) -> Value {
    match method {
        "host.describe" => host_describe(state),
        "session.list" => session_list(state).await,
        "session.create" => session_create(state, payload).await,
        "session.history" => session_history(state, payload).await,
        "session.search" => session_search(state, payload).await,
        "session.fork" => session_fork(state, payload).await,
        "session.prompt" => session_prompt(state, payload).await,
        "session.cancel" => session_cancel(state, payload),
        "session.rename" => session_rename(state, payload),
        "session.models" => session_models(state, payload),
        "session.selectModel" => session_select_model(state, payload),
        "llm.providers" => llm_providers(state).await,
        "llm.models" => llm_models(state).await,
        "llm.discoverModels" => llm_discover_models(state, payload).await,
        "workspace.list" => workspace_list(state),
        "workspace.create" => workspace_create(state, payload),
        "workspace.rename" => workspace_rename(state, payload),
        "workspace.delete" => workspace_delete(state, payload),
        "workspace.insertBefore" => workspace_insert_before(state, payload),
        "workspace.insertSessionBefore" => workspace_insert_session_before(state, payload),
        "workspace.archiveSession" => workspace_archive_session(state, payload).await,
        "agentPreset.list" => agent_preset_list(),
        "skill.list" => skill_list(payload),
        "host.listDirectory" => host_list_directory(payload),
        "host.createDirectory" => host_create_directory(payload),
        // M2.5：无 OS 目录选择对话框，pickDirectory 返回服务 cwd 作默认工作目录
        // （契约形状 {path: string|null} 对齐；null=用户取消）。
        "host.pickDirectory" => host_pick_directory(state),
        "settings.describe" => settings_describe(state),
        "settings.update" => settings_update(state, payload),
        "settings.replace" => settings_replace(state, payload),
        "settings.mutate" => settings_mutate(state, payload),
        "credentials.describe" => credentials_describe(state, payload),
        "credentials.set" => credentials_set(state, payload),
        "credentials.unset" => credentials_unset(state, payload),
        _ => err(
            "bad-request",
            format!("method \"{method}\" is not implemented by this server"),
        ),
    }
}

fn host_describe(state: &AppState) -> Value {
    let attached = state
        .sessions
        .lock()
        .unwrap()
        .values()
        .filter(|s| s.running || !s.blank)
        .count();
    ok(json!({
        "version": state.version,
        "cwd": state.host_cwd,
        "provider": state.runtime.provider,
        "model": state.runtime.model,
        "attachedSessions": attached,
        "canOpenPath": false,
    }))
}

async fn session_list(state: &Arc<AppState>) -> Value {
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

async fn session_create(state: &Arc<AppState>, payload: Value) -> Value {
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
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);

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
            // prepend 到 sessionIds（活动时显示序首）。未知 workspace → workspace-not-found。
            if let Some(ws_id) = payload.get("workspaceId").and_then(Value::as_str) {
                let mut ws = state.workspaces.lock().unwrap();
                let Some(view) = ws.get_mut(ws_id) else {
                    return err_with_details(
                        "workspace-not-found",
                        "workspace not found",
                        json!({ "workspaceId": ws_id }),
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
                    agent: Arc::new(agent),
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
            // workspace attach 后也广播 workspace 快照。
            if payload.get("workspaceId").and_then(Value::as_str).is_some() {
                state.broadcast_host("host/workspace-changed", state.workspace_snapshot());
            }
            ok(json!({ "sessionId": session_id, "agentPreset": "standard" }))
        }
        Err(e) => err("internal", format!("session create failed: {e}")),
    }
}

async fn session_history(state: &Arc<AppState>, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let events = match state.runtime.persist.load_events(session_id).await {
        Ok(Some(e)) => e,
        Ok(None) => return err("session-not-found", format!("session {session_id} not found")),
        Err(e) => return err("internal", format!("history failed: {e}")),
    };
    let wire = translate_events(&events);
    let items: Vec<Value> = wire
        .iter()
        .map(|ev| json!({ "event": ev }))
        .collect();
    // projections：tail 页才有；M1 简化给空。
    ok(json!({
        "events": items,
        "hasMore": false,
        "projections": { "asOfSeq": -1i64, "values": {} }
    }))
}

/// 搜索结果限制（台账 §2 session.search：`SESSION_SEARCH_RESULT_LIMIT=20`、
/// `SESSION_SEARCH_SNIPPET_MAX_CODE_POINTS=240`）。
const SESSION_SEARCH_RESULT_LIMIT: usize = 20;
const SESSION_SEARCH_SNIPPET_MAX_CODE_POINTS: usize = 240;

/// session.search：query trim 后 1-500 字符、禁 NUL；扫全部会话日志找文本匹配，
/// snippet 取匹配点附近窗口（≤240 code points），结果 ≤20 条。
async fn session_search(state: &Arc<AppState>, payload: Value) -> Value {
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
        let events = match state.runtime.persist.load_events(&sid).await {
            Ok(Some(e)) => e,
            _ => continue,
        };
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
                SessionEvent::AssistantMessage { content } => {
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
fn make_snippet(text: &str, query: &str) -> Option<String> {
    let pos = text.find(query)?;
    let max = SESSION_SEARCH_SNIPPET_MAX_CODE_POINTS;
    // 以匹配点为中心取窗口：匹配前留 100、匹配后留到 240（不足则向前补）。
    let total_chars = text.chars().count();
    let lead = 100usize;
    let start_char = pos.saturating_sub(lead).min(total_chars.saturating_sub(max));
    let mut out: String = text.chars().skip(start_char).take(max).collect();
    if out.chars().count() >= max {
        out = out.chars().take(max.saturating_sub(1)).collect::<String>() + "…";
    }
    // 折叠连续空白为单空格，trim。
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(collapsed)
}

/// session.fork：以 atSeq 锚定第一个 ≥atSeq 的 turn/end；省略/越界回退最后完成 turn。
/// 复制源日志（不含 SessionStarted）到新会话（新 id、连续 seq）。返回 `{sessionId}`。
/// 日志中无完成 turn → `fork-unavailable`。
async fn session_fork(state: &Arc<AppState>, payload: Value) -> Value {
    use kernel_contracts::session::TurnEvent;

    let Some(source_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let at_seq = payload.get("atSeq").and_then(Value::as_u64);
    let events = match state.runtime.persist.load_events(source_id).await {
        Ok(Some(e)) => e,
        Ok(None) => {
            return err("session-not-found", format!("session {source_id} not found"))
        }
        Err(e) => return err("internal", format!("fork failed: {e}")),
    };

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
            return err("internal", format!("fork persist failed: {e}"));
        }
    }

    let mut sessions = state.sessions.lock().unwrap();
    sessions.insert(
        new_id.clone(),
        SessionHandle {
            agent: Arc::new(agent),
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

async fn session_prompt(state: &Arc<AppState>, payload: Value) -> Value {
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
        state2.broadcast_host(
            "host/session-status",
            json!({ "sessionId": sid, "running": false }),
        );
    });
    ok(json!({ "accepted": true }))
}

/// mock provider 模型分组（llm.models / session.models 共用）。
fn mock_model_group() -> Value {
    json!({
        "id": "mock",
        "provider": "mock",
        "label": "Mock",
        "name": "Mock",
        "models": [{ "id": "mock-1", "name": "Mock 1" }],
    })
}

/// 模型分组目录（llm.models / session.models 共用）：真 provider 时每 provider 一组，
/// mock 模式（无配置）时单 mock 组。
fn model_groups(state: &AppState) -> Value {
    if state.providers.is_empty() {
        return json!([mock_model_group()]);
    }
    let groups: Vec<Value> = state
        .providers
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "provider": p.id,
                "label": p.display_name,
                "name": p.display_name,
                "models": p.models.iter().map(|m| json!({
                    "id": m.id,
                    "name": m.label.clone().unwrap_or_else(|| m.id.clone()),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!(groups)
}

/// 当前模型选择：(provider, model)——会话 override 优先，否则运行时默认。
fn current_model(state: &AppState, session_id: &str) -> (String, String) {
    if let Some(h) = state.sessions.lock().unwrap().get(session_id) {
        if let Some(sel) = &h.selected {
            return sel.clone();
        }
    }
    (state.runtime.provider.clone(), state.runtime.model.clone())
}

/// session.models：当前选择 + 可路由 + 目录（subagent → `agent-busy` 未区分，M2.5 简化）。
fn session_models(state: &AppState, payload: Value) -> Value {
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
fn session_select_model(state: &AppState, payload: Value) -> Value {
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

fn session_cancel(_state: &Arc<AppState>, payload: Value) -> Value {
    let Some(_session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    // M1：无取消实现（ReactLoopAgent 无 cancel 端口）；接受但注明。
    ok(json!({ "accepted": true }))
}

fn session_rename(state: &Arc<AppState>, payload: Value) -> Value {
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

async fn llm_providers(state: &AppState) -> Value {
    if state.providers.is_empty() {
        // M1：单一 mock provider。
        return ok(json!({
            "providers": [{
                "provider": state.runtime.provider,
                "displayName": "Mock",
                "settingsNs": "llm.mock",
                "settingsPath": ["llm", "mock"],
                "active": true,
            }]
        }));
    }
    let providers: Vec<Value> = state
        .providers
        .iter()
        .map(|p| {
            json!({
                "provider": p.id,
                "displayName": p.display_name,
                "settingsNs": p.settings_ns,
                "settingsPath": p.settings_path(),
                "active": true,
            })
        })
        .collect();
    ok(json!({ "providers": providers }))
}

async fn llm_models(state: &AppState) -> Value {
    if state.providers.is_empty() {
        // mock 模式：单 mock 组。
        return ok(json!({
            "groups": [mock_model_group()],
            "failures": []
        }));
    }
    ok(json!({
        "groups": model_groups(state),
        "failures": []
    }))
}

/// llm.discoverModels（特权）：真实探测。settingsNs 匹配到已装配 provider →
/// 用其 API 请求模型列表端点；不匹配/失败 → `model-discovery-failed`。
async fn llm_discover_models(state: &AppState, payload: Value) -> Value {
    let settings_ns = payload
        .get("settingsNs")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(provider) = state.providers.iter().find(|p| p.settings_ns == settings_ns) else {
        // 无配置（mock 模式）或未知 ns：回退 mock 已知 ns。
        if state.providers.is_empty() && (settings_ns == "llm.mock" || settings_ns.is_empty()) {
            return ok(json!({
                "models": [{ "id": "mock-1", "name": "Mock 1" }]
            }));
        }
        return err_with_details(
            "model-discovery-failed",
            "no provider for this settings namespace",
            json!({ "settingsNs": settings_ns }),
        );
    };
    let Some(adapter) = &provider.adapter else {
        return err_with_details(
            "model-discovery-failed",
            "provider has no discovery endpoint",
            json!({ "settingsNs": settings_ns }),
        );
    };
    match adapter.list_models_remote().await {
        Ok(models) => ok(json!({
            "models": models.iter().map(|m| json!({
                "id": m.id,
                "name": m.label.clone().unwrap_or_else(|| m.id.clone()),
                "contextWindow": null,
                "maxTokens": null,
            })).collect::<Vec<_>>()
        })),
        Err(e) => err_with_details(
            "model-discovery-failed",
            e.message,
            json!({ "settingsNs": settings_ns, "baseURL": provider.base_url }),
        ),
    }
}

/// workspace.list：内存注册表快照（createdAt/updatedAt 为 ISO-8601 string）+ 归档集。
fn workspace_list(state: &AppState) -> Value {
    ok(state.workspace_snapshot())
}

/// workspace.create（未特权）：对已存在目录幂等注册。
/// 目录缺失/非目录 → `workspace-invalid-path`；已属某 workspace → 返回该 workspace（created:false）。
fn workspace_create(state: &AppState, payload: Value) -> Value {
    use std::path::Path;

    let Some(path) = payload.get("path").and_then(Value::as_str) else {
        return err("bad-request", "missing path");
    };
    let p = Path::new(path);
    if !p.is_dir() {
        return err_with_details(
            "workspace-invalid-path",
            "path is not a directory",
            json!({ "path": path }),
        );
    }

    // 已注册同路径 → 幂等返回（created:false）。
    let mut ws = state.workspaces.lock().unwrap();
    for v in ws.values() {
        if v.get("path").and_then(Value::as_str) == Some(path) {
            return ok(json!({ "workspace": v, "created": false }));
        }
    }
    let now = chrono::Utc::now().to_rfc3339();
    let workspace = json!({
        "workspaceId": uuid::Uuid::new_v4().to_string(),
        "path": path,
        "title": p.file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string(),
        "sessionIds": [],
        "createdAt": now,
        "updatedAt": now,
    });
    let id = workspace["workspaceId"].as_str().unwrap().to_string();
    ws.insert(id, workspace.clone());
    drop(ws);
    state.broadcast_host("host/workspace-changed", state.workspace_snapshot());
    ok(json!({ "workspace": workspace, "created": true }))
}

/// workspace.rename：title trim 后非空；未知 id → `workspace-not-found`；冲突 → `workspace-name-conflict`；
/// 改回原名 = 空操作成功。返回更新后的 workspace。
fn workspace_rename(state: &AppState, payload: Value) -> Value {
    let Some(workspace_id) = payload.get("workspaceId").and_then(Value::as_str) else {
        return err("bad-request", "missing workspaceId");
    };
    let Some(title) = payload.get("title").and_then(Value::as_str).map(str::trim) else {
        return err("bad-request", "missing title");
    };
    if title.is_empty() {
        return err("bad-request", "title must be non-empty");
    }
    let mut ws = state.workspaces.lock().unwrap();
    // 标题冲突预检（不持可变借用）：另一 workspace 已用同名（改回原名除外）。
    for v in ws.values() {
        if v.get("title").and_then(Value::as_str) == Some(title)
            && v.get("workspaceId").and_then(Value::as_str) != Some(workspace_id)
        {
            return err_with_details(
                "workspace-name-conflict",
                "workspace title already in use",
                json!({ "name": title }),
            );
        }
    }
    let Some(view) = ws.get_mut(workspace_id) else {
        return err_with_details(
            "workspace-not-found",
            "workspace not found",
            json!({ "workspaceId": workspace_id }),
        );
    };
    view["title"] = json!(title);
    view["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let updated = view.clone();
    drop(ws);
    state.broadcast_host("host/workspace-changed", state.workspace_snapshot());
    ok(json!({ "workspace": updated }))
}

/// workspace.delete：仅删注册，目录/文件/日志不动；未知 id → `workspace-not-found`。
fn workspace_delete(state: &AppState, payload: Value) -> Value {
    let Some(workspace_id) = payload.get("workspaceId").and_then(Value::as_str) else {
        return err("bad-request", "missing workspaceId");
    };
    let mut ws = state.workspaces.lock().unwrap();
    if ws.remove(workspace_id).is_none() {
        return err_with_details(
            "workspace-not-found",
            "workspace not found",
            json!({ "workspaceId": workspace_id }),
        );
    }
    drop(ws);
    // HostFrame：注册删除增量（台账 §3.1 host/workspace-removed）+ 快照。
    state.broadcast_host(
        "host/workspace-removed",
        json!({ "workspaceId": workspace_id }),
    );
    state.broadcast_host("host/workspace-changed", state.workspace_snapshot());
    ok(json!({ "deleted": true }))
}

/// workspace.insertBefore：DOM-insertBefore 语义；省略锚点 = 追加末尾。
/// 返回完整显示序。
fn workspace_insert_before(state: &AppState, payload: Value) -> Value {
    let Some(workspace_id) = payload.get("workspaceId").and_then(Value::as_str) else {
        return err("bad-request", "missing workspaceId");
    };
    let before = payload.get("beforeWorkspaceId").and_then(Value::as_str);
    let ws = state.workspaces.lock().unwrap();
    let ids: Vec<String> = ws.keys().cloned().collect();
    if !ids.contains(&workspace_id.to_string()) {
        return err_with_details(
            "workspace-not-found",
            "workspace not found",
            json!({ "workspaceId": workspace_id }),
        );
    }
    if let Some(anchor) = before {
        if !ids.contains(&anchor.to_string()) {
            return err_with_details(
                "workspace-not-found",
                "anchor workspace not found",
                json!({ "workspaceId": anchor }),
            );
        }
    }
    // 重排：把 workspace_id 从当前位移除，插到锚点前（无锚点 = 末尾）。
    let mut order: Vec<String> = ids
        .into_iter()
        .filter(|id| id != workspace_id)
        .collect();
    match before {
        Some(anchor) => {
            if let Some(pos) = order.iter().position(|id| id == anchor) {
                order.insert(pos, workspace_id.to_string());
            } else {
                order.push(workspace_id.to_string());
            }
        }
        None => order.push(workspace_id.to_string()),
    }
    drop(ws);
    // HostFrame：重排后完整持久序（台账 §3.1 host/workspace-order-changed）。
    state.broadcast_host(
        "host/workspace-order-changed",
        json!({ "workspaceIds": order }),
    );
    ok(json!({ "workspaceIds": order }))
}

/// workspace.insertSessionBefore：把 sessionId 加进（或重排）workspace.sessionIds。
/// 未知 workspace → `workspace-not-found`；session/锚点不在账 → `workspace-move-invalid`；
/// 原位移动 = 空操作。返回更新后的 workspace。
fn workspace_insert_session_before(state: &AppState, payload: Value) -> Value {
    let Some(workspace_id) = payload.get("workspaceId").and_then(Value::as_str) else {
        return err("bad-request", "missing workspaceId");
    };
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let before = payload.get("beforeSessionId").and_then(Value::as_str);
    let mut ws = state.workspaces.lock().unwrap();
    let Some(view) = ws.get_mut(workspace_id) else {
        return err_with_details(
            "workspace-not-found",
            "workspace not found",
            json!({ "workspaceId": workspace_id }),
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
    // 会话须已在账（新会话由前端 create 后 insert 进来）。
    if !session_ids.contains(&session_id.to_string()) {
        return err_with_details(
            "workspace-move-invalid",
            "session is not in this workspace",
            json!({
                "workspaceId": workspace_id,
                "sessionId": session_id,
                "beforeSessionId": before,
            }),
        );
    }
    if let Some(anchor) = before {
        if !session_ids.contains(&anchor.to_string()) {
            return err_with_details(
                "workspace-move-invalid",
                "anchor session is not in this workspace",
                json!({ "workspaceId": workspace_id, "sessionId": session_id }),
            );
        }
    }
    // 原位移动 = 空操作成功。
    let already_in_place = before.is_none()
        || session_ids.last() == before.map(|b| b.to_string()).as_ref();
    if !already_in_place {
        session_ids.retain(|id| id != session_id);
        match before {
            Some(anchor) => {
                if let Some(pos) = session_ids.iter().position(|id| id == anchor) {
                    session_ids.insert(pos, session_id.to_string());
                } else {
                    session_ids.push(session_id.to_string());
                }
            }
            None => session_ids.push(session_id.to_string()),
        }
        view["sessionIds"] = json!(session_ids);
        view["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    }
    let updated = view.clone();
    drop(ws);
    state.broadcast_host("host/workspace-changed", state.workspace_snapshot());
    ok(json!({ "workspace": updated }))
}

/// workspace.archiveSession：把 sessionId 加入归档集（幂等）；会话既非 live 也不在持久化
/// → `session-not-found`。返回完整新归档集。
async fn workspace_archive_session(state: &AppState, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    // 会话存在性：live 或持久化。
    let live = state.sessions.lock().unwrap().contains_key(session_id);
    let persisted = state
        .runtime
        .persist
        .list_sessions()
        .await
        .map(|ids| ids.contains(&session_id.to_string()))
        .unwrap_or(false);
    if !live && !persisted {
        return err_with_details(
            "session-not-found",
            "session not found",
            json!({ "sessionId": session_id }),
        );
    }
    let mut archived = state.archived_session_ids.lock().unwrap();
    if !archived.contains(&session_id.to_string()) {
        archived.push(session_id.to_string());
    }
    let new_set = archived.clone();
    drop(archived);
    // HostFrame：归档集每次持久化变更后全量（台账 §3.1 host/archived-sessions-changed）。
    state.broadcast_host(
        "host/archived-sessions-changed",
        json!({ "archivedSessionIds": new_set }),
    );
    ok(json!({ "archivedSessionIds": new_set }))
}

/// host.pickDirectory（特权）：无 OS 对话框实现 → 返回服务 cwd 作默认选择。
fn host_pick_directory(state: &AppState) -> Value {
    ok(json!({ "path": state.host_cwd }))
}

/// 目录条目隐藏判定：`.` 前缀（Unix 惯例）+ Windows FILE_ATTRIBUTE_HIDDEN。
fn is_hidden_path(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with('.') && !name.is_empty() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if let Ok(meta) = p.metadata() {
            return meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0;
        }
    }
    false
}

/// host.listDirectory（特权）：列一个目录层级 + 祖先 breadcrumb。
/// 缺省 path = 家目录；不可读 → `directory-unreadable {path}`。
fn host_list_directory(payload: Value) -> Value {
    use std::path::{Component, PathBuf};

    let raw = payload.get("path").and_then(Value::as_str);
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let home_str = home.to_string_lossy().to_string();
    let target: PathBuf = match raw {
        Some(p) if !p.is_empty() => {
            let p = PathBuf::from(p);
            if p.is_absolute() { p } else { home.join(p) }
        }
        _ => home,
    };

    // 目录必须存在且可读。
    let read = match std::fs::read_dir(&target) {
        Ok(rd) => rd,
        Err(e) => {
            return err_with_details(
                "directory-unreadable",
                format!("cannot read directory: {e}"),
                json!({ "path": target.to_string_lossy() }),
            );
        }
    };

    let mut entries: Vec<Value> = Vec::new();
    for item in read.flatten() {
        let path = item.path();
        entries.push(json!({
            "name": item.file_name().to_string_lossy(),
            "path": path.to_string_lossy(),
            "hidden": is_hidden_path(&path),
        }));
    }
    entries.sort_by(|a, b| {
        let an = a["name"].as_str().unwrap_or("");
        let bn = b["name"].as_str().unwrap_or("");
        an.to_lowercase().cmp(&bn.to_lowercase())
    });

    // crumbs：从根到当前目录每层一个 {name, path}（hidden 恒 false）。
    // Windows 上 Prefix("D:") + RootDir("\") 合并为一段 "D:\"。
    let mut crumbs: Vec<Value> = Vec::new();
    let mut acc = PathBuf::new();
    let mut pending_prefix: Option<String> = None;
    for comp in target.components() {
        match comp {
            Component::Prefix(_) => {
                acc.push(comp.as_os_str());
                pending_prefix = Some(acc.to_string_lossy().to_string());
            }
            Component::RootDir => {
                acc.push(comp.as_os_str());
                crumbs.push(json!({
                    "name": pending_prefix
                        .take()
                        .unwrap_or_else(|| acc.to_string_lossy().trim_end_matches(['/', '\\']).to_string()),
                    "path": acc.to_string_lossy(),
                    "hidden": false,
                }));
            }
            Component::Normal(seg) => {
                acc.push(seg);
                crumbs.push(json!({
                    "name": seg.to_string_lossy(),
                    "path": acc.to_string_lossy(),
                    "hidden": false,
                }));
            }
            _ => {}
        }
    }
    // 家目录缺省空 crumbs 时的兜底：至少一段。
    if crumbs.is_empty() {
        crumbs.push(json!({
            "name": home_str,
            "path": home_str,
            "hidden": false,
        }));
    }

    ok(json!({
        "path": target.to_string_lossy(),
        "home": home_str,
        "crumbs": crumbs,
        "entries": entries,
        "truncated": false,
    }))
}

/// host.createDirectory（特权）：name 须单路径段；已存在 → `directory-exists`；
/// 创建失败 → `directory-create-failed`。返回创建后的绝对路径。
fn host_create_directory(payload: Value) -> Value {
    use std::path::PathBuf;

    let Some(path) = payload.get("path").and_then(Value::as_str) else {
        return err("bad-request", "missing path");
    };
    let Some(name) = payload.get("name").and_then(Value::as_str) else {
        return err("bad-request", "missing name");
    };
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return err(
            "bad-request",
            "host.createDirectory requires a single non-blank path segment name",
        );
    }
    let dir = PathBuf::from(path).join(name);
    if dir.exists() {
        return err_with_details(
            "directory-exists",
            "directory already exists",
            json!({ "path": dir.to_string_lossy() }),
        );
    }
    match std::fs::create_dir(&dir) {
        Ok(()) => ok(json!({ "path": dir.to_string_lossy() })),
        Err(e) => err_with_details(
            "directory-create-failed",
            format!("create directory failed: {e}"),
            json!({ "path": dir.to_string_lossy() }),
        ),
    }
}

/// agentPreset.list（未特权）：M2.5 返回空清单（无 authoring 预设）。
/// hasDocument=false：无原生 preset 文档可编辑（特权面 openDocument 相关）。
fn agent_preset_list() -> Value {
    ok(json!({ "presets": [], "authorable": true, "hasDocument": false }))
}

/// skill.list：M1 无技能注册，返回空清单（技能调用是 session.prompt 前导 `/name`）。
fn skill_list(payload: Value) -> Value {
    if payload.get("sessionId").and_then(Value::as_str).is_none() {
        return err("bad-request", "missing sessionId");
    }
    ok(json!({ "skills": [] }))
}

/// Web 可写设置命名空间白名单（台账 §2 settings.*：WEB_SETTINGS_NAMESPACES 子集）。
const WEB_SETTINGS_NAMESPACES: &[&str] = &[
    "agent-loop",
    "shell",
    "locale",
    "permission",
    "ui-conversation",
    "ui-theme",
    "ui-onboarding",
];

/// 构造单个 SettingsNamespaceView（台账 §2：{ns, schema, value, base?, user?, applies, secrets, revision}）。
/// 脱敏铁律：secret 字段永不随响应出域（M2.5 无 secret 字段，secrets 槽恒空）。
fn settings_view(ns: &str, value: &serde_json::Map<String, Value>) -> Value {
    json!({
        "ns": ns,
        "schema": {},
        "value": value,
        "applies": "restart",
        "secrets": [],
        "revision": 0,
    })
}

/// settings.describe（特权）：返回白名单命名空间视图。
fn settings_describe(state: &AppState) -> Value {
    let settings = state.settings.lock().unwrap();
    let mut namespaces = Vec::new();
    for ns in WEB_SETTINGS_NAMESPACES {
        let value = settings.get(*ns).cloned().unwrap_or_default();
        namespaces.push(settings_view(ns, &value));
    }
    ok(json!({ "writable": true, "hasDocument": false, "namespaces": namespaces }))
}

/// settings.update（特权）：整 ns patch 合并（对象深合并，JSON patch 语义近似）。
fn settings_update(state: &AppState, payload: Value) -> Value {
    settings_write(state, payload, |cur, payload| {
        if let Some(patch) = payload.get("patch").and_then(Value::as_object) {
            for (k, v) in patch {
                cur.insert(k.clone(), v.clone());
            }
        }
    })
}

/// settings.replace（特权）：整段替换。
fn settings_replace(state: &AppState, payload: Value) -> Value {
    settings_write(state, payload, |cur, payload| {
        if let Some(section) = payload.get("section").and_then(Value::as_object) {
            *cur = section.clone();
        }
    })
}

/// settings.mutate（特权）：{op:'set',path,value} / {op:'unset',path} 点路径写。
fn settings_mutate(state: &AppState, payload: Value) -> Value {
    settings_write(state, payload, |cur, payload| {
        let Some(ops) = payload.get("ops").and_then(Value::as_array) else {
            return;
        };
        for op in ops {
            let Some(op_kind) = op.get("op").and_then(Value::as_str) else {
                continue;
            };
            let path: Vec<String> = op
                .get("path")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            match op_kind {
                "set" => {
                    if let Some(value) = op.get("value").cloned() {
                        if path.is_empty() {
                            continue;
                        }
                        let last = path.len() - 1;
                        let mut node = &mut *cur;
                        for (i, seg) in path.iter().enumerate() {
                            if i == last {
                                node.insert(seg.clone(), value.clone());
                            } else {
                                // 中间段：缺失或非对象 → 以 {} 垫底后下钻。
                                let missing = match node.get(seg) {
                                    Some(v) => !v.is_object(),
                                    None => true,
                                };
                                if missing {
                                    node.insert(seg.clone(), json!({}));
                                }
                                node = node.get_mut(seg).unwrap().as_object_mut().unwrap();
                            }
                        }
                    }
                }
                "unset" => {
                    if let Some(last) = path.last() {
                        cur.remove(last);
                    }
                }
                _ => {}
            }
        }
    })
}

/// 共用写面：定位 ns → 应用闭包变更 → 返回新视图。
/// 未知 ns → `settings-rejected {ns}`（台账：schema 校验/未知 ns/只读 provider → settings-rejected）。
fn settings_write<F>(state: &AppState, payload: Value, apply: F) -> Value
where
    F: FnOnce(&mut serde_json::Map<String, Value>, &Value),
{
    let Some(ns) = payload.get("ns").and_then(Value::as_str) else {
        return err("bad-request", "missing ns");
    };
    if !WEB_SETTINGS_NAMESPACES.contains(&ns) {
        return err_with_details("settings-rejected", "namespace not writable", json!({ "ns": ns }));
    }
    let mut settings = state.settings.lock().unwrap();
    let mut value = settings.get(ns).cloned().unwrap_or_default();
    apply(&mut value, &payload);
    settings.insert(ns.to_string(), value.clone());
    ok(settings_view(ns, &value))
}

/// credentials.describe（特权）：永不带值，只报 configured/writable。
fn credentials_describe(state: &AppState, payload: Value) -> Value {
    let refs = payload
        .get("refs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let store = state.credentials.lock().unwrap();
    let mut credentials = serde_json::Map::new();
    for r in refs {
        if let Some(name) = r.as_str() {
            credentials.insert(
                name.to_string(),
                json!({ "configured": store.contains_key(name), "writable": true }),
            );
        }
    }
    ok(json!({ "credentials": credentials }))
}

/// ref 名校验：`/^[A-Za-z_][A-Za-z0-9_]*$/`（台账 §2 credentials.*）。
fn valid_credential_ref(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// credentials.set（特权）：ref + value(≥1) → {}。内存存储（持久化后置）。
/// ref 非法 → bad-request；value 空 → bad-request。
fn credentials_set(state: &AppState, payload: Value) -> Value {
    let Some(name) = payload.get("ref").and_then(Value::as_str) else {
        return err("bad-request", "missing ref");
    };
    if !valid_credential_ref(name) {
        return err(
            "bad-request",
            "ref must match /^[A-Za-z_][A-Za-z0-9_]*$/",
        );
    }
    let Some(value) = payload.get("value").and_then(Value::as_str) else {
        return err("bad-request", "missing value");
    };
    if value.is_empty() {
        return err("bad-request", "value must be at least 1 character");
    }
    state.credentials.lock().unwrap().insert(name.to_string(), value.to_string());
    ok(json!({}))
}

/// credentials.unset（特权）：ref → {}（无引用也成功）。
fn credentials_unset(state: &AppState, payload: Value) -> Value {
    let Some(name) = payload.get("ref").and_then(Value::as_str) else {
        return err("bad-request", "missing ref");
    };
    if !valid_credential_ref(name) {
        return err(
            "bad-request",
            "ref must match /^[A-Za-z_][A-Za-z0-9_]*$/",
        );
    }
    state.credentials.lock().unwrap().remove(name);
    ok(json!({}))
}
