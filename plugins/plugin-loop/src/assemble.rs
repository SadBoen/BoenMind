//! 增量 chunk 装配器（对齐 DSH `BlockAssembler`）。
//!
//! 原始 chunk 累积 → 消息内容块。协议：`BlockStart` 开块、delta 累积、
//! `BlockEnd` 权威冻结（首闭胜出，迟到增量/重闭忽略）、usage/finish 单独持有。
//! 纯装配逻辑，无 IO——独立模块便于单测（test: unknown_block_type_not_flattened_to_text）。

use std::collections::BTreeMap;

use kernel_contracts::llm::{
    ContentBlock, FinishReason, StreamChunk, TokenUsage, ToolCall,
};

/// 增量 chunk → 消息装配器：原始 chunk 累积，`block-end` 权威冻结，
/// delta-only 协议也容忍；usage/finish 单独持有。
#[derive(Default)]
pub struct BlockAssembler {
    partials: BTreeMap<usize, PartialBlock>,
    order: Vec<usize>,
    usage: Option<TokenUsage>,
    finish: Option<FinishReason>,
}

struct PartialBlock {
    block_type: String,
    text: String,
    tool_call_id: String,
    tool_call_name: String,
    /// block-end 已闭：权威块，冻结 partial。
    block: Option<ContentBlock>,
}

impl BlockAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &StreamChunk) {
        match chunk {
            StreamChunk::BlockStart { index, block_type } => {
                if !self.partials.contains_key(index) {
                    self.order.push(*index);
                    self.partials.insert(
                        *index,
                        PartialBlock {
                            block_type: block_type.clone(),
                            text: String::new(),
                            tool_call_id: String::new(),
                            tool_call_name: String::new(),
                            block: None,
                        },
                    );
                }
            }
            StreamChunk::TextDelta { index, text } | StreamChunk::ReasoningDelta { index, text } => {
                let is_text = matches!(chunk, StreamChunk::TextDelta { .. });
                let partial = self.ensure(*index, if is_text { "text" } else { "reasoning" });
                if partial.block.is_some() {
                    return; // 已闭：忽略迟到增量
                }
                partial.text.push_str(text);
            }
            StreamChunk::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                let partial = self.ensure(*index, "tool-call");
                if partial.block.is_some() {
                    return;
                }
                if !id.is_empty() {
                    partial.tool_call_id.clone_from(id);
                }
                if let Some(n) = name {
                    partial.tool_call_name.clone_from(n);
                }
                partial.text.push_str(arguments_delta);
            }
            StreamChunk::BlockEnd { index, block } => {
                let partial = self.ensure(*index, "");
                // 首闭胜出：忽略重闭迟到块。
                if partial.block.is_some() {
                    return;
                }
                partial.block = Some(block.clone());
            }
            StreamChunk::Usage(u) => {
                self.usage = Some(u.clone());
            }
            StreamChunk::Finish(reason) => {
                self.finish = Some(reason.clone());
            }
        }
    }

    fn ensure(&mut self, index: usize, block_type: &str) -> &mut PartialBlock {
        if !self.partials.contains_key(&index) {
            self.order.push(index);
        }
        self.partials
            .entry(index)
            .or_insert_with(|| PartialBlock {
                block_type: block_type.to_string(),
                text: String::new(),
                tool_call_id: String::new(),
                tool_call_name: String::new(),
                block: None,
            })
    }

    fn assemble(&self, index: usize, partial: &PartialBlock) -> Option<ContentBlock> {
        if let Some(b) = &partial.block {
            return Some(b.clone());
        }
        match partial.block_type.as_str() {
            "text" => Some(ContentBlock::Text(partial.text.clone())),
            "reasoning" => Some(ContentBlock::Reasoning(partial.text.clone())),
            "tool-call" => Some(ContentBlock::ToolCall(ToolCall {
                id: if partial.tool_call_id.is_empty() {
                    format!("call-{index}")
                } else {
                    partial.tool_call_id.clone()
                },
                name: partial.tool_call_name.clone(),
                arguments: partial.text.clone(),
            })),
            // 未知块类型：不产出消息块（绝不静默 flatten 成 Text——上游协议外
            // 类型要么有对应语义要么被丢弃，冒充文本会污染投影/模型输入）。
            _ => None,
        }
    }

    /// 组装全部已见块（按开块序）。max-tokens 截断丢弃无法安全执行的 tool-call 块。
    pub fn blocks(&self) -> Vec<ContentBlock> {
        let blocks: Vec<ContentBlock> = self
            .order
            .iter()
            .filter_map(|i| self.assemble(*i, self.partials.get(i).expect("order invariant")))
            .collect();
        if self.finish() == FinishReason::MaxTokens {
            blocks
                .into_iter()
                .filter(|b| !matches!(b, ContentBlock::ToolCall(_)))
                .collect()
        } else {
            blocks
        }
    }

    pub fn usage(&self) -> Option<TokenUsage> {
        self.usage.clone()
    }

    /// 是否收到过 Finish chunk（判 torn：流以 Finish 收尾等价于非 None）。
    pub fn has_finish(&self) -> bool {
        self.finish.is_some()
    }

    /// finish 缺省 stop（对齐 DSH `get finish()`）。
    pub fn finish(&self) -> FinishReason {
        self.finish.clone().unwrap_or(FinishReason::Stop)
    }
}