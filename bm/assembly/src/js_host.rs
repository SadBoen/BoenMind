//! QuickJS 插件宿主装配（§5.4 接真 LLM + §5.5 tools/session 面接线）：把组合根
//! 装配好的内核端口拧成 [`quickjs_bridge::HostApi`] 实现。
//!
//! 组合根是唯一装配点（唯一组合根纪律）：web-server / headless 不直接依赖
//! quickjs-bridge 的宿主实现。`RealHost` 把当前装配的内核端口经
//! `HostApi` 接进桥：
//! - **llm**：当前装配的聚合 `LlmPort`（与 agent-loop 共享同一 `Arc`——
//!   swap_llm 后对 JS 插件同样下一请求生效）；
//! - **tools**：`ToolRegistry` + `ToolGate`（与 agent-loop 同一门控语义，
//!   fail-closed：未启用工具一律拒绝，`tool-disabled`）；
//! - **session**：`SessionStore` 只读投影 + 追加（拉模型，禁止 JS 回调重入）。
//!
//! 边界（与 quickjs-bridge crate 顶部同源）：JS 插件当 Tool/Policy，不当
//! 第二 Agent；`host.llm.complete` 只做文本补全，不做 turn/工具循环。

use std::collections::HashMap;
use std::sync::Arc;

use kernel_contracts::llm::LlmPort;
use kernel_contracts::session::SessionEvent;
use kernel_contracts::tools::ToolExecutionInput;
use kernel_contracts::ToolHandler;
use kernel_session::SessionStore;
use plugin_tools::{ToolGate, ToolRegistry};
use quickjs_bridge::{HostApi, HostError, HostResult};
use serde_json::{json, Value};

/// 真宿主：组合根装配的 `HostApi` 实现。llm / tools / session / config 面已接线。
pub struct RealHost {
    llm: Arc<dyn LlmPort>,
    tools: Arc<ToolRegistry>,
    gate: Arc<ToolGate>,
    store: Arc<SessionStore>,
    /// config 面白名单：`"{plugin_id}.{key}" → value`。白名单即全部内容——
    /// **永不返回 secret**（credentials/API key 由凭据面管，不进此表）。
    config: HashMap<String, String>,
}

impl RealHost {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm: Arc<dyn LlmPort>,
        tools: Arc<ToolRegistry>,
        gate: Arc<ToolGate>,
        store: Arc<SessionStore>,
        config: HashMap<String, String>,
    ) -> Self {
        Self { llm, tools, gate, store, config }
    }

    /// 执行一个工具（对齐 agent-loop 门控语义：先查注册、再查启用——
    /// 未注册 `tool-not-found`（对齐 MockHost 契约），注册但未启用
    /// `tool-disabled`（fail-closed））。
    async fn run_tool(&self, name: &str, arguments: Value) -> HostResult {
        let handler: Arc<dyn ToolHandler> = match self.tools.get(name) {
            Some(h) => h,
            None => {
                return HostResult::err(HostError::new(
                    "tool-not-found",
                    format!("tool '{name}' not registered"),
                ))
            }
        };
        if !self.gate.is_enabled(name) {
            return HostResult::err(HostError::new(
                "tool-disabled",
                format!("tool '{name}' is disabled (fail-closed)"),
            ));
        }
        let input = ToolExecutionInput {
            name: name.to_string(),
            arguments,
        };
        match handler.execute(input).await {
            Ok(r) => HostResult::ok(json!({ "output": r.output, "isError": r.is_error })),
            Err(e) => HostResult::err(HostError::new(
                "tool-error",
                format!("tool '{name}' failed: {}", e.0),
            )),
        }
    }
}

#[async_trait::async_trait]
impl HostApi for RealHost {
    fn log(&self, level: &str, msg: &str) {
        tracing::info!(level = %level, "js plugin: {msg}");
    }

    fn config_get(&self, plugin_id: &str, key: &str) -> HostResult {
        // 白名单查询：`{plugin_id}.{key}`；未命中 → config-not-found（对齐
        // MockHost 契约）。白名单即全部内容，secret 不在其中（见字段注释）。
        match self.config.get(&format!("{plugin_id}.{key}")) {
            Some(v) => HostResult::ok(Value::String(v.clone())),
            None => HostResult::err(HostError::new(
                "config-not-found",
                format!("{plugin_id}.{key}"),
            )),
        }
    }

    fn tools_list(&self) -> HostResult {
        let schemas = self.gate.enabled_schemas(&self.tools);
        let tools: Vec<quickjs_bridge::ToolSpec> = schemas
            .into_iter()
            .map(|s| quickjs_bridge::ToolSpec {
                name: s.name,
                description: s.description,
                parameters: s.parameters,
            })
            .collect();
        HostResult::ok(serde_json::to_value(&tools).unwrap_or(Value::Null))
    }

    async fn tools_invoke(&self, name: &str, arguments: Value) -> HostResult {
        self.run_tool(name, arguments).await
    }

    fn session_append(&self, session_id: &str, event: Value) -> HostResult {
        let session = match self.store.get(session_id) {
            Some(s) => s,
            None => {
                return HostResult::err(HostError::new(
                    "session-not-found",
                    format!("session {session_id} not found"),
                ))
            }
        };
        // 事件经 JSON 反序列化成内核 SessionEvent（拉模型；JS 侧不持锁不重入）。
        let event: SessionEvent = match serde_json::from_value(event) {
            Ok(e) => e,
            Err(e) => {
                return HostResult::err(HostError::new(
                    "bad-event",
                    format!("event not a SessionEvent: {e}"),
                ))
            }
        };
        let rec = session.append(event);
        HostResult::ok(json!({ "seq": rec.seq }))
    }

    fn session_get(&self, session_id: &str) -> HostResult {
        let Some(session) = self.store.get(session_id) else {
            return HostResult::err(HostError::new(
                "session-not-found",
                format!("session {session_id} not found"),
            ));
        };
        let events: Vec<Value> = session
            .events()
            .iter()
            .map(|r| serde_json::to_value(&r.event).unwrap_or(Value::Null))
            .collect();
        HostResult::ok(json!({ "events": events, "cursor": events.len() }))
    }

    fn session_poll(&self, session_id: &str, cursor: u64) -> HostResult {
        let Some(session) = self.store.get(session_id) else {
            return HostResult::err(HostError::new(
                "session-not-found",
                format!("session {session_id} not found"),
            ));
        };
        let events: Vec<Value> = session
            .events()
            .iter()
            .map(|r| serde_json::to_value(&r.event).unwrap_or(Value::Null))
            .collect();
        let start = cursor as usize;
        let delta = if start >= events.len() { vec![] } else { events[start..].to_vec() };
        HostResult::ok(json!({ "events": delta, "cursor": events.len() }))
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
    use kernel_contracts::llm::{ChunkStream, FinishReason, GenerateOptions, StreamChunk};
    use kernel_contracts::session::{SessionHeader, SessionId};
    use kernel_contracts::{LlmModelInfo, ToolExecutionResult};
    use quickjs_bridge::{Cancellation, ChatMessage, CompleteRequest};

    /// 脚本化 LLM：产一个 text-delta + stop。
    struct StubLlm;
    #[async_trait::async_trait]
    impl LlmPort for StubLlm {
        async fn list_models(
            &self,
            _provider: &str,
        ) -> Result<Vec<LlmModelInfo>, kernel_contracts::error::LlmError> {
            Ok(vec![])
        }
        fn stream(&self, _request: GenerateOptions) -> ChunkStream {
            Box::pin(futures::stream::iter(vec![
                Ok(StreamChunk::TextDelta {
                    index: 0,
                    text: "hello from real host".to_string(),
                }),
                Ok(StreamChunk::Finish(FinishReason::Stop)),
            ]))
        }
    }

    /// 脚本化工具：echo 参数。
    struct EchoTool;
    #[async_trait::async_trait]
    impl ToolHandler for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echo back"
        }
        fn parameters(&self) -> Value {
            json!({ "type": "object", "properties": { "text": { "type": "string" } } })
        }
        async fn execute(
            &self,
            input: ToolExecutionInput,
        ) -> Result<ToolExecutionResult, kernel_contracts::error::ToolError> {
            let text = input.arguments.get("text").and_then(Value::as_str).unwrap_or("");
            Ok(ToolExecutionResult::ok(format!("echo:{text}")))
        }
    }

    /// 永不启用的工具（验证 fail-closed）。
    struct DisabledTool;
    #[async_trait::async_trait]
    impl ToolHandler for DisabledTool {
        fn name(&self) -> &str {
            "disabled"
        }
        fn description(&self) -> &str {
            "never enabled"
        }
        fn parameters(&self) -> Value {
            json!({})
        }
        async fn execute(
            &self,
            _input: ToolExecutionInput,
        ) -> Result<ToolExecutionResult, kernel_contracts::error::ToolError> {
            Ok(ToolExecutionResult::ok("x"))
        }
    }

    fn header(id: &str) -> SessionHeader {
        SessionHeader {
            id: SessionId(id.to_string()),
            app: "test".into(),
            profile: "headless".into(),
            workspace: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn host() -> (RealHost, Arc<ToolRegistry>, Arc<ToolGate>) {
        let tools = Arc::new(ToolRegistry::new());
        tools.register(Arc::new(EchoTool)).unwrap();
        let gate = Arc::new(ToolGate::new());
        gate.enable("echo");
        let host = RealHost::new(
            Arc::new(StubLlm),
            Arc::clone(&tools),
            Arc::clone(&gate),
            Arc::new(SessionStore::new()),
            std::collections::HashMap::new(),
        );
        (host, tools, gate)
    }

    #[test]
    fn llm_face_wires_kernel_port() {
        let (host, _t, _g) = host();
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
        let value = match r {
            HostResult::Ok { value } => value,
            _ => panic!("expected ok, got {r:?}"),
        };
        assert_eq!(value["chunks"][0]["text"], json!("hello from real host"));
        assert_eq!(value["chunks"][1]["reason"], json!("stop"));
    }

    #[test]
    fn tools_face_lists_enabled_and_invokes() {
        let (host, tools, gate) = host();
        // 第二个工具注册但未启用。
        tools.register(Arc::new(DisabledTool)).unwrap();
        // 清单只含已启用工具（fail-closed：未启用不暴露）。
        let listed = host.tools_list().ok_value_();
        assert_eq!(listed.as_array().unwrap().len(), 1);
        assert_eq!(listed[0]["name"], json!("echo"));
        // 调用已启用工具。
        let r = block_on(host.tools_invoke("echo", json!({ "text": "hi" })));
        assert_eq!(r.ok_value_()["output"], json!("echo:hi"));
        assert_eq!(r.ok_value_()["isError"], json!(false));
        // 未注册工具 → tool-not-found。
        let r2 = block_on(host.tools_invoke("nope", json!({})));
        assert_eq!(r2.err_code_().as_deref(), Some("tool-not-found"));
        // 注册但未启用 → tool-disabled（fail-closed，与 agent-loop 同语义）。
        let r3 = block_on(host.tools_invoke("disabled", json!({})));
        assert_eq!(r3.err_code_().as_deref(), Some("tool-disabled"));
        // 启用后可用。
        gate.enable("disabled");
        let r4 = block_on(host.tools_invoke("disabled", json!({})));
        assert_eq!(r4.ok_value_()["output"], json!("x"));
    }

    #[test]
    fn session_faces_read_write_pull_model() {
        let store = Arc::new(SessionStore::new());
        let _ = store.create(header("s1"), kernel_contracts::bus::EventBus::new());
        let host = RealHost::new(
            Arc::new(StubLlm),
            Arc::new(ToolRegistry::new()),
            Arc::new(ToolGate::new()),
            Arc::clone(&store),
            std::collections::HashMap::new(),
        );
        // 追加一个事件 → get 投影（SessionEvent 外部 tag 形状；
        // 新建会话已含 SessionStarted，故 cursor 起点 = 2）。
        let r = host.session_append("s1", json!({ "UserMessage": { "text": "hi" } }));
        assert!(matches!(r, HostResult::Ok { .. }), "got {r:?}");
        let g = host.session_get("s1").ok_value_();
        assert_eq!(g["cursor"], json!(2));
        assert_eq!(g["events"][1]["UserMessage"]["text"], json!("hi"));
        // poll 拉模型：游标续读，无回调重入。
        let p = host.session_poll("s1", 2).ok_value_();
        assert_eq!(p["events"].as_array().unwrap().len(), 0);
        // 未知会话 → session-not-found。
        let r2 = host.session_get("nope");
        assert_eq!(r2.err_code_().as_deref(), Some("session-not-found"));
    }

    #[test]
    fn config_face_whitelist_only() {
        // config 白名单：命中返回；未命中 config-not-found（永不返回 secret——
        // secret 不进白名单表，这是注入方的纪律，host 层白名单即全部内容）。
        let mut cfg = std::collections::HashMap::new();
        cfg.insert("demo.url".to_string(), "https://x".to_string());
        let host = RealHost::new(
            Arc::new(StubLlm),
            Arc::new(ToolRegistry::new()),
            Arc::new(ToolGate::new()),
            Arc::new(SessionStore::new()),
            cfg,
        );
        let hit = host.config_get("demo", "url").ok_value_();
        assert_eq!(hit, json!("https://x"));
        let miss = host.config_get("demo", "secret");
        assert_eq!(miss.err_code_().as_deref(), Some("config-not-found"));
    }

    // 测试小助手：HostResult 解包。
    trait ResultExt {
        fn ok_value_(&self) -> Value;
        fn err_code_(&self) -> Option<String>;
    }
    impl ResultExt for HostResult {
        fn ok_value_(&self) -> Value {
            match self {
                HostResult::Ok { value } => value.clone(),
                _ => Value::Null,
            }
        }
        fn err_code_(&self) -> Option<String> {
            match self {
                HostResult::Err { err } => Some(err.code.clone()),
                _ => None,
            }
        }
    }
}
