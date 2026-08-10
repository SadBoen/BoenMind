//! 应用配置：模型提供商、默认模型、工作文件夹、主题。
//!
//! 配置以 TOML 存储于 `~/.boenmind/config.toml`，首次启动自动生成默认配置。
//! 模型提供商分为两类来源：
//! - 云提供商（openai / anthropic / gemini）：默认官方端点，可覆盖 base_url
//! - 本地或 OpenAI 兼容端点（ollama / llamacpp / 自定义 openai 兼容）：必须提供 base_url

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
    /// 启用的插件（~/.boenmind/extensions 下的扩展 id）
    #[serde(default)]
    pub enabled_plugins: Vec<String>,
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
    /// 任意 OpenAI 兼容服务（自定义端点）
    Custom,
}

impl ProviderKind {
    /// pi agent 注册表中的提供商名。
    ///
    /// minimax / deepseek / openrouter / custom 不在 pi 的内置 provider 列表中，
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
            ProviderKind::Custom => format!("custom-{}", provider_id),
        }
    }

    /// 是否走 OpenAI 兼容自定义路由（models.json 中需显式 `api: "openai-completions"`）
    pub fn is_openai_compatible_route(&self) -> bool {
        matches!(
            self,
            ProviderKind::Minimax | ProviderKind::Deepseek | ProviderKind::Openrouter | ProviderKind::Custom
        )
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ProviderKind::Openai => "OpenAI 兼容",
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::Gemini => "Google Gemini",
            ProviderKind::Ollama => "Ollama",
            ProviderKind::Llamacpp => "llama.cpp",
            ProviderKind::Minimax => "MiniMax",
            ProviderKind::Deepseek => "DeepSeek",
            ProviderKind::Openrouter => "OpenRouter",
            ProviderKind::Custom => "自定义 OpenAI 兼容",
        }
    }
}

fn default_theme() -> String {
    "system".to_string()
}

fn default_working_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(DEFAULT_WORKSPACE_DIR)
}

pub fn app_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
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
            enabled_plugins: Vec::new(),
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
                eprintln!("[bm-core] 配置解析失败 ({err})，使用默认配置");
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

/// 保存配置并确保目录存在。
pub fn save(config: &AppConfig) -> Result<(), std::io::Error> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = toml::to_string_pretty(config).map_err(std::io::Error::other)?;
    fs::write(path, text)
}

/// 确保工作文件夹存在。
pub fn ensure_working_dir(config: &AppConfig) -> Result<(), std::io::Error> {
    fs::create_dir_all(&config.working_dir)
}

/// 查找提供商；id 不存在时回退到 default_provider。
pub fn resolve_provider<'a>(config: &'a AppConfig, provider_id: Option<&str>) -> Option<&'a ProviderConfig> {
    if let Some(id) = provider_id {
        if let Some(p) = config.providers.iter().find(|p| p.id == id) {
            return Some(p);
        }
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
/// - 自定义 OpenAI 兼容提供商（minimax/deepseek/openrouter/custom-*）：
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
        if let Some(key) = &p.api_key {
            if !key.is_empty() {
                fs::create_dir_all(&keys_dir)?;
                let key_file = keys_dir.join(format!("{name}.key"));
                fs::write(&key_file, key)?;
                entry.insert(
                    "apiKey".to_string(),
                    json!(format!("file:{}", key_file.display())),
                );
                // 自定义 provider 无内置 auth metadata，必须显式声明才会携带 Authorization 头
                entry.insert("authHeader".to_string(), json!(true));
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_pi_name() {
        assert_eq!(ProviderKind::Openai.pi_name("p1"), "openai");
        assert_eq!(ProviderKind::Ollama.pi_name("p1"), "ollama");
        assert_eq!(ProviderKind::Minimax.pi_name("p1"), "minimax");
        assert_eq!(ProviderKind::Custom.pi_name("p-abc"), "custom-p-abc");
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
        };
        assert_eq!(resolve_provider(&config, Some("missing")).unwrap().id, "b");
        assert_eq!(resolve_model(&config.providers[1], None).unwrap(), "qwen");
        assert_eq!(resolve_model(&config.providers[1], Some("x")).unwrap(), "x");
    }
}
