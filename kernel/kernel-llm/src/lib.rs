//! # kernel-llm
//!
//! `LlmPort` 的实现集：
//! - [`ScriptLlm`]（mock，门禁 1 用，不接真实 API）；
//! - [`openai::OpenAICompatLlm`]（M3 真 provider 三通道的 OpenAI 兼容底座）；
//! - [`MultiProviderLlm`]（按 `GenerateOptions.provider` 路由到子端口的聚合实现，
//!   ——多 provider 同时装配时，`LoopRuntime` 仍只需持有单一 `Arc<dyn LlmPort>`）。

mod openai;
mod multi;

pub use multi::MultiProviderLlm;
pub use openai::{ModelListEndpoint, OpenAiProviderConfig, OpenAICompatLlm};

// ScriptLlm 按脚本逐回合产出（对齐 DSH 流协议：block-start/delta/block-end 序）：
// 纯文本回合产出 BlockStart(text) + TextDelta + BlockEnd + Usage + Finish(Stop)；
// 工具回合产出 BlockStart(tool-call) + ToolCallDelta + BlockEnd + Usage + Finish(ToolCalls)，
// 并把 then_text 推入内部 followup 队列，下一次 stream() 调用优先消费 followup 产出文本。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_stream::stream;
use async_trait::async_trait;
use kernel_contracts::{
    ChunkStream, ContentBlock, FinishReason, GenerateOptions, LlmError, LlmModelInfo, LlmPort,
    StreamChunk, TokenUsage, ToolCall,
};
use serde_json::Value;

/// 一回合 mock 输出。
#[derive(Debug, Clone)]
pub enum MockTurn {
    /// 纯文本回合：产出 BlockStart(text) + TextDelta + BlockEnd + Finish(Stop)。
    Text(String),
    /// 工具回合：产出 BlockStart(tool-call) + ToolCallDelta + BlockEnd + Finish(ToolCalls)，
    /// 并把 `then_text` 推入内部 followup 队列（下一轮调用先消费 followup 产出文本）。
    Tool {
        name: String,
        arguments: Value,
        then_text: String,
    },
}

/// 脚本化 mock LLM：按脚本逐回合产出，工具回合的 followup 文本在下一轮被消费。
pub struct ScriptLlm {
    provider: String,
    model: String,
    script: Arc<Mutex<VecDeque<MockTurn>>>,
    followups: Arc<Mutex<VecDeque<String>>>,
}

impl ScriptLlm {
    pub fn new(provider: String, model: String, script: Vec<MockTurn>) -> Self {
        Self {
            provider,
            model,
            script: Arc::new(Mutex::new(script.into_iter().collect())),
            followups: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

#[async_trait]
impl LlmPort for ScriptLlm {
    async fn list_models(&self, provider: &str) -> Result<Vec<LlmModelInfo>, LlmError> {
        if provider != self.provider {
            return Ok(Vec::new());
        }
        Ok(vec![LlmModelInfo {
            id: self.model.clone(),
            label: None,
            supports_tools: true,
            context_window: None,
            max_tokens: None,
            reasoning: None,
        }])
    }

    fn stream(&self, _request: GenerateOptions) -> ChunkStream {
        let script = Arc::clone(&self.script);
        let followups = Arc::clone(&self.followups);
        Box::pin(stream! {
            // 1) 优先消费 followup 队列（工具回合的 then_text）
            let followup = { followups.lock().unwrap().pop_front() };
            if let Some(text) = followup {
                let output = text.chars().count() as u64;
                yield Ok(StreamChunk::BlockStart { index: 0, block_type: "text".to_string() });
                yield Ok(StreamChunk::TextDelta { index: 0, text: text.clone() });
                yield Ok(StreamChunk::BlockEnd { index: 0, block: ContentBlock::Text(text) });
                yield Ok(StreamChunk::Usage(TokenUsage {
                    input: 1,
                    output,
                    cache_read: None,
                    cache_write: None,
                    reasoning: None,
                }));
                yield Ok(StreamChunk::Finish(FinishReason::Stop));
            } else {
                // 2) 否则 pop 脚本
                let turn = { script.lock().unwrap().pop_front() };
                match turn {
                    Some(MockTurn::Text(text)) => {
                        let output = text.chars().count() as u64;
                        yield Ok(StreamChunk::BlockStart { index: 0, block_type: "text".to_string() });
                        yield Ok(StreamChunk::TextDelta { index: 0, text: text.clone() });
                        yield Ok(StreamChunk::BlockEnd { index: 0, block: ContentBlock::Text(text) });
                        yield Ok(StreamChunk::Usage(TokenUsage {
                            input: 1,
                            output,
                            cache_read: None,
                            cache_write: None,
                            reasoning: None,
                        }));
                        yield Ok(StreamChunk::Finish(FinishReason::Stop));
                    }
                    Some(MockTurn::Tool {
                        name,
                        arguments,
                        then_text,
                    }) => {
                        // 注意：必须在 yield 之前入队——async_stream 是惰性的，
                        // 消费者在收到 Finish 后 break，yield 之后的代码不会执行。
                        followups.lock().unwrap().push_back(then_text);
                        let arguments_json = serde_json::to_string(&arguments)
                            .unwrap_or_else(|_| "{}".to_string());
                        yield Ok(StreamChunk::BlockStart { index: 0, block_type: "tool-call".to_string() });
                        yield Ok(StreamChunk::ToolCallDelta {
                            index: 0,
                            id: "call_0".to_string(),
                            name: Some(name.clone()),
                            arguments_delta: arguments_json.clone(),
                        });
                        yield Ok(StreamChunk::BlockEnd {
                            index: 0,
                            block: ContentBlock::ToolCall(ToolCall {
                                id: "call_0".to_string(),
                                name,
                                arguments: arguments_json,
                            }),
                        });
                        yield Ok(StreamChunk::Usage(TokenUsage {
                            input: 1,
                            output: 1,
                            cache_read: None,
                            cache_write: None,
                            reasoning: None,
                        }));
                        yield Ok(StreamChunk::Finish(FinishReason::ToolCalls));
                    }
                    // 3) 空脚本 + 空 followup：空回合安全返回
                    None => {
                        yield Ok(StreamChunk::Finish(FinishReason::Stop));
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use kernel_contracts::{text_message, Role};

    fn options() -> GenerateOptions {
        GenerateOptions {
            provider: "mock".to_string(),
            model: "mock-1".to_string(),
            messages: vec![text_message(Role::User, "hello")],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            session_id: None,
            signal: None,
            reasoning_effort: None,
            thinking: None,
            purpose: None,
        }
    }

    fn collect(stream: ChunkStream) -> Vec<StreamChunk> {
        futures::executor::block_on(async {
            let mut stream = stream;
            let mut chunks = Vec::new();
            while let Some(chunk) = stream.next().await {
                chunks.push(chunk.expect("stream item should not be an error"));
            }
            chunks
        })
    }

    #[test]
    fn text_turn_yields_blocks_usage_finish() {
        let llm = ScriptLlm::new(
            "mock".to_string(),
            "mock-1".to_string(),
            vec![MockTurn::Text("hello world".to_string())],
        );
        let chunks = collect(llm.stream(options()));
        assert_eq!(
            chunks,
            vec![
                StreamChunk::BlockStart { index: 0, block_type: "text".to_string() },
                StreamChunk::TextDelta { index: 0, text: "hello world".to_string() },
                StreamChunk::BlockEnd {
                    index: 0,
                    block: ContentBlock::Text("hello world".to_string())
                },
                StreamChunk::Usage(TokenUsage {
                    input: 1,
                    output: 11,
                    cache_read: None,
                    cache_write: None,
                    reasoning: None,
                }),
                StreamChunk::Finish(FinishReason::Stop),
            ]
        );
    }

    #[test]
    fn tool_turn_then_next_call_consumes_followup_text() {
        let llm = ScriptLlm::new(
            "mock".to_string(),
            "mock-1".to_string(),
            vec![MockTurn::Tool {
                name: "echo".to_string(),
                arguments: serde_json::json!({ "text": "hi" }),
                then_text: "done".to_string(),
            }],
        );

        // 第一轮：工具回合
        let chunks1 = collect(llm.stream(options()));
        assert_eq!(
            chunks1,
            vec![
                StreamChunk::BlockStart { index: 0, block_type: "tool-call".to_string() },
                StreamChunk::ToolCallDelta {
                    index: 0,
                    id: "call_0".to_string(),
                    name: Some("echo".to_string()),
                    arguments_delta: r#"{"text":"hi"}"#.to_string(),
                },
                StreamChunk::BlockEnd {
                    index: 0,
                    block: ContentBlock::ToolCall(ToolCall {
                        id: "call_0".to_string(),
                        name: "echo".to_string(),
                        arguments: r#"{"text":"hi"}"#.to_string(),
                    }),
                },
                StreamChunk::Usage(TokenUsage {
                    input: 1,
                    output: 1,
                    cache_read: None,
                    cache_write: None,
                    reasoning: None,
                }),
                StreamChunk::Finish(FinishReason::ToolCalls),
            ]
        );

        // 第二轮：消费 followup 文本
        let chunks2 = collect(llm.stream(options()));
        assert_eq!(
            chunks2,
            vec![
                StreamChunk::BlockStart { index: 0, block_type: "text".to_string() },
                StreamChunk::TextDelta { index: 0, text: "done".to_string() },
                StreamChunk::BlockEnd {
                    index: 0,
                    block: ContentBlock::Text("done".to_string())
                },
                StreamChunk::Usage(TokenUsage {
                    input: 1,
                    output: 4,
                    cache_read: None,
                    cache_write: None,
                    reasoning: None,
                }),
                StreamChunk::Finish(FinishReason::Stop),
            ]
        );
    }

    #[test]
    fn empty_script_yields_empty_finish_without_panicking() {
        let llm = ScriptLlm::new("mock".to_string(), "mock-1".to_string(), Vec::new());
        let chunks = collect(llm.stream(options()));
        assert_eq!(chunks, vec![StreamChunk::Finish(FinishReason::Stop)]);
    }

    #[test]
    fn list_models_matches_provider() {
        let llm = ScriptLlm::new(
            "mock".to_string(),
            "mock-1".to_string(),
            Vec::new(),
        );
        let models = futures::executor::block_on(llm.list_models("mock")).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "mock-1");
        assert!(models[0].supports_tools);
        let others = futures::executor::block_on(llm.list_models("openai")).unwrap();
        assert!(others.is_empty());
    }
}
