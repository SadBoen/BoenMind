//! LLM provider 插件声明（核心分类）。
//!
//! 对应 dsh 官方 `@deepseek-ai/dsh-llm`（LlmPort 实现集：mock / OpenAI 兼容 /
//! 多 provider 路由）。装配点 = `Runtime::swap_llm`，组件已是 `Arc<dyn LlmPort>`
//! trait object（运行期可换装），本模块只声明清单身份。

use kernel_contracts::plugin::{PluginManifestEntry, PLUGIN_LLM};

/// LLM provider 插件清单条目（核心组件）。
pub fn manifest() -> PluginManifestEntry {
    PluginManifestEntry::core(
        PLUGIN_LLM,
        "LLM Provider",
        "LLM 提供者适配：stream/list_models/resolve_model（mock / OpenAI 兼容 / 多 provider 路由）",
    )
}
