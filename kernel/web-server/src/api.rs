//! RPC 方法分派（契约台账 §2）：52 方法表中的聊天闭环子集。
//! 当前实现子集：host.describe / session.{list,create,history,prompt,cancel,rename} /
//! llm.{providers,models} / workspace.list。其余方法返回 `bad-request`（not implemented 语义
//! 由错误码承载），随 conformance 轮逐步补齐。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kernel_assembly::Runtime;
use kernel_contracts::session::{SessionHeader, SessionId};
use kernel_loop::ReactLoopAgent;
use serde_json::{json, Value};

use crate::events::translate_events;
use crate::rpc::{err, ok};

/// 活跃会话句柄。
pub struct SessionHandle {
    pub agent: Arc<ReactLoopAgent>,
    pub running: bool,
    pub blank: bool,
    pub title: Option<String>,
}

/// 兼容层应用状态。
pub struct AppState {
    pub runtime: Runtime,
    pub sessions: Mutex<HashMap<String, SessionHandle>>,
    pub version: String,
    pub host_cwd: String,
    /// 实时 wire 事件广播（bus → WS/SSE 下行）：payload 已是 WireSessionEvent JSON。
    pub events_tx: tokio::sync::broadcast::Sender<Value>,
    /// 信任栅栏的 trustedHosts（部署时 --trusted-host 传入）。
    pub trusted_hosts: Vec<String>,
}

impl AppState {
    pub fn new(runtime: Runtime) -> Self {
        Self::with_trusted_hosts(runtime, vec![])
    }

    pub fn with_trusted_hosts(runtime: Runtime, trusted_hosts: Vec<String>) -> Self {
        let host_cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let (events_tx, _rx) = tokio::sync::broadcast::channel(256);
        Self {
            runtime,
            sessions: Mutex::new(HashMap::new()),
            version: "0.1.0".to_string(),
            host_cwd,
            events_tx,
            trusted_hosts,
        }
    }

    /// 把 kernel 事件总线接到实时下行通道（幂等：仅调用一次）。
    /// bus listener 是同步闭包：锁翻译游标 → 翻译单条 → 塞进 broadcast。
    pub fn attach_event_bus(&self) {
        let tx = self.events_tx.clone();
        let translator = std::sync::Mutex::new(crate::events::EventTranslator::new());
        let listener = move |record: &kernel_contracts::SessionRecord| {
            let mut trans = translator.lock().unwrap();
            if let Some(wire) = trans.translate_one(&record.event) {
                let _ = tx.send(serde_json::to_value(&wire).unwrap_or_default());
            }
        };
        let _ = self.runtime.bus.on_event(listener);
    }
}

/// 分派入口：method 与路径端点不一致时由调用方先判 bad-request（fetch/handler 语义）。
pub async fn dispatch(state: &Arc<AppState>, method: &str, payload: Value) -> Value {
    match method {
        "host.describe" => host_describe(state),
        "session.list" => session_list(state).await,
        "session.create" => session_create(state, payload).await,
        "session.history" => session_history(state, payload).await,
        "session.prompt" => session_prompt(state, payload).await,
        "session.cancel" => session_cancel(state, payload),
        "session.rename" => session_rename(state, payload),
        "llm.providers" => llm_providers(state).await,
        "llm.models" => llm_models(state).await,
        "workspace.list" => workspace_list(),
        "settings.describe" => settings_describe(),
        "credentials.describe" => credentials_describe(payload),
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
    let cwd = payload.get("cwd").and_then(Value::as_str).map(str::to_string);

    let header = SessionHeader {
        id: SessionId(session_id.clone()),
        app: "web".into(),
        profile: "web".into(),
        workspace: cwd,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    match state.runtime.create_session(header).await {
        Ok(agent) => {
            let mut sessions = state.sessions.lock().unwrap();
            sessions.insert(
                session_id.clone(),
                SessionHandle {
                    agent: Arc::new(agent),
                    running: false,
                    blank: true,
                    title: None,
                },
            );
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
        Arc::clone(&h.agent)
    };
    let state2 = Arc::clone(state);
    let sid = session_id.to_string();
    tokio::spawn(async move {
        let _ = agent.run_turn(Some(&text)).await;
        if let Some(h) = state2.sessions.lock().unwrap().get_mut(&sid) {
            h.running = false;
        }
    });
    ok(json!({ "accepted": true }))
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
    // M1：单一 mock provider。
    ok(json!({
        "providers": [{
            "provider": state.runtime.provider,
            "displayName": "Mock",
            "settingsNs": "llm.mock",
            "settingsPath": ["llm", "mock"],
            "active": true,
        }]
    }))
}

async fn llm_models(state: &AppState) -> Value {
    let models = state
        .runtime
        .llm
        .list_models(&state.runtime.provider)
        .await
        .unwrap_or_default();
    // Node 形状：groups[{id, provider, label, name, models:[{id, name, ...}]}]。
    let group = json!({
        "id": state.runtime.provider,
        "provider": state.runtime.provider,
        "label": "Mock",
        "name": "Mock",
        "models": models.iter().map(|m| json!({
            "id": m.id,
            "name": m.label.clone().unwrap_or_else(|| m.id.clone()),
        })).collect::<Vec<_>>(),
    });
    ok(json!({
        "groups": [group],
        "failures": []
    }))
}

fn workspace_list() -> Value {
    ok(json!({ "items": [], "archivedSessionIds": [] }))
}

/// settings.describe（特权）：基本形状；namespaces 内容依赖实际配置（动态）。
fn settings_describe() -> Value {
    ok(json!({
        "writable": true,
        "hasDocument": false,
        "namespaces": []
    }))
}

/// credentials.describe（特权）：永不带值，只报 configured/writable。
fn credentials_describe(payload: Value) -> Value {
    let refs = payload
        .get("refs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut credentials = serde_json::Map::new();
    for r in refs {
        if let Some(name) = r.as_str() {
            credentials.insert(
                name.to_string(),
                json!({ "configured": false, "writable": true }),
            );
        }
    }
    ok(json!({ "credentials": credentials }))
}
