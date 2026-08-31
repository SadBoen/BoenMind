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

use crate::config_store::ConfigStore;
use crate::AppState;
use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bm_contract::events::EventType;
use bm_contract::ids::BmId;
use bm_contract::wire::{AgentSpec, InputTrust, SendInputParams, SessionCreateParams};
use serde_json::json;
use std::convert::Infallible;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// dsh 工作区/会话的内存存储(重启即失;config/model.json 的持久化由
/// config_store 承担,会话元数据持久化列为 M10 候选尾巴)。
#[derive(Default)]
pub struct DshState {
    pub workspaces: Vec<serde_json::Value>,
    pub sessions: Vec<serde_json::Value>,
    pub seq: u64,
    /// dsh 会话 → 每会话选中模型(provider, model);session.selectModel 写入。
    pub model_selections: std::collections::HashMap<String, (String, String)>,
    /// dsh 会话 → 真实 runtime 会话(session_id, agent_id);首次 prompt 懒建。
    pub runtime_map: std::collections::HashMap<String, (String, String)>,
}

fn dsh_state() -> &'static Mutex<DshState> {
    static S: OnceLock<Mutex<DshState>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(DshState::default()))
}

/// dsh 会话的事件翻译态(M10-S3):mux 帧序列号、session.history 投影、
/// 进行中回合的流式块状态。内存态,与会话元数据同生命周期。
#[derive(Default)]
struct Translation {
    seq: u64,
    history: Vec<serde_json::Value>,
    turn: u64,
    block_started: bool,
    /// 最近回合的 (agent_id, operation_id):session.cancel 需要三元组里的
    /// 后两项,而 dsh 协议只暴露 sessionId。
    last_turn: Option<(String, String)>,
}

fn translations() -> &'static Mutex<std::collections::HashMap<String, Translation>> {
    static T: OnceLock<Mutex<std::collections::HashMap<String, Translation>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 合同 BmId 形态的请求 id(模式 ^[a-z][a-z0-9_]{1,15}_[0-9A-HJKMNP-TV-Z]{26}$,
/// Crockford 32 去除 I/L/O/U)。仅作关联相关性使用,不落实体 id。
fn dsh_request_id() -> BmId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = n ^ C.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut s = String::from("dshreq_");
    for _ in 0..26 {
        s.push(ALPHABET[(x & 31) as usize] as char);
        x = x.wrapping_shr(5) ^ x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    BmId::parse(s).expect("dsh 请求 id 恒为合同形态")
}

/// dsh 信封错误响应(code 必须是 dsh 封闭枚举合法值,否则前端 zod 拒渲染)。
fn dsh_error(rpc_id: &str, code: &str, message: impl std::fmt::Display) -> Response {
    dsh_error_details(rpc_id, code, message, json!({}))
}

/// 带 details 的变体:错误码的 details 形状在前端是逐码封闭的(如
/// model-unavailable 必须 {provider,model}),空对象会被 zod 拒收,
/// 错误永远到不了界面。
fn dsh_error_details(
    rpc_id: &str,
    code: &str,
    message: impl std::fmt::Display,
    details: serde_json::Value,
) -> Response {
    let details = match code {
        "bad-request" => json!({ "issues": [] }),
        _ => details,
    };
    server_response(
        rpc_id,
        json!({"ok": false, "error": {"code": code, "message": message.to_string(), "details": details}}),
    )
}

/// 生效模型目录(M10-S2):设置页与聊天模型选择器**共用同一数据源**——
/// 配置文件(baseUrl+modelId 齐备)= 自定义提供方;env 网关仅兜底。
/// 单一事实源,两处所见一致。
struct ModelDir {
    provider: &'static str,
    display: String,
    model: String,
    /// 模型清单(空 = 只用 model)。
    models: Vec<String>,
    key_set: bool,
    base_url: Option<String>,
}

/// 自定义提供方档案的派生凭据名(与前端 deriveKeyRef 同规则)。
const CUSTOM_KEY_REF: &str = "BOENMIND_CUSTOM_API_KEY";

fn model_dir(state: &AppState) -> Option<ModelDir> {
    if let Some(data_dir) = state.data_dir.as_ref() {
        let eff = crate::config_store::effective_model(data_dir);
        let base = eff.base_url.unwrap_or_default();
        let model = eff.model_id.unwrap_or_default();
        if !base.is_empty() && !model.is_empty() {
            let mut models = eff.models;
            if !models.contains(&model) {
                models.insert(0, model.clone());
            }
            return Some(ModelDir {
                provider: "boenmind-custom",
                display: eff.display_name.unwrap_or_else(|| model.clone()),
                models,
                key_set: eff.api_key.map(|k| !k.is_empty()).unwrap_or(false),
                base_url: Some(base),
                model,
            });
        }
    }
    let env = std::env::var("BOEN_MODEL_ID").ok().filter(|s| !s.is_empty())?;
    Some(ModelDir {
        provider: "boenmind",
        display: "BoenMind 网关(服务器配置)".to_string(),
        models: vec![env.clone()],
        key_set: std::env::var("BOEN_MODEL_API_KEY").map(|k| !k.is_empty()).unwrap_or(false),
        base_url: None,
        model: env,
    })
}

/// 聊天模型选择器的 groups(与设置页行同源)。
fn dir_groups(dir: &Option<ModelDir>) -> Vec<serde_json::Value> {
    match dir {
        Some(d) => {
            let models: Vec<serde_json::Value> = if d.models.is_empty() {
                vec![json!({ "id": d.model, "name": d.model })]
            } else {
                d.models.iter().map(|m| json!({ "id": m, "name": m })).collect()
            };
            vec![json!({ "id": d.provider, "name": d.display, "models": models })]
        }
        None => vec![],
    }
}

/// llm-pi-ai 命名空间的 Schemastery 序列化 schema(uid/refs 信封):providers
/// 为 dict,档案含 api/baseURL/models/displayName。前端从
/// `providers.\0probe.api` 的 union 取「自定义提供方」可用协议列表——按
/// dsh 原版三种线格式(用户截图);Chat Completions 放首位为默认,当前
/// 后端路由实际只走 Chat Completions。
fn llm_pi_ai_schema() -> serde_json::Value {
    json!({
        "uid": 0,
        "refs": {
            "0": { "type": "object", "dict": { "providers": 1 } },
            "1": { "type": "dict", "inner": 3, "sKey": 2 },
            "2": { "type": "string" },
            "3": { "type": "object", "dict": { "api": 4, "baseURL": 5, "models": 6, "displayName": 7 } },
            "4": { "type": "union", "list": [12, 13, 14] },
            "5": { "type": "string" },
            "6": { "type": "array", "inner": 9 },
            "7": { "type": "string" },
            "9": { "type": "object", "dict": { "id": 10, "name": 11 } },
            "10": { "type": "string" },
            "11": { "type": "string" },
            "12": { "type": "string", "value": "Chat Completions (/chat/completions)" },
            "13": { "type": "string", "value": "Anthropic Messages (/v1/messages)" },
            "14": { "type": "string", "value": "Responses (/responses)" }
        }
    })
}

/// llm-pi-ai 命名空间视图(设置页行列表/编辑器共用;value 与 config 文件同源)。
fn llm_pi_ai_view(state: &AppState) -> serde_json::Value {
    let profile = model_dir(state).and_then(|d| match d.provider {
        "boenmind-custom" => {
            let models: Vec<serde_json::Value> = if d.models.is_empty() {
                vec![json!({ "id": d.model, "name": d.model })]
            } else {
                d.models.iter().map(|m| json!({ "id": m, "name": m })).collect()
            };
            Some(json!({
                "displayName": d.display,
                "apiKeyEnv": CUSTOM_KEY_REF,
                // 必须取 schema union 里列出的值之一,否则前端协议下拉无
                // 匹配项显示空白;后端路由只走 Chat Completions
                "api": "Chat Completions (/chat/completions)",
                "baseURL": d.base_url,
                "models": models
            }))
        }
        _ => None,
    });
    let ns_value = match profile {
        Some(p) => json!({ "providers": { "boenmind-custom": p } }),
        None => json!({ "providers": {} }),
    };
    // user = 界面可编辑草稿基线,base = 出厂基线:ProviderEditor 从
    // namespace.user 取表单值,两者都缺席时已存配置不回填(baseURL/
    // displayName 只剩 placeholder)且「删除提供方」永不出现。单文档
    // 配置下两者同源。
    json!({
        "ns": "llm-pi-ai",
        "schema": llm_pi_ai_schema(),
        "value": ns_value,
        "user": ns_value,
        "base": serde_json::Value::Null,
        "applies": "restart",
        "secrets": [],
        "revision": llm_pi_ai_revision(),
    })
}

/// llm-pi-ai 的 revision 计数:settings.mutate 每写一次自增。前端
/// ProviderEditor 带 expectedRevision 做乐观锁,恒 1 会让并发编辑互相
/// 静默覆盖。
fn llm_pi_ai_revision() -> u64 {
    static REV: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    REV.load(std::sync::atomic::Ordering::Relaxed)
}

fn bump_llm_pi_ai_revision() -> u64 {
    static REV: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    REV.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// 把配置的 api_key 实时播种进运行时密钥库(secret:model.{model_id})。
/// 启动时只为启动模型播种一次;界面改钥/换模型后不补种,下一回合取钥
/// 必失败且错误帧曾被前端吞掉(表现为「静默失败」)。Connector 每次请求
/// 现取密钥,播种即生效,免重启。`model` 覆写目标引用(换模型时传所选
/// 模型);None = 配置主模型。
fn seed_runtime_secret(state: &AppState, model: Option<String>) {
    let Some(secrets) = state.secrets.as_ref() else {
        return;
    };
    let Some(data_dir) = state.data_dir.as_ref() else {
        return;
    };
    let eff = crate::config_store::effective_model(data_dir);
    let Some(key) = eff.api_key else {
        return;
    };
    let Some(model) = model.or_else(|| eff.model_id) else {
        return;
    };
    let _ = bm_core::ports::SecretStore::put(
        secrets.as_ref(),
        &bm_core::runtime::default_secret_ref(&model),
        &key,
    );
}

/// llm-pi-ai 的界面编辑写入翻译:ops(providers.<route>[.field]) →
/// config.set/delete。只翻译模型接入相关字段;api 协议字符串原样接受但
/// 当前路由只走 Chat Completions。
fn settings_mutate_llm_pi_ai(state: &AppState, req: &ClientRequest) -> Response {
    let Some(store) = state.data_dir.as_ref().map(crate::config_store::ConfigStore::new) else {
        return dsh_error(&req.rpc_id, "bad-request", "配置存储未启用(无数据目录)");
    };
    // 乐观锁:前端写路径带 expectedRevision(首次写省略);不匹配报
    // settings-conflict(details 形状封闭:{ns,expected,actual}),否则
    // 双开页面互相静默覆盖
    if let Some(expected) = req.payload["expectedRevision"].as_u64() {
        let actual = llm_pi_ai_revision();
        if expected != actual {
            return dsh_error_details(
                &req.rpc_id,
                "settings-conflict",
                format!("llm-pi-ai 配置已被其他窗口修改(期望 {expected},实际 {actual})"),
                json!({ "ns": "llm-pi-ai", "expected": expected, "actual": actual }),
            );
        }
    }
    let empty = vec![];
    let ops = req.payload["ops"].as_array().unwrap_or(&empty);
    for op in ops {
        let kind = op["op"].as_str().unwrap_or_default();
        let path: Vec<String> = op["path"]
            .as_array()
            .map(|a| a.iter().filter_map(|p| p.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        if path.first().map(|s| s.as_str()) != Some("providers") {
            continue;
        }
        let route = path.get(1).cloned().unwrap_or_default();
        if route != "boenmind-custom" {
            continue; // 只管理自定义提供方
        }
        let field = path.get(2).map(|s| s.as_str());
        let outcome: Result<(), bm_core::CoreError> = match (kind, field) {
            ("unset", None) => store.delete("model", None).map(|_| ()),
            ("unset", Some("displayName")) => store.delete("model", Some("displayName")).map(|_| ()),
            ("set", None) => {
                // 整份档案提交:映射为 config 字段
                let profile = &op["value"];
                let mut values = serde_json::Map::new();
                if let Some(b) = profile["baseURL"].as_str() {
                    values.insert("baseUrl".to_string(), json!(b));
                }
                if let Some(ids) = profile["models"].as_array().map(|a| {
                    a.iter().filter_map(|m| m["id"].as_str()).map(|x| json!(x)).collect::<Vec<_>>()
                }) {
                    if !ids.is_empty() {
                        values.insert("models".to_string(), json!(ids));
                        values.insert("modelId".to_string(), ids[0].clone());
                    }
                }
                match profile["displayName"].as_str() {
                    Some(dn) if !dn.is_empty() => {
                        values.insert("displayName".to_string(), json!(dn));
                    }
                    _ => {}
                }
                store.set("model", &serde_json::Value::Object(values)).map(|_| ())
            }
            ("set", Some("displayName")) => store.set("model", &json!({ "displayName": op["value"] })).map(|_| ()),
            ("set", Some("baseURL")) => store.set("model", &json!({ "baseUrl": op["value"] })).map(|_| ()),
            ("set", Some("models")) => {
                let ids: Vec<serde_json::Value> = op["value"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|m| m["id"].as_str()).map(|x| json!(x)).collect())
                    .unwrap_or_default();
                if ids.is_empty() {
                    return dsh_error(&req.rpc_id, "bad-request", "模型清单不能为空");
                }
                store
                    .set("model", &json!({ "models": ids.clone(), "modelId": ids[0] }))
                    .map(|_| ())
            }
            _ => Ok(()),
        };
        if let Err(e) = outcome {
            return dsh_error(&req.rpc_id, "bad-request", e);
        }
    }
    // 写入成功:revision 自增,乐观锁才有意义
    bump_llm_pi_ai_revision();
    // 返回更新后的命名空间视图(界面就地刷新)
    server_response(&req.rpc_id, json!({ "ok": true, "value": llm_pi_ai_view(state) }))
}

/// OpenAI 兼容模型探测:GET {base}/models(Bearer),10s 超时;失败信息
/// 脱敏(不含密钥与响应原文)。
async fn discover_models(base: &str, key: Option<&str>) -> Result<Vec<serde_json::Value>, String> {
    let url = format!("{}/models", base.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "HTTP 客户端初始化失败".to_string())?;
    let mut build = client.get(&url);
    if let Some(k) = key {
        build = build.bearer_auth(k);
    }
    let resp = build.send().await.map_err(|e| format!("探测失败: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("探测失败: HTTP {}", status.as_u16()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|_| "响应不是合法 JSON(非 OpenAI 兼容端点)".to_string())?;
    let data = body["data"]
        .as_array()
        .ok_or("响应缺少 data 数组(非 OpenAI 兼容格式)")?;
    let mut models: Vec<serde_json::Value> = data
        .iter()
        .filter_map(|m| {
            let id = m["id"].as_str()?;
            Some(json!({ "id": id, "name": id }))
        })
        .collect();
    models.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    models.truncate(200);
    if models.is_empty() {
        return Err("该端点未返回任何模型".to_string());
    }
    Ok(models)
}

/// 翻译并落一条 dsh 会话事件:写 history 投影 + 向 mux 广播 session/event 帧。
fn push_dsh_event(t: &mut Translation, dsh_sid: &str, type_: &str, append: bool, data: serde_json::Value) {
    t.seq += 1;
    let mut event = json!({"type": type_, "seq": t.seq, "time": now_ms(), "data": data});
    if append {
        event["surfaceOp"] = json!("append");
    }
    t.history.push(event.clone());
    broadcast_frame(
        "mux",
        "events.mux",
        json!({ "type": "session/event", "sessionId": dsh_sid, "event": event }),
    );
}

/// 置会话 running 位并广播 host/session-status。界面的「生成中」指示与
/// Stop 按钮唯一由该帧驱动(事件流不参与),不发 = 永远无法停止回合。
fn set_session_running(dsh_sid: &str, running: bool) {
    {
        let mut st = dsh_state().lock().unwrap();
        if let Some(s) = st.sessions.iter_mut().find(|s| s["sessionId"] == json!(dsh_sid)) {
            s["running"] = json!(running);
        }
    }
    broadcast_frame(
        "host",
        "events.host",
        json!({ "type": "host/session-status", "sessionId": dsh_sid, "running": running }),
    );
}

/// 每个已映射会话一个转发任务(M10-S3):轮询持久事件日志(200ms,与自有
/// SSE 同节奏),把 runtime 事件翻译为 dsh mux 帧——user 输入由 prompt 直落,
/// 这里只翻译回合/流式/终稿/失败四类。
fn spawn_forwarder(state: &AppState, dsh_sid: String, rt_sid: String) {
    let store = state.store.clone();
    let rt = BmId::parse(rt_sid).ok();
    // stream/error 的 details 形状封闭(须含 provider/model),spawn 时定死
    let provider = model_dir(state)
        .map(|d| d.provider.to_string())
        .unwrap_or_else(|| "boenmind".to_string());
    tokio::spawn(async move {
        let mut last: u64 = 0;
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let Ok(events) = store.replay_since(last) else { continue };
            let mut closed = false;
            for e in events {
                last = e.event_seq;
                if e.session_id.as_ref() != rt.as_ref() {
                    continue;
                }
                match e.event_type {
                    EventType::SessionClosed => {
                        set_session_running(&dsh_sid, false);
                        closed = true;
                    }
                    EventType::AgentTurnStarted => {
                        let mut map = translations().lock().unwrap();
                        let t = map.entry(dsh_sid.clone()).or_default();
                        t.turn = e.payload["turn_index"].as_u64().unwrap_or(t.turn);
                        t.block_started = false;
                        // 记下本回合的 (agent_id, operation_id),供 session.cancel
                        t.last_turn = Some((
                            e.payload["agent_id"].as_str().unwrap_or_default().to_string(),
                            e.payload["operation_id"].as_str().unwrap_or_default().to_string(),
                        ));
                        let turn = t.turn;
                        // 补齐回合边界:turn/start 与 step/start 配对
                        push_dsh_event(t, &dsh_sid, "turn/start", false, json!({"turn": turn}));
                        push_dsh_event(t, &dsh_sid, "step/start", false, json!({"turn": turn, "step": 0}));
                        drop(map);
                        set_session_running(&dsh_sid, true);
                    }
                    EventType::AgentCompleted
                    | EventType::AgentFailed
                    | EventType::AgentCancelled
                    | EventType::AgentInterrupted => {
                        let mut map = translations().lock().unwrap();
                        let t = map.entry(dsh_sid.clone()).or_default();
                        let turn = t.turn;
                        // 闭合 step 与 turn 边界:如果不发 step/end 与 turn/end,
                        // 历史时间线处于未闭合(open)状态,下一次 prompt 的
                        // user/message 坐标会被归到上一回合的 open step 里,
                        // 导致第二句消息无法形成新节点上屏显示!
                        push_dsh_event(t, &dsh_sid, "step/end", false, json!({"turn": turn, "step": 0}));
                        let reason = match e.event_type {
                            EventType::AgentFailed => json!({"kind": "error", "error": e.payload["error_code"].as_str().unwrap_or("unknown")}),
                            EventType::AgentCancelled => json!({"kind": "cancelled"}),
                            _ => json!({"kind": "completed"}),
                        };
                        push_dsh_event(t, &dsh_sid, "turn/end", false, json!({"turn": turn, "reason": reason}));
                        drop(map);

                        if e.event_type == EventType::AgentFailed {
                            let msg = format!(
                                "回合失败({})",
                                e.payload["error_code"].as_str().unwrap_or("unknown")
                            );
                            broadcast_frame(
                                "host",
                                "events.host",
                                json!({ "type": "host/agent-error", "sessionId": dsh_sid, "message": msg }),
                            );
                        }
                        set_session_running(&dsh_sid, false);
                    }
                    EventType::ModelContentDelta => {
                        let delta = e.payload["delta"].as_str().unwrap_or_default().to_string();
                        if delta.is_empty() {
                            continue;
                        }
                        let mut map = translations().lock().unwrap();
                        let t = map.entry(dsh_sid.clone()).or_default();
                        if !t.block_started {
                            t.block_started = true;
                            push_dsh_event(t, &dsh_sid, "assistant/chunk", false, json!({
                                "turn": t.turn, "step": 0,
                                "chunk": {"type": "block-start", "index": 0, "blockType": "text"}
                            }));
                        }
                        push_dsh_event(t, &dsh_sid, "assistant/chunk", false, json!({
                            "turn": t.turn, "step": 0,
                            "chunk": {"type": "text-delta", "index": 0, "text": delta}
                        }));
                    }
                    EventType::ModelInvocationCompleted => {
                        let content = e.payload["content"].as_str().unwrap_or_default().to_string();
                        let id = format!(
                            "am-{}",
                            e.operation_id.as_ref().map(|o| o.to_string()).unwrap_or_else(uuid_like)
                        );
                        let mut map = translations().lock().unwrap();
                        let t = map.entry(dsh_sid.clone()).or_default();
                        t.block_started = false;
                        push_dsh_event(t, &dsh_sid, "assistant/message", true, json!({
                            "turn": t.turn, "step": 0,
                            "message": { "id": id, "content": [{ "type": "text", "text": content }] }
                        }));
                    }
                    EventType::ModelInvocationFailed => {
                        // sessionId 必带(前端按会话路由错误);error.details
                        // 形状封闭:须含 provider/model,空对象会被 zod 弃帧
                        broadcast_frame(
                            "mux",
                            "events.mux",
                            json!({
                                "type": "stream/error",
                                "sessionId": dsh_sid,
                                "error": {
                                    "code": "model-unavailable",
                                    "message": format!("模型调用失败({})", e.payload["error_code"].as_str().unwrap_or("unknown")),
                                    "details": {
                                        "provider": provider,
                                        "model": e.payload["model_id"].as_str().unwrap_or_default()
                                    }
                                }
                            }),
                        );
                    }
                    _ => {}
                }
            }
            if closed {
                break;
            }
        }
    });
}

/// mux/host 两条事件流的广播(帧 JSON 文本)。容量 4096:流式回复一次可推
/// 上百条增量帧,64 的环会把 user/message 与回复开头挤掉(界面表现为
/// 「第二句起看不见」)。
fn event_bus(channel: &str) -> &'static tokio::sync::broadcast::Sender<String> {
    static MUX: OnceLock<tokio::sync::broadcast::Sender<String>> = OnceLock::new();
    static HOST: OnceLock<tokio::sync::broadcast::Sender<String>> = OnceLock::new();
    match channel {
        "host" => HOST.get_or_init(|| tokio::sync::broadcast::channel(4096).0),
        _ => MUX.get_or_init(|| tokio::sync::broadcast::channel(4096).0),
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
    bm_contract::timestamp::now()
}

fn now_ts() -> u64 {
    // dsh 界面的 updatedAt 是毫秒时间戳(秒会让会话时间显示成「56 年前」)
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
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

fn not_implemented(rpc_id: &str, method: &str) -> Response {
    // code 用 dsh 错误码封闭枚举内的合法值(bad-request),否则前端 zod
    // union 校验失败、错误无法渲染;details 形状也是封闭的:bad-request
    // 必须 {issues:[]},缺字段会被前端 zod 拒收(ZodError 而非业务错误)
    server_response(
        rpc_id,
        json!({
            "ok": false,
            "error": {
                "code": "bad-request",
                "message": format!("BoenMind 尚未适配 dsh 方法 {method}(逐项接入中)"),
                "details": { "issues": [] }
            }
        }),
    )
}

/// dsh unary 入口:POST /api/{*rest}(方法名可含多段,如
/// /api/dynamicCordisRunner/inventory)。
pub async fn unary(
    State(state): State<AppState>,
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
    eprintln!("[dsh-req] {} {}", req.method, req.rpc_id); // M10 诊断:页面请求到达流水
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
            let mut namespaces: Vec<serde_json::Value> = guard
                .iter()
                .map(|(ns, (value, revision))| {
                    json!({
                        "ns": ns,
                        "schema": null,
                        "value": value,
                        "user": value,
                        "applies": "live",
                        "secrets": [],
                        "revision": revision,
                    })
                })
                .collect();
            // M10-S2:llm-pi-ai 命名空间(档案+schema 与编辑写入口同源)
            namespaces.push(llm_pi_ai_view(&state));
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
            // llm-pi-ai 的界面编辑(创建后的改名/改地址/改模型)→ config API,
            // 与创建入口同一份配置文件(M10-S2 闭环)
            if ns == "llm-pi-ai" {
                return settings_mutate_llm_pi_ai(&state, &req);
            }
            let store = settings_store();
            let mut guard = store.lock().unwrap();
            let entry = guard.entry(ns.clone()).or_insert((json!({}), 0u64));
            // 乐观锁:与 llm-pi-ai 同语义,不匹配报 settings-conflict
            if let Some(expected) = req.payload["expectedRevision"].as_u64() {
                let actual = entry.1;
                if expected != actual {
                    drop(guard);
                    return dsh_error_details(
                        &req.rpc_id,
                        "settings-conflict",
                        format!("{ns} 配置已被其他窗口修改(期望 {expected},实际 {actual})"),
                        json!({ "ns": ns, "expected": expected, "actual": actual }),
                    );
                }
            }
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
                "user": entry.0,
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
                    let is_dir = e.path().is_dir();
                    let fp = e.path().display().to_string();
                    // 目录条目 path 以分隔符结尾:前端据此区分可进入目录与
                    // 普通文件(目录条目合同没有 kind 字段,zod 会剥离未知
                    // 键,尾分隔符是能穿过校验的唯一标记位)
                    let marked = if is_dir {
                        format!("{}{}", fp, std::path::MAIN_SEPARATOR)
                    } else {
                        fp.clone()
                    };
                    entries.push(json!({ "name": name, "path": marked, "hidden": false }));
                }
            }
            // 面包屑:逐级拆分绝对路径(盘符/根 → … → 当前),供面板顶部
            // 点击跳转;空段过滤,UNC 与裸相对路径兜底为单节
            let norm = path.replace('\\', "/");
            let mut crumb_rows: Vec<serde_json::Value> = Vec::new();
            let mut acc = String::new();
            for (i, seg) in norm.split('/').filter(|s| !s.is_empty()).enumerate() {
                if i == 0 {
                    acc = format!("{seg}/");
                } else {
                    acc = format!("{acc}{seg}/");
                }
                crumb_rows.push(json!({ "name": seg, "path": acc.clone(), "hidden": false }));
            }
            let crumbs = if crumb_rows.is_empty() {
                json!([{ "name": path, "path": path, "hidden": false }])
            } else {
                json!(crumb_rows)
            };
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
            let mut path = req.payload["path"].as_str().unwrap_or_default().to_string();
            if path.is_empty() {
                return server_response(&req.rpc_id, json!({ "ok": false, "error": {
                    "code": "workspace-invalid-path", "message": "缺少 path", "details": { "path": "" } } }));
            }
            // 浏览器原生文件夹选择框只能提供文件夹名(不含路径):裸名落到
            // 服务器托管目录 workspaces/<名字>;完整路径仍按用户给的建
            if !path.contains('/') && !path.contains('\\') && !path.contains(':') {
                let root = state
                    .data_dir
                    .clone()
                    .unwrap_or_else(|| std::path::PathBuf::from("workspaces"))
                    .join("workspaces");
                path = root.join(&path).display().to_string();
            }
            // 用户裁决:路径不存在就自动新建
            if let Err(e) = std::fs::create_dir_all(&path) {
                return server_response(&req.rpc_id, json!({ "ok": false, "error": {
                    "code": "workspace-invalid-path", "message": format!("目录创建失败: {e}"),
                    "details": { "path": path } } }));
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
            broadcast_frame("host", "events.host", json!({
                "type": "host/workspace-changed",
                "workspace": ws
            }));
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
            // cwd 用所属工作区路径:前端 connectWorkspace 以 summary.cwd ===
            // workspace.path 判定可复用的 blank 会话,恒用服务器 cwd 会让
            // 复用永不命中,每进一次工作区就新建一个空会话
            let cwd = st
                .workspaces
                .iter()
                .find(|w| w["workspaceId"] == json!(workspace_id))
                .and_then(|w| w["path"].as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default()
                });
            let session = json!({
                "sessionId": sid,
                "updatedAt": now_ts(),
                "running": false,
                "blank": true,
                "cwd": cwd,
            });
            st.sessions.push(session);
            let changed_ws = if let Some(w) = st.workspaces.iter_mut().find(|w| w["workspaceId"] == json!(workspace_id)) {
                if let Some(arr) = w["sessionIds"].as_array_mut() {
                    arr.push(json!(sid));
                }
                w["updatedAt"] = json!(now_iso());
                Some(w.clone())
            } else {
                None
            };
            drop(st);
            if let Some(ws) = changed_ws {
                broadcast_frame("host", "events.host", json!({
                    "type": "host/workspace-changed",
                    "workspace": ws
                }));
            }
            broadcast_frame("host", "events.host", json!({
                "type": "host/session-added", "sessionId": sid, "blank": true, "cwd": cwd
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
        "session.history" => {
            let sid = req.payload["sessionId"].as_str().unwrap_or_default().to_string();
            let map = translations().lock().unwrap();
            let events: Vec<serde_json::Value> = map
                .get(&sid)
                .map(|t| t.history.iter().map(|e| json!({ "event": e })).collect())
                .unwrap_or_default();
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": { "events": events, "hasMore": false } }),
            )
        }
        "session.models" => {
            let sid = req.payload["sessionId"].as_str().unwrap_or_default().to_string();
            let dir = model_dir(&state);
            let groups = dir_groups(&dir);
            let current = {
                let st = dsh_state().lock().unwrap();
                st.model_selections
                    .get(&sid)
                    .map(|(p, m)| json!({ "provider": p, "model": m }))
                    .or_else(|| {
                        dir.as_ref().map(|d| json!({ "provider": d.provider, "model": d.model }))
                    })
            };
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": {
                    "current": current.unwrap_or(serde_json::Value::Null),
                    "routable": !groups.is_empty(),
                    "groups": groups,
                    "failures": []
                } }),
            )
        }
        "session.selectModel" => {
            let sid = req.payload["sessionId"].as_str().unwrap_or_default().to_string();
            let provider = req.payload["provider"].as_str().unwrap_or_default().to_string();
            let model = req.payload["model"].as_str().unwrap_or_default().to_string();
            let groups = dir_groups(&model_dir(&state));
            let known = groups.iter().any(|g| {
                g["id"] == json!(provider)
                    && g["models"]
                        .as_array()
                        .map(|ms| ms.iter().any(|m| m["id"] == json!(model)))
                        .unwrap_or(false)
            });
            if !known {
                return dsh_error_details(
                    &req.rpc_id,
                    "bad-request",
                    format!("未知模型选择 {provider}/{model}"),
                    json!({ "issues": [] }),
                );
            }
            dsh_state().lock().unwrap().model_selections.insert(sid, (provider.clone(), model.clone()));
            // 换模型 = 换密钥引用(secret:model.{id});启动时只为启动模型
            // 播种,不补种则下一回合静默失败。Connector 每请求现取,播种即生效。
            seed_runtime_secret(&state, Some(model.clone()));
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": { "selected": { "provider": provider, "model": model } } }),
            )
        }
        "session.prompt" => {
            // M10-S3 对话闭环:dsh prompt → 懒建真会话 → agent.send_input;
            // 回复经 spawn_forwarder 翻译回流(纯对话;工具/审批回归待裁决)。
            let Some(sid) = req.payload["sessionId"].as_str().map(|s| s.to_string()) else {
                return dsh_error(&req.rpc_id, "bad-request", "缺少 sessionId");
            };
            let mut text = String::new();
            if let Some(parts) = req.payload["content"].as_array() {
                for p in parts {
                    if p["type"] == json!("text")
                        && let Some(t) = p["text"].as_str()
                    {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
            }
            if text.is_empty() {
                return dsh_error(&req.rpc_id, "bad-request", "消息内容为空");
            }
            let selected = {
                let st = dsh_state().lock().unwrap();
                st.model_selections.get(&sid).cloned()
            }
            .or_else(|| model_dir(&state).as_ref().map(|d| (d.provider.to_string(), d.model.clone())));
            let Some((_provider, model)) = selected else {
                // details 形状封闭:model-unavailable 必须 {provider,model},
                // 空对象会被前端 zod 拒收,「未配置模型」的提示永远到不了界面
                return dsh_error_details(
                    &req.rpc_id,
                    "model-unavailable",
                    "未配置模型:请在设置里保存模型配置并重启服务,或以 BOEN_MODEL_* 环境变量启动",
                    json!({ "provider": "boenmind", "model": "" }),
                );
            };
            // 懒建真会话(std 锁不跨 await:先查 → await → 回填)。
            // 界面本地会话可能未走过 session.create:补登记进会话表,
            // 否则 session.list 缺行、刷新后对话「消失」。
            {
                let mut st = dsh_state().lock().unwrap();
                match st.sessions.iter_mut().find(|s| s["sessionId"] == json!(sid)) {
                    Some(s) => {
                        s["updatedAt"] = json!(now_ts());
                        s["blank"] = json!(false);
                    }
                    None => st.sessions.push(json!({
                        "sessionId": sid,
                        "updatedAt": now_ts(),
                        "running": false,
                        "blank": false,
                        "cwd": std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
                    })),
                }
            }
            let existing = dsh_state().lock().unwrap().runtime_map.get(&sid).cloned();
            let (rt_sid, rt_aid) = match existing {
                Some(v) => v,
                None => match state
                    .handle
                    .session_create(
                        dsh_request_id(),
                        SessionCreateParams {
                            agent: AgentSpec {
                                name: "web-surface".to_string(),
                                model_chain: vec![model],
                                budget: None,
                            },
                        },
                    )
                    .await
                {
                    Ok(r) => {
                        let rt = (r.session_id.to_string(), r.agent_id.to_string());
                        dsh_state().lock().unwrap().runtime_map.insert(sid.clone(), rt.clone());
                        spawn_forwarder(&state, sid.clone(), rt.0.clone());
                        rt
                    }
                    Err(e) => return dsh_error(&req.rpc_id, "bad-request", e),
                },
            };
            let (Ok(rs), Ok(ra)) = (BmId::parse(rt_sid), BmId::parse(rt_aid)) else {
                return dsh_error(&req.rpc_id, "bad-request", "会话映射损坏");
            };
            let sent = state
                .handle
                .send_input(
                    dsh_request_id(),
                    SendInputParams {
                        session_id: rs,
                        agent_id: ra,
                        content: text.clone(),
                        input_trust: InputTrust::Trusted,
                    },
                )
                .await;
            // 用户消息在 send_input 成功后才落(history 投影 + mux 帧上屏):
            // 先落后发会让发送失败时界面留下一条永远不会被回复的幽灵消息,
            // 用户重发即重复。send_input 是命令应答(收据即回,不等回合完成),
            // 后落不会把用户消息排到回复之后。
            match sent {
                Ok(_) => {
                    {
                        let mut map = translations().lock().unwrap();
                        let t = map.entry(sid.clone()).or_default();
                        push_dsh_event(
                            t,
                            &sid,
                            "user/message",
                            true,
                            json!({
                                "id": format!("um-{}", uuid_like()),
                                "source": { "kind": "user" },
                                "content": [{ "type": "text", "text": text }]
                            }),
                        );
                    }
                    broadcast_frame(
                        "mux",
                        "events.mux",
                        json!({ "type": "session/queue", "sessionId": sid, "items": [] }),
                    );
                    server_response(&req.rpc_id, json!({ "ok": true, "value": { "accepted": true } }))
                }
                Err(e) => dsh_error(&req.rpc_id, "bad-request", e),
            }
        }
        "session.cancel" => {
            // 停止进行中的回合:三元组里 agent/operation 来自最近一次
            // AgentTurnStarted 的跟踪;无在途回合时无可取消。
            let Some(sid) = req.payload["sessionId"].as_str().map(|s| s.to_string()) else {
                return dsh_error(&req.rpc_id, "bad-request", "缺少 sessionId");
            };
            let Some((rs, ra)) = dsh_state().lock().unwrap().runtime_map.get(&sid).cloned() else {
                return dsh_error(&req.rpc_id, "bad-request", "会话未绑定运行时实例,无可取消回合");
            };
            let Some((_agent_id, operation_id)) = translations()
                .lock()
                .unwrap()
                .get(&sid)
                .and_then(|t| t.last_turn.clone())
            else {
                return dsh_error(&req.rpc_id, "bad-request", "该会话没有可取消的进行中回合");
            };
            let (Ok(rs), Ok(ra), Ok(rop)) = (
                BmId::parse(rs),
                BmId::parse(ra),
                BmId::parse(operation_id),
            ) else {
                return dsh_error(&req.rpc_id, "bad-request", "会话映射损坏,无法取消");
            };
            match state
                .handle
                .agent_cancel(bm_contract::wire::CancelParams {
                    session_id: rs,
                    agent_id: ra,
                    operation_id: rop,
                })
                .await
            {
                // 前端值 schema 只认 {accepted:true};未受理按业务错误回报
                Ok(r) if r.accepted => {
                    set_session_running(&sid, false);
                    server_response(&req.rpc_id, json!({ "ok": true, "value": { "accepted": true } }))
                }
                Ok(_) => dsh_error(&req.rpc_id, "bad-request", "运行时未受理取消请求"),
                Err(e) => dsh_error(&req.rpc_id, "bad-request", e),
            }
        }
        "commands/list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": [] }),
        ),
        "commands/execute" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": serde_json::Value::Null }),
        ),
        "fileReferences/list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": [] }),
        ),
        "goals/clear"
        | "goals/complete"
        | "goals/create"
        | "goals/edit"
        | "goals/pause"
        | "goals/resume" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": serde_json::Value::Null }),
        ),
        "messageFeedback/list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": [] }),
        ),
        "messageFeedback/put" | "messageFeedback/delete" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": serde_json::Value::Null }),
        ),
        "dynamicCordisRunner/inventory" | "dynamicCordisRunner/syncInspectManifest" => {
            // dsh 插件清单:BoenMind 无 dsh 插件,空清单
            server_response(&req.rpc_id, json!({ "ok": true, "value": { "packages": [], "items": [] } }))
        }
        "pluginInventory/list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": { "plugins": [] } }),
        ),
        "sessionReferenceResolver/candidates" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": [] }),
        ),
        "skill.list" => server_response(
            &req.rpc_id,
            json!({ "ok": true, "value": { "skills": [] } }),
        ),
        "llm.providers" => {
            // 与设置页同源:自定义提供方(配置文件)或 env 兜底网关,二选一
            let dir = model_dir(&state);
            let providers = match &dir {
                Some(d) if d.provider == "boenmind-custom" => vec![json!({
                    "provider": "boenmind-custom",
                    "displayName": d.display,
                    "settingsNs": "llm-pi-ai",
                    "settingsPath": ["providers", "boenmind-custom"],
                    "active": true,
                    "declared": true
                })],
                Some(d) => vec![json!({
                    "provider": "boenmind",
                    "displayName": d.display,
                    "settingsNs": "llm-boenmind",
                    "settingsPath": ["providers"],
                    "active": true,
                    "declared": true
                })],
                None => vec![],
            };
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": { "providers": providers } }),
            )
        }
        "llm.models" => {
            let dir = model_dir(&state);
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": { "groups": dir_groups(&dir), "failures": [] } }),
            )
        }
        "llm.discoverModels" => {
            // 表单「获取可用模型」:按表单当前填的地址/密钥(缺省回落已存
            // 配置)真实探测 {base}/models
            let dir = model_dir(&state);
            let base = req.payload["baseURL"]
                .as_str()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| dir.as_ref().and_then(|d| d.base_url.clone()))
                .unwrap_or_default();
            let key = req.payload["apiKey"]
                .as_str()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    state
                        .data_dir
                        .as_ref()
                        .map(|d| crate::config_store::effective_model(d))
                        .and_then(|e| e.api_key)
                });
            if base.is_empty() {
                return dsh_error(&req.rpc_id, "bad-request", "缺少 API 地址");
            }
            match discover_models(&base, key.as_deref()).await {
                Ok(models) => server_response(
                    &req.rpc_id,
                    json!({ "ok": true, "value": { "models": models, "failures": [] } }),
                ),
                Err(msg) => dsh_error(&req.rpc_id, "bad-request", msg),
            }
        }
        "credentials.describe" => {
            // 密钥状态(设置页行的绿点/缺钥匙提示):只认识自定义提供方的
            // 派生凭据名;状态即 config.model 的 apiKey 是否已设置
            let dir = model_dir(&state);
            let mut credentials = json!({});
            if let Some(refs) = req.payload["refs"].as_array() {
                for r in refs {
                    if let Some(name) = r.as_str() {
                        let configured = name == CUSTOM_KEY_REF
                            && dir.as_ref().map(|d| d.provider == "boenmind-custom" && d.key_set).unwrap_or(false);
                        credentials[name] = json!({ "configured": configured, "writable": true });
                    }
                }
            }
            server_response(
                &req.rpc_id,
                json!({ "ok": true, "value": { "credentials": credentials } }),
            )
        }
        "credentials.set" => {
            // 界面编辑路径的密钥写入:落到 config.model.apiKey(打码不回显);
            // 并实时播种进运行时密钥库(OpenAiConnector 每次请求现取),
            // 否则界面显示已保存、真实请求仍用旧钥直到重启
            let name = req.payload["ref"].as_str().unwrap_or_default();
            let value = req.payload["value"].as_str().unwrap_or_default();
            if name != CUSTOM_KEY_REF {
                return dsh_error(&req.rpc_id, "bad-request", format!("未知凭据引用 {name}"));
            }
            if value.is_empty() {
                return dsh_error(&req.rpc_id, "bad-request", "密钥不能为空");
            }
            let Some(store) = state.data_dir.as_ref().map(crate::config_store::ConfigStore::new) else {
                return dsh_error(&req.rpc_id, "bad-request", "配置存储未启用(无数据目录)");
            };
            match store.set("model", &json!({ "apiKey": value })) {
                Ok(_) => {
                    seed_runtime_secret(&state, None);
                    server_response(&req.rpc_id, json!({ "ok": true, "value": {} }))
                }
                Err(e) => dsh_error(&req.rpc_id, "bad-request", e),
            }
        }
        "credentials.unset" => {
            let name = req.payload["ref"].as_str().unwrap_or_default();
            if name != CUSTOM_KEY_REF {
                return dsh_error(&req.rpc_id, "bad-request", format!("未知凭据引用 {name}"));
            }
            let Some(store) = state.data_dir.as_ref().map(crate::config_store::ConfigStore::new) else {
                return dsh_error(&req.rpc_id, "bad-request", "配置存储未启用(无数据目录)");
            };
            match store.delete("model", Some("apiKey")) {
                Ok(_) => {
                    // 密钥库里的旧凭据一并清掉(best-effort,失败不阻塞 UI)
                    if let Some(model) = crate::config_store::effective_model(
                        state.data_dir.as_ref().unwrap(),
                    )
                    .model_id
                    {
                        if let Some(secrets) = &state.secrets {
                            let _ = bm_core::ports::SecretStore::delete(
                                secrets.as_ref(),
                                &bm_core::runtime::default_secret_ref(&model),
                            );
                        }
                    }
                    server_response(&req.rpc_id, json!({ "ok": true, "value": {} }))
                }
                Err(e) => dsh_error(&req.rpc_id, "bad-request", e),
            }
        }
        "config.list" | "config.get" | "config.set" | "config.delete" => {
            // M10-S1 配置管理 API 的界面喂食口(公开挂载 = 已登记欠账,公网
            // 部署前必须补鉴权;成熟通道是 /rpc/config.*,Bearer 鉴权)
            let Some(store) = state.data_dir.as_ref().map(ConfigStore::new) else {
                return dsh_error(&req.rpc_id, "bad-request", "配置存储未启用(无数据目录)");
            };
            let result = match req.method.as_str() {
                "config.list" => serde_json::to_value(store.list())
                    .map_err(|_| bm_core::CoreError::Internal),
                "config.get" => {
                    let ns = req.payload["ns"].as_str().unwrap_or_default();
                    store.get(ns).and_then(|v| serde_json::to_value(v).map_err(|_| bm_core::CoreError::Internal))
                }
                "config.set" => {
                    let ns = req.payload["ns"].as_str().unwrap_or_default();
                    let values = req.payload["values"].clone();
                    store.set(ns, &values)
                        .and_then(|v| serde_json::to_value(v).map_err(|_| bm_core::CoreError::Internal))
                }
                _ => {
                    let ns = req.payload["ns"].as_str().unwrap_or_default();
                    let field = req.payload["field"].as_str();
                    store.delete(ns, field)
                        .and_then(|v| serde_json::to_value(v).map_err(|_| bm_core::CoreError::Internal))
                }
            };
            match result {
                Ok(v) => server_response(&req.rpc_id, json!({ "ok": true, "value": v })),
                Err(e) => dsh_error(&req.rpc_id, "bad-request", e),
            }
        }
        other => not_implemented(&req.rpc_id, other),
    }
}

/// dsh SSE 流(mux/host 两路):转发广播总线帧 + 心跳。浏览器装配只用
/// WS,此路是非浏览器/代理环境的回退;此前只发心跳,回退客户端的会话面
/// 完全失明。
fn events_stream(channel: &'static str) -> Response {
    let mut rx = event_bus(channel).subscribe();
    let stream = async_stream::stream! {
        let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    // 单行 JSON,与前端 readSse 的 data 行拼接规则兼容
                    Ok(text) => yield Ok::<_, Infallible>(
                        Event::default().event("envelope").data(text)
                    ),
                    // 落环不断流也不跳帧,交由客户端按 seq 修复(与 WS 一致)
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[dsh-sse] {channel} 落后 {n} 帧"); // M10 诊断
                    }
                    Err(_) => break,
                },
                _ = heartbeat.tick() => {
                    yield Ok(Event::default().comment("ping"));
                }
            }
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// 跨源 WebSocket 劫持(CSWSH)防护:/api 全程无 Bearer 鉴权,而 WS 不受
/// CORS preflight 约束——不校验 Origin,浏览器里任何网页都能连上事件流
/// 窃听全部会话内容。规则:带 Origin 且 host 与 Host 头不符 → 403;
/// 无 Origin(CLI/curl)放行。
fn origin_allowed(headers: &axum::http::HeaderMap) -> bool {
    let Some(origin) = headers.get(axum::http::header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let Some(host) = headers.get(axum::http::header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let origin_host = origin
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or_default();
    origin_host.eq_ignore_ascii_case(host)
}

/// dsh WS 事件流:接受升级后保持打开、周期 Ping(前端只依赖 open 事件)。
async fn events_ws_channel(
    upgrade: axum::extract::ws::WebSocketUpgrade,
    headers: axum::http::HeaderMap,
    channel: &'static str,
) -> Response {
    if !origin_allowed(&headers) {
        return (axum::http::StatusCode::FORBIDDEN, "origin 不符").into_response();
    }
    let mut rx = event_bus(channel).subscribe();
    upgrade
        .on_upgrade(move |mut socket| async move {
            eprintln!("[dsh-ws] {channel} 已连接"); // M10 诊断
            // mux 连接(重)建立:对本 socket 直发全部已知会话的
            // session/subscribed 基线帧。该帧原本只在 session.create 时
            // 广播一次——页面刷新/断线重连后永远等不到 baseline,会话
            // 窗口卡「载入历史…」。lastSeq 取当前翻译序号(幂等,重复
            // 收到只是重置队列镜像)。
            if channel == "mux" {
                let baseline: Vec<(String, u64)> = {
                    let map = translations().lock().unwrap();
                    map.iter().map(|(sid, t)| (sid.clone(), t.seq)).collect()
                };
                for (sid, seq) in baseline {
                    let frame = json!({
                        "type": "server-request",
                        "rpcId": uuid_like(),
                        "method": "events.mux",
                        "payload": { "type": "session/subscribed", "sessionId": sid, "lastSeq": seq }
                    });
                    if socket.send(axum::extract::ws::Message::Text(frame.to_string().into())).await.is_err() {
                        return;
                    }
                }
            }
            let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
            loop {
                tokio::select! {
                    // 必须读入站:客户端开门就发订阅类消息,不读=对端等不到
                    // 处理,判定连接失败进无限重连(M10 根因)
                    inbound = socket.recv() => {
                        match inbound {
                            Some(Ok(axum::extract::ws::Message::Text(t))) => {
                                eprintln!("[dsh-ws] {channel} 收到客户端消息: {}", &t[..t.len().min(200)]); // M10 诊断
                            }
                            Some(Ok(_)) => {}
                            Some(Err(e)) => {
                                eprintln!("[dsh-ws] {channel} 断开:入站错误 {e}"); // M10 诊断
                                break;
                            }
                            None => {
                                eprintln!("[dsh-ws] {channel} 断开:客户端关闭"); // M10 诊断
                                break;
                            }
                        }
                    }
                    _ = heartbeat.tick() => {
                        if socket
                            .send(axum::extract::ws::Message::Ping(axum::body::Bytes::new()))
                            .await
                            .is_err()
                        {
                            eprintln!("[dsh-ws] {channel} 断开:ping 发送失败"); // M10 诊断
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
                                    eprintln!("[dsh-ws] {channel} 断开:帧发送失败"); // M10 诊断
                                    break;
                                }
                            }
                            // 落环 = 客户端已缺帧:主动断开,前端重连后经
                            // history 重 sync,绝不静默跳帧
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                eprintln!("[dsh-ws] {channel} 断开:落后 {n} 帧"); // M10 诊断
                                break;
                            }
                            Err(_) => {
                                eprintln!("[dsh-ws] {channel} 断开:广播通道关闭"); // M10 诊断
                                break;
                            }
                        }
                    }
                }
            }
            eprintln!("[dsh-ws] {channel} socket 循环结束"); // M10 诊断
        })
}

/// GET /api/events.mux 与 /api/events.host 共用:WebSocket 升级优先,
/// 普通 GET(无升级头)走 SSE 帧转发。升级提取器用 Option 包裹——
/// 强提取会在非 WS 请求上直接 400,SSE 分支永不可达。
async fn events_channel(
    upgrade: Result<axum::extract::ws::WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    headers: axum::http::HeaderMap,
    channel: &'static str,
) -> Response {
    match upgrade {
        Ok(upgrade) => events_ws_channel(upgrade, headers, channel).await,
        Err(_) => events_stream(channel),
    }
}

/// GET /api/events.mux
pub async fn events_mux(
    upgrade: Result<axum::extract::ws::WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    headers: axum::http::HeaderMap,
) -> Response {
    events_channel(upgrade, headers, "mux").await
}

/// GET /api/events.host
pub async fn events_host(
    upgrade: Result<axum::extract::ws::WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    headers: axum::http::HeaderMap,
) -> Response {
    events_channel(upgrade, headers, "host").await
}

