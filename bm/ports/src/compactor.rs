//! 上下文压缩策略端口（产品契约层）。
//!
//! 长会话上下文压缩：输入 token 达到水线时，把历史中部摘要成一条
//! `System` 消息，替换为「摘要 + 尾部保留」的模型上下文——**日志不改、
//! 前端无感**。
//!
//! 架构约束（v2.1 定调）：`kernel/` 是外部 submodule 不改；事件日志是
//! 唯一事实源且 append-only（不可物理删中间事件）。因此压缩是**运行态
//! 视图变换**：日志完整保留（审计/回看），`Session::derive_messages()`
//! 的输出经 Compactor 变换后才发给模型。模型可见的一切仍来自日志投影，
//! 只是视图被压缩；聊天界面的历史投影（`session.history`）不受影响。
//!
//! 端口合同（loop 只问两件事）：[`Compactor::should_compact`]（压不压）与
//! [`Compactor::summarize_request`]（怎么压）。事务执行（选区间/摘要/变换）
//! 由 [`Compactor::maybe_compact`] 默认实现完成——事务协议只有一份、由
//! 契约层定为基线；策略（水线/保留预算/摘要 prompt）随实现自治。loop 只
//! 装配与调用。
//!
//! **无装配 = 优雅透传**：`LoopRuntime.compactor = None` 时上下文原样发给
//! 模型，无任何行为变化；装配后软水线触发，未达水线的正常对话经第一道
//! 快速筛直接透传（零 I/O、零开销）。
//! **fail-safe**：摘要失败 / 非 stop finish / 空摘要 → `None`（保持原上下文，
//! 不遮蔽、不阻塞回合——宁可让模型看到完整历史也不静默丢信息）。

use kernel_contracts::llm::{
    AbortSignal, ContentBlock, FinishReason, GenerateOptions, LlmMessage, LlmPort, Role,
    StreamChunk,
};
use futures::StreamExt;

/// 默认上下文窗口（provider 未声明 `context_window` 时的回退值；与 legacy
/// 前端 TokenRing 的参考窗口 128K 一致）。
pub const DEFAULT_CONTEXT_WINDOW: u64 = 128_000;
/// 摘要 prompt 的最大长度（字符）：超过则截断，防摘要请求自身超窗。
const MAX_DIALOGUE_CHARS: usize = 30_000;
/// 单条工具结果/参数进摘要的最大长度（字符）。
const MAX_TOOL_OUTPUT_CHARS: usize = 400;

/// 压缩策略端口：策略与事务分离——调用方（loop）只问「压不压」与
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

/// 文本 token 粗估：chars/4（中英混合近似，legacy 同款；精度不足由
/// 水线判定容忍——粗估只是触发判据，不参与计费）。除默认实现外，
/// 策略实现也可用它做预算计算，故公开。
pub fn estimate_tokens(messages: &[LlmMessage]) -> u64 {
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

/// 拼装可摘要文本（供默认事务实现使用；策略侧自建摘要 prompt 可复用）。
/// 角色 + 内容；思维链（reasoning）剔除不进摘要，工具结果/参数截断防
/// 注入超长文本。
pub fn build_dialogue(messages: &[LlmMessage]) -> String {
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