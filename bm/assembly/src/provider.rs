//! 真实 provider 装配（M3）：配置 → OpenAICompatLlm 适配器 → MultiProviderLlm 聚合。
//!
//! 组合根的装配职责：`assemble_providers` 是"真 LLM provider"的唯一装配点——
//! web-server（L0 最终程序）只消费结果，不直接依赖 plugin-llm（避免第二组合根）。
//!
//! 返回 `(provider_runtimes, 聚合 llm, default_provider, default_model)`：
//! - provider_runtimes：前端 llm.providers / llm.models / llm.discoverModels 的数据源
//! - 聚合 llm：已 swap 进 Runtime 的 `Arc<dyn LlmPort>`（MultiProviderLlm 路由全部通道）

use std::sync::Arc;

use kernel_contracts::llm::{LlmModelInfo, LlmPort, ModelReasoning, ReasoningEffort};

use crate::config::LlmConfig;

/// 真实 provider 运行时（M3）：静态模型清单 + 流式适配器。
/// `adapter` 为 None 表示 mock 单 provider 模式。
pub struct ProviderRuntime {
    pub id: String,
    pub display_name: String,
    pub settings_ns: String,
    pub base_url: String,
    pub models: Vec<LlmModelInfo>,
    pub adapter: Option<Arc<dyn LlmAdapter>>,
}

/// 适配器端口（组合根暴露给 L0 消费的最小面）：
/// 前端 RPC 需要的三个操作——模型发现 / baseURL 覆盖 / API key 热补。
/// 具体适配器（OpenAICompatLlm）是 plugin-llm 的实现细节，L0 只见此 trait。
#[async_trait::async_trait]
pub trait LlmAdapter: Send + Sync {
    fn models(&self) -> Vec<LlmModelInfo>;
    fn set_base_url_override(&self, base_url: Option<String>);
    fn set_api_key_override(&self, api_key: Option<String>);
    async fn list_models_remote(&self) -> Result<Vec<LlmModelInfo>, kernel_contracts::LlmError>;
}

#[async_trait::async_trait]
impl LlmAdapter for plugin_llm::OpenAICompatLlm {
    fn models(&self) -> Vec<LlmModelInfo> {
        self.models().to_vec()
    }
    fn set_base_url_override(&self, base_url: Option<String>) {
        self.set_base_url_override(base_url);
    }
    fn set_api_key_override(&self, api_key: Option<String>) {
        self.set_api_key_override(api_key);
    }
    async fn list_models_remote(&self) -> Result<Vec<LlmModelInfo>, kernel_contracts::LlmError> {
        self.list_models_remote().await
    }
}

impl ProviderRuntime {
    pub fn settings_path(&self) -> Vec<String> {
        vec!["llm".to_string(), self.id.clone()]
    }
}

/// 装配结果：`(provider 元数据, 聚合 llm, 默认 provider, 默认 model)`。
pub type AssembledLlm = (Vec<ProviderRuntime>, Arc<dyn LlmPort>, String, String);

/// 装配全部 provider：构造 OpenAICompatLlm 适配器 + ProviderRuntime 元数据 + 聚合路由。
/// 失败 = 配置不可用（fail-loud，由调用方决定退出）。
pub fn assemble_providers(config: &LlmConfig, user_id: String) -> Result<AssembledLlm, String> {
    let mut provider_runtimes: Vec<ProviderRuntime> = Vec::new();
    let mut ports: Vec<(String, Arc<dyn LlmPort>)> = Vec::new();
    for p in &config.providers {
        if p.models.is_empty() {
            tracing::warn!("provider {}: no models declared, skipped", p.id);
            continue;
        }
        let list_endpoint = plugin_llm::ModelListEndpoint::Standard;
        let models: Vec<LlmModelInfo> = p
            .models
            .iter()
            .map(|m| LlmModelInfo {
                id: m.clone(),
                label: None,
                supports_tools: true,
                context_window: None,
                max_tokens: None,
                // #8 生产接线：DeepSeek 系默认具备推理能力（high 档），
                // 声明后 resolve_thinking 生产路径吃到 adapter 档位——
                // thinking:{type:enabled} + reasoning_effort:high 上 wire；
                // 其余模型不声明（None → provider 默认，能力未声明不上 thinking）。
                reasoning: if m.contains("deepseek") {
                    Some(ModelReasoning {
                        efforts: vec![ReasoningEffort {
                            id: "high".to_string(),
                            name: "High".to_string(),
                            description: None,
                        }],
                        default_effort: Some("high".to_string()),
                    })
                } else {
                    None
                },
            })
            .collect();
        let adapter = Arc::new(plugin_llm::OpenAICompatLlm::new(
            plugin_llm::OpenAiProviderConfig {
                id: p.id.clone(),
                display_name: p.name.clone(),
                settings_ns: format!("llm.{}", p.id),
                base_url: p.base_url.clone(),
                api_key: p.api_key.clone(),
                models,
                list_endpoint,
                user_id: user_id.clone(),
            },
        ));
        provider_runtimes.push(ProviderRuntime {
            id: p.id.clone(),
            display_name: p.name.clone(),
            settings_ns: format!("llm.{}", p.id),
            base_url: p.base_url.clone(),
            models: adapter.models().to_vec(),
            adapter: Some(Arc::clone(&adapter) as Arc<dyn LlmAdapter>),
        });
        ports.push((p.id.clone(), adapter as Arc<dyn LlmPort>));
    }
    if ports.is_empty() {
        return Err("config has no usable providers (all skipped)".into());
    }
    // 默认 provider/model：config 顶层优先，否则首个 provider 的 default_model，否则其首模型。
    let default_provider = config
        .default_provider
        .clone()
        .unwrap_or_else(|| ports[0].0.clone());
    let default_model = config
        .default_model
        .clone()
        .or_else(|| {
            config
                .providers
                .iter()
                .find(|p| p.id == default_provider)
                .and_then(|p| p.default_model.clone())
        })
        .or_else(|| {
            config
                .providers
                .iter()
                .find(|p| p.id == default_provider)
                .and_then(|p| p.models.first().cloned())
        })
        .unwrap_or_default();
    Ok((
        provider_runtimes,
        Arc::new(plugin_llm::MultiProviderLlm::new(ports)),
        default_provider,
        default_model,
    ))
}
