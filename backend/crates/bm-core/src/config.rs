//! 应用配置：模型提供商、默认模型、工作文件夹、主题。
//!
//! 配置以 TOML 存储于 `~/.boenmind/config.toml`，首次启动自动生成默认配置。
//! 模型提供商分为两类来源：
//! - 云提供商（openai / anthropic / gemini）：默认官方端点，可覆盖 base_url
//! - 本地或 OpenAI 兼容端点（ollama / llamacpp / 自定义 openai 兼容）：必须提供 base_url

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::compaction::CompactionConfig;

/// 配置与数据目录名（位于用户主目录下）
pub const APP_DIR: &str = ".boenmind";
/// 配置文件相对 APP_DIR 的路径
pub const CONFIG_FILE: &str = "config.toml";
/// 默认工作文件夹名（位于用户主目录下）
pub const DEFAULT_WORKSPACE_DIR: &str = "BoenMind";
/// 子代理/兼容目录（`~/.boenmind/agents`，子代理角色 + 兼容 skills）
pub const AGENTS_DIR: &str = "agents";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default = "default_working_dir")]
    pub working_dir: PathBuf,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 界面语言（zh / en / ja / ko）
    #[serde(default = "default_lang")]
    pub lang: String,
    /// 启用的插件（~/.boenmind/extensions 下的扩展 id）
    #[serde(default)]
    pub enabled_plugins: Vec<String>,
    /// 已卸载的内置插件（启动预装时跳过：卸载即彻底删除，不再自动恢复）
    #[serde(default)]
    pub removed_builtin_plugins: Vec<String>,
    /// 启用的 skill（~/.boenmind/skills 下的 skill id，启用时同步到 pi 目录）
    #[serde(default)]
    pub enabled_skills: Vec<String>,
    /// 上下文压缩配置（按模型水线/尾部保护，见 compaction 模块）
    #[serde(default)]
    pub compaction: CompactionConfig,
    /// 插件权限档位（extension_policy）："safe"（默认，关键能力询问）/ "balanced" /
    /// "permissive"（全自动放行）。None = 上游默认档位
    #[serde(default)]
    pub extension_policy: Option<String>,
    /// YOLO 开关：放行危险能力（exec / env）。与 permissive 组合 = 全自动放行
    #[serde(default)]
    pub extension_allow_dangerous: Option<bool>,
    /// 用户批准的系统提示词追加段（refine-suggest 审批生效；拼接在 SYSTEM_PROMPT 之后）
    #[serde(default)]
    pub custom_system_prompt: Option<String>,
    /// MCP server 连接配置（bm-mcp 官方插件）：JSON 数组形态（TOML 内联
    /// 表数组），组装层反序列化为 bm_mcp::McpServerConfig。示例：
    /// ```toml
    /// mcp = [
    ///   { name = "fs", transport = "stdio", command = "node",
    ///     args = ["C:/x/index.js", "D:/work"] },
    /// ]
    /// ```
    #[serde(default)]
    pub mcp: Option<serde_json::Value>,
    /// 插件作用域（设置架构 §八）：插件 id → 生效的 APP 列表。
    /// 空/缺失/含 "*" = 公共（所有 APP 生效）；["chat"] = 仅聊天。
    /// 引擎组装会话工具面时按 session.app 过滤（公共扩展 + 命中扩展）。
    #[serde(default)]
    pub plugin_scopes: HashMap<String, Vec<String>>,
    /// skill 作用域（同上；注入面 = system prompt 的 available_skills 块）
    #[serde(default)]
    pub skill_scopes: HashMap<String, Vec<String>>,
    /// 每 APP 专属配置（设置架构 §五；单源 config.toml 内分段，底层引擎共用一套）：
    /// appId（chat/coding/…）→ 专家绑定/记忆/工作区覆盖
    #[serde(default)]
    pub apps: HashMap<String, AppProfile>,
}

/// 每软件 APP 的专属配置（单源 config.toml 的 `[apps.<id>]` 段）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppProfile {
    /// 该 APP 默认专家预设 id（~/.boenmind/agents/*.md；None = 未绑定）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expert: Option<String>,
    /// 记忆桶（None = APP 默认；如编程 = 按项目分桶）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    /// 工作目录覆盖（None = 全局 working_dir / 项目切换）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

/// 作用域匹配：作用域空/含 `*` = 公共（任何 APP 生效）；否则要求包含 app。
pub fn scope_matches(scopes: &[String], app: &str) -> bool {
    scopes.is_empty() || scopes.iter().any(|s| s == "*" || s == app)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// 稳定标识（settings 中用于引用）
    pub id: String,
    /// 展示名称
    pub name: String,
    pub kind: ProviderKind,
    /// 协议形状（仅 custom/未知厂商生效；None = OpenAI 兼容）。
    /// 内置厂商形状固定（minimax/deepseek = OpenAI 兼容），见 [`ProviderConfig::shape`]。
    #[serde(default)]
    pub shape: Option<ProviderShape>,
    /// API 端点；云提供商留空使用官方端点，本地/兼容端点必填
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub default_model: Option<String>,
}

impl ProviderConfig {
    /// 协议形状：内置厂商固定（minimax/deepseek = OpenAI 兼容），
    /// custom/未知厂商读配置字段（默认 OpenAI 兼容）。
    pub fn shape(&self) -> ProviderShape {
        match self.kind {
            ProviderKind::Minimax | ProviderKind::Deepseek => ProviderShape::OpenaiCompatible,
            ProviderKind::Custom | ProviderKind::Unknown => self.shape.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// MiniMax 国内版（OpenAI 兼容，api.minimaxi.com）
    Minimax,
    /// DeepSeek（api.deepseek.com/v1）
    Deepseek,
    /// 任意 OpenAI/Anthropic/Gemini 兼容服务（用户填端点 + 协议形状；
    /// 厂商插件化的核心价值，必须端到端可用）
    Custom,
    /// 未知 kind：config.toml 里残留的已下线内置厂商（groq/mistral/…）。
    /// 按 custom 语义处理（不炸启动；端点/形状由用户配置决定）——内置
    /// 厂商已精简，需要时以插件/Custom 接回（方案 A，2026-08-16 拍板）。
    #[serde(other)]
    Unknown,
}

/// 厂商 API 协议形状（方言）。内置厂商固定（minimax/deepseek = OpenAI
/// 兼容）；custom 厂商由用户选择。插件协议里"协议形状"即此字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderShape {
    /// OpenAI 兼容（/models、/chat/completions；绝大多数厂商）
    #[default]
    OpenaiCompatible,
    /// Anthropic 方言（x-api-key + anthropic-version 头，/v1/messages）
    Anthropic,
    /// Gemini 方言（v1beta generateContent）
    Gemini,
}

impl ProviderShape {
    pub fn is_openai_compatible(&self) -> bool {
        matches!(self, Self::OpenaiCompatible)
    }
}

impl ProviderKind {
    /// 全部内置变体。新增 kind 时同步更新（遍历生成官方端点表下发前端，
    /// 测试也依赖它做全覆盖断言）。Unknown 不进 ALL（无官方端点、不参与
    /// 前端预设下发，只作为旧配置兼容落点）。
    pub const ALL: [ProviderKind; 3] = [ProviderKind::Minimax, ProviderKind::Deepseek, ProviderKind::Custom];
}

/// 字符串 → ProviderKind（手写 match，与 serde kebab-case 反序列化同一来源）。
/// 路由层把用户输入的 kind 解析为枚举后，拼写/大小写错误在此处被显式拒绝
/// （不会静默落入 custom 语义）——与 config.toml 反序列化不同：配置文件里
/// 残留的已下线 kind 走 `#[serde(other)]` 落到 [`ProviderKind::Unknown`]
/// 按自定义处理（兼容旧配置），而 API 手填拼错是用户错误，显式报 400。
impl std::str::FromStr for ProviderKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "minimax" => Ok(ProviderKind::Minimax),
            "deepseek" => Ok(ProviderKind::Deepseek),
            "custom" => Ok(ProviderKind::Custom),
            _ => Err(format!("未知提供商类型: {s}")),
        }
    }
}

fn default_theme() -> String {
    "system".to_string()
}

fn default_lang() -> String {
    "zh".to_string()
}

/// 数据基础目录：默认用户主目录；服务器部署用 `BOENMIND_HOME` 覆盖
/// （systemd / Docker 可指向 /var/lib/boenmind 等专用目录）。
fn home_base() -> PathBuf {
    std::env::var_os("BOENMIND_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
}

fn default_working_dir() -> PathBuf {
    home_base().join(DEFAULT_WORKSPACE_DIR)
}

pub fn app_dir() -> PathBuf {
    home_base().join(APP_DIR)
}

pub fn config_path() -> PathBuf {
    app_dir().join(CONFIG_FILE)
}

pub fn agents_dir() -> PathBuf {
    app_dir().join(AGENTS_DIR)
}

/// 一次性迁移：pi 引擎时代命名 `~/.boenmind/pi` → `~/.boenmind/agents`
/// （2026-08-15 用户拍板脱钩 pi 语义）。旧目录存在且新目录不存在时整体
/// rename（用户自定义角色/技能随之搬迁）；已迁移/新安装无操作。
pub fn migrate_legacy_agent_dir() -> std::io::Result<()> {
    let old = app_dir().join("pi");
    let new = agents_dir();
    if old.exists() && !new.exists() {
        fs::rename(&old, &new)?;
    }
    Ok(())
}

/// 预置子代理角色/专家定义（`<agents_dir>/agents/*.md`，上游 subagent 工具的
/// 角色来源 + 专家预设池）。逐个创建，已存在/用户自定义的绝不覆盖。
pub fn ensure_builtin_agents() -> Result<(), std::io::Error> {
    use std::io::Write;

    migrate_legacy_agent_dir()?;
    let dir = agents_dir().join("agents");
    fs::create_dir_all(&dir)?;
    let builtins: [(&str, &str); 4] = [
        ("default", DEFAULT_AGENT_DEFINITION),
        ("architect", ARCHITECT_AGENT_DEFINITION),
        ("coder", CODER_AGENT_DEFINITION),
        ("reviewer", REVIEWER_AGENT_DEFINITION),
    ];
    for (name, content) in builtins {
        let path = dir.join(format!("{name}.md"));
        if path.exists() {
            continue;
        }
        // frontmatter 字段对齐上游 subagents.rs 解析（name/description/tools/model/reasoning）
        let mut f = fs::File::create(&path)?;
        f.write_all(content.as_bytes())?;
    }
    Ok(())
}

/// 默认执行者角色定义（开箱可用；用户可在 agents/ 目录新增/修改角色）。
const DEFAULT_AGENT_DEFINITION: &str = r#"---
name: default
description: 通用执行者：在委派的任务范围内独立工作，完成后汇报结果
tools: read,bash,edit,write,grep,find,ls,hashline_edit
---
你是 BoenMind 的执行者（subagent），由主代理委派完成一项具体任务。

职责：
1. 聚焦任务本身，不要自行扩大范围；
2. 必要时使用工具调查或修改工作区文件；
3. 完成后用简洁的结构化文本汇报：结论、关键依据、已完成事项。

输出契约：
- 若主代理在 task 里指定了输出格式（如"最终输出必须是 JSON 对象，含字段 xxx"），严格按其要求交付；
- 未指定时按默认汇报：先给不超过 3 行的人类可读摘要，再附 JSON 块：
  {"summary":"结论","findings":["关键依据"],"done":["已完成事项"],"open":["未决/风险"]}

行为准则：
- 不确定时明确说明，不声称完成未完成的事；
- 不修改工作区 .boenmind 目录下的任何文件。
"#;

/// 编程专家预置①：架构师（方案与设计，只读 + 写方案文档）
const ARCHITECT_AGENT_DEFINITION: &str = r#"---
name: architect
description: 架构师：需求拆解、方案设计、代码结构决策与技术评审。用于编程 APP 的设计与规划任务
tools: read,grep,find,ls,write
---
你是 BoenMind 的架构师专家，负责设计而非实现。

职责：
1. 把需求拆解为可执行的方案：模块划分、数据流、接口契约、里程碑；
2. 评审现有代码结构，指出设计问题与改进路径（含取舍与风险）；
3. 输出简洁的决策记录（选型理由、备选方案、未决项）。

行为准则：
- 只读代码与写方案文档；不直接修改业务实现代码；
- 方案需要落地时，明确指出应交给 coder 专家实现的部分；
- 不确定时明确说明，不臆造接口与假设。
"#;

/// 编程专家预置②：码农（按方案实现，全工具面）
const CODER_AGENT_DEFINITION: &str = r#"---
name: coder
description: 码农：按方案实现代码、修 bug（全工具面，写完跑构建/测试验证）
tools: read,bash,edit,write,grep,find,ls,hashline_edit
---
你是 BoenMind 的码农专家，负责把方案变成可运行的代码。

职责：
1. 按架构师方案或明确需求实现代码，保持既有代码风格；
2. 修 bug 先复现定位，再最小改动修复，并补测试；
3. 完成后跑构建/测试验证，附验证输出摘要。

行为准则：
- 不擅自扩大改动范围；设计取舍不确定时先问主代理；
- 改动遵循"最小可验证"：一次提交一个关注点；
- 不修改工作区 .boenmind 目录下的任何文件。
"#;

/// 编程专家预置③：审查者（质量把关，只读 + 跑测试）
const REVIEWER_AGENT_DEFINITION: &str = r#"---
name: reviewer
description: 审查者：代码审查、测试验证与质量报告（只读 + 跑测试/静态检查）
tools: read,grep,find,ls,bash
---
你是 BoenMind 的审查者专家，负责把质量关。

职责：
1. 审查代码：正确性、边界条件、安全隐患、可维护性、与需求的一致性；
2. 运行测试与静态检查验证改动（只读环境 + 执行验证命令）；
3. 输出结构化审查报告：按严重度列出问题（位置、原因、建议），
   并明确"通过 / 需修复后复审"的结论。

行为准则：
- 只读代码与执行验证命令，不修改任何文件；
- 问题交回实现方修复；复审查看修复是否到位；
- 不确定时明确说明，不放过可疑点也不夸大风险。
"#;

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            default_provider: None,
            default_model: None,
            working_dir: default_working_dir(),
            theme: default_theme(),
            lang: default_lang(),
            enabled_plugins: vec![
                "ctx-compactor".to_string(), // 官方压缩插件默认启用（新用户开箱即用，可禁用/卸载）
                "web-search".to_string(),   // 官方搜索插件默认启用（无 key 时优雅降级，设置页配置后生效）
                "refine-suggest".to_string(), // 官方自我改进建议插件默认启用（仅记录建议，审批后才生效）
            ],
            removed_builtin_plugins: Vec::new(),
            enabled_skills: Vec::new(),
            compaction: CompactionConfig::default(),
            extension_policy: None,
            extension_allow_dangerous: None,
            custom_system_prompt: None,
            mcp: None,
            plugin_scopes: HashMap::new(),
            skill_scopes: HashMap::new(),
            apps: HashMap::new(),
        }
    }
}

/// 加载配置；文件不存在时生成默认配置并落盘。
pub fn load() -> AppConfig {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<AppConfig>(&text) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("[bm-core] 配置解析失败 ({err})，使用默认配置（原文件备份为 {CONFIG_FILE}.bak）");
                // 损坏的配置先备份再覆盖：曾出现用户手写/损坏配置被默认值静默抹掉
                let _ = fs::copy(&path, path.with_extension("toml.bak"));
                let config = AppConfig::default();
                let _ = save(&config);
                config
            }
        },
        Err(_) => {
            let config = AppConfig::default();
            let _ = save(&config);
            config
        }
    }
}

/// 写入文件并收紧权限：config.toml 与 pi keys 含明文 API key，
/// Unix 下仅属主可读写（与 plugin_settings 的密钥文件同标准）。
#[cfg(unix)]
fn write_private(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, content)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    fs::write(path, content)
}

/// 保存配置并确保目录存在。
pub fn save(config: &AppConfig) -> Result<(), std::io::Error> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = toml::to_string_pretty(config).map_err(std::io::Error::other)?;
    write_private(&path, text.as_bytes())
}

/// 确保工作文件夹存在。
pub fn ensure_working_dir(config: &AppConfig) -> Result<(), std::io::Error> {
    fs::create_dir_all(&config.working_dir)
}

/// 查找提供商；id 不存在时回退到 default_provider。
pub fn resolve_provider<'a>(config: &'a AppConfig, provider_id: Option<&str>) -> Option<&'a ProviderConfig> {
    if let Some(id) = provider_id
        && let Some(p) = config.providers.iter().find(|p| p.id == id) {
            return Some(p);
        }
    if let Some(default_id) = &config.default_provider {
        return config.providers.iter().find(|p| p.id == *default_id);
    }
    config.providers.first()
}

/// 解析模型名：提供商默认模型 -> 全局默认模型 -> 提供商第一个模型。
pub fn resolve_model(provider: &ProviderConfig, model: Option<&str>) -> Option<String> {
    model
        .map(str::to_string)
        .or_else(|| provider.default_model.clone())
        .or_else(|| provider.models.first().cloned())
}



/// 测试用共享锁：串行化所有会修改全局 BOENMIND_HOME 的测试（cfg(test) 才存在）。
/// 并行测试共享进程 env，读 app_dir/plugins_dir 的测试在别的测试改 env 时会读到
/// 跳变路径；需要读写真实 ~/.boenmind 下文件的测试应持有此锁。
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_from_str() {
        use std::str::FromStr;
        // 合法 kind（与 serde kebab-case 同源）
        assert_eq!(ProviderKind::from_str("minimax"), Ok(ProviderKind::Minimax));
        assert_eq!(ProviderKind::from_str("deepseek"), Ok(ProviderKind::Deepseek));
        assert_eq!(ProviderKind::from_str("custom"), Ok(ProviderKind::Custom));
        // 已下线的旧 kind（groq/mistral/…）在 API 入口显式拒绝——不静默
        // 落入 custom 语义（手填拼错是错误；config 残留走 serde(other)）
        assert!(ProviderKind::from_str("groq").is_err());
        assert!(ProviderKind::from_str("OPENAI").is_err());
        assert!(ProviderKind::from_str("minimaxx").is_err());
    }

    #[test]
    fn provider_kind_config_compat_unknown_kind() {
        // config.toml 残留已下线 kind 不炸启动：serde(other) → Unknown
        let cfg: AppConfig = toml::from_str(
            r#"
            [[providers]]
            id = "old-groq"
            name = "Groq"
            kind = "groq"
            base_url = "https://api.groq.com/openai/v1"
            api_key = "sk-x"
            models = ["llama-3.3"]
            "#,
        )
        .unwrap();
        let p = &cfg.providers[0];
        assert_eq!(p.kind, ProviderKind::Unknown);
        // 形状默认 OpenAI 兼容（旧配置行为不变）；custom/unknown 可显式覆盖
        assert!(p.shape().is_openai_compatible());
    }

    #[test]
    fn provider_shape_fixed_for_builtin_overridable_for_custom() {
        let builtin = ProviderConfig {
            id: "minimax".into(),
            name: "MiniMax".into(),
            kind: ProviderKind::Minimax,
            // 内置厂商忽略 shape 字段（协议形状固定 OpenAI 兼容）
            shape: Some(ProviderShape::Gemini),
            base_url: None,
            api_key: None,
            models: vec![],
            default_model: None,
        };
        assert!(builtin.shape().is_openai_compatible());

        let custom = ProviderConfig {
            id: "my".into(),
            name: "My".into(),
            kind: ProviderKind::Custom,
            shape: Some(ProviderShape::Anthropic),
            base_url: None,
            api_key: None,
            models: vec![],
            default_model: None,
        };
        assert_eq!(custom.shape(), ProviderShape::Anthropic);

        let custom_default = ProviderConfig {
            id: "my2".into(),
            name: "My2".into(),
            kind: ProviderKind::Custom,
            shape: None,
            base_url: None,
            api_key: None,
            models: vec![],
            default_model: None,
        };
        assert!(custom_default.shape().is_openai_compatible());
    }

    #[test]
    fn resolve_provider_falls_back_to_default() {
        let config = AppConfig {
            plugin_scopes: HashMap::new(),
            skill_scopes: HashMap::new(),
            apps: HashMap::new(),
            providers: vec![
                ProviderConfig {
                    id: "a".into(),
                    name: "A".into(),
                    kind: ProviderKind::Minimax,
                    shape: None,
                    base_url: None,
                    api_key: None,
                    models: vec!["m1".into()],
                    default_model: None,
                },
                ProviderConfig {
                    id: "b".into(),
                    name: "B".into(),
                    kind: ProviderKind::Custom,
                    shape: None,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    api_key: None,
                    models: vec!["qwen".into()],
                    default_model: None,
                },
            ],
            default_provider: Some("b".into()),
            default_model: None,
            working_dir: default_working_dir(),
            theme: "system".into(),
            lang: "zh".into(),
            enabled_plugins: vec![],
            removed_builtin_plugins: vec![],
            enabled_skills: vec![],
            compaction: CompactionConfig::default(),
            extension_policy: None,
            extension_allow_dangerous: None,
            custom_system_prompt: None,
            mcp: None,
        };
        assert_eq!(resolve_provider(&config, Some("missing")).unwrap().id, "b");
        assert_eq!(resolve_model(&config.providers[1], None).unwrap(), "qwen");
        assert_eq!(resolve_model(&config.providers[1], Some("x")).unwrap(), "x");
    }

    #[test]
    fn save_writes_private_permissions() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let original = std::env::var_os("BOENMIND_HOME");
        let dir = std::env::temp_dir().join(format!("bm-config-save-{}", std::process::id()));
        unsafe { std::env::set_var("BOENMIND_HOME", &dir) };
        let config = AppConfig {
            providers: vec![ProviderConfig {
                id: "k".into(),
                name: "K".into(),
                kind: ProviderKind::Minimax,
                shape: None,
                base_url: None,
                api_key: Some("sk-secret".into()),
                models: vec![],
                default_model: None,
            }],
            ..AppConfig::default()
        };
        save(&config).unwrap();
        // config.toml 含明文 API key：Unix 下权限必须为 0600
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(config_path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        match original {
            Some(v) => unsafe { std::env::set_var("BOENMIND_HOME", v) },
            None => unsafe { std::env::remove_var("BOENMIND_HOME") },
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_backs_up_corrupt_config() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let original = std::env::var_os("BOENMIND_HOME");
        let dir = std::env::temp_dir().join(format!("bm-config-load-{}", std::process::id()));
        unsafe { std::env::set_var("BOENMIND_HOME", &dir) };
        let path = config_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "this is not [valid toml").unwrap();
        let config = load();
        assert!(config.providers.is_empty()); // 解析失败回默认
        // 备份保留原损坏内容，未被默认配置覆盖
        let bak = fs::read_to_string(path.with_extension("toml.bak")).unwrap();
        assert_eq!(bak, "this is not [valid toml");
        match original {
            Some(v) => unsafe { std::env::set_var("BOENMIND_HOME", v) },
            None => unsafe { std::env::remove_var("BOENMIND_HOME") },
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
