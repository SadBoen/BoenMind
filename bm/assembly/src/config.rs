//! 真 provider 配置文件解析（M3）。
//!
//! 复用既有 boenmind 形态的 `config.toml` 子集（`~/.boenmind/config.toml` 兼容，
//! 见旧版 backend 配置），取 LLM 相关段 + 上下文压缩段：
//!
//! ```toml
//! default_provider = "minimax"        # 可选：默认 provider
//! default_model = "MiniMax-M3"        # 可选：默认模型
//!
//! [[providers]]
//! id = "minimax"                      # provider id（wire 上 llm.providers.provider）
//! name = "MiniMax"                    # 可选：显示名（缺省 = id）
//! kind = "minimax"                    # minimax | deepseek | custom（OpenAI 兼容）
//! base_url = "https://api.minimaxi.com/v1"   # 可选：kind 有内置缺省
//! api_key = "..."                     # 可选：缺省读 env {ID}_API_KEY（大写）
//! models = ["MiniMax-M3", ...]        # 可选：静态模型清单
//! default_model = "MiniMax-M3"        # 可选：该 provider 的默认模型
//!
//! [compaction]                        # 可选：上下文压缩默认策略（--compact 时生效）
//! enabled = true                      # 可选：false = 覆盖默认装配（不压）
//! watermark = 0.5                     # 可选：软水线（窗口占用比例）
//! keep_recent_ratio = 0.1             # 可选：尾部保留比例
//! keep_recent_floor = 4000            # 可选：尾部保留 token 下限
//! min_middle_tokens = 512             # 可选：中部不足多少 token 不压
//! ```
//!
//! 无 key（配置缺 + env 缺）的 provider 以 keyless 装配（请求时 MISSING_CREDENTIAL，
//! key 可经 credentials.set `{ID}_API_KEY` 热补）；base_url 无法推断的 custom
//! provider 跳过并警告。**不传 --config 时服务保持 mock provider（旧行为不变）；
//! 不传 --compact 时 compaction 段不生效（装配开关在 CLI）。**

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

/// 加载后的 LLM 配置。
#[derive(Debug, Clone, Default)]
pub struct LlmConfig {
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    pub providers: Vec<ProviderConfig>,
    /// 上下文压缩策略段（`[compaction]`，可选）。
    pub compaction: Option<CompactionConfig>,
}

/// `[compaction]` 段：上下文压缩默认策略参数（--compact 装配时消费）。
/// 全部字段可选（缺省回落 DefaultCompactor 默认值）。
#[derive(Debug, Clone, Default)]
pub struct CompactionConfig {
    /// false = 即使 --compact 也不压（配置级覆盖 CLI）。None = 用 CLI 决定。
    pub enabled: Option<bool>,
    /// 软水线（窗口占用比例）。缺省 0.5。
    pub watermark: Option<f64>,
    /// 尾部保留比例（窗口比例）。缺省 0.1。
    pub keep_recent_ratio: Option<f64>,
    /// 尾部保留 token 下限。缺省 4000。
    pub keep_recent_floor: Option<u64>,
    /// 中部不足多少 token 不压。缺省 512。
    pub min_middle_tokens: Option<u64>,
}

/// 单个 provider 配置（已校验，可直接构造适配器）。
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub default_model: Option<String>,
}

/// provider 通道类别（决定模型列表端点形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// MiniMax：OpenAI 兼容 + reasoning_content；模型列表 `GET /models/list`。
    Minimax,
    /// DeepSeek：OpenAI 兼容；模型列表 `GET /models`。
    DeepSeek,
    /// 任意 OpenAI 兼容端点。
    Custom,
}

impl ProviderKind {
    fn infer(id: &str, kind: &str) -> ProviderKind {
        match kind.to_ascii_lowercase().as_str() {
            "minimax" => ProviderKind::Minimax,
            "deepseek" => ProviderKind::DeepSeek,
            "custom" | "openai-compatible" | "" => {
                // 未声明时按 id 推断（兼容旧 config.toml 只有 kind 字段的写法）。
                match id.to_ascii_lowercase().as_str() {
                    "minimax" => ProviderKind::Minimax,
                    "deepseek" => ProviderKind::DeepSeek,
                    _ => ProviderKind::Custom,
                }
            }
            other => {
                tracing::warn!("provider {id}: unknown kind '{other}', treating as custom");
                ProviderKind::Custom
            }
        }
    }
}

#[derive(Deserialize)]
struct ConfigFile {
    default_provider: Option<String>,
    default_model: Option<String>,
    providers: Option<Vec<RawProvider>>,
    compaction: Option<RawCompaction>,
}

#[derive(Deserialize)]
struct RawCompaction {
    enabled: Option<bool>,
    watermark: Option<f64>,
    keep_recent_ratio: Option<f64>,
    keep_recent_floor: Option<u64>,
    min_middle_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct RawProvider {
    id: String,
    name: Option<String>,
    kind: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    models: Option<Vec<String>>,
    default_model: Option<String>,
}

/// 读取并校验配置文件。文件不存在/解析失败 → Err（fail-loud：配置是显式请求的）。
pub fn load_llm_config(path: &Path) -> Result<LlmConfig, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;
    let file: ConfigFile =
        toml::from_str(&text).map_err(|e| format!("config parse failed: {e}"))?;

    let mut providers = Vec::new();
    for raw in file.providers.unwrap_or_default() {
        match build_provider(raw) {
            Ok(Some(p)) => providers.push(p),
            Ok(None) => {} // 跳过并已警告
            Err(e) => tracing::warn!("{e}"),
        }
    }

    // 去重（同 id 后者覆盖前者的语义 → 报错更安全）。
    let mut seen: HashMap<String, ()> = HashMap::new();
    providers.retain(|p| seen.insert(p.id.clone(), ()).is_none());

    Ok(LlmConfig {
        default_provider: file.default_provider,
        default_model: file.default_model,
        providers,
        compaction: file.compaction.map(|raw| CompactionConfig {
            enabled: raw.enabled,
            watermark: raw.watermark,
            keep_recent_ratio: raw.keep_recent_ratio,
            keep_recent_floor: raw.keep_recent_floor,
            min_middle_tokens: raw.min_middle_tokens,
        }),
    })
}

fn build_provider(raw: RawProvider) -> Result<Option<ProviderConfig>, String> {
    let id = raw.id.trim().to_string();
    if id.is_empty() {
        return Err("provider entry with empty id skipped".into());
    }
    let kind = ProviderKind::infer(&id, raw.kind.as_deref().unwrap_or(""));
    let name = raw.name.unwrap_or_else(|| id.clone());

    let base_url = match raw.base_url {
        Some(u) => u.trim_end_matches('/').to_string(),
        None => match kind {
            ProviderKind::Minimax => "https://api.minimaxi.com/v1".to_string(),
            ProviderKind::DeepSeek => "https://api.deepseek.com/v1".to_string(),
            ProviderKind::Custom => {
                tracing::warn!("provider {id}: custom kind requires base_url, skipped");
                return Ok(None);
            }
        },
    };
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        tracing::warn!("provider {id}: invalid base_url '{base_url}', skipped");
        return Ok(None);
    }

    // api_key：配置 > env {ID}_API_KEY > keyless（空串；请求时 MISSING_CREDENTIAL）。
    // 对齐 DSH `dynamic-config.spec.ts`："starts keyless and serves the next request
    // once the key arrives"——装配不要求 key，key 可经 credentials.set 热补。
    let api_key = match raw.api_key {
        Some(k) if !k.trim().is_empty() => k.trim().to_string(),
        _ => std::env::var(format!("{}_API_KEY", id.to_uppercase())).unwrap_or_default(),
    };
    if api_key.is_empty() {
        tracing::warn!(
            "provider {id}: no api_key (config or {}_API_KEY); serving keyless — requests fail MISSING_CREDENTIAL until a key is set",
            id.to_uppercase()
        );
    }

    Ok(Some(ProviderConfig {
        id,
        name,
        kind,
        base_url,
        api_key,
        models: raw.models.unwrap_or_default(),
        default_model: raw.default_model,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write_tmp(content: &str) -> PathBuf {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("bm-llmcfg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn parses_minimax_and_deepseek() {
        let path = write_tmp(
            r#"
default_provider = "minimax"
default_model = "MiniMax-M3"

[[providers]]
id = "minimax"
name = "MiniMax"
kind = "minimax"
api_key = "k1"
models = ["MiniMax-M3", "MiniMax-M2.7"]

[[providers]]
id = "deepseek"
kind = "deepseek"
api_key = "k2"
models = ["deepseek-chat"]
"#,
        );
        let cfg = load_llm_config(&path).unwrap();
        assert_eq!(cfg.default_provider.as_deref(), Some("minimax"));
        assert_eq!(cfg.providers.len(), 2);
        assert_eq!(cfg.providers[0].kind, ProviderKind::Minimax);
        assert_eq!(cfg.providers[0].base_url, "https://api.minimaxi.com/v1");
        assert_eq!(cfg.providers[1].kind, ProviderKind::DeepSeek);
        assert_eq!(cfg.providers[1].models, vec!["deepseek-chat"]);
    }

    #[test]
    fn missing_key_keeps_provider_keyless() {
        let path = write_tmp(
            r#"
[[providers]]
id = "minimax"
kind = "minimax"
models = ["MiniMax-M3"]
"#,
        );
        // env 缺 → keyless（不跳过；key 可经 credentials.set 热补）。
        std::env::remove_var("MINIMAX_API_KEY");
        let cfg = load_llm_config(&path).unwrap();
        assert_eq!(cfg.providers.len(), 1);
        assert_eq!(cfg.providers[0].id, "minimax");
        assert!(cfg.providers[0].api_key.is_empty());
    }

    #[test]
    fn custom_requires_base_url() {
        let path = write_tmp(
            r#"
[[providers]]
id = "myproxy"
kind = "custom"
api_key = "k"
"#,
        );
        let cfg = load_llm_config(&path).unwrap();
        assert!(cfg.providers.is_empty());
    }
}
