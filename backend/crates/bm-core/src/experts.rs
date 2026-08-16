//! 专家预设（设置架构 §六）：`~/.boenmind/agents/*.md` 的管理面。
//!
//! 专家 = 管家指派给 APP 的"工作人格"，是 subagent 角色（AgentDefinition）
//! 的超集：角色提示词 + 模型 + 工具子集 + 扩展子集 + 记忆桶。与 subagent
//! 同池——子代理派工与 APP 专家读同一批文件；subagent 解析只读已知字段，
//! 本模块的扩展字段（extensions/memory）由专家设置页读写，互不干扰。
//!
//! 文件格式 = frontmatter + Markdown 正文（同 pi agents/*.md 约定）：
//! ```markdown
//! ---
//! name: architect
//! description: 架构师：… 
//! tools: read,grep,find,ls,write
//! model: provider::model   # 可选
//! extensions: web-search   # 可选：允许的扩展 id（逗号分隔）
//! memory: project          # 可选：记忆桶
//! ---
//! <角色提示词正文>
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::config::{AppConfig, agents_dir};
use crate::error::AppError;

/// 预置专家（出厂自带，禁删；与 `default` 同池）。
/// 2026-08-16 统一 APP 前缀（用户定调）：编程专家 = `coding-*`，
/// 避免未来多 APP 撞名（如 WIKI 的审查者 = `wiki-reviewer`）。
pub const BUILTIN_EXPERTS: [&str; 3] = [
    "coding-architect",
    "coding-coder",
    "coding-reviewer",
];

/// 旧名 → 新名（2026-08-16 前缀迁移；default 保持原名不过前缀）。
const LEGACY_BUILTIN: [(&str, &str); 3] = [
    ("architect", "coding-architect"),
    ("coder", "coding-coder"),
    ("reviewer", "coding-reviewer"),
];

/// 记忆桶默认名：专家未显式指定时自动绑定 = 专家名（用户定调：
/// "我只产角色，不关心桶命名，你自已绑定好就行了"——表单不出现
/// 记忆桶字段；删除专家保留桶（记忆资产），使用记录写 usage.json）。
fn auto_bucket_for(name: &str) -> String {
    name.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpertDef {
    /// 文件名（也是启用列表中的 id）
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub reasoning: Option<String>,
    /// 工具子集（逗号分隔；None = 全部工具）
    pub tools: Option<Vec<String>>,
    /// 允许的扩展子集（插件/SKILL id；None/空 = 不限制）
    pub extensions: Option<Vec<String>>,
    /// 记忆桶（None/空 = 默认）
    pub memory: Option<String>,
    /// 角色提示词正文
    pub system_prompt: String,
    /// 是否出厂预置（禁删）
    pub builtin: bool,
}

/// frontmatter 的已知键（其余键原样保留，编辑不丢失用户手写字段）
const KNOWN_KEYS: [&str; 8] = [
    "name",
    "description",
    "model",
    "reasoning",
    "thinking",
    "tools",
    "extensions",
    "memory",
];

fn experts_dir() -> PathBuf {
    agents_dir().join("agents")
}

fn expert_path(id: &str) -> PathBuf {
    experts_dir().join(format!("{id}.md"))
}

fn is_valid_expert_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && id != "."
        && id != ".."
}

/// 解析 frontmatter（`---\nkey: value\n---`）为键值表 + 正文。
fn parse_frontmatter(text: &str) -> (BTreeMap<String, String>, String) {
    let mut fields = BTreeMap::new();
    let mut rest = text.strip_prefix("---").unwrap_or(text).lines();
    let mut body = Vec::new();
    let mut in_frontmatter = text.starts_with("---");
    for line in rest.by_ref() {
        if in_frontmatter {
            let line = line.trim();
            if line == "---" {
                in_frontmatter = false;
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                fields.insert(k.trim().to_string(), clean_yaml_value(v));
            }
        } else {
            body.push(line);
        }
    }
    (fields, body.join("\n"))
}

fn clean_yaml_value(v: &str) -> String {
    v.trim().trim_matches(['"', '\'', ',', '`']).trim().to_string()
}

/// 逗号分隔值 → 列表（空/None → None）。
fn csv_list(v: Option<&String>) -> Option<Vec<String>> {
    let list: Vec<String> = v
        .map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect())
        .unwrap_or_default();
    (!list.is_empty()).then_some(list)
}

fn csv_join(list: &Option<Vec<String>>) -> Option<String> {
    list.as_ref().map(|l| l.join(","))
}

/// 读取全部专家预设（按文件名排序）。
pub fn list_experts() -> Result<Vec<ExpertDef>, std::io::Error> {
    let dir = experts_dir();
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("md")) {
            continue;
        }
        if let Ok(def) = parse_expert_file(&path) {
            out.push(def);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// 读取单个专家预设（不存在 → Err）。
pub fn read_expert(id: &str) -> Result<ExpertDef, AppError> {
    if !is_valid_expert_id(id) {
        return Err(AppError::invalid(format!("非法的专家 id: {id}")));
    }
    let path = expert_path(id);
    if !path.is_file() {
        return Err(AppError::invalid(format!("专家 {id} 不存在")));
    }
    parse_expert_file(&path).map_err(|e| AppError::internal(e.to_string()))
}

fn parse_expert_file(path: &std::path::Path) -> Result<ExpertDef, std::io::Error> {
    let raw = fs::read_to_string(path)?;
    let (fields, body) = parse_frontmatter(&raw);
    let name = fields.get("name").cloned().unwrap_or_default();
    let builtin = BUILTIN_EXPERTS.contains(&name.as_str()) || name == "default";
    Ok(ExpertDef {
        description: fields.get("description").cloned().unwrap_or_default(),
        model: fields.get("model").cloned(),
        reasoning: fields
            .get("reasoning")
            .or_else(|| fields.get("thinking"))
            .cloned(),
        tools: csv_list(fields.get("tools")),
        extensions: csv_list(fields.get("extensions")),
        memory: fields.get("memory").cloned(),
        system_prompt: body.trim().to_string(),
        name,
        builtin,
    })
}

/// 写专家预设（不存在则创建）。未知 frontmatter 字段保留。
/// 2026-08-16：描述字段不再强制（名称即描述）；记忆桶自动绑定 =
/// 专家名（用户定调"你自己绑定好"）。
pub fn write_expert(def: &ExpertDef) -> Result<(), AppError> {
    if !is_valid_expert_id(&def.name) {
        return Err(AppError::invalid("非法的专家 id"));
    }
    fs::create_dir_all(experts_dir())?;
    let path = expert_path(&def.name);
    // 保留已有文件的未知字段
    let mut fields = BTreeMap::new();
    if let Ok(text) = fs::read_to_string(&path) {
        let (raw, _) = parse_frontmatter(&text);
        for (k, v) in raw {
            if !KNOWN_KEYS.contains(&k.as_str()) {
                fields.insert(k, v);
            }
        }
    }
    fields.insert("name".to_string(), def.name.clone());
    if def.description.is_empty() {
        fields.remove("description");
    } else {
        fields.insert("description".to_string(), def.description.clone());
    }
    if let Some(m) = &def.model {
        fields.insert("model".to_string(), m.clone());
    } else {
        fields.remove("model");
    }
    if let Some(r) = &def.reasoning {
        fields.insert("reasoning".to_string(), r.clone());
    } else {
        fields.remove("reasoning");
        fields.remove("thinking");
    }
    if let Some(t) = csv_join(&def.tools) {
        fields.insert("tools".to_string(), t);
    } else {
        fields.remove("tools");
    }
    if let Some(e) = csv_join(&def.extensions) {
        fields.insert("extensions".to_string(), e);
    } else {
        fields.remove("extensions");
    }
    // 记忆桶：自动绑定 = 专家名（用户不关心命名）
    fields.insert(
        "memory".to_string(),
        def.memory
            .clone()
            .unwrap_or_else(|| auto_bucket_for(&def.name)),
    );
    let mut text = String::from("---\n");
    for (k, v) in &fields {
        text.push_str(&format!("{k}: {v}\n"));
    }
    text.push_str("---\n\n");
    text.push_str(def.system_prompt.trim());
    text.push('\n');
    fs::write(&path, text)?;
    Ok(())
}

/// 删除专家预设（预置禁删）。删除前记录记忆桶使用档案
/// （memory/usage.json——桶文件保留，记忆资产不随角色删除）；
/// 记忆管理插件（二期）据档案做桶的浏览/迁移/清理。
pub fn delete_expert(id: &str) -> Result<(), AppError> {
    if !is_valid_expert_id(id) {
        return Err(AppError::invalid(format!("非法的专家 id: {id}")));
    }
    if BUILTIN_EXPERTS.contains(&id) || id == "default" {
        return Err(AppError::invalid(format!("预置专家 {id} 不可删除")));
    }
    let path = expert_path(id);
    if !path.is_file() {
        return Err(AppError::invalid(format!("专家 {id} 不存在")));
    }
    if let Ok(def) = read_expert(id) {
        let bucket = def.memory.unwrap_or_else(|| auto_bucket_for(id));
        record_bucket_usage(&bucket, id);
    }
    fs::remove_file(&path)?;
    Ok(())
}

/// 记忆桶使用档案（memory/usage.json）：桶 → 用过它的角色与删除时间。
/// 角色删除时追加；桶文件不动（记忆资产保留）。
pub fn record_bucket_usage(bucket: &str, expert: &str) {
    let path = crate::config::app_dir().join("memory").join("usage.json");
    let mut usage: serde_json::Value = fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let arr = usage
        .get_mut(bucket)
        .and_then(|v| v.as_array_mut())
        .map(std::mem::take)
        .unwrap_or_default();
    let mut arr = arr;
    let deleted_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    arr.push(serde_json::json!({
        "expert": expert,
        "deleted_at": deleted_at,
    }));
    usage[bucket] = serde_json::Value::Array(arr);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&usage) {
        let _ = fs::write(path, text);
    }
}

/// 预置专家前缀迁移（2026-08-16，幂等）：旧名文件
/// `architect.md` → `coding-architect.md`（frontmatter name 同步），
/// config `[apps.*].expert` 引用一并更新。返回是否发生变更（调用方
/// 负责 save config）。启动时执行一次即可。
pub fn migrate_builtin_experts(config: &mut AppConfig) -> Result<bool, AppError> {
    let mut changed = false;
    for (old, new) in LEGACY_BUILTIN {
        let old_path = expert_path(old);
        if old_path.is_file() {
            let def = parse_expert_file(&old_path)
                .map_err(|e| AppError::internal(e.to_string()))?;
            let mut def = def;
            def.name = new.to_string();
            // 写新文件（memory 自动绑定新名桶）再删旧
            write_expert(&def)?;
            fs::remove_file(&old_path)?;
            eprintln!("[bm-core] expert migrated: {old} -> {new}");
            changed = true;
        }
        for (_, profile) in config.apps.iter_mut() {
            if profile.expert.as_deref() == Some(old) {
                profile.expert = Some(new.to_string());
                changed = true;
            }
        }
    }
    Ok(changed)
}
