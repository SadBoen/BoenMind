//! B4 工具方向——CompatEngine：bm-compat QuickJS 引擎的 bm-server 侧宿主。
//!
//! `HostThread` 内部持 `Rc<PiJsRuntime>`（非 Send）——与 legacy 同款模型：
//! runtime 独占专用线程，外界经命令通道通信（legacy `JsRuntimeCommand` 的
//! 单 runtime 简化版）。命令在通道内天然串行：加载/执行/读回互不交错。
//!
//! B4 范围：工具执行方向（`__pi_execute_tool` 桥）。B5 权限桥：宿主服务
//! 端口换 [`BridgeServices`]——`request_approval` 接 PermissionBridge 同款
//! 询问链（SSE 弹窗 → oneshot 等决策 → 超时 fail-closed），`http` 端口
//! reqwest 真实现（web_search 全链路的网络能力）。其余端口（exec/session/
//! ui/events/内置工具集）留 B6 前补齐，B5 仍返 "unwired"。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bm_compat::error::Result as CompatResult;
use bm_compat::execute::execute_tool;
use bm_compat::extensions::ExtensionPolicy;
use bm_compat::extensions_js::ExtensionToolDef;
use bm_compat::host::{HostServices, HostThread};
use bm_compat::load::{JsExtensionLoadSpec, load_extension};
use bm_compat::scheduler::HostcallOutcome;
use bm_core::agent::AgentStreamEvent;
use bm_loop::engine::{ToolCallRequest, ToolExecutor, ToolOutcome};
use tokio::sync::{Mutex as TokioMutex, mpsc, oneshot};

use crate::PermissionDecision;

/// 插件工具执行超时（对齐 legacy 默认：agent.rs 的 JS_EXTENSION_TOOL_TIMEOUT_MS）。
pub const TOOL_TIMEOUT_MS: u64 = 60_000;

/// 宿主 hostcall（pi.http）默认超时。
const HOSTCALL_TIMEOUT: Duration = Duration::from_secs(60);

/// 权限询问等待上限：用户无响应时按拒绝处理（fail-closed，对齐 pi 路径）。
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(60);

/// 命令通道（专用线程内执行，oneshot 回结果）。
enum CompatCmd {
    Load {
        spec: JsExtensionLoadSpec,
        reply: oneshot::Sender<CompatResult<serde_json::Value>>,
    },
    Execute {
        name: String,
        call_id: String,
        input: serde_json::Value,
        timeout_ms: u64,
        /// 发起工具调用的会话（权限询问路由到该会话的 SSE 通道）
        session_id: String,
        reply: oneshot::Sender<CompatResult<serde_json::Value>>,
    },
    Tools {
        reply: oneshot::Sender<CompatResult<Vec<ExtensionToolDef>>>,
    },
}

/// B5 权限桥的宿主服务实现。
///
/// `current_session` 是专用线程内的"执行期上下文"：命令循环串行，execute
/// 期间 hostcall 同步发生——set/clear 即 thread-local 语义，把询问路由到
/// 发起工具调用的会话。加载期（Load 命令）无会话 → fail-closed。
pub struct BridgeServices {
    pub session_streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>>,
    pub permission_pending:
        Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
    pub current_session: Mutex<Option<String>>,
}

impl BridgeServices {
    fn set_session(&self, session_id: Option<String>) {
        *self.current_session.lock().unwrap() = session_id;
    }

    /// 与 pi 路径 PermissionBridge 同款的询问链：
    /// 注册 pending → SSE 推 PermissionRequest → 等决策 → 超时 fail-closed。
    async fn request_permission(&self, capability: &str, extension_id: Option<&str>) -> bool {
        let Some(session_id) = self.current_session.lock().unwrap().clone() else {
            // 无会话上下文（加载期 hostcall）→ fail-closed
            return false;
        };
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel::<PermissionDecision>();
        self.permission_pending
            .lock()
            .await
            .insert(request_id.clone(), tx);

        let extension_id = extension_id.unwrap_or("unknown");
        let message = format!("插件请求能力：{capability}");
        crate::chat::send_permission_request(
            &self.session_streams,
            &session_id,
            &request_id,
            extension_id,
            capability,
            &message,
        )
        .await;

        let decision = tokio::time::timeout(PERMISSION_TIMEOUT, rx)
            .await
            .ok()
            .and_then(|r| r.ok());
        self.permission_pending.lock().await.remove(&request_id);

        // 决策记忆：pi 路径由上游写 extension-permissions.json；bm 路径 B5
        // 先不持久化（每次询问），记忆化是 B6 前的补丁
        decision.is_some_and(|d| d.allow)
    }
}

#[async_trait::async_trait]
impl HostServices for BridgeServices {
    async fn execute_tool(&self, call_id: &str, name: &str, _input: serde_json::Value) -> HostcallOutcome {
        let _ = call_id;
        HostcallOutcome::Error {
            code: "unwired".to_string(),
            message: format!("宿主工具 {name} 未接线（内置工具集 B6 前补齐）"),
        }
    }

    async fn exec(&self, call_id: &str, cmd: &str, _payload: serde_json::Value) -> HostcallOutcome {
        let _ = (call_id, cmd);
        HostcallOutcome::Error {
            code: "unwired".to_string(),
            message: "exec 未接线（内置工具集 B6 前补齐）".to_string(),
        }
    }

    /// `pi.http(request)`——reqwest 真实现（B5）：镜像 legacy HttpConnector
    /// 的简化形态：method 除 GET 外一律 POST、响应 {status, headers, body}。
    async fn http(&self, call_id: &str, payload: serde_json::Value) -> HostcallOutcome {
        let Some(url) = payload.get("url").and_then(serde_json::Value::as_str) else {
            return HostcallOutcome::Error {
                code: "invalid_request".to_string(),
                message: "http hostcall: url is required".to_string(),
            };
        };
        let is_get = payload
            .get("method")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|m| m.eq_ignore_ascii_case("GET"));
        let client = reqwest::Client::new();
        let mut builder = if is_get {
            client.get(url)
        } else {
            client.post(url)
        };
        if let Some(headers) = payload.get("headers").and_then(serde_json::Value::as_object) {
            for (key, value) in headers {
                if let Some(v) = value.as_str() {
                    builder = builder.header(key, v);
                }
            }
        }
        if !is_get
            && let Some(body) = payload.get("body") {
                builder = builder.body(
                    body.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| body.to_string()),
                );
            }
        let response = match builder.timeout(HOSTCALL_TIMEOUT).send().await {
            Ok(r) => r,
            Err(err) => {
                return HostcallOutcome::Error {
                    code: "io".to_string(),
                    message: format!("http hostcall failed: {err}"),
                };
            }
        };
        let status = response.status().as_u16();
        let headers: serde_json::Map<String, serde_json::Value> = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    serde_json::Value::String(v.to_str().unwrap_or_default().to_string()),
                )
            })
            .collect();
        let body_bytes = match response.bytes().await {
            Ok(b) => b,
            Err(err) => {
                return HostcallOutcome::Error {
                    code: "io".to_string(),
                    message: format!("http hostcall body read failed: {err}"),
                };
            }
        };
        let mut output = serde_json::Map::new();
        output.insert("status".to_string(), serde_json::json!(status));
        output.insert("headers".to_string(), serde_json::Value::Object(headers));
        match String::from_utf8(body_bytes.to_vec()) {
            Ok(text) => {
                output.insert("body".to_string(), serde_json::Value::String(text));
            }
            Err(_) => {
                use base64::Engine as _;
                output.insert(
                    "body_bytes".to_string(),
                    serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(&body_bytes)),
                );
            }
        }
        let _ = call_id;
        HostcallOutcome::Success(serde_json::Value::Object(output))
    }

    async fn session(&self, call_id: &str, op: &str, _payload: serde_json::Value) -> HostcallOutcome {
        let _ = (call_id, op);
        HostcallOutcome::Error {
            code: "unwired".to_string(),
            message: "会话 hostcall 未接线（B6 前补齐）".to_string(),
        }
    }

    async fn ui(
        &self,
        call_id: &str,
        op: &str,
        _payload: serde_json::Value,
        _extension_id: Option<&str>,
    ) -> HostcallOutcome {
        let _ = (call_id, op);
        HostcallOutcome::Error {
            code: "unwired".to_string(),
            message: "UI hostcall 未接线（B6 前补齐）".to_string(),
        }
    }

    async fn events(
        &self,
        call_id: &str,
        op: &str,
        _payload: serde_json::Value,
        _extension_id: Option<&str>,
    ) -> HostcallOutcome {
        let _ = (call_id, op);
        HostcallOutcome::Error {
            code: "unwired".to_string(),
            message: "事件 hostcall 未接线（B6 前补齐）".to_string(),
        }
    }

    /// 权限询问：`prompt` 裁决走询问链（PermissionBridge 同款）；fail-closed。
    async fn request_approval(&self, capability: &str, extension_id: Option<&str>) -> bool {
        self.request_permission(capability, extension_id).await
    }
}

/// QuickJS 引擎宿主：专用线程 + 命令通道 + 启动加载后的工具快照。
pub struct CompatEngine {
    tx: mpsc::UnboundedSender<CompatCmd>,
    join: Option<std::thread::JoinHandle<()>>,
    /// 启动加载后的工具快照（bm-loop ToolDef 形态；B4 无运行时安装，快照即全集）
    pub tools: Vec<bm_loop::model::ToolDef>,
}

impl CompatEngine {
    /// 起专用线程 + 引导 runtime。返回引擎句柄（工具快照由
    /// [`crate::compat_engine::init_compat`] 在加载完成后填入）。
    /// `session_streams`/`permission_pending` 是 AppState 的同名组件
    /// （本引擎建于 AppState 之前，只拿组件不拿整态）。
    pub async fn spawn(
        session_streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>>,
        permission_pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
    ) -> Result<Self, String> {
        let (tx, mut rx) = mpsc::unbounded_channel::<CompatCmd>();
        let (boot_tx, boot_rx) = oneshot::channel::<Result<(), String>>();
        let services = Arc::new(BridgeServices {
            session_streams,
            permission_pending,
            current_session: Mutex::new(None),
        });
        let handle = std::thread::Builder::new()
            .name("bm-compat".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(rt) => rt,
                    Err(err) => {
                        let _ = boot_tx.send(Err(format!("tokio runtime: {err}")));
                        return;
                    }
                };
                rt.block_on(async move {
                    let runtime = match bm_compat::extensions_js::PiJsRuntime::new().await {
                        Ok(r) => r,
                        Err(err) => {
                            let _ = boot_tx.send(Err(format!("QuickJS runtime: {err}")));
                            return;
                        }
                    };
                    let thread = HostThread::new(
                        std::rc::Rc::new(runtime),
                        services.clone(),
                        ExtensionPolicy::default(),
                    );
                    let _ = boot_tx.send(Ok(()));
                    while let Some(cmd) = rx.recv().await {
                        match cmd {
                            CompatCmd::Load { spec, reply } => {
                                let res = load_extension(&thread, &spec).await;
                                let _ = reply.send(res);
                            }
                            CompatCmd::Execute { name, call_id, input, timeout_ms, session_id, reply } => {
                                // 执行期会话上下文：命令循环串行，hostcall 的
                                // 权限询问经 current_session 路由到发起会话
                                services.set_session(Some(session_id));
                                let res = execute_tool(
                                    &thread,
                                    &name,
                                    &call_id,
                                    input,
                                    serde_json::json!({}),
                                    Duration::from_millis(timeout_ms),
                                )
                                .await;
                                services.set_session(None);
                                let _ = reply.send(res);
                            }
                            CompatCmd::Tools { reply } => {
                                let res = thread.runtime().get_registered_tools().await;
                                let _ = reply.send(res);
                            }
                        }
                    }
                });
            })
            .map_err(|err| format!("spawn 失败: {err}"))?;
        // 等引导完成（runtime boot 失败时错误在此浮出，不静默）
        boot_rx.await.map_err(|_| "boot 通道关闭".to_string())??;
        Ok(Self {
            tx,
            join: Some(handle),
            tools: Vec::new(),
        })
    }

    /// 加载一个插件入口（B4 只做启动期加载；运行时安装是后续切片）。
    pub async fn load(&self, spec: &JsExtensionLoadSpec) -> Result<serde_json::Value, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(CompatCmd::Load { spec: spec.clone(), reply })
            .map_err(|_| "compat 引擎已停止".to_string())?;
        rx.await
            .map_err(|_| "compat 引擎已停止".to_string())?
            .map_err(|err| err.to_string())
    }

    /// 执行一个插件工具（`__pi_execute_tool` 桥）。`session_id` 用于把
    /// 执行期间的权限询问路由到发起会话的 SSE 通道。
    pub async fn execute(
        &self,
        name: &str,
        call_id: &str,
        input: serde_json::Value,
        timeout_ms: u64,
        session_id: &str,
    ) -> Result<serde_json::Value, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(CompatCmd::Execute {
                name: name.to_string(),
                call_id: call_id.to_string(),
                input,
                timeout_ms,
                session_id: session_id.to_string(),
                reply,
            })
            .map_err(|_| "compat 引擎已停止".to_string())?;
        rx.await
            .map_err(|_| "compat 引擎已停止".to_string())?
            .map_err(|err| err.to_string())
    }

    /// 读回已注册工具（ExtensionToolDef 形态）。
    pub async fn read_tools(&self) -> Result<Vec<ExtensionToolDef>, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(CompatCmd::Tools { reply })
            .map_err(|_| "compat 引擎已停止".to_string())?;
        rx.await
            .map_err(|_| "compat 引擎已停止".to_string())?
            .map_err(|err| err.to_string())
    }
}

impl Drop for CompatEngine {
    fn drop(&mut self) {
        // 最后一个 sender（self.tx）落下 → 专用线程 rx.recv() 返 None →
        // 命令循环退出 → join（runtime 随线程收尾）
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

/// ExtensionToolDef → bm-loop ToolDef（ToolRegistry 汇合点，B4）。
fn to_loop_tool(def: &ExtensionToolDef) -> bm_loop::model::ToolDef {
    bm_loop::model::ToolDef::new(
        def.name.clone(),
        def.description.clone(),
        def.parameters.clone(),
    )
}

/// 启动 CompatEngine：引导 → 加载启用插件 → 读回工具快照。
/// 失败不阻断服务（bm 引擎退化为无工具模式，QuickJsToolExecutor 兜底报错）。
pub async fn init_compat(
    config: &bm_core::AppConfig,
    session_streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>>,
    permission_pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
) -> Option<Arc<CompatEngine>> {
    let mut engine = match CompatEngine::spawn(session_streams, permission_pending).await {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!(event = "bm.compat_boot_failed", error = %err, "bm 引擎插件工具不可用");
            return None;
        }
    };

    let paths = bm_core::plugins::enabled_extension_paths(config);
    let mut loaded = 0usize;
    for path in &paths {
        let spec = match JsExtensionLoadSpec::from_entry_path(path) {
            Ok(spec) => spec,
            Err(err) => {
                tracing::warn!(event = "bm.compat_load_spec_failed", path = %path.display(), error = %err);
                continue;
            }
        };
        match engine.load(&spec).await {
            Ok(_) => {
                loaded += 1;
                tracing::info!(event = "bm.compat_plugin_loaded", id = %spec.extension_id);
            }
            Err(err) => {
                tracing::warn!(event = "bm.compat_load_failed", id = %spec.extension_id, error = %err);
            }
        }
    }

    match engine.read_tools().await {
        Ok(tools) => {
            engine.tools = tools.iter().map(to_loop_tool).collect();
            tracing::info!(
                event = "bm.compat_ready",
                plugins = loaded,
                tools = engine.tools.len(),
            );
        }
        Err(err) => {
            tracing::warn!(event = "bm.compat_tools_failed", error = %err);
        }
    }
    Some(Arc::new(engine))
}

/// bm-loop `ToolExecutor` 的 QuickJS 实现（B4/B5）：`execute` →
/// `__pi_execute_tool` 桥。compat 未启用（None）时兜底报错。
/// 每会话一个实例（携带 session_id，权限询问路由用）。
pub struct QuickJsToolExecutor {
    engine: Option<Arc<CompatEngine>>,
    session_id: String,
}

impl QuickJsToolExecutor {
    pub fn new(engine: Option<Arc<CompatEngine>>, session_id: impl Into<String>) -> Self {
        Self {
            engine,
            session_id: session_id.into(),
        }
    }
}

impl ToolExecutor for QuickJsToolExecutor {
    async fn execute(&self, req: ToolCallRequest) -> ToolOutcome {
        let Some(engine) = &self.engine else {
            return ToolOutcome {
                ok: false,
                output: "插件引擎未启用（bm-compat 启动失败或已禁用）".to_string(),
                meta: None,
            };
        };
        match engine
            .execute(&req.name, &req.call_id, req.args, TOOL_TIMEOUT_MS, &self.session_id)
            .await
        {
            Ok(value) => ToolOutcome {
                ok: true,
                output: tool_result_text(&value),
                meta: Some(value),
            },
            Err(err) => ToolOutcome {
                ok: false,
                output: err,
                meta: None,
            },
        }
    }
}

/// 插件返回值 → 输出文本：`content[].text` 拼接优先；无文本退回整包 JSON
/// （镜像 pi 路径 chat.rs `tool_output_text` 语义——审计之家不静默截断）。
fn tool_result_text(value: &serde_json::Value) -> String {
    let text: Vec<&str> = value
        .get("content")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| block.get("type").and_then(serde_json::Value::as_str).map(|t| (t, block)))
        .filter(|(t, _)| *t == "text")
        .filter_map(|(_, block)| block.get("text").and_then(serde_json::Value::as_str))
        .collect();
    if !text.is_empty() {
        return text.join("\n");
    }
    serde_json::to_string(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge() -> BridgeServices {
        BridgeServices {
            session_streams: Arc::new(TokioMutex::new(HashMap::new())),
            permission_pending: Arc::new(TokioMutex::new(HashMap::new())),
            current_session: Mutex::new(None),
        }
    }

    #[tokio::test]
    async fn approval_fails_closed_without_session() {
        let services = bridge();
        // 无执行期会话上下文（加载期/异常路径）→ 拒绝且不挂起
        let allowed = services.request_approval("http", Some("web-search")).await;
        assert!(!allowed);
        assert!(services.permission_pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn http_requires_url() {
        let services = bridge();
        match services.http("c1", serde_json::json!({})).await {
            HostcallOutcome::Error { code, .. } => assert_eq!(code, "invalid_request"),
            other => panic!("应返回 invalid_request，得到 {other:?}"),
        }
    }

    #[test]
    fn result_text_joins_content_blocks() {
        let v = serde_json::json!({
            "content": [
                { "type": "text", "text": "Hello, Boen!" },
                { "type": "text", "text": "第二段" }
            ],
            "details": { "greeted": "Boen" }
        });
        assert_eq!(tool_result_text(&v), "Hello, Boen!\n第二段");
    }

    #[test]
    fn result_text_falls_back_to_json() {
        // 无 text 块（如纯图片输出）→ 整包 JSON 保真
        let v = serde_json::json!({ "content": [{ "type": "image", "data": "AAAA" }] });
        let text = tool_result_text(&v);
        assert!(text.contains("image"), "JSON 兜底应保留原始输出: {text}");
    }

    #[test]
    fn tool_def_mapping_keeps_schema() {
        let def = ExtensionToolDef {
            name: "hello".into(),
            label: Some("Hello".into()),
            description: "greet".into(),
            parameters: serde_json::json!({ "type": "object" }),
        };
        let td = to_loop_tool(&def);
        assert_eq!(td.name, "hello");
        assert_eq!(td.description, "greet");
        assert_eq!(td.input_schema, serde_json::json!({ "type": "object" }));
    }
}
