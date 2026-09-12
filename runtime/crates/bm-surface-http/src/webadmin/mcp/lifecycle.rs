//! MCP 插件生命周期:墓碑 + 来源推断 + 候选条目构造(ADR-0023)。
//! 自 webadmin/mcp.rs 机械移出(2026-09-12,追 ADR-0048 手法);行为零变化。

use super::AdminConfig;
use super::{read_mcp_servers, validated_mcp_entry, write_mcp_servers};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// 墓碑文件(<data>/config/mcp-removed.json,私有管理文件不入合同):
/// 用户明确不要的插件名册。官方随包默认安装(启动播种)跳过墓碑名单;
/// 显式批准接入即除名(恢复安装的唯一正路)。
pub(super) fn tombstone_path(data_dir: &Path) -> PathBuf {
    data_dir.join("config").join("mcp-removed.json")
}

/// 墓碑文件读原语(#71 单源):宽容策略——缺/坏 = 空名册。
/// 用途仅「默认安装是否跳过该名」,损坏回退空表的后果是随包插件重新出现,
/// 不涉用户内容丢失,故取宽容(与既有行为一致)。
fn read_tombstone_value(data_dir: &Path) -> Value {
    bm_core::json_store::read_json_lenient(&tombstone_path(data_dir)).unwrap_or_else(|| json!({}))
}

pub(super) fn read_tombstones(data_dir: &Path) -> Vec<String> {
    read_tombstone_value(data_dir)["removed"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e["name"].as_str().map(String::from))
        .collect()
}

fn write_tombstones(data_dir: &Path, list: Vec<Value>) -> Result<(), String> {
    bm_core::json_store::write_json_file(
        &tombstone_path(data_dir),
        &json!({
            "removed": list,
            "note": "ADR-0023 墓碑:官方随包默认安装跳过本名单;显式批准接入即除名",
        }),
        "墓碑写盘失败",
    )
}

pub(super) fn upsert_tombstone(data_dir: &Path, name: &str) -> Result<(), String> {
    let existing = read_tombstone_value(data_dir)["removed"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let now = crate::unix_now();
    let mut list: Vec<Value> = existing
        .into_iter()
        .filter(|e| e["name"].as_str() != Some(name))
        .collect();
    list.push(json!({ "name": name, "removed_at": now }));
    write_tombstones(data_dir, list)
}

/// 清墓碑(显式批准安装时调用)。2026-09-08 审计修复:失败不再静默——
/// 墓碑残留会导致该官方插件重启后被再次自动移除,必须让调用方可见。
pub(super) fn remove_tombstone(data_dir: &Path, name: &str) -> Result<(), String> {
    let v = read_tombstone_value(data_dir);
    let Some(arr) = v["removed"].as_array() else {
        return Ok(());
    };
    let filtered: Vec<Value> = arr
        .iter()
        .filter(|e| e["name"].as_str() != Some(name))
        .cloned()
        .collect();
    if filtered.len() == arr.len() {
        return Ok(());
    }
    write_tombstones(data_dir, filtered)
}

/// 官方随包清单(plugins/.official.json,release 打包写入);缺失=未知,
/// 弃用标记优雅降级(旧版安装/本地开发)。
pub(super) fn official_plugin_list(cfg: &AdminConfig) -> Option<Vec<String>> {
    let bundled = cfg.bundled_plugins_dir.as_ref()?;
    let v = bm_core::json_store::read_json_lenient(&bundled.join(".official.json"))?;
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
pub(super) fn server_origin(cfg: &AdminConfig, srv: &Value, mcp_json_path: &Path) -> &'static str {
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
pub(super) fn build_candidate_entry(
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
    let bytes = std::fs::read(file).map_err(|e| format!("读取 {} 失败: {e}", file.display()))?;
    let sha = bm_contract::hash::sha256_hex(&bytes);
    let entry_body = json!({
        "name": name,
        "command": file.display().to_string(),
        // ADR-0035:托管条目恒声明 payload=候选文件,使完整性校验目标无歧义
        // (哈希对象 = 真实载荷,而非可能的启动器);trust 显式落盘。
        "payload": file.display().to_string(),
        "sha256": sha,
        "trust": "explicit-config",
        "args": args,
        "tool_timeout_ms": decl.pointer("/suggested_entry/tool_timeout_ms").cloned().unwrap_or(json!(30000)),
        "restart_limit": decl.pointer("/suggested_entry/restart_limit").cloned().unwrap_or(json!(3)),
    });
    validated_mcp_entry(&entry_body)
}

/// manifest 双写(批准/播种共用):manifests/<name>.manifest.json,
/// 设置页「配置」表单的声明来源。
pub(super) fn write_plugin_manifest(mcp_json_path: &Path, name: &str, decl: &Value) {
    let manifest = json!({
        "name": name,
        "title": decl.get("title").cloned().unwrap_or(json!("")),
        "description": decl.get("description").cloned().unwrap_or(json!("")),
        "config_schema": decl.get("config_schema").cloned().unwrap_or(json!([])),
    });
    if let Some(mdir) = mcp_json_path.parent().map(|d| d.join("manifests")) {
        let _ = std::fs::create_dir_all(&mdir);
        if let Ok(text) = serde_json::to_string_pretty(&manifest) {
            let _ = bm_core::ports::persist::atomic_write(
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
    for (p, decl) in super::scan::scan_candidates(bundled_dir)
        .await
        .unwrap_or_default()
    {
        let name = decl["name"].as_str().unwrap_or_default().to_string();
        if seen.iter().any(|r| r == &name) || tombstones.iter().any(|t| t == &name) {
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
