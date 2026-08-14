//! 压缩插件（默认实现，D10）：双触发 + 摘要事务。
//! 默认软水线 0.5，对齐 pi 引擎（2026-08-14 双开对比后收敛，见 DefaultCompactor）。
//!
//! 实现 [`bm_loop::Compactor`] 的**策略面**——水线/尾部保留/摘要 prompt 全部
//! 插件自治（可换实现、可关闭；关闭后核心 loop 以硬触发兜底：超窗失败
//! 回合、不丢历史——缺插件优雅失败，见 bm-loop compact.rs；框架的重点
//! 不是裸跑，是装上插件后跑得好，v0.17 用户定调）。
//!
//! 事务协议在 loop（三事件落盘 + replace 遮蔽 + fail-safe），本 crate 不含
//! 任何日志/投影逻辑——策略与协议分离是本次越界修正（v0.17）的拆法。
//!
//! 组装层（bm-server）把本策略挂进 `LoopConfig.compactor`；参数来源
//! （bm-core compaction 配置）由组装层换算注入，本 crate 不依赖
//! bm-core/bm-server（插件依赖核心，方向守卫见 tests/architecture.rs）。

use bm_loop::llm::LlmRequest;
use bm_loop::Compactor;

/// 默认压缩策略（原 bm-loop 内建策略迁出）：
/// 软水线 0.5（对齐 pi 引擎——30 轮 pi/bm 双开对比实测：水线 0.8 时 bm 发送量
/// 2263.0K ≈ pi 基线 888.6K 的 2.5×、峰值上下文 205.7K vs 94.1K，
/// 主因水线差；收敛 0.5 后复测 token 曲线再拍切换）、
/// 尾部保留 10% / 下限 4000 token、中部不足 512 token 不压。
///
/// 全部参数公开可变：**参数进化 = 插件自治**（v0.17 定调 2）——调水线/换
/// 摘要 prompt 不用碰核心。
#[derive(Debug, Clone, PartialEq)]
pub struct DefaultCompactor {
    /// 软水线（0.0 ~ 1.0，占用窗口比例）
    pub watermark: f64,
    /// 尾部保留比例（占窗口比例）
    pub keep_recent_ratio: f64,
    /// 尾部保留 token 下限
    pub keep_recent_floor: u64,
    /// 中部不足多少 token 不值得压
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
    fn should_compact(&self, total_tokens: u64, context_window: u64) -> bool {
        let soft = (context_window as f64 * self.watermark) as u64;
        total_tokens >= soft.max(1)
    }

    /// 尾部保留预算：max(窗口 × ratio, floor)。
    fn keep_recent_tokens(&self, context_window: u64) -> u64 {
        let ratio = (context_window as f64 * self.keep_recent_ratio) as u64;
        ratio.max(self.keep_recent_floor)
    }

    fn min_middle_tokens(&self) -> u64 {
        self.min_middle_tokens
    }

    /// 摘要请求（prompt 插件自治）：保留用户意图、关键事实、已完成的
    /// 工具操作与结论；与原文同语言，300 字内。
    fn summarize_request(&self, model: &str, dialogue: &str) -> LlmRequest {
        let prompt = format!(
            "请总结以下对话历史（保留用户意图、关键事实、已完成的工具操作与结论；\
             用与原文相同的语言，控制在 300 字内）：\n\n{dialogue}"
        );
        LlmRequest {
            payload: serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0.3,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_threshold_at_default_watermark() {
        let p = DefaultCompactor::default();
        assert_eq!(p.watermark, 0.5, "默认水线 0.5，对齐 pi");
        assert!(!p.should_compact(0, 100));
        assert!(!p.should_compact(49, 100));
        assert!(p.should_compact(50, 100), "50/100 = 0.5 水线");
        assert!(p.should_compact(100, 100));
    }

    #[test]
    fn soft_threshold_at_explicit_watermark() {
        // 显式非默认水线（0.8 旧口径）依旧生效——参数公开可变，插件自治。
        let p = DefaultCompactor {
            watermark: 0.8,
            ..Default::default()
        };
        assert!(!p.should_compact(79, 100));
        assert!(p.should_compact(80, 100), "80/100 = 0.8 显式水线");
    }

    #[test]
    fn keep_recent_is_max_of_ratio_and_floor() {
        let p = DefaultCompactor::default();
        assert_eq!(p.keep_recent_tokens(1_000), 4_000, "floor 生效");
        assert_eq!(p.keep_recent_tokens(100_000), 10_000, "ratio 生效");
    }

    #[test]
    fn summarize_request_shapes_payload() {
        let p = DefaultCompactor::default();
        let req = p.summarize_request("m", "对话内容");
        assert_eq!(req.payload["model"], "m");
        assert_eq!(req.payload["messages"][0]["role"], "user");
        assert!(req.payload["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("对话内容"));
    }
}
