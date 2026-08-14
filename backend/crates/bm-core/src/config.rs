//! 应用配置：模型提供商、默认模型、工作文件夹、主题。
//!
//! 配置以 TOML 存储于 `~/.boenmind/config.toml`，首次启动自动生成默认配置。
//! 模型提供商分为两类来源：
//! - 云提供商（openai / anthropic / gemini）：默认官方端点，可覆盖 base_url
//! - 本地或 OpenAI 兼容端点（ollama / llamacpp / 自定义 openai 兼容）：必须提供 base_url

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::compaction::CompactionConfig;

/// 配置与数据目录名（位于用户主目录下）
pub const APP_DIR: &str = ".boenmind";
/// 配置文件相对 APP_DIR 的路径
pub const CONFIG_FILE: &str = "config.toml";
/// 默认工作文件夹名（位于用户主目录下）
pub const DEFAULT_WORKSPACE_DIR: &str = "BoenMind";
/// pi agent 的全局目录（`~/.boenmind/pi`，agents 角色 + skills 兼容目录）
pub const PI_AGENT_DIR: &str = "pi";

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
    /// 对话执行引擎（前端开关持久化）："bm" = 自研 ReactLoopAgent（事件日志
    /// 原生/压缩可审计）；"pi" = 上游 pi SDK 引擎。None = 跟随默认（当前 pi，
    /// 切换拍板后反转）。BM_LOOP_ENGINE 环境变量优先（双开对比调试通道）。
    #[serde(default)]
    pub loop_engine: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// 稳定标识（settings 中用于引用）
    pub id: String,
    /// 展示名称
    pub name: String,
    pub kind: ProviderKind,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// OpenAI 官方端点（或兼容端点覆盖）
    Openai,
    Anthropic,
    Gemini,
    Ollama,
    Llamacpp,
    /// MiniMax 国内版（OpenAI 兼容，api.minimaxi.com）
    Minimax,
    Deepseek,
    Openrouter,
    /// Kimi（Moonshot，api.moonshot.cn）
    Moonshot,
    /// 智谱 GLM（open.bigmodel.cn）
    Zhipu,
    /// 阿里云百炼 Qwen（dashscope 兼容模式）
    Qwen,
    /// xAI Grok
    Xai,
    /// Z.AI 国际版（api.z.ai）
    Zai,
    Groq,
    Mistral,
    Together,
    Cerebras,
    Fireworks,
    Huggingface,
    Nvidia,
    /// 小米 MiMo
    Xiaomi,
    /// 蚂蚁 AntLing
    Antling,
    Baseten,
    /// 任意 OpenAI 兼容服务（自定义端点）
    Custom,
}

impl ProviderKind {
    /// 全部枚举变体。新增 kind 时同步更新（遍历生成官方端点表下发前端，
    /// 测试也依赖它做全覆盖断言）。
    pub const ALL: [ProviderKind; 24] = [
        ProviderKind::Openai,
        ProviderKind::Anthropic,
        ProviderKind::Gemini,
        ProviderKind::Ollama,
        ProviderKind::Llamacpp,
        ProviderKind::Minimax,
        ProviderKind::Deepseek,
        ProviderKind::Openrouter,
        ProviderKind::Moonshot,
        ProviderKind::Zhipu,
        ProviderKind::Qwen,
        ProviderKind::Xai,
        ProviderKind::Zai,
        ProviderKind::Groq,
        ProviderKind::Mistral,
        ProviderKind::Together,
        ProviderKind::Cerebras,
        ProviderKind::Fireworks,
        ProviderKind::Huggingface,
        ProviderKind::Nvidia,
        ProviderKind::Xiaomi,
        ProviderKind::Antling,
        ProviderKind::Baseten,
        ProviderKind::Custom,
    ];

    /// pi agent 注册表中的提供商名（模型路由名称兼容层）。
    ///
    /// 大部分新提供商与 pi 内置注册表同名（groq/mistral/xai/…），
    /// 名称映射保持稳定供子代理/插件按名解析；custom 类使用稳定前缀 +
    /// 提供商 id 保证唯一。pi 已废除，此映射仅为既有数据兼容而保留。
    pub fn pi_name(&self, provider_id: &str) -> String {
        match self {
            ProviderKind::Openai => "openai".to_string(),
            ProviderKind::Anthropic => "anthropic".to_string(),
            ProviderKind::Gemini => "gemini".to_string(),
            ProviderKind::Ollama => "ollama".to_string(),
            ProviderKind::Llamacpp => "llamacpp".to_string(),
            ProviderKind::Minimax => "minimax".to_string(),
            ProviderKind::Deepseek => "deepseek".to_string(),
            ProviderKind::Openrouter => "openrouter".to_string(),
            ProviderKind::Moonshot => "moonshotai".to_string(),
            ProviderKind::Zhipu => "zhipu".to_string(),
            ProviderKind::Qwen => "qwen".to_string(),
            ProviderKind::Xai => "xai".to_string(),
            ProviderKind::Zai => "zai".to_string(),
            ProviderKind::Groq => "groq".to_string(),
            ProviderKind::Mistral => "mistral".to_string(),
            ProviderKind::Together => "together".to_string(),
            ProviderKind::Cerebras => "cerebras".to_string(),
            ProviderKind::Fireworks => "fireworks".to_string(),
            ProviderKind::Huggingface => "huggingface".to_string(),
            ProviderKind::Nvidia => "nvidia".to_string(),
            ProviderKind::Xiaomi => "xiaomi".to_string(),
            ProviderKind::Antling => "ant-ling".to_string(),
            ProviderKind::Baseten => "baseten".to_string(),
            ProviderKind::Custom => format!("custom-{}", provider_id),
        }
    }

    /// 是否走 OpenAI 兼容自定义路由（openai-completions 形状，非原生 API）
    pub fn is_openai_compatible_route(&self) -> bool {
        matches!(
            self,
            ProviderKind::Minimax
                | ProviderKind::Deepseek
                | ProviderKind::Openrouter
                | ProviderKind::Moonshot
                | ProviderKind::Zhipu
                | ProviderKind::Qwen
                | ProviderKind::Xai
                | ProviderKind::Zai
                | ProviderKind::Groq
                | ProviderKind::Mistral
                | ProviderKind::Together
                | ProviderKind::Cerebras
                | ProviderKind::Fireworks
                | ProviderKind::Huggingface
                | ProviderKind::Nvidia
                | ProviderKind::Xiaomi
                | ProviderKind::Antling
                | ProviderKind::Baseten
                | ProviderKind::Custom
        )
    }
}

/// 字符串 → ProviderKind（serde kebab-case 反序列化驱动，与 config.toml 同一来源）。
/// 路由层把用户输入的 kind 解析为枚举后，拼写/大小写错误在此处被显式拒绝，
/// 不会像旧签名（收裸字符串）那样静默落入 custom 语义。
impl std::str::FromStr for ProviderKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use serde::Deserialize;
        Self::deserialize(serde::de::value::StringDeserializer::<
            serde::de::value::Error,
        >::new(s.to_string()))
        .map_err(|_| format!("未知提供商类型: {s}"))
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

pub fn pi_agent_dir() -> PathBuf {
    app_dir().join(PI_AGENT_DIR)
}

/// 预置子代理角色定义（`<pi_agent_dir>/agents/*.md`，上游 subagent 工具的
/// 角色来源）。仅创建目录与首个 `default.md`，已存在/用户自定义的绝不覆盖。
pub fn ensure_builtin_agents() -> Result<(), std::io::Error> {
    use std::io::Write;

    let dir = pi_agent_dir().join("agents");
    fs::create_dir_all(&dir)?;
    let default_path = dir.join("default.md");
    if default_path.exists() {
        return Ok(());
    }
    // frontmatter 字段对齐上游 subagents.rs 解析（name/description/tools/model/reasoning）
    let mut f = fs::File::create(&default_path)?;
    f.write_all(DEFAULT_AGENT_DEFINITION.as_bytes())?;
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
            loop_engine: None,
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
    fn provider_kind_pi_name() {
        assert_eq!(ProviderKind::Openai.pi_name("p1"), "openai");
        assert_eq!(ProviderKind::Ollama.pi_name("p1"), "ollama");
        assert_eq!(ProviderKind::Minimax.pi_name("p1"), "minimax");
        assert_eq!(ProviderKind::Custom.pi_name("p-abc"), "custom-p-abc");
        // 迁移自 pi 注册表的映射
        assert_eq!(ProviderKind::Moonshot.pi_name("p1"), "moonshotai");
        assert_eq!(ProviderKind::Antling.pi_name("p1"), "ant-ling");
        assert_eq!(ProviderKind::Zhipu.pi_name("p1"), "zhipu");
        assert_eq!(ProviderKind::Groq.pi_name("p1"), "groq");
        assert!(ProviderKind::Groq.is_openai_compatible_route());
        assert!(ProviderKind::Xai.is_openai_compatible_route());
        assert!(!ProviderKind::Anthropic.is_openai_compatible_route());
    }

    #[test]
    fn provider_kind_from_str() {
        use std::str::FromStr;
        // 合法 kind（与 serde kebab-case 同源）
        assert_eq!(ProviderKind::from_str("openai"), Ok(ProviderKind::Openai));
        assert_eq!(ProviderKind::from_str("llamacpp"), Ok(ProviderKind::Llamacpp));
        assert_eq!(ProviderKind::from_str("custom"), Ok(ProviderKind::Custom));
        // 拼写/大小写错误显式拒绝（旧签名收裸字符串时静默落入 custom 语义）
        assert!(ProviderKind::from_str("OPENAI").is_err());
        assert!(ProviderKind::from_str("open-ai").is_err());
        assert!(ProviderKind::from_str("ollamaa").is_err());
    }

    #[test]
    fn resolve_provider_falls_back_to_default() {
        let config = AppConfig {
            providers: vec![
                ProviderConfig {
                    id: "a".into(),
                    name: "A".into(),
                    kind: ProviderKind::Openai,
                    base_url: None,
                    api_key: None,
                    models: vec!["m1".into()],
                    default_model: None,
                },
                ProviderConfig {
                    id: "b".into(),
                    name: "B".into(),
                    kind: ProviderKind::Ollama,
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
            loop_engine: None,
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
                kind: ProviderKind::Openai,
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
