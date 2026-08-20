//! # Context Compactor 插件（功能分类）
//!
//! 长会话上下文压缩：输入 token 达到水线时，把历史中部摘要成一条
//! `System` 消息，替换为「摘要 + 尾部保留」的模型上下文——**日志不改、
//! 前端无感**。
//!
//! 架构约束（v2.1 定调）：`kernel/` 是外部 submodule 不改；事件日志是
//! 唯一事实源且 append-only（不可物理删中间事件）。因此压缩是**运行态
//! 视图变换**：日志完整保留（审计/回看），`Session::derive_messages()`
//! 的输出经本插件变换后才发给模型。模型可见的一切仍来自日志投影，
//! 只是视图被压缩；聊天界面的历史投影（`session.history`）不受影响。
//!
//! 插件面契约（loop 只问两件事）：[`Compactor::should_compact`]（压不压）
//! 与 [`Compactor::summarize_request`]（怎么压）。事务执行（选区间/摘要/
//! 变换）由 [`Compactor::maybe_compact`] 默认实现完成，loop 只装配与调用。
//!
//! **无插件 = 优雅透传**：`LoopRuntime.compactor = None` 时上下文原样
//! 发给模型，无任何行为变化；装配后软水线触发，未达水线的正常对话
//! 经第一道快速筛直接透传（零 I/O、零开销）。
//! **fail-safe**：摘要失败 / 非 stop finish / 空摘要 → `None`（保持原
//! 上下文，不遮蔽、不阻塞回合——宁可让模型看到完整历史也不静默丢信息）。
//!
//! 默认策略对齐 legacy `DefaultCompactor`（2026-08-14 pi/bm 双开对比收敛）：
//! 软水线 0.5、尾部保留 10% / 下限 4000 token、中部不足 512 token 不压。
//! 全部参数公开可变（参数进化 = 插件自治，调参不碰核心）。

use kernel_contracts::llm::{
    AbortSignal, ContentBlock, FinishReason, GenerateOptions, LlmMessage, LlmPort, Role,
    StreamChunk,
};
use kernel_contracts::text_message;
use futures::StreamExt;

pub mod plugin;

/// 默认上下文窗口（provider 未声明 `context_window` 时的回退值；与 legacy
/// 前端 TokenRing 的参考窗口 128K 一致）。
pub const DEFAULT_CONTEXT_WINDOW: u64 = 128_000;
/// 摘要 prompt 的最大长度（字符）：超过则截断，防摘要请求自身超窗。
const MAX_DIALOGUE_CHARS: usize = 30_000;
/// 单条工具结果/参数进摘要的最大长度（字符）。
const MAX_TOOL_OUTPUT_CHARS: usize = 400;

/// 压缩策略契约（插件面）：策略与事务分离——loop 只问「压不压」与
/// 「怎么压」，事务（选区间/摘要/变换）由默认实现统一执行。
#[async_trait::async_trait]
pub trait Compactor: Send + Sync + std::fmt::Debug {
    /// 软触发判定：输入 token 占用是否达到策略水线。
    fn should_compact(&self, input_tokens: u64, context_window: u64) -> bool;
    /// 尾部保留预算（token）：从后往前累积保留，其余为可压缩中部。
    fn keep_recent_tokens(&self, context_window: u64) -> u64;
    /// 中部不足多少 token 不值得压。
    fn min_middle_tokens(&self) -> u64;
    /// 摘要请求构造（摘要 prompt 策略自治）。
    fn summarize_request(&self, provider: &str, model: &str, dialogue: &str) -> GenerateOptions;

    /// 压缩判定 + 执行：`Some(transformed)` = 压缩后的上下文（System 摘要 +
    /// 尾部保留）；`None` = 透传原上下文（未达水线 / 中部过少 / 摘要失败）。
    ///
    /// fail-safe 纪律：摘要调用任何失败（流 Err / 非 stop finish / 空摘要）
    /// 都回落 `None`——不遮蔽、不阻塞回合，绝不静默丢历史。
    async fn maybe_compact(
        &self,
        llm: &dyn LlmPort,
        messages: &[LlmMessage],
        provider: &str,
        model: &str,
        signal: Option<AbortSignal>,
    ) -> Option<Vec<LlmMessage>> {
        // 第一道快速筛：默认窗口下水线未到 → 直接透传（零 I/O，正常对话无感）。
        let input_tokens = estimate_tokens(messages);
        if !self.should_compact(input_tokens, DEFAULT_CONTEXT_WINDOW) {
            return None;
        }
        // 达标的：解析真实窗口再判一次（provider 窗口更大时可能不压）。
        let window = llm
            .resolve_model(provider, model)
            .await
            .context_window
            .unwrap_or(DEFAULT_CONTEXT_WINDOW);
        if !self.should_compact(input_tokens, window) {
            return None;
        }

        // 选区间：尾部按保留预算从后往前累积，其余为可压缩中部。
        let keep = self.keep_recent_tokens(window);
        let mut tail_start = messages.len();
        let mut acc = 0u64;
        for (i, m) in messages.iter().enumerate().rev() {
            if acc >= keep {
                tail_start = i + 1;
                break;
            }
            acc += estimate_tokens(std::slice::from_ref(m));
        }
        let middle = &messages[..tail_start];
        let middle_tokens: u64 = middle
            .iter()
            .map(|m| estimate_tokens(std::slice::from_ref(m)))
            .sum();
        if middle_tokens < self.min_middle_tokens() {
            return None;
        }

        // 摘要：dialogue 拼装（工具结果截断、思维链剔除）→ 同模型、无工具、thinking 禁用。
        let dialogue = build_dialogue(middle);
        let request = self.summarize_request(provider, model, &dialogue);
        let summary = match collect_summary(llm, request, signal).await {
            Some(s) => s,
            None => return None, // fail-safe：摘要失败保持原上下文
        };
        if summary.trim().is_empty() {
            return None;
        }

        // 变换：System 摘要 + 尾部保留。
        let tail = messages[tail_start..].to_vec();
        let mut out = Vec::with_capacity(tail.len() + 1);
        out.push(LlmMessage {
            role: Role::System,
            content: vec![ContentBlock::Text(summary.clone())],
        });
        out.extend(tail);
        Some(out)
    }
}

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

/// 文本 token 粗估：chars/4（中英混合近似，legacy 同款；精度不足由
/// 水线判定容忍——粗估只是触发判据，不参与计费）。
fn estimate_tokens(messages: &[LlmMessage]) -> u64 {
    let chars: u64 = messages
        .iter()
        .map(|m| {
            m.content
                .iter()
                .map(|b| match b {
                    ContentBlock::Text(t) | ContentBlock::Reasoning(t) => t.chars().count() as u64,
                    ContentBlock::ToolCall(c) => (c.name.len() + c.arguments.len()) as u64,
                    ContentBlock::ToolResult(r) => r.output.chars().count() as u64,
                })
                .sum::<u64>()
        })
        .sum();
    chars.div_ceil(4)
}

/// 拼装可摘要文本：角色 + 内容；思维链（reasoning）剔除不进摘要，
/// 工具结果/参数截断防注入超长文本。
fn build_dialogue(messages: &[LlmMessage]) -> String {
    let mut out = String::new();
    for m in messages {
        out.push_str(m.role.as_str());
        out.push_str(": ");
        for b in &m.content {
            match b {
                ContentBlock::Text(t) => out.push_str(t),
                ContentBlock::Reasoning(_) => {} // 思维链不进摘要
                ContentBlock::ToolCall(c) => {
                    out.push_str(&format!(
                        "[tool call: {} {}]",
                        c.name,
                        truncate(&c.arguments, MAX_TOOL_OUTPUT_CHARS)
                    ));
                }
                ContentBlock::ToolResult(r) => {
                    out.push_str("[tool result] ");
                    out.push_str(&truncate(&r.output, MAX_TOOL_OUTPUT_CHARS));
                }
            }
        }
        out.push('\n');
    }
    if out.chars().count() > MAX_DIALOGUE_CHARS {
        out = out.chars().take(MAX_DIALOGUE_CHARS).collect();
    }
    out
}

/// 字符截断。
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

/// 消费摘要流：收集文本块直到 `Finish(Stop)`。任何失败（流 Err / 非 stop
/// finish）返回 `None`——调用方按 fail-safe 处理。
async fn collect_summary(
    llm: &dyn LlmPort,
    mut request: GenerateOptions,
    signal: Option<AbortSignal>,
) -> Option<String> {
    request.signal = signal;
    let mut stream = llm.stream(request);
    let mut text = String::new();
    let mut finish: Option<FinishReason> = None;
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(StreamChunk::TextDelta { text: t, .. }) => text.push_str(&t),
            Ok(StreamChunk::Finish(r)) => finish = Some(r),
            Ok(_) => {}
            Err(_) => return None, // torn 流：fail-safe
        }
    }
    if !matches!(finish, Some(FinishReason::Stop)) {
        return None;
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel_contracts::error::LlmError;
    use kernel_contracts::llm::LlmModelInfo;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    // ---------- mocks ----------

    /// 脚本式 LLM：`stream()` 产出预先编排的块序列；记录最近一次请求。
    struct ScriptLlm {
        model: String,
        context_window: Option<u64>,
        /// 队列：每次 stream() 弹一条流（全部元素）。
        scripts: Mutex<VecDeque<Vec<StreamChunk>>>,
        /// 记录最近一次请求的消息（断言摘要 prompt 形态）。
        last_request: Mutex<Option<GenerateOptions>>,
    }

    impl ScriptLlm {
        fn new(model: &str, window: Option<u64>, scripts: Vec<Vec<StreamChunk>>) -> Self {
            Self {
                model: model.to_string(),
                context_window: window,
                scripts: Mutex::new(scripts.into()),
                last_request: Mutex::new(None),
            }
        }

        fn text_stream(text: &str) -> Vec<StreamChunk> {
            vec![
                StreamChunk::TextDelta { index: 0, text: text.to_string() },
                StreamChunk::Finish(FinishReason::Stop),
            ]
        }

        fn fail_stream() -> Vec<StreamChunk> {
            vec![StreamChunk::Finish(FinishReason::Error {
                message: "boom".to_string(),
                code: "E_TEST".to_string(),
                extra: None,
            })]
        }
    }

    #[async_trait::async_trait]
    impl LlmPort for ScriptLlm {
        async fn list_models(&self, _provider: &str) -> Result<Vec<LlmModelInfo>, LlmError> {
            Ok(vec![LlmModelInfo {
                id: self.model.clone(),
                label: None,
                supports_tools: false,
                context_window: self.context_window,
                max_tokens: None,
                reasoning: None,
            }])
        }

        fn stream(&self, request: GenerateOptions) -> kernel_contracts::ChunkStream {
            *self.last_request.lock().unwrap() = Some(request);
            let script = self
                .scripts
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Self::text_stream(""));
            Box::pin(futures::stream::iter(script.into_iter().map(Ok)))
        }
    }

    // ---------- helpers ----------

    fn msg(role: Role, text: &str) -> LlmMessage {
        text_message(role, text.to_string())
    }

    /// 制造一条 N 字符文本消息（约 N/4 token）。
    fn long_text(n: usize) -> LlmMessage {
        msg(Role::User, &"x".repeat(n))
    }

    fn default_compactor() -> DefaultCompactor {
        DefaultCompactor::default()
    }

    // ---------- tests ----------

    #[test]
    fn estimate_tokens_is_chars_over_four() {
        let m = msg(Role::User, "abcd");
        assert_eq!(estimate_tokens(&[m]), 1);
        let m2 = msg(Role::User, "abcdefgh");
        assert_eq!(estimate_tokens(&[m2]), 2);
        // 空消息 = 0。
        assert_eq!(estimate_tokens(&[msg(Role::User, "")]), 0);
        // 多条求和。
        let all = vec![msg(Role::User, "abcd"), msg(Role::Assistant, "efgh")];
        assert_eq!(estimate_tokens(&all), 2);
    }

    #[test]
    fn should_compact_respects_watermark() {
        let c = default_compactor();
        // 窗口 128K × 0.5 = 64K：64K 恰好触发，以下不触发。
        assert!(!c.should_compact(63_999, 128_000));
        assert!(c.should_compact(64_000, 128_000));
        // 空窗口下限 1：任何非零输入都触发（防御 0 除/0 窗口）。
        assert!(!c.should_compact(0, 0));
        assert!(c.should_compact(1, 0));
    }

    #[test]
    fn keep_recent_tokens_uses_ratio_with_floor() {
        let c = default_compactor();
        // 128K × 10% = 12.8K > 4K 下限 → 12_800。
        assert_eq!(c.keep_recent_tokens(128_000), 12_800);
        // 8K × 10% = 800 < 4K 下限 → 4_000。
        assert_eq!(c.keep_recent_tokens(8_000), 4_000);
    }

    /// 未达水线 → None（透传，零 I/O——不调 resolve_model/stream）。
    #[tokio::test]
    async fn below_watermark_passes_through() {
        let llm = Arc::new(ScriptLlm::new("m", Some(128_000), vec![]));
        let c = default_compactor();
        // 10 条 100 字消息 ≈ 250 token，远低于 64K 水线。
        let messages: Vec<LlmMessage> = (0..10).map(|_| msg(Role::User, &"y".repeat(100))).collect();
        let out = c
            .maybe_compact(&*llm, &messages, "p", "m", None)
            .await;
        assert!(out.is_none());
        assert!(llm.last_request.lock().unwrap().is_none());
    }

    /// 达到水线但中部不足（keep_recent 下限吃掉了全部）→ None。
    #[tokio::test]
    async fn middle_too_small_passes_through() {
        // 水线调 0：任何输入都触发判定；窗口 8K 时保留预算 4K，输入 5K 时
        // 中部 ≈ 1K > 512 仍可压——把 min_middle 调高到 4K 使中部不足。
        let c = DefaultCompactor {
            watermark: 0.0,
            min_middle_tokens: 4_000,
            ..Default::default()
        };
        let llm = Arc::new(ScriptLlm::new("m", Some(8_000), vec![]));
        let messages: Vec<LlmMessage> = (0..20).map(|_| long_text(200)).collect(); // ≈1000 token
        let out = c.maybe_compact(&*llm, &messages, "p", "m", None).await;
        assert!(out.is_none());
    }

    /// 摘要流失败（Finish Error）→ fail-safe None（保持原上下文）。
    #[tokio::test]
    async fn summary_failure_falls_back_to_original() {
        let c = DefaultCompactor {
            watermark: 0.0,
            min_middle_tokens: 1,
            ..Default::default()
        };
        let llm = Arc::new(ScriptLlm::new(
            "m",
            Some(8_000),
            vec![ScriptLlm::fail_stream()],
        ));
        let messages: Vec<LlmMessage> = (0..20).map(|_| long_text(200)).collect();
        let out = c.maybe_compact(&*llm, &messages, "p", "m", None).await;
        assert!(out.is_none(), "summary failure must not alter context");
    }

    /// 成功压缩：返回 System 摘要 + 尾部保留；摘要请求带 purpose=compaction、
    /// thinking=disabled、无工具、temperature 0.3。
    #[tokio::test]
    async fn compact_returns_system_summary_and_tail() {
        let c = DefaultCompactor {
            watermark: 0.0, // 强制触发
            min_middle_tokens: 1,
            ..Default::default()
        };
        let llm = Arc::new(ScriptLlm::new(
            "m",
            Some(128_000),
            vec![ScriptLlm::text_stream("用户要压缩聊天，结论是插件化。")],
        ));
        // 20 条 × 400 字 ≈ 2000 token：中部（tail 之外）足够大，tail 保留下限内。
        let messages: Vec<LlmMessage> = (0..20).map(|_| long_text(400)).collect();
        let out = c
            .maybe_compact(&*llm, &messages, "p", "m", None)
            .await
            .expect("compaction should succeed");
        // 变换后 = 1 条 System 摘要 + 尾部保留（keep 12.8K，400 字 × 4 ≈ 400 token/条
        // ——尾部在 12.8K/100 ≈ 32 条内，本用例 20 条全被保留则 middle 为空，会走
        // min_middle 拦截；因此断言长度 < 原长度即可，不锁死具体尾部条数）。
        assert!(out.len() < messages.len(), "context must shrink");
        assert_eq!(out[0].role, Role::System);
        assert!(matches!(&out[0].content[..], [ContentBlock::Text(t)] if t.contains("压缩")));

        // 摘要请求形态断言。
        let req = llm.last_request.lock().unwrap().clone().expect("summary request issued");
        assert_eq!(req.purpose.as_deref(), Some("compaction"));
        assert_eq!(req.thinking.as_deref(), Some("disabled"));
        assert_eq!(req.temperature, Some(0.3));
        assert!(req.tools.is_empty());
        assert_eq!(req.model, "m");
        assert!(req.messages.iter().any(|m| m.role == Role::User));
    }

    /// 真实窗口判定：provider 声明窗口很大（256K），默认窗口 128K 下已过水线、
    /// 但真实窗口水线未到 → 不压（正确尊重 provider 容量）。
    #[tokio::test]
    async fn respects_real_context_window() {
        let c = DefaultCompactor {
            watermark: 0.5,
            ..Default::default()
        };
        // 输入 70K token（超过 128K×0.5=64K 快速筛），但 provider 窗口 256K
        // → 真实水线 128K，未达 → 不压。
        let messages: Vec<LlmMessage> = (0..140).map(|_| long_text(2000)).collect();
        assert!(estimate_tokens(&messages) >= 70_000);
        let llm = Arc::new(ScriptLlm::new("m", Some(256_000), vec![]));
        let out = c.maybe_compact(&*llm, &messages, "p", "m", None).await;
        assert!(out.is_none());
    }
}
