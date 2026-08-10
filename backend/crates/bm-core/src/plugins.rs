//! 插件系统：基于 pi 扩展机制（QuickJS 运行时直接加载 TypeScript 扩展）。
//!
//! 插件 = `~/.boenmind/extensions/` 下的单文件 `.ts` 扩展或含 `extension.json` 的目录。
//! 启用列表记录在 config.toml 的 `enabled_plugins`；agent 会话通过
//! `SessionOptions.extension_paths` 加载启用插件。

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{AppConfig, app_dir};

/// 插件根目录名（位于 ~/.boenmind 下）
pub const PLUGINS_DIR: &str = "extensions";
/// 内置示例插件清单（来自 vendored pi_agent_rust 官方示例，均为类型级依赖，QuickJS 可直接加载）
pub const BUILTIN_PLUGINS: &[(&str, &str)] = &[
    ("hello", "注册演示工具：Hello，展示 LLM 可调用工具"),
    ("bookmark", "注册斜杠命令：/bookmark 为消息添加书签"),
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
    /// 内置示例（不可删除，随 vendored 仓库提供）
    pub builtin: bool,
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
        out.push(PluginInfo {
            id: id.clone(),
            name: id.clone(),
            description: desc,
            kind,
            enabled: config.enabled_plugins.contains(&id),
            builtin: BUILTIN_PLUGINS.iter().any(|(bid, _)| *bid == id),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// 从扩展源提取描述（读取文件头部注释或 extension.json 的 description）。
fn describe_plugin(path: &Path, fallback: &str) -> String {
    let manifest = path.join("extension.json");
    if manifest.is_file() {
        if let Ok(text) = fs::read_to_string(&manifest)
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(desc) = json.get("description").and_then(|v| v.as_str())
        {
            return desc.to_string();
        }
    }
    if let Ok(text) = fs::read_to_string(path) {
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
        copy_dir(source, &dest).map_err(|e| e.to_string())?;
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
        .map(|id| {
            let dir = plugins_dir();
            let file = dir.join(format!("{id}.ts"));
            let dir_plugin = dir.join(id);
            if file.is_file() {
                file
            } else if dir_plugin.join("extension.json").is_file() {
                dir_plugin
            } else {
                file // 不存在时返回预期路径，由 pi 加载时报错
            }
        })
        .collect()
}

/// 首次启动时预装内置示例插件。
pub fn ensure_builtin_plugins() -> Result<(), std::io::Error> {
    let dir = plugins_dir();
    fs::create_dir_all(&dir)?;
    for (id, _desc) in BUILTIN_PLUGINS {
        let dest = dir.join(format!("{id}.ts"));
        if dest.exists() {
            continue;
        }
        if let Some(src) = vendored_example_path(id) {
            fs::copy(&src, &dest)?;
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

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugins_dir_under_app_dir() {
        assert!(plugins_dir().ends_with(".boenmind/extensions"));
    }
}
