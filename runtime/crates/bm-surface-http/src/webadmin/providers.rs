//! provider 库(config/providers.json):CRUD/连通探针/模型清单 + 当前生效
//! 模型(config/model.json)+ W6 对话级模型路由重建。

use super::{AdminConfig, admin_error};
use crate::config_store::ModelConfigStore;
use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---- provider 库(config/providers.json)--------------------------------

fn providers_file(data_dir: &Path) -> PathBuf {
    data_dir.join("config/providers.json")
}

fn read_providers(data_dir: &Path) -> Result<Vec<Value>, (StatusCode, String)> {
    let path = providers_file(data_dir);
    let s = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("读取 providers 配置文件失败: {e}"),
            ));
        }
    };
    let v: Value = serde_json::from_str(&s).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("providers 配置文件 JSON 格式已损坏,拒绝加载/覆写: {e}"),
        )
    })?;
    let list = v["providers"].as_array().cloned().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "providers.json 缺少合法的 providers 数组".to_string(),
        )
    })?;
    Ok(list)
}

fn write_providers(data_dir: &Path, providers: &[Value]) -> Result<(), String> {
    let path = providers_file(data_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("配置目录创建失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&json!({ "providers": providers }))
        .map_err(|_| "序列化失败".to_string())?
        .replace('\n', "\r\n");
    bm_persist::atomic_write(&path, text.as_bytes()).map_err(|e| format!("配置文件写入失败: {e}"))
}

/// provider 条目字段校验;返回归一化后的错误消息。
fn validate_provider_input(body: &Value) -> Result<(), (StatusCode, String)> {
    let bad = |m: &str| Err((StatusCode::BAD_REQUEST, m.to_string()));
    let name = body["name"].as_str().unwrap_or("");
    if name.is_empty() || name.len() > 100 {
        return bad("name 必须是非空字符串(≤100 字符)");
    }
    let base = body["baseUrl"].as_str().unwrap_or("");
    if !(base.len() <= 500 && (base.starts_with("http://") || base.starts_with("https://"))) {
        return bad("baseUrl 必须以 http:// 或 https:// 开头(≤500 字符)");
    }
    if body["apiKey"].as_str().is_some_and(|k| k.len() > 4096) {
        return bad("apiKey ≤4096 字符");
    }
    if let Some(models) = body["models"].as_array() {
        if models.len() > 50 {
            return bad("models 至多 50 个");
        }
        for m in models {
            let id = m.as_str().unwrap_or("");
            if id.is_empty() || id.len() > 200 {
                return bad("models 项必须是非空字符串(≤200 字符)");
            }
        }
    }
    // W6 常用清单:可选;给出时须为 models 子集(对话输入框的候选来源)
    if let Some(common) = body["modelsCommon"].as_array() {
        if common.len() > 50 {
            return bad("modelsCommon 至多 50 个");
        }
        for m in common {
            let id = m.as_str().unwrap_or("");
            if id.is_empty() || id.len() > 200 {
                return bad("modelsCommon 项必须是非空字符串(≤200 字符)");
            }
        }
        if let Some(models) = body["models"].as_array() {
            for c in common {
                if !models.contains(c) {
                    return bad(&format!(
                        "modelsCommon 项「{c}」不在 models 清单内(须为子集)"
                    ));
                }
            }
        }
    }
    // 模型窗口登记(可选,context-inspector 的「真实水位」数据源):
    // model_id → 上下文窗口 token 数。不猜不算,登记多少显示多少;
    // 未登记的模型面板如实显示「窗口未知」。
    if let Some(windows) = body["modelWindows"].as_object() {
        if windows.len() > 50 {
            return bad("modelWindows 至多 50 条登记");
        }
        for (k, v) in windows {
            if k.is_empty() || k.len() > 200 {
                return bad("modelWindows 的模型名必须是非空字符串(≤200 字符)");
            }
            let Some(n) = v.as_u64() else {
                return bad("modelWindows 值必须是正整数(token 数)");
            };
            if !(1..=4_000_000).contains(&n) {
                return bad("modelWindows 值超出合理区间(1..=4,000,000 token)");
            }
        }
    }
    Ok(())
}

/// apiKey 归一:缺省/null/空串 → None(编辑语义 = 保持不变)。
fn norm_key(body: &Value) -> Option<String> {
    body["apiKey"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// 打码投影:apiKey 恒 null,是否已设置由 secretSet 标记(ADR-0012 口径)。
fn mask_provider(p: &Value) -> Value {
    json!({
        "id": p["id"],
        "name": p["name"],
        "baseUrl": p["baseUrl"],
        "models": p["models"],
        "modelsCommon": p["modelsCommon"].as_array().cloned().unwrap_or_default(),
        "modelWindows": p["modelWindows"].as_object().cloned().unwrap_or_default(),
        "defaultModel": p["defaultModel"],
        "secretSet": p["apiKey"].as_str().map(|s| !s.is_empty()).unwrap_or(false),
    })
}

fn new_provider_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("系统熵源不可用");
    format!("prov_{}", bm_contract::hash::hex(&bytes))
}

/// W6:providers.json 变更后重建对话模型路由表 + 密钥播种(缺则种,INV-5:
/// 明文仍只落盘 providers.json,密钥库只进加密副本)。
/// 有密钥的 provider 才入路由;模型 id 跨 provider 重复 = 先到优先(告警);
/// cfg.model_routes/secrets 缺省(测试态)为空操作。
pub fn rebuild_routes(cfg: &AdminConfig) {
    let (Some(rc), Some(secrets)) = (&cfg.model_routes, &cfg.secrets) else {
        return;
    };
    let mut table: std::collections::HashMap<String, Arc<dyn bm_core::ports::ModelConnector>> =
        std::collections::HashMap::new();
    let list = match read_providers(&cfg.data_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[W6] 无法读取 providers 重建路由: {}", e.1);
            return;
        }
    };
    for p in list {
        let (Some(base), Some(key)) = (
            p["baseUrl"].as_str(),
            p["apiKey"].as_str().filter(|s| !s.is_empty()),
        ) else {
            continue;
        };
        let connector: Arc<dyn bm_core::ports::ModelConnector> = Arc::new(
            bm_providers::openai_http::OpenAiConnector::new(base.to_string(), secrets.clone()),
        );
        let models = p["models"].as_array().cloned().unwrap_or_default();
        for m in models {
            let Some(id) = m.as_str().map(|s| s.to_string()) else {
                continue;
            };
            if table.contains_key(&id) {
                eprintln!("[W6] 模型「{id}」在多个 provider 重复,路由保留先到者");
                continue;
            }
            let secret_ref = bm_core::runtime::default_secret_ref(&id);
            if bm_core::ports::SecretStore::get(secrets.as_ref(), &secret_ref).is_err()
                && let Err(e) = bm_core::ports::SecretStore::put(secrets.as_ref(), &secret_ref, key)
            {
                eprintln!("[W6] 模型「{id}」密钥播种失败(不入路由): {e:?}");
                continue;
            }
            // INV-5:将播种的 provider API key 同步注册进脱敏扫描面
            cfg.handle.register_redaction_value(key);
            table.insert(id, connector.clone());
        }
    }
    rc.replace_table(table);
}

// ---- handler:provider CRUD ---------------------------------------------

pub async fn providers_list(State(cfg): State<AdminConfig>) -> Response {
    let raw_list = match read_providers(&cfg.data_dir) {
        Ok(l) => l,
        Err(e) => return admin_error(e.0, e.1),
    };
    let list = raw_list.iter().map(mask_provider).collect::<Vec<_>>();
    Json(json!({ "providers": list })).into_response()
}

pub async fn providers_create(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    if let Err(e) = validate_provider_input(&body) {
        return admin_error(e.0, e.1);
    }
    let mut list = match read_providers(&cfg.data_dir) {
        Ok(l) => l,
        Err(e) => return admin_error(e.0, e.1),
    };
    let record = json!({
        "id": new_provider_id(),
        "name": body["name"],
        "baseUrl": body["baseUrl"],
        "apiKey": norm_key(&body),
        "models": body["models"].as_array().cloned().unwrap_or_default(),
        "modelsCommon": body["modelsCommon"].as_array().cloned().unwrap_or_default(),
        "modelWindows": body["modelWindows"].as_object().cloned().unwrap_or_default(),
        "defaultModel": body["defaultModel"].as_str().unwrap_or(""),
    });
    list.push(record.clone());
    if let Err(e) = write_providers(&cfg.data_dir, &list) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    rebuild_routes(&cfg);
    Json(json!({ "provider": mask_provider(&record) })).into_response()
}

pub async fn providers_update(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    if let Err(e) = validate_provider_input(&body) {
        return admin_error(e.0, e.1);
    }
    let mut list = match read_providers(&cfg.data_dir) {
        Ok(l) => l,
        Err(e) => return admin_error(e.0, e.1),
    };
    let Some(pos) = list.iter().position(|p| p["id"] == json!(id)) else {
        return admin_error(StatusCode::NOT_FOUND, format!("provider '{id}' 不存在"));
    };
    let mut record = list[pos].clone();
    record["name"] = body["name"].clone();
    record["baseUrl"] = body["baseUrl"].clone();
    // apiKey 缺省/null/空 = 保持不变(ADR-0012 密钥口径);显式清除走字段删除
    if let Some(k) = norm_key(&body) {
        record["apiKey"] = json!(k);
    }
    if let Some(models) = body["models"].as_array() {
        record["models"] = json!(models);
    }
    // W6 常用清单:缺省 = 保持不变(与 models 同口径)
    if let Some(mc) = body["modelsCommon"].as_array() {
        record["modelsCommon"] = json!(mc);
    }
    // 模型窗口登记:缺省 = 保持不变;显式给空对象 = 清空
    if let Some(w) = body["modelWindows"].as_object() {
        record["modelWindows"] = json!(w);
    }
    if let Some(dm) = body["defaultModel"].as_str() {
        record["defaultModel"] = json!(dm);
    }
    list[pos] = record.clone();
    if let Err(e) = write_providers(&cfg.data_dir, &list) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    rebuild_routes(&cfg);
    Json(json!({ "provider": mask_provider(&record) })).into_response()
}

pub async fn providers_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let mut list = match read_providers(&cfg.data_dir) {
        Ok(l) => l,
        Err(e) => return admin_error(e.0, e.1),
    };
    let before = list.len();
    list.retain(|p| p["id"] != json!(id));
    if list.len() == before {
        return admin_error(StatusCode::NOT_FOUND, format!("provider '{id}' 不存在"));
    }
    // 删除 provider = 其密钥一并清除(明文只在该条目内,条目移除即没)
    if let Err(e) = write_providers(&cfg.data_dir, &list) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    rebuild_routes(&cfg);
    Json(json!({ "ok": true })).into_response()
}

/// 连通性探针 + 模型清单拉取(一个端点双用途:GET {baseUrl}/models,
/// OpenAI 兼容网关必有;2xx = 连通绿,同时解析 data[].id 回模型清单)。
/// UA 自报(opencode zen 网关套 Cloudflare,无 UA 拒收——W1 已踩实)。
pub async fn providers_probe(State(_cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let Some(base) = body["baseUrl"].as_str().map(|s| s.trim_end_matches('/')) else {
        return admin_error(StatusCode::BAD_REQUEST, "baseUrl 必须是字符串");
    };
    if !(base.len() <= 500 && (base.starts_with("http://") || base.starts_with("https://"))) {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "baseUrl 必须以 http:// 或 https:// 开头",
        );
    }
    let client = match reqwest::Client::builder()
        .user_agent(concat!("boenmind-server/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return admin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("HTTP 客户端构建失败: {e}"),
            );
        }
    };
    let mut req = client.get(format!("{base}/models"));
    if let Some(k) = body["apiKey"].as_str().filter(|s| !s.is_empty()) {
        req = req.bearer_auth(k);
    }
    let started = std::time::Instant::now();
    match req.send().await {
        Ok(resp) => {
            let latency = started.elapsed().as_millis() as u64;
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            if (200..300).contains(&status) {
                let models = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["data"].as_array().cloned())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Json(
                    json!({ "ok": true, "status": status, "latencyMs": latency, "models": models }),
                )
                .into_response()
            } else {
                let snippet: String = text.chars().take(200).collect();
                Json(json!({ "ok": false, "status": status, "latencyMs": latency, "error": snippet }))
                    .into_response()
            }
        }
        Err(e) => {
            let latency = started.elapsed().as_millis() as u64;
            Json(json!({ "ok": false, "latencyMs": latency, "error": format!("{e}") }))
                .into_response()
        }
    }
}

// ---- handler:当前生效模型(config/model.json,重启生效)----------------

pub async fn model_active_get(State(cfg): State<AdminConfig>) -> Response {
    let store = ModelConfigStore::new(&cfg.data_dir);
    Json(store.get()).into_response()
}

/// 「设为当前」:把选中 provider 落入 config/model.json(重启生效)。
pub async fn model_active_set(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let Some(id) = body["providerId"].as_str() else {
        return admin_error(StatusCode::BAD_REQUEST, "providerId 必须是字符串");
    };
    let list = match read_providers(&cfg.data_dir) {
        Ok(l) => l,
        Err(e) => return admin_error(e.0, e.1),
    };
    let Some(p) = list.iter().find(|p| p["id"] == json!(id)) else {
        return admin_error(StatusCode::NOT_FOUND, format!("provider '{id}' 不存在"));
    };
    let model_id = body["modelId"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| p["defaultModel"].as_str().map(|s| s.to_string()))
        .or_else(|| {
            p["models"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        });
    let Some(model_id) = model_id else {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "该 provider 没有可用模型(先拉取模型清单)",
        );
    };
    let store = ModelConfigStore::new(&cfg.data_dir);
    let mut values = json!({
        "baseUrl": p["baseUrl"],
        "modelId": model_id,
        "displayName": p["name"],
    });
    if let Some(k) = p["apiKey"].as_str() {
        values["apiKey"] = json!(k);
    }
    if let Some(models) = p["models"].as_array() {
        values["models"] = json!(models);
    }
    // 模型窗口登记并入 model.json(透视面板「真实水位」数据源):
    // 已登记的旧值保留(换 provider 不丢历史登记),provider 侧新值覆盖同名键。
    let mut windows = store.get()["values"]["contextWindows"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if let Some(pw) = p["modelWindows"].as_object() {
        for (k, v) in pw {
            windows.insert(k.clone(), v.clone());
        }
    }
    if !windows.is_empty() {
        values["contextWindows"] = json!(windows);
    }
    match store.set(&values) {
        Ok(_) => Json(json!({ "ok": true, "restartRequired": true, "note": "已写入 config/model.json,重启服务器后生效" })).into_response(),
        Err(e) => admin_error(StatusCode::BAD_REQUEST, format!("{e}")),
    }
}
