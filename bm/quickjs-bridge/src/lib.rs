//! quickjs-bridge：QuickJS 宿主桥（host API 契约层 + rquickjs 内嵌引擎）。
//!
//! 本 crate 定义 JS 插件能调用的**宿主 API 面契约**（[`HostApi`] trait + JSON 出入参），
//! 并把实现注册进 rquickjs 全局 `host`（[`JsBridge`]），异步泵打通（见 `js` 模块）。
//! 引擎只在组合根装配，kernel 保持纯 Rust 内核库；桥只通过内核契约端口暴露宿主 API。
//!
//! 边界（grok 评审 + 实测定稿）：
//! - JS 只做编排胶水；重逻辑（字符串/正则/JSON/网络/文件）一律回调宿主 Rust API；
//! - 类型只跨 JSON + 显式 schema；失败模型 `{ok, err:{code,retryable}}` 与 ToolGate 同码；
//! - **不暴露 agent.step**：JS 插件当 Tool/Policy，不当第二 Agent（防双驱动毁掉 interrupted-turn）；
//! - `session.subscribe` 用**拉模型**（host 队列，禁止 JS 回调里再 session.append → 死锁）；
//! - rquickjs 不注入 fs/fetch（权限治理单点）；config.get 白名单键 + 永不返回 secret。

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------- 失败模型（与 ToolGate 同码，单一码表在 kernel-contracts） ----------

/// host 调用失败（JSON 序列化为 `{ok:false, err:{code, message, retryable}}`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostError {
    /// 码表来自 kernel_contracts（ToolError / LlmError 的 code；JS 只回传，不发明）。
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
}

impl HostError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), retryable: false }
    }
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }
}

/// host 统一响应信封：`{ok:true, value}` 或 `{ok:false, err}`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "ok")]
pub enum HostResult {
    #[serde(rename = "true")]
    Ok { value: Value },
    #[serde(rename = "false")]
    Err { err: HostError },
}

impl HostResult {
    pub fn ok(value: Value) -> Self {
        Self::Ok { value }
    }
    pub fn err(err: HostError) -> Self {
        Self::Err { err }
    }
}

// ---------- 出入参（JSON 形态，跨桥只走这些） ----------

/// host.llm.complete 的请求（序列化后过桥；供 JS 拼装）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteRequest {
    pub provider: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// 与 kernel-contracts 对齐的消息形状（跨桥最小子集）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// 工具声明（ToolHandler.parameters 的 JSON schema）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// host.llm.complete 的流式块（供 JS 逐块消费；与 kernel StreamChunk 同构子集）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CompletionChunk {
    TextDelta { text: String },
    ToolCallDelta { index: u32, name: Option<String>, arguments: Option<String> },
    Finish { reason: String, code: Option<String>, message: Option<String> },
}

// ---------- host API 面（JS 插件可见的全部能力） ----------

/// 宿主 API 面：组合根把此 trait 的实现注册进 QuickJS 全局 `host`。
/// 所有方法同步返回 [`HostResult`]（异步经内部泵；v1 契约先定同步面）。
#[async_trait::async_trait]
pub trait HostApi: Send + Sync {
    /// 日志（level: debug|info|warn|error）。
    fn log(&self, level: &str, msg: &str);

    /// 配置读取：按插件 id 命名空间白名单键。**永不返回 secret**（credentials 不在此）。
    fn config_get(&self, plugin_id: &str, key: &str) -> HostResult;

    /// 工具清单（含 schema）。
    fn tools_list(&self) -> HostResult;

    /// 工具调用（走 ToolRegistry + ToolGate；失败码与 ToolGate 同码）。
    async fn tools_invoke(&self, name: &str, arguments: Value) -> HostResult;

    /// LLM 补全（流式）：返回迭代块；取消经 `cancel` 的 Drop/信号传播到传输层。
    async fn llm_complete_stream(
        &self,
        request: CompleteRequest,
        cancel: Cancellation,
    ) -> HostResult {
        // §5.4 默认实现：经 `llm_port()` 走内核契约端口（与 agent-loop 同一
        // 聚合 LLM）。未装配真 LlmPort（如 MockHost 覆写前）→ `LLM_UNAVAILABLE`
        // 诚实失败，绝不假成功。组合根装配的真宿主只需覆写 `llm_port()`。
        match self.llm_port() {
            Ok(llm) => crate::host::complete_with_port(llm, request, cancel).await,
            Err(e) => HostResult::err(e),
        }
    }

    /// 会话追加事件（走 Session.append；JS 不得自管回合）。
    fn session_append(&self, session_id: &str, event: Value) -> HostResult;

    /// 会话只读投影（事件列表；拉模型，不订阅回调）。
    fn session_get(&self, session_id: &str) -> HostResult;

    /// 会话事件拉取：从游标起取增量（v1：一次性快照 + 游标续读；禁止回调重入）。
    fn session_poll(&self, session_id: &str, cursor: u64) -> HostResult;

    /// 内核 LLM 端口（§5.4 接真 LLM）：默认实现 = 不可用（`LLM_UNAVAILABLE`）；
    /// 组合根装配的真宿主把聚合 `LlmPort` 接进来，`llm_complete_stream` 经此
    /// 走与 agent-loop 同一契约端口。**不提供 `agent.step`**——JS 插件当
    /// Tool/Policy，不当第二 Agent（防双驱动毁掉 interrupted-turn）。
    fn llm_port(&self) -> Result<Arc<dyn kernel_contracts::llm::LlmPort>, HostError> {
        Err(HostError::new(
            "LLM_UNAVAILABLE",
            "host api has no llm port bound (assembly did not wire a real LlmPort)",
        ))
    }
}

/// 取消令牌：JS 任务 id → Rust CancellationToken/Drop。
/// v1 契约：实现持有 `tokio_util::sync::CancellationToken` 或类似，
/// JS 侧 abort 时触发，传输层（reqwest SSE）应停止拉取。
#[derive(Clone, Default)]
pub struct Cancellation {
    pub token: Arc<tokio::sync::Notify>,
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl Cancellation {
    pub fn new() -> Self {
        Self {
            token: Arc::new(tokio::sync::Notify::new()),
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.token.notify_waiters();
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }
}

// ---------- rquickjs 内嵌引擎（落地顺序 §5.2：全局 host + 异步泵） ----------

pub mod js;
pub mod plugin;
pub mod host;
pub mod registry;

pub use js::JsBridge;
pub use plugin::{JsPluginManifest, LoadedPlugin};
pub use registry::{load_plugin, scan_plugins, PluginDir};

// ---------- mock 实现（契约测试用，不接真实运行时） ----------

/// 契约测试用 mock：所有方法记录调用 + 返回可预测结果。
pub struct MockHost {
    pub log_lines: std::sync::Mutex<Vec<(String, String)>>,
    pub config: HashMap<String, String>,
    pub tools: Vec<ToolSpec>,
    pub sessions: std::sync::Mutex<HashMap<String, Vec<Value>>>,
}

impl MockHost {
    pub fn new() -> Self {
        Self {
            log_lines: std::sync::Mutex::new(vec![]),
            config: HashMap::new(),
            tools: vec![ToolSpec {
                name: "echo".into(),
                description: "echo back".into(),
                parameters: serde_json::json!({ "type": "object", "properties": { "text": { "type": "string" } } }),
            }],
            sessions: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MockHost {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HostApi for MockHost {
    fn log(&self, level: &str, msg: &str) {
        self.log_lines.lock().unwrap().push((level.to_string(), msg.to_string()));
    }

    fn config_get(&self, plugin_id: &str, key: &str) -> HostResult {
        match self.config.get(&format!("{plugin_id}.{key}")) {
            Some(v) => HostResult::ok(Value::String(v.clone())),
            None => HostResult::err(HostError::new("config-not-found", format!("{plugin_id}.{key}"))),
        }
    }

    fn tools_list(&self) -> HostResult {
        HostResult::ok(serde_json::to_value(&self.tools).unwrap())
    }

    async fn tools_invoke(&self, name: &str, _arguments: Value) -> HostResult {
        if name == "echo" {
            HostResult::ok(serde_json::json!({ "ok": true }))
        } else {
            HostResult::err(HostError::new("tool-not-found", name))
        }
    }

    async fn llm_complete_stream(
        &self,
        _request: CompleteRequest,
        cancel: Cancellation,
    ) -> HostResult {
        // mock：若已取消 → 取消 finish；否则产一个 text chunk + stop。
        if cancel.is_cancelled() {
            return HostResult::ok(serde_json::json!({
                "chunks": [ { "type": "finish", "reason": "cancelled" } ]
            }));
        }
        HostResult::ok(serde_json::json!({
            "chunks": [
                { "type": "text-delta", "text": "hello from mock" },
                { "type": "finish", "reason": "stop" }
            ]
        }))
    }

    fn session_append(&self, session_id: &str, event: Value) -> HostResult {
        self.sessions.lock().unwrap().entry(session_id.to_string()).or_default().push(event);
        HostResult::ok(Value::Null)
    }

    fn session_get(&self, session_id: &str) -> HostResult {
        let events = self.sessions.lock().unwrap().get(session_id).cloned().unwrap_or_default();
        HostResult::ok(serde_json::json!({ "events": events, "cursor": events.len() as u64 }))
    }

    fn session_poll(&self, session_id: &str, cursor: u64) -> HostResult {
        let events = self.sessions.lock().unwrap().get(session_id).cloned().unwrap_or_default();
        let start = cursor as usize;
        let delta = if start >= events.len() { vec![] } else { events[start..].to_vec() };
        HostResult::ok(serde_json::json!({
            "events": delta,
            "cursor": events.len() as u64,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{JsPluginManifest, LoadedPlugin};

    #[test]
    fn host_result_serializes_ok_and_err() {
        let ok = HostResult::ok(serde_json::json!({ "a": 1 }));
        assert_eq!(
            serde_json::to_string(&ok).unwrap(),
            r#"{"ok":"true","value":{"a":1}}"#
        );
        let err = HostResult::err(HostError::new("tool-not-found", "echo"));
        let v: Value = serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(v["ok"], "false");
        assert_eq!(v["err"]["code"], "tool-not-found");
        assert_eq!(v["err"]["retryable"], false);
    }

    #[test]
    fn config_get_namespaced_and_never_secret() {
        let h = MockHost::new();
        // 白名单键：只查 {plugin}.{key}，secret（credentials）不在 config 面。
        assert_eq!(
            h.config_get("my-plugin", "missing").err_or_code().as_deref(),
            Some("config-not-found")
        );
    }

    #[test]
    fn session_poll_is_pull_model_no_reentry() {
        let h = MockHost::new();
        assert!(h.session_append("s1", serde_json::json!({"type":"user/message"})).is_ok());
        let first = h.session_get("s1").ok_value();
        assert_eq!(first["events"].as_array().unwrap().len(), 1);
        // 拉模型：游标续读，无回调重入。
        let delta = h.session_poll("s1", 1).ok_value();
        assert_eq!(delta["events"].as_array().unwrap().len(), 0);
        h.session_append("s1", serde_json::json!({"type":"assistant/message"}));
        let delta2 = h.session_poll("s1", 1).ok_value();
        assert_eq!(delta2["events"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn llm_complete_respects_cancellation() {
        let h = MockHost::new();
        let cancel = Cancellation::new();
        let req = CompleteRequest {
            provider: "mock".into(),
            model: "mock-1".into(),
            messages: vec![ChatMessage { role: "user".into(), content: "hi".into() }],
            tools: None,
            temperature: None,
            max_tokens: None,
        };
        let r = futures::executor::block_on(h.llm_complete_stream(req.clone(), cancel.clone()));
        let chunks = r.ok_value()["chunks"].as_array().unwrap().clone();
        assert_eq!(chunks[0]["type"], "text-delta");
        // 取消后：返回取消 finish。
        cancel.cancel();
        let r2 = futures::executor::block_on(h.llm_complete_stream(req, cancel));
        let chunks2 = r2.ok_value()["chunks"].as_array().unwrap().clone();
        assert_eq!(chunks2[0]["type"], "finish");
        assert_eq!(chunks2[0]["reason"], "cancelled");
    }

    #[test]
    fn no_agent_step_in_host_api() {
        // 契约面禁止 agent.step：JS 当 Tool/Policy，不当第二 Agent。
        let host_api: &dyn HostApi = &MockHost::new();
        // 无法编译 `host.agent.step`——HostApi trait 没有该方法（此测试是编译期断言）。
        let _ = host_api.tools_list();
    }

    #[test]
    fn llm_port_default_is_unavailable() {
        // 未装配真 LlmPort 时 llm_complete_stream 必须诚实失败（不假成功）。
        // 组合根装配的真宿主覆写 llm_port() 后，此路径被真实 provider 取代。
        // 用只实现同步面的最小宿主（不覆写 llm_complete_stream → 走默认 llm_port）。
        struct NoLlmHost;
        #[async_trait::async_trait]
        impl HostApi for NoLlmHost {
            fn log(&self, _level: &str, _msg: &str) {}
            fn config_get(&self, _plugin_id: &str, _key: &str) -> HostResult {
                HostResult::ok(Value::Null)
            }
            fn tools_list(&self) -> HostResult {
                HostResult::ok(Value::Null)
            }
            async fn tools_invoke(&self, _name: &str, _arguments: Value) -> HostResult {
                HostResult::ok(Value::Null)
            }
            fn session_append(&self, _session_id: &str, _event: Value) -> HostResult {
                HostResult::ok(Value::Null)
            }
            fn session_get(&self, _session_id: &str) -> HostResult {
                HostResult::ok(Value::Null)
            }
            fn session_poll(&self, _session_id: &str, _cursor: u64) -> HostResult {
                HostResult::ok(Value::Null)
            }
        }
        let h = NoLlmHost;
        let r = futures::executor::block_on(h.llm_complete_stream(
            CompleteRequest {
                provider: "minimax".into(),
                model: "MiniMax-M3".into(),
                messages: vec![ChatMessage { role: "user".into(), content: "hi".into() }],
                tools: None,
                temperature: None,
                max_tokens: None,
            },
            Cancellation::new(),
        ));
        assert_eq!(r.err_or_code().as_deref(), Some("LLM_UNAVAILABLE"));
    }

    trait ResultExt {
        fn is_ok(&self) -> bool;
        fn ok_value(&self) -> Value;
        fn err_or_code(&self) -> Option<String>;
    }
    impl ResultExt for HostResult {
        fn is_ok(&self) -> bool {
            matches!(self, HostResult::Ok { .. })
        }
        fn ok_value(&self) -> Value {
            match self {
                HostResult::Ok { value } => value.clone(),
                HostResult::Err { .. } => Value::Null,
            }
        }
        fn err_or_code(&self) -> Option<String> {
            match self {
                HostResult::Err { err } => Some(err.code.clone()),
                _ => None,
            }
        }
    }

    // ---------- rquickjs 端到端：host 面注册 + 异步泵（落地顺序 §5.2） ----------

    fn new_bridge() -> (Arc<MockHost>, JsBridge) {
        let host = Arc::new(MockHost::new());
        let bridge = JsBridge::new(host.clone()).expect("JsBridge::new");
        (host, bridge)
    }

    /// 同步读全局变量值（JS 值 → JSON）。
    fn read_global(bridge: &JsBridge, name: &str) -> Value {
        bridge.eval_value(&format!("globalThis.{name}")).unwrap()
    }

    // ---------- manifest 驱动装载 + 最小权限授面（落地顺序 §5.3） ----------

    #[test]
    fn manifest_parse_and_validate() {
        let m = JsPluginManifest::from_json(
            r#"{"id":"p1","name":"P1","version":"1.0.0","entry":"main.js","host":["tools.list","llm.complete"]}"#,
        )
        .unwrap();
        assert_eq!(m.id, "p1");
        assert_eq!(m.entry, "main.js");
        assert_eq!(m.face_set().len(), 2);
        // host 缺省 = 最小（空集）。
        let m2 = JsPluginManifest::from_json(r#"{"id":"p2","name":"P2","entry":"main.js"}"#).unwrap();
        assert!(m2.face_set().is_empty());
        // 未知面名 → 拒绝（防拼错静默失效）。
        assert!(JsPluginManifest::from_json(
            r#"{"id":"p3","name":"P3","entry":"m.js","host":["fs.read"]}"#,
        )
        .is_err());
    }

    #[test]
    fn manifest_face_set_dedups() {
        let m = JsPluginManifest::from_json(
            r#"{"id":"p","name":"P","entry":"m.js","host":["log","log","tools.list"]}"#,
        )
        .unwrap();
        assert_eq!(m.face_set().len(), 2);
    }

    #[test]
    fn js_least_privilege_unlisted_face_is_undefined() {
        // 只授 log + tools.list：host.llm / host.session / host.tools.invoke 必须 undefined。
        let host = Arc::new(MockHost::new());
        let bridge =
            JsBridge::new_with_faces(host, &["log", "tools.list"]).expect("new_with_faces");
        bridge.exec("globalThis.__l = typeof host.log;").unwrap();
        assert_eq!(read_global(&bridge, "__l"), serde_json::json!("function"));
        bridge.exec("globalThis.__t = typeof host.tools.list;").unwrap();
        assert_eq!(read_global(&bridge, "__t"), serde_json::json!("function"));
        // 未授面：对象与方法都不存在。
        bridge.exec("globalThis.__i = typeof host.tools.invoke;").unwrap();
        assert_eq!(read_global(&bridge, "__i"), serde_json::json!("undefined"));
        bridge.exec("globalThis.__llm = typeof host.llm;").unwrap();
        assert_eq!(read_global(&bridge, "__llm"), serde_json::json!("undefined"));
        bridge.exec("globalThis.__s = typeof host.session;").unwrap();
        assert_eq!(read_global(&bridge, "__s"), serde_json::json!("undefined"));
        bridge.exec("globalThis.__c = typeof host.config;").unwrap();
        assert_eq!(read_global(&bridge, "__c"), serde_json::json!("undefined"));
    }

    #[test]
    fn js_invoke_requires_tools_invoke_face() {
        // 未授 tools.invoke 时调用 `host.tools.invoke` 必须抛（ReferenceError/TypeError）。
        let host = Arc::new(MockHost::new());
        let bridge = JsBridge::new_with_faces(host, &["log"]).expect("new_with_faces");
        let r = bridge.exec_async("await host.tools.invoke('echo', {});");
        assert!(r.is_err(), "invoke 应因面未授而失败: {r:?}");
    }

    #[test]
    fn loaded_plugin_reads_dir() {
        // 构造一个临时插件目录：plugin.json + main.js。
        let dir = std::env::temp_dir().join(format!("qjs-plugin-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"id":"demo","name":"Demo","version":"0.1.0","entry":"main.js","host":["log","tools.list"]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("main.js"), "host.log('info', 'loaded');").unwrap();
        let loaded = LoadedPlugin::load(&dir).unwrap();
        assert_eq!(loaded.manifest.id, "demo");
        assert!(loaded.entry_source.contains("host.log"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn js_global_host_registered() {
        let (_h, bridge) = new_bridge();
        bridge.exec(r#"globalThis.__t = typeof host;"#).unwrap();
        assert_eq!(read_global(&bridge, "__t"), serde_json::json!("object"));
    }

    #[test]
    fn js_host_log_and_config_get() {
        let (host, bridge) = new_bridge();
        bridge
            .exec(r#"host.log("info", "hello from js"); globalThis.__c = host.config.get("p1", "k1");"#)
            .unwrap();
        assert_eq!(host.log_lines.lock().unwrap()[0], ("info".into(), "hello from js".into()));
        // config.get 未命中 → err 信封。
        let c = read_global(&bridge, "__c");
        assert_eq!(c["ok"], serde_json::json!("false"));
        assert_eq!(c["err"]["code"], serde_json::json!("config-not-found"));
    }

    #[test]
    fn js_host_tools_list_sync() {
        let (_h, bridge) = new_bridge();
        bridge.exec(r#"globalThis.__t = host.tools.list();"#).unwrap();
        let t = read_global(&bridge, "__t");
        // {ok:'true', value:[{name:'echo',...}]}
        assert_eq!(t["ok"], serde_json::json!("true"));
        assert_eq!(t["value"][0]["name"], serde_json::json!("echo"));
    }

    #[test]
    fn js_host_tools_invoke_async_pump() {
        let (host, bridge) = new_bridge();
        // 异步工具调用：JS await host.tools.invoke → 泵线程驱动 → 宿主执行 → resolve。
        // call_async 内部 await 整个 async 函数，结果 JSON 读回。
        bridge
            .exec(r#"globalThis.__invoke = async (n, a) => host.tools.invoke(n, a);"#)
            .unwrap();
        let r = bridge
            .call_async("__invoke", &[serde_json::json!("echo"), serde_json::json!({ "text": "hi" })])
            .unwrap();
        assert_eq!(r["ok"], serde_json::json!("true"));
        // 未知工具 → err 信封。
        let r2 = bridge
            .call_async("__invoke", &[serde_json::json!("nope"), serde_json::json!({})])
            .unwrap();
        assert_eq!(r2["err"]["code"], serde_json::json!("tool-not-found"));
        let _ = host;
    }

    #[test]
    fn js_host_llm_complete_async_pump() {
        let (_h, bridge) = new_bridge();
        // LLM 补全异步：JS await → mock 产 chunks。
        bridge
            .exec(
                r#"globalThis.__llmCall = async (req) => host.llm.complete(req);"#,
            )
            .unwrap();
        let r = bridge
            .call_async(
                "__llmCall",
                &[serde_json::json!({
                    "provider": "mock", "model": "mock-1",
                    "messages": [{ "role": "user", "content": "hi" }],
                })],
            )
            .unwrap();
        assert_eq!(r["ok"], serde_json::json!("true"));
        assert_eq!(r["value"]["chunks"][0]["type"], serde_json::json!("text-delta"));
    }

    #[test]
    fn js_host_session_append_get_poll() {
        let (host, bridge) = new_bridge();
        bridge
            .exec(
                r#"
                host.session.append("s1", { type: "user/message", text: "hi" });
                globalThis.__g = host.session.get("s1");
                globalThis.__p = host.session.poll("s1", 1);
                "#,
            )
            .unwrap();
        let g = read_global(&bridge, "__g");
        assert_eq!(g["value"]["events"].as_array().unwrap().len(), 1);
        let p = read_global(&bridge, "__p");
        assert_eq!(p["value"]["events"].as_array().unwrap().len(), 0);
        let _ = host;
    }

    // ---------- §5.4 接真 LLM：默认 llm_complete_stream 经 llm_port() 走内核端口 ----------

    /// 只实现 llm_port 的宿主（其余方法空实现）：验证 §5.4 默认路径把真 LlmPort
    /// 接进 JS `host.llm.complete`——JS → 泵线程 → 默认 llm_complete_stream →
    /// llm_port() → 内核块流 → 块 JSON 返回 JS。这就是组合根装配后的完整链路
    /// （组合根只需覆写 llm_port 返回聚合 LLM）。
    struct LlmOnlyHost {
        llm: Arc<dyn kernel_contracts::llm::LlmPort>,
    }

    #[async_trait::async_trait]
    impl HostApi for LlmOnlyHost {
        fn log(&self, _level: &str, _msg: &str) {}
        fn config_get(&self, _plugin_id: &str, _key: &str) -> HostResult {
            HostResult::ok(Value::Null)
        }
        fn tools_list(&self) -> HostResult {
            HostResult::ok(Value::Null)
        }
        async fn tools_invoke(&self, _name: &str, _arguments: Value) -> HostResult {
            HostResult::ok(Value::Null)
        }
        fn session_append(&self, _session_id: &str, _event: Value) -> HostResult {
            HostResult::ok(Value::Null)
        }
        fn session_get(&self, _session_id: &str) -> HostResult {
            HostResult::ok(Value::Null)
        }
        fn session_poll(&self, _session_id: &str, _cursor: u64) -> HostResult {
            HostResult::ok(Value::Null)
        }
        fn llm_port(&self) -> Result<Arc<dyn kernel_contracts::llm::LlmPort>, HostError> {
            Ok(Arc::clone(&self.llm))
        }
    }

    /// 脚本化 LlmPort（断言请求透传 + 产固定块流）。
    struct ScriptLlm {
        seen: Arc<std::sync::Mutex<Option<serde_json::Value>>>,
    }

    #[async_trait::async_trait]
    impl kernel_contracts::llm::LlmPort for ScriptLlm {
        async fn list_models(
            &self,
            _provider: &str,
        ) -> Result<Vec<kernel_contracts::llm::LlmModelInfo>, kernel_contracts::error::LlmError> {
            Ok(vec![])
        }
        fn stream(
            &self,
            request: kernel_contracts::llm::GenerateOptions,
        ) -> kernel_contracts::llm::ChunkStream {
            *self.seen.lock().unwrap() = Some(serde_json::to_value(&request).unwrap());
            Box::pin(futures::stream::iter(vec![
                Ok(kernel_contracts::llm::StreamChunk::TextDelta {
                    index: 0,
                    text: "real provider".to_string(),
                }),
                Ok(kernel_contracts::llm::StreamChunk::Finish(
                    kernel_contracts::llm::FinishReason::Stop,
                )),
            ]))
        }
    }

    #[test]
    fn js_llm_complete_wires_kernel_port_default_path() {
        let seen = Arc::new(std::sync::Mutex::new(None));
        let host = Arc::new(LlmOnlyHost {
            llm: Arc::new(ScriptLlm { seen: Arc::clone(&seen) }),
        });
        let bridge = JsBridge::new_with_faces(host, &["llm.complete"]).expect("bridge");
        bridge
            .exec(r#"globalThis.__call = async (req) => host.llm.complete(req);"#)
            .unwrap();
        let r = bridge
            .call_async(
                "__call",
                &[serde_json::json!({
                    "provider": "minimax", "model": "MiniMax-M3",
                    "messages": [
                        { "role": "system", "content": "sys" },
                        { "role": "user", "content": "hi" },
                    ],
                    "tools": [{ "name": "echo", "description": "echo back", "parameters": {} }],
                })],
            )
            .unwrap();
        // 真端口链路：块流翻译回 JS。
        assert_eq!(r["ok"], serde_json::json!("true"));
        assert_eq!(
            r["value"]["chunks"][0]["type"],
            serde_json::json!("text-delta")
        );
        assert_eq!(
            r["value"]["chunks"][0]["text"],
            serde_json::json!("real provider")
        );
        assert_eq!(
            r["value"]["chunks"][1]["reason"],
            serde_json::json!("stop")
        );
        // 请求透传：provider/model/messages 已翻译成内核 GenerateOptions（text-only）。
        let seen = seen.lock().unwrap();
        let req = seen.as_ref().expect("stream was called");
        assert_eq!(req["provider"], serde_json::json!("minimax"));
        assert_eq!(req["model"], serde_json::json!("MiniMax-M3"));
        assert_eq!(req["messages"][0]["role"], serde_json::json!("System"));
        assert_eq!(req["messages"][1]["content"][0]["Text"], serde_json::json!("hi"));
        assert_eq!(req["tools"][0]["name"], serde_json::json!("echo"));
        assert_eq!(req["sessionId"], serde_json::Value::Null);
    }

    #[test]
    fn js_llm_complete_unavailable_without_port() {
        // 宿主未覆写 llm_port → 默认 LLM_UNAVAILABLE 诚实失败（不假成功）。
        #[async_trait::async_trait]
        impl HostApi for NoPortHost {
            fn log(&self, _level: &str, _msg: &str) {}
            fn config_get(&self, _plugin_id: &str, _key: &str) -> HostResult {
                HostResult::ok(Value::Null)
            }
            fn tools_list(&self) -> HostResult {
                HostResult::ok(Value::Null)
            }
            async fn tools_invoke(&self, _name: &str, _arguments: Value) -> HostResult {
                HostResult::ok(Value::Null)
            }
            fn session_append(&self, _session_id: &str, _event: Value) -> HostResult {
                HostResult::ok(Value::Null)
            }
            fn session_get(&self, _session_id: &str) -> HostResult {
                HostResult::ok(Value::Null)
            }
            fn session_poll(&self, _session_id: &str, _cursor: u64) -> HostResult {
                HostResult::ok(Value::Null)
            }
        }
        struct NoPortHost;
        let bridge =
            JsBridge::new_with_faces(Arc::new(NoPortHost), &["llm.complete"]).expect("bridge");
        bridge
            .exec(r#"globalThis.__call = async (req) => host.llm.complete(req);"#)
            .unwrap();
        let r = bridge
            .call_async(
                "__call",
                &[serde_json::json!({
                    "provider": "minimax", "model": "MiniMax-M3",
                    "messages": [{ "role": "user", "content": "hi" }],
                })],
            )
            .unwrap();
        assert_eq!(r["ok"], serde_json::json!("false"));
        assert_eq!(r["err"]["code"], serde_json::json!("LLM_UNAVAILABLE"));
    }
}
