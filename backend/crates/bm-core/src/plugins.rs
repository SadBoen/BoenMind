//! 插件系统：基于 pi 扩展机制（QuickJS 运行时直接加载 TypeScript 扩展）。
//!
//! 插件 = `~/.boenmind/extensions/` 下的单文件 `.ts` 扩展或含 `extension.json` 的目录。
//! 启用列表记录在 config.toml 的 `enabled_plugins`；agent 会话通过
//! `SessionOptions.extension_paths` 加载启用插件。
//! 出厂内置插件（[`BUILTIN_PLUGINS`]）全部为目录型插件，首次启动预装
//! （用户卸载后写入 removed_builtin_plugins，不再恢复）。

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{AppConfig, app_dir};
use crate::error::AppError;
use crate::http_util::copy_dir_excluding;

/// 插件根目录名（位于 ~/.boenmind 下）
pub const PLUGINS_DIR: &str = "extensions";
/// 出厂内置插件清单（全部为仓库自带目录型插件，自带 extension.json）
pub const BUILTIN_PLUGINS: &[(&str, &str)] = &[
    (
        "role",
        "角色定义：role 工具创建/切换助手角色（人格与职责设定），宿主注入当前角色到系统提示",
    ),
    (
        "coding-memory",
        "编程记忆（编程 APP 专用）：coding_remember/coding_recall/coding_forget 按项目存取记忆，不污染全局长期记忆",
    ),
    (
        "ctx-compactor",
        "上下文压缩补强：ctx_execute 沙箱执行 + 大工具输出修剪落库 + ctx_search 检索",
    ),
    (
        "web-search",
        "搜索增强：web_search 多源聚合（免费源用量管理与自动切换）+ web_fetch 网页正文提取",
    ),
    (
        "refine-suggest",
        "自我改进建议采集：任务完成后提交针对 skill 描述/系统提示词的改进建议（提交仅记录，用户审批后才生效）",
    ),
    (
        "pdf-omni",
        "PDF 智能解析（Rust 核心）：MinerU 主 + LlamaParse 交叉验证/级联增强，版面/表格/公式保真转 Markdown",
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
    /// 分类标签（manifest category 声明；缺省 "system" 系统增强）。
    /// "system" = 系统增强（记忆/压缩/搜索等运行时能力）；"app" = 功能插件
    /// （PDF/WIKI/编程工具等用户功能）。前端插件页按此分标签页展示。
    pub category: String,
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
///
/// 便携形态（BOENMIND_PORTABLE_DIR）合并两个来源：包内 plugins/（出厂默认，
/// 只读随包）+ 用户 ~/.boenmind/extensions/（用户安装与覆盖版本）；同 id 时
/// **用户目录优先**（可覆盖出厂版本）。enabled 按配置判定（包内出厂插件同样
/// 可被启用/禁用）。
pub fn list_plugins(config: &AppConfig) -> Result<Vec<PluginInfo>, std::io::Error> {
    let user_dir = plugins_dir();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(pkg) = crate::config::portable_plugins_dir() {
        dirs.push(pkg);
    }
    dirs.push(user_dir.clone());

    let mut out: Vec<PluginInfo> = Vec::new();
    for dir in &dirs {
        if !dir.exists() {
            continue;
        }
        for entry in fs::read_dir(dir)? {
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
            // 同 id 已收录（用户目录在后，覆盖包内）→ 跳过
            if out.iter().any(|p| p.id == id) {
                continue;
            }
            // 插件文件/目录已被手动删除（配置残留）时，不显示为已启用
            let enabled = config.enabled_plugins.contains(&id) && plugin_exists(&user_dir, &id);
            out.push(PluginInfo {
                id: id.clone(),
                name: id.clone(),
                description: desc,
                kind,
                enabled,
                builtin: BUILTIN_PLUGINS.iter().any(|(bid, _)| *bid == id),
                category: manifest_category(&path),
                settings_schema: manifest_settings_schema(&path),
                quota: manifest_quota(&path),
                test_sources: manifest_test_sources(&path),
            });
        }
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

/// 解析插件分类标签（manifest category 声明；单文件插件/未声明默认 "system"）。
/// "system" = 系统增强（记忆/压缩/搜索等运行时能力）；"app" = 功能插件
/// （PDF/WIKI/编程工具等用户功能）。
fn manifest_category(path: &Path) -> String {
    let Some(json) = read_manifest(path) else { return "system".to_string() };
    match json.get("category").and_then(serde_json::Value::as_str) {
        Some("app") => "app".to_string(),
        _ => "system".to_string(),
    }
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
pub fn install_plugin(source: &Path) -> Result<PluginInfo, AppError> {
    let dir = plugins_dir();
    fs::create_dir_all(&dir)?;
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::invalid("无效的插件路径"))?;
    if !(name.ends_with(".ts") || source.is_dir()) {
        return Err(AppError::invalid("仅支持 .ts 扩展文件或插件目录"));
    }
    let dest = dir.join(name);
    if dest.exists() {
        return Err(AppError::invalid(format!("插件 {name} 已存在")));
    }
    if source.is_dir() {
        copy_dir_excluding(source, &dest, &[])?;
    } else {
        fs::copy(source, &dest)?;
    }
    let id = name.strip_suffix(".ts").unwrap_or(name).to_string();
    Ok(PluginInfo {
        id,
        name: name.to_string(),
        description: describe_plugin(&dest, name),
        kind: if dest.is_dir() { "manifest" } else { "single" }.to_string(),
        enabled: false,
        builtin: false,
        category: manifest_category(&dest),
        settings_schema: manifest_settings_schema(&dest),
        quota: manifest_quota(&dest),
        test_sources: manifest_test_sources(&dest),
    })
}

/// 按包源安装插件（`npm:包名` / `git:host/owner/repo[@ref]` / 本地路径）。
///
/// 流程（自研实现，2026-08-15 pi 废除后替代上游 PackageManager）：
/// 源解析 → 装到源缓存目录（npm install --prefix / git clone --depth 1）→
/// 定位包目录 → 找出包内扩展资源（`extensions/` 子目录或包根即扩展）→
/// 复制进插件根目录。一个包可含多个扩展，返回新装入的全部插件
/// （安装后默认禁用，由 UI 启用）。npm 安装可能耗时较长，调用方应放
/// 阻塞线程执行。
pub fn install_plugin_from_source(source: &str) -> Result<Vec<PluginInfo>, AppError> {
    let source = source.trim().to_string();
    if source.is_empty() {
        return Err(AppError::invalid("插件源不能为空"));
    }
    let pkg_root = match source.split_once(':') {
        Some(("npm", spec)) if !spec.trim().is_empty() => install_npm_source(spec)?,
        Some(("git", spec)) if !spec.trim().is_empty() => install_git_source(spec)?,
        _ => PathBuf::from(&source), // 本地路径（存在性由扩展探查兜底）
    };
    let entries = package_extension_entries(&pkg_root)
        .map_err(|e| AppError::internal(format!("读取包内扩展失败: {e}")))?;
    if entries.is_empty() {
        return Err(AppError::invalid(format!("包 {source} 内没有扩展资源")));
    }
    let dir = plugins_dir();
    fs::create_dir_all(&dir)?;
    let mut installed = Vec::new();
    for entry in entries {
        let name = entry
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AppError::invalid("包内扩展名无效"))?
            .to_string();
        let dest = dir.join(&name);
        if dest.exists() {
            return Err(AppError::invalid(format!("插件 {name} 已存在（可先卸载再重装）")));
        }
        if entry.is_dir() {
            copy_dir_excluding(&entry, &dest, &[])?;
        } else {
            fs::copy(&entry, &dest)?;
        }
        let id = name.strip_suffix(".ts").unwrap_or(&name).to_string();
        installed.push(PluginInfo {
            id,
            name: name.clone(),
            description: describe_plugin(&dest, &name),
            kind: if dest.is_dir() { "manifest" } else { "single" }.to_string(),
            enabled: false,
            builtin: false,
            category: manifest_category(&dest),
            settings_schema: manifest_settings_schema(&dest),
            quota: manifest_quota(&dest),
            test_sources: manifest_test_sources(&dest),
        });
    }
    Ok(installed)
}

/// 包源缓存目录（npm/git 源的落地处；隔离在 app_dir 下便于清理与重装）。
fn package_sources_dir() -> PathBuf {
    app_dir().join("plugin-sources")
}

/// npm 源：`npm install --prefix <缓存目录> <spec>` → 定位 `node_modules/<name>`。
/// spec 形如 `pkg@1.2.3` 或 `@scope/pkg@1.2.3`——包名取第一个 `@` 之前的
/// 段（scope 包名 = `@scope/pkg`，版本号在第二个 `@` 后）。
fn install_npm_source(spec: &str) -> Result<PathBuf, AppError> {
    let root = package_sources_dir();
    fs::create_dir_all(&root)?;
    let mut cmd = std::process::Command::new("npm");
    cmd.arg("install")
        .arg("--prefix")
        .arg(&root)
        .arg("--")
        .arg(spec);
    run_package_command(cmd, "npm 安装失败")?;
    let name = npm_package_name(spec);
    let installed = root.join("node_modules").join(name);
    if !installed.is_dir() {
        return Err(AppError::internal(format!(
            "npm 安装完成但无法定位包目录: {}",
            installed.display()
        )));
    }
    Ok(installed)
}

/// 从 npm spec 提取包名（`@scope/pkg@1.0.0` → `@scope/pkg`；`pkg@1.0.0` → `pkg`）。
fn npm_package_name(spec: &str) -> String {
    let s = spec.trim();
    if let Some(rest) = s.strip_prefix('@') {
        let mut parts = rest.splitn(2, '/');
        let scope = parts.next().unwrap_or("");
        let pkg = parts
            .next()
            .unwrap_or("")
            .split('@')
            .next()
            .unwrap_or("");
        format!("@{scope}/{pkg}")
    } else {
        s.split('@').next().unwrap_or(s).to_string()
    }
}

/// git 源：`git:host/owner/repo[@ref]` → `git clone --depth 1 [--branch ref]`
/// 到缓存目录（URL = https://<host>/<owner>/<repo>；重装先清缓存目录）。
fn install_git_source(spec: &str) -> Result<PathBuf, AppError> {
    let (repo, reference) = match spec.rsplit_once('@') {
        Some((r, refname)) if !refname.contains('/') => (r, Some(refname)),
        _ => (spec, None),
    };
    let url = format!("https://{repo}");
    let dir_name = repo.replace(['/', '.'], "__");
    let dest = package_sources_dir().join(dir_name);
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| AppError::internal(format!("清理旧缓存失败: {e}")))?;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.arg("clone").arg("--depth").arg("1");
    if let Some(r) = reference {
        cmd.arg("--branch").arg(r);
    }
    cmd.arg(&url).arg(&dest);
    run_package_command(cmd, "git clone 失败")?;
    Ok(dest)
}

/// 运行包管理命令（npm/git），失败带 stderr 摘要。
fn run_package_command(mut cmd: std::process::Command, what: &str) -> Result<(), AppError> {
    let out = cmd
        .output()
        .map_err(|e| AppError::internal(format!("{what}: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: String = stderr
            .chars()
            .rev()
            .take(300)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        return Err(AppError::internal(format!("{what}: {tail}")));
    }
    Ok(())
}

/// 包内扩展条目的探查：优先 `extensions/` 子目录（上游包的默认布局），
/// 否则接受"包根即扩展"（根有 extension.json 的目录型或根下的 .ts 单文件）。
fn package_extension_entries(pkg_root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut out = Vec::new();
    let ext_dir = pkg_root.join("extensions");
    if ext_dir.is_dir() {
        for entry in fs::read_dir(&ext_dir)? {
            let entry = entry?;
            let path = entry.path();
            let is_extension = (path.is_dir() && path.join("extension.json").is_file())
                || (path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("ts"));
            if is_extension {
                out.push(path);
            }
        }
        return Ok(out);
    }
    if pkg_root.join("extension.json").is_file() {
        out.push(pkg_root.to_path_buf());
    } else if let Some(ts) = fs::read_dir(pkg_root)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("ts"))
    {
        out.push(ts);
    }
    Ok(out)
}

/// 启用/禁用插件（写入 config.enabled_plugins 并持久化）。
pub fn set_plugin_enabled(
    config: &mut AppConfig,
    id: &str,
    enabled: bool,
) -> Result<(), AppError> {
    if enabled {
        if !config.enabled_plugins.iter().any(|p| p == id) {
            config.enabled_plugins.push(id.to_string());
        }
    } else {
        config.enabled_plugins.retain(|p| p != id);
    }
    crate::config::save(config)?;
    Ok(())
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
pub fn uninstall_plugin(config: &mut AppConfig, id: &str) -> Result<(), AppError> {
    let dir = plugins_dir();
    // 便携形态：出厂插件在包内（只读随包），卸载 = 配置移除 + 记入
    // removed_builtin_plugins（启动预装跳过），不删包内文件。
    let in_pkg = crate::config::portable_plugins_dir()
        .map(|p| p.join(id).join("extension.json").is_file())
        .unwrap_or(false);
    let file = dir.join(format!("{id}.ts"));
    let dir_plugin = dir.join(id);
    if in_pkg {
        // 包内出厂插件：仅配置移除
    } else if file.exists() {
        fs::remove_file(&file).map_err(|e| AppError::internal(format!("删除插件文件失败: {e}")))?;
    } else if dir_plugin.exists() {
        fs::remove_dir_all(&dir_plugin)
            .map_err(|e| AppError::internal(format!("删除插件目录失败: {e}")))?;
    } else {
        return Err(AppError::invalid(format!("插件 {id} 不存在")));
    }
    config.enabled_plugins.retain(|p| p != id);
    // 内置插件记录到"已卸载"列表，避免下次启动被 ensure_builtin_plugins 重新预装
    if BUILTIN_PLUGINS.iter().any(|(bid, _)| *bid == id)
        && !config.removed_builtin_plugins.iter().any(|p| p == id)
    {
        config.removed_builtin_plugins.push(id.to_string());
    }
    crate::config::save(config)?;
    Ok(())
}

/// 首次启动时预装内置插件；用户已卸载的（removed_builtin_plugins）跳过。
///
/// 两种构建形态：
/// - 普通构建：从仓库路径复制（backend/plugins/<id>/ 目录型插件）
/// - embed 构建（服务器版）：目录型插件从二进制内嵌资源写出（部署机没有仓库路径，
///   此前静默失败导致服务器用户实际无插件）
pub fn ensure_builtin_plugins(config: &AppConfig) -> Result<(), std::io::Error> {
    // 便携形态：出厂插件随包分发（包内 plugins/，list_plugins 合并扫描），
    // 无需预装；removed_builtin_plugins 语义由包内插件跳过启用承接。
    if crate::config::portable_plugins_dir().is_some() {
        return Ok(());
    }
    let dir = plugins_dir();
    fs::create_dir_all(&dir)?;
    for (id, _desc) in BUILTIN_PLUGINS {
        // 软件自由：用户卸载的内置插件尊重其选择，不自动"复活"
        if config.removed_builtin_plugins.iter().any(|p| p == id) {
            continue;
        }
        #[cfg(feature = "embed-plugins")]
        {
            let _ = embed_repo_plugin(id, &dir)?;
            continue;
        }
        #[cfg(not(feature = "embed-plugins"))]
        {
            // 仓库内自带插件（目录型，含 extension.json）
            if let Some(src) = repo_plugin_dir(id) {
                let dest = dir.join(id);
                if dest.exists() {
                    continue;
                }
                copy_dir_excluding(&src, &dest, &[])?;
            }
        }
    }
    Ok(())
}

/// embed 构建：把 backend/plugins/<id>/ 从二进制内嵌资源写出到 extensions/<id>/。
/// 返回是否找到该目录型插件（未找到 = 非仓库内置，embed 构建不预装）。
#[cfg(feature = "embed-plugins")]
fn embed_repo_plugin(id: &str, dest_dir: &std::path::Path) -> Result<bool, std::io::Error> {
    let prefix = format!("{id}/");
    let mut found = false;
    for file in EmbeddedPlugins::iter() {
        let rel = file.as_ref();
        let Some(inner) = rel.strip_prefix(&prefix) else {
            continue;
        };
        found = true;
        let dest = dest_dir.join(id).join(inner);
        if dest.exists() {
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = EmbeddedPlugins::get(&file)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "内嵌插件文件缺失"))?;
        fs::write(&dest, data.data.as_ref())?;
    }
    Ok(found)
}

#[cfg(feature = "embed-plugins")]
#[derive(rust_embed::Embed)]
#[folder = "../../plugins"]
struct EmbeddedPlugins;

/// 仓库自带插件目录（backend/plugins/<id>/，含 extension.json；仅普通构建使用）。
#[cfg(not(feature = "embed-plugins"))]
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

    /// 包内扩展探查：extensions/ 子目录布局（上游 npm 包默认布局）优先，
    /// 目录型与单文件 .ts 均被识别，无关文件（README 等）被忽略。
    #[test]
    fn package_extension_entries_finds_extensions_dir() {
        let _guard = crate::config::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = std::env::temp_dir().join(format!("bm-pkg-ext-{}", std::process::id()));
        let pkg = dir.join("pkg");
        let ext = pkg.join("extensions");
        fs::create_dir_all(&ext).unwrap();
        fs::write(ext.join("alpha.ts"), "export default function (pi) {}").unwrap();
        let manifest_dir = ext.join("beta");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(manifest_dir.join("extension.json"), "{}").unwrap();
        fs::write(ext.join("README.md"), "not an extension").unwrap();
        fs::write(pkg.join("package.json"), "{}").unwrap();

        let entries = package_extension_entries(&pkg).unwrap();
        let mut names: Vec<String> = entries
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["alpha.ts", "beta"]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 包根即扩展：根含 extension.json 的目录型包整个作为扩展。
    #[test]
    fn package_extension_entries_root_as_extension() {
        let _guard = crate::config::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = std::env::temp_dir().join(format!("bm-pkg-root-{}", std::process::id()));
        let pkg = dir.join("my-plugin");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("extension.json"), "{}").unwrap();
        fs::write(pkg.join("index.ts"), "export default function (pi) {}").unwrap();

        let entries = package_extension_entries(&pkg).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], pkg);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 本地目录源安装：包根即扩展时，整个目录拷入插件根目录并可被 list 识别。
    #[test]
    fn install_plugin_from_local_source_copies_dir() {
        let _guard = crate::config::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original = std::env::var_os("BOENMIND_HOME");
        let home = std::env::temp_dir().join(format!("bm-install-{}", std::process::id()));
        unsafe { std::env::set_var("BOENMIND_HOME", &home) };
        let pkg = std::env::temp_dir().join(format!("bm-pkg-src-{}", std::process::id()));
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join("extension.json"),
            r#"{"name": "demo", "description": "demo plugin"}"#,
        )
        .unwrap();
        fs::write(pkg.join("index.ts"), "export default function (pi) {}").unwrap();

        let result = install_plugin_from_source(pkg.to_str().unwrap());
        let expected_id = pkg.file_name().unwrap().to_string_lossy().into_owned();
        let infos = result.expect("本地目录源安装应成功");
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, expected_id);
        assert!(plugins_dir().join(&expected_id).join("extension.json").is_file());
        // 二次安装同名插件应报"已存在"
        assert!(install_plugin_from_source(pkg.to_str().unwrap()).is_err());

        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&pkg);
        match original {
            Some(v) => unsafe { std::env::set_var("BOENMIND_HOME", v) },
            None => unsafe { std::env::remove_var("BOENMIND_HOME") },
        }
    }

    /// embed 构建的关键路径：内嵌资源能写出目录型插件（服务器版预装依赖此路径）。
    /// 用 `cargo test -p bm-core --features embed-plugins` 运行。
    #[cfg(feature = "embed-plugins")]
    #[test]
    fn embed_plugin_writes_dir_plugins() {
        let _guard = crate::config::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original = std::env::var_os("BOENMIND_HOME");
        let dir = std::env::temp_dir().join(format!("bm-embed-{}", std::process::id()));
        unsafe { std::env::set_var("BOENMIND_HOME", &dir) };
        let dir_plugins = plugins_dir();
        assert!(embed_repo_plugin("ctx-compactor", &dir_plugins).unwrap());
        assert!(dir_plugins.join("ctx-compactor/index.ts").is_file());
        assert!(dir_plugins.join("ctx-compactor/extension.json").is_file());
        assert!(embed_repo_plugin("web-search", &dir_plugins).unwrap());
        // 出厂插件（role/coding-memory）全部为目录型，均内嵌可预装
        assert!(embed_repo_plugin("role", &dir_plugins).unwrap());
        assert!(dir_plugins.join("role/extension.json").is_file());
        assert!(embed_repo_plugin("coding-memory", &dir_plugins).unwrap());
        match original {
            Some(v) => unsafe { std::env::set_var("BOENMIND_HOME", v) },
            None => unsafe { std::env::remove_var("BOENMIND_HOME") },
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 分类标签解析：manifest category 声明生效，未声明/未知值/无 manifest 回落 "system"。
    #[test]
    fn manifest_category_parses_label() {
        let dir = std::env::temp_dir().join(format!("bm-cat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 无 manifest → 默认 system
        assert_eq!(manifest_category(&dir), "system");
        // 显式 app
        std::fs::write(dir.join("extension.json"), r#"{"category":"app"}"#).unwrap();
        assert_eq!(manifest_category(&dir), "app");
        // 未声明 → 默认 system
        std::fs::write(dir.join("extension.json"), r#"{"name":"x"}"#).unwrap();
        assert_eq!(manifest_category(&dir), "system");
        // 未知值 → 默认 system
        std::fs::write(dir.join("extension.json"), r#"{"category":"other"}"#).unwrap();
        assert_eq!(manifest_category(&dir), "system");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
