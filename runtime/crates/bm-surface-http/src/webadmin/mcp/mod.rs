//! MCP 接入配置管理:配置 CRUD(落盘+热重载)/探活/自声明配置/插件清单/
//! 候选扫描与批准接入(两段式)/墓碑与来源推断(ADR-0023)/启动播种/
//! 全量热同步。
//!
//! ):候选扫描与批准 →
//! [`scan`],墓碑/来源/条目构造/播种 → [`lifecycle`];本文件保留配置 CRUD、
//! 探活、自声明配置、能力清单与热同步。公共路径经 `pub use` 不变。

mod lifecycle;
mod scan;

use super::{
    AdminConfig, admin_error, bad_request, conflict, internal, not_found, respond_or_fail,
};
use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use bm_core::ports::mcp_admin::read_mcp_servers;
use lifecycle::{official_plugin_list, server_origin, upsert_tombstone};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// 公共路径不变:路由装配(`webadmin::router`)与启动播种按原符号引用。
pub use lifecycle::seed_bundled_plugins;
pub use scan::{mcp_approve, mcp_candidates};

// ---- handler:MCP 配置管理(落盘重启生效)--------------------------------

/// 管理端点必备:mcp.json 路径(未启用 = 400 直回)。
#[allow(clippy::result_large_err)] // Err 即响应体(冷路径),不值得装箱
fn mcp_file_or_error(cfg: &AdminConfig) -> Result<PathBuf, Response> {
    cfg.mcp_config
        .clone()
        .ok_or_else(|| bad_request("服务器未启用 MCP 配置文件(--mcp-config),无法管理"))
}

// read_mcp_servers 复用 bm_providers::mcp::supervisor 同名实现(
// 复核批:。

fn write_mcp_servers(path: &Path, servers: &[Value]) -> Result<(), String> {
 // P2():CRLF 收口 config_store::crlf 单一实现。
 // 注意:mcp.json 顶层即数组(与 providers/skills 的 {域: [...]} 包裹不同)。
    super::json_store::write_json_file(path, &Value::Array(servers.to_vec()), "MCP 配置写入失败")
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
    let path = respond_or_fail!(mcp_file_or_error(&cfg));
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
 // #71:列表面只读回显,走宽容原语(缺/坏 = 不展示)。
                    let manifest = manifests_dir.as_ref().and_then(|d| {
                        bm_core::json_store::read_json_lenient(
                            &d.join(format!("{name}.manifest.json")),
                        )
                    });
                    let config = config_dir
                        .as_ref()
                        .and_then(|d| {
                            bm_core::json_store::read_json_lenient(
                                &d.join(format!("mcp-{name}.json")),
                            )
                        })
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
        Err(e) => internal(e),
    }
}

pub async fn mcp_create(State(cfg): State<AdminConfig>, Json(body): Json<Value>) -> Response {
    let path = respond_or_fail!(mcp_file_or_error(&cfg));
    let entry = match validated_mcp_entry(&body) {
        Ok(e) => e,
        Err(e) => return bad_request(e),
    };
    let mut servers = match read_mcp_servers(&path) {
        Ok(s) => s,
        Err(e) => return internal(e),
    };
    let name = entry["name"].as_str().unwrap_or("").to_string();
    if servers.iter().any(|s| s["name"] == json!(name)) {
        return conflict(format!("MCP server '{name}' 已存在"));
    }
    servers.push(entry);
    respond_or_fail!(write_mcp_servers(&path, &servers), internal);
    Json(json!({ "ok": true, "note": "已落盘,点「重载 MCP」可免重启生效" })).into_response()
}

pub async fn mcp_update(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let path = respond_or_fail!(mcp_file_or_error(&cfg));
    let entry = respond_or_fail!(validated_mcp_entry(&body), bad_request);
    let mut servers = respond_or_fail!(read_mcp_servers(&path), internal);
    let Some(pos) = servers.iter().position(|s| s["name"] == json!(name)) else {
        return not_found(format!("MCP server '{name}' 不存在"));
    };
    servers[pos] = entry;
    respond_or_fail!(write_mcp_servers(&path, &servers), internal);
    Json(json!({ "ok": true, "note": "已落盘,点「重载 MCP」可免重启生效" })).into_response()
}

pub async fn mcp_delete(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    let path = respond_or_fail!(mcp_file_or_error(&cfg));
    let mut servers = respond_or_fail!(read_mcp_servers(&path), internal);
    let Some(pos) = servers.iter().position(|s| s["name"] == json!(name)) else {
        return not_found(format!("MCP server '{name}' 不存在"));
    };
    let origin = server_origin(&cfg, &servers[pos], &path);
    servers.remove(pos);
    respond_or_fail!(write_mcp_servers(&path, &servers), internal);
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
    let path = respond_or_fail!(mcp_file_or_error(&cfg));
    let mut servers = respond_or_fail!(read_mcp_servers(&path), internal);
    let Some(pos) = servers.iter().position(|s| s["name"] == json!(name)) else {
        return not_found(format!(
            "MCP server '{name}' 不存在(未登记的插件无从定位其文件)"
        ));
    };
    let exe = servers[pos]["command"].as_str().map(String::from);
    servers.remove(pos);
    respond_or_fail!(write_mcp_servers(&path, &servers), internal);
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
                    let now = crate::unix_now();
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
    let hub = respond_or_fail!(
        cfg.hub
            .clone()
            .ok_or_else(|| bad_request("服务器未启用 MCP 接线(--mcp-config)"))
    );
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
    let hub = respond_or_fail!(
        cfg.hub
            .clone()
            .ok_or_else(|| bad_request("服务器未启用 MCP 接线(--mcp-config)"))
    );
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
    let hub = respond_or_fail!(
        cfg.hub
            .clone()
            .ok_or_else(|| bad_request("服务器未启用 MCP 接线(--mcp-config)"))
    );
    match hub.raw_request(&name, "web_usage", json!({})).await {
        Ok(resp) => {
            let usage = resp.get("structuredContent").cloned().unwrap_or(resp);
            Json(json!({ "ok": true, "name": name, "usage": usage })).into_response()
        }
        Err(e) => Json(json!({ "ok": false, "name": name, "error": e })).into_response(),
    }
}

/// 读子进程 stderr 尾部:GET /admin/mcp/stderr/{name}?lines=N(issue #28)
/// 管道采集的环形缓冲回看(跨 respawn 带代标记);远程传输/未连接报错。
pub async fn mcp_stderr(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let hub = respond_or_fail!(
        cfg.hub
            .clone()
            .ok_or_else(|| bad_request("服务器未启用 MCP 接线(--mcp-config)"))
    );
    let lines = params
        .get("lines")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(1, 400);
    match hub.stderr_tail(&name, lines) {
        Ok(lines) => Json(json!({ "ok": true, "name": name, "lines": lines })).into_response(),
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
 // issue #3:握手协商结果(协议版本/capabilities)一并露出;无记录
 // (stdio 或未完成握手)时缺省省略,前端如实不显示
        let caps = hub.server_capabilities(&name).ok();
        match hub.probe_server(&name).await {
            Ok((count, tool_list)) => status.push(json!({
                "name": name, "ok": true, "tools": count, "tool_list": tool_list,
                "protocol_version": caps.as_ref().and_then(|c| c["protocolVersion"].as_str()),
                "capabilities": caps.as_ref().map(|c| c["capabilities"].clone()),
            })),
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

/// 自声明配置名校验(文件名拼接防注入;get/set 同规)。
#[allow(clippy::result_large_err)] // Err 即响应体(冷路径),不值得装箱
fn valid_config_name(name: &str) -> Result<(), Response> {
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Ok(())
    } else {
        Err(bad_request("非法配置名称(仅限英数字/下划线/连字符)"))
    }
}

/// 读 config/mcp-<name>.json 配置值:缺文件=空对象;读失败/损坏=500
/// (损坏前缀文案由调用方给,get/set 各有口径)。
#[allow(clippy::result_large_err)] // Err 即响应体(冷路径),不值得装箱
/// 读某 server 的配置文件(缺失 = 空对象;损坏拒绝)。ADR-0046 P5:改用
/// `json_store` 统一原语(。
fn read_server_config(file: &Path, corrupt_prefix: String) -> Result<Value, Response> {
    match super::json_store::read_json_file(file, "读取配置文件失败", &corrupt_prefix) {
        Ok(super::json_store::JsonRead::Value(v)) => Ok(v),
        Ok(super::json_store::JsonRead::Missing) => Ok(json!({})),
        Err(e) => Err(internal(e)),
    }
}

/// 读某 server 当前配置值(供设置页表单回显)。
pub async fn mcp_config_get(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
) -> Response {
    respond_or_fail!(valid_config_name(&name));
    let path = respond_or_fail!(
        cfg.mcp_config
            .clone()
            .ok_or_else(|| bad_request("服务器未启用 MCP 配置文件(--mcp-config)"))
    );
    let file = path
        .parent()
        .map(|d| d.join("config").join(format!("mcp-{name}.json")))
        .unwrap_or_else(|| path.clone());
    let values = respond_or_fail!(read_server_config(
        &file,
        format!("配置文件 mcp-{name}.json 格式损坏")
    ));
    Json(json!({ "name": name, "values": values })).into_response()
}

/// 写某 server 配置值(merge 保存)。改 key 免重启(override 文件链),
/// 其余配置项重载/重启生效。
pub async fn mcp_config_set(
    State(cfg): State<AdminConfig>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<McpConfigBody>,
) -> Response {
    respond_or_fail!(valid_config_name(&name));
    let path = respond_or_fail!(
        cfg.mcp_config
            .clone()
            .ok_or_else(|| bad_request("服务器未启用 MCP 配置文件(--mcp-config)"))
    );
    let dir = respond_or_fail!(
        path.parent()
            .map(|d| d.join("config"))
            .ok_or_else(|| internal("配置目录解析失败"))
    );
    let values = respond_or_fail!(
        body.values
            .as_object()
            .ok_or_else(|| bad_request("values 必须是对象"))
    );
    respond_or_fail!(
        std::fs::create_dir_all(&dir).map_err(|e| internal(format!("配置目录创建失败: {e}")))
    );
    let file = dir.join(format!("mcp-{name}.json"));
    let mut current = respond_or_fail!(read_server_config(
        &file,
        format!("mcp-{name}.json 格式损坏,拒绝合并覆写")
    ));
    if let Some(obj) = current.as_object_mut() {
        for (k, v) in values {
            obj.insert(k.clone(), v.clone());
        }
    }
 // CRLF 统一:与 config_store.write_file 同款(pretty 后按平台换行)
    let text = respond_or_fail!(
        serde_json::to_string_pretty(&current)
            .map(crate::config_store::crlf)
            .map_err(|_| internal("序列化失败"))
    );
    respond_or_fail!(
        bm_core::ports::persist::atomic_write(&file, text.as_bytes())
            .map_err(|e| internal(format!("写入失败: {e}")))
    );
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

// ---- handler:MCP 热装载(支持新增、修改与删除免重启)---------------

/// F-07 同款收口:热装载装配段(reload/批准上线/卸载下线/purge 共用)。
/// 全量同步 mcp.json 与已装载名单:新增 spawn+握手+注册、修改先拔后插、
/// 摘除 disconnect+unregister(ChildKill 兜底杀子进程)。
struct CoreRegistrar {
    handle: bm_core::runtime::RuntimeHandle,
}

#[async_trait::async_trait]
impl bm_core::ports::mcp_admin::CapabilityRegistrar for CoreRegistrar {
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
) -> Result<bm_core::ports::mcp_admin::SyncOutcome, (StatusCode, String)> {
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
 // ADR-0046:经端口同步(不再是 free function + 具体 hub)。
    let outcome = hub
        .sync(
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

/// 重载 MCP 配置:读 mcp.json 全量,与已装载名单对比:
/// - 已移除的 server:从 hub 摘除路由、发送 shutdown 通知,从 Registry/Persist 摘除能力
/// - 修改/保留的 server (如配置变更):先热拔旧 server,再用新配置重新握手连接并更新能力
/// - 新增的 server:spawn+握手+运行期注册
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
