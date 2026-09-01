//! W2 管理面(ADR-0014 W 序列):provider 库增删改查 + 连通探针/模型清单、
//! MCP 接入配置管理、插件(能力)清单、工作区文件浏览(只读)。
//!
//! 形态裁决(2026-09-01,登记于 W2 规格 §5):
//! - 本面是 webapp 壳子私用的 REST 端点,不走 Wire 信封协议(dsh 协议
//!   已随 ADR-0013 归档);**整批暂不入 boenmind-contracts 冻结库**,
//!   以本模块 + W2 实现规格约束,W 序列稳定后一次性评估入册
//!   (合同只增不破,晚入不亏);
//! - 公开挂载 = W1 同款**已登记欠账**(单机 localhost 口径;公网部署前
//!   补 Bearer,沿 ADR-0009 T-13/T-14);
//! - 「插件」对象语义 = 运行时能力提供方(用户裁决 2026-09-01 视同确认,
//!   选项已按推荐执行):清单 = 内置能力(系统类,禁卸载)+ MCP 服务器组
//!   (卸载 = 移出 MCP 配置文件,重启生效);PIN 是壳子本地偏好,不入后端;
//! - 变更生效时机沿 ADR-0012 口径:落盘后**下次启动生效**(v0 诚实边界,
//!   前端明示「重启生效」)。
//!
//! 安全:
//! - provider apiKey 回显恒打码(与 config_store 同口径,INV-5 面);
//! - 文件浏览限 workspace_root 内:路径组件白名单(Normal 段)+ 逐级
//!   拒符号链接 + realpath 包含校验(X-01 先例:lstat 拒链 + realpath
//!   包含校验);文件读取只读、≤512KB、非 UTF-8 拒(二进制不预览)。

use crate::config_store::ModelConfigStore;
use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// 管理面配置(服务器启动时装配注入)。
#[derive(Clone)]
pub struct AdminConfig {
    /// 数据目录(config/ 文件根:providers.json / model.json)。
    pub data_dir: PathBuf,
    /// 文件浏览根(BOEN_WORKSPACE_DIR env > <data_dir>/workspace)。
    pub workspace_root: PathBuf,
    /// MCP 配置文件路径(--mcp-config;None = 未启用 MCP 接入)。
    pub mcp_config: Option<PathBuf>,
    /// 内置能力清单摘要(server 启动时从 capability 注册集提取:
    /// [{name, provider, effect, idempotent}])。
    pub builtin_caps: Arc<Vec<Value>>,
    /// 已装载的 MCP 服务器([{name, tools}];reload 会追加,可变共享)。
    pub mcp_servers: Arc<std::sync::RwLock<Vec<Value>>>,
    /// 热装载句柄:运行期把新 MCP server 的能力注册进核心(actor 命令)。
    pub handle: bm_core::runtime::RuntimeHandle,
    /// MCP hub(与启动装载共用同一实例;None = 启动未配 --mcp-config)。
    pub hub: Option<Arc<bm_providers::mcp::McpHub>>,
    /// MCP env secret: 引用解析用加密库(与启动装载同一实例)。
    pub secrets: Option<Arc<dyn bm_core::ports::SecretStore>>,
}

/// 文件预览大小上限(512KB;个人单机预览面,防整读大文件)。
const FILE_PREVIEW_LIMIT: u64 = 512 * 1024;

// ---- provider 库(config/providers.json)--------------------------------

fn providers_file(data_dir: &Path) -> PathBuf {
    data_dir.join("config/providers.json")
}

fn read_providers(data_dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(providers_file(data_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["providers"].as_array().cloned())
        .unwrap_or_default()
}

fn write_providers(data_dir: &Path, providers: &[Value]) -> Result<(), String> {
    let path = providers_file(data_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("配置目录创建失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&json!({ "providers": providers }))
        .map_err(|_| "序列化失败".to_string())?
        .replace('\n', "\r\n");
    std::fs::write(&path, text).map_err(|e| format!("配置文件写入失败: {e}"))
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
        "defaultModel": p["defaultModel"],
        "secretSet": p["apiKey"].as_str().map(|s| !s.is_empty()).unwrap_or(false),
    })
}

fn new_provider_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("系统熵源不可用");
    let mut s = String::from("prov_");
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

// ---- 工作区文件浏览(X-01 先例:组件白名单 + 拒链 + realpath 包含)-----

/// 相对路径安全解析:仅接受 Normal 段;逐级拒符号链接;canonicalize 后
/// 必须仍位于 workspace root 内。rel 为空 = 根。
fn safe_resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.starts_with(['/', '\\']) || rel.as_bytes().get(1) == Some(&b':') {
        return Err("拒绝绝对路径".to_string());
    }
    let rel = rel.trim_start_matches(['/', '\\']);
    let mut cur = root.to_path_buf();
    if !rel.is_empty() {
        for seg in Path::new(rel).components() {
            match seg {
                Component::Normal(s) => cur.push(s),
                _ => return Err("路径含非法段(拒绝 .. 与绝对路径)".to_string()),
            }
        }
        // 逐级拒链:从 root 下第一级起检查(根自身由启动方保证)
        let mut probe = root.to_path_buf();
        for seg in Path::new(rel).components() {
            if let Component::Normal(s) = seg {
                probe.push(s);
                let meta =
                    std::fs::symlink_metadata(&probe).map_err(|_| "路径不存在".to_string())?;
                if meta.file_type().is_symlink() {
                    return Err("拒绝符号链接".to_string());
                }
            }
        }
    }
    // realpath 包含校验(末端可不存在于 list 场景;此处两场景都要求存在)
    let canon_root = std::fs::canonicalize(root).map_err(|_| "工作区根不可用".to_string())?;
    let canon = std::fs::canonicalize(&cur).map_err(|_| "路径不存在".to_string())?;
    if !canon.starts_with(&canon_root) {
        return Err("路径越出工作区".to_string());
    }
    Ok(canon)
}

// ---- handler:provider CRUD ---------------------------------------------

pub async fn providers_list(State(cfg): State<AdminConfig>) -> Response {
    let list = read_providers(&cfg.data_dir)
        .iter()
        .map(mask_provider)
        .collect::<Vec<_>>();
    Json(json!({ "providers": list })).into_response()
}

pub async fn providers_create(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    if let Err(e) = validate_provider_input(&body) {
        return admin_error(e.0, e.1);
    }
    let mut list = read_providers(&cfg.data_dir);
    let record = json!({
        "id": new_provider_id(),
        "name": body["name"],
        "baseUrl": body["baseUrl"],
        "apiKey": norm_key(&body),
        "models": body["models"].as_array().cloned().unwrap_or_default(),
        "defaultModel": body["defaultModel"].as_str().unwrap_or(""),
    });
    list.push(record.clone());
    if let Err(e) = write_providers(&cfg.data_dir, &list) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
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
    let mut list = read_providers(&cfg.data_dir);
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
    if let Some(dm) = body["defaultModel"].as_str() {
        record["defaultModel"] = json!(dm);
    }
    list[pos] = record.clone();
    if let Err(e) = write_providers(&cfg.data_dir, &list) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "provider": mask_provider(&record) })).into_response()
}

pub async fn providers_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let mut list = read_providers(&cfg.data_dir);
    let before = list.len();
    list.retain(|p| p["id"] != json!(id));
    if list.len() == before {
        return admin_error(StatusCode::NOT_FOUND, format!("provider '{id}' 不存在"));
    }
    // 删除 provider = 其密钥一并清除(明文只在该条目内,条目移除即没)
    if let Err(e) = write_providers(&cfg.data_dir, &list) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
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
    let list = read_providers(&cfg.data_dir);
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
    match store.set(&values) {
        Ok(_) => Json(json!({ "ok": true, "restartRequired": true, "note": "已写入 config/model.json,重启服务器后生效" })).into_response(),
        Err(e) => admin_error(StatusCode::BAD_REQUEST, format!("{e}")),
    }
}

// ---- handler:MCP 配置管理(落盘重启生效)--------------------------------

fn mcp_file_or_error(cfg: &AdminConfig) -> Result<PathBuf, (StatusCode, String)> {
    match &cfg.mcp_config {
        Some(p) => Ok(p.clone()),
        None => Err((
            StatusCode::BAD_REQUEST,
            "服务器未启用 MCP 配置文件(--mcp-config),无法管理".to_string(),
        )),
    }
}

fn read_mcp_servers(path: &Path) -> Result<Vec<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let arr: Vec<Value> =
                serde_json::from_str(&text).map_err(|e| format!("MCP 配置不是 JSON 数组: {e}"))?;
            Ok(arr)
        }
        Err(_) => Ok(vec![]), // 文件不存在 = 空清单(首条新增时创建)
    }
}

fn write_mcp_servers(path: &Path, servers: &[Value]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("配置目录创建失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(servers)
        .map_err(|_| "序列化失败".to_string())?
        .replace('\n', "\r\n");
    std::fs::write(path, text).map_err(|e| format!("MCP 配置写入失败: {e}"))
}

/// 单条过合同 schema(mcp-server.v0_1,x-frozen;v0 仅 stdio 传输)。
fn validated_mcp_entry(body: &Value) -> Result<Value, String> {
    let mut entry = json!({
        "name": body["name"].as_str().unwrap_or(""),
        "transport": "stdio",
        "command": body["command"].as_str().unwrap_or(""),
        "args": body["args"].as_array().cloned().unwrap_or_default(),
    });
    if let Some(env) = body["env"].as_object() {
        entry["env"] = json!(env);
    }
    if let Some(t) = body["tool_timeout_ms"].as_u64() {
        entry["tool_timeout_ms"] = json!(t);
    }
    if let Some(r) = body["restart_limit"].as_u64() {
        entry["restart_limit"] = json!(r);
    }
    bm_contract::schemas::validate(bm_contract::registries::MCP_SERVER_SCHEMA, &entry)
        .map_err(|e| format!("MCP 配置项不合规: {e}"))?;
    Ok(entry)
}

pub async fn mcp_list(State(cfg): State<AdminConfig>) -> Response {
    let path = match mcp_file_or_error(&cfg) {
        Ok(p) => p,
        Err((s, m)) => return admin_error(s, m),
    };
    match read_mcp_servers(&path) {
        Ok(servers) => {
            let loaded: Vec<String> = cfg
                .mcp_servers
                .read()
                .map(|g| g.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
                .iter()
                .filter_map(|s| s["name"].as_str().map(|n| n.to_string()))
                .collect();
            // 自声明式配置:manifests/<name>.manifest.json(配置 schema)+
            // config/mcp-<name>.json(当前配置值),均在 mcp.json 同级目录约定
            let manifests_dir = path.parent().map(|d| d.join("manifests"));
            let config_dir = path.parent().map(|d| d.join("config"));
            let enriched: Vec<Value> = servers
                .iter()
                .map(|srv| {
                    let name = srv["name"].as_str().unwrap_or("");
                    let manifest = manifests_dir
                        .as_ref()
                        .and_then(|d| {
                            std::fs::read_to_string(d.join(format!("{name}.manifest.json"))).ok()
                        })
                        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
                    let config = config_dir
                        .as_ref()
                        .and_then(|d| {
                            std::fs::read_to_string(d.join(format!("mcp-{name}.json"))).ok()
                        })
                        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                        .unwrap_or_else(|| json!({}));
                    json!({
                        "server": srv,
                        "manifest": manifest,
                        "config": config,
                    })
                })
                .collect();
            Json(json!({
                "file": path.display().to_string(),
                "servers": servers,
                "entries": enriched,
                "loadedAtBoot": loaded,
                "note": "增删改只落配置文件,重启或「重载」后生效",
            }))
            .into_response()
        }
        Err(e) => admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

pub async fn mcp_create(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let path = match mcp_file_or_error(&cfg) {
        Ok(p) => p,
        Err((s, m)) => return admin_error(s, m),
    };
    let entry = match validated_mcp_entry(&body) {
        Ok(e) => e,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    let mut servers = match read_mcp_servers(&path) {
        Ok(s) => s,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let name = entry["name"].as_str().unwrap_or("").to_string();
    if servers.iter().any(|s| s["name"] == json!(name)) {
        return admin_error(StatusCode::CONFLICT, format!("MCP server '{name}' 已存在"));
    }
    servers.push(entry);
    if let Err(e) = write_mcp_servers(&path, &servers) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "已落盘,重启服务器后生效" })).into_response()
}

pub async fn mcp_update(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = match mcp_file_or_error(&cfg) {
        Ok(p) => p,
        Err((s, m)) => return admin_error(s, m),
    };
    let entry = match validated_mcp_entry(&body) {
        Ok(e) => e,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    let mut servers = match read_mcp_servers(&path) {
        Ok(s) => s,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let Some(pos) = servers.iter().position(|s| s["name"] == json!(name)) else {
        return admin_error(StatusCode::NOT_FOUND, format!("MCP server '{name}' 不存在"));
    };
    servers[pos] = entry;
    if let Err(e) = write_mcp_servers(&path, &servers) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "已落盘,重启服务器后生效" })).into_response()
}

pub async fn mcp_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let path = match mcp_file_or_error(&cfg) {
        Ok(p) => p,
        Err((s, m)) => return admin_error(s, m),
    };
    let mut servers = match read_mcp_servers(&path) {
        Ok(s) => s,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let before = servers.len();
    servers.retain(|s| s["name"] != json!(name));
    if servers.len() == before {
        return admin_error(StatusCode::NOT_FOUND, format!("MCP server '{name}' 不存在"));
    }
    if let Err(e) = write_mcp_servers(&path, &servers) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "已从配置移除,重启服务器后生效" })).into_response()
}

// ---- handler:默认角色(W4;config/roles.json,ADR-0012 口径)---------

/// 读默认角色(设置页回显)。
pub async fn roles_get(State(cfg): State<AdminConfig>) -> Response {
    let file = roles_file(&cfg);
    let raw = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or_else(|| json!({ "name": "assistant", "system_prompt": "" }));
    Json(raw).into_response()
}

/// 写默认角色(system_prompt;保存即热生效——turn 每回合读此文件)。
pub async fn roles_set(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let file = roles_file(&cfg);
    if let Some(dir) = file.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("目录创建失败: {e}"),
        );
    }
    let doc = json!({
        "name": body["name"].as_str().unwrap_or("assistant"),
        "system_prompt": body["system_prompt"].as_str().unwrap_or(""),
    });
    let text = match serde_json::to_string_pretty(&doc) {
        Ok(t) => crate::config_store::crlf(t),
        Err(_) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, "序列化失败"),
    };
    if let Err(e) = std::fs::write(&file, text) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("写入失败: {e}"));
    }
    Json(json!({ "ok": true, "note": "已保存,下一回合起生效" })).into_response()
}

fn roles_file(cfg: &AdminConfig) -> std::path::PathBuf {
    cfg.data_dir.join("config").join("roles.json")
}

// ---- handler:MCP 探活(主动测试 + 被动轮询共用 hub.probe_server)-------

/// 主动探活单条:POST /admin/mcp/test/{name}
pub async fn mcp_test(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let Some(hub) = cfg.hub.clone() else {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "服务器未启用 MCP 接线(--mcp-config)",
        );
    };
    match hub.probe_server(&name).await {
        Ok(tools) => Json(json!({ "ok": true, "name": name, "tools": tools })).into_response(),
        Err(e) => Json(json!({ "ok": false, "name": name, "error": e })).into_response(),
    }
}

/// 批量轮询:GET /admin/mcp/status(前端定时拉取刷新状态点)
pub async fn mcp_status(State(cfg): State<AdminConfig>) -> Response {
    let Some(hub) = cfg.hub.clone() else {
        return Json(json!({ "status": [] })).into_response();
    };
    let loaded: Vec<String> = cfg
        .mcp_servers
        .read()
        .map(|g| {
            g.iter()
                .filter_map(|s| s["name"].as_str().map(|n| n.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut status = Vec::new();
    for name in loaded {
        match hub.probe_server(&name).await {
            Ok(tools) => status.push(json!({"name": name, "ok": true, "tools": tools})),
            Err(e) => status.push(json!({"name": name, "ok": false, "error": e})),
        }
    }
    Json(json!({ "status": status })).into_response()
}

// ---- handler:每 server 自声明配置(读/写 config/mcp-<name>.json)---------

#[derive(serde::Deserialize)]
pub struct McpConfigBody {
    pub values: Value,
}

/// 读某 server 当前配置值(供设置页表单回显)。
pub async fn mcp_config_get(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let Some(path) = cfg.mcp_config.clone() else {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "服务器未启用 MCP 配置文件(--mcp-config)",
        );
    };
    let file = path
        .parent()
        .map(|d| d.join("config").join(format!("mcp-{name}.json")))
        .unwrap_or_else(|| path.clone());
    let values = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or_else(|| json!({}));
    Json(json!({ "name": name, "values": values })).into_response()
}

/// 写某 server 配置值(merge 保存)。改 key 免重启(override 文件链),
/// 其余配置项重载/重启生效。
pub async fn mcp_config_set(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<McpConfigBody>,
) -> Response {
    let Some(path) = cfg.mcp_config.clone() else {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "服务器未启用 MCP 配置文件(--mcp-config)",
        );
    };
    let Some(dir) = path.parent().map(|d| d.join("config")) else {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, "配置目录解析失败");
    };
    let Some(values) = body.values.as_object() else {
        return admin_error(StatusCode::BAD_REQUEST, "values 必须是对象");
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("配置目录创建失败: {e}"),
        );
    }
    let file = dir.join(format!("mcp-{name}.json"));
    let mut current: Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    if let Some(obj) = current.as_object_mut() {
        for (k, v) in values {
            obj.insert(k.clone(), v.clone());
        }
    }
    // CRLF 统一:与 config_store.write_file 同款(pretty 后按平台换行)
    let text = match serde_json::to_string_pretty(&current) {
        Ok(t) => crate::config_store::crlf(t),
        Err(_) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, "序列化失败"),
    };
    if let Err(e) = std::fs::write(&file, text) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("写入失败: {e}"));
    }
    Json(json!({
        "ok": true,
        "file": file.display().to_string(),
        "note": "已保存;改 Key 对下一次搜索立即生效,其余项重载/重启生效",
    }))
    .into_response()
}

// ---- handler:插件(能力)清单 --------------------------------------------

/// 插件 = 运行时能力提供方:builtin(系统类,禁卸载)+ MCP 服务器组
/// (可卸载 = 移出 MCP 配置,重启生效)。
/// MCP 项 = **配置文件全集**(与 MCP 管理页同源,否则运行期新增的条目
/// 在插件页不可见、卸载落空);loaded = 本次启动已装载;pendingRemoval =
/// 已从文件移除但仍在本次启动清单中(重启后消失)。
pub async fn capabilities_list(State(cfg): State<AdminConfig>) -> Response {
    let file_servers: Vec<Value> = cfg
        .mcp_config
        .as_ref()
        .map(|p| read_mcp_servers(p).unwrap_or_default())
        .unwrap_or_default();
    let mut mcp: Vec<Value> = file_servers
        .iter()
        .map(|s| {
            let name = s["name"].as_str().unwrap_or("");
            let boot = cfg
                .mcp_servers
                .read()
                .ok()
                .and_then(|g| g.iter().find(|b| b["name"].as_str() == Some(name)).cloned());
            json!({
                "name": name,
                "tools": boot.as_ref().map(|b| b["tools"].clone()).unwrap_or(Value::Null),
                "loaded": boot.is_some(),
                "pendingRemoval": false,
            })
        })
        .collect();
    let boot_snapshot = cfg
        .mcp_servers
        .read()
        .map(|g| g.clone())
        .unwrap_or_default();
    for b in boot_snapshot.iter() {
        let name = b["name"].as_str().unwrap_or("");
        if !file_servers
            .iter()
            .any(|s| s["name"].as_str() == Some(name))
        {
            mcp.push(json!({
                "name": name, "tools": b["tools"].clone(),
                "loaded": true, "pendingRemoval": true,
            }));
        }
    }
    Json(json!({
        "builtin": cfg.builtin_caps.iter().cloned().collect::<Vec<_>>(),
        "mcp": mcp,
    }))
    .into_response()
}

// ---- handler:工作区文件浏览(只读)---------------------------------------

#[derive(serde::Deserialize)]
pub struct FsPathParams {
    #[serde(default)]
    pub path: String,
}

pub async fn fs_list(State(cfg): State<AdminConfig>, Query(p): Query<FsPathParams>) -> Response {
    let dir = match safe_resolve(&cfg.workspace_root, &p.path) {
        Ok(d) => d,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    if !dir.is_dir() {
        return admin_error(StatusCode::BAD_REQUEST, "目标不是目录");
    }
    let mut entries = Vec::new();
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => {
            return admin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("读取目录失败: {e}"),
            );
        }
    };
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        // 目录内容里的符号链接不跟随:显示为 file 但不带 size(读取会被拒链)
        let is_dir = meta.is_dir();
        entries.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "kind": if is_dir { "dir" } else { "file" },
            "size": if is_dir { Value::Null } else { json!(meta.len()) },
        }));
    }
    entries.sort_by(|a, b| {
        let ka = if a["kind"] == "dir" { 0 } else { 1 };
        let kb = if b["kind"] == "dir" { 0 } else { 1 };
        ka.cmp(&kb).then_with(|| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
        })
    });
    Json(json!({
        "path": p.path,
        "entries": entries,
        "root": cfg.workspace_root.display().to_string(),
    }))
    .into_response()
}

pub async fn fs_file(State(cfg): State<AdminConfig>, Query(p): Query<FsPathParams>) -> Response {
    let file = match safe_resolve(&cfg.workspace_root, &p.path) {
        Ok(f) => f,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    let Ok(meta) = std::fs::metadata(&file) else {
        return admin_error(StatusCode::NOT_FOUND, "文件不存在");
    };
    if meta.is_dir() {
        return admin_error(StatusCode::BAD_REQUEST, "目标是目录,请先展开目录树");
    }
    if meta.len() > FILE_PREVIEW_LIMIT {
        return admin_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "文件超过预览上限({}KB > {}KB)",
                meta.len() / 1024,
                FILE_PREVIEW_LIMIT / 1024
            ),
        );
    }
    match std::fs::read(&file) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Json(json!({
                "path": p.path,
                "name": file.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
                "size": meta.len(),
                "content": text,
            }))
            .into_response(),
            Err(_) => admin_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "二进制文件不支持预览(仅 UTF-8 文本)",
            ),
        },
        Err(e) => admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("读取失败: {e}")),
    }
}

// ---- 错误形状 ------------------------------------------------------------

/// 管理面统一错误形状(壳子私用 REST 惯例,非 Wire 信封)。
fn admin_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": { "message": message.into() } })),
    )
        .into_response()
}

// ---- handler:MCP 插件目录扫描与批准接入(两段式,2026-09-02 用户批准)----
//
// 目录约定:MCP 插件(可执行文件)放 `<mcp.json 同级>/mcp/`。扫描只认该
// 目录;候选以 `--self-describe` 参数打印声明 JSON 识别(识别过程会运行
// 候选文件——用户手动放入即视为安装意图,正式激活仍以「批准接入」落盘
// mcp.json 为准,显式批准=安装,ADR-0005/0006)。

fn mcp_plugins_dir(path: &Path) -> PathBuf {
    path.parent()
        .map(|d| d.join("mcp"))
        .unwrap_or_else(|| path.to_path_buf())
}

/// 运行单个候选的 --self-describe(5s 超时;stdin 接 null 防候选挂住;
/// 超时/退出失败/输出无 JSON = 非候选)。
async fn self_describe(path: &Path) -> Option<Value> {
    let spawn_result = tokio::process::Command::new(path)
        .arg("--self-describe")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match spawn_result {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[mcp-candidates] 候选 spawn 失败 {}: {e}", path.display());
            return None;
        }
    };
    let mut stdout = child.stdout.take()?;
    let mut out = Vec::new();
    let wait = async {
        use tokio::io::AsyncReadExt;
        let _ = stdout.read_to_end(&mut out).await;
        let _ = child.wait().await;
    };
    if tokio::time::timeout(std::time::Duration::from_secs(5), wait)
        .await
        .is_err()
    {
        return None;
    }
    let text = String::from_utf8_lossy(&out);
    for line in text.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line.trim())
            && v.get("name").and_then(Value::as_str).is_some()
        {
            return Some(v);
        }
    }
    eprintln!(
        "[mcp-candidates] 候选 {} 自描述输出 {} 字节,无有效声明行;前 160 字节: {:?}",
        path.display(),
        out.len(),
        text.get(..160).unwrap_or(&text)
    );
    None
}

fn candidate_is_executable(path: &Path) -> bool {
    #[cfg(windows)]
    {
        matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("exe") | Some("cmd") | Some("bat")
        )
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
}

/// POST /admin/mcp/candidates:扫描插件目录,返回可批准接入的候选清单
/// (含已在 mcp.json 中的标记,便于前端过滤)。
pub async fn mcp_candidates(State(cfg): State<AdminConfig>) -> Response {
    let path = match mcp_file_or_error(&cfg) {
        Ok(p) => p,
        Err((s, m)) => return admin_error(s, m),
    };
    let dir = mcp_plugins_dir(&path);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("插件目录创建失败: {e}"),
        );
    }
    let registered: Vec<String> = read_mcp_servers(&path)
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s["name"].as_str().map(String::from))
        .collect();
    let mut candidates: Vec<Value> = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => {
            return admin_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("插件目录读取失败: {e}"),
            );
        }
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() || !candidate_is_executable(&p) {
            continue;
        }
        let Some(decl) = self_describe(&p).await else {
            continue;
        };
        let name = decl["name"].as_str().unwrap_or_default().to_string();
        if name.is_empty() {
            continue;
        }
        candidates.push(json!({
            "file": p.display().to_string(),
            "name": name,
            "title": decl.get("title").cloned().unwrap_or(json!("")),
            "description": decl.get("description").cloned().unwrap_or(json!("")),
            "registered": registered.iter().any(|r| r == &name),
        }));
    }
    Json(json!({
        "ok": true,
        "dir": dir.display().to_string(),
        "candidates": candidates,
        "note": "扫描会以 --self-describe 运行目录内可执行文件;批准后才落盘 mcp.json",
    }))
    .into_response()
}

/// POST /admin/mcp/approve:批准候选接入。body {"name": "..."}。
/// 落盘两处:mcp.json 条目(command=候选路径,args 用声明模板替换
/// {config_file} 为数据目录配置路径)+ manifests/<name>.manifest.json
/// (设置页配置表单的声明来源)。新增条目随后「重载 MCP」免重启上线。
pub async fn mcp_approve(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let path = match mcp_file_or_error(&cfg) {
        Ok(p) => p,
        Err((s, m)) => return admin_error(s, m),
    };
    let dir = mcp_plugins_dir(&path);
    let want_name = body["name"].as_str().unwrap_or_default().to_string();
    if want_name.is_empty() {
        return admin_error(StatusCode::BAD_REQUEST, "缺少 name");
    }
    // 在插件目录内找到声明 name 匹配的候选(目录限定,防路径逃逸)
    let mut target: Option<PathBuf> = None;
    let mut decl: Option<Value> = None;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_file() || !candidate_is_executable(&p) {
                continue;
            }
            if let Some(d) = self_describe(&p).await
                && d["name"].as_str() == Some(want_name.as_str())
            {
                target = Some(p);
                decl = Some(d);
                break;
            }
        }
    }
    let (Some(file), Some(decl)) = (target, decl) else {
        return admin_error(
            StatusCode::NOT_FOUND,
            format!("插件目录中没有自声明 name={want_name} 的候选"),
        );
    };

    // args 模板:{config_file} → 数据目录 config/mcp-<name>.json
    let config_dir = path
        .parent()
        .map(|d| d.join("config"))
        .unwrap_or_else(|| path.clone());
    let config_file = config_dir.join(format!("mcp-{want_name}.json"));
    let placeholder = "{config_file}".to_string();
    let default_args = vec![Value::String("--config".into()), Value::String(placeholder)];
    let template = decl
        .pointer("/suggested_entry/args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or(default_args);
    let args: Vec<Value> = template
        .iter()
        .map(|a| match a.as_str() {
            Some(s) => json!(s.replace("{config_file}", &config_file.display().to_string())),
            None => a.clone(),
        })
        .collect();
    let entry_body = json!({
        "name": want_name,
        "command": file.display().to_string(),
        "args": args,
        "tool_timeout_ms": decl.pointer("/suggested_entry/tool_timeout_ms").cloned().unwrap_or(json!(30000)),
        "restart_limit": decl.pointer("/suggested_entry/restart_limit").cloned().unwrap_or(json!(3)),
    });
    let entry = match validated_mcp_entry(&entry_body) {
        Ok(e) => e,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };

    let mut servers = match read_mcp_servers(&path) {
        Ok(s) => s,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    if servers
        .iter()
        .any(|s| s["name"].as_str() == Some(want_name.as_str()))
    {
        return admin_error(
            StatusCode::CONFLICT,
            format!("MCP server '{want_name}' 已存在"),
        );
    }
    servers.push(entry.clone());
    if let Err(e) = write_mcp_servers(&path, &servers) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }

    // 双写 manifests/<name>.manifest.json(设置页「配置」表单的声明来源)
    let manifest = json!({
        "name": want_name,
        "title": decl.get("title").cloned().unwrap_or(json!("")),
        "description": decl.get("description").cloned().unwrap_or(json!("")),
        "config_schema": decl.get("config_schema").cloned().unwrap_or(json!([])),
    });
    if let Some(mdir) = path.parent().map(|d| d.join("manifests")) {
        let _ = std::fs::create_dir_all(&mdir);
        if let Ok(text) = serde_json::to_string_pretty(&manifest) {
            let _ = std::fs::write(mdir.join(format!("{want_name}.manifest.json")), text);
        }
    }

    Json(json!({
        "ok": true,
        "entry": entry,
        "note": "已落盘(mcp.json + manifest);点「重载 MCP」免重启上线",
    }))
    .into_response()
}

/// 管理面子路由(挂载于 /admin;公开 = W1 同款已登记欠账)。
pub fn admin_routes(cfg: AdminConfig) -> axum::Router {
    use axum::routing::{get, post, put};
    let cfg = Arc::new(cfg);
    axum::Router::new()
        .route("/providers", get(providers_list).post(providers_create))
        .route("/providers/probe", post(providers_probe))
        .route(
            "/providers/{id}",
            put(providers_update).delete(providers_delete),
        )
        .route("/model/active", get(model_active_get).put(model_active_set))
        .route("/mcp", get(mcp_list).post(mcp_create))
        .route("/mcp/reload", post(mcp_reload))
        .route("/mcp/candidates", post(mcp_candidates))
        .route("/mcp/approve", post(mcp_approve))
        .route("/mcp/test/{name}", post(mcp_test))
        .route("/mcp/status", get(mcp_status))
        .route(
            "/mcp-config/{name}",
            get(mcp_config_get).put(mcp_config_set),
        )
        .route("/mcp/{name}", put(mcp_update).delete(mcp_delete))
        .route("/capabilities", get(capabilities_list))
        .route("/roles", get(roles_get).put(roles_set))
        .route("/fs/list", get(fs_list))
        .route("/fs/file", get(fs_file))
        .with_state((*cfg).clone())
}

// ---- handler:MCP 热装载(v0 收敛:只装载新增 server,免重启)---------------

/// 重载 MCP 配置:读 mcp.json 全量,对比已装载名单,**仅对新增条目**执行
/// spawn+握手+运行期注册(修改/删除仍需重启,v0 收敛边界,UI 明示)。
/// 装载成功后刷新 AdminConfig.mcp_servers 快照(插件清单页可见)。
pub async fn mcp_reload(State(cfg): State<AdminConfig>) -> Response {
    use bm_providers::mcp::{McpHub, StdioMcpTransport};

    let Some(path) = cfg.mcp_config.clone() else {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "服务器未启用 MCP 配置文件(--mcp-config)",
        );
    };
    let Some(hub) = cfg.hub.clone() else {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "启动时未完成 MCP 接线(检查启动日志的 MCP server 装载行)",
        );
    };
    let Some(secrets) = cfg.secrets.clone() else {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, "Secret Store 未就绪");
    };

    let servers = match read_mcp_servers(&path) {
        Ok(servers) => servers,
        Err(e) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e),
    };
    let loaded_names: Vec<String> = cfg
        .mcp_servers
        .read()
        .map(|g| {
            g.iter()
                .filter_map(|s| s["name"].as_str().map(|n| n.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let fresh: Vec<Value> = servers
        .iter()
        .filter(|s| {
            let n = s["name"].as_str().unwrap_or("");
            !n.is_empty() && !loaded_names.iter().any(|l| l == n)
        })
        .cloned()
        .collect();
    if fresh.is_empty() {
        return Json(json!({
            "ok": true,
            "registered": [],
            "failed": [],
            "note": "无新增 server(修改/删除条目仍需重启服务器)",
        }))
        .into_response();
    }

    let mut registered: Vec<String> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    for item in &fresh {
        let name = item["name"].as_str().unwrap_or("").to_string();
        let command = item["command"].as_str().unwrap_or("").to_string();
        let args: Vec<String> = item["args"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let timeout = item["tool_timeout_ms"].as_u64().unwrap_or(30_000);
        // env 解析:配置文件里的 secret: 引用走与启动装载同一 SecretStore 链
        let setup = match bm_providers::mcp::load_mcp_setups(&path, secrets.as_ref()) {
            Ok(setups) => setups.into_iter().find(|s| s.name == name),
            Err(e) => {
                failed.push(json!({"name": name, "error": format!("配置解析失败: {e}")}));
                continue;
            }
        };
        let Some(setup) = setup else {
            continue;
        };
        let loaded = async {
            let transport = StdioMcpTransport::spawn(&command, &args, &setup.env_resolved)
                .map_err(|e| e.to_string())?;
            hub.connect(&name, transport, timeout)
                .await
                .map_err(|e| e.to_string())
        }
        .await;
        match loaded {
            Ok(manifests) => {
                let count = manifests.len();
                let entries = McpHub::capability_entries(manifests);
                match cfg.handle.capabilities_register(entries).await {
                    Ok(names) => {
                        registered.extend(names);
                        if let Ok(mut g) = cfg.mcp_servers.write() {
                            g.push(json!({"name": name, "tools": count}));
                        }
                    }
                    Err(e) => failed.push(json!({"name": name, "error": format!("{e}")})),
                }
            }
            Err(e) => failed.push(json!({"name": name, "error": e})),
        }
    }
    Json(json!({
        "ok": failed.is_empty(),
        "registered": registered,
        "failed": failed,
        "note": "新增已装载;修改/删除条目仍需重启服务器",
    }))
    .into_response()
}
