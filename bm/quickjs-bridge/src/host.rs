//! 真宿主实现（§5.4）：把内核契约端口拧成 `HostApi` 实现，供组合根装配。
//!
//! [`HostApi`] 的 8 个方法中，5 个（log/config/tools/session）在组合根直接可得；
//! 唯一需要内核契约端口的是 **LLM**（`llm_complete_stream`）——它把 JS 插件的
//! `host.llm.complete(req)` 翻译成 `LlmPort::stream(GenerateOptions)`（走与 agent-loop
//! 同一契约端口、同一 provider 聚合路由），流式块经 [`CompletionChunk`] 转回 JSON。
//!
//! 因此本模块只含一个可复用的最小件：**[`translate_llm_chunks`]**——`LlmPort` 流 →
//! JS 侧 `CompletionChunk` JSON 数组。真实 `HostApi` 实现（`RealHost`）留在组合根
//! `bm-assembly`（装配点唯一组合根纪律，见该 crate 文档），quickjs-bridge 不与
//! web-server / 具体 provider 适配器耦合。
//!
//! 边界（与 [`crate`] 顶部同源）：
//! - 不做 turn 语义：JS 插件当 Tool/Policy，不当第二 Agent；
//! - 消息只走文本块（`LlmMessage` 含工具块 → 显式 `UNSUPPORTED_CONTENT` 错误，
//!   绝不静默 flatten）；
//! - provider 未知 → `NO_ADAPTER` 终态（对齐 MultiProviderLlm，不产 torn 流）。

use std::sync::Arc;

use futures::StreamExt;
use kernel_contracts::llm::{text_message, FinishReason, LlmMessage, Role, StreamChunk};

use crate::{CompletionChunk, HostError, HostResult};

/// 跨桥 `ChatMessage` 序列 → 内核 `LlmMessage` 序列（text-only）。
///
/// 桥只传文本块（`ChatMessage.content` 是 `String`）；角色词表映射内核 `Role`，
/// 未知角色显式报错（绝不静默归一）。JS 插件无法携带工具块/推理块过桥（跨桥
/// 最小子集）——工具调用增量由插件自行拼装处理。
pub fn to_kernel_messages(messages: &[crate::ChatMessage]) -> Result<Vec<LlmMessage>, HostError> {
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        let role = match m.role.as_str() {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "system" => Role::System,
            "tool" => Role::Tool,
            other => {
                return Err(HostError::new(
                    "UNSUPPORTED_ROLE",
                    format!("JS host llm: unknown role '{other}'"),
                ))
            }
        };
        out.push(text_message(role, m.content.clone()));
    }
    Ok(out)
}

/// 桥工具声明 → 内核 `ToolSchema`（形状一致，纯拷贝）。
pub fn to_kernel_tools(tools: &[crate::ToolSpec]) -> Vec<kernel_contracts::tools::ToolSchema> {
    tools
        .iter()
        .map(|t| kernel_contracts::tools::ToolSchema {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
        })
        .collect()
}

/// 把一次 `LlmPort::stream` 的完整流消费为 `CompletionChunk` JSON 数组。
///
/// 这是 §5.4 的核心翻译：**不做 turn/工具循环语义**——只把内核块流逐块转成
/// JS 侧可见的增量块序列（含块索引，JS 可自组文本/tool-call 块）。流以
/// `Err` 结束（torn）→ 转成 `{type:'finish', reason:'error', code:'STREAM_CLOSED'}`
/// 终态块；以 `Finish{Error}` 结束 → `reason:'error'` + code/message；正常 →
/// `reason` 透传。翻译失败（chunk 无法表示）→ 提前终止，产出同形 finish 错误块。
///
/// 不产出 `usage` 块（`TokenUsage` 是内核 wire 细节，JS 编排不需要）。
pub async fn translate_llm_chunks<S>(stream: S) -> Vec<CompletionChunk>
where
    S: futures::Stream<Item = Result<StreamChunk, kernel_contracts::LlmError>> + Send,
{
    consume(stream, None).await
}

/// 桥请求 + 内核 LLM 端口 → 完整 `HostResult`（§5.4 组合根/默认实现的共用入口）。
///
/// 组装：桥消息/工具 → 内核 `GenerateOptions`（text-only；`session_id` 不跨桥——
/// JS 插件无会话回合语义）→ `LlmPort::stream` → 消费 → `{ok, value:{chunks}}`。
/// 取消经 `cancel` 订阅触发内核 `AbortSignal`（provider 应停止拉取并以
/// `finish{reason:'cancelled'}` 收尾）。
pub async fn complete_with_port(
    llm: Arc<dyn kernel_contracts::llm::LlmPort>,
    request: crate::CompleteRequest,
    cancel: crate::Cancellation,
) -> HostResult {
    let messages = match to_kernel_messages(&request.messages) {
        Ok(m) => m,
        Err(e) => return HostResult::err(e),
    };
    let signal = kernel_contracts::llm::AbortSignal::new();
    let stream = llm.stream(kernel_contracts::llm::GenerateOptions {
        provider: request.provider,
        model: request.model,
        messages,
        tools: to_kernel_tools(&request.tools.unwrap_or_default()),
        temperature: request.temperature,
        max_tokens: request.max_tokens.map(u64::from),
        session_id: None,
        signal: Some(signal.clone()),
        reasoning_effort: None,
        thinking: None,
        purpose: None,
    });
    let chunks = consume(stream, Some((signal, cancel))).await;
    HostResult::ok(serde_json::json!({ "chunks": chunks }))
}

/// 消费内核块流 → JS 侧块序列。`cancel` 提供时订阅取消信号并触发
/// `AbortSignal::abort()`（provider 应以 `finish{reason:'cancelled'}` 或
/// `Err` 收尾；取消后继续消费到流结束，保证无 torn 尾）。
async fn consume<S>(
    stream: S,
    cancel: Option<(kernel_contracts::llm::AbortSignal, crate::Cancellation)>,
) -> Vec<CompletionChunk>
where
    S: futures::Stream<Item = Result<StreamChunk, kernel_contracts::LlmError>> + Send,
{
    futures::pin_mut!(stream);
    let mut chunks = Vec::new();
    let mut finish: Option<CompletionChunk> = None;
    let mut aborted = false;
    loop {
        let item = if let Some((signal, cancel)) = &cancel {
            tokio::select! {
                item = stream.next() => item,
                _ = cancel.token.notified() => {
                    if !aborted {
                        aborted = true;
                        signal.abort();
                    }
                    continue; // 等流以终态收尾（provider 应响应 abort）
                }
            }
        } else {
            stream.next().await
        };
        let Some(item) = item else { break };
        match item {
            Ok(c) => match translate_chunk(c) {
                // 终态块先存：流结束（或 Err）时统一落到数组尾部；正常结束则以
                // 流内最后一个 Finish 收尾（后到覆盖先到，对齐内核单终态契约）。
                Some(ch @ CompletionChunk::Finish { .. }) => finish = Some(ch),
                Some(ch) => chunks.push(ch),
                None => {}
            },
            Err(e) => {
                chunks.push(CompletionChunk::Finish {
                    reason: "error".to_string(),
                    code: Some("STREAM_CLOSED".to_string()),
                    message: Some(e.message),
                });
                return chunks;
            }
        }
    }
    // 流正常结束时以 Finish 收尾（torn 纪律：Finish 缺失 = 中断，须补终态）。
    chunks.push(finish.unwrap_or_else(|| CompletionChunk::Finish {
        reason: "error".to_string(),
        code: Some("STREAM_CLOSED".to_string()),
        message: Some("stream ended without Finish (torn)".to_string()),
    }));
    chunks
}

/// 单块翻译：`StreamChunk` → JS 侧 `CompletionChunk`；无法表示（usage）→ None。
fn translate_chunk(c: StreamChunk) -> Option<CompletionChunk> {
    match c {
        StreamChunk::BlockStart { .. } => None, // 块起止/用量是 wire 细节，JS 编排不需要
        StreamChunk::BlockEnd { .. } => None,
        StreamChunk::TextDelta { text, .. } => Some(CompletionChunk::TextDelta { text }),
        StreamChunk::ReasoningDelta { text, .. } => {
            // 推理增量：JS 侧无独立面，折叠进文本（插件的"想法"不单独暴露）。
            Some(CompletionChunk::TextDelta { text })
        }
        StreamChunk::ToolCallDelta { index, name, arguments_delta, .. } => {
            Some(CompletionChunk::ToolCallDelta {
                index: index as u32,
                name,
                arguments: Some(arguments_delta),
            })
        }
        StreamChunk::Usage(_) => None,
        StreamChunk::Finish(reason) => {
            let (reason, code, message) = match reason {
                FinishReason::Stop => ("stop".to_string(), None, None),
                FinishReason::MaxTokens => ("max-tokens".to_string(), None, None),
                FinishReason::ToolCalls => ("tool-calls".to_string(), None, None),
                FinishReason::Cancelled => ("cancelled".to_string(), None, None),
                FinishReason::Error { message, code, .. } => {
                    ("error".to_string(), Some(code), Some(message))
                }
            };
            Some(CompletionChunk::Finish { reason, code, message })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use kernel_contracts::error::LlmError;
    use kernel_contracts::llm::{text_message, ContentBlock, GenerateOptions, LlmModelInfo, LlmPort, Role};

    /// 脚本化 LlmPort（测试用，与 assembly::scripted_llm 同形）。
    pub struct ScriptLlm {
        provider: String,
        model: String,
        chunks: Vec<StreamChunk>,
    }

    impl ScriptLlm {
        pub fn new(provider: String, model: String, chunks: Vec<StreamChunk>) -> Self {
            Self { provider, model, chunks }
        }
    }

    #[async_trait::async_trait]
    impl LlmPort for ScriptLlm {
        async fn list_models(
            &self,
            _provider: &str,
        ) -> Result<Vec<LlmModelInfo>, LlmError> {
            Ok(vec![])
        }
        fn stream(&self, request: GenerateOptions) -> kernel_contracts::ChunkStream {
            assert_eq!(request.provider, self.provider);
            assert_eq!(request.model, self.model);
            let chunks = self.chunks.clone();
            Box::pin(stream::iter(chunks.into_iter().map(Ok)))
        }
    }

    fn options(provider: &str) -> GenerateOptions {
        GenerateOptions {
            provider: provider.to_string(),
            model: "m".to_string(),
            messages: vec![text_message(Role::User, "hi")],
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

    fn collect(llm: &dyn LlmPort, provider: &str) -> Vec<CompletionChunk> {
        let stream = llm.stream(options(provider));
        futures::executor::block_on(translate_llm_chunks(stream))
    }

    #[test]
    fn text_deltas_plus_stop() {
        let llm = ScriptLlm::new(
            "p".into(),
            "m".into(),
            vec![
                StreamChunk::TextDelta { index: 0, text: "hi".into() },
                StreamChunk::TextDelta { index: 0, text: " there".into() },
                StreamChunk::Finish(FinishReason::Stop),
            ],
        );
        let chunks = collect(&llm, "p");
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], CompletionChunk::TextDelta { text: "hi".into() });
        assert_eq!(chunks[1], CompletionChunk::TextDelta { text: " there".into() });
        assert!(matches!(&chunks[2], CompletionChunk::Finish { reason, code: None, message: None } if reason == "stop"));
    }

    #[test]
    fn error_finish_carries_code_and_message() {
        let llm = ScriptLlm::new(
            "p".into(),
            "m".into(),
            vec![StreamChunk::Finish(FinishReason::Error {
                message: "boom".into(),
                code: "E42".into(),
                extra: None,
            })],
        );
        let chunks = collect(&llm, "p");
        assert_eq!(chunks.len(), 1);
        assert!(matches!(
            &chunks[0],
            CompletionChunk::Finish { reason, code: Some(c), message: Some(m) }
                if reason == "error" && c == "E42" && m == "boom"
        ));
    }

    #[test]
    fn torn_stream_closes_with_stream_closed() {
        // 流以 Err 结束（无 Finish）→ 补 STREAM_CLOSED 终态（torn 纪律）。
        // 用包装 llm 模拟 Err：覆写 stream 返回 Err 流。
        struct ErrLlm(ScriptLlm);
        #[async_trait::async_trait]
        impl LlmPort for ErrLlm {
            async fn list_models(&self, p: &str) -> Result<Vec<LlmModelInfo>, LlmError> {
                self.0.list_models(p).await
            }
            fn stream(&self, request: GenerateOptions) -> kernel_contracts::ChunkStream {
                let _ = self.0.stream(request);
                Box::pin(stream::iter(vec![Err(LlmError {
                    message: "net down".into(),
                    code: Some("NET".into()),
                    retryable: true,
                    status: None,
                    provider_retry_after_ms: None,
                    request_id: None,
                })]))
            }
        }
        let llm = ErrLlm(ScriptLlm::new(
            "p".into(),
            "m".into(),
            vec![StreamChunk::TextDelta { index: 0, text: "partial".into() }],
        ));
        let chunks = collect(&llm, "p");
        assert_eq!(chunks.len(), 1);
        assert!(matches!(
            &chunks[0],
            CompletionChunk::Finish { reason, code: Some(c), message: Some(_) }
                if reason == "error" && c == "STREAM_CLOSED"
        ));
    }

    #[test]
    fn missing_finish_treated_as_torn() {
        // 流正常结束但无 Finish 块 → 补 torn 终态（对齐 loop 的 torn 判定）。
        let llm = ScriptLlm::new("p".into(), "m".into(), vec![]);
        let chunks = collect(&llm, "p");
        assert_eq!(chunks.len(), 1);
        assert!(matches!(
            &chunks[0],
            CompletionChunk::Finish { reason, code: Some(c), .. }
                if reason == "error" && c == "STREAM_CLOSED"
        ));
    }

    #[test]
    fn tool_call_delta_passes_through() {
        let llm = ScriptLlm::new(
            "p".into(),
            "m".into(),
            vec![
                StreamChunk::ToolCallDelta {
                    index: 0,
                    id: "t1".into(),
                    name: Some("echo".into()),
                    arguments_delta: "{}".into(),
                },
                StreamChunk::Finish(FinishReason::ToolCalls),
            ],
        );
        let chunks = collect(&llm, "p");
        assert_eq!(chunks.len(), 2);
        assert!(matches!(
            &chunks[0],
            CompletionChunk::ToolCallDelta { index, name: Some(n), arguments: Some(a) }
                if *index == 0 && n == "echo" && a == "{}"
        ));
        assert!(matches!(&chunks[1], CompletionChunk::Finish { reason, .. } if reason == "tool-calls"));
    }

    #[test]
    fn reasoning_delta_folded_into_text() {
        let llm = ScriptLlm::new(
            "p".into(),
            "m".into(),
            vec![
                StreamChunk::ReasoningDelta { index: 0, text: "think".into() },
                StreamChunk::Finish(FinishReason::Stop),
            ],
        );
        let chunks = collect(&llm, "p");
        assert_eq!(chunks[0], CompletionChunk::TextDelta { text: "think".into() });
    }

    #[test]
    fn unknown_role_rejected() {
        let messages = vec![crate::ChatMessage {
            role: "robot".into(),
            content: "hi".into(),
        }];
        let err = to_kernel_messages(&messages).unwrap_err();
        assert_eq!(err.code, "UNSUPPORTED_ROLE");
    }

    #[test]
    fn bridge_messages_map_to_kernel() {
        let messages = vec![
            crate::ChatMessage { role: "system".into(), content: "sys".into() },
            crate::ChatMessage { role: "user".into(), content: "hi".into() },
            crate::ChatMessage { role: "assistant".into(), content: "ok".into() },
        ];
        let out = to_kernel_messages(&messages).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].role, Role::System);
        assert_eq!(out[1].role, Role::User);
        assert_eq!(out[2].role, Role::Assistant);
        assert!(matches!(&out[1].content[0], ContentBlock::Text(t) if t == "hi"));
    }

    #[test]
    fn tools_copy_across() {
        let tools = vec![crate::ToolSpec {
            name: "echo".into(),
            description: "echo back".into(),
            parameters: serde_json::json!({ "type": "object" }),
        }];
        let out = to_kernel_tools(&tools);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "echo");
        assert_eq!(out[0].parameters, serde_json::json!({ "type": "object" }));
    }
}
