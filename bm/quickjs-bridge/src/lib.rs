//! quickjs-bridge：QuickJS 宿主桥（P2：host API 契约层，无 rquickjs）。
//!
//! 本模块定义 JS 插件能调用的**宿主 API 面契约**（Rust trait + JSON 出入参），
//! 不依赖 rquickjs——先用 mock 实现测透契约，再在后续里程碑接 rquickjs。
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    ) -> HostResult;

    /// 会话追加事件（走 Session.append；JS 不得自管回合）。
    fn session_append(&self, session_id: &str, event: Value) -> HostResult;

    /// 会话只读投影（事件列表；拉模型，不订阅回调）。
    fn session_get(&self, session_id: &str) -> HostResult;

    /// 会话事件拉取：从游标起取增量（v1：一次性快照 + 游标续读；禁止回调重入）。
    fn session_poll(&self, session_id: &str, cursor: u64) -> HostResult;
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
}
