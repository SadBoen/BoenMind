//! Skill 系统：安装/管理 Agent Skill（SKILL.md 目录）。
//!
//! 目录约定：
//! - 管理目录 `~/.boenmind/skills/<id>/`：安装的 skill 源（统一含 SKILL.md）
//! - pi 目录 `~/.boenmind/pi/skills/<id>/`：启用时同步，pi agent 启动时
//!   自动收集其中的 SKILL.md 并注入会话（无需改 agent 创建逻辑）
//!
//! 数据源：skills.sh 无公开 JSON API，其 sitemap XML 列出全部 skill 地址
//! `https://www.skills.sh/{owner}/{repo}/{skillId}`，可作随机抽取的目录；
//! skill 内容从 GitHub 仓库下载（tarball 解压或 raw 抓取）。

use rand::seq::SliceRandom;
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::{AppConfig, pi_agent_dir};
use crate::http_util::{copy_dir_excluding, http_agent};

/// 管理目录名（位于 ~/.boenmind 下）
pub const SKILLS_DIR: &str = "skills";
/// skills.sh sitemap 分片（全部 skill 的 URL 列表）
const SITEMAP_URLS: [&str; 2] = [
    "https://www.skills.sh/sitemap-skills-1.xml",
    "https://www.skills.sh/sitemap-skills-2.xml",
];
/// sitemap 结果内存缓存时长（避免重复抓取触发 GitHub/站点限流）
const SITEMAP_CACHE_TTL: Duration = Duration::from_secs(300);
/// 仓库内 SKILL.md 的候选路径（按出现概率排序）
const SKILL_MD_PROBES: [&str; 3] = [
    "skills/{id}/SKILL.md",
    "{id}/SKILL.md",
    "SKILL.md",
];

#[derive(Debug, Clone, Serialize)]
pub struct SkillInfo {
    /// 本地目录名（也是启用列表中的 id）
    pub id: String,
    /// SKILL.md frontmatter 的 name（无则回退 id）
    pub name: String,
    pub description: String,
    /// 来源仓库（skills.sh 安装时为 Some）
    pub owner: Option<String>,
    pub repo: Option<String>,
    /// 来源：registry（skills.sh）/ local
    pub source: String,
    pub enabled: bool,
}

/// skills.sh 随机抽取的候选（已抓取描述，尚未安装）。
#[derive(Debug, Clone, Serialize)]
pub struct SkillCandidate {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub owner: String,
    pub repo: String,
    /// skills.sh 页面地址
    pub url: String,
}

/// sitemap 解析出的 skill 引用。
#[derive(Debug, Clone)]
struct SkillRef {
    owner: String,
    repo: String,
    skill_id: String,
}

// ---------------------------------------------------------------------------
// 目录
// ---------------------------------------------------------------------------

/// 管理目录：~/.boenmind/skills
pub fn skills_dir() -> PathBuf {
    crate::config::app_dir().join(SKILLS_DIR)
}

/// pi agent 的 skills 目录：~/.boenmind/pi/skills（pi 启动时自动收集）
pub fn pi_skills_dir() -> PathBuf {
    pi_agent_dir().join(SKILLS_DIR)
}

/// skill id 合法性：仅允许字母数字、中划线、下划线、点（作为目录名安全子集）。
fn is_valid_skill_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && id != "."
        && id != ".."
}

// ---------------------------------------------------------------------------
// SKILL.md 解析
// ---------------------------------------------------------------------------

/// 解析 SKILL.md frontmatter（`---\nkey: value\n---`），取 name/description。
fn parse_frontmatter(text: &str) -> (String, String) {
    let mut name = String::new();
    let mut description = String::new();
    let mut rest = text.strip_prefix("---").unwrap_or(text).lines();
    for line in rest.by_ref() {
        let line = line.trim();
        if line == "---" || line.is_empty() {
            if line == "---" {
                break;
            }
            continue;
        }
        // 仅处理 YAML 标量键（避免把正文当 frontmatter）
        if let Some(v) = line.strip_prefix("name:") {
            name = clean_yaml_value(v);
        } else if let Some(v) = line.strip_prefix("description:") {
            description = clean_yaml_value(v);
        }
    }
    (name, description)
}

/// 清理 YAML 标量值：去引号与尾随逗号，截断过长描述。
fn clean_yaml_value(v: &str) -> String {
    let cleaned = v.trim().trim_matches(['"', '\'', ',', '`']).trim().to_string();
    cleaned.chars().take(200).collect()
}

/// 读取 skill 目录的描述信息。
fn describe_skill_dir(dir: &Path, fallback: &str) -> (String, String) {
    let md = dir.join("SKILL.md");
    if let Ok(text) = fs::read_to_string(&md) {
        let (name, desc) = parse_frontmatter(&text);
        let name = if name.is_empty() { fallback.to_string() } else { name };
        return (name, if desc.is_empty() { fallback.to_string() } else { desc });
    }
    (fallback.to_string(), fallback.to_string())
}

// ---------------------------------------------------------------------------
// HTTP（同步，调用方应放 spawn_blocking；agent 见 http_util）
// ---------------------------------------------------------------------------

/// GitHub raw 内容（默认分支 HEAD）；404 等错误返回 None。
fn fetch_github_raw(owner: &str, repo: &str, path: &str) -> Option<String> {
    let url = format!("https://raw.githubusercontent.com/{owner}/{repo}/HEAD/{path}");
    let resp = http_agent()
        .get(&url)
        .header("User-Agent", "BoenMind")
        .call()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    // 只读前 64KB（SKILL.md 的 frontmatter 与开头通常足够）
    let mut body = String::new();
    resp.into_body()
        .into_reader()
        .take(64 * 1024)
        .read_to_string(&mut body)
        .ok()?;
    Some(body)
}

/// 从 GitHub 探测并抓取 skill 的 SKILL.md 内容。
fn fetch_skill_md(owner: &str, repo: &str, skill_id: &str) -> Option<String> {
    for probe in SKILL_MD_PROBES {
        let rel = probe.replace("{id}", skill_id);
        if let Some(text) = fetch_github_raw(owner, repo, &rel) {
            return Some(text);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 列表
// ---------------------------------------------------------------------------

/// 扫描管理目录返回已安装 skill 列表。
pub fn list_skills(config: &AppConfig) -> Result<Vec<SkillInfo>, std::io::Error> {
    let dir = skills_dir();
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() || !path.join("SKILL.md").is_file() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let (name, description) = describe_skill_dir(&path, &id);
        let meta = read_meta(&path);
        out.push(SkillInfo {
            id: id.clone(),
            name,
            description,
            owner: meta.as_ref().and_then(|m| (!m.owner.is_empty()).then(|| m.owner.clone())),
            repo: meta.as_ref().and_then(|m| (!m.repo.is_empty()).then(|| m.repo.clone())),
            source: meta.map(|m| m.source).unwrap_or_else(|| "local".to_string()),
            enabled: config.enabled_skills.contains(&id),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

// ---------------------------------------------------------------------------
// 安装
// ---------------------------------------------------------------------------

/// 从 skills.sh（GitHub 仓库）安装 skill。
pub fn install_skill_from_github(owner: &str, repo: &str, skill_id: &str) -> Result<SkillInfo, String> {
    if !is_valid_skill_id(skill_id) {
        return Err(format!("非法的 skill id: {skill_id}"));
    }
    if !is_valid_skill_id(owner) || !is_valid_skill_id(repo) {
        return Err(format!("非法的仓库名: {owner}/{repo}"));
    }

    // 1. 下载仓库 tarball 到临时目录。
    //    不查 GitHub API（未认证限流 60 req/h），直接试 main → master 两个常用分支
    let tmp = std::env::temp_dir().join(format!("bm-skill-{}-{}", std::process::id(), skill_id));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let archive_path = tmp.join("repo.tar.gz");
    let mut archive_body: Option<Vec<u8>> = None;
    for branch in ["main", "master"] {
        let tar_url = format!("https://codeload.github.com/{owner}/{repo}/tar.gz/{branch}");
        let download = (|| -> Result<Option<Vec<u8>>, String> {
            let resp = http_agent()
                .get(&tar_url)
                .header("User-Agent", "BoenMind")
                .call()
                .map_err(|e| format!("下载仓库失败: {e}"))?;
            match resp.status().as_u16() {
                // read_to_vec 默认 10MB 上限，聚合类仓库（如 agentic-awesome-skills）会超限
                200 => resp
                    .into_body()
                    .into_with_config()
                    .limit(128 * 1024 * 1024)
                    .read_to_vec()
                    .map(Some)
                    .map_err(|e| format!("读取仓库数据失败: {e}")),
                404 => Ok(None), // 分支不存在，尝试下一个
                code => Err(format!("下载仓库失败: HTTP {code}")),
            }
        })();
        match download {
            Ok(Some(body)) => {
                archive_body = Some(body);
                break;
            }
            Ok(None) => continue,
            Err(err) => {
                let _ = fs::remove_dir_all(&tmp);
                return Err(err);
            }
        }
    }
    let body = archive_body.ok_or_else(|| format!("仓库 {owner}/{repo} 不存在（main/master 均 404）"))?;
    if let Err(err) = fs::write(&archive_path, &body) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(err.to_string());
    }

    // 2. 解压，定位 skill 目录
    let root = match unpack_tarball(&archive_path, &tmp) {
        Ok(r) => r,
        Err(err) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(format!("解压仓库失败: {err}"));
        }
    };
    let skill_dir = find_skill_dir(&root, skill_id)
        .ok_or_else(|| format!("仓库 {owner}/{repo} 中未找到 skill 目录 {skill_id}（无 SKILL.md）"))?;

    // 3. 复制到管理目录
    let dest = skills_dir().join(skill_id);
    if dest.exists() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("skill {skill_id} 已安装"));
    }
    let result = (|| -> Result<(), String> {
        fs::create_dir_all(skills_dir()).map_err(|e| e.to_string())?;
        copy_dir_excluding(&skill_dir, &dest, &[]).map_err(|e| e.to_string())?;
        write_meta(&dest, owner, repo, skill_id, "registry")
    })();
    let _ = fs::remove_dir_all(&tmp);
    result?;

    let (name, description) = describe_skill_dir(&dest, skill_id);
    Ok(SkillInfo {
        id: skill_id.to_string(),
        name,
        description,
        owner: Some(owner.to_string()),
        repo: Some(repo.to_string()),
        source: "registry".to_string(),
        enabled: false,
    })
}

/// 从本地路径安装 skill：含 SKILL.md 的目录或单个 .md 文件。
pub fn install_skill_from_path(source: &Path) -> Result<SkillInfo, String> {
    let id = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "无效的 skill 路径".to_string())?;
    let id = id.strip_suffix(".md").unwrap_or(id).to_string();
    if !is_valid_skill_id(&id) {
        return Err(format!("非法的 skill id: {id}"));
    }
    let dest = skills_dir().join(&id);
    if dest.exists() {
        return Err(format!("skill {id} 已安装"));
    }
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    if source.is_dir() {
        if !source.join("SKILL.md").is_file() && !source.join(format!("{id}.md")).is_file() {
            let _ = fs::remove_dir_all(&dest);
            return Err("目录中未找到 SKILL.md".to_string());
        }
        copy_dir_excluding(source, &dest, &[]).map_err(|e| e.to_string())?;
        // 目录布局与 skills.sh 不同时补一层 SKILL.md
        if !dest.join("SKILL.md").is_file() {
            fs::copy(source.join(format!("{id}.md")), dest.join("SKILL.md")).map_err(|e| e.to_string())?;
        }
        write_meta(&dest, "", "", &id, "local")
    } else {
        if !id.ends_with(".md") && !source.extension().is_some_and(|e| e == "md") {
            let _ = fs::remove_dir_all(&dest);
            return Err("仅支持 .md 文件或含 SKILL.md 的目录".to_string());
        }
        fs::copy(source, dest.join("SKILL.md")).map_err(|e| e.to_string())?;
        write_meta(&dest, "", "", &id, "local")
    }
    .map_err(|e| e.to_string())?;

    let (name, description) = describe_skill_dir(&dest, &id);
    Ok(SkillInfo {
        id,
        name,
        description,
        owner: None,
        repo: None,
        source: "local".to_string(),
        enabled: false,
    })
}

/// 从 tarball 解压出仓库根目录（安全解压：拒绝绝对路径与 `..`，解压总量限 512MB
/// 防 zip-bomb——下载限 128MB 的压缩包可膨胀到 GB 级撑满磁盘）。
fn unpack_tarball(archive_path: &Path, tmp: &Path) -> Result<PathBuf, String> {
    const MAX_UNPACK_BYTES: u64 = 512 * 1024 * 1024;
    let file = fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let base = tmp.join("repo");
    let entries = archive.entries().map_err(|e| e.to_string())?;
    let mut total: u64 = 0;
    for entry in entries {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_path_buf();
        // GitHub tarball 首个条目是 pax_global_header（tar 全局头），跳过
        if path.file_name().is_some_and(|n| n == "pax_global_header") {
            continue;
        }
        // 逐个组件校验：仅允许 Normal（拒绝绝对路径 / `..` / 符号链接逃逸）
        let mut components = path.components();
        let Some(std::path::Component::Normal(top)) = components.next() else {
            continue;
        };
        let mut target = base.join(top);
        for comp in components {
            let std::path::Component::Normal(name) = comp else {
                return Err("tarball 含非法路径".to_string());
            };
            target.push(name);
        }
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        } else if entry.header().entry_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = fs::File::create(&target).map_err(|e| e.to_string())?;
            let n = std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            total += n;
            if total > MAX_UNPACK_BYTES {
                return Err("tarball 解压超限（>512MB）".to_string());
            }
        }
    }
    // 仓库根 = base 下第一个目录（tarball 根为 {repo}-{branch}/）
    let mut read = fs::read_dir(&base).map_err(|e| e.to_string())?;
    read.find_map(|e| e.ok().map(|e| e.path()).filter(|p| p.is_dir()))
        .ok_or_else(|| "仓库内容为空".to_string())
}

/// 在仓库根下探测 skill 目录：skills/{id}/、{id}/、根。
fn find_skill_dir(root: &Path, skill_id: &str) -> Option<PathBuf> {
    for probe in SKILL_MD_PROBES {
        let rel = probe.replace("{id}", skill_id);
        let md = root.join(&rel);
        if md.is_file() {
            return md.parent().map(Path::to_path_buf);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 来源元数据（.bm-meta.json，不入 pi 同步）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct SkillMeta {
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub skill_id: String,
    /// registry / local
    #[serde(default)]
    pub source: String,
}

fn meta_path(dir: &Path) -> PathBuf {
    dir.join(".bm-meta.json")
}

fn write_meta(dir: &Path, owner: &str, repo: &str, skill_id: &str, source: &str) -> Result<(), String> {
    let meta = SkillMeta {
        owner: owner.to_string(),
        repo: repo.to_string(),
        skill_id: skill_id.to_string(),
        source: source.to_string(),
    };
    let text = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    fs::write(meta_path(dir), text).map_err(|e| e.to_string())
}

fn read_meta(dir: &Path) -> Option<SkillMeta> {
    fs::read_to_string(meta_path(dir))
        .ok()
        .and_then(|t| serde_json::from_str::<SkillMeta>(&t).ok())
}

// ---------------------------------------------------------------------------
// 启停 / 卸载
// ---------------------------------------------------------------------------

/// 启用/禁用 skill：更新 config，并把目录同步到/移出 pi 的 skills 目录
/// （pi 启动时会话只收集 pi/skills 下的 SKILL.md，目录即开关）。
pub fn set_skill_enabled(config: &mut AppConfig, id: &str, enabled: bool) -> Result<(), String> {
    let src = skills_dir().join(id);
    if !src.join("SKILL.md").is_file() {
        return Err(format!("skill {id} 未安装"));
    }
    let pi_dir = pi_skills_dir().join(id);
    if enabled {
        fs::create_dir_all(&pi_dir).map_err(|e| e.to_string())?;
        copy_dir_excluding(&src, &pi_dir, &[".bm-meta.json"]).map_err(|e| e.to_string())?;
        if !config.enabled_skills.iter().any(|s| s == id) {
            config.enabled_skills.push(id.to_string());
        }
    } else {
        let _ = fs::remove_dir_all(&pi_dir);
        config.enabled_skills.retain(|s| s != id);
    }
    crate::config::save(config).map_err(|e| e.to_string())
}

/// 按配置收敛 pi/skills 目录（`put_config` 直接替换 enabled_skills 时调用，
/// 与 set_skill_enabled 的启停语义保持一致）：
/// - 启用且已安装的：确保 pi 目录存在（不存在则复制）
/// - 未启用/已卸载的：移出 pi 目录
pub fn sync_skills_to_pi(config: &AppConfig) -> Result<(), String> {
    // 1. 移除 pi 目录中不在启用列表的 skill（含已卸载残留）
    if let Ok(read) = fs::read_dir(pi_skills_dir()) {
        for entry in read.flatten() {
            let id = entry.file_name().to_string_lossy().to_string();
            if !config.enabled_skills.iter().any(|s| s == &id) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
    // 2. 启用且已安装的补入 pi 目录（幂等：已存在即跳过）
    for id in &config.enabled_skills {
        let src = skills_dir().join(id);
        if !src.join("SKILL.md").is_file() {
            continue; // 未安装：配置里的悬空 id 不产生目录
        }
        let pi_dir = pi_skills_dir().join(id);
        if pi_dir.exists() {
            continue;
        }
        fs::create_dir_all(&pi_dir).map_err(|e| e.to_string())?;
        copy_dir_excluding(&src, &pi_dir, &[".bm-meta.json"]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 卸载：删除管理目录、pi 目录，并移出启用列表。
pub fn uninstall_skill(config: &mut AppConfig, id: &str) -> Result<(), String> {
    let dir = skills_dir().join(id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_dir_all(pi_skills_dir().join(id));
    config.enabled_skills.retain(|s| s != id);
    crate::config::save(config).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// skills.sh 随机抽取
// ---------------------------------------------------------------------------

/// 抓取 sitemap 并解析全部 skill 引用（带内存缓存）。
fn fetch_all_skill_refs() -> Result<Vec<SkillRef>, String> {
    static CACHE: Mutex<Option<(Instant, Vec<SkillRef>)>> = Mutex::new(None);
    {
        let cache = CACHE.lock().unwrap();
        if let Some((at, refs)) = &*cache
            && at.elapsed() < SITEMAP_CACHE_TTL {
                return Ok(refs.clone());
            }
    }
    let mut refs = Vec::new();
    for url in SITEMAP_URLS {
        let resp = http_agent()
            .get(url)
            .header("User-Agent", "BoenMind")
            .call()
            .map_err(|e| format!("抓取 skills.sh 目录失败: {e}"))?;
        let body = resp
            .into_body()
            .into_with_config()
            .limit(64 * 1024 * 1024) // sitemap 全量目录可能超默认 10MB
            .read_to_string()
            .map_err(|e| format!("读取目录失败: {e}"))?;
        parse_sitemap_urls(&body, &mut refs);
    }
    if refs.is_empty() {
        return Err("skills.sh 目录为空（网络或站点问题）".to_string());
    }
    *CACHE.lock().unwrap() = Some((Instant::now(), refs.clone()));
    Ok(refs)
}

/// 从 sitemap XML 提取 `https://www.skills.sh/{owner}/{repo}/{skillId}`。
fn parse_sitemap_urls(xml: &str, out: &mut Vec<SkillRef>) {
    for tag in xml.split("<loc>").skip(1) {
        let Some(url) = tag.split("</loc>").next() else { continue };
        let Some(rest) = url.trim().strip_prefix("https://www.skills.sh/") else {
            continue;
        };
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
            continue;
        }
        let owner = parts[0];
        let repo = parts[1];
        let skill_id = parts[2];
        if is_valid_skill_id(skill_id) {
            out.push(SkillRef {
                owner: owner.to_string(),
                repo: repo.to_string(),
                skill_id: skill_id.to_string(),
            });
        }
    }
}

/// 从 skills.sh 随机抽取 count 个 skill 候选（抓取名称/描述，失败项剔除）。
pub fn random_skills(count: usize) -> Result<Vec<SkillCandidate>, String> {
    let all = fetch_all_skill_refs()?;
    let count = count.clamp(1, 20);
    let mut rng = rand::thread_rng();
    let picked: Vec<&SkillRef> = all.choose_multiple(&mut rng, count.min(all.len())).collect();

    let mut out = Vec::new();
    for r in picked {
        let (name, description) = match fetch_skill_md(&r.owner, &r.repo, &r.skill_id) {
            Some(text) => {
                let (n, d) = parse_frontmatter(&text);
                let n = if n.is_empty() { r.skill_id.clone() } else { n };
                let d = if d.is_empty() { format!("来自 {}/{} 的 skill", r.owner, r.repo) } else { d };
                (n, d)
            }
            None => (r.skill_id.clone(), format!("来自 {}/{} 的 skill", r.owner, r.repo)),
        };
        out.push(SkillCandidate {
            skill_id: r.skill_id.clone(),
            name,
            description,
            owner: r.owner.clone(),
            repo: r.repo.clone(),
            url: format!("https://www.skills.sh/{}/{}/{}", r.owner, r.repo, r.skill_id),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 会话注入（pi CLI 同款格式：available_skills 块拼入 system prompt）
// ---------------------------------------------------------------------------

/// 生成启用的 skill 的注入文本（追加到 system prompt）。
/// 格式与 pi CLI 的 `format_skills_for_prompt` 一致，location 指向
/// pi/skills 下的 SKILL.md（agent 可用 read 工具加载全文与脚本）。
pub fn enabled_skills_prompt(config: &AppConfig) -> String {
    let Ok(infos) = list_skills(config) else {
        return String::new();
    };
    let enabled: Vec<&SkillInfo> = infos.iter().filter(|s| s.enabled).collect();
    if enabled.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "\n\nThe following skills provide specialized instructions for specific tasks.".to_string(),
        "Use the read tool to load a skill's file when the task matches its description."
            .to_string(),
        "When a skill file references a relative path, resolve it against the skill directory (parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.".to_string(),
        String::new(),
        "<available_skills>".to_string(),
    ];
    for skill in enabled {
        let location = pi_skills_dir().join(&skill.id).join("SKILL.md");
        lines.push("  <skill>".to_string());
        lines.push(format!("    <name>{}</name>", escape_xml(&skill.name)));
        lines.push(format!(
            "    <description>{}</description>",
            escape_xml(&skill.description)
        ));
        lines.push(format!(
            "    <location>{}</location>",
            escape_xml(&location.display().to_string())
        ));
        lines.push("  </skill>".to_string());
    }
    lines.push("</available_skills>".to_string());
    lines.join("\n")
}

fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_dir_under_app_dir() {
        assert!(skills_dir().ends_with(".boenmind/skills"));
        assert!(pi_skills_dir().ends_with(".boenmind/pi/skills"));
    }

    #[test]
    fn skill_id_validation() {
        assert!(is_valid_skill_id("frontend-design"));
        assert!(is_valid_skill_id("a-b_c.d"));
        assert!(!is_valid_skill_id("../evil"));
        assert!(!is_valid_skill_id("a/b"));
        assert!(!is_valid_skill_id(""));
        assert!(!is_valid_skill_id("名字"));
    }

    #[test]
    fn frontmatter_parsing() {
        let md = "---\nname: frontend-design\nslug: fd\ndescription: A skill for building frontends.\n---\n\n# Title\n\nBody text.";
        let (name, desc) = parse_frontmatter(md);
        assert_eq!(name, "frontend-design");
        assert_eq!(desc, "A skill for building frontends.");

        let (n2, d2) = parse_frontmatter("no frontmatter here");
        assert!(n2.is_empty() && d2.is_empty());

        // 带引号的值
        let quoted = "---\ndescription: \"Quoted, value\"\n---\n";
        let (_, d3) = parse_frontmatter(quoted);
        assert_eq!(d3, "Quoted, value");
    }

    #[test]
    fn sitemap_url_parsing() {
        let xml = r#"<?xml version="1.0"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://www.skills.sh/vercel-labs/skills/find-skills</loc></url>
  <url><loc>https://www.skills.sh/anthropics/skills/frontend-design</loc></url>
  <url><loc>https://www.skills.sh/owner-only</loc></url>
  <url><loc>https://www.skills.sh/a/b/c/d</loc></url>
</urlset>"#;
        let mut out = Vec::new();
        parse_sitemap_urls(xml, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].owner, "vercel-labs");
        assert_eq!(out[0].repo, "skills");
        assert_eq!(out[0].skill_id, "find-skills");
        assert_eq!(out[1].skill_id, "frontend-design");
    }

    #[test]
    fn tarball_layout_detection() {
        // 构造三种布局的目录树，验证 find_skill_dir
        let tmp = std::env::temp_dir().join(format!("bm-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("skills/my-skill")).unwrap();
        fs::write(tmp.join("skills/my-skill/SKILL.md"), "---\nname: my-skill\n---\n").unwrap();
        fs::create_dir_all(tmp.join("other-skill")).unwrap();
        fs::write(tmp.join("other-skill/SKILL.md"), "# other\n").unwrap();
        fs::write(tmp.join("SKILL.md"), "# root\n").unwrap();

        assert_eq!(
            find_skill_dir(&tmp, "my-skill").unwrap(),
            tmp.join("skills/my-skill")
        );
        assert_eq!(find_skill_dir(&tmp, "other-skill").unwrap(), tmp.join("other-skill"));
        // 根 SKILL.md 时返回仓库根
        assert_eq!(find_skill_dir(&tmp, "nonexistent").unwrap(), tmp);
        let _ = fs::remove_dir_all(&tmp);
    }
}
