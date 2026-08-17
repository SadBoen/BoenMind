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
            // 未知 provider：torn 错误流（调用方以 Finish 缺失判 torn，绝不静默空转）。
            None => Box::pin(futures::stream::iter(vec![
                Err(LlmError::new(format!(
                    "no llm provider registered for '{}'",
                    request.provider
                ))),
                Ok(StreamChunk::Finish(FinishReason::Error)),
            ])),
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
    async fn unknown_provider_returns_torn_error_stream() {
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
        assert!(matches!(first, Some(Err(_))), "unknown provider must error");
    }
}
