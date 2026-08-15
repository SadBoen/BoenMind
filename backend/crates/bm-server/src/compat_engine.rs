//! B4/B5/B6 — CompatEngine：bm-compat QuickJS 引擎的 bm-server 侧宿主。
//!
//! `HostThread` 内部持 `Rc<PiJsRuntime>`（非 Send）——与 legacy 同款模型：
//! runtime 独占专用线程，外界经命令通道通信（legacy `JsRuntimeCommand` 的
//! 单 runtime 简化版）。命令在通道内天然串行：加载/执行/读回互不交错。
//!
//! 范围演进：
//! - B4：工具执行方向（`__pi_execute_tool` 桥）。
//! - B5：权限桥——`request_approval` 接 PermissionBridge 同款询问链
//!   （SSE 弹窗 → oneshot 等决策 → 超时 fail-closed），`http` 端口 reqwest
//!   真实现。
//! - B6：宿主端口补齐——`execute_tool`（内置工具集 read/write/edit/grep/
//!   find/ls/bash，递归防护 = 只查内置表不查插件注册表）、`exec`（镜像
//!   legacy 非流式 `{stdout,stderr,code,killed}`）、`session`（会话 DB 的
//!   最小诚实子集）、`ui`（无扩展 UI 通道 → confirm 返回 false/custom
//!   closed，其余 not_configured）、`events`（active tools 记忆）；决策记忆
//!   （extension-permissions.json 持久化，询问前命中直返、always 决策回写）；
//!   宿主→插件事件推送（`__pi_dispatch_extension_event` 桥，startup/
//!   tool_call/tool_result 的宿主侧通道）。

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
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
use bm_kernel::EventLog;
use bm_loop::engine::{ToolCallRequest, ToolExecutor, ToolOutcome};
use tokio::sync::{Mutex as TokioMutex, mpsc, oneshot};

use crate::builtin_tools::BuiltinTools;
use crate::permission_store::PermissionStore;
use crate::PermissionDecision;

/// 插件工具执行超时（对齐 legacy 默认：agent.rs 的 JS_EXTENSION_TOOL_TIMEOUT_MS）。
pub const TOOL_TIMEOUT_MS: u64 = 60_000;

/// 宿主 hostcall（pi.http）默认超时。
const HOSTCALL_TIMEOUT: Duration = Duration::from_secs(60);

/// 权限询问等待上限：用户无响应时按拒绝处理（fail-closed，对齐 pi 路径）。
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(60);

/// exec hostcall 默认超时（legacy 非流式 exec 无默认上限；bm 路径保守 60s）。
const EXEC_TIMEOUT_MS: u64 = 60_000;

/// 事件分发（__pi_dispatch_extension_event）超时：handler 链跑完或放弃。
const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// B6：宿主→插件事件（pi.on handler 链），返回 handler 链最后非
    /// undefined 值（无 handler → Null）。
    DispatchEvent {
        name: String,
        payload: serde_json::Value,
        ctx: serde_json::Value,
        reply: oneshot::Sender<CompatResult<serde_json::Value>>,
    },
}

/// B5/B6 权限桥的宿主服务实现。
///
/// `current_session` 是专用线程内的"执行期上下文"：命令循环串行，execute
/// 期间 hostcall 同步发生——set/clear 即 thread-local 语义，把询问路由到
/// 发起工具调用的会话。加载期（Load 命令）无会话 → fail-closed。
pub struct BridgeServices {
    pub session_streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>>,
    pub permission_pending:
        Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
    pub current_session: Mutex<Option<String>>,
    /// B6：内置工具集（cwd = 工作文件夹）。
    pub builtin: BuiltinTools,
    /// B6：决策记忆（extension-permissions.json）。std Mutex：record 含
    /// 文件 IO，专用线程内临界区短、无 await 跨锁。
    pub permission_store: std::sync::Mutex<PermissionStore>,
    /// B6：会话端口的数据面（get_state/get_messages/set_name…）。
    pub db: Arc<bm_core::Db>,
    /// 事件日志（投影面数据源：getmessagesurface = 模型可见历史，含压缩
    /// 遮蔽；None = 双写未启用时该 op 降级到 messages 表）。
    pub event_log: Option<bm_kernel::EventLog>,
    /// B6：events 端口的 active tools 记忆（None = 全量）。
    pub active_tools: Mutex<Option<Vec<String>>>,
}

impl BridgeServices {
    fn set_session(&self, session_id: Option<String>) {
        *self.current_session.lock().unwrap() = session_id;
    }

    fn current_session_id(&self) -> Option<String> {
        self.current_session.lock().unwrap().clone()
    }

    /// 与 pi 路径 PermissionBridge 同款的询问链：
    /// 决策记忆命中 → 直返；否则注册 pending → SSE 推 PermissionRequest →
    /// 等决策（超时 fail-closed）→ always 决策回写记忆。
    async fn request_permission(&self, capability: &str, extension_id: Option<&str>) -> bool {
        // 1. 决策记忆：命中（allow/deny）直接返回，不再打扰用户
        if let Some(id) = extension_id {
            let store = self.permission_store.lock().unwrap();
            if let Some(allow) = store.lookup(id, capability) {
                tracing::debug!(
                    event = "bm.permission_memory_hit",
                    extension = id,
                    capability,
                    allow,
                    "决策记忆命中，跳过询问",
                );
                return allow;
            }
        }

        // 2. 无记忆 → 询问链
        let Some(session_id) = self.current_session_id() else {
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

        let Some(decision) = decision else {
            return false;
        };

        // 3. "总是"决策 → 回写记忆（对齐 pi 上游：问一次记一次；once 不记）
        if decision.always {
            let mut store = self.permission_store.lock().unwrap();
            if let Err(err) = store.record(extension_id, capability, decision.allow) {
                tracing::warn!(
                    event = "bm.permission_memory_write_failed",
                    extension = extension_id,
                    capability,
                    error = %err,
                );
            }
        }
        decision.allow
    }

    /// 宿主事件推送的 ctx payload（会话工作目录 + 空会话投影）。
    /// sessionEntries 投影留给后续切片（需要 db 消息 → JS ctx 桥接）。
    fn event_ctx(&self) -> serde_json::Value {
        serde_json::json!({
            "cwd": self.builtin.cwd().display().to_string(),
            "hasUI": false,
        })
    }
}

#[async_trait::async_trait]
impl HostServices for BridgeServices {
    /// `pi.tool(name, input)` → 内置工具集（B6）。
    /// 递归防护：只查内置表，未知名字即报错——插件工具互调在 JS 侧
    /// （import）完成，宿主桥不代查，天然无「插件→宿主→同引擎」递归环。
    async fn execute_tool(&self, _call_id: &str, name: &str, input: serde_json::Value) -> HostcallOutcome {
        match self.builtin.execute(name, input).await {
            Ok(value) => HostcallOutcome::Success(value),
            Err(err) => HostcallOutcome::Error {
                code: err.code.to_string(),
                message: err.message,
            },
        }
    }

    /// `pi.exec(cmd, {args?, options?})` → 进程执行（B6，镜像 legacy 非流式）。
    /// stream=true 暂不支持（三插件不用流式）；返回形状与 legacy 一致。
    async fn exec(&self, _call_id: &str, cmd: &str, payload: serde_json::Value) -> HostcallOutcome {
        let args = payload
            .get("args")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|v| v.as_str().map_or_else(|| v.to_string(), ToString::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let options = payload.get("options").cloned().unwrap_or(serde_json::json!({}));
        let cwd = options
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let timeout_ms = options
            .get("timeout")
            .and_then(serde_json::Value::as_u64)
            .filter(|t| *t > 0)
            .unwrap_or(EXEC_TIMEOUT_MS);

        match self.builtin.exec_cmd(cmd, &args, cwd.as_deref(), timeout_ms).await {
            Ok(value) => HostcallOutcome::Success(value),
            Err(err) => HostcallOutcome::Error {
                code: err.code.to_string(),
                message: err.message,
            },
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

    /// `pi.session(op, args)` → 会话数据面（B6 最小诚实子集）。
    /// 无执行期会话（加载期调用）→ denied。
    async fn session(&self, _call_id: &str, op: &str, payload: serde_json::Value) -> HostcallOutcome {
        let Some(session_id) = self.current_session_id() else {
            return HostcallOutcome::Error {
                code: "denied".to_string(),
                message: "session hostcall 无会话上下文".to_string(),
            };
        };
        let op = fold_op(op);
        let result = match op.as_str() {
            "getstate" => {
                let session = self.db.get_session(&session_id).await;
                match session {
                    Ok(Some(s)) => Ok(serde_json::json!({
                        "sessionId": s.id,
                        "sessionName": s.title,
                        "model": s.model,
                        "provider": s.provider_id,
                    })),
                    Ok(None) => Err("no session".to_string()),
                    Err(e) => Err(e.to_string()),
                }
            }
            "getname" => {
                let session = self.db.get_session(&session_id).await;
                match session {
                    Ok(Some(s)) => Ok(serde_json::Value::String(s.title)),
                    Ok(None) => Err("no session".to_string()),
                    Err(e) => Err(e.to_string()),
                }
            }
            "setname" => {
                let name = payload.get("name").and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.db
                    .rename_session(&session_id, &name)
                    .await
                    .map(|()| serde_json::Value::Null)
                    .map_err(|e| e.to_string())
            }
            "getmessages" => self
                .db
                .list_messages(&session_id)
                .await
                .map(|msgs| {
                    serde_json::Value::Array(
                        msgs.into_iter()
                            .map(|m| {
                                serde_json::json!({
                                    "id": m.id,
                                    "role": m.role,
                                    "content": m.content,
                                })
                            })
                            .collect(),
                    )
                })
                .map_err(|e| e.to_string()),
            // 投影面（模型可见历史，含压缩遮蔽）——事件日志 derive_messages；
            // 双写未启用时降级 messages 表（与 getmessages 同源）
            "getmessagesurface" => match self.event_log.as_ref() {
                None => self
                    .db
                    .list_messages(&session_id)
                    .await
                    .map(|msgs| {
                        serde_json::Value::Array(
                            msgs.into_iter()
                                .map(|m| {
                                    serde_json::json!({
                                        "id": m.id,
                                        "role": m.role,
                                        "content": m.content,
                                    })
                                })
                                .collect(),
                        )
                    })
                    .map_err(|e| e.to_string()),
                Some(log) => {
                    let sid = bm_protocol::SessionId::new(session_id.clone());
                    let bid = bm_protocol::BranchId::new("main");
                    log.derive_messages(&sid, &bid)
                        .await
                        .map(|msgs| {
                            serde_json::Value::Array(
                                msgs.into_iter()
                                    .map(|m| {
                                        serde_json::json!({
                                            "seq": m.seq,
                                            "role": m.role,
                                            "content": m.content,
                                        })
                                    })
                                    .collect(),
                            )
                        })
                        .map_err(|e| e.to_string())
                }
            },
            "getmodel" => {
                let session = self.db.get_session(&session_id).await;
                match session {
                    Ok(Some(s)) => Ok(serde_json::json!({
                        "provider": s.provider_id,
                        "modelId": s.model,
                    })),
                    Ok(None) => Err("no session".to_string()),
                    Err(e) => Err(e.to_string()),
                }
            }
            "setmodel" => {
                let provider = payload.get("provider").and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let model_id = payload.get("modelId").and_then(serde_json::Value::as_str)
                    .or_else(|| payload.get("model_id").and_then(serde_json::Value::as_str))
                    .unwrap_or_default()
                    .to_string();
                if provider.is_empty() || model_id.is_empty() {
                    return HostcallOutcome::Error {
                        code: "invalid_request".to_string(),
                        message: "setModel: provider and modelId are required".to_string(),
                    };
                }
                self.db
                    .set_session_model(&session_id, Some(&provider), Some(&model_id))
                    .await
                    .map(|()| serde_json::Value::Bool(true))
                    .map_err(|e| e.to_string())
            }
            _ => Err(format!("Unknown session op: {op}")),
        };
        match result {
            Ok(value) => HostcallOutcome::Success(value),
            Err(message) => HostcallOutcome::Error {
                code: "invalid_request".to_string(),
                message,
            },
        }
    }

    /// `pi.ui(op, args)` → bm 路径无扩展 UI 通道（前端扩展面板是 pi 桌面的
    /// 能力）。诚实返回：confirm → false（取消语义）、custom → closed，
    /// 其余 not_configured——不让插件把无响应当成功。
    async fn ui(
        &self,
        _call_id: &str,
        op: &str,
        _payload: serde_json::Value,
        _extension_id: Option<&str>,
    ) -> HostcallOutcome {
        match op.trim() {
            "confirm" => HostcallOutcome::Success(serde_json::Value::Bool(false)),
            "custom" => HostcallOutcome::Success(serde_json::json!({ "closed": true })),
            other => HostcallOutcome::Error {
                code: "not_configured".to_string(),
                message: format!("UI hostcall 未配置宿主通道：{other}"),
            },
        }
    }

    /// `pi.events(op, args)` → active tools 记忆 + 会话数据面（B6 子集）。
    async fn events(
        &self,
        _call_id: &str,
        op: &str,
        payload: serde_json::Value,
        _extension_id: Option<&str>,
    ) -> HostcallOutcome {
        match fold_op(op).as_str() {
            "getactivetools" => {
                let active = self.active_tools.lock().unwrap().clone();
                HostcallOutcome::Success(serde_json::json!({ "tools": active }))
            }
            "setactivetools" => {
                let tools = payload
                    .get("tools")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                *self.active_tools.lock().unwrap() = Some(tools);
                HostcallOutcome::Success(serde_json::Value::Null)
            }
            other => HostcallOutcome::Error {
                code: "invalid_request".to_string(),
                message: format!("Unknown events op: {other}"),
            },
        }
    }

    /// 权限询问：决策记忆命中直返；否则 `prompt` 裁决走询问链
    /// （PermissionBridge 同款）；fail-closed。
    async fn request_approval(&self, capability: &str, extension_id: Option<&str>) -> bool {
        self.request_permission(capability, extension_id).await
    }
}

/// QuickJS 引擎宿主：专用线程 + 命令通道 + 工具快照。
/// 工具快照启动加载后固化，插件安装/启用后经 [`CompatEngine::reload`]
/// 增量加载新扩展并刷新快照（2026-08-15 长程测试 P2：当前对话即时生效）；
/// 禁用/卸载无运行时卸载路径，工具面保留至服务重启（`reload` 只加载不卸载）。
pub struct CompatEngine {
    tx: mpsc::UnboundedSender<CompatCmd>,
    join: Option<std::thread::JoinHandle<()>>,
    /// 已成功加载的扩展 id（`reload` 据此跳过已加载项；重复加载会重注册工具）。
    loaded_ids: TokioMutex<HashSet<String>>,
    /// 工具快照（bm-loop ToolDef 形态；reload 后整体替换，std Mutex 短临界区）
    pub tools: std::sync::Mutex<Vec<bm_loop::model::ToolDef>>,
    /// B6：已注册工具名快照（events getActiveTools 的数据面；加载完成后填入）。
    pub tool_names: std::sync::Mutex<Vec<String>>,
    /// B6：工作目录（插件事件 ctx 的 cwd 数据面）。
    pub working_dir: PathBuf,
}

impl CompatEngine {
    /// 起专用线程 + 引导 runtime。返回引擎句柄（工具快照由
    /// [`crate::compat_engine::init_compat`] 在加载完成后填入）。
    /// `session_streams`/`permission_pending` 是 AppState 的同名组件
    /// （本引擎建于 AppState 之前，只拿组件不拿整态）；`db`/`working_dir`
    /// 是 B6 会话端口与内置工具集的数据面；`event_log` = 投影面数据源
    /// （getmessagesurface，None 降级 messages 表）。
    pub async fn spawn(
        session_streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>>,
        permission_pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
        db: Arc<bm_core::Db>,
        working_dir: PathBuf,
        permission_store: PermissionStore,
        event_log: Option<bm_kernel::EventLog>,
    ) -> Result<Self, String> {
        let (tx, mut rx) = mpsc::unbounded_channel::<CompatCmd>();
        let (boot_tx, boot_rx) = oneshot::channel::<Result<(), String>>();
        let engine_working_dir = working_dir.clone();
        let services = Arc::new(BridgeServices {
            session_streams,
            permission_pending,
            current_session: Mutex::new(None),
            builtin: BuiltinTools::new(working_dir),
            permission_store: std::sync::Mutex::new(permission_store),
            db,
            event_log,
            active_tools: Mutex::new(None),
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
                                    services.event_ctx(),
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
                            CompatCmd::DispatchEvent { name, payload, ctx, reply } => {
                                let ctx_cwd = ctx
                                    .get("cwd")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string)
                                    .unwrap_or_else(|| "<none>".to_string());
                                let res = bm_compat::events::dispatch_extension_event(
                                    &thread,
                                    &name,
                                    payload,
                                    ctx,
                                    EVENT_TIMEOUT,
                                )
                                .await;
                                // 观测：事件名 + 结果摘要（不打 payload——含工具输出正文）
                                let summary = match &res {
                                    Ok(v) if v.is_null() => "null".to_string(),
                                    Ok(v) => v.get("trimmed").map_or_else(
                                        || "value".to_string(),
                                        |_| "trimmed".to_string(),
                                    ),
                                    Err(e) => format!("error: {e}"),
                                };
                                tracing::info!(
                                    event = "bm.plugin_event_done",
                                    name = %name,
                                    result = %summary,
                                    ctx_cwd = %ctx_cwd,
                                );
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
            loaded_ids: TokioMutex::new(HashSet::new()),
            tools: std::sync::Mutex::new(Vec::new()),
            tool_names: std::sync::Mutex::new(Vec::new()),
            working_dir: engine_working_dir,
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
            .map_err(|err| err.to_string())    }

    /// 增量加载当前配置里尚未加载的启用插件，并刷新工具快照
    /// （P2，2026-08-15 长程测试：插件安装/启用后当前对话即时生效——
    /// 调用方先改配置，再 reload + 失效会话 agent）。禁用/卸载无运行时
    /// 卸载路径，工具面保留至服务重启。
    pub async fn reload(&self, config: &bm_core::AppConfig) -> usize {
        let paths = bm_core::plugins::enabled_extension_paths(config);
        let mut loaded = 0usize;
        for path in &paths {
            let spec = match JsExtensionLoadSpec::from_entry_path(path) {
                Ok(spec) => spec,
                Err(err) => {
                    tracing::warn!(event = "bm.compat_reload_spec_failed", path = %path.display(), error = %err);
                    continue;
                }
            };
            if self.loaded_ids.lock().await.contains(&spec.extension_id) {
                continue;
            }
            match self.load(&spec).await {
                Ok(_) => {
                    self.loaded_ids.lock().await.insert(spec.extension_id.clone());
                    loaded += 1;
                    tracing::info!(event = "bm.compat_plugin_loaded", id = %spec.extension_id, reload = true);
                }
                Err(err) => {
                    tracing::warn!(event = "bm.compat_reload_failed", id = %spec.extension_id, error = %err);
                }
            }
        }
        if loaded > 0 {
            match self.read_tools().await {
                Ok(tools) => {
                    *self.tools.lock().unwrap() = tools.iter().map(to_loop_tool).collect();
                    *self.tool_names.lock().unwrap() = tools.iter().map(|t| t.name.clone()).collect();
                    tracing::info!(
                        event = "bm.compat_reloaded",
                        loaded,
                        tools = self.tools.lock().unwrap().len(),
                    );
                }
                Err(err) => {
                    tracing::warn!(event = "bm.compat_reload_tools_failed", error = %err);
                }
            }
        }
        loaded
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

    /// B6：宿主→插件事件（startup/tool_call/tool_result…）。返回 handler
    /// 链最后非 undefined 值（无 handler → Null）。
    pub async fn dispatch_event(
        &self,
        name: &str,
        payload: serde_json::Value,
        ctx: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(CompatCmd::DispatchEvent {
                name: name.to_string(),
                payload,
                ctx,
                reply,
            })
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
/// B6：`db`/`working_dir` 接入（session 端口/内置工具集），决策记忆在
/// `~/.boenmind/extension-permissions.json`（app_dir 下，与 pi 上游的
/// 同名文件并置——上游在 agents 目录下，两文件独立但格式兼容）。
pub async fn init_compat(
    config: &bm_core::AppConfig,
    session_streams: Arc<TokioMutex<HashMap<String, mpsc::UnboundedSender<AgentStreamEvent>>>>,
    permission_pending: Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
    db: Arc<bm_core::Db>,
    event_log: Option<bm_kernel::EventLog>,
) -> Option<Arc<CompatEngine>> {
    let permission_store = {
        let path = bm_core::config::app_dir().join("extension-permissions.json");
        match PermissionStore::open(&path) {
            Ok(store) => store,
            Err(err) => {
                // 决策记忆打不开 → 每次询问（fail-open 到询问链，不阻断引擎）
                tracing::warn!(event = "bm.permission_store_open_failed", path = %path.display(), error = %err);
                PermissionStore::open(&tempfile::tempdir()
                    .map(|d| d.keep())
                    .unwrap_or_else(|_| std::env::temp_dir())
                    .join("extension-permissions.json"))
                .unwrap_or_else(|_| {
                    PermissionStore::open(&std::env::temp_dir().join(format!(
                        "boenmind-permissions-{}.json",
                        uuid::Uuid::new_v4()
                    )))
                    .expect("临时决策记忆不可用")
                })
            }
        }
    };
    let engine = match CompatEngine::spawn(
        session_streams,
        permission_pending,
        db,
        config.working_dir.clone(),
        permission_store,
        event_log,
    )
    .await
    {
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
                engine.loaded_ids.lock().await.insert(spec.extension_id.clone());
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
            *engine.tools.lock().unwrap() = tools.iter().map(to_loop_tool).collect();
            *engine.tool_names.lock().unwrap() = tools.iter().map(|t| t.name.clone()).collect();
            tracing::info!(
                event = "bm.compat_ready",
                plugins = loaded,
                tools = engine.tools.lock().unwrap().len(),
            );
        }
        Err(err) => {
            tracing::warn!(event = "bm.compat_tools_failed", error = %err);
        }
    }
    Some(Arc::new(engine))
}

/// bm-loop `ToolExecutor` 的 QuickJS 实现（B4/B5/B6）：`execute` →
/// `__pi_execute_tool` 桥；执行前后推 `tool_call`/`tool_result` 插件事件
/// （fire-and-forget——ctx-compactor 的修剪/落库挂在 tool_result 上）。
/// 每会话一个实例（携带 session_id，权限询问路由用）。
pub struct QuickJsToolExecutor {
    engine: Option<Arc<CompatEngine>>,
    session_id: String,
    /// 管家状态（set_wake 工具执行侧；Steward 轮 v0.19）。
    steward: Option<Arc<crate::steward::StewardStore>>,
    /// 事件日志（todo 工具执行侧；M2 活任务清单。None = 事件日志不可用，
    /// todo 工具返回错误——子进程无日志，且子进程工具面不含 todo 双保险）。
    event_log: Option<EventLog>,
}

impl QuickJsToolExecutor {
    pub fn new(
        engine: Option<Arc<CompatEngine>>,
        session_id: impl Into<String>,
        steward: Option<Arc<crate::steward::StewardStore>>,
    ) -> Self {
        Self {
            engine,
            session_id: session_id.into(),
            steward,
            event_log: None,
        }
    }

    /// 挂事件日志（todo 工具用）。bm 引擎父进程路径调用；子进程不挂。
    pub fn with_event_log(mut self, log: EventLog) -> Self {
        self.event_log = Some(log);
        self
    }

    /// 插件事件 ctx：cwd（JS `__pi_make_extension_ctx` 的输入）。
    /// sessionEntries 投影留后续切片；B6 先给 cwd。
    fn event_ctx(&self) -> serde_json::Value {
        let cwd = self
            .engine
            .as_ref()
            .map(|e| e.working_dir.display().to_string())
            .unwrap_or_default();
        serde_json::json!({
            "cwd": cwd,
            "hasUI": false,
        })
    }
}

impl ToolExecutor for QuickJsToolExecutor {
    async fn execute(&self, req: ToolCallRequest) -> ToolOutcome {
        // tool_call 事件（fire-and-forget；handler 可 block——B6 先不消费返回值）。
        // 内置与插件工具两个路径都发（插件钩子按 toolName 自行过滤）。
        let ctx = self.event_ctx();
        if let Some(engine) = &self.engine {
            let payload = serde_json::json!({
                "type": "tool_call",
                "toolName": req.name,
                "toolCallId": req.call_id,
                "input": req.args,
            });
            if let Err(err) = engine.dispatch_event("tool_call", payload, ctx.clone()).await {
                tracing::debug!(event = "bm.plugin_event_failed", name = "tool_call", error = %err);
            }
        }

        // 分派：subagent（专家团队，父侧自研）→ 内置工具名 → BuiltinTools
        // （闭合表，递归防护与 execute_tool 端口一致）；其余 → QuickJS 插件引擎。
        let working_dir = self
            .engine
            .as_ref()
            .map(|e| e.working_dir.clone())
            .unwrap_or_default();
        let outcome = if req.name == "subagent" {
            // 父侧 subagent 工具：发现角色 → spawn 子进程 → 摄取事件流
            // （子进程协议 = subagent_child；取消经 kill_on_drop 传播）
            crate::subagent_tool::run(req.args.clone(), &working_dir).await
        } else if req.name == "set_wake" {
            // 管家自调节奏（Steward 轮）：写 next_wake_at（治理层夹区间）。
            // 只注册进管家会话工具面；store 缺失时模型面也不会有此工具（双保险）。
            match &self.steward {
                Some(store) => match crate::steward::execute_set_wake(store, &self.session_id, &req.args).await {
                    Ok(value) => ToolOutcome {
                        ok: true,
                        output: tool_result_text(&value),
                        meta: Some(value),
                    },
                    Err(err) => ToolOutcome { ok: false, output: err, meta: None },
                },
                None => ToolOutcome {
                    ok: false,
                    output: "管家未启用（set_wake 不可用）".to_string(),
                    meta: None,
                },
            }
        } else if req.name == "todo" {
            // 活任务清单（M2）：读/写事件日志 todo/write 快照（事实源）。
            // 事件日志不可用或未挂（子进程）→ 明确报错，不让模型空转。
            match &self.event_log {
                Some(log) => match crate::todo_tool::execute_todo(log, &self.session_id, &req.args).await {
                    Ok(value) => ToolOutcome {
                        ok: true,
                        output: tool_result_text(&value),
                        meta: Some(value),
                    },
                    Err(err) => ToolOutcome { ok: false, output: err, meta: None },
                },
                None => ToolOutcome {
                    ok: false,
                    output: "todo 工具不可用（事件日志未挂载）".to_string(),
                    meta: None,
                },
            }
        } else if crate::builtin_tools::BuiltinTools::NAMES.contains(&req.name.as_str()) {
            let builtin = BuiltinTools::new(working_dir);
            match builtin.execute(&req.name, req.args.clone()).await {
                Ok(value) => ToolOutcome {
                    ok: true,
                    output: tool_result_text(&value),
                    meta: Some(value),
                },
                Err(err) => ToolOutcome {
                    ok: false,
                    output: err.to_string(),
                    meta: None,
                },
            }
        } else {
            let Some(engine) = &self.engine else {
                return ToolOutcome {
                    ok: false,
                    output: "插件引擎未启用（bm-compat 启动失败或已禁用）".to_string(),
                    meta: None,
                };
            };
            match engine
                .execute(&req.name, &req.call_id, req.args.clone(), TOOL_TIMEOUT_MS, &self.session_id)
                .await
            {
                Ok(value) => ToolOutcome {
                    ok: true,
                    output: tool_result_text(&value),
                    meta: Some(value.clone()),
                },
                Err(err) => ToolOutcome {
                    ok: false,
                    output: err.clone(),
                    meta: None,
                },
            }
        };

        // tool_result 事件（ctx-compactor 的修剪 hook 在此）。
        // payload 形状对齐 legacy ToolResult 事件：content = ContentBlock
        // 数组（插件返回包的 content 字段优先，无则文本块兜底）——插件钩子
        // （extractText）按此形状消费。
        if let Some(engine) = &self.engine {
            let payload = serde_json::json!({
                "type": "tool_result",
                "toolName": req.name,
                "toolCallId": req.call_id,
                "input": req.args,
                "content": content_blocks(&outcome),
                "details": outcome.meta.as_ref().and_then(|m| m.get("details")).cloned(),
                "isError": !outcome.ok,
            });
            if let Err(err) = engine.dispatch_event("tool_result", payload, ctx).await {
                tracing::debug!(event = "bm.plugin_event_failed", name = "tool_result", error = %err);
            }
        }
        outcome
    }
}

/// 工具结果 → legacy `ToolResult` 事件同款 content 块数组（ctx-compactor
/// 的 `extractText` 消费此形状；无 content 数组时文本块兜底保真）。
fn content_blocks(outcome: &ToolOutcome) -> serde_json::Value {
    match outcome
        .meta
        .as_ref()
        .and_then(|m| m.get("content"))
        .and_then(serde_json::Value::as_array)
    {
        Some(blocks) if !blocks.is_empty() => serde_json::Value::Array(blocks.clone()),
        _ => serde_json::json!([{ "type": "text", "text": outcome.output }]),
    }
}

/// op 名折叠比较（get_state/getState/getstate 同义——对齐 legacy 的
/// folded-alnum token 语义的简化版）。
fn fold_op(op: &str) -> String {
    op.to_ascii_lowercase().replace(['_', '-'], "")
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

    /// 测试库环境锁：BOENMIND_HOME 是进程级 env，并发测试互斥设置。
    static DB_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 临时目录内的测试 Db（不触碰用户真实数据目录）。
    async fn test_db() -> Arc<bm_core::Db> {
        let _guard = DB_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir = std::env::temp_dir().join(format!("bm-compat-db-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let original = std::env::var_os("BOENMIND_HOME");
        unsafe { std::env::set_var("BOENMIND_HOME", &dir) };
        let db = bm_core::Db::open().await.expect("test db open");
        match original {
            Some(v) => unsafe { std::env::set_var("BOENMIND_HOME", v) },
            None => unsafe { std::env::remove_var("BOENMIND_HOME") },
        }
        Arc::new(db)
    }

    async fn bridge() -> BridgeServices {
        // keep()：builtin 的 cwd 要跨调用存活（exec 在 bridge 返回后 spawn）
        let temp_dir = tempfile::tempdir().unwrap().keep();
        let store_path = temp_dir.join("extension-permissions.json");
        BridgeServices {
            session_streams: Arc::new(TokioMutex::new(HashMap::new())),
            permission_pending: Arc::new(TokioMutex::new(HashMap::new())),
            current_session: Mutex::new(None),
            builtin: BuiltinTools::new(temp_dir.clone()),
            permission_store: std::sync::Mutex::new(
                PermissionStore::open(&store_path).expect("store"),
            ),
            db: test_db().await,
            event_log: None,
            active_tools: Mutex::new(None),
        }
    }

    #[tokio::test]
    async fn approval_fails_closed_without_session() {
        let services = bridge().await;
        // 无执行期会话上下文（加载期/异常路径）→ 拒绝且不挂起
        let allowed = services.request_approval("http", Some("web-search")).await;
        assert!(!allowed);
        assert!(services.permission_pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn approval_memory_hit_skips_prompt() {
        let services = bridge().await;
        services
            .permission_store
            .lock()
            .unwrap()
            .record("web-search", "http", true)
            .unwrap();
        // 决策记忆命中：无会话上下文也放行（不走询问链）
        assert!(services.request_approval("http", Some("web-search")).await);
        assert!(services.permission_pending.lock().await.is_empty());
        // 未记忆的能力仍 fail-closed
        assert!(!services.request_approval("exec", Some("web-search")).await);
    }

    #[tokio::test]
    async fn http_requires_url() {
        let services = bridge().await;
        match services.http("c1", serde_json::json!({})).await {
            HostcallOutcome::Error { code, .. } => assert_eq!(code, "invalid_request"),
            other => panic!("应返回 invalid_request，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_tool_routes_to_builtins() {
        let services = bridge().await;
        // 内置 write → 成功形状（content[].text）
        match services
            .execute_tool(
                "c1",
                "write",
                serde_json::json!({ "path": "out/note.txt", "content": "hi" }),
            )
            .await
        {
            HostcallOutcome::Success(v) => {
                assert!(v["content"][0]["text"].as_str().unwrap().contains("Successfully wrote"));
            }
            other => panic!("write 应成功，得到 {other:?}"),
        }
        // 未知工具 → 报错（递归防护：不查插件注册表）
        match services.execute_tool("c2", "hello", serde_json::json!({})).await {
            HostcallOutcome::Error { code, message } => {
                assert_eq!(code, "invalid_request");
                assert!(message.contains("Unknown tool"), "{message}");
            }
            other => panic!("未知工具应报错，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn exec_runs_process_and_captures() {
        let services = bridge().await;
        #[cfg(windows)]
        let (cmd, args) = ("cmd", vec!["/C".to_string(), "echo exec-ok".to_string()]);
        #[cfg(not(windows))]
        let (cmd, args) = ("/bin/sh", vec!["-c".to_string(), "echo exec-ok".to_string()]);
        match services
            .exec("c1", cmd, serde_json::json!({ "args": args }))
            .await
        {
            HostcallOutcome::Success(v) => {
                assert_eq!(v["code"], 0, "{v}");
                assert!(v["stdout"].as_str().unwrap().contains("exec-ok"), "{v}");
            }
            other => panic!("exec 应成功，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn session_requires_active_session() {
        let services = bridge().await;
        // 无会话上下文 → denied
        match services.session("c1", "get_state", serde_json::json!({})).await {
            HostcallOutcome::Error { code, .. } => assert_eq!(code, "denied"),
            other => panic!("应 denied，得到 {other:?}"),
        }
        // 未知 op（有会话上下文时走 op 分发）→ invalid_request
        services.set_session(Some("s1".to_string()));
        match services.session("c2", "no_such_op", serde_json::json!({})).await {
            HostcallOutcome::Error { code, message } => {
                assert_eq!(code, "invalid_request");
                assert!(message.contains("Unknown session op"), "{message}");
            }
            other => panic!("应 invalid_request，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn ui_returns_honest_unconfigured() {
        let services = bridge().await;
        match services.ui("c1", "confirm", serde_json::json!({}), None).await {
            HostcallOutcome::Success(v) => assert_eq!(v, serde_json::Value::Bool(false)),
            other => panic!("confirm 应返回 false，得到 {other:?}"),
        }
        match services.ui("c2", "toast", serde_json::json!({}), None).await {
            HostcallOutcome::Error { code, .. } => assert_eq!(code, "not_configured"),
            other => panic!("toast 应 not_configured，得到 {other:?}"),
        }
    }

    #[tokio::test]
    async fn events_active_tools_memory() {
        let services = bridge().await;
        match services.events("c1", "get_active_tools", serde_json::json!({}), None).await {
            HostcallOutcome::Success(v) => assert_eq!(v["tools"], serde_json::Value::Null),
            other => panic!("应返回 null 工具表，得到 {other:?}"),
        }
        services
            .events(
                "c2",
                "set_active_tools",
                serde_json::json!({ "tools": ["read", "write"] }),
                None,
            )
            .await;
        match services.events("c3", "get_active_tools", serde_json::json!({}), None).await {
            HostcallOutcome::Success(v) => {
                assert_eq!(v["tools"], serde_json::json!(["read", "write"]))
            }
            other => panic!("应返回记忆的工具表，得到 {other:?}"),
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
    fn content_blocks_prefers_plugin_blocks() {
        // 插件返回包带 content 数组 → 原样透传（ctx-compactor extractText 消费）
        let ok = ToolOutcome {
            ok: true,
            output: "你好".into(),
            meta: Some(serde_json::json!({
                "content": [{ "type": "text", "text": "你好" }],
                "details": { "x": 1 },
            })),
        };
        assert_eq!(content_blocks(&ok), serde_json::json!([{ "type": "text", "text": "你好" }]));
        // 无 meta（错误）→ 文本块兜底保真
        let err = ToolOutcome {
            ok: false,
            output: "boom".into(),
            meta: None,
        };
        assert_eq!(content_blocks(&err), serde_json::json!([{ "type": "text", "text": "boom" }]));
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
