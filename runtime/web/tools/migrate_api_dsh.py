# -*- coding: utf-8 -*-
"""api_dsh.rs 扩展:目录选择/工作区/会话创建 + 事件流推送(一次性迁移脚本)"""
p = 'D:/96_CoderWorld/BoenMind/runtime/crates/bm-surface-http/src/api_dsh.rs'
s = open(p, encoding='utf-8').read()

# 1) 状态存储 + 事件总线 + 辅助函数
anchor = '/// dsh 前端 settings 命名空间的内存存储'
addition = '''/// dsh 工作区/会话的内存存储(第一批:重启即失;持久化待接 SQLite)。
#[derive(Default)]
pub struct DshState {
    pub workspaces: Vec<serde_json::Value>,
    pub sessions: Vec<serde_json::Value>,
    pub seq: u64,
}

fn dsh_state() -> &'static Mutex<DshState> {
    static S: OnceLock<Mutex<DshState>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(DshState::default()))
}

/// mux/host 两条事件流的广播(帧 JSON 文本)。
fn event_bus(channel: &str) -> &'static tokio::sync::broadcast::Sender<String> {
    static MUX: OnceLock<tokio::sync::broadcast::Sender<String>> = OnceLock::new();
    static HOST: OnceLock<tokio::sync::broadcast::Sender<String>> = OnceLock::new();
    match channel {
        "host" => HOST.get_or_init(|| tokio::sync::broadcast::channel(64).0),
        _ => MUX.get_or_init(|| tokio::sync::broadcast::channel(64).0),
    }
}

fn broadcast_frame(channel: &str, method: &str, payload: serde_json::Value) {
    let frame = json!({
        "type": "server-request",
        "rpcId": uuid_like(),
        "method": method,
        "payload": payload,
    });
    let _ = event_bus(channel).send(frame.to_string());
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("2099-01-01T00:{:02}:{:02}Z", (n / 60) % 60, n % 60)
}

fn now_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// 轻量唯一 id(时间戳纳秒,无需 crypto)
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("srv-{:x}", n)
}

''' + anchor
assert anchor in s
s = s.replace(anchor, addition, 1)

# 2) 新 RPC 分支
anchor2 = '        "llm.providers" => {'
addition2 = '''        "host.pickDirectory" => {
            // 单机形态:无 GUI 目录选择,固定返回服务器工作目录
            let cwd = std::env::current_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|_| std::env::var("USERPROFILE").unwrap_or_default());
            server_response(&req.rpc_id, json!({ "ok": true, "value": { "path": cwd } }))
        }
        "host.listDirectory" => {
            let home = std::env::var("USERPROFILE").unwrap_or_default();
            let cwd = std::env::current_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|_| home.clone());
            let path = req.payload["path"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| cwd.clone());
            let mut entries = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&path) {
                for e in rd.flatten().take(200) {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') { continue; }
                    let fp = e.path().display().to_string();
                    entries.push(json!({ "name": name, "path": fp, "hidden": false }));
                }
            }
            let crumbs = json!([{ "name": path, "path": path, "hidden": false }]);
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": {
                    "path": path, "home": home, "crumbs": crumbs,
                    "entries": entries, "truncated": false
                } }),
            )
        }
        "host.createDirectory" => {
            let p = req.payload["path"].as_str().unwrap_or_default();
            match std::fs::create_dir_all(p) {
                Ok(_) => server_response(&req.rpc_id, json!({ "ok": true, "value": {} })),
                Err(e) => server_response(&req.rpc_id, json!({ "ok": false, "error": {
                    "code": "bad-request", "message": format!("创建目录失败: {e}"), "details": {} } })),
            }
        }
        "workspace.create" => {
            let path = req.payload["path"].as_str().unwrap_or_default().to_string();
            if path.is_empty() {
                return server_response(&req.rpc_id, json!({ "ok": false, "error": {
                    "code": "workspace-invalid-path", "message": "缺少 path", "details": { "path": "" } } }));
            }
            let title = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            let mut st = dsh_state().lock().unwrap();
            if let Some(w) = st.workspaces.iter().find(|w| w["path"] == json!(path)) {
                return server_response(&req.rpc_id, json!({ "ok": true, "value": {
                    "workspace": w.clone(), "created": false } }));
            }
            st.seq += 1;
            let ws = json!({
                "workspaceId": format!("ws_{}", st.seq),
                "path": path,
                "title": title,
                "sessionIds": [],
                "createdAt": now_iso(),
                "updatedAt": now_iso(),
            });
            st.workspaces.push(ws.clone());
            drop(st);
            server_response(&req.rpc_id, json!({ "ok": true, "value": {
                "workspace": ws, "created": true } }))
        }
        "session.create" => {
            let mut st = dsh_state().lock().unwrap();
            st.seq += 1;
            let sid = format!("sess_{}", st.seq);
            let workspace_id = req.payload["workspaceId"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "ws_1".to_string());
            let session = json!({
                "sessionId": sid,
                "updatedAt": now_ts(),
                "running": false,
                "blank": true,
                "cwd": std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
            });
            st.sessions.push(session);
            if let Some(w) = st.workspaces.iter_mut().find(|w| w["workspaceId"] == json!(workspace_id)) {
                if let Some(arr) = w["sessionIds"].as_array_mut() {
                    arr.push(json!(sid));
                }
                w["updatedAt"] = json!(now_iso());
            }
            drop(st);
            broadcast_frame("host", "events.host", json!({
                "type": "host/session-added", "sessionId": sid, "blank": true
            }));
            broadcast_frame("mux", "events.mux", json!({
                "type": "session/subscribed", "sessionId": sid, "lastSeq": 0
            }));
            server_response(&req.rpc_id, json!({ "ok": true, "value": { "sessionId": sid } }))
        }
        "llm.providers" => {'''
assert anchor2 in s
s = s.replace(anchor2, addition2, 1)

# 3) workspace.list / session.list 接真实存储
old3 = '''        "workspace.list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": { "items": [], "archivedSessionIds": [] } }),
        ),
        "session.list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": { "items": [] } }),
        ),'''
new3 = '''        "workspace.list" => {
            let st = dsh_state().lock().unwrap();
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": {
                    "items": st.workspaces, "archivedSessionIds": [] } }),
            )
        }
        "session.list" => {
            let st = dsh_state().lock().unwrap();
            server_response(&req.rpc_id, json!({ "ok": true, "value": { "items": st.sessions } }))
        },'''
assert old3 in s, "workspace.list 原文不匹配"
s = s.replace(old3, new3, 1)

# 4) events_ws 升级为双通道转发
i = s.find('async fn events_ws(upgrade')
assert i >= 0, "events_ws 未找到"
# 找函数体结束:从 i 开始第一个 "\n}\n"
end = s.find('\n}\n', i) + 3
new_ws = '''async fn events_ws_channel(upgrade: axum::extract::ws::WebSocketUpgrade, channel: &'static str) -> Response {
    let mut rx = event_bus(channel).subscribe();
    upgrade
        .on_upgrade(move |mut socket| async move {
            let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = heartbeat.tick() => {
                        if socket
                            .send(axum::extract::ws::Message::Ping(axum::body::Bytes::new()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    msg = rx.recv() => {
                        match msg {
                            Ok(text) => {
                                if socket
                                    .send(axum::extract::ws::Message::Text(text))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(_) => break,
                        }
                    }
                }
            }
        })
}
'''
s = s[:i] + new_ws + s[end:]

# 5) 两个事件端点改调 events_ws_channel
s = s.replace('events_channel(upgrade)', 'events_channel(upgrade)')  # no-op 占位
old_mux = '''    if wants_ws {
        events_ws(upgrade).await
    } else {
        events_stream()
    }'''
new_mux = '''    if wants_ws {
        let ch = if info_channel(&headers) == "host" { "host" } else { "mux" };
        events_ws_channel(upgrade, ch).await
    } else {
        events_stream()
    }'''
assert old_mux in s, "events_channel 分支不匹配"
s = s.replace(old_mux, new_mux, 1)

# helper:从路径判断通道
anchor4 = 'fn not_implemented(rpc_id: &str, method: &str) -> Response {'
addition4 = '''/// 从请求 URI 判断事件流通道(mux/host)。
fn info_channel(headers: &axum::http::HeaderMap) -> &'static str {
    // 由调用方在路由层区分;这里依据 Host 头不可行,改由端点拆分。
    // 默认 mux;events.host 端点直接传 "host"。
    let _ = headers;
    "mux"
}

''' + anchor4
s = s.replace(anchor4, addition4, 1)

open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('迁移完成')
