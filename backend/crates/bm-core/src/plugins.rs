//! 插件系统：基于 pi 扩展机制（QuickJS 运行时直接加载 TypeScript 扩展）。
//!
//! 插件 = `~/.boenmind/extensions/` 下的单文件 `.ts` 扩展或含 `extension.json` 的目录。
//! 启用列表记录在 config.toml 的 `enabled_plugins`；agent 会话通过
//! `SessionOptions.extension_paths` 加载启用插件。

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{AppConfig, app_dir};
use crate::http_util::copy_dir_excluding;

/// 插件根目录名（位于 ~/.boenmind 下）
pub const PLUGINS_DIR: &str = "extensions";
/// 内置示例插件清单（来自 vendored pi_agent_rust 官方示例，均为类型级依赖，QuickJS 可直接加载）
pub const BUILTIN_PLUGINS: &[(&str, &str)] = &[
    ("hello", "注册演示工具：Hello，展示 LLM 可调用工具"),
    ("bookmark", "注册斜杠命令：/bookmark 为消息添加书签"),
    (
        "ctx-compactor",
        "上下文压缩补强：ctx_execute 沙箱执行 + 大工具输出修剪落库 + ctx_search 检索",
    ),
    (
        "web-search",
        "搜索增强：web_search 多源聚合（免费源用量管理与自动切换）+ web_fetch 网页正文提取",
    ),
];

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    /// 插件 id（文件名或目录名）
    pub id: String,
    pub name: String,
    pub description: String,
    /// 扩展类型：单文件（single）或清单目录（manifest）
    pub kind: String,
    pub enabled: bool,
    /// 内置插件（随仓库/上游提供；卸载后写入 removed_builtin_plugins，不再自动恢复）
    pub builtin: bool,
    /// 插件设置页 schema（manifest 的 settings 声明；None = 无设置页）
    #[serde(skip_serializing_if = "Option::is_none", rename = "settingsSchema")]
    pub settings_schema: Option<Vec<crate::plugin_settings::SettingField>>,
    /// 用量声明（manifest 的 quota 段；None = 无用量统计）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota: Option<crate::plugin_settings::QuotaDecl>,
    /// 设置页测试按钮的探测模板（manifest 的 testSources 段；None = 无测试按钮）
    #[serde(skip_serializing_if = "Option::is_none", rename = "testSources")]
    pub test_sources: Option<std::collections::HashMap<String, crate::plugin_settings::TestSourceDecl>>,
}

/// 插件根目录。
pub fn plugins_dir() -> PathBuf {
    app_dir().join(PLUGINS_DIR)
}

/// 扫描插件目录并返回插件列表。
pub fn list_plugins(config: &AppConfig) -> Result<Vec<PluginInfo>, std::io::Error> {
    let dir = plugins_dir();
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // 单文件 .ts 扩展 或 含 extension.json 的目录
        let (id, kind, desc) = if path.is_file() {
            if !name.ends_with(".ts") {
                continue;
            }
            let id = name.strip_suffix(".ts").unwrap_or(&name).to_string();
            (id.clone(), "single".to_string(), describe_plugin(&path, &id))
        } else if path.is_dir() && path.join("extension.json").is_file() {
            let id = name.clone();
            (id, "manifest".to_string(), describe_plugin(&path, &name))
        } else {
            continue;
        };
        // 插件文件/目录已被手动删除（配置残留）时，不显示为已启用
        let enabled = config.enabled_plugins.contains(&id) && plugin_exists(&dir, &id);
        out.push(PluginInfo {
            id: id.clone(),
            name: id.clone(),
            description: desc,
            kind,
            enabled,
            builtin: BUILTIN_PLUGINS.iter().any(|(bid, _)| *bid == id),
            settings_schema: manifest_settings_schema(&path),
            quota: manifest_quota(&path),
            test_sources: manifest_test_sources(&path),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// 解析插件目录 manifest（extension.json），返回 JSON 值（不存在/损坏返回 None）。
fn read_manifest(path: &Path) -> Option<serde_json::Value> {
    let manifest = path.join("extension.json");
    if !manifest.is_file() {
        return None;
    }
    let text = fs::read_to_string(&manifest).ok()?;
    serde_json::from_str::<serde_json::Value>(&text).ok()
}

/// 解析插件目录 manifest 的 settings schema（单文件插件/无 settings 声明返回 None）。
fn manifest_settings_schema(path: &Path) -> Option<Vec<crate::plugin_settings::SettingField>> {
    let json = read_manifest(path)?;
    crate::plugin_settings::parse_settings_schema(&json)
}

/// 解析插件目录 manifest 的 quota 声明（单文件插件/未声明返回 None）。
fn manifest_quota(path: &Path) -> Option<crate::plugin_settings::QuotaDecl> {
    let json = read_manifest(path)?;
    crate::plugin_settings::parse_quota_decl(&json)
}

/// 解析插件目录 manifest 的 testSources 声明（单文件插件/未声明返回 None）。
fn manifest_test_sources(
    path: &Path,
) -> Option<std::collections::HashMap<String, crate::plugin_settings::TestSourceDecl>> {
    let json = read_manifest(path)?;
    crate::plugin_settings::parse_test_sources(&json)
}

/// 插件文件/目录是否实际存在。
fn plugin_exists(dir: &Path, id: &str) -> bool {
    dir.join(format!("{id}.ts")).is_file() || dir.join(id).join("extension.json").is_file()
}

/// 从扩展源提取描述（读取文件头部注释或 extension.json 的 description）。
fn describe_plugin(path: &Path, fallback: &str) -> String {
    let manifest = path.join("extension.json");
    if manifest.is_file()
        && let Ok(text) = fs::read_to_string(&manifest)
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(desc) = json.get("description").and_then(|v| v.as_str())
        {
            return desc.to_string();
        }
    if let Ok(text) = fs::read_to_string(path) {
        // 优先提取代码中的 description 字段（如 pi.registerTool({ description: "..." })）
        if let Some(captures) = text
            .lines()
            .find_map(|line| {
                let line = line.trim();
                line.strip_prefix("description:")
                    .map(|rest| rest.trim().trim_matches(['"', ',', '`']))
                    .filter(|desc| !desc.is_empty() && desc.len() < 120)
            })
        {
            return captures.to_string();
        }
        for line in text.lines().take(8) {
            let line = line.trim().trim_start_matches(['*', '/', ' ', '\t']);
            if line.starts_with("Shows") || line.starts_with("Registers") || line.starts_with("注册") {
                return line.to_string();
            }
        }
    }
    fallback.to_string()
}

/// 安装插件：复制源路径（文件或目录）到插件根目录。
pub fn install_plugin(source: &Path) -> Result<PluginInfo, String> {
    let dir = plugins_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "无效的插件路径".to_string())?;
    if !(name.ends_with(".ts") || source.is_dir()) {
        return Err("仅支持 .ts 扩展文件或插件目录".to_string());
    }
    let dest = dir.join(name);
    if dest.exists() {
        return Err(format!("插件 {name} 已存在"));
    }
    if source.is_dir() {
        copy_dir_excluding(source, &dest, &[]).map_err(|e| e.to_string())?;
    } else {
        fs::copy(source, &dest).map_err(|e| e.to_string())?;
    }
    let id = name.strip_suffix(".ts").unwrap_or(name).to_string();
    Ok(PluginInfo {
        id,
        name: name.to_string(),
        description: describe_plugin(&dest, name),
        kind: if dest.is_dir() { "manifest" } else { "single" }.to_string(),
        enabled: false,
        builtin: false,
        settings_schema: manifest_settings_schema(&dest),
        quota: manifest_quota(&dest),
        test_sources: manifest_test_sources(&dest),
    })
}

/// 启用/禁用插件（写入 config.enabled_plugins 并持久化）。
pub fn set_plugin_enabled(
    config: &mut AppConfig,
    id: &str,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        if !config.enabled_plugins.iter().any(|p| p == id) {
            config.enabled_plugins.push(id.to_string());
        }
    } else {
        config.enabled_plugins.retain(|p| p != id);
    }
    crate::config::save(config).map_err(|e| e.to_string())
}

/// 当前启用的插件路径列表（供 agent 会话加载）。
pub fn enabled_extension_paths(config: &AppConfig) -> Vec<PathBuf> {
    config
        .enabled_plugins
        .iter()
        .filter_map(|id| {
            let dir = plugins_dir();
            let file = dir.join(format!("{id}.ts"));
            let dir_plugin = dir.join(id);
            if file.is_file() {
                Some(file)
            } else if dir_plugin.join("extension.json").is_file() {
                Some(dir_plugin)
            } else {
                // 插件已被手动删除（配置残留）：静默跳过，避免会话创建失败
                eprintln!("[bm-core] 插件 {id} 已不存在，忽略（可从配置移除）");
                None
            }
        })
        .collect()
}

/// 卸载插件：删除文件/目录并从启用列表移除。
/// 内置插件同样可卸载；卸载后写入 removed_builtin_plugins，启动预装时不再恢复。
pub fn uninstall_plugin(config: &mut AppConfig, id: &str) -> Result<(), String> {
    let dir = plugins_dir();
    let file = dir.join(format!("{id}.ts"));
    let dir_plugin = dir.join(id);
    if file.exists() {
        fs::remove_file(&file).map_err(|e| format!("删除插件文件失败: {e}"))?;
    } else if dir_plugin.exists() {
        fs::remove_dir_all(&dir_plugin).map_err(|e| format!("删除插件目录失败: {e}"))?;
    } else {
        return Err(format!("插件 {id} 不存在"));
    }
    config.enabled_plugins.retain(|p| p != id);
    // 内置插件记录到"已卸载"列表，避免下次启动被 ensure_builtin_plugins 重新预装
    if BUILTIN_PLUGINS.iter().any(|(bid, _)| *bid == id)
        && !config.removed_builtin_plugins.iter().any(|p| p == id)
    {
        config.removed_builtin_plugins.push(id.to_string());
    }
    crate::config::save(config).map_err(|e| e.to_string())?;
    Ok(())
}

/// 首次启动时预装内置插件；用户已卸载的（removed_builtin_plugins）跳过。
pub fn ensure_builtin_plugins(config: &AppConfig) -> Result<(), std::io::Error> {
    let dir = plugins_dir();
    fs::create_dir_all(&dir)?;
    for (id, _desc) in BUILTIN_PLUGINS {
        // 软件自由：用户卸载的内置插件尊重其选择，不自动"复活"
        if config.removed_builtin_plugins.iter().any(|p| p == id) {
            continue;
        }
        if let Some(src) = vendored_example_path(id) {
            let dest = dir.join(format!("{id}.ts"));
            if dest.exists() {
                continue;
            }
            fs::copy(&src, &dest)?;
        }
        // 仓库内自带插件（目录型，如 ctx-compactor）
        if let Some(src) = repo_plugin_dir(id) {
            let dest = dir.join(id);
            if dest.exists() {
                continue;
            }
            copy_dir_excluding(&src, &dest, &[])?;
        }
    }
    Ok(())
}

/// vendored pi_agent_rust 仓库内官方示例扩展的路径。
fn vendored_example_path(id: &str) -> Option<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let p = base
        .join("../../vendor/pi_agent_rust/legacy_pi_mono_code/pi-mono/packages/coding-agent/examples/extensions")
        .join(format!("{id}.ts"));
    p.is_file().then_some(p)
}

/// BoenMind 仓库自带插件目录（backend/plugins/<id>/，含 extension.json）。
fn repo_plugin_dir(id: &str) -> Option<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let p = base.join("../../plugins").join(id);
    (p.join("extension.json").is_file()).then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugins_dir_under_app_dir() {
        assert!(plugins_dir().ends_with(".boenmind/extensions"));
    }
}
