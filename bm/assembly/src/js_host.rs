//! QuickJS 插件宿主装配（§5.4 接真 LLM）：把组合根装配好的内核端口拧成
//! [`quickjs_bridge::HostApi`] 实现。
//!
//! 组合根是唯一装配点（唯一组合根纪律）：web-server / headless 不直接依赖
//! quickjs-bridge 的宿主实现。`RealHost` 把当前装配的 LLM（聚合 `LlmPort`，
//! 与 agent-loop 共享同一 `Arc`——swap_llm 后对 JS 插件同样下一请求生效）
//! 经 `HostApi::llm_port` 接进桥；其余面（log/config/tools/session）在 JS
//! 插件编排跑通前留占位实现（诚实报错，不假成功）。
//!
//! 边界（与 quickjs-bridge crate 顶部同源）：JS 插件当 Tool/Policy，不当
//! 第二 Agent；`host.llm.complete` 只做文本补全，不做 turn/工具循环。

use std::sync::Arc;

use kernel_contracts::llm::LlmPort;
use quickjs_bridge::{HostApi, HostError, HostResult};
use serde_json::Value;

/// 真宿主：组合根装配的 `HostApi` 实现。LLM 面已接线（`llm_port`）；
/// 其余面占位（按 manifest 接入后续面时替换占位实现）。
pub struct RealHost {
    llm: Arc<dyn LlmPort>,
}

impl RealHost {
    pub fn new(llm: Arc<dyn LlmPort>) -> Self {
        Self { llm }
    }
}

#[async_trait::async_trait]
impl HostApi for RealHost {
    fn log(&self, level: &str, msg: &str) {
        tracing::info!(level = %level, "js plugin: {msg}");
    }

    fn config_get(&self, plugin_id: &str, key: &str) -> HostResult {
        HostResult::err(HostError::new(
            "config-not-found",
            format!("config face not wired yet for {plugin_id}.{key}"),
        ))
    }

    fn tools_list(&self) -> HostResult {
        HostResult::ok(serde_json::json!([]))
    }

    async fn tools_invoke(&self, name: &str, _arguments: Value) -> HostResult {
        HostResult::err(HostError::new(
            "tool-not-found",
            format!("tool {name} not available (tools face not wired yet)"),
        ))
    }

    fn session_append(&self, _session_id: &str, _event: Value) -> HostResult {
        HostResult::err(HostError::new(
            "session-unavailable",
            "session face not wired yet",
        ))
    }

    fn session_get(&self, _session_id: &str) -> HostResult {
        HostResult::err(HostError::new(
            "session-unavailable",
            "session face not wired yet",
        ))
    }

    fn session_poll(&self, _session_id: &str, _cursor: u64) -> HostResult {
        HostResult::err(HostError::new(
            "session-unavailable",
            "session face not wired yet",
        ))
    }

    /// LLM 面：返回当前装配的聚合 LLM（与 agent-loop 同一 `Arc`）。
    fn llm_port(&self) -> Result<Arc<dyn LlmPort>, HostError> {
        Ok(Arc::clone(&self.llm))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use quickjs_bridge::{Cancellation, ChatMessage, CompleteRequest};
    use serde_json::json;

    /// 脚本化 LLM：产一个 text-delta + stop。
    struct StubLlm;
    #[async_trait::async_trait]
    impl LlmPort for StubLlm {
        async fn list_models(
            &self,
            _provider: &str,
        ) -> Result<Vec<kernel_contracts::LlmModelInfo>, kernel_contracts::error::LlmError>
        {
            Ok(vec![])
        }
        fn stream(
            &self,
            request: kernel_contracts::llm::GenerateOptions,
        ) -> kernel_contracts::ChunkStream {
            let _ = request;
            Box::pin(futures::stream::iter(vec![
                Ok(kernel_contracts::llm::StreamChunk::TextDelta {
                    index: 0,
                    text: "hello from real host".to_string(),
                }),
                Ok(kernel_contracts::llm::StreamChunk::Finish(
                    kernel_contracts::llm::FinishReason::Stop,
                )),
            ]))
        }
    }

    #[test]
    fn real_host_wires_llm_port_into_host_api() {
        let host = RealHost::new(Arc::new(StubLlm));
        let req = CompleteRequest {
            provider: "minimax".into(),
            model: "MiniMax-M3".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            tools: None,
            temperature: None,
            max_tokens: None,
        };
        let r = block_on(host.llm_complete_stream(req, Cancellation::new()));
        assert!(matches!(r, HostResult::Ok { .. }), "got {r:?}");
        let value = match r {
            HostResult::Ok { value } => value,
            _ => unreachable!(),
        };
        assert_eq!(value["chunks"][0]["type"], json!("text-delta"));
        assert_eq!(value["chunks"][0]["text"], json!("hello from real host"));
        assert_eq!(value["chunks"][1]["reason"], json!("stop"));
    }
}
