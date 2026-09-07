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
use bm_contract::ids::{IdGen, UlidIdGen};
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
    /// W6:对话级模型路由表(providers 写后重建;None = 未装配,如测试态)。
    pub model_routes: Option<Arc<bm_providers::routing::RoutingConnector>>,
    /// W7 在线升级:应用层停机信号(apply 后排空本进程);None = 测试态。
    pub shutdown: Option<Arc<tokio::sync::Notify>>,
    /// W7 在线升级:Web 静态目录(--web-dir,升级时覆盖 dist);None = 未挂载。
    pub web_dir: Option<PathBuf>,
    /// 官方随包 MCP 插件目录(exe 同级 plugins/,v0.0.4 起随包发布)。扫描与
    /// 批准同数据目录 mcp/ 对待,同名候选以数据目录优先;None = 测试态或
    /// 无法定位(开发态 cargo run 无此目录,静默跳过)。
    pub bundled_plugins_dir: Option<PathBuf>,
    /// W10(ADR-0024):运行时限制共享单元(缺省 = 代码默认;测试态零变化)。
    pub limits: bm_core::limits::LimitsCell,
    /// W10:来源追踪(env/file 徽标;PUT 后随写更新)。
    pub limits_sources: Arc<std::sync::Mutex<bm_core::limits::LimitsSources>>,
    /// W10(ADR-0025):后台作业台账(/admin/jobs;None = 未装配,测试态)。
    pub jobs: Option<Arc<bm_providers::jobs::JobTable>>,
}

// W10:预览/下载/删除/浏览上限走 cfg.limits
// (原 FILE_PREVIEW_LIMIT=512KB 等常量已收编进 limits,默认值同前)。

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
    bm_persist::atomic_write(path, text.as_bytes()).map_err(|e| format!("MCP 配置写入失败: {e}"))
}

/// 单条过合同 schema(mcp-server.v0_1;支持 stdio / sse / http 传输)。
fn validated_mcp_entry(body: &Value) -> Result<Value, String> {
    let transport = body["transport"].as_str().unwrap_or("stdio");
    let mut entry = json!({
        "name": body["name"].as_str().unwrap_or(""),
        "transport": transport,
    });
    if transport == "stdio" {
        entry["command"] = json!(body["command"].as_str().unwrap_or(""));
        entry["args"] = json!(body["args"].as_array().cloned().unwrap_or_default());
    } else {
        if let Some(url) = body["url"].as_str() {
            entry["url"] = json!(url);
        }
        if let Some(tok) = body["bearer_token"].as_str() {
            entry["bearer_token"] = json!(tok);
        }
        if let Some(cmd) = body["command"].as_str() {
            entry["command"] = json!(cmd);
        }
        if let Some(args) = body["args"].as_array() {
            entry["args"] = json!(args);
        }
    }
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
                    // ADR-0023:来源与弃用标记——bundled 来源但不在官方随包
                    // 清单(.official.json)=最新官方版本已不携带,建议删除
                    let origin = server_origin(&cfg, srv, &path);
                    let deprecated = origin == "bundled"
                        && official_plugin_list(&cfg)
                            .map(|l| !l.iter().any(|n| n == name))
                            .unwrap_or(false);
                    json!({
                        "server": srv,
                        "manifest": manifest,
                        "config": config,
                        "origin": origin,
                        "deprecated": deprecated,
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
    Json(json!({ "ok": true, "note": "已落盘,点「重载 MCP」可免重启生效" })).into_response()
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
    Json(json!({ "ok": true, "note": "已落盘,点「重载 MCP」可免重启生效" })).into_response()
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
    let Some(pos) = servers.iter().position(|s| s["name"] == json!(name)) else {
        return admin_error(StatusCode::NOT_FOUND, format!("MCP server '{name}' 不存在"));
    };
    let origin = server_origin(&cfg, &servers[pos], &path);
    servers.remove(pos);
    if let Err(e) = write_mcp_servers(&path, &servers) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    // ADR-0023:bundled 来源写墓碑——官方随包默认安装(启动播种)永不复活;
    // 数据目录来源不写(用户手动放置=安装意图,重启后扫描仍会作为候选出现)。
    let mut tombstoned = false;
    if origin == "bundled" {
        tombstoned = upsert_tombstone(&cfg.data_dir, &name).is_ok();
    }
    // 卸载即时下线:全量同步把摘除项 disconnect+unregister(失败不回滚配置)
    let (sync_ok, sync_detail) = match run_mcp_sync(&cfg).await {
        Ok(o) => (true, json!({ "uninstalled": o.uninstalled })),
        Err((_, m)) => (false, json!({ "skipped": m })),
    };
    let note = if sync_ok {
        "已卸载并即时下线".to_string()
    } else {
        format!(
            "已从配置移除;进程下线未完成:{};可点「重载 MCP」收尾",
            sync_detail["skipped"].as_str().unwrap_or("")
        )
    };
    Json(json!({
        "ok": true,
        "origin": origin,
        "tombstoned": tombstoned,
        "sync": sync_detail,
        "note": note,
    }))
    .into_response()
}

/// POST /admin/mcp/{name}/purge:卸载并物理删除插件文件(ADR-0023)。
/// 顺序=摘配置 → 全量同步停进程(摘除项 disconnect+unregister,ChildKill
/// 兜底杀子进程)→ 删文件(exe 删不掉走 rename-aside 让位,Windows 允许
/// 改名运行中的 exe,about.rs 同款)→ 写墓碑(将来升级若带回官方文件,
/// 默认安装仍保持停用)。
pub async fn mcp_purge(
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
    let Some(pos) = servers.iter().position(|s| s["name"] == json!(name)) else {
        return admin_error(
            StatusCode::NOT_FOUND,
            format!("MCP server '{name}' 不存在(未登记的插件无从定位其文件)"),
        );
    };
    let exe = servers[pos]["command"].as_str().map(String::from);
    servers.remove(pos);
    if let Err(e) = write_mcp_servers(&path, &servers) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    // 停进程 + 注销能力(必须在删 exe 之前,否则 Windows 文件锁挡路)
    let sync_detail = match run_mcp_sync(&cfg).await {
        Ok(o) => json!({ "ok": true, "uninstalled": o.uninstalled }),
        Err((_, m)) => json!({ "ok": false, "skipped": m }),
    };
    let mut deleted = Vec::new();
    let mut renamed_aside = Vec::new();
    let mut errors = Vec::new();
    if let Some(exe) = &exe {
        let p = std::path::Path::new(exe);
        if p.exists() {
            match std::fs::remove_file(p) {
                Ok(_) => deleted.push(exe.clone()),
                Err(e) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let aside = p.with_extension(format!("old-{now}"));
                    match std::fs::rename(p, &aside) {
                        Ok(_) => renamed_aside.push(aside.display().to_string()),
                        Err(_) => errors.push(format!("{}: {e}", exe)),
                    }
                }
            }
        }
    }
    if let Some(parent) = path.parent() {
        let manifest = parent
            .join("manifests")
            .join(format!("{name}.manifest.json"));
        if manifest.exists() && std::fs::remove_file(&manifest).is_ok() {
            deleted.push(manifest.display().to_string());
        }
        let config = parent.join("config").join(format!("mcp-{name}.json"));
        if config.exists() && std::fs::remove_file(&config).is_ok() {
            deleted.push(config.display().to_string());
        }
    }
    if let Err(e) = upsert_tombstone(&cfg.data_dir, &name) {
        errors.push(format!("墓碑写盘失败: {e}"));
    }
    Json(json!({
        "ok": errors.is_empty(),
        "deleted": deleted,
        "renamed_aside": renamed_aside,
        "errors": errors,
        "sync": sync_detail,
        "note": "已卸载并物理删除插件文件;官方随包插件将来升级可能重新出现文件,但保持停用(墓碑)",
    }))
    .into_response()
}

// ---- handler:多角色管理(W4b;config/roles.json,ADR-0012 口径)---------

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct RoleConfigDoc {
    pub active_id: String,
    pub roles: Vec<RoleItem>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct RoleItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub system_prompt: String,
    /// W4b:挂载的技能 skill_id 列表(只是数据;加载不改变权限)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
}

pub fn read_roles_doc(file: &std::path::Path) -> RoleConfigDoc {
    if let Ok(raw) = std::fs::read_to_string(file)
        && let Ok(v) = serde_json::from_str::<Value>(&raw)
    {
        if let Some(roles_arr) = v["roles"].as_array() {
            let active_id = v["active_id"].as_str().unwrap_or("assistant").to_string();
            let roles: Vec<RoleItem> = roles_arr
                .iter()
                .filter_map(|r| serde_json::from_value(r.clone()).ok())
                .collect();
            if !roles.is_empty() {
                return RoleConfigDoc { active_id, roles };
            }
        } else if let Some(sp) = v["system_prompt"].as_str() {
            let name = v["name"].as_str().unwrap_or("assistant").to_string();
            return RoleConfigDoc {
                active_id: "assistant".into(),
                roles: vec![RoleItem {
                    id: "assistant".into(),
                    name,
                    description: Some("默认通用助理".into()),
                    system_prompt: sp.to_string(),
                    skills: None,
                }],
            };
        }
    }
    RoleConfigDoc {
        active_id: "assistant".into(),
        roles: vec![RoleItem {
            id: "assistant".into(),
            name: "assistant".into(),
            description: Some("默认通用助理".into()),
            system_prompt: "".into(),
            skills: None,
        }],
    }
}

pub fn write_roles_doc(file: &std::path::Path, doc: &RoleConfigDoc) -> Result<(), String> {
    if let Some(dir) = file.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return Err(format!("目录创建失败: {e}"));
    }
    let text = match serde_json::to_string_pretty(doc) {
        Ok(t) => crate::config_store::crlf(t),
        Err(e) => return Err(format!("序列化失败: {e}")),
    };
    bm_persist::atomic_write(file, text.as_bytes()).map_err(|e| format!("写入失败: {e}"))
}

/// 读全部角色与激活角色 id(设置页与聊天页下拉)。
pub async fn roles_get(State(cfg): State<AdminConfig>) -> Response {
    let file = roles_file(&cfg);
    let doc = read_roles_doc(&file);
    Json(json!({
        "ok": true,
        "active_id": doc.active_id,
        "roles": doc.roles,
    }))
    .into_response()
}

/// 保存单角色(创建或更新,向后兼容 roles_set 以及多角色编辑)。
pub async fn roles_set(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let file = roles_file(&cfg);
    let mut doc = read_roles_doc(&file);

    // 如果传递了全量 roles 数组，则全量更新
    if let Some(roles_arr) = body["roles"].as_array() {
        let roles: Vec<RoleItem> = roles_arr
            .iter()
            .filter_map(|r| serde_json::from_value(r.clone()).ok())
            .collect();
        if roles.is_empty() {
            return admin_error(StatusCode::BAD_REQUEST, "角色列表不能为空");
        }
        let active_id = body["active_id"]
            .as_str()
            .unwrap_or(&doc.active_id)
            .to_string();
        doc.roles = roles;
        doc.active_id = active_id;
    } else {
        // 单角色增改形态
        let id = body["id"]
            .as_str()
            .unwrap_or(body["name"].as_str().unwrap_or("assistant"))
            .to_string();
        let name = body["name"].as_str().unwrap_or(&id).to_string();
        let description = body["description"].as_str().map(|s| s.to_string());
        let system_prompt = body["system_prompt"].as_str().unwrap_or("").to_string();

        if let Some(existing) = doc.roles.iter_mut().find(|r| r.id == id) {
            existing.name = name;
            existing.description = description;
            existing.system_prompt = system_prompt;
            // W4b:技能挂载(传了才覆盖,保持既有挂载不丢)
            if let Some(sk) = body["skills"].as_array() {
                existing.skills = Some(
                    sk.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect(),
                );
            }
        } else {
            doc.roles.push(RoleItem {
                id: id.clone(),
                name,
                description,
                system_prompt,
                skills: body["skills"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                }),
            });
        }
        if body["set_active"].as_bool().unwrap_or(false) {
            doc.active_id = id;
        }
    }

    if let Err(e) = write_roles_doc(&file, &doc) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "已保存,下一回合起生效", "active_id": doc.active_id }))
        .into_response()
}

/// 删除指定角色
pub async fn roles_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let file = roles_file(&cfg);
    let mut doc = read_roles_doc(&file);
    if doc.roles.len() <= 1 {
        return admin_error(StatusCode::BAD_REQUEST, "至少需要保留一个角色");
    }
    let orig_len = doc.roles.len();
    doc.roles.retain(|r| r.id != id);
    if doc.roles.len() == orig_len {
        return admin_error(StatusCode::NOT_FOUND, "指定角色不存在");
    }
    if doc.active_id == id {
        doc.active_id = doc.roles[0].id.clone();
    }
    if let Err(e) = write_roles_doc(&file, &doc) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "角色已删除", "active_id": doc.active_id })).into_response()
}

/// 设置默认激活角色
pub async fn roles_set_active(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let file = roles_file(&cfg);
    let mut doc = read_roles_doc(&file);
    if !doc.roles.iter().any(|r| r.id == id) {
        return admin_error(StatusCode::NOT_FOUND, "指定角色不存在");
    }
    doc.active_id = id;
    if let Err(e) = write_roles_doc(&file, &doc) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "note": "已设为默认角色", "active_id": doc.active_id }))
        .into_response()
}

fn roles_file(cfg: &AdminConfig) -> std::path::PathBuf {
    cfg.data_dir.join("config").join("roles.json")
}

// ---- handler:技能库(W4b;config/skills.json;合同 capability/skill.v0_1)--

fn skills_file(cfg: &AdminConfig) -> std::path::PathBuf {
    cfg.data_dir.join("config").join("skills.json")
}

/// 读技能库(缺文件 = 空库)。
fn read_skills(file: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["skills"].as_array().cloned())
        .unwrap_or_default()
}

/// GET /admin/skills:技能库清单(角色页挂载勾选 + 展示)。
pub async fn skills_get(State(cfg): State<AdminConfig>) -> Response {
    let skills = read_skills(&skills_file(&cfg));
    Json(json!({ "ok": true, "skills": skills })).into_response()
}

/// POST /admin/skills:新建或更新技能(skill_id 同则覆盖)。
/// 校验走合同 skill.v0_1——Skill 只是数据,加载不改变权限。
pub async fn skills_set(State(cfg): State<AdminConfig>, Json(mut body): Json<Value>) -> Response {
    if body["description"].is_null() {
        body["description"] = json!(null);
    }
    if body["allowed_capabilities"].is_null() {
        body["allowed_capabilities"] = json!([]);
    }
    if let Err(e) = bm_contract::schemas::validate(bm_contract::registries::SKILL_SCHEMA, &body) {
        return admin_error(StatusCode::BAD_REQUEST, format!("技能不合规: {e}"));
    }
    let file = skills_file(&cfg);
    let mut skills = read_skills(&file);
    let id = body["skill_id"].as_str().unwrap_or_default().to_string();
    if let Some(slot) = skills
        .iter_mut()
        .find(|s| s["skill_id"].as_str() == Some(&id))
    {
        *slot = body.clone();
    } else {
        skills.push(body.clone());
    }
    if let Some(dir) = file.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("目录创建失败: {e}"),
        );
    }
    let text = match serde_json::to_string_pretty(&json!({ "skills": skills })) {
        Ok(t) => crate::config_store::crlf(t),
        Err(_) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, "序列化失败"),
    };
    if let Err(e) = bm_persist::atomic_write(&file, text.as_bytes()) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("写入失败: {e}"));
    }
    Json(json!({ "ok": true, "note": "技能已保存,下一回合起生效" })).into_response()
}

/// DELETE /admin/skills/{id}:删除技能(已挂载角色在组装时自动跳过缺失技能)。
pub async fn skills_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let file = skills_file(&cfg);
    let mut skills = read_skills(&file);
    let before = skills.len();
    skills.retain(|s| s["skill_id"].as_str() != Some(&id));
    if skills.len() == before {
        return admin_error(StatusCode::NOT_FOUND, "技能不存在");
    }
    let text = match serde_json::to_string_pretty(&json!({ "skills": skills })) {
        Ok(t) => crate::config_store::crlf(t),
        Err(_) => return admin_error(StatusCode::INTERNAL_SERVER_ERROR, "序列化失败"),
    };
    if let Err(e) = bm_persist::atomic_write(&file, text.as_bytes()) {
        return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("写入失败: {e}"));
    }
    Json(json!({ "ok": true, "note": "技能已删除" })).into_response()
}

// ---- handler:对话内审批裁决(W4b;与 /rpc/approval.respond 同一执行体,
// 走 /admin 免鉴权口径——W1 同款已登记欠账;前端审批卡片无令牌可带)------

/// POST /admin/approvals/{id}/respond  body: {decision: "approve"|"deny",
/// scope?: "once"(approve 必带,走 once 单次口径)}
pub async fn approval_respond(
    State(cfg): State<AdminConfig>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let decision = body["decision"].as_str().unwrap_or("").to_string();
    let scope = body["scope"].as_str().map(|s| s.to_string());
    let request_id = UlidIdGen.next_id("req");
    let Ok(appr_id) = bm_contract::ids::BmId::parse(&id) else {
        return admin_error(StatusCode::BAD_REQUEST, "非法审批单 id");
    };
    match cfg
        .handle
        .approval_respond(
            request_id,
            bm_contract::wire::ApprovalRespondParams {
                approval_id: appr_id,
                decision,
                scope,
            },
        )
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => admin_error(StatusCode::BAD_REQUEST, format!("审批裁决失败: {e}")),
    }
}

/// GET /admin/approvals:待裁决审批单列表(前端轮询面)。2026-09-07 批准
/// 可达性修复:审批标记此前仅随回合 /v1 流下发,流到期或后台续跑回合
/// 无主流时,审批单永远无人可批,任务一律卡死在审批轮询。本查询同时
/// 触发到期清扫,滞留单不占待决队列。
pub async fn approvals_list(State(cfg): State<AdminConfig>) -> Response {
    match cfg
        .handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => admin_error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_wire().message),
    }
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
        Ok((count, tool_list)) => {
            Json(json!({ "ok": true, "name": name, "tools": count, "tool_list": tool_list }))
                .into_response()
        }
        Err(e) => Json(json!({ "ok": false, "name": name, "error": e })).into_response(),
    }
}

/// 供应商真搜索测试:POST /admin/mcp/search-test/{name}
/// 把 query + provider_id 转发给插件的 web_search_test(跑一次该家的真实搜索
/// 并返回真结果),并把 structuredContent 原样回给前端。
pub async fn mcp_search_test(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some(hub) = cfg.hub.clone() else {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "服务器未启用 MCP 接线(--mcp-config)",
        );
    };
    let provider_id = body
        .get("provider_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query = body
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if provider_id.is_empty() || query.is_empty() {
        return Json(json!({
            "success": false,
            "error": "provider_id 与 query 均不能为空"
        }))
        .into_response();
    }
    let limit = body.get("limit").and_then(Value::as_i64).unwrap_or(5);
    let params = json!({ "provider_id": provider_id, "query": query, "limit": limit });
    match hub.raw_request(&name, "web_search_test", params).await {
        Ok(resp) => {
            // 插件返回的是 {content:[...], structuredContent:{...}} 的 JSON-RPC result
            let sc = resp.get("structuredContent").cloned().unwrap_or(resp);
            Json(json!({ "ok": true, "name": name, "result": sc })).into_response()
        }
        Err(e) => Json(json!({ "ok": false, "name": name, "error": e })).into_response(),
    }
}

/// 读插件月度用量:GET /admin/mcp/usage/{name}
/// 调 web_usage 拿 {month, providers:{id:used}},回给前端画进度条。
pub async fn mcp_usage(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let Some(hub) = cfg.hub.clone() else {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "服务器未启用 MCP 接线(--mcp-config)",
        );
    };
    match hub.raw_request(&name, "web_usage", json!({})).await {
        Ok(resp) => {
            let usage = resp.get("structuredContent").cloned().unwrap_or(resp);
            Json(json!({ "ok": true, "name": name, "usage": usage })).into_response()
        }
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
            Ok((count, tool_list)) => status
                .push(json!({"name": name, "ok": true, "tools": count, "tool_list": tool_list})),
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
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "非法配置名称(仅限英数字/下划线/连字符)",
        );
    }
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
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "非法配置名称(仅限英数字/下划线/连字符)",
        );
    }
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
    if let Err(e) = bm_persist::atomic_write(&file, text.as_bytes()) {
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
        "note": "注意: allowed_capabilities 仅作为提示词注入与客户端提示面数据,不构成内核级权限控制(以 Broker 权限判定为唯一权威)"
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
    if meta.len() > cfg.limits.get().fs_preview_max_bytes {
        return admin_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!(
                "文件超过预览上限({}KB > {}KB)",
                meta.len() / 1024,
                cfg.limits.get().fs_preview_max_bytes / 1024
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

/// W7 反馈:目录树右键菜单——重命名(路径防护同 X-01;新名校验)。
pub async fn fs_rename(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let Some(path) = body["path"].as_str() else {
        return admin_error(StatusCode::BAD_REQUEST, "path 必须是字符串");
    };
    let target = match safe_resolve(&cfg.workspace_root, path) {
        Ok(t) => t,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    let Some(new_name) = body["name"].as_str().map(|s| s.trim()) else {
        return admin_error(StatusCode::BAD_REQUEST, "name 必须是字符串");
    };
    if new_name.is_empty() || new_name.len() > 200 || new_name == "." || new_name == ".." {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "name 必须是非空文件名(≤200 字符,不含路径分隔)",
        );
    }
    if new_name.contains(['/', '\\']) {
        return admin_error(StatusCode::BAD_REQUEST, "name 不允许包含路径分隔符");
    }
    let Some(parent) = target.parent() else {
        return admin_error(StatusCode::BAD_REQUEST, "目标无父目录");
    };
    let new_path = parent.join(new_name);
    if new_path.exists() {
        return admin_error(StatusCode::CONFLICT, format!("「{new_name}」已存在"));
    }
    match std::fs::rename(&target, &new_path) {
        Ok(_) => Json(json!({ "ok": true })).into_response(),
        Err(e) => admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("重命名失败: {e}"),
        ),
    }
}

/// W7 反馈:目录树右键菜单——下载(单文件原样)与打包下载(文件夹 zip)。
/// 仅工作区内(safe_resolve 防逃逸);总量守门 256MB / 5000 条目。
pub async fn fs_download(
    State(cfg): State<AdminConfig>,
    Query(p): Query<FsPathParams>,
) -> Response {
    let target = match safe_resolve(&cfg.workspace_root, &p.path) {
        Ok(t) => t,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, e),
    };
    let Ok(meta) = std::fs::metadata(&target) else {
        return admin_error(StatusCode::NOT_FOUND, "路径不存在");
    };
    let download_name = if meta.is_dir() {
        target
            .file_name()
            .map(|n| format!("{}.zip", n.to_string_lossy()))
            .unwrap_or_else(|| "workspace.zip".to_string())
    } else {
        target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "download".to_string())
    };
    let content_type = if meta.is_dir() {
        "application/zip"
    } else {
        "application/octet-stream"
    };
    let bytes = if meta.is_dir() {
        match zip_dir(
            &target,
            cfg.limits.get().fs_download_max_entries,
            cfg.limits.get().fs_download_max_bytes,
        ) {
            Ok(b) => b,
            Err(e) => {
                return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("打包失败: {e}"));
            }
        }
    } else {
        if meta.len() > cfg.limits.get().fs_download_max_bytes {
            return admin_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!(
                    "文件超过下载上限({}MB)",
                    cfg.limits.get().fs_download_max_bytes / 1024 / 1024
                ),
            );
        }
        match std::fs::read(&target) {
            Ok(b) => b,
            Err(e) => {
                return admin_error(StatusCode::INTERNAL_SERVER_ERROR, format!("读取失败: {e}"));
            }
        }
    };
    // Content-Disposition:ASCII 兜底 + RFC 5987 UTF-8(中文文件名)
    let ascii_name: String = download_name
        .chars()
        .map(|c| if c.is_ascii() { c } else { '_' })
        .collect();
    let mut resp = (StatusCode::OK, bytes).into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&format!(
        "attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{}",
        utf8_percent_encode(&download_name)
    )) {
        resp.headers_mut()
            .insert(axum::http::header::CONTENT_DISPOSITION, v);
    }
    if let Ok(v) = axum::http::HeaderValue::from_str(content_type) {
        resp.headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, v);
    }
    resp
}

fn utf8_percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 2026-09-07 目录树批次:删除(工作区文件树右键,支持多选批量)。
/// 仅工作区内:逐条 safe_resolve 防逃逸(天然拒 `..`/绝对路径/符号链接);
/// 目录整棵递归删;永久删除不进回收站,防误删由前端确认弹窗承担。
pub async fn fs_delete(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let Some(paths) = body["paths"].as_array() else {
        return admin_error(StatusCode::BAD_REQUEST, "paths 必须是字符串数组");
    };
    if paths.is_empty() {
        return admin_error(StatusCode::BAD_REQUEST, "paths 不能为空");
    }
    if paths.len() > cfg.limits.get().fs_delete_batch_max {
        return admin_error(
            StatusCode::BAD_REQUEST,
            format!("单次最多删除 {} 项", cfg.limits.get().fs_delete_batch_max),
        );
    }
    let mut results = Vec::new();
    for p in paths {
        let Some(rel) = p.as_str().map(|s| s.trim()) else {
            results.push(json!({ "path": Value::Null, "ok": false, "error": "路径必须是字符串" }));
            continue;
        };
        if rel.is_empty() {
            results.push(json!({ "path": rel, "ok": false, "error": "拒绝删除工作区根" }));
            continue;
        }
        let target = match safe_resolve(&cfg.workspace_root, rel) {
            Ok(t) => t,
            Err(e) => {
                results.push(json!({ "path": rel, "ok": false, "error": e }));
                continue;
            }
        };
        let meta = match std::fs::symlink_metadata(&target) {
            Ok(m) => m,
            Err(_) => {
                results.push(json!({ "path": rel, "ok": false, "error": "路径不存在" }));
                continue;
            }
        };
        let r = if meta.is_dir() {
            std::fs::remove_dir_all(&target)
        } else {
            std::fs::remove_file(&target)
        };
        match r {
            Ok(_) => results.push(json!({ "path": rel, "ok": true })),
            Err(e) => results.push(json!({
                "path": rel, "ok": false, "error": format!("删除失败: {e}")
            })),
        }
    }
    let deleted = results
        .iter()
        .filter(|r| r["ok"].as_bool() == Some(true))
        .count();
    Json(json!({
        "ok": deleted == results.len(),
        "deleted": deleted,
        "results": results,
    }))
    .into_response()
}

// ---- handler:任意目录浏览(只读;设置页工作目录选择器专用)----------------
// 与 /fs/list 的工作区沙箱浏览互补:「添加工作目录」的路径选择器需要覆盖
// 全盘任意绝对路径。守门:只列目录、只报名字,零文件内容零大小;上限 1000 条。

/// GET /admin/fs/browse?path=<绝对路径;空 = 根视图(Windows 盘符 / Unix /)>
pub async fn fs_browse(State(cfg): State<AdminConfig>, Query(p): Query<FsPathParams>) -> Response {
    let raw = p.path.trim().to_string();
    if raw.is_empty() {
        return Json(json!({
            "path": "",
            "parent": Value::Null,
            "entries": browse_roots(),
            "truncated": false,
        }))
        .into_response();
    }
    let path = std::path::Path::new(&raw);
    if !path.exists() {
        return admin_error(StatusCode::BAD_REQUEST, format!("路径不存在: {raw}"));
    }
    let canon = match std::fs::canonicalize(path) {
        Ok(c) => c,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, format!("路径解析失败: {e}")),
    };
    if !canon.is_dir() {
        return admin_error(StatusCode::BAD_REQUEST, "目标不是目录");
    }
    let pretty =
        |p: &std::path::Path| crate::workspace_admin::pretty_normalized(p.display().to_string());
    let mut entries = Vec::new();
    let mut truncated = false;
    let mut unreadable = false;
    match std::fs::read_dir(&canon) {
        Ok(rd) => {
            for entry in rd.flatten() {
                if entries.len() >= cfg.limits.get().fs_browse_max_entries {
                    truncated = true;
                    break;
                }
                // 只收目录:实体目录直收;符号链接/junction 跟随判一次
                // (选择器允许经由链接进入目录;文件一律不出现)
                let Ok(ft) = entry.file_type() else { continue };
                let is_dir = if ft.is_dir() {
                    true
                } else if ft.is_symlink() {
                    std::fs::metadata(entry.path())
                        .map(|m| m.is_dir())
                        .unwrap_or(false)
                } else {
                    false
                };
                if !is_dir {
                    continue;
                }
                entries.push(json!({
                    "name": entry.file_name().to_string_lossy(),
                    "path": pretty(&entry.path()),
                }));
            }
        }
        Err(_) => {
            // 无权限等读取失败:返回空列表而非 500,用户可回上级继续浏览
            unreadable = true;
        }
    }
    entries.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    Json(json!({
        "path": pretty(&canon),
        "parent": parent_json(&canon),
        "entries": entries,
        "truncated": truncated,
        "note": if unreadable { "目录不可读或为空" } else { "" },
    }))
    .into_response()
}

/// 根视图条目:Windows 枚举现存盘符;Unix 统一一条 /。
fn browse_roots() -> Vec<Value> {
    #[cfg(windows)]
    {
        (b'A'..=b'Z')
            .map(|c| format!("{}:\\", c as char))
            .filter(|d| std::path::Path::new(d).is_dir())
            .map(|d| json!({ "name": d, "path": d }))
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![json!({ "name": "/", "path": "/" })]
    }
}

/// 上级目录;盘符根/文件系统根无上级,返回 null。
fn parent_json(canon: &std::path::Path) -> Value {
    canon
        .parent()
        .map(|p| {
            let t = crate::workspace_admin::pretty_normalized(p.display().to_string());
            if t.is_empty() { Value::Null } else { json!(t) }
        })
        .unwrap_or(Value::Null)
}

/// 2026-09-07 目录树批次:新建目录(工作目录选择器「新建文件夹」配套)。
/// 与 /fs/browse 同一全盘信任域:browse 全盘只读浏览,本端点是其唯一配套
/// 写例外——只建空目录、单级、零内容;name 校验与 fs_rename 同规;门户墙保护。
pub async fn fs_mkdir(State(_cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let Some(parent) = body["parent"].as_str().map(|s| s.trim()) else {
        return admin_error(StatusCode::BAD_REQUEST, "parent 必须是字符串");
    };
    if parent.is_empty() {
        return admin_error(StatusCode::BAD_REQUEST, "请先进入某个盘符或目录再新建");
    }
    let Some(name) = body["name"].as_str().map(|s| s.trim()) else {
        return admin_error(StatusCode::BAD_REQUEST, "name 必须是字符串");
    };
    if name.is_empty() || name.len() > 200 || name == "." || name == ".." {
        return admin_error(
            StatusCode::BAD_REQUEST,
            "name 必须是非空目录名(≤200 字符,不含路径分隔)",
        );
    }
    if name.contains(['/', '\\']) {
        return admin_error(StatusCode::BAD_REQUEST, "name 不允许包含路径分隔符");
    }
    let parent_path = std::path::Path::new(parent);
    if !parent_path.exists() {
        return admin_error(StatusCode::BAD_REQUEST, format!("父目录不存在: {parent}"));
    }
    let canon_parent = match std::fs::canonicalize(parent_path) {
        Ok(c) => c,
        Err(e) => return admin_error(StatusCode::BAD_REQUEST, format!("父目录解析失败: {e}")),
    };
    if !canon_parent.is_dir() {
        return admin_error(StatusCode::BAD_REQUEST, "父路径不是目录");
    }
    let target = canon_parent.join(name);
    if target.exists() {
        return admin_error(StatusCode::CONFLICT, format!("「{name}」已存在"));
    }
    match std::fs::create_dir(&target) {
        Ok(_) => Json(json!({
            "ok": true,
            "path": crate::workspace_admin::pretty_normalized(target.display().to_string()),
        }))
        .into_response(),
        Err(e) => admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("新建目录失败: {e}"),
        ),
    }
}

/// 递归打包目录为 zip(内存;守门条目/总量上限走 limits,W10)。
fn zip_dir(dir: &std::path::Path, max_entries: usize, max_bytes: u64) -> Result<Vec<u8>, String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut count = 0usize;
        let mut total = 0u64;
        #[allow(clippy::too_many_arguments)] // zip 递归走签名面,钳制值不私挂全局
        fn walk(
            zip: &mut zip::ZipWriter<&mut std::io::Cursor<Vec<u8>>>,
            options: &zip::write::FileOptions,
            prefix: &str,
            base: &std::path::Path,
            count: &mut usize,
            total: &mut u64,
            max_entries: usize,
            max_bytes: u64,
        ) -> Result<(), String> {
            for entry in
                std::fs::read_dir(base).map_err(|e| format!("read_dir {}: {e}", base.display()))?
            {
                let entry = entry.map_err(|e| format!("{e}"))?;
                let file_type = entry.file_type().map_err(|e| format!("{e}"))?;
                if file_type.is_symlink() {
                    // 安全防护:工作区打包下载不跟随符号链接,防止链接到工作区外敏感文件
                    continue;
                }
                let path = entry.path();
                let rel = if prefix.is_empty() {
                    entry.file_name().to_string_lossy().to_string()
                } else {
                    format!("{prefix}/{}", entry.file_name().to_string_lossy())
                };
                if file_type.is_dir() {
                    zip.add_directory(rel.clone(), *options)
                        .map_err(|e| format!("{e}"))?;
                    walk(
                        zip,
                        options,
                        &rel,
                        &path,
                        count,
                        total,
                        max_entries,
                        max_bytes,
                    )?;
                } else {
                    *count += 1;
                    if *count > max_entries {
                        return Err(format!("条目超过 {max_entries},拒绝打包"));
                    }
                    let data = std::fs::read(&path).map_err(|e| format!("read {rel}: {e}"))?;
                    *total += data.len() as u64;
                    if *total > max_bytes {
                        return Err("总量超过下载上限,拒绝打包".into());
                    }
                    zip.start_file(rel.clone(), *options)
                        .map_err(|e| format!("{e}"))?;
                    std::io::Write::write_all(zip, &data).map_err(|e| format!("{e}"))?;
                }
            }
            Ok(())
        }
        walk(
            &mut zip,
            &options,
            "",
            dir,
            &mut count,
            &mut total,
            max_entries,
            max_bytes,
        )?;
        zip.finish().map_err(|e| format!("{e}"))?;
    }
    Ok(buf.into_inner())
}

// ---- 错误形状 ------------------------------------------------------------

/// 管理面统一错误形状(壳子私用 REST 惯例,非 Wire 信封)。
pub(crate) fn admin_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": { "message": message.into() } })),
    )
        .into_response()
}

// ---- handler:MCP 插件目录扫描与批准接入(两段式,2026-09-02 用户批准)----
//
// 目录约定:MCP 插件(可执行文件)放 `<mcp.json 同级>/mcp/`;官方随包插件
// 位于安装目录 `plugins/`(exe 同级,v0.0.4 起随包发布;升级链路把它装到
// exe 同级而非数据目录——2026-09-03 修复:扫描/批准同样认该目录,否则
// 「随包」对在线升级用户不可见)。两处候选均以 `--self-describe` 参数打印
// 声明 JSON 识别(识别过程会运行候选文件——数据目录是用户手动放入=安装
// 意图,随包目录随官方主程序一同安装=同等安装意图;正式激活仍以「批准
// 接入」落盘 mcp.json 为准,显式批准=安装,ADR-0005/0006/0017)。
// 同名候选以数据目录(用户手动放置)优先。

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
    // ADR-0023 墓碑名单:候选若在册,前端提示「批准即恢复」
    let tombstones = read_tombstones(&cfg.data_dir);
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
            "tombstoned": tombstones.iter().any(|t| t == &name),
            "source": "data",
        }));
    }
    // 官方随包目录(exe 同级 plugins/):随包插件免手动拷贝即可被发现;
    // 同名候选以数据目录优先(用户手动放置覆盖官方包)。
    if let Some(bundled) = &cfg.bundled_plugins_dir
        && let Ok(entries) = std::fs::read_dir(bundled)
    {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_file() || !candidate_is_executable(&p) {
                continue;
            }
            let Some(decl) = self_describe(&p).await else {
                continue;
            };
            let name = decl["name"].as_str().unwrap_or_default().to_string();
            if name.is_empty() || candidates.iter().any(|c| c["name"] == json!(name)) {
                continue;
            }
            candidates.push(json!({
                "file": p.display().to_string(),
                "name": name,
                "title": decl.get("title").cloned().unwrap_or(json!("")),
                "description": decl.get("description").cloned().unwrap_or(json!("")),
                "registered": registered.iter().any(|r| r == &name),
                "tombstoned": tombstones.iter().any(|t| t == &name),
                "source": "bundled",
            }));
        }
    }
    Json(json!({
        "ok": true,
        "dir": dir.display().to_string(),
        "bundled_dir": cfg
            .bundled_plugins_dir
            .as_ref()
            .map(|d| json!(d.display().to_string()))
            .unwrap_or(Value::Null),
        "candidates": candidates,
        "note": "扫描会以 --self-describe 运行候选目录内可执行文件(数据目录 mcp/ 与官方随包 plugins/);批准后才落盘 mcp.json",
    }))
    .into_response()
}

/// POST /admin/mcp/approve:批准候选接入。body {"name": "..."}。
/// 落盘两处:mcp.json 条目(command=候选路径,args 用声明模板替换
/// {config_file} 为数据目录配置路径)+ manifests/<name>.manifest.json
/// (设置页配置表单的声明来源);随后自动热重载上线(ADR-0023,热重载
/// 按钮保留作手动保险),并清除该名墓碑(被卸载/删除过的官方插件由此恢复)。
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
    // 在候选目录(数据目录 mcp/ 优先,官方随包 plugins/ 次之)内找到声明
    // name 匹配的候选(目录限定,防路径逃逸)
    let mut target: Option<PathBuf> = None;
    let mut decl: Option<Value> = None;
    let mut search_dirs: Vec<PathBuf> = vec![dir];
    if let Some(bundled) = &cfg.bundled_plugins_dir {
        search_dirs.push(bundled.clone());
    }
    for search_dir in &search_dirs {
        if let Ok(entries) = std::fs::read_dir(search_dir) {
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
        if target.is_some() {
            break;
        }
    }
    let (Some(file), Some(decl)) = (target, decl) else {
        return admin_error(
            StatusCode::NOT_FOUND,
            format!("候选目录中没有自声明 name={want_name} 的候选"),
        );
    };

    // 条目构造+manifest 双写(与启动播种同一 helper,形状天然一致)
    let entry = match build_candidate_entry(&path, &file, &decl, &want_name) {
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
    write_plugin_manifest(&path, &want_name, &decl);

    // ADR-0023:显式批准=安装意图最高级——清墓碑(被卸载/删除过的官方
    // 插件由此恢复)+ 立即热重载上线;失败不回滚落盘,可手动重试
    remove_tombstone(&cfg.data_dir, &want_name);
    let (reload_ok, reload_payload) = match run_mcp_sync(&cfg).await {
        Ok(o) => {
            let tools = o
                .note_loaded
                .iter()
                .find(|s| s["name"].as_str() == Some(want_name.as_str()))
                .and_then(|s| s.get("tools").cloned())
                .unwrap_or(Value::Null);
            (
                true,
                json!({ "ok": true, "tools": tools, "failed": o.failed }),
            )
        }
        Err((_, m)) => (false, json!({ "ok": false, "skipped": m })),
    };
    let note = if reload_ok {
        "已批准并自动上线;「重载 MCP」按钮保留作手动保险".to_string()
    } else {
        format!(
            "已落盘,但自动上线未完成:{};可点「重载 MCP」重试",
            reload_payload["skipped"].as_str().unwrap_or("未知")
        )
    };
    Json(json!({
        "ok": true,
        "entry": entry,
        "reload": reload_payload,
        "note": note,
    }))
    .into_response()
}

// ---- handler:运行日志查看(2026-09-02 用户要求「设置里接入日志」)--------

/// 从文件尾部读最多 `max_bytes` 字节,返回最后 `n` 行(首行可能被截断则丢弃)。
fn tail_lines(path: &Path, max_bytes: u64, n: usize) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return vec![];
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        return vec![];
    }
    let mut lines: Vec<&str> = buf.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // 截断边界上的半行不可信
    }
    let skip = lines.len().saturating_sub(n);
    lines.into_iter().skip(skip).map(String::from).collect()
}

/// GET /admin/logs:数据目录三份日志的尾部直读(各取最后 200 行,单文件
/// 最多回读 512KB)——execution-log.jsonl(回合/工具调用明细)、
/// events.jsonl(事件流,含 capability.invoked 的 intent/result/error)与
/// context-log.jsonl(W5 上下文快照原文,调试用),供诊断「工具调用卡死」
/// 一类运行期问题。
pub async fn logs_tail(State(cfg): State<AdminConfig>) -> Response {
    let dir = cfg.data_dir;
    Json(json!({
        "ok": true,
        "exec": tail_lines(&dir.join("execution-log.jsonl"), 512 * 1024, 200),
        "events": tail_lines(&dir.join("events.jsonl"), 512 * 1024, 200),
        "context": tail_lines(&dir.join("context-log.jsonl"), 512 * 1024, 200),
    }))
    .into_response()
}

/// GET /admin/limits(W10/ADR-0024):逐键返回 当前值/默认值/区间/来源。
pub async fn limits_get(State(cfg): State<AdminConfig>) -> Response {
    let limits = cfg.limits.get();
    let defaults = bm_core::limits::Limits::default();
    let source_of = |k: &str| cfg.limits_sources.lock().expect("锁未中毒").source_of(k);
    let all: Value = serde_json::to_value(&limits).unwrap_or(json!({}));
    let dfl: Value = serde_json::to_value(&defaults).unwrap_or(json!({}));
    let keys: Vec<Value> = bm_core::limits::KEY_META
        .iter()
        .map(|m| {
            json!({
                "key": m.key,
                "group": m.group,
                "label": m.label,
                "min": m.min,
                "max": m.max,
                "editable": m.editable,
                "value": all.get(m.key).cloned().unwrap_or(Value::Null),
                "default": dfl.get(m.key).cloned().unwrap_or(Value::Null),
                "source": source_of(m.key),
            })
        })
        .collect();
    Json(json!({ "ok": true, "keys": keys })).into_response()
}

/// PUT /admin/limits(W10/ADR-0024):全量快照写——body {values: {key: value}},
/// 仅接受已登记键;钳制→原子写 config/limits.json→更新 Cell(热生效)。
pub async fn limits_put(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let incoming = body
        .get("values")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let editable: std::collections::HashSet<&str> = bm_core::limits::KEY_META
        .iter()
        .filter(|m| m.editable)
        .map(|m| m.key)
        .collect();
    // 以默认值为底、只叠加登记键(整数化浮点,避免 u64 反序列化拒 5.0)
    let mut merged = match serde_json::to_value(bm_core::limits::Limits::default()) {
        Ok(Value::Object(m)) => m,
        _ => Default::default(),
    };
    for (k, v) in incoming {
        if !editable.contains(k.as_str()) {
            continue;
        }
        let v = match v.as_f64() {
            Some(f) if f.fract() == 0.0 => json!(f as i64),
            _ => v,
        };
        merged.insert(k, v);
    }
    let new_limits = bm_core::limits::Limits::from_file_value(&Value::Object(merged.clone()));
    // 原子写(与 fs.write/配置面同款语义:临时文件+rename,崩溃不留半截)
    let path = cfg.data_dir.join("config").join("limits.json");
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("建配置目录失败: {e}"),
        );
    }
    let pretty = serde_json::to_string_pretty(&new_limits.to_file_value()).unwrap_or_default();
    if let Err(e) = bm_persist::atomic_write(&path, pretty.as_bytes()) {
        return admin_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("写 limits.json 失败: {e}"),
        );
    }
    cfg.limits.set(new_limits);
    if let Ok(mut src) = cfg.limits_sources.lock() {
        src.file_raw = Some(Value::Object(merged));
    }
    Json(json!({
        "ok": true,
        "note": "已保存并热生效:命令/工具下一条、回合下一回合、流式下一条起生效",
    }))
    .into_response()
}

/// GET /admin/jobs(W10/ADR-0025):后台作业列表(新→旧)。
pub async fn jobs_list(State(cfg): State<AdminConfig>) -> Response {
    let jobs = cfg.jobs.as_ref().map(|j| j.list()).unwrap_or_default();
    Json(json!({ "ok": true, "jobs": jobs })).into_response()
}

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
/// 个人单机数据量下用整文件行级扫描(context-log.jsonl 任一行含 q 即命中,
/// 大小写不敏感);数据量上来再换 FTS5 索引(规格 W9 二期备注)。
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
        return admin_error(StatusCode::BAD_REQUEST, "缺少 q");
    }
    let path = cfg.data_dir.join("context-log.jsonl");
    let needle = q.to_lowercase();
    let mut hits: Vec<Value> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        for line in text.lines().rev() {
            if line.to_lowercase().contains(&needle)
                && let Ok(v) = serde_json::from_str::<Value>(line)
            {
                hits.push(v);
                if hits.len() >= limit {
                    break;
                }
            }
        }
    }
    Json(json!({ "ok": true, "q": q, "hits": hits, "total": hits.len() })).into_response()
}

/// DELETE /admin/sessions/{session_id}:会话删除(2026-09-06 A+B)。
/// 经核心单写者执行:墓碑 + operations 原文擦除 + context-log 流式过滤;
/// events.jsonl 仅元数据不动(A4/审计口径)。不可恢复。
async fn session_delete(
    State(cfg): State<AdminConfig>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Response {
    match cfg
        .handle
        .session_delete(
            UlidIdGen.next_id("req"),
            bm_contract::wire::SessionDeleteParams {
                session_id: match bm_contract::ids::BmId::parse(&session_id) {
                    Ok(id) => id,
                    Err(_) => return admin_error(StatusCode::BAD_REQUEST, "非法会话 id"),
                },
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
        Err(e) => admin_error(StatusCode::BAD_REQUEST, e.to_wire().message),
    }
}

/// GET /admin/sessions/{session_id}/messages?limit=&skip=:
/// 会话历史回放(2026-09-06;同日二改:分页+流式,防长会话一口气载入卡
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
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .clamp(1, cfg.limits.get().context_search_max_limit);
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
fn read_context_tail(path: &Path, max_bytes: u64, limit: usize) -> Vec<Value> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return vec![];
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        return vec![];
    }
    let mut lines: Vec<&str> = buf.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // 截断边界上的半行不可信
    }
    let mut steps: Vec<Value> = lines
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if steps.len() > limit {
        steps = steps.split_off(steps.len() - limit);
    }
    steps
}

/// 管理面子路由(挂载于 /admin;公开 = W1 同款已登记欠账)。
pub fn admin_routes(cfg: AdminConfig) -> axum::Router {
    use axum::routing::{delete, get, post, put};
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
        .route("/mcp/search-test/{name}", post(mcp_search_test))
        .route("/mcp/usage/{name}", get(mcp_usage))
        .route("/mcp/status", get(mcp_status))
        .route(
            "/mcp-config/{name}",
            get(mcp_config_get).put(mcp_config_set),
        )
        .route("/mcp/{name}", put(mcp_update).delete(mcp_delete))
        // ADR-0023:卸载并物理删除插件文件(警告栏确认后调用)
        .route("/mcp/{name}/purge", post(mcp_purge))
        .route("/capabilities", get(capabilities_list))
        .route("/roles", get(roles_get).post(roles_set).put(roles_set))
        .route("/roles/{id}", put(roles_set).delete(roles_delete))
        .route("/roles/active/{id}", put(roles_set_active))
        .route("/approvals", get(approvals_list))
        .route("/approvals/{id}/respond", post(approval_respond))
        .route("/skills", get(skills_get).post(skills_set))
        .route("/skills/{id}", delete(skills_delete))
        .route("/logs", get(logs_tail))
        .route("/context", get(context_tail))
        .route("/context/search", get(context_search))
        // W10(ADR-0024/0025):限制配置面 + 后台作业列表
        .route("/limits", get(limits_get).put(limits_put))
        .route("/jobs", get(jobs_list))
        // 会话历史回放(2026-09-06):切会话/刷新后前端按此拉历史消息
        .route("/sessions/{session_id}/messages", get(session_messages))
        // 会话删除(2026-09-06 A+B):墓碑+原文擦除,经核心单写者执行
        .route(
            "/sessions/{session_id}",
            axum::routing::delete(session_delete),
        )
        .route("/fs/list", get(fs_list))
        // 工作目录选择器:全盘只读目录浏览(仅目录名,零内容)
        .route("/fs/browse", get(fs_browse))
        .route("/fs/file", get(fs_file))
        // W7 目录树右键菜单:重命名 / 下载(文件)与打包下载(文件夹 zip)
        .route("/fs/rename", post(fs_rename))
        .route("/fs/download", get(fs_download))
        // 2026-09-07 目录树批次:新建目录(选择器全盘)与删除(工作区多选)
        .route("/fs/mkdir", post(fs_mkdir))
        .route("/fs/delete", post(fs_delete))
        // W7 关于与在线升级(apply 仅回环;铁规矩:绝不由此触发发布)
        .route("/about", get(crate::about::about))
        .route("/about/check-update", post(crate::about::check_update))
        .route("/about/apply-update", post(crate::about::apply_update))
        // W8 常规:工作区注册表 CRUD/探测 + 运行环境探针(ADR-0018)
        .route(
            "/workspaces",
            get(crate::workspace_admin::workspaces_list)
                .post(crate::workspace_admin::workspaces_create),
        )
        .route(
            "/workspaces/{id}",
            axum::routing::put(crate::workspace_admin::workspaces_update)
                .delete(crate::workspace_admin::workspaces_delete),
        )
        .route(
            "/workspaces/{id}/check",
            post(crate::workspace_admin::workspaces_check),
        )
        .route("/runtime/env", get(crate::workspace_admin::runtime_env))
        .with_state((*cfg).clone())
}

// ---- handler:MCP 热装载(支持新增、修改与删除免重启)---------------

/// F-07 同款收口:热装载装配段(reload/批准上线/卸载下线/purge 共用)。
/// 全量同步 mcp.json 与已装载名单:新增 spawn+握手+注册、修改先拔后插、
/// 摘除 disconnect+unregister(ChildKill 兜底杀子进程)。
struct CoreRegistrar {
    handle: bm_core::runtime::RuntimeHandle,
}

#[async_trait::async_trait]
impl bm_providers::mcp::supervisor::CapabilityRegistrar for CoreRegistrar {
    async fn register(
        &self,
        entries: Vec<(
            bm_contract::capability::CapabilityManifest,
            Arc<dyn bm_core::registry::CapabilityProvider>,
        )>,
    ) -> Result<(), String> {
        self.handle
            .capabilities_register(entries)
            .await
            .map(|_| ())
            .map_err(|e| format!("{e}"))
    }
    async fn unregister(&self, names: Vec<String>) -> Result<(), String> {
        // 与既有行为一致:注销经核心命令;错误走 failed 通道
        self.handle
            .capabilities_unregister(names)
            .await
            .map(|_| ())
            .map_err(|e| format!("{e}"))
    }
}

/// 全量热同步;完成后写回 AdminConfig.mcp_servers 快照。测试态/未接线
/// (无 mcp_config 或 hub)返回 Err,调用方自行降级。
async fn run_mcp_sync(
    cfg: &AdminConfig,
) -> Result<bm_providers::mcp::supervisor::SyncOutcome, (StatusCode, String)> {
    use bm_providers::mcp::supervisor::sync_from_config;
    let path = cfg.mcp_config.clone().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "服务器未启用 MCP 配置文件(--mcp-config)".to_string(),
        )
    })?;
    let hub = cfg.hub.clone().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "启动时未完成 MCP 接线(检查启动日志的 MCP server 装载行)".to_string(),
        )
    })?;
    let secrets = cfg.secrets.clone().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Secret Store 未就绪".to_string(),
        )
    })?;
    let loaded_names: Vec<String> = cfg
        .mcp_servers
        .read()
        .map(|g| {
            g.iter()
                .filter_map(|s| s["name"].as_str().map(|n| n.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let outcome = sync_from_config(
        &hub,
        &path,
        secrets,
        loaded_names,
        &CoreRegistrar {
            handle: cfg.handle.clone(),
        },
        &cfg.limits,
    )
    .await;
    if let Ok(mut g) = cfg.mcp_servers.write() {
        *g = outcome.note_loaded.clone();
    }
    Ok(outcome)
}

// ---- MCP 插件生命周期:墓碑 + 来源推断 + 候选条目构造(ADR-0023)----

/// 墓碑文件(<data>/config/mcp-removed.json,私有管理文件不入合同):
/// 用户明确不要的插件名册。官方随包默认安装(启动播种)跳过墓碑名单;
/// 显式批准接入即除名(恢复安装的唯一正路)。
fn tombstone_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config").join("mcp-removed.json")
}

fn read_tombstones(data_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(tombstone_path(data_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["removed"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e["name"].as_str().map(String::from))
        .collect()
}

fn write_tombstones(data_dir: &Path, list: Vec<Value>) -> Result<(), String> {
    let dir = data_dir.join("config");
    std::fs::create_dir_all(&dir).map_err(|e| format!("config 目录创建失败: {e}"))?;
    let text = serde_json::to_string_pretty(&json!({
        "removed": list,
        "note": "ADR-0023 墓碑:官方随包默认安装跳过本名单;显式批准接入即除名",
    }))
    .map_err(|e| format!("序列化失败: {e}"))?;
    bm_persist::atomic_write(&tombstone_path(data_dir), text.as_bytes())
        .map_err(|e| format!("墓碑写盘失败: {e}"))
}

fn upsert_tombstone(data_dir: &Path, name: &str) -> Result<(), String> {
    let existing = std::fs::read_to_string(tombstone_path(data_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["removed"].as_array().cloned())
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut list: Vec<Value> = existing
        .into_iter()
        .filter(|e| e["name"].as_str() != Some(name))
        .collect();
    list.push(json!({ "name": name, "removed_at": now }));
    write_tombstones(data_dir, list)
}

fn remove_tombstone(data_dir: &Path, name: &str) {
    let Some(v) = std::fs::read_to_string(tombstone_path(data_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
    else {
        return;
    };
    let Some(arr) = v["removed"].as_array() else {
        return;
    };
    let filtered: Vec<Value> = arr
        .iter()
        .filter(|e| e["name"].as_str() != Some(name))
        .cloned()
        .collect();
    if filtered.len() == arr.len() {
        return;
    }
    let _ = write_tombstones(data_dir, filtered);
}

/// 官方随包清单(plugins/.official.json,release 打包写入);缺失=未知,
/// 弃用标记优雅降级(旧版安装/本地开发)。
fn official_plugin_list(cfg: &AdminConfig) -> Option<Vec<String>> {
    let bundled = cfg.bundled_plugins_dir.as_ref()?;
    let text = std::fs::read_to_string(bundled.join(".official.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    Some(
        v["plugins"]
            .as_array()?
            .iter()
            .filter_map(|p| p.as_str().map(String::from))
            .collect(),
    )
}

/// 插件来源推断(合同不动,mcp.json 无 source 字段):command 落在
/// 官方随包目录=bundled;落在 mcp.json 同级 mcp/=用户手动放置(data)。
fn server_origin(cfg: &AdminConfig, srv: &Value, mcp_json_path: &Path) -> &'static str {
    let command = srv["command"].as_str().unwrap_or("");
    if command.is_empty() {
        return "unknown";
    }
    let p = std::path::Path::new(command);
    if let Some(b) = &cfg.bundled_plugins_dir
        && p.starts_with(b)
    {
        return "bundled";
    }
    if let Some(parent) = mcp_json_path.parent()
        && p.starts_with(parent.join("mcp"))
    {
        return "data";
    }
    "unknown"
}

/// 批准/播种共用:候选声明 → 合规 mcp.json 条目(args 模板把 {config_file}
/// 替换为数据目录配置路径;过合同 schema)。
fn build_candidate_entry(
    mcp_json_path: &Path,
    file: &Path,
    decl: &Value,
    name: &str,
) -> Result<Value, String> {
    let config_dir = mcp_json_path
        .parent()
        .map(|d| d.join("config"))
        .unwrap_or_else(|| mcp_json_path.to_path_buf());
    let config_file = config_dir.join(format!("mcp-{name}.json"));
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
    let sha = bm_providers::mcp::sha256_file(&file.display().to_string())?;
    let entry_body = json!({
        "name": name,
        "command": file.display().to_string(),
        "sha256": sha,
        "args": args,
        "tool_timeout_ms": decl.pointer("/suggested_entry/tool_timeout_ms").cloned().unwrap_or(json!(30000)),
        "restart_limit": decl.pointer("/suggested_entry/restart_limit").cloned().unwrap_or(json!(3)),
    });
    validated_mcp_entry(&entry_body)
}

/// manifest 双写(批准/播种共用):manifests/<name>.manifest.json,
/// 设置页「配置」表单的声明来源。
fn write_plugin_manifest(mcp_json_path: &Path, name: &str, decl: &Value) {
    let manifest = json!({
        "name": name,
        "title": decl.get("title").cloned().unwrap_or(json!("")),
        "description": decl.get("description").cloned().unwrap_or(json!("")),
        "config_schema": decl.get("config_schema").cloned().unwrap_or(json!([])),
    });
    if let Some(mdir) = mcp_json_path.parent().map(|d| d.join("manifests")) {
        let _ = std::fs::create_dir_all(&mdir);
        if let Ok(text) = serde_json::to_string_pretty(&manifest) {
            let _ = bm_persist::atomic_write(
                &mdir.join(format!("{name}.manifest.json")),
                text.as_bytes(),
            );
        }
    }
}

/// ADR-0023:官方随包插件默认安装(启动播种)。扫描 bundled 目录候选,
/// 「mcp.json 未登记 且 不在墓碑」的按批准同款形状落盘 mcp.json + manifest,
/// 返回播种名册(启动日志用)。注意:识别 name 需运行 --self-describe,
/// 已登记插件每次启动约花亚秒级自述(量小可接受)。任何失败只记日志,
/// 绝不阻启动;mcp.json 损坏(读失败)时整体放弃播种防覆盖。
pub async fn seed_bundled_plugins(
    mcp_config_path: &Path,
    bundled_dir: &Path,
    data_dir: &Path,
) -> Vec<String> {
    let mut seeded = Vec::new();
    let Ok(mut servers) = read_mcp_servers(mcp_config_path) else {
        eprintln!("[mcp-seed] mcp.json 读取失败,跳过官方插件默认安装");
        return seeded;
    };
    // 已见名册 = 现有条目 + 本次已播种(防同声明名的多个文件重复落盘)
    let mut seen: Vec<String> = servers
        .iter()
        .filter_map(|s| s["name"].as_str().map(String::from))
        .collect();
    let tombstones = read_tombstones(data_dir);
    let Ok(entries) = std::fs::read_dir(bundled_dir) else {
        return seeded;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() || !candidate_is_executable(&p) {
            continue;
        }
        let Some(decl) = self_describe(&p).await else {
            continue;
        };
        let Some(name) = decl["name"].as_str().map(String::from) else {
            continue;
        };
        if name.is_empty()
            || seen.iter().any(|r| r == &name)
            || tombstones.iter().any(|t| t == &name)
        {
            continue;
        }
        match build_candidate_entry(mcp_config_path, &p, &decl, &name) {
            Ok(entry) => {
                write_plugin_manifest(mcp_config_path, &name, &decl);
                servers.push(entry);
                seen.push(name.clone());
                seeded.push(name);
            }
            Err(e) => eprintln!("[mcp-seed] 候选 {} 条目构造失败: {e}", p.display()),
        }
    }
    if !seeded.is_empty() {
        match write_mcp_servers(mcp_config_path, &servers) {
            Ok(_) => eprintln!("[mcp-seed] 官方随包插件默认安装: {}", seeded.join(", ")),
            Err(e) => {
                eprintln!("[mcp-seed] mcp.json 写盘失败,播种放弃: {e}");
                return Vec::new();
            }
        }
    }
    seeded
}

/// 重载 MCP 配置:读 mcp.json 全量,与已装载名单对比:
/// - 已移除的 server:从 hub 摘除路由、发送 shutdown 通知,从 Registry/Persist 摘除能力
/// - 修改/保留的 server (如配置变更):先热拔旧 server,再用新配置重新握手连接并更新能力
/// - 新增的 server:spawn+握手+运行期注册
///
/// 装载完成后刷新 AdminConfig.mcp_servers 快照。
pub async fn mcp_reload(State(cfg): State<AdminConfig>) -> Response {
    match run_mcp_sync(&cfg).await {
        Ok(outcome) => Json(json!({
            "ok": outcome.failed.is_empty(),
            "registered": outcome.registered,
            "updated": outcome.updated,
            "uninstalled": outcome.uninstalled,
            "failed": outcome.failed,
            "note": "MCP 服务已完成热重载(支持新增、修改与卸载免重启)",
        }))
        .into_response(),
        Err((s, m)) => admin_error(s, m),
    }
}
