//! # Context Compactor 插件（功能分类）
//!
//! 长会话上下文压缩策略实现：给 [`bm_ports::Compactor`] 端口提供默认策略
//! （软水线 0.5 / 尾部保留 10% / 下限 4000 token / 中部不足 512 token 不压）。
//! 事务协议（选区间/摘要/变换）在契约层 [`bm_ports::Compactor::maybe_compact`]
//! 默认实现统一执行——策略与事务分离，loop 只依赖端口不依赖本插件。
//!
//! 架构（2026-08-20 回头看收口）：`Compactor` trait 原定义在本插件、核心
//! loop 编译期依赖它 →「核心依赖功能插件」依赖倒置。修复后 trait 上提
//! `bm/ports`（产品契约层），本插件只留策略 + 清单身份。
//!
//! **日志不改、前端无感**：压缩是运行态视图变换（审计/回看仍见全历史）。
//! 参数全部公开可变（参数进化 = 插件自治，调参不碰核心）。

use kernel_contracts::llm::{GenerateOptions, Role};
use kernel_contracts::text_message;
use bm_ports::Compactor;

pub mod plugin;

/// 默认压缩策略：软水线 0.5 / 尾部保留 10%（下限 4000 token）/ 中部不足
/// 512 token 不压。全部参数公开可变。
#[derive(Debug, Clone, PartialEq)]
pub struct DefaultCompactor {
    /// 软水线（0.0 ~ 1.0，占用窗口比例）。
    pub watermark: f64,
    /// 尾部保留比例（占窗口比例）。
    pub keep_recent_ratio: f64,
    /// 尾部保留 token 下限。
    pub keep_recent_floor: u64,
    /// 中部不足多少 token 不值得压。
    pub min_middle_tokens: u64,
}

impl Default for DefaultCompactor {
    fn default() -> Self {
        Self {
            watermark: 0.5,
            keep_recent_ratio: 0.10,
            keep_recent_floor: 4_000,
            min_middle_tokens: 512,
        }
    }
}

impl Compactor for DefaultCompactor {
    fn should_compact(&self, input_tokens: u64, context_window: u64) -> bool {
        let soft = (context_window as f64 * self.watermark) as u64;
        input_tokens >= soft.max(1)
    }

    /// 尾部保留预算：max(窗口 × ratio, floor)。
    fn keep_recent_tokens(&self, context_window: u64) -> u64 {
        let ratio = (context_window as f64 * self.keep_recent_ratio) as u64;
        ratio.max(self.keep_recent_floor)
    }

    fn min_middle_tokens(&self) -> u64 {
        self.min_middle_tokens
    }

    /// 摘要请求：保留用户意图、关键事实、已完成的工具操作与结论；与原文
    /// 同语言，300 字内。thinking 强制禁用（`purpose: compaction`），
    /// temperature 0.3 收窄发散。
    fn summarize_request(&self, provider: &str, model: &str, dialogue: &str) -> GenerateOptions {
        let prompt = format!(
            "请总结以下对话历史（保留用户意图、关键事实、已完成的工具操作与结论；\
             用与原文相同的语言，控制在 300 字内）：\n\n{dialogue}"
        );
        GenerateOptions {
            provider: provider.to_string(),
            model: model.to_string(),
            messages: vec![text_message(Role::User, prompt)],
            tools: vec![],
            temperature: Some(0.3),
            max_tokens: Some(1024),
            session_id: None,
            signal: None, // 由 maybe_compact 传入
            reasoning_effort: None,
            thinking: Some("disabled".to_string()),
            purpose: Some("compaction".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_compact_respects_watermark() {
        let c = DefaultCompactor::default();
        // 窗口 128K × 0.5 = 64K：64K 恰好触发，以下不触发。
        assert!(!c.should_compact(63_999, 128_000));
        assert!(c.should_compact(64_000, 128_000));
        // 空窗口下限 1：任何非零输入都触发（防御 0 除/0 窗口）。
        assert!(!c.should_compact(0, 0));
        assert!(c.should_compact(1, 0));
    }

    #[test]
    fn keep_recent_tokens_uses_ratio_with_floor() {
        let c = DefaultCompactor::default();
        // 128K × 10% = 12.8K > 4K 下限 → 12_800。
        assert_eq!(c.keep_recent_tokens(128_000), 12_800);
        // 8K × 10% = 800 < 4K 下限 → 4_000。
        assert_eq!(c.keep_recent_tokens(8_000), 4_000);
    }

    #[test]
    fn summarize_request_compact_shape() {
        let c = DefaultCompactor::default();
        let req = c.summarize_request("p", "m", "dialogue-body");
        assert_eq!(req.purpose.as_deref(), Some("compaction"));
        assert_eq!(req.thinking.as_deref(), Some("disabled"));
        assert_eq!(req.temperature, Some(0.3));
        assert!(req.tools.is_empty());
        assert_eq!(req.model, "m");
        assert!(req.messages.iter().any(|m| m.role == Role::User));
        assert!(req.messages.iter().any(|m| m
            .content
            .iter()
            .any(|b| matches!(b, kernel_contracts::ContentBlock::Text(t) if t.contains("dialogue-body")))));
    }
}