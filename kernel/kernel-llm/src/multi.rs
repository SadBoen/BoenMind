//! 多 provider 聚合路由（M3）：把多个 `LlmPort` 子实现按 provider id 编排为
//! 单一端口。`GenerateOptions.provider` 路由到对应子实现；未知 provider →
//! 空清单 / torn 错误流（fail-loud，绝不静默回落）。

use std::sync::Arc;

use async_trait::async_trait;
use kernel_contracts::error::LlmError;
use kernel_contracts::llm::{
    ChunkStream, GenerateOptions, LlmModelInfo, LlmPort, StreamChunk, FinishReason,
};

/// 按 provider 分派的聚合端口。
pub struct MultiProviderLlm {
    /// (provider id, 子实现)，顺序即 wire 显示序。
    providers: Vec<(String, Arc<dyn LlmPort>)>,
}

impl MultiProviderLlm {
    pub fn new(providers: Vec<(String, Arc<dyn LlmPort>)>) -> Self {
        Self { providers }
    }

    pub fn provider_ids(&self) -> Vec<String> {
        self.providers.iter().map(|(id, _)| id.clone()).collect()
    }

    fn route(&self, provider: &str) -> Option<Arc<dyn LlmPort>> {
        self.providers
            .iter()
            .find(|(id, _)| id == provider)
            .map(|(_, p)| Arc::clone(p))
    }
}

#[async_trait]
impl LlmPort for MultiProviderLlm {
    async fn list_models(&self, provider: &str) -> Result<Vec<LlmModelInfo>, LlmError> {
        match self.route(provider) {
            Some(p) => p.list_models(provider).await,
            None => Ok(Vec::new()),
        }
    }

    fn stream(&self, request: GenerateOptions) -> ChunkStream {
        match self.route(&request.provider) {
            Some(p) => p.stream(request),
            // 未知 provider：错误以 finish 呈现（对齐 DSH service.spec：
            // 未注册 provider → NO_ADAPTER 终态 error finish；不产 Err chunk，
            // 否则 loop 的 torn 分支会把 code 覆盖成 LLM_STREAM 并双回合收尾）。
            None => Box::pin(futures::stream::iter(vec![Ok(
                StreamChunk::Finish(FinishReason::Error {
                    message: format!(
                        "no adapter registered for provider '{}'",
                        request.provider
                    ),
                    code: "NO_ADAPTER".to_string(),
                }),
            )])),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use kernel_contracts::llm::{text_message, Role};

    #[test]
    fn provider_ids_preserve_order() {
        let m = MultiProviderLlm::new(vec![
            ("minimax".to_string(), Arc::new(dummy("minimax"))),
            ("deepseek".to_string(), Arc::new(dummy("deepseek"))),
        ]);
        assert_eq!(m.provider_ids(), vec!["minimax", "deepseek"]);
    }

    fn dummy(_id: &str) -> DummyLlm {
        DummyLlm
    }

    struct DummyLlm;

    #[async_trait]
    impl LlmPort for DummyLlm {
        async fn list_models(&self, _provider: &str) -> Result<Vec<LlmModelInfo>, LlmError> {
            Ok(vec![])
        }
        fn stream(&self, _request: GenerateOptions) -> ChunkStream {
            Box::pin(futures::stream::iter(vec![Ok(StreamChunk::Finish(
                FinishReason::Stop,
            ))]))
        }
    }

    #[tokio::test]
    async fn unknown_provider_returns_no_adapter_finish() {
        let m = MultiProviderLlm::new(vec![]);
        let mut s = m.stream(GenerateOptions {
            provider: "nope".into(),
            model: "m".into(),
            messages: vec![text_message(Role::User, "hi")],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            session_id: None,
        });
        let first = s.next().await;
        match first {
            Some(Ok(StreamChunk::Finish(FinishReason::Error { code, .. }))) => {
                assert_eq!(code, "NO_ADAPTER");
            }
            other => panic!("expected NO_ADAPTER finish, got {other:?}"),
        }
        // 只一个 chunk（不产 Err，避免 loop torn 覆盖 code）。
        assert!(s.next().await.is_none());
    }
}
