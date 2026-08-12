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
/// pi agent 的全局目录（models.json 所在），由 PI_CODING_AGENT_DIR 环境变量指定
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
    /// pi agent 注册表中的提供商名。
    ///
    /// 大部分新提供商与 pi 内置注册表同名（groq/mistral/xai/…），models.json
    /// 中同名条目按覆盖写入（baseUrl + openai-completions 路由）；
    /// minimax / deepseek / openrouter 等不在 pi 的常用内置列表中，
    /// 但 pi 的 models.json 支持自定义 provider（`api: "openai-completions"` 路由），
    /// 见 sync_pi_models_json。custom 类使用稳定前缀 + 提供商 id 保证唯一。
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

    /// 是否走 OpenAI 兼容自定义路由（models.json 中需显式 `api: "openai-completions"`）
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
            ],
            removed_builtin_plugins: Vec::new(),
            enabled_skills: Vec::new(),
            compaction: CompactionConfig::default(),
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

/// 将 BoenMind 配置同步为 pi agent 的 models.json。
///
/// - 内置提供商（openai/anthropic/gemini/ollama/llamacpp）：注册 baseUrl 覆盖 + 模型
/// - 自定义 OpenAI 兼容提供商（minimax/deepseek/openrouter/moonshot/…）：
///   以独立 provider 名注册，`api: "openai-completions"` 路由 + 独立 baseUrl，
///   多个 OpenAI 兼容端点可共存互不覆盖
///
/// pi agent 通过 `PI_CODING_AGENT_DIR` 环境变量定位其全局目录，我们指向
/// `~/.boenmind/pi`，与用户自己的 `~/.pi` 配置互不干扰。
pub fn sync_pi_models_json(config: &AppConfig) -> Result<(), std::io::Error> {
    use serde_json::{Map, Value, json};

    let dir = pi_agent_dir();
    fs::create_dir_all(&dir)?;

    let mut providers = Map::new();
    let keys_dir = dir.join("keys");
    for p in &config.providers {
        let name = p.kind.pi_name(&p.id);
        let mut entry = Map::new();
        if let Some(base) = &p.base_url {
            entry.insert("baseUrl".to_string(), json!(base));
        }
        if p.kind.is_openai_compatible_route() {
            // 自定义 OpenAI 兼容路由：必须显式指定 API 类型
            entry.insert("api".to_string(), json!("openai-completions"));
        }
        // API key 通过 file: 引用写入独立文件（pi 官方支持模式，避免凭据落在 models.json）
        if let Some(key) = &p.api_key
            && !key.is_empty() {
                fs::create_dir_all(&keys_dir)?;
                let key_file = keys_dir.join(format!("{name}.key"));
                write_private(&key_file, key.as_bytes())?;
                entry.insert(
                    "apiKey".to_string(),
                    json!(format!("file:{}", key_file.display())),
                );
                // 自定义 provider 无内置 auth metadata，必须显式声明才会携带 Authorization 头
                entry.insert("authHeader".to_string(), json!(true));
            }
        // 注册模型列表（仅注册本提供商声明的模型，避免污染内置目录）
        // 自定义 OpenAI 兼容路由标记 reasoning: true，使 pi 的思考控制
        // （DeepSeek / MiniMax 方言）对该提供商生效
        let reasoning = p.kind.is_openai_compatible_route();
        let models: Vec<Value> = p
            .models
            .iter()
            .map(|m| {
                let mut entry = json!({ "id": m });
                if reasoning {
                    entry["reasoning"] = json!(true);
                }
                // 压缩配置中声明了 context_window 时写入 models.json，
                // 使探测值与模型注册表一致（声明窗口）
                if let Some(window) =
                    crate::compaction::override_window_for_model(&config.compaction, &name, m)
                {
                    entry["contextWindow"] = json!(window);
                }
                entry
            })
            .collect();
        if !models.is_empty() {
            entry.insert("models".to_string(), Value::Array(models));
        }
        providers.insert(name, Value::Object(entry));
    }

    let doc = json!({
        "providers": Value::Object(providers),
    });

    fs::write(dir.join("models.json"), serde_json::to_string_pretty(&doc)?)
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
        };
        assert_eq!(resolve_provider(&config, Some("missing")).unwrap().id, "b");
        assert_eq!(resolve_model(&config.providers[1], None).unwrap(), "qwen");
        assert_eq!(resolve_model(&config.providers[1], Some("x")).unwrap(), "x");
    }

    #[test]
    fn sync_pi_models_json_writes_compaction_window() {
        use crate::compaction::CompactionOverride;
        // 本测试修改全局 BOENMIND_HOME：与其它读取 app_dir 的测试并行会互相污染
        // （曾经导致真实 ~/.boenmind/config.toml 被默认配置覆盖）。用共享锁串行化。
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let original = std::env::var_os("BOENMIND_HOME");
        let dir = std::env::temp_dir().join(format!("bm-config-sync-{}", std::process::id()));
        // 注意：edition 2024 中 set_var/remove_var 为 unsafe
        unsafe { std::env::set_var("BOENMIND_HOME", &dir) };
        let mut config = AppConfig {
            providers: vec![ProviderConfig {
                id: "ds".into(),
                name: "DeepSeek".into(),
                kind: ProviderKind::Deepseek,
                base_url: None,
                api_key: None,
                models: vec!["deepseek-chat".into(), "deepseek-reasoner".into()],
                default_model: None,
            }],
            ..AppConfig::default()
        };
        config.compaction.overrides.insert(
            "deepseek/deepseek-chat".to_string(),
            CompactionOverride { context_window: Some(200_000), ..Default::default() },
        );
        sync_pi_models_json(&config).unwrap();
        let text = std::fs::read_to_string(dir.join(APP_DIR).join("pi").join("models.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        let models = &doc["providers"]["deepseek"]["models"];
        assert_eq!(models[0]["id"], "deepseek-chat");
        assert_eq!(models[0]["contextWindow"], 200_000);
        // 未配置窗口的模型不写字段
        assert!(models[1].get("contextWindow").is_none());
        match original {
            Some(v) => unsafe { std::env::set_var("BOENMIND_HOME", v) },
            None => unsafe { std::env::remove_var("BOENMIND_HOME") },
        }
        std::fs::remove_dir_all(&dir).ok();
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
