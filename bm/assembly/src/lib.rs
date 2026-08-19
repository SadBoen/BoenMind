//! # bm-assembly
//!
//! 组合根：装配微内核各端口为运行时，并提供会话创建/恢复的完整闭环
//! （含 interrupted-turn 修复：kill -9 后重载日志，把未完成回合的尾部
//! 未配对事件修剪掉，保证恢复后的日志没有 torn 状态）。
//!
//! 核心三插件化（最小基座 = provider + loop + tools）：
//! - LLM 与 Tools 已是 `Arc<dyn>` trait object / 运行期注册表（插拔友好），
//!   此处补**正式换装 API**（`swap_llm` / `register_tool` / `unregister_tool`，
//!   替代上层裸改 pub 字段）；
//! - Loop 抽象为 [`kernel_loop::AgentPort`]（对应 dsh 官方 `dsh-agent-loop`
//!   插件），`swap_loop` 换装会话代理工厂；
//! - 三插件各有清单身份（category=Core），经 [`Runtime::plugin_manifest`]
//!   对外暴露，供插件管理员分组隐藏。

use std::path::PathBuf;
use std::sync::Arc;

use kernel_contracts::bus::EventBus;
use kernel_contracts::llm::{GenerateOptions, LlmPort};
use kernel_contracts::plugin::PluginManifestEntry;
use kernel_contracts::ports::{
    PluginRuntimeAvailability, PluginRuntimePort, SessionPersistPort,
};
use kernel_contracts::session::{
    SessionEvent, SessionHeader, StepPhase, TurnEndReason, TurnEvent,
};
use kernel_contracts::PortResult;
use kernel_session::{AgentPort};
use plugin_loop::{LoopRuntime, ReactLoopAgent};
use kernel_session::{Session, SessionStore};
use kernel_storage::SqlitePersist;
use plugin_tools::{ToolGate, ToolRegistry};
use parking_lot::RwLock;

pub mod config;
pub mod js_host;
pub mod provider;

/// 脚本化 mock LLM 装配（门禁/headless 用）：按脚本产出工具调用 + 续文本。
/// 组合根职责：装配 mock 也是装配；headless（L0）经此获得 mock，不直接依赖 plugin-llm。
pub use plugin_llm::MockTurn;

/// 构造一个脚本化 mock LLM（`swap_llm` 的输入）。见 [`MockTurn`]。
pub fn scripted_llm(
    provider: String,
    model: String,
    script: Vec<MockTurn>,
) -> Arc<dyn LlmPort> {
    Arc::new(plugin_llm::ScriptLlm::new(provider, model, script))
}

/// 装配错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssemblyError {
    #[error("persist error: {0}")]
    Persist(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("invalid session log: {0}")]
    InvalidLog(String),
    #[error("plugin runtime unavailable: {0}")]
    PluginUnavailable(String),
}

/// 会话代理工厂：装配方用它把（loop 运行时 + 会话）组装成代理。
/// `Runtime` 本身持有默认工厂（ReactLoopAgent）；`swap_loop` 可换装
/// 自定义实现——运行中会话不受影响，后续 create/restore 用新实现。
/// `Arc`（非 `Box`）：读锁内 clone 后释放锁再调用，避免持锁跨 await。
pub type AgentFactory = Arc<dyn Fn(Arc<LoopRuntime>, Arc<Session>) -> Arc<dyn AgentPort> + Send + Sync>;

/// 默认工厂：产出标准 [`ReactLoopAgent`]（官方 `dsh-agent-loop` 的 Rust 移植）。
pub fn default_agent_factory() -> AgentFactory {
    Arc::new(|rt, session| -> Arc<dyn AgentPort> { Arc::new(ReactLoopAgent::new(rt, session)) })
}

/// LLM 共享换装代理：每回合**现读**当前装配实现（`swap_llm` 后下一请求
/// 生效，对**所有**已创建会话生效——对齐 settingsNs 热替换"写后下一请求
/// 生效"语义；运行中回合持旧 `Arc` 不受影响，RwLock 读侧零锁开销）。
struct SharedLlm {
    inner: Arc<RwLock<Arc<dyn LlmPort>>>,
}

impl SharedLlm {
    fn new(inner: Arc<RwLock<Arc<dyn LlmPort>>>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl LlmPort for SharedLlm {
    async fn list_models(&self, provider: &str) -> Result<Vec<kernel_contracts::LlmModelInfo>, kernel_contracts::LlmError> {
        // parking_lot guard 非 Send：先 clone 出 Arc 释放锁，再跨 await。
        let llm = self.inner.read().clone();
        llm.list_models(provider).await
    }

    fn stream(&self, request: GenerateOptions) -> kernel_contracts::ChunkStream {
        let llm = self.inner.read().clone();
        llm.stream(request)
    }
}

/// 微内核组合根：所有端口经此装配。
pub struct Runtime {
    /// LLM 端口（核心插件 `llm`）：`Arc<RwLock<..>>` 承载 `swap_llm` 热换装，
    /// 与各会话的 SharedLlm 共享同一把锁——换装后**下一请求生效**（所有会话），
    /// 运行中回合持旧 `Arc` 不受影响。
    llm: Arc<RwLock<Arc<dyn LlmPort>>>,
    /// 会话代理工厂（核心插件 `loop`）：`swap_loop` 换装后新会话生效。
    agent_factory: RwLock<AgentFactory>,
    pub store: Arc<SessionStore>,
    pub tools: Arc<ToolRegistry>,
    pub gate: Arc<ToolGate>,
    pub persist: Arc<dyn SessionPersistPort>,
    pub plugin_runtime: Arc<dyn PluginRuntimePort>,
    pub provider: String,
    pub model: String,
    pub bus: EventBus,
    /// 单回合最大 step 数（数值可配置；装配默认 [`kernel_session::DEFAULT_MAX_STEPS`]）。
    pub max_steps: u64,
    /// 核心插件清单（llm / loop / tools，category=Core）。
    core_plugins: Vec<PluginManifestEntry>,
}

impl Runtime {
    /// 创建一个新的运行时（headless profile：内存 store + sqlite 持久化 + mock LLM）。
    pub fn headless(sqlite_path: PathBuf) -> Result<Self, AssemblyError> {
        Self::headless_with_max_steps(sqlite_path, kernel_session::DEFAULT_MAX_STEPS)
    }

    /// 带 max_steps 的 headless 装配（web-server 经 `--max-steps` 覆盖）。
    ///
    /// 走**核心三插件装配路径**：llm 插件（ScriptLlm mock）+ loop 插件
    /// （ReactLoopAgent 默认工厂）+ tools 插件（内置工具组）——最小基座
    /// 即完整回合闭环；真实 provider 由上层 `swap_llm` 换装。
    pub fn headless_with_max_steps(
        sqlite_path: PathBuf,
        max_steps: u64,
    ) -> Result<Self, AssemblyError> {
        let persist = Arc::new(
            SqlitePersist::open(&sqlite_path).map_err(|e| AssemblyError::Persist(e.to_string()))?,
        );
        let llm = Arc::new(plugin_llm::ScriptLlm::new(
            "mock".to_string(),
            "mock-1".to_string(),
            vec![],
        ));
        let bus = EventBus::new();
        let store = Arc::new(SessionStore::new());
        let tools = Arc::new(ToolRegistry::new());
        // tools 插件装配点：内置工具组注册（当前为空集，host 工具由 web-server 注册）。
        plugin_tools::plugin::register_builtin(&tools);
        let gate = Arc::new(ToolGate::new());
        Ok(Self {
            llm: Arc::new(RwLock::new(llm)),
            store,
            tools,
            gate,
            persist,
            plugin_runtime: Arc::new(kernel_contracts::UnavailablePluginRuntime),
            provider: "mock".to_string(),
            model: "mock-1".to_string(),
            bus,
            max_steps,
            agent_factory: RwLock::new(default_agent_factory()),
            core_plugins: vec![
                plugin_llm::plugin::manifest(),
                plugin_loop::plugin::manifest(),
                plugin_tools::plugin::manifest(),
            ],
        })
    }

    /// 核心插件清单（llm / loop / tools，category 全 Core）。供插件管理员/
    /// 前端按分类分组展示——核心组件与功能插件分界，用户日常不可见。
    pub fn plugin_manifest(&self) -> Vec<PluginManifestEntry> {
        self.core_plugins.clone()
    }

    /// 热换装 LLM 实现（核心插件 `llm`）：替换后**下一请求生效**——
    /// 运行中回合持旧 `Arc` 不受影响；每回合现读当前实现，所有已创建
    /// 会话共享（对齐 settingsNs 热替换语义）。替代旧做法（裸改 pub 字段）。
    pub fn swap_llm(&self, llm: Arc<dyn LlmPort>) {
        *self.llm.write() = llm;
    }

    /// 装配真 provider（组合根唯一装配点）：从配置构造适配器 + 聚合路由 +
    /// 元数据，swap 进运行时，返回前端数据源。见 [`provider::assemble_providers`]。
    pub fn apply_llm(
        &mut self,
        config: &config::LlmConfig,
        user_id: String,
    ) -> Result<(Vec<provider::ProviderRuntime>, String, String), String> {
        let (runtimes, llm, default_provider, default_model) =
            provider::assemble_providers(config, user_id)?;
        self.swap_llm(llm);
        self.provider = default_provider.clone();
        self.model = default_model.clone();
        Ok((runtimes, default_provider, default_model))
    }

    /// 热换装 loop 实现（核心插件 `loop`）：替换会话代理工厂——
    /// 运行中会话不受影响，后续 `create_session` / `restore_session`
    /// 产出新实现。形态 = 进程内组件（trait object 分发），与 dsh 官方
    /// `dsh-agent-loop` Cordis 插件同构，非独立进程。
    pub fn swap_loop(&self, factory: AgentFactory) {
        *self.agent_factory.write() = factory;
    }

    /// 注册一个工具（运行期热插拔；重名拒绝）。门控 fail-closed 默认全禁用，
    /// 需显式 `gate.enable`。
    pub fn register_tool(&self, handler: Arc<dyn kernel_contracts::ToolHandler>) -> Result<(), kernel_contracts::ToolError> {
        self.tools.register(handler)
    }

    /// 卸载一个工具（运行期热插拔）。
    pub fn unregister_tool(&self, name: &str) -> Result<(), kernel_contracts::ToolError> {
        self.tools.unregister(name)
    }

    /// 创建一个新会话（写入 header 索引 + 首条 SessionStarted），返回代理。
    pub async fn create_session(
        &self,
        header: SessionHeader,
    ) -> Result<Arc<dyn AgentPort>, AssemblyError> {
        let session = self.store.create(header, self.bus.clone());
        self.persist
            .create_session(session.header())
            .await
            .map_err(|e| AssemblyError::Persist(e.to_string()))?;
        let rt = self.loop_runtime();
        let factory = self.agent_factory.read().clone();
        Ok(factory(rt, session))
    }

    /// 从持久化日志恢复会话（kill -9 后重载），自动做 interrupted-turn 修复。
    pub async fn restore_session(
        &self,
        session_id: &str,
    ) -> Result<Arc<dyn AgentPort>, AssemblyError> {
        let Some(records) = self
            .persist
            .load_events(session_id)
            .await
            .map_err(|e| AssemblyError::Persist(e.to_string()))?
        else {
            return Err(AssemblyError::SessionNotFound(session_id.to_string()));
        };
        if records.is_empty() {
            return Err(AssemblyError::InvalidLog("empty event log".into()));
        }
        // 首条必须是 SessionStarted，从中取回 header。
        let first = records
            .first()
            .ok_or_else(|| AssemblyError::InvalidLog("empty log".into()))?;
        let header = match &first.event {
            SessionEvent::SessionStarted { header } => header.clone(),
            _ => {
                return Err(AssemblyError::InvalidLog(
                    "first event not SessionStarted".into(),
                ))
            }
        };

        let original_len = records.len();
        // interrupted-turn 修复：闭合孤儿回合尾部（追加 closers，不删事件）。
        let events: Vec<SessionEvent> = records.iter().map(|r| r.event.clone()).collect();
        let repaired = repair_interrupted_turn(events);
        // 修复必须落盘：磁盘与内存一致，torn-tail 是磁盘层不变量。
        if repaired.len() != original_len {
            self.persist
                .rewrite_events(session_id, &repaired)
                .await
                .map_err(|e| AssemblyError::Persist(e.to_string()))?;
        }
        // 沿用磁盘 seq/timestamp（时间线保真）。rewrite 后磁盘 seq 连续从 1 起、
        // 追加 closers 的时间戳 = rewrite 落盘时间，内存须与此一致。
        let records: Vec<kernel_contracts::SessionRecord> = if repaired.len() != original_len {
            repaired
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    // 原事件沿用落盘时间戳；新增 closers（超出原长度）用现在。
                    let timestamp = records
                        .get(i)
                        .map(|r| r.timestamp)
                        .unwrap_or_else(chrono::Utc::now);
                    kernel_contracts::SessionRecord {
                        seq: (i + 1) as u64,
                        timestamp,
                        session_id: kernel_contracts::SessionId(session_id.to_string()),
                        event: e.clone(),
                    }
                })
                .collect()
        } else {
            records
        };
        let session = self
            .store
            .restore(header, records, self.bus.clone())
            .map_err(|e| AssemblyError::InvalidLog(e.to_string()))?;
        let rt = self.loop_runtime();
        let factory = self.agent_factory.read().clone();
        Ok(factory(rt, session))
    }

    /// 列出已持久化的会话 id。
    pub async fn list_sessions(&self) -> PortResult<Vec<String>> {
        self.persist.list_sessions().await
    }

    /// 探测插件运行时（fail-loud：未装配必须显式处理）。
    pub fn plugin_availability(&self) -> PluginRuntimeAvailability {
        self.plugin_runtime.availability()
    }

    /// QuickJS 桥装配（§5.4 接真 LLM + §5.5 tools/session 面 + §5.6 config 面）：
    /// 把当前装配的 LLM（`self.llm`，聚合 `LlmPort`，与 agent-loop 共享）、工具
    /// 注册表+门控（`self.tools`/`self.gate`，fail-closed）、会话存储（`self.store`
    /// 拉模型投影）与 config 白名单接进 `HostApi` 宿主并返回引擎。
    ///
    /// `faces` = manifest 声明的 host 面子集（最小权限授面，见
    /// quickjs-bridge §5.3）。`apply_llm` 之后调用即走真 provider；headless
    /// （ScriptLlm mock）也走同一端口（桥层对 mock/真 provider 无感）。
    pub fn js_bridge(&self, faces: &[&str]) -> Result<quickjs_bridge::JsBridge, String> {
        self.js_bridge_with_config(faces, std::collections::HashMap::new())
    }

    /// 同 [`Self::js_bridge`]，额外注入 config 面白名单（`"{plugin_id}.{key}" →
    /// value`）。**白名单即全部内容，永不返回 secret**（credentials 由凭据面管）。
    pub fn js_bridge_with_config(
        &self,
        faces: &[&str],
        config: std::collections::HashMap<String, String>,
    ) -> Result<quickjs_bridge::JsBridge, String> {
        let llm = self.llm.read().clone();
        let host = Arc::new(js_host::RealHost::new(
            llm,
            Arc::clone(&self.tools),
            Arc::clone(&self.gate),
            Arc::clone(&self.store),
            config,
        ));
        quickjs_bridge::JsBridge::new_with_faces(host, faces)
    }

    /// §5.6 目录插件注册表：扫描 `dir` 下全部插件（plugin.json），返回装载清单
    /// （manifest + 入口源码，按 id 字典序；损坏插件 fail-loud）。引擎装配走
    /// [`Self::js_bridge`]（按 manifest 授面）。
    pub fn scan_js_plugins(&self, dir: &std::path::Path) -> Result<Vec<quickjs_bridge::LoadedPlugin>, String> {
        let _ = &self;
        quickjs_bridge::scan_plugins(dir)
    }

    /// §5.6 装载并装配单个插件目录：`LoadedPlugin::load` → 按 manifest 授面 →
    /// [`Self::js_bridge`] 引擎。这是「目录插件跑起来」的组合根唯一入口。
    pub fn load_js_plugin(&self, dir: &std::path::Path) -> Result<quickjs_bridge::JsBridge, String> {
        let loaded = quickjs_bridge::load_plugin(dir)?;
        let face_set = loaded.manifest.face_set();
        let faces: Vec<&str> = face_set.iter().map(|s| s.as_str()).collect();
        self.js_bridge(&faces)
    }

    fn loop_runtime(&self) -> Arc<LoopRuntime> {
        Arc::new(LoopRuntime {
            // SharedLlm：每回合现读当前装配实现（swap_llm 后下一请求生效，
            // 对所有会话生效——不随会话创建快照旧实现）。
            llm: Arc::new(SharedLlm::new(Arc::clone(&self.llm))),
            store: Arc::clone(&self.store),
            tools: Arc::clone(&self.tools),
            gate: Arc::clone(&self.gate),
            persist: Arc::clone(&self.persist),
            provider: self.provider.clone(),
            model: self.model.clone(),
            max_steps: self.max_steps,
        })
    }
}

/// interrupted-turn 修复：闭合崩溃孤儿回合（对齐 DSH session-persistence 恢复语义）。
///
/// kill -9 可能发生在回合中途（Step Started 已落、Ended 未落，或 Turn Started 已落、
/// Ended 未落）。**不删除任何已落盘事件**——只扫描配对，发现未闭合的 Step/Turn
/// 就在日志尾部追加 closers（Step Ended + `Turn Ended{Interrupted}`），把回合闭合。
/// 这样事件日志作为唯一事实源完整保留（含已闭合错误回合的 requestId 审计事实），
/// 且不越过 Turn Ended 截断——后续已闭合回合的历史永不丢失。
fn repair_interrupted_turn(events: Vec<SessionEvent>) -> Vec<SessionEvent> {
    let mut repaired = events;
    // 正向扫描配对深度：depth>0 说明有未配对 Started（本回合或嵌套残留）。
    let mut step_open: u64 = 0;
    let mut turn_open: u64 = 0;
    let mut last_open_step: Option<(u64, u64)> = None;
    let mut last_open_turn: Option<u64> = None;
    for ev in &repaired {
        match ev {
            SessionEvent::Step {
                turn,
                step,
                phase: StepPhase::Started,
            } => {
                step_open += 1;
                last_open_step = Some((*turn, *step));
            }
            SessionEvent::Step {
                phase: StepPhase::Ended,
                ..
            } => {
                step_open = step_open.saturating_sub(1);
                if step_open == 0 {
                    last_open_step = None;
                }
            }
            SessionEvent::Turn(TurnEvent::Started { turn }) => {
                turn_open += 1;
                last_open_turn = Some(*turn);
            }
            SessionEvent::Turn(TurnEvent::Ended { .. }) => {
                turn_open = turn_open.saturating_sub(1);
                if turn_open == 0 {
                    last_open_turn = None;
                }
            }
            _ => {}
        }
    }
    if let Some((turn, step)) = last_open_step {
        repaired.push(SessionEvent::Step {
            turn,
            step,
            phase: StepPhase::Ended,
        });
    }
    if let Some(turn) = last_open_turn {
        repaired.push(SessionEvent::Turn(TurnEvent::Ended {
            turn,
            reason: TurnEndReason::Interrupted,
        }));
    }
    repaired
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kernel_contracts::session::SessionId;

    fn header(id: &str) -> SessionHeader {
        SessionHeader {
            id: SessionId(id.to_string()),
            app: "test".into(),
            profile: "headless".into(),
            workspace: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn tmp_db(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bm-kernel-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("test.db")
    }

    /// 脚本化工具（echo）：§5.5 真 JS 插件跑通用。
    struct EchoTool;
    impl EchoTool {
        fn new() -> Self {
            Self
        }
    }
    #[async_trait::async_trait]
    impl kernel_contracts::ToolHandler for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echo back"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({ "type": "object", "properties": { "text": { "type": "string" } } })
        }
        async fn execute(
            &self,
            input: kernel_contracts::tools::ToolExecutionInput,
        ) -> Result<kernel_contracts::tools::ToolExecutionResult, kernel_contracts::error::ToolError> {
            let text = input
                .arguments
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            Ok(kernel_contracts::tools::ToolExecutionResult::ok(format!("echo:{text}")))
        }
    }

    #[tokio::test]
    async fn create_restore_roundtrip() {
        let db = tmp_db("roundtrip");
        let rt = Runtime::headless(db.clone()).unwrap();
        let agent = rt.create_session(header("s1")).await.unwrap();
        agent.run_turn(Some("hi")).await.unwrap();
        drop(rt);

        // 重开运行时，从持久化恢复。
        let rt2 = Runtime::headless(db.clone()).unwrap();
        let restored = rt2.restore_session("s1").await.unwrap();
        let events = restored.session().events();
        // 空脚本 LLM：SessionStarted + User + Turn Started + Step/S + AssistantChunk(Finish)
        // + AssistantMessage(空) + Step/E + TurnE
        assert_eq!(events.len(), 8);
        // 恢复后的会话可继续跑（turn 编号接续，不重复）。
        let outcome = restored.run_turn(Some("again")).await.unwrap();
        assert!(outcome.steps >= 1);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn repair_closes_open_step_and_turn_tail() {
        // 未配对 Step Started（kill-9 于 step 中）：追加 closers 闭合，
        // 历史事件（SessionStarted/UserMessage/Turn Started）全部保留。
        let v = vec![
            SessionEvent::SessionStarted {
                header: header("x"),
            },
            SessionEvent::UserMessage { text: "hi".into() },
            SessionEvent::Turn(TurnEvent::Started { turn: 1 }),
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Started,
            },
        ];
        let r = repair_interrupted_turn(v);
        assert_eq!(r.len(), 6);
        assert!(matches!(
            r[4],
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Ended
            }
        ));
        assert!(matches!(
            r[5],
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Interrupted
            })
        ));
    }

    #[test]
    fn repair_keeps_closed_history_and_closes_only_tail() {
        // 完整闭合的 turn 1 + kill-9 于 turn 2 中途：只闭合 turn 2 尾部，
        // turn 1 的 Turn Ended（含审计事实）原样保留——不越过 Turn Ended 截断。
        let v = vec![
            SessionEvent::SessionStarted {
                header: header("x"),
            },
            SessionEvent::UserMessage { text: "hi".into() },
            SessionEvent::Turn(TurnEvent::Started { turn: 1 }),
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Started,
            },
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Ended,
            },
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Error {
                    message: "boom".into(),
                    code: "E".into(),
                    request_id: Some("req-1".into()),
                },
            }),
            SessionEvent::UserMessage { text: "again".into() },
            SessionEvent::Turn(TurnEvent::Started { turn: 2 }),
            SessionEvent::Step {
                turn: 2,
                step: 1,
                phase: StepPhase::Started,
            },
        ];
        let r = repair_interrupted_turn(v);
        assert_eq!(r.len(), 11);
        // turn 1 的闭合回合（含 requestId）保留。
        assert!(matches!(
            &r[5],
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Error {
                    code,
                    request_id: Some(rid),
                    ..
                }
            }) if code == "E" && rid == "req-1"
        ));
        // 尾部 = closers（Step Ended + Turn Ended{Interrupted}）。
        assert!(matches!(
            r[9],
            SessionEvent::Step {
                turn: 2,
                step: 1,
                phase: StepPhase::Ended
            }
        ));
        assert!(matches!(
            r[10],
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 2,
                reason: TurnEndReason::Interrupted
            })
        ));
    }

    /// 取消/报错回合（已闭合，含 requestId）→ 用户再发消息 → kill-9 于新回合中途 →
    /// restore：错误回合历史完整保留，中断回合被 closers 闭合（回归 P1-2/P1-3：
    /// 旧实现会在未配对 Step Started 处整段截断，删掉错误回合及后续全部历史）。
    #[tokio::test]
    async fn restore_after_closed_error_turn_preserves_history() {
        let db = tmp_db("cancel-history");
        let rt = Runtime::headless(db.clone()).unwrap();
        let agent = rt.create_session(header("s1")).await.unwrap();
        // t0 记录写入窗口上界：恢复必须沿用落盘时间戳，不得在恢复时刻重造。
        let write_deadline = chrono::Utc::now();
        // 手工构造：turn 1 错误闭合（Step 配对 + Turn Ended Error 带 requestId），
        // turn 2 中断于 Step Started（模拟 kill-9 于取消后新回合中途）。
        let seq = [
            SessionEvent::Turn(TurnEvent::Started { turn: 1 }),
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Started,
            },
            SessionEvent::Step {
                turn: 1,
                step: 1,
                phase: StepPhase::Ended,
            },
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Error {
                    message: "boom".into(),
                    code: "E".into(),
                    request_id: Some("req-1".into()),
                },
            }),
            SessionEvent::Turn(TurnEvent::Started { turn: 2 }),
            SessionEvent::Step {
                turn: 2,
                step: 1,
                phase: StepPhase::Started,
            },
        ];
        for e in seq {
            let rec = agent.session().append(e);
            rt.persist
                .append_events("s1", std::slice::from_ref(&rec.event))
                .await
                .unwrap();
        }
        drop(rt);

        let rt2 = Runtime::headless(db.clone()).unwrap();
        let restored = rt2.restore_session("s1").await.unwrap();
        let events: Vec<kernel_contracts::session::SessionEvent> = restored
            .session()
            .events()
            .into_iter()
            .map(|r| r.event)
            .collect();
        // 错误回合的审计事实（requestId）不丢。
        assert!(events.iter().any(|e| matches!(
            e,
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 1,
                reason: TurnEndReason::Error {
                    code,
                    request_id: Some(rid),
                    ..
                }
            }) if code == "E" && rid == "req-1"
        )));
        // 中断回合被 closers 闭合。
        assert!(events.iter().any(|e| matches!(
            e,
            SessionEvent::Turn(TurnEvent::Ended {
                turn: 2,
                reason: TurnEndReason::Interrupted
            })
        )));
        // 恢复后 turn 编号接续（下一次 run_turn 应开 turn 3）。
        let outcome = restored.run_turn(Some("again")).await.unwrap();
        assert!(outcome.steps >= 1);
        let starts: Vec<u64> = restored
            .session()
            .events()
            .into_iter()
            .filter_map(|r| match r.event {
                SessionEvent::Turn(TurnEvent::Started { turn }) => Some(turn),
                _ => None,
            })
            .collect();
        assert_eq!(starts, vec![1, 2, 3]);
        // #7 时间戳保真：首条 SessionStarted 的时间戳是写入时刻（≤ write_deadline），
        // 不是恢复时刻重造（旧实现 SessionRecord::new 会打上恢复时间 > write_deadline）。
        let first_ts = restored.session().events()[0].timestamp;
        assert!(
            first_ts <= write_deadline,
            "timestamp must be preserved from disk, got {first_ts} after deadline {write_deadline}"
        );
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[tokio::test]
    async fn plugin_runtime_fails_loud() {
        let rt = Runtime::headless(tmp_db("plugin")).unwrap();
        assert_eq!(
            rt.plugin_availability(),
            PluginRuntimeAvailability::Unavailable {
                reason: "plugin runtime is not registered in this delivery profile".into()
            }
        );
    }

    // ---------- QuickJS 桥装配（§5.4 接真 LLM） ----------

    #[test]
    fn js_bridge_wires_headless_llm_into_host() {
        // headless（ScriptLlm mock）→ js_bridge → JS host.llm.complete 走同一端口。
        let db = tmp_db("js-bridge");
        let rt = Runtime::headless(db.clone()).unwrap();
        let bridge = rt.js_bridge(&["llm.complete"]).expect("js bridge");
        bridge
            .exec(r#"globalThis.__call = async (req) => host.llm.complete(req);"#)
            .unwrap();
        let r = bridge
            .call_async(
                "__call",
                &[serde_json::json!({
                    "provider": "mock", "model": "mock-1",
                    "messages": [{ "role": "user", "content": "hi" }],
                })],
            )
            .unwrap();
        // 空脚本 LLM：finish stop 终态（torn 纪律兜底）。
        assert_eq!(r["ok"], serde_json::json!("true"));
        assert_eq!(r["value"]["chunks"][0]["type"], serde_json::json!("finish"));
        assert_eq!(r["value"]["chunks"][0]["reason"], serde_json::json!("stop"));
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn js_bridge_uses_swapped_llm() {
        // swap_llm 换装真 provider 形状的 LLM 后，js_bridge 同一装配入口
        // 让 JS 插件吃到新实现——桥层对 mock/真 provider 无感。
        let db = tmp_db("js-bridge-swap");
        let rt = Runtime::headless(db.clone()).unwrap();
        rt.swap_llm(scripted_llm(
            "minimax".to_string(),
            "MiniMax-M3".to_string(),
            vec![plugin_llm::MockTurn::Text("hi from script".to_string())],
        ));
        let bridge = rt.js_bridge(&["llm.complete"]).expect("js bridge");
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
        assert_eq!(r["ok"], serde_json::json!("true"));
        assert_eq!(r["value"]["chunks"][0]["type"], serde_json::json!("text-delta"));
        assert_eq!(r["value"]["chunks"][0]["text"], serde_json::json!("hi from script"));
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    // ---------- §5.5 真 JS 插件跑通（LoadedPlugin + Runtime::js_bridge 组合） ----------

    #[test]
    fn js_plugin_load_and_run_full_tool_llm_flow() {
        // 目录插件（plugin.json + main.js）→ LoadedPlugin → js_bridge 按 manifest
        // 授面 → JS 内 await host.llm.complete + host.tools.invoke + host.session
        // 全链路跑通（工具经 ToolGate fail-closed、会话拉模型投影）。
        // 注意：JsBridge 内部 block_on 自带 runtime，桥调用必须脱离 tokio 测试
        // 上下文（#[tokio::test] 里调会 "Cannot start a runtime from within a runtime"）。
        let db = tmp_db("js-plugin");
        let rt = Runtime::headless(db.clone()).unwrap();
        // 注册一个工具并启用（echo）。
        let echo = EchoTool::new();
        rt.register_tool(Arc::new(echo)).unwrap();
        rt.gate.enable("echo");
        // 插件要 append 的会话：先创建（headless 内存 store）。
        let _agent = futures::executor::block_on(rt.create_session(header("s1"))).unwrap();

        // 装配工具脚本 LLM：文本回合 "hello plugin"。
        rt.swap_llm(scripted_llm(
            "mock".to_string(),
            "mock-1".to_string(),
            vec![plugin_llm::MockTurn::Text("hello plugin".to_string())],
        ));

        // 构造临时插件目录。
        let dir = std::env::temp_dir().join(format!("qjs-plugin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "demo",
                "name": "Demo",
                "version": "0.1.0",
                "entry": "main.js",
                "host": ["log", "llm.complete", "tools.list", "tools.invoke", "session.get", "session.append"]
            }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("main.js"),
            r#"
            globalThis.__main = async () => {
                const list = host.tools.list();
                const toolNames = list.value.map(t => t.name);
                const llm = await host.llm.complete({
                    provider: "mock", model: "mock-1",
                    messages: [{ role: "user", content: "hi" }],
                });
                const echo = await host.tools.invoke("echo", { text: "ping" });
                host.session.append("s1", { UserMessage: { text: "from plugin" } });
                const s = host.session.get("s1");
                return { tools: toolNames, llmText: llm.value.chunks[0].text, echo: echo.value.output, sessionCursor: s.value.cursor };
            };
            "#,
        )
        .unwrap();

        let loaded = quickjs_bridge::LoadedPlugin::load(&dir).unwrap();
        let face_set = loaded.manifest.face_set();
        let faces: Vec<&str> = face_set.iter().map(|s| s.as_str()).collect();
        let bridge = rt.js_bridge(&faces).expect("js bridge");
        bridge
            .exec(&loaded.entry_source)
            .expect("load plugin entry");
        let r = bridge
            .call_async("__main", &[])
            .expect("run plugin main");
        // llm.complete（真端口块流）→ "hello plugin"。
        assert_eq!(r["llmText"], serde_json::json!("hello plugin"));
        // tools.list（经 ToolGate）→ 只有 echo。
        assert_eq!(r["tools"], serde_json::json!(["echo"]));
        // tools.invoke → echo:ping。
        assert_eq!(r["echo"], serde_json::json!("echo:ping"));
        // session.append/get（拉模型投影）→ cursor 2（SessionStarted + UserMessage）。
        assert_eq!(r["sessionCursor"], serde_json::json!(2));

        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn js_plugin_unlisted_face_is_undefined() {
        // manifest 最小权限：未声明 host.tools.invoke 的插件在 JS 里访问
        // host.tools.invoke 必须 undefined（§5.3 授面纪律在装配层生效）。
        let db = tmp_db("js-plugin-lp");
        let rt = Runtime::headless(db.clone()).unwrap();
        let dir = std::env::temp_dir().join(format!("qjs-plugin-lp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"id":"lp","name":"LP","entry":"main.js","host":["log","tools.list"]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("main.js"), r#"globalThis.__t = typeof host.tools.invoke;"#)
            .unwrap();
        let loaded = quickjs_bridge::LoadedPlugin::load(&dir).unwrap();
        let face_set = loaded.manifest.face_set();
        let faces: Vec<&str> = face_set.iter().map(|s| s.as_str()).collect();
        let bridge = rt.js_bridge(&faces).expect("js bridge");
        bridge.exec(&loaded.entry_source).unwrap();
        let r = bridge.eval_value("globalThis.__t").unwrap();
        assert_eq!(r, serde_json::json!("undefined"));
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    // ---------- §5.6 目录插件注册表 + config 面 ----------

    #[test]
    fn load_js_plugin_end_to_end() {
        // 组合根唯一入口 load_js_plugin：目录插件 → manifest 授面 → 引擎 →
        // JS 调 llm/tools/config 全链路。config 面白名单注入 + 未命中诚实报错。
        let db = tmp_db("js-plugin-e2e");
        let rt = Runtime::headless(db.clone()).unwrap();
        rt.register_tool(Arc::new(EchoTool::new())).unwrap();
        rt.gate.enable("echo");
        rt.swap_llm(scripted_llm(
            "mock".to_string(),
            "mock-1".to_string(),
            vec![plugin_llm::MockTurn::Text("e2e".to_string())],
        ));

        let dir = std::env::temp_dir().join(format!("qjs-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{
                "id": "e2e",
                "name": "E2E",
                "entry": "main.js",
                "host": ["log", "config.get", "llm.complete", "tools.invoke"]
            }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("main.js"),
            r#"
            globalThis.__main = async () => {
                const cfg = host.config.get("e2e", "url");
                const llm = await host.llm.complete({
                    provider: "mock", model: "mock-1",
                    messages: [{ role: "user", content: "hi" }],
                });
                const echo = await host.tools.invoke("echo", { text: "x" });
                const miss = host.config.get("e2e", "secret");
                return { cfg: cfg.ok === "true" ? cfg.value : null, llm: llm.value.chunks[0].text, echo: echo.value.output, miss: miss.ok === "false" ? miss.err.code : null };
            };
            "#,
        )
        .unwrap();

        let loaded = quickjs_bridge::load_plugin(&dir).unwrap();
        let face_set = loaded.manifest.face_set();
        let faces: Vec<&str> = face_set.iter().map(|s| s.as_str()).collect();
        let mut config = std::collections::HashMap::new();
        config.insert("e2e.url".to_string(), "https://e2e".to_string());
        let bridge = rt.js_bridge_with_config(&faces, config).expect("js bridge");
        bridge.exec(&loaded.entry_source).unwrap();
        let r = bridge.call_async("__main", &[]).unwrap();
        assert_eq!(r["cfg"], serde_json::json!("https://e2e"));
        assert_eq!(r["llm"], serde_json::json!("e2e"));
        assert_eq!(r["echo"], serde_json::json!("echo:x"));
        // secret 不在白名单 → config-not-found（host 层白名单即全部内容）。
        assert_eq!(r["miss"], serde_json::json!("config-not-found"));
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    #[test]
    fn scan_js_plugins_finds_all() {
        // Runtime::scan_js_plugins：目录扫描 → 全清单（含嵌套、跳过非插件目录）。
        let db = tmp_db("js-plugin-scan");
        let rt = Runtime::headless(db.clone()).unwrap();
        let root = std::env::temp_dir().join(format!("qjs-scan-{}", uuid::Uuid::new_v4()));
        let mk = |sub: &str, id: &str| {
            let d = root.join(sub);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("plugin.json"),
                format!(r#"{{"id":"{id}","name":"{id}","entry":"main.js","host":["log"]}}"#),
            )
            .unwrap();
            std::fs::write(d.join("main.js"), "host.log('info','x');").unwrap();
        };
        mk("a", "a");
        mk("sub/b", "b");
        std::fs::create_dir_all(root.join("sub").join("plain")).unwrap();
        std::fs::write(root.join("sub").join("plain").join("main.js"), "x").unwrap();
        let plugins = rt.scan_js_plugins(&root).unwrap();
        let ids: Vec<&str> = plugins.iter().map(|p| p.manifest.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]); // 字典序
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }
}
