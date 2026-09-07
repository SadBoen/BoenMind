//! MCP 接入配置管理:配置 CRUD(落盘+热重载)/探活/自声明配置/插件清单/
//! 候选扫描与批准接入(两段式)/墓碑与来源推断(ADR-0023)/启动播种/
//! 全量热同步。

use super::{AdminConfig, admin_error};
use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use bm_providers::mcp::supervisor::read_mcp_servers;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

// read_mcp_servers 复用 bm_providers::mcp::supervisor 同名实现(2026-09-07
// 复核批:此前两处逐字重复,启动装载与管理面读取口径有漂移风险)。

fn write_mcp_servers(path: &Path, servers: &[Value]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("配置目录创建失败: {e}"))?;
    }
    // P2(2026-09-07 架构评审):CRLF 收口 config_store::crlf 单一实现。
    let text = crate::config_store::crlf(
        serde_json::to_string_pretty(servers).map_err(|_| "序列化失败".to_string())?,
    );
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
