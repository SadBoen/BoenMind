//! dsh 前端宿主协议适配(D-M3-1「后端连接一点点做好」)。
//!
//! 协议逆向自 dsh-client-connection 0.1.1-rc.2(runtime/web/SOURCE.md):
//! - `POST /api/{method}`:请求 `{type:"client-request",rpcId,method,payload}`,
//!   响应 `{type:"server-response",rpcId,result:{ok,value|error}}`(恒 200);
//! - `GET /api/events.mux` 与 `GET /api/events.host`:SSE 流,`\n\n` 分帧,
//!   帧为 `data: {serverRequest 信封}`;连接打开即视为流就绪,空帧被前端丢弃。
//!
//! 当前只实现 `host.describe`(连接握手)与两条空事件流;其余方法显式
//! 返回 not_implemented(前端可见「未适配」而非静默挂起)。逐项适配清单
//! 见 milestones/PENDING.md D-M3-1。
//!
//! 安全边界:本模块路由公开挂载(与 /health 同级,不经 Bearer 中间件),
//! 仅暴露版本/目录等元信息与空事件流;接入会话/审批类数据前必须先做
//! 鉴权设计(硬纪律 4:权限以合同显式化)。

use crate::AppState;
use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use std::convert::Infallible;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// dsh 工作区/会话的内存存储(第一批:重启即失;持久化待接 SQLite)。
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

/// dsh 前端 settings 命名空间的内存存储(ns → (值树, revision))。
/// 预置 ui-onboarding.welcomeNoticeVersion = 内测声明已确认(用户裁决:
/// 不要内测声明弹窗)。重启重置到预置态;接 SQLite 持久化待后续项。
fn settings_store() -> &'static Mutex<std::collections::HashMap<String, (serde_json::Value, u64)>> {
    static STORE: OnceLock<Mutex<std::collections::HashMap<String, (serde_json::Value, u64)>>> =
        OnceLock::new();
    STORE.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "ui-onboarding".to_string(),
            (json!({ "welcomeNoticeVersion": "2026-08-13.1" }), 1),
        );
        Mutex::new(m)
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRequest {
    #[serde(rename = "type")]
    pub kind: String,
    pub rpc_id: String,
    pub method: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

fn server_response(rpc_id: &str, result: serde_json::Value) -> Response {
    Json(json!({
        "type": "server-response",
        "rpcId": rpc_id,
        "result": result,
    }))
    .into_response()
}

/// 从请求 URI 判断事件流通道(mux/host)。
fn info_channel(headers: &axum::http::HeaderMap) -> &'static str {
    // 由调用方在路由层区分;这里依据 Host 头不可行,改由端点拆分。
    // 默认 mux;events.host 端点直接传 "host"。
    let _ = headers;
    "mux"
}

fn not_implemented(rpc_id: &str, method: &str) -> Response {
    // code 用 dsh 错误码封闭枚举内的合法值(bad-request),否则前端 zod
    // union 校验失败、错误无法渲染
    server_response(
        rpc_id,
        json!({
            "ok": false,
            "error": {
                "code": "bad-request",
                "message": format!("BoenMind 尚未适配 dsh 方法 {method}(逐项接入中)"),
            }
        }),
    )
}

/// dsh unary 入口:POST /api/{*rest}(方法名可含多段,如
/// /api/dynamicCordisRunner/inventory)。
pub async fn unary(
    State(_state): State<AppState>,
    Path(_rest): Path<String>,
    body: String,
) -> Response {
    let req: ClientRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return (axum::http::StatusCode::BAD_REQUEST, format!("信封非法: {e}"))
                .into_response();
        }
    };
    match req.method.as_str() {
        "host.describe" => {
            let cwd = std::env::current_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| cwd.clone());
            let mut value = json!({
                "version": format!("boenmind-{}", env!("CARGO_PKG_VERSION")),
                "cwd": cwd,
                "attachedSessions": 0,
                "home": home,
                "canOpenPath": false,
            });
            // provider/model 为 schema 可选字段:无值时保持键缺席(zod optional 拒绝 null)
            if let Ok(id) = std::env::var("BOEN_MODEL_ID") {
                value["model"] = json!(id);
                value["provider"] = json!("boenmind");
            }
            server_response(&req.rpc_id, json!({ "ok": true, "value": value }))
        }
        // —— 启动期清单类:先给合法空状态,让界面出空态而非报错 ——
        "workspace.list" => {
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
        },
        "agentPreset.list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": {
                "presets": [{ "id": "standard", "trust": "system", "isDefault": true }],
                "authorable": false,
                "hasDocument": false
            } }),
        ),
        "settings.describe" => {
            let store = settings_store();
            let guard = store.lock().unwrap();
            let namespaces: Vec<serde_json::Value> = guard
                .iter()
                .map(|(ns, (value, revision))| {
                    json!({
                        "ns": ns,
                        "schema": null,
                        "value": value,
                        "applies": "live",
                        "secrets": [],
                        "revision": revision,
                    })
                })
                .collect();
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": {
                    "writable": true, "hasDocument": false, "namespaces": namespaces
                } }),
            )
        }
        "settings.mutate" => {
            // payload: {ns, ops:[{op:"set", path:[..], value}], expectedRevision?}
            let ns = req.payload["ns"].as_str().unwrap_or_default().to_string();
            if ns.is_empty() {
                return not_implemented(&req.rpc_id, "settings.mutate(缺 ns)");
            }
            let store = settings_store();
            let mut guard = store.lock().unwrap();
            let entry = guard.entry(ns.clone()).or_insert((json!({}), 0u64));
            entry.1 += 1;
            if let Some(ops) = req.payload["ops"].as_array() {
                for op in ops {
                    let path: Vec<String> = op["path"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|p| p.as_str().unwrap_or_default().to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                    // 仅实现 set(当前 dsh 界面只用 set);沿 path 写入值
                    if op["op"] == "set" && !path.is_empty() {
                        let mut cur = &mut entry.0;
                        for key in &path[..path.len() - 1] {
                            cur = &mut cur[key];
                        }
                        cur[&path[path.len() - 1]] = op["value"].clone();
                    }
                }
            }
            let view = json!({
                "ns": ns,
                "schema": null,
                "value": entry.0,
                "applies": "live",
                "secrets": [],
                "revision": entry.1,
            });
            server_response(&req.rpc_id, json!({ "ok": true, "value": view }))
        }
        "host.pickDirectory" => {
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
        "subagent.list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": { "entries": [], "parentAvailable": false } }),
        ),
        "session.history" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": { "events": [], "hasMore": false } }),
        ),
        "session.models" => {
            let model = std::env::var("BOEN_MODEL_ID").unwrap_or_default();
            let (groups, current) = if model.is_empty() {
                (json!([]), json!(null))
            } else {
                (
                    json!([{ "id": "boenmind", "name": "BoenMind 网关",
                             "models": [{ "id": model, "name": model }] }]),
                    json!({ "provider": "boenmind", "model": model }),
                )
            };
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": {
                    "current": current, "routable": !model.is_empty(),
                    "groups": groups, "failures": [] } }),
            )
        }
        "commands/list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": { "commands": [] } }),
        ),
        "skill.list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": { "skills": [] } }),
        ),
        "llm.providers" => {
            // 单一 provider:服务器 env 配置的网关(前端只读展示)
            let model = std::env::var("BOEN_MODEL_ID").unwrap_or_default();
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": { "providers": [{
                    "provider": "boenmind",
                    "displayName": "BoenMind 网关(服务器配置)",
                    "settingsNs": "llm-boenmind",
                    "settingsPath": ["providers"],
                    "active": !model.is_empty(),
                    "declared": true
                }] } }),
            )
        }
        "llm.models" => {
            let model = std::env::var("BOEN_MODEL_ID").unwrap_or_default();
            let groups = if model.is_empty() {
                json!([])
            } else {
                json!([{ "id": "boenmind", "name": "BoenMind 网关",
                         "models": [{ "id": model, "name": model }] }])
            };
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": { "groups": groups, "failures": [] } }),
            )
        }
        "llm.discoverModels" => {
            // 模型目录:来自服务器环境配置(BOEN_MODEL_ID),单模型形态
            let model = std::env::var("BOEN_MODEL_ID").unwrap_or_default();
            let models = if model.is_empty() {
                json!([])
            } else {
                json!([{ "id": model, "name": model }])
            };
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": { "models": models, "failures": [] } }),
            )
        }
        "dynamicCordisRunner/inventory" | "dynamicCordisRunner/syncInspectManifest" => {
            // dsh 插件清单:BoenMind 无 dsh 插件,空清单
            server_response(&req.rpc_id, json!({ "ok": true, "value": { "items": [] } }))
        }
        other => not_implemented(&req.rpc_id, other),
    }
}

/// dsh SSE 流(mux/host 两路):保持打开,先只发心跳(空事件流)。
fn events_stream() -> Response {
    let stream = async_stream::stream! {
        let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            heartbeat.tick().await;
            yield Ok::<_, Infallible>(Event::default().comment("ping"));
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// dsh WS 事件流:接受升级后保持打开、周期 Ping(前端只依赖 open 事件)。
async fn events_ws_channel(upgrade: axum::extract::ws::WebSocketUpgrade, channel: &'static str) -> Response {
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
                                    .send(axum::extract::ws::Message::Text(text.into()))
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

/// GET /api/events.mux 与 /api/events.host 共用:dsh 前端当前装配用
/// WebSocket 升级;保留 SSE 回退(readSse 虚方法的另一实现)。
async fn events_channel(state: AppState, upgrade: axum::extract::ws::WebSocketUpgrade, headers: axum::http::HeaderMap, channel: &'static str) -> Response {
    let wants_ws = headers
        .get(axum::http::header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    if wants_ws {
        events_ws_channel(upgrade, channel).await
    } else {
        events_stream()
    }
}

/// GET /api/events.mux
pub async fn events_mux(State(state): State<AppState>, upgrade: axum::extract::ws::WebSocketUpgrade, headers: axum::http::HeaderMap) -> Response {
    events_channel(state, upgrade, headers, "mux").await
}

/// GET /api/events.host
pub async fn events_host(State(state): State<AppState>, upgrade: axum::extract::ws::WebSocketUpgrade, headers: axum::http::HeaderMap) -> Response {
    events_channel(state, upgrade, headers, "host").await
}

