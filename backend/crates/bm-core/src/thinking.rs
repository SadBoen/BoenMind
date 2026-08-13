//! 思考档位能力判定：复刻 pi 运行时的 per-model 白名单
//! （legacy/pi_agent_rust/src/models.rs 的 available_thinking_levels /
//! supports_xhigh / supports_max），供 /api/thinking-levels 出 UI 档位。
//!
//! 对齐要点（2026-08-12 读 legacy 源码核对）：
//! - OpenAI 兼容路由提供商（minimax/deepseek/custom 等）在 sync_pi_models_json
//!   时对全部模型写入 `reasoning: true`，因此这些模型一律按推理模型出档；
//!   内置路由（openai/anthropic/gemini/ollama/llamacpp）由 pi 目录判定，
//!   这里用 ID 白名单近似（白名单外默认推理，pi 运行时 clamp 兜底）。
//! - xhigh/max 白名单与 vendor 逐条一致（含 deepseek provider/base_url 判定、
//!   anthropic claude- 前缀 + 档位家族）。
//! - minimal 档 pi 对推理模型全量暴露，但 UI 决策不展示（与 ChatInput 4 档一致）。
//!
//! pi 运行时仍是最终权威：不支持的档会被 clamp 逐级降级（Max→XHigh→High、
//! 非推理→Off），这里的判定只负责 UI 出什么档。

use crate::config::ProviderKind;

/// 已知不支持推理的模型 ID（内置路由白名单外的模型默认按推理处理，
/// 避免新旗舰模型被误伤；误判时 pi 运行时 clamp 兜底）。
fn is_known_non_reasoning(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m == "deepseek-chat"
        || m == "deepseek-v3"
        || m.starts_with("gpt-4o")
        || m.starts_with("gpt-4.1")
        || m == "gpt-4"
        || m.starts_with("gpt-4-turbo")
        || m.starts_with("claude-3")
        || m.starts_with("claude-haiku")
        || m.starts_with("minimax-m1")
        || m.starts_with("minimax-text-01")
        || m.starts_with("gemini-1.5")
        || m.starts_with("qwen2")
        || m.starts_with("qwen-turbo")
        || m.starts_with("qwen-plus")
        || m.starts_with("moonshot-v1")
        || m.starts_with("glm-4-flash")
        || m.starts_with("glm-4-plus")
        || m.starts_with("glm-4-air")
}

/// 是否 deepseek 推理模型（vendor is_deepseek_reasoning_model：
/// provider 为 deepseek 或 base_url 含 deepseek.com，不检查模型 ID）。
fn is_deepseek_reasoning(kind: ProviderKind, base_url: Option<&str>) -> bool {
    kind == ProviderKind::Deepseek
        || base_url
            .is_some_and(|url| url.to_ascii_lowercase().contains("deepseek.com"))
}

/// 是否 anthropic xhigh 档模型（vendor is_anthropic_xhigh_effort_model：
/// claude-opus-4-7/4-8/5、claude-sonnet-5、claude-fable-/mythos-）。
fn is_anthropic_xhigh(model: &str) -> bool {
    let id = model.to_ascii_lowercase();
    let Some(pos) = id.find("claude-") else {
        return false;
    };
    let id = &id[pos..];
    id.starts_with("claude-opus-4-7")
        || id.starts_with("claude-opus-4-8")
        || id.starts_with("claude-opus-5")
        || id.starts_with("claude-sonnet-5")
        || id.starts_with("claude-fable-")
        || id.starts_with("claude-mythos-")
}

/// 是否 anthropic max 档模型（vendor is_anthropic_max_effort_model：
/// xhigh 家族 + claude-opus-4-6 / claude-sonnet-4-6，后者支持 max 但不支持 xhigh）。
fn is_anthropic_max(model: &str) -> bool {
    is_anthropic_xhigh(model)
        || {
            let id = model.to_ascii_lowercase();
            let Some(pos) = id.find("claude-") else {
                return false;
            };
            let id = &id[pos..];
            id.starts_with("claude-opus-4-6") || id.starts_with("claude-sonnet-4-6")
        }
}

/// 该模型是否支持 xhigh 思考档（vendor supports_xhigh 白名单）。
fn supports_xhigh(kind: ProviderKind, base_url: Option<&str>, model: &str) -> bool {
    matches!(
        model,
        "gpt-5.1-codex-max"
            | "gpt-5.2"
            | "gpt-5.5"
            | "gpt-5.6"
            | "gpt-5.6-sol"
            | "gpt-5.6-terra"
            | "gpt-5.6-luna"
            | "gpt-5.4"
            | "gpt-5.2-codex"
            | "gpt-5.3-codex"
            | "gpt-5.3-codex-spark"
    ) || is_deepseek_reasoning(kind, base_url)
        || (kind == ProviderKind::Anthropic && is_anthropic_xhigh(model))
}

/// 该模型是否支持 max 思考档（vendor supports_max 白名单）。
fn supports_max(kind: ProviderKind, base_url: Option<&str>, model: &str) -> bool {
    matches!(model, "gpt-5.6" | "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna")
        || is_deepseek_reasoning(kind, base_url)
        || (kind == ProviderKind::Anthropic && is_anthropic_max(model))
}

/// 返回某模型在 UI 上应展示的思考档位（对应 pi ThinkingLevel 的字符串序列化，
/// 顺序即档位强度升序）。非推理模型只有 `off`；推理模型为
/// off/low/medium/high，白名单模型追加 xhigh / max。
pub fn thinking_levels_for(
    kind: ProviderKind,
    base_url: Option<&str>,
    model: &str,
) -> Vec<&'static str> {
    // OpenAI 兼容路由：models.json 同步时对该类提供商全部标记 reasoning: true，
    // pi 运行时会按推理模型处理（含 deepseek-chat 等非推理 ID）
    let reasoning = if kind.is_openai_compatible_route() {
        true
    } else {
        !is_known_non_reasoning(model)
    };
    if !reasoning {
        return vec!["off"];
    }
    let mut levels = vec!["off", "low", "medium", "high"];
    if supports_xhigh(kind, base_url, model) {
        levels.push("xhigh");
    }
    if supports_max(kind, base_url, model) {
        levels.push("max");
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels(kind: ProviderKind, url: Option<&str>, model: &str) -> Vec<&'static str> {
        thinking_levels_for(kind, url, model)
    }

    #[test]
    fn minimax_all_models_reasoning() {
        // OpenAI 兼容路由：全部按推理出 4 档（M1 也不受 ID 白名单影响）
        let kind = ProviderKind::Minimax;
        assert_eq!(levels(kind, None, "MiniMax-M3"), vec!["off", "low", "medium", "high"]);
        assert_eq!(levels(kind, None, "MiniMax-M2.5-highspeed"), vec!["off", "low", "medium", "high"]);
        assert_eq!(levels(kind, None, "MiniMax-M1"), vec!["off", "low", "medium", "high"]);
    }

    #[test]
    fn deepseek_provider_gets_xhigh_max() {
        // kind=deepseek：deepseek-reasoner 出 6 档（vendor：provider 判定不看模型 ID）
        let kind = ProviderKind::Deepseek;
        assert_eq!(
            levels(kind, None, "deepseek-reasoner"),
            vec!["off", "low", "medium", "high", "xhigh", "max"]
        );
        // 同样受 models.json reasoning:true 影响，deepseek-chat 也按推理出 6 档
        assert_eq!(
            levels(kind, None, "deepseek-chat"),
            vec!["off", "low", "medium", "high", "xhigh", "max"]
        );
    }

    #[test]
    fn deepseek_com_base_url_also_counts() {
        // vendor：base_url 含 deepseek.com 即按 deepseek 推理模型处理
        let kind = ProviderKind::Custom;
        assert_eq!(
            levels(kind, Some("https://api.deepseek.com/v1"), "x-reasoner"),
            vec!["off", "low", "medium", "high", "xhigh", "max"]
        );
        // 自定义端点非 deepseek：仅 4 档
        assert_eq!(
            levels(kind, Some("https://api.example.com/v1"), "x-reasoner"),
            vec!["off", "low", "medium", "high"]
        );
    }

    #[test]
    fn openai_builtin_route_uses_id_heuristics() {
        let kind = ProviderKind::Openai;
        // 非推理老模型：只有 off
        assert_eq!(levels(kind, None, "gpt-4o"), vec!["off"]);
        assert_eq!(levels(kind, None, "gpt-4.1-mini"), vec!["off"]);
        // 推理模型：4 档起步
        assert_eq!(levels(kind, None, "gpt-5"), vec!["off", "low", "medium", "high"]);
        // xhigh 白名单（gpt-5.2/5.5 等）
        assert_eq!(
            levels(kind, None, "gpt-5.2"),
            vec!["off", "low", "medium", "high", "xhigh"]
        );
        // max 白名单（gpt-5.6 系）
        assert_eq!(
            levels(kind, None, "gpt-5.6"),
            vec!["off", "low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            levels(kind, None, "gpt-5.6-sol"),
            vec!["off", "low", "medium", "high", "xhigh", "max"]
        );
    }

    #[test]
    fn anthropic_families() {
        let kind = ProviderKind::Anthropic;
        // claude-3 系非推理：只有 off
        assert_eq!(levels(kind, None, "claude-3-5-sonnet-20241022"), vec!["off"]);
        // claude-sonnet-4.5：推理 4 档（无 xhigh）
        assert_eq!(
            levels(kind, None, "claude-sonnet-4-5-20250929"),
            vec!["off", "low", "medium", "high"]
        );
        // claude-opus-4-6：max 有、xhigh 无（vendor 专门注释的家族）
        assert_eq!(
            levels(kind, None, "claude-opus-4-6-20250805"),
            vec!["off", "low", "medium", "high", "max"]
        );
        // claude-opus-4-7：xhigh + max
        assert_eq!(
            levels(kind, None, "claude-opus-4-7-20250901"),
            vec!["off", "low", "medium", "high", "xhigh", "max"]
        );
        // claude-opus-5：xhigh + max
        assert_eq!(
            levels(kind, None, "claude-opus-5-20260101"),
            vec!["off", "low", "medium", "high", "xhigh", "max"]
        );
    }

    #[test]
    fn unknown_models_default_to_reasoning() {
        // 内置路由白名单外默认推理（新旗舰不被误伤），pi clamp 兜底
        let kind = ProviderKind::Ollama;
        assert_eq!(levels(kind, None, "llama3.1"), vec!["off", "low", "medium", "high"]);
        let kind = ProviderKind::Openai;
        assert_eq!(levels(kind, None, "o4-mini"), vec!["off", "low", "medium", "high"]);
    }
}
