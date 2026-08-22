//! RPC 方法分派（契约台账 §2）：52 方法表中的聊天闭环子集。
//! 当前实现子集：host.describe / session.{list,create,history,prompt,cancel,rename} /
//! llm.{providers,models} / workspace.list。其余方法返回 `bad-request`（not implemented 语义
//! 由错误码承载），随 conformance 轮逐步补齐。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bm_assembly::provider::ProviderRuntime;
use bm_assembly::Runtime;
use kernel_contracts::session::SessionEvent;
use kernel_session::AgentPort;
use serde_json::{json, Value};

use crate::rpc::{err, ok};
use crate::rpc_m3::{
    agent_preset_copy, agent_preset_open_document, agent_preset_read, agent_preset_remove,
    agent_preset_select, goal_clear, goal_complete, goal_create, goal_edit, goal_pause,
    goal_resume, host_open_path, session_attachment, session_update_queue, settings_open_document,
    subagent_history, subagent_interrupt, subagent_list, subagent_prompt,
};

// handler 领域子模块（2026-08-20 拆分）：api.rs 保留 AppState/dispatch/会话领域，
// credentials/auth/session/llm/workspace/host/settings/plugins 各自成文件。
mod credentials;
use credentials::*;
mod auth;
use auth::*;
mod llm;
use llm::*;
mod workspace;
use workspace::*;
mod host;
use host::*;
/// re-export：web-server 其他模块（lib.rs 等）经 `api::host_workdir` 取工作目录。
pub use host::host_workdir;
mod settings;
use settings::*;
mod session;
use session::*;

/// 活跃会话句柄。
pub struct SessionHandle {
    /// 会话代理（loop 插件：`swap_loop` 换装后新会话用新实现）。
    pub agent: Arc<dyn AgentPort>,
    pub running: bool,
    pub blank: bool,
    pub title: Option<String>,
    /// per-session 模型选择（session.selectModel 写入；prompt 时同步给 agent）。
    pub selected: Option<(String, String)>,
}

/// 兼容层应用状态。
pub struct AppState {
    pub runtime: Runtime,
    pub sessions: Mutex<HashMap<String, SessionHandle>>,
    pub version: String,
    pub host_cwd: String,
    /// 实时 wire 事件广播（bus → WS/SSE 下行）：payload 已是 WireSessionEvent JSON。
    pub events_tx: tokio::sync::broadcast::Sender<Value>,
    /// host 流事件广播（HostFrame 下行）：(method, payload) 对，host_loop 包帧发送。
    pub host_events_tx: tokio::sync::broadcast::Sender<(String, Value)>,
    /// mux 流额外帧广播（approval/question 重放 + session/projection 等）。
    pub mux_events_tx: tokio::sync::broadcast::Sender<crate::rpc::ServerRequestFrame>,
    /// 信任栅栏的 trustedHosts（部署时 --trusted-host 传入）。
    pub trusted_hosts: Vec<String>,
    /// 设置命名空间内存视图（M2.5：settings.* 写面的最小存储；持久化后置）。
    pub settings: Mutex<HashMap<String, serde_json::Map<String, Value>>>,
    /// 设置命名空间单调 revision（M4 P1-4：settings-conflict 语义——
    /// 写成功 +1；写带 expectedRevision 不匹配 → settings-conflict）。
    pub settings_revisions: Mutex<HashMap<String, u64>>,
    /// 工作区注册表（M2.5 内存态）：workspaceId → WorkspaceView。
    pub workspaces: Mutex<HashMap<String, Value>>,
    /// 归档会话集（workspace.archiveSession 持久集）。
    pub archived_session_ids: Mutex<Vec<String>>,
    /// 凭据存储（credentials.set/unset 写面；内存态，值永不出域，只报 configured）。
    pub credentials: Mutex<HashMap<String, String>>,
    /// settings/credentials 持久化文件（None = 内存态不落盘；Some = 每次写面
    /// 成功后原子写盘，启动时加载恢复——P2-C：重启不再静默丢配置/凭据）。
    pub settings_path: Option<PathBuf>,
    /// 真实 provider 运行时（M3；空 = mock 单 provider 模式）。
    pub providers: Vec<ProviderRuntime>,
    /// respond pending 表（approval 先、question 后；M3 收尾）。
    pub pending: crate::pending::PendingState,
    /// session/projection 槽（契约层注册机制）：(key, (watermark_seq, value))。
    pub projections: Mutex<HashMap<String, (i64, Value)>>,
    /// goal 内存态最小桥（wire 契约在 web-server，自动续跑语义归 goal 插件）。
    pub goals: Mutex<HashMap<String, GoalRecord>>,
    /// 会话日志的附件引用表（session.attachment 语义：日志含 attachmentId 才回）。
    pub attachments: Mutex<HashMap<String, Vec<String>>>,
    /// 审批等待表（approval_id → oneshot 发送端）：工具审批端口登记后，
    /// respond 路由（allowed-once/rejected）经此唤醒正在等待的 loop 调用。
    /// 与 pending.approvals 同生共死（pending 管应答帧，这里管执行继续）。
    pub approval_waiters: Mutex<HashMap<String, tokio::sync::oneshot::Sender<bm_ports::ApprovalVerdict>>>,
    /// 目标续跑驱动（`--goal` 装配）：回合完成点检查 active 目标并续跑。
    /// Mutex 承载（AppState 已在 Arc 中时仍可 `&Arc<Self>` 装配）。
    pub goal_driver: Mutex<Option<Arc<crate::goal_driver::GoalDriver>>>,
}

/// goal 记录（对齐 DSH `GoalSnapshot`/`GoalView` 的 wire 形状；web-server 内存态）。
#[derive(Debug, Clone)]
pub struct GoalRecord {
    pub id: String,
    pub revision: u64,
    pub objective: String,
    pub phase: String, // 'active' | 'paused' | 'blocked' | 'complete'
    pub max_goal_rounds: u64,
    pub rounds_started: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl GoalRecord {
    /// wire 投影值（`GoalProjection`：goal snapshot + roundsStarted + createdAt/updatedAt）。
    pub fn projection(&self) -> Value {
        json!({
            "goal": {
                "id": self.id,
                "revision": self.revision,
                "objective": self.objective,
                "phase": self.phase,
                "maxGoalRounds": self.max_goal_rounds,
            },
            "roundsStarted": self.rounds_started,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

impl AppState {
    pub fn new(runtime: Runtime) -> Self {
        Self::with_trusted_hosts(runtime, vec![])
    }

    pub fn with_trusted_hosts(runtime: Runtime, trusted_hosts: Vec<String>) -> Self {
        Self::assemble(runtime, trusted_hosts, vec![])
    }

    /// 装配宿主文件工具插件（核心）：注册 host.* 工具 + 注入 workdir 源
    /// （从 settings host.workdir 现读——设置页改工作目录后工具即时生效）。
    /// 经 bm-assembly 组合根装配（L0 不直接依赖 plugin-host-tools，守卫规则 2）。
    pub fn install_host_tools(self: &Arc<Self>) {
        let workdir: Arc<dyn bm_ports::WorkdirPort> =
            Arc::new(crate::api::host::SettingsWorkdir::new(Arc::clone(self)));
        self.runtime.install_host_tools(workdir);
    }

    /// 装配代码执行沙箱插件（功能，`--code-runtime`）：注册 code.* 工具 + 注入
    /// workdir 源（与 host.* 同一事实源 settings host.workdir）。经 bm-assembly
    /// 组合根装配（L0 不直接依赖 plugin-code-runtime，守卫规则 2）。
    pub fn install_code_runtime(self: &Arc<Self>) {
        let workdir: Arc<dyn bm_ports::WorkdirPort> =
            Arc::new(crate::api::host::SettingsWorkdir::new(Arc::clone(self)));
        self.runtime.install_code_runtime(workdir);
    }

    /// 装配定时任务调度器（功能，`--schedule`）：创建 Scheduler（实现
    /// `SchedulePort`）→ 经 bm-assembly `install_schedule`（注入 plugin-schedule
    /// 全局源 + 注册/启用工具）→ 启动后台驱动循环。
    pub fn install_scheduler(self: &Arc<Self>) -> Arc<crate::scheduler::Scheduler> {
        let sched = Arc::new(crate::scheduler::Scheduler::new(Arc::clone(self)));
        let port: Arc<dyn bm_ports::SchedulePort> = Arc::clone(&sched) as Arc<dyn bm_ports::SchedulePort>;
        self.runtime.install_schedule(port);
        sched.start();
        sched
    }

    /// 装配目标管理工具插件（功能，`--goal`）：创建 GoalRouter（实现 `GoalPort`，
    /// 对接现有 goal RPC 状态机）→ 经 bm-assembly `install_goal`（注入 plugin-goal
    /// 全局源 + 注册/启用 goal.* 工具）。goal-round-driver（同会话续跑）由
    /// [`AppState::start_goal_driver`] 独立装配。
    pub fn install_goal(self: &Arc<Self>) -> Arc<crate::goal::GoalRouter> {
        let router = Arc::new(crate::goal::GoalRouter::new(Arc::clone(self)));
        let port: Arc<dyn bm_ports::GoalPort> = Arc::clone(&router) as Arc<dyn bm_ports::GoalPort>;
        self.runtime.install_goal(port);
        router
    }

    /// 装配目标续跑驱动（`--goal` 的一部分）：把 GoalDriver 挂进 state，
    /// 之后 session_prompt 回合完成点经 `goal_driver.maybe_continue(sid)` 续跑。
    pub fn start_goal_driver(self: &Arc<Self>) {
        let driver = Arc::new(crate::goal_driver::GoalDriver::new(Arc::clone(self)));
        *self.goal_driver.lock().unwrap() = Some(driver);
    }

    /// 装配上下文压缩（`--compact`）为运行时可调语义：没有显式 settings.compaction
    /// 记录时，用 config.toml `[compaction]` 段（缺省回落默认策略 0.5/10%/4000/512）
    /// **种入** settings 作为初始值，再装配 [`crate::compaction::SettingsBackedCompactor`]
    /// （每次 maybe_compact 现场锁读——设置页改参数后下一回合即生效）。
    /// 语义升级：config 段的 `enabled=false` 否决权保留（种入 enabled=false）。
    pub fn install_compactor(self: &Arc<Self>, cfg: Option<&bm_assembly::config::CompactionConfig>) {
        let mut settings = self.settings.lock().unwrap();
        let compaction = settings.get_mut("compaction");
        let empty = compaction.is_none() || compaction.map(|m| m.is_empty() || !m.contains_key("watermark")).unwrap_or(true);
        if empty {
            let def = bm_assembly::DefaultCompactor::default();
            let mut seed = serde_json::Map::new();
            seed.insert("enabled".into(), serde_json::json!(cfg.and_then(|c| c.enabled).unwrap_or(true)));
            seed.insert(
                "watermark".into(),
                serde_json::json!(cfg.and_then(|c| c.watermark).unwrap_or(def.watermark)),
            );
            seed.insert(
                "keepRecentRatio".into(),
                serde_json::json!(cfg.and_then(|c| c.keep_recent_ratio).unwrap_or(def.keep_recent_ratio)),
            );
            seed.insert(
                "keepRecentFloor".into(),
                serde_json::json!(cfg.and_then(|c| c.keep_recent_floor).unwrap_or(def.keep_recent_floor)),
            );
            seed.insert(
                "minMiddleTokens".into(),
                serde_json::json!(cfg.and_then(|c| c.min_middle_tokens).unwrap_or(def.min_middle_tokens)),
            );
            settings.insert("compaction".to_string(), seed);
        }
        drop(settings);
        // 装配 settings-backed 实现（RwLock 换装，&self——与 approval 装配同构）。
        let compactor: Arc<dyn bm_ports::Compactor> =
            Arc::new(crate::compaction::SettingsBackedCompactor::new(Arc::clone(self)));
        self.runtime.install_compactor(compactor);
        tracing::info!("context compactor plugin installed (settings-backed, live-updatable)");
    }

    pub fn assemble(
        runtime: Runtime,
        trusted_hosts: Vec<String>,
        providers: Vec<ProviderRuntime>,
    ) -> Self {
        Self::assemble_with_settings_path(runtime, trusted_hosts, providers, None)
    }

    /// 带 settings 持久化文件的装配（main.rs 传 home 下的 settings.json；
    /// 测试/无 home 场景传 None 保持内存态）。
    pub fn assemble_with_settings_path(
        runtime: Runtime,
        trusted_hosts: Vec<String>,
        providers: Vec<ProviderRuntime>,
        settings_path: Option<PathBuf>,
    ) -> Self {
        let host_cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let (events_tx, _rx) = tokio::sync::broadcast::channel(256);
        let (host_events_tx, _hrx) = tokio::sync::broadcast::channel(256);
        let (mux_events_tx, _mrx) = tokio::sync::broadcast::channel(256);
        let state = Self {
            runtime,
            sessions: Mutex::new(HashMap::new()),
            version: "0.1.0".to_string(),
            host_cwd,
            events_tx,
            host_events_tx,
            mux_events_tx,
            trusted_hosts,
            settings: Mutex::new(HashMap::new()),
            settings_revisions: Mutex::new(HashMap::new()),
            workspaces: Mutex::new(HashMap::new()),
            archived_session_ids: Mutex::new(Vec::new()),
            credentials: Mutex::new(HashMap::new()),
            providers,
            pending: crate::pending::PendingState::new(),
            projections: Mutex::new(HashMap::new()),
            goals: Mutex::new(HashMap::new()),
            attachments: Mutex::new(HashMap::new()),
            approval_waiters: Mutex::new(HashMap::new()),
            goal_driver: Mutex::new(None),
            settings_path,
        };
        state.load_settings_file();
        state
    }

    /// 把 settings/credentials 快照原子写盘（tmp + rename；Unix 0600）。
    /// 固定锁序 settings → revisions → credentials；所有写路径先释放各自锁
    /// 再调用本方法，无交叉等待。写盘失败仅记录（内存已生效，不阻断请求）。
    pub fn persist_settings(&self) {
        let Some(path) = &self.settings_path else {
            return;
        };
        let snapshot = {
            let settings = self.settings.lock().unwrap();
            let revisions = self.settings_revisions.lock().unwrap();
            let credentials = self.credentials.lock().unwrap();
            let mut s = serde_json::Map::new();
            for (ns, value) in settings.iter() {
                let rev = revisions.get(ns).copied().unwrap_or(0);
                s.insert(
                    ns.clone(),
                    json!({ "value": value, "revision": rev }),
                );
            }
            let creds: serde_json::Map<String, Value> = credentials
                .iter()
                .map(|(k, v)| (k.clone(), json!(v)))
                .collect();
            json!({ "settings": s, "credentials": creds })
        };
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        let write_res = (|| -> std::io::Result<()> {
            std::fs::write(&tmp, serde_json::to_string_pretty(&snapshot).unwrap_or_default())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
            }
            std::fs::rename(&tmp, path)
        })();
        if let Err(e) = write_res {
            tracing::error!("settings persist failed: {e}");
        }
    }

    /// 启动加载 settings 文件（损坏/缺失 → 从空开始并记录；恢复后同步
    /// provider 覆盖——凭据回填 adapter，baseURL 覆盖重放）。
    pub fn load_settings_file(&self) {
        let Some(path) = &self.settings_path else {
            return;
        };
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return, // 首次启动无文件
        };
        let parsed: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("settings file parse failed, starting empty: {e}");
                return;
            }
        };
        if let Some(s) = parsed.get("settings").and_then(Value::as_object) {
            {
                let mut settings = self.settings.lock().unwrap();
                let mut revisions = self.settings_revisions.lock().unwrap();
                for (ns, entry) in s {
                    let rev = entry.get("revision").and_then(Value::as_u64).unwrap_or(0);
                    let value = entry
                        .get("value")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    settings.insert(ns.clone(), value);
                    revisions.insert(ns.clone(), rev);
                }
            }
            for (ns, entry) in s {
                if let Some(value) = entry.get("value").and_then(Value::as_object) {
                    sync_provider_overrides(self, ns, value);
                }
            }
        }
        if let Some(creds) = parsed.get("credentials").and_then(Value::as_object) {
            {
                let mut store = self.credentials.lock().unwrap();
                for (k, v) in creds {
                    if let Some(s) = v.as_str() {
                        store.insert(k.clone(), s.to_string());
                    }
                }
            }
            for k in creds.keys() {
                let value = self.credentials.lock().unwrap().get(k).cloned();
                sync_provider_key_override(self, k, value);
            }
        }
    }

    /// 广播一帧 host 事件（HostFrame 下行）。调用方自行构造 payload 全形。
    pub fn broadcast_host(&self, method: impl Into<String>, payload: Value) {
        let _ = self.host_events_tx.send((method.into(), payload));
    }

    /// 广播一帧 mux 事件（MuxFrame 下行；approval/question 重放与 session/projection）。
    pub fn broadcast_mux_frame(
        &self,
        rpc_id: impl Into<String>,
        method: impl Into<String>,
        payload: Value,
    ) {
        let frame = crate::rpc::ServerRequestFrame::new(rpc_id, method, payload);
        let _ = self.mux_events_tx.send(frame);
    }

    /// 全部投影单元的当前值（key → value；seq 由单元持有，快照不带）。
    pub fn projection_snapshot(&self) -> serde_json::Map<String, Value> {
        let mut map = serde_json::Map::new();
        for (k, (_seq, v)) in self.projections.lock().unwrap().iter() {
            map.insert(k.clone(), v.clone());
        }
        map
    }

    /// 写一个投影单元（key, seq 单调递增，客户端 higher-seq-wins）并广播 session/projection 帧。
    pub fn write_projection(&self, session_id: &str, key: &str, value: Value) {
        let mut proj = self.projections.lock().unwrap();
        let (seq, _) = proj.get(key).cloned().unwrap_or((0, Value::Null));
        let next_seq = seq + 1;
        proj.insert(key.to_string(), (next_seq, value.clone()));
        drop(proj);
        self.broadcast_mux_frame(
            uuid::Uuid::new_v4().to_string(),
            "session/projection",
            json!({
                "sessionId": session_id,
                "key": key,
                "value": value,
                "seq": next_seq,
            }),
        );
    }

    /// 全量 workspace 快照（workspace-changed 帧的 value 形态）。
    pub fn workspace_snapshot(&self) -> Value {
        let mut items: Vec<Value> = self.workspaces.lock().unwrap().values().cloned().collect();
        items.sort_by_key(|w| {
            w.get("createdAt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        });
        let archived = self.archived_session_ids.lock().unwrap().clone();
        json!({ "items": items, "archivedSessionIds": archived })
    }

    /// 把 kernel 事件总线接到实时下行通道（幂等：仅调用一次）。
    /// bus listener 是同步闭包：按会话维护翻译游标 + wire seq 累计（每会话从 0 连续，
    /// 与 translate_events 一致；SessionStarted 不翻译但占 record.seq，故不能用 record.seq-1）。
    /// 预填：启动恢复过的会话按历史 wire 长度播种游标——重启后实时 seq 从历史尾部
    /// 接续，不回退到 0 撞历史基线（回归 BUG-002：前端按水位单调去重会丢弃恢复后事件）。
    /// 返回的 Disposer 必须持有（drop 即注销），调用方负责保活到进程结束。
    /// 必须在启动恢复循环（live 表填完）之后调用。
    pub fn attach_event_bus(&self) -> kernel_contracts::bus::Disposer {
        let tx = self.events_tx.clone();
        let per_session: std::sync::Mutex<
            HashMap<String, (crate::events::EventTranslator, i64)>,
        > = std::sync::Mutex::new(HashMap::new());
        // 预填种子：live 表里已恢复会话的历史 wire 长度（events() 含修复后的完整日志）。
        {
            let sessions = self.sessions.lock().unwrap();
            let mut table = per_session.lock().unwrap();
            for (sid, h) in sessions.iter() {
                let history: Vec<SessionEvent> =
                    h.agent.session().events().into_iter().map(|r| r.event).collect();
                let seed = crate::events::translate_events(&history).len() as i64;
                table.insert(
                    sid.clone(),
                    (crate::events::EventTranslator::with_emitted(seed), seed),
                );
            }
        }
        let listener = move |record: &kernel_contracts::SessionRecord| {
            let mut table = per_session.lock().unwrap();
            let (trans, seq) = table
                .entry(record.session_id.as_str().to_string())
                .or_insert_with(|| (crate::events::EventTranslator::with_emitted(0), 0));
            if let Some(mut wire) = trans.translate_one(&record.event) {
                wire.seq = *seq;
                *seq += 1;
                wire.time = record.timestamp.timestamp_millis();
                let payload = json!({
                    "sessionId": record.session_id.as_str(),
                    "event": wire,
                });
                let _ = tx.send(payload);
            }
        };
        self.runtime.bus.on_event(listener)
    }
}

/// 分派入口：method 与路径端点不一致时由调用方先判 bad-request（fetch/handler 语义）。
///
/// `token` = `x-boenmind-session` 请求头（认证插件 `--auth` 装配后生效）。
/// 认证门控：装配了 AuthPort 时，除 [`auth_methods`] 外全部 RPC 需有效会话
/// （`auth-required` 拒绝）；未装配（无 --auth）= 旧行为（不门控）。
pub async fn dispatch(state: &Arc<AppState>, method: &str, payload: Value, token: Option<&str>) -> Value {
    // 认证门控（fail-closed）：装配了认证插件才启用；未装配放行（旧行为）。
    if let Some(auth) = &state.runtime.auth {
        let is_auth_method = auth_methods().contains(&method);
        if !is_auth_method && !auth.is_authenticated(token.unwrap_or("")) {
            return err("auth-required", format!("method '{method}' requires login (--auth)"));
        }
    }
    match method {
        // 认证面（--auth 装配；未装配 auth.* 报 not-available——诚实失败）。
        "auth.status" => auth_status(state, token).await,
        "auth.login" => auth_login(state, payload).await,
        "auth.logout" => auth_logout(state, token).await,
        "auth.changePassword" => auth_change_password(state, payload, token).await,
        "host.describe" => host_describe(state),
        "session.list" => session_list(state).await,
        "session.create" => session_create(state, payload).await,
        "session.history" => session_history(state, payload).await,
        "session.search" => session_search(state, payload).await,
        "session.fork" => session_fork(state, payload).await,
        "session.prompt" => session_prompt(state, payload).await,
        "session.cancel" => session_cancel(state, payload),
        "session.rename" => session_rename(state, payload),
        "session.delete" => session_delete(state, payload).await,
        "session.models" => session_models(state, payload),
        "session.selectModel" => session_select_model(state, payload),
        "llm.providers" => llm_providers(state).await,
        "llm.models" => llm_models(state).await,
        "llm.discoverModels" => llm_discover_models(state, payload).await,
        // 核心插件清单（llm / loop / tools，category=Core）：插件管理员按类
        // 分组/隐藏的数据源（当前仅核心三件；Feature 插件随功能面扩展追加）。
        "plugin.core.list" => plugin_core_list(state),
        // JS 插件管理面（--plugins-dir 装配；未装配 fail-loud）：
        // list = 已装配插件清单；invoke = 执行插件主函数（__main）。
        "plugin.js.list" => plugin_js_list(state),
        "plugin.js.invoke" => plugin_js_invoke(state, payload).await,
        "workspace.list" => workspace_list(state),
        "workspace.create" => workspace_create(state, payload),
        "workspace.rename" => workspace_rename(state, payload),
        "workspace.delete" => workspace_delete(state, payload),
        "workspace.insertBefore" => workspace_insert_before(state, payload),
        "workspace.insertSessionBefore" => workspace_insert_session_before(state, payload),
        "workspace.archiveSession" => workspace_archive_session(state, payload).await,
        "agentPreset.list" => agent_preset_list(),
        "skill.list" => skill_list(payload),
        "host.listDirectory" => host_list_directory(payload),
        "host.createDirectory" => host_create_directory(payload),
        // M2.5：无 OS 目录选择对话框，pickDirectory 返回服务 cwd 作默认工作目录
        // （契约形状 {path: string|null} 对齐；null=用户取消）。
        "host.pickDirectory" => host_pick_directory(state),
        // 工作目录作用域文件面（文件管理器窗口单元；settings host.workdir 为事实源）：
        // 全部经 host_fs::resolve_in_workdir 约束在 workdir 内，绕过一律拒绝。
        "host.listWorkdir" => host_list_workdir(state, payload),
        "host.readFile" => host_read_file(state, payload),
        "host.writeFile" => host_write_file(state, payload),
        "host.createWorkdirDirectory" => host_create_workdir_directory(state, payload),
        "settings.describe" => settings_describe(state),
        "settings.update" => settings_update(state, payload),
        "settings.replace" => settings_replace(state, payload),
        "settings.mutate" => settings_mutate(state, payload),
        "credentials.describe" => credentials_describe(state, payload),
        "credentials.set" => credentials_set(state, payload),
        "credentials.unset" => credentials_unset(state, payload),
        "session.attachment" => session_attachment(state, payload),
        "session.updateQueue" => session_update_queue(state, payload),
        "settings.openDocument" => settings_open_document(),
        "host.openPath" => host_open_path(payload),
        "goal.create" => goal_create(state, payload),
        "goal.edit" => goal_edit(state, payload),
        "goal.pause" => goal_pause(state, payload),
        "goal.resume" => goal_resume(state, payload),
        "goal.complete" => goal_complete(state, payload),
        "goal.clear" => goal_clear(state, payload),
        "subagent.list" => subagent_list(state, payload),
        "subagent.history" => subagent_history(state, payload),
        "subagent.prompt" => subagent_prompt(state, payload),
        "subagent.interrupt" => subagent_interrupt(state, payload),
        "agentPreset.select" => agent_preset_select(state, payload),
        "agentPreset.read" => agent_preset_read(state, payload),
        "agentPreset.copy" => agent_preset_copy(state, payload),
        "agentPreset.openDocument" => agent_preset_open_document(state, payload),
        "agentPreset.remove" => agent_preset_remove(state, payload),
        // 测试钩子（仅 BM_TEST_HOOKS=1 时可用）：注入 pending 条目走 respond 全链路。
        "_test.registerApproval" | "_test.registerQuestion" if test_hooks_enabled() => {
            test_register_pending(state, method, payload)
        }
        _ => err(
            "bad-request",
            format!("method \"{method}\" is not implemented by this server"),
        ),
    }
}

/// 测试钩子开关：`BM_TEST_HOOKS=1`（生产缺省关闭，绝不暴露管理面）。
fn test_hooks_enabled() -> bool {
    std::env::var("BM_TEST_HOOKS").map(|v| v == "1").unwrap_or(false)
}

/// 注入 pending 条目（respond 全链路验收用；approval/question 表逐字登记）。
fn test_register_pending(state: &AppState, method: &str, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let rpc_id = payload
        .get("rpcId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let rpc_id = if rpc_id.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        rpc_id
    };
    let mut reg = state.pending.lock();
    if method == "_test.registerApproval" {
        let Some(approval_id) = payload.get("approvalId").and_then(Value::as_str) else {
            return err("bad-request", "missing approvalId");
        };
        let tool_name = payload
            .get("toolName")
            .and_then(Value::as_str)
            .unwrap_or("test_tool")
            .to_string();
        reg.register_approval(
            rpc_id.clone(),
            session_id.to_string(),
            approval_id.to_string(),
            tool_name,
            payload.get("callId").and_then(Value::as_str).map(str::to_string),
            payload.get("reason").and_then(Value::as_str).map(str::to_string),
        );
        // 测试钩子补齐广播（对齐 ApprovalRouter 的 push 语义）：cast 前端
        // approval/requested 帧，方便豁免全链路验收（豁免消费 mux 帧，不直接调 serve）。
        let frame = reg.approval_frame(reg.approvals.get(&rpc_id).expect("just registered"));
        drop(reg);
        state.broadcast_mux_frame(frame.rpc_id, frame.method, frame.payload);
        return ok(json!({ "rpcId": rpc_id }));
    } else {
        let questions = payload
            .get("questions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        reg.register_question(rpc_id.clone(), session_id.to_string(), questions);
    }
    drop(reg);
    ok(json!({ "rpcId": rpc_id }))
}

/// 核心插件清单：返回 llm / loop / tools 三条清单条目（category=Core）。
/// 形状 `{ "plugins": [{id, category, name, description, version}] }`。
fn plugin_core_list(state: &AppState) -> Value {
    ok(json!({ "plugins": state.runtime.plugin_manifest() }))
}

/// JS 插件清单（--plugins-dir 装配后；未装配 fail-loud 报错，不假空清单）。
/// 形状 `{ "plugins": [{id, name, version, faces}] }`（faces = manifest 声明的
/// host 面子集，供管理面展示最小权限）。
fn plugin_js_list(state: &AppState) -> Value {
    let Some(rt) = state.runtime.js_plugins() else {
        return err("plugin-runtime-unavailable", "no JS plugin runtime (pass --plugins-dir)");
    };
    let plugins: Vec<Value> = rt
        .entries()
        .iter()
        .map(|e| {
            json!({
                "id": e.manifest.id,
                "name": e.manifest.name,
                "version": e.manifest.version,
                "faces": e.manifest.host,
            })
        })
        .collect();
    ok(json!({ "plugins": plugins }))
}

/// 执行 JS 插件主函数（`__main`）。
///
/// 测试纪律（JsBridge 内部 block_on 自带 runtime）：`call` 必须脱离 tokio
/// 上下文执行，经 `spawn_blocking` 派到阻塞线程——不能在 dispatch 的 async
/// 上下文里直接调（"Cannot start a runtime from within a runtime"）。
async fn plugin_js_invoke(state: &AppState, payload: Value) -> Value {
    let Some(rt) = state.runtime.js_plugins_arc() else {
        return err("plugin-runtime-unavailable", "no JS plugin runtime (pass --plugins-dir)");
    };
    let Some(plugin_id) = payload.get("pluginId").and_then(Value::as_str) else {
        return err("bad-request", "missing pluginId");
    };
    let plugin_id = plugin_id.to_string();
    // spawn_blocking：QuickJS 引擎执行（含内部 block_on）必须脱离 tokio 上下文。
    let pid = plugin_id.clone();
    match tokio::task::spawn_blocking(move || rt.call(&pid)).await {
        Ok(Ok(value)) => ok(json!({ "result": value })),
        Ok(Err(e)) => err("plugin-exec-error", format!("js plugin '{plugin_id}' failed: {e}")),
        Err(e) => err("plugin-exec-error", format!("js plugin task panicked: {e}")),
    }
}

fn host_describe(state: &AppState) -> Value {
    let attached = state
        .sessions
        .lock()
        .unwrap()
        .values()
        .filter(|s| s.running || !s.blank)
        .count();
    ok(json!({
        "version": state.version,
        "cwd": state.host_cwd,
        "provider": state.runtime.provider,
        "model": state.runtime.model,
        "attachedSessions": attached,
        "canOpenPath": true,
    }))
}

/// mock provider 模型分组（llm.models / session.models 共用；对齐 modelProviderGroupSchema）。
pub(super) fn mock_model_group() -> Value {
    json!({
        "id": "mock",
        "name": "Mock",
        "models": [{ "id": "mock-1", "name": "Mock 1" }],
    })
}

/// 模型分组目录（llm.models / session.models 共用）：真 provider 时每 provider 一组，
/// mock 模式（无配置）时单 mock 组。wire 形状对齐 modelProviderGroupSchema：
/// `{id, name, models:[{id, name, description?, reasoning?}]}`。
pub(super) fn model_groups(state: &AppState) -> Value {
    if state.providers.is_empty() {
        return json!([mock_model_group()]);
    }
    let groups: Vec<Value> = state
        .providers
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.display_name,
                "models": p.models.iter().map(|m| {
                    let mut v = json!({
                        "id": m.id,
                        "name": m.label.clone().unwrap_or_else(|| m.id.clone()),
                    });
                    if let Some(r) = &m.reasoning {
                        let efforts: Vec<Value> = r.efforts.iter().map(|e| {
                            let mut ev = json!({ "id": e.id, "name": e.name });
                            if let Some(d) = &e.description {
                                ev["description"] = json!(d);
                            }
                            ev
                        }).collect();
                        let mut rv = json!({ "efforts": efforts });
                        if let Some(d) = &r.default_effort {
                            rv["defaultEffort"] = json!(d);
                        }
                        v["reasoning"] = rv;
                    }
                    v
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!(groups)
}

/// 当前模型选择：(provider, model)——会话 override 优先，否则运行时默认。
pub(super) fn current_model(state: &AppState, session_id: &str) -> (String, String) {
    if let Some(h) = state.sessions.lock().unwrap().get(session_id) {
        if let Some(sel) = &h.selected {
            return sel.clone();
        }
    }
    (state.runtime.provider.clone(), state.runtime.model.clone())
}

/// agentPreset.list（未特权）：M2.5 返回空清单（无 authoring 预设）。
/// hasDocument=false：无原生 preset 文档可编辑（特权面 openDocument 相关）。
fn agent_preset_list() -> Value {
    ok(json!({ "presets": [], "authorable": true, "hasDocument": false }))
}

/// skill.list：M1 无技能注册，返回空清单（技能调用是 session.prompt 前导 `/name`）。
fn skill_list(payload: Value) -> Value {
    if payload.get("sessionId").and_then(Value::as_str).is_none() {
        return err("bad-request", "missing sessionId");
    }
    ok(json!({ "skills": [] }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_never_splits_astral_boundaries() {
        // 纯 emoji（surrogate pair）长文本：截断必须落在 code point 边界。
        let emoji = "🦀".repeat(300);
        let s = make_snippet(&emoji, "🦀").expect("snippet");
        // 不劈 surrogate pair：snippet 内每个字符都是完整 emoji（char 数 ≤ 240+省略号）。
        assert!(s.chars().all(|c| c == '🦀' || c == '…'));
        assert!(s.chars().count() <= 241);
    }

    #[test]
    fn snippet_byte_offset_converted_to_char_offset() {
        // 真实 bug 回归：query 前的多字节字符（中文）把字节偏移算进 char 窗口。
        // 修复前 start_char 用字节 pos → 窗口整体前移、跳过匹配点。
        let text = format!("{}hello", "心".repeat(120));
        let s = make_snippet(&text, "hello").expect("snippet");
        // 窗口必须仍包含匹配词（修复前会丢）。
        assert!(s.contains("hello"), "snippet: {s}");
    }

    #[test]
    fn snippet_collapses_whitespace() {
        let s = make_snippet("a   b\n\nc  query  here", "query").unwrap();
        assert!(!s.contains('\n'));
        assert!(s.contains("query here"), "snippet: {s}");
    }

    /// session.delete：删除会话（running 拒绝）；持久化日志 + live 表 +
    /// host 广播 session-removed。
    #[tokio::test]
    async fn session_delete_removes_and_rejects_running() {
        use bm_assembly::Runtime;
        use std::sync::Arc;

        let db = std::env::temp_dir().join(format!("bm-del-{}.db", uuid::Uuid::new_v4()));
        let rt = Runtime::headless(db.clone()).unwrap();
        let state = Arc::new(AppState::assemble(rt, vec![], vec![]));

        // 创建会话 → 删除成功，会话从 live 表消失。
        let c = session_create(&state, serde_json::json!({ "sessionId": "s-del" })).await;
        assert_eq!(c["ok"], true, "create ok: {c}");
        let d = session_delete(&state, serde_json::json!({ "sessionId": "s-del" })).await;
        assert_eq!(d["ok"], true, "delete ok: {d}");
        assert_eq!(d["value"]["deleted"], true);
        assert!(state.sessions.lock().unwrap().get("s-del").is_none(), "live table must drop session");

        // 持久化日志也被删除（delete_session 语义）。
        let list = state.runtime.persist.list_sessions().await.unwrap_or_default();
        assert!(!list.contains(&"s-del".to_string()), "persist must drop session");

        // 未知会话 → 幂等删除（delete_session 对不存在的会话无副作用返回 Ok）。
        let missing = session_delete(&state, serde_json::json!({ "sessionId": "nope" })).await;
        assert_eq!(missing["ok"], true, "idempotent delete of missing session: {missing}");

        // 运行中会话拒绝（agent-busy）。
        let c2 = session_create(&state, serde_json::json!({ "sessionId": "s-run" })).await;
        assert_eq!(c2["ok"], true);
        state.sessions.lock().unwrap().get_mut("s-run").unwrap().running = true;
        let r = session_delete(&state, serde_json::json!({ "sessionId": "s-run" })).await;
        assert_eq!(r["error"]["code"], "agent-busy");
        // 取消后即可删。
        state.sessions.lock().unwrap().get_mut("s-run").unwrap().running = false;
        let r2 = session_delete(&state, serde_json::json!({ "sessionId": "s-run" })).await;
        assert_eq!(r2["ok"], true, "delete after unbusy: {r2}");

        let _ = std::fs::remove_file(db);
    }

    /// 回归 P2-C：settings/credentials 写盘后，新 AppState 从文件恢复
    /// （value/revision/凭据三样齐全；重启不再静默丢配置）。
    #[tokio::test]
    async fn settings_file_persists_and_reloads() {
        use bm_assembly::Runtime;
        let dir = std::env::temp_dir().join(format!("bm-settings-{}.json", uuid::Uuid::new_v4()));
        let db = dir.with_extension("db");
        let rt = Runtime::headless(db.clone()).unwrap();
        let state = Arc::new(AppState::assemble_with_settings_path(
            rt,
            vec![],
            vec![],
            Some(dir.clone()),
        ));
        {
            let mut s = state.settings.lock().unwrap();
            s.insert(
                "shell".to_string(),
                serde_json::json!({ "cwd": "/tmp" })
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            state.settings_revisions.lock().unwrap().insert("shell".to_string(), 1);
            state
                .credentials
                .lock()
                .unwrap()
                .insert("TEST_API_KEY".to_string(), "sekret".to_string());
        }
        state.persist_settings();
        assert!(dir.exists(), "settings file must exist after persist");

        let db2 = db.with_extension("db2");
        let rt2 = Runtime::headless(db2.clone()).unwrap();
        let state2 = Arc::new(AppState::assemble_with_settings_path(
            rt2,
            vec![],
            vec![],
            Some(dir.clone()),
        ));
        assert_eq!(
            state2.settings.lock().unwrap().get("shell").cloned(),
            Some(
                serde_json::json!({ "cwd": "/tmp" })
                    .as_object()
                    .unwrap()
                    .clone()
            )
        );
        assert_eq!(
            state2.settings_revisions.lock().unwrap().get("shell").copied(),
            Some(1)
        );
        assert_eq!(
            state2.credentials.lock().unwrap().get("TEST_API_KEY").map(String::as_str),
            Some("sekret")
        );
        let _ = std::fs::remove_file(&dir);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&db2);
    }

    /// 回归 BUG-002：attach_event_bus 对已恢复会话的实时 wire seq 必须从历史
    /// 长度播种，不回退到 0 撞历史基线（前端按水位单调去重会丢恢复后事件）。
    #[tokio::test]
    async fn attach_seeds_seq_from_history() {
        use bm_assembly::Runtime;
        use kernel_contracts::session::{SessionHeader, SessionId, StepPhase, TurnEndReason, TurnEvent};
        use std::sync::Arc;

        let db = std::env::temp_dir().join(format!("bm-seed-{}.db", uuid::Uuid::new_v4()));
        let rt = Runtime::headless(db.clone()).unwrap();
        let agent = rt.create_session(SessionHeader {
            id: SessionId("s1".into()),
            app: "test".into(),
            profile: "test".into(),
            workspace: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
        // 历史：User + Turn Started + Step Started + Step Ended + Turn Ended（5 个事件全翻译）。
        let seq = [
            SessionEvent::UserMessage { text: "hi".into() },
            SessionEvent::Turn(TurnEvent::Started { turn: 1 }),
            SessionEvent::Step { turn: 1, step: 1, phase: StepPhase::Started },
            SessionEvent::Step { turn: 1, step: 1, phase: StepPhase::Ended },
            SessionEvent::Turn(TurnEvent::Ended { turn: 1, reason: TurnEndReason::Completed }),
        ];
        for e in seq {
            let rec = agent.session().append(e);
            rt.persist
                .append_events("s1", std::slice::from_ref(&rec.event))
                .await
                .unwrap();
        }
        let state = Arc::new(AppState::assemble(rt, vec![], vec![]));
        state.sessions.lock().unwrap().insert(
            "s1".into(),
            SessionHandle {
                agent,
                running: false,
                blank: false,
                title: None,
                selected: None,
            },
        );
        let mut rx = state.events_tx.subscribe();
        let _disposer = state.attach_event_bus();
        // 经会话 append 触发总线（attach 后实时链路）→ seq 应从历史 wire 数（5）接续。
        let agent2 = state.runtime.store.get("s1").unwrap();
        agent2.append(SessionEvent::UserMessage { text: "second".into() });
        let payload = rx.recv().await.expect("event frame");
        let seq_value = payload["event"]["seq"].as_i64().unwrap_or(-1);
        assert_eq!(seq_value, 5, "实时 seq 应从历史 wire 长度接续，实际 {seq_value}");
        let _ = std::fs::remove_file(db);
    }

    /// 对齐上游 session.create 语义（api-proxy ensureSession）：
    /// 同 cwd 重复 create → 幂等采用（返回现有）；不同 cwd → `session-conflict`
    /// {sessionId, requestedCwd, existingCwd}（上游 RpcErrorDetailsMap 逐字形状，
    /// 无 session-exists 码）。回归 BUG-007 旧实现（静默拒绝）。
    #[tokio::test]
    async fn session_create_idempotent_or_conflict() {
        use bm_assembly::Runtime;
        use std::sync::Arc;

        let db = std::env::temp_dir().join(format!("bm-create-{}.db", uuid::Uuid::new_v4()));
        let rt = Runtime::headless(db.clone()).unwrap();
        let state = Arc::new(AppState::assemble(rt, vec![], vec![]));

        // 首次 create（带 cwd）。
        let r1 = session_create(&state, serde_json::json!({
            "sessionId": "s1",
            "cwd": "/work/a",
        })).await;
        assert_eq!(r1["ok"], true, "first create ok: {r1}");
        assert_eq!(r1["value"]["sessionId"], "s1");

        // 同 cwd 重复 → 幂等采用（ok:true，不报错）。
        let r2 = session_create(&state, serde_json::json!({
            "sessionId": "s1",
            "cwd": "/work/a",
        })).await;
        assert_eq!(r2["ok"], true, "same-cwd repeat must adopt: {r2}");

        // 不同 cwd → session-conflict + 逐字 details。
        let r3 = session_create(&state, serde_json::json!({
            "sessionId": "s1",
            "cwd": "/work/b",
        })).await;
        assert_eq!(r3["ok"], false);
        assert_eq!(r3["error"]["code"], "session-conflict");
        assert_eq!(r3["error"]["details"]["sessionId"], "s1");
        assert_eq!(r3["error"]["details"]["requestedCwd"], "/work/b");
        assert_eq!(r3["error"]["details"]["existingCwd"], "/work/a");

        // 未知 workspace 前置拒绝（创建副作用发生前）：无残留会话。
        let r4 = session_create(&state, serde_json::json!({
            "sessionId": "s2",
            "workspaceId": "nope",
        })).await;
        assert_eq!(r4["ok"], false);
        assert_eq!(r4["error"]["code"], "workspace-not-found");
        assert!(state.sessions.lock().unwrap().get("s2").is_none());
        assert!(
            !state.runtime.persist.list_sessions().await.unwrap_or_default().contains(&"s2".to_string()),
            "unknown workspace must not leave a half-created session"
        );

        // 无 sessionId → 自动生成（幂等点仍以生成 id 为准）。
        let r5 = session_create(&state, serde_json::json!({ "cwd": "/work/x" })).await;
        assert_eq!(r5["ok"], true);
        assert!(r5["value"]["sessionId"].is_string());

        let _ = std::fs::remove_file(db);
    }

    /// §6 插件装配：plugin.core.list 合并 JS 插件（category=Feature）。
    /// 核心三插件（Core）不变；--plugins-dir 装配后 Feature 条目追加——
    /// 插件管理员按类分组展示的数据源。
    ///
    /// 注意：JsBridge 内部 block_on 自带 runtime（测试纪律），本测试用
    /// `#[test]`（同步）而非 `#[tokio::test]`，避免嵌套 runtime 冲突。
    #[test]
    fn plugin_core_list_merges_js_plugins() {
        use bm_assembly::Runtime;
        use std::sync::Arc;

        let db = std::env::temp_dir().join(format!("bm-plugin-list-{}.db", uuid::Uuid::new_v4()));
        let rt = Runtime::headless(db.clone()).unwrap();
        // 未装配：只有核心三件（category 全 Core）。
        let r0 = plugin_core_list(&Arc::new(AppState::assemble(rt, vec![], vec![])));
        let plugins0 = r0["value"]["plugins"].as_array().unwrap().clone();
        assert_eq!(plugins0.len(), 3);
        assert!(plugins0.iter().all(|p| p["category"] == "core"));

        // 装配一个 JS 插件目录。
        let mut rt = Runtime::headless(db.clone()).unwrap();
        let root = std::env::temp_dir().join(format!("bm-plugin-list-js-{}", uuid::Uuid::new_v4()));
        let dir = root.join("alpha");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"id":"alpha","name":"Alpha","version":"1.2.3","entry":"main.js","host":["log"]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("main.js"), "host.log('info','alive');").unwrap();
        let n = rt.load_js_plugins_dir(&root).unwrap();
        assert_eq!(n, 1);
        // 探针变 Ready（fail-loud 语义：装配了就 Ready）。
        assert_eq!(
            rt.plugin_availability(),
            kernel_contracts::ports::PluginRuntimeAvailability::Ready
        );

        let state = Arc::new(AppState::assemble(rt, vec![], vec![]));
        let r = plugin_core_list(&state);
        let plugins = r["value"]["plugins"].as_array().unwrap().clone();
        assert_eq!(plugins.len(), 4, "core 3 + js 1: {r}");
        let js: Vec<&serde_json::Value> =
            plugins.iter().filter(|p| p["category"] == "feature").collect();
        assert_eq!(js.len(), 1);
        assert_eq!(js[0]["id"], "alpha");
        assert_eq!(js[0]["version"], "1.2.3");

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db);
    }

    /// §6 JS 插件管理面：plugin.js.list 列已装配插件（faces 最小权限展示）；
    /// plugin.js.invoke 经 spawn_blocking 执行插件主函数（脱离 tokio 上下文）。
    ///
    /// 测试纪律：JsBridge 内部 block_on 自带 runtime——装配 JS 插件必须在
    /// `#[test]`（同步）上下文；invoke 的 spawn_blocking 需要 tokio runtime，
    /// 故用 `#[test]` + 手建 runtime（`Runtime::new().block_on`）驱动 invoke，
    /// 不经 `#[tokio::test]`（其上下文会与 JsBridge block_on 冲突）。
    #[test]
    fn plugin_js_list_and_invoke() {
        use bm_assembly::Runtime;
        use std::sync::Arc;

        let db = std::env::temp_dir().join(format!("bm-plugin-js-{}.db", uuid::Uuid::new_v4()));
        let root = std::env::temp_dir().join(format!("bm-plugin-js-dir-{}", uuid::Uuid::new_v4()));

        // 未装配：list/invoke 都 fail-loud（不假空清单/假成功）。
        let rt = Runtime::headless(db.clone()).unwrap();
        let state = Arc::new(AppState::assemble(rt, vec![], vec![]));
        let r0 = plugin_js_list(&state);
        assert_eq!(r0["ok"], false);
        assert_eq!(r0["error"]["code"], "plugin-runtime-unavailable");

        // 装配一个 JS 插件（id=greet，faces=[log]；主函数返回欢迎语）。
        let mut rt = Runtime::headless(db.clone()).unwrap();
        let dir = root.join("greet");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"id":"greet","name":"Greet","version":"0.1.0","entry":"main.js","host":["log"]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("main.js"),
            r#"
            globalThis.__main = () => {
                host.log('info', 'greet called');
                return { greeting: 'hello from greet plugin' };
            };
            "#,
        )
        .unwrap();
        let n = rt.load_js_plugins_dir(&root).unwrap();
        assert_eq!(n, 1);
        let state = Arc::new(AppState::assemble(rt, vec![], vec![]));

        // list：插件清单 + faces。
        let rl = plugin_js_list(&state);
        assert_eq!(rl["ok"], true);
        assert_eq!(rl["value"]["plugins"][0]["id"], "greet");
        assert_eq!(rl["value"]["plugins"][0]["faces"][0], "log");

        // invoke：手建 tokio runtime 驱动（spawn_blocking 需要 runtime 上下文；
        // 装配已在同步阶段完成，此处不重新装配 JS 引擎）。
        let tokio_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let ri = tokio_rt.block_on(plugin_js_invoke(
            &state,
            serde_json::json!({ "pluginId": "greet" }),
        ));
        assert_eq!(ri["ok"], true, "invoke ok: {ri}");
        assert_eq!(ri["value"]["result"]["greeting"], "hello from greet plugin");

        // 未知插件 id → plugin-exec-error。
        let rmiss = tokio_rt.block_on(plugin_js_invoke(
            &state,
            serde_json::json!({ "pluginId": "nope" }),
        ));
        assert_eq!(rmiss["ok"], false);
        assert_eq!(rmiss["error"]["code"], "plugin-exec-error");

        // 缺 pluginId → bad-request。
        let rbad = tokio_rt.block_on(plugin_js_invoke(&state, serde_json::json!({})));
        assert_eq!(rbad["error"]["code"], "bad-request");

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(db);
    }

    /// §7 认证门控：装配 AuthPort 后，非 auth 方法需会话 token（auth-required 拒绝）；
    /// auth.login 签发 token、auth.status 校验、auth.changePassword 改密。
    ///
    /// 测试用极简 AuthPort（web-server 是 L0，边界守卫禁 dev-deps 依赖 plugin-auth；
    /// plugin-auth 自身行为在其 crate 测试覆盖，此处只测 web-server 门控接线）。
    struct TestAuth;
    #[async_trait::async_trait]
    impl kernel_contracts::AuthPort for TestAuth {
        fn is_authenticated(&self, token: &str) -> bool {
            token == "valid-token"
        }
        async fn login(&self, password: &str) -> Result<kernel_contracts::AuthResult, kernel_contracts::PortError> {
            if password == "secret" {
                Ok(kernel_contracts::AuthResult::success("valid-token".into()))
            } else {
                Ok(kernel_contracts::AuthResult::failure("wrong-password"))
            }
        }
        fn logout(&self, _token: &str) {}
        async fn change_password(
            &self,
            token: &str,
            _current: &str,
            new: &str,
        ) -> Result<kernel_contracts::AuthResult, kernel_contracts::PortError> {
            if !self.is_authenticated(token) {
                Ok(kernel_contracts::AuthResult::failure("login-required"))
            } else if new.len() < 4 {
                Ok(kernel_contracts::AuthResult::failure("password-too-short"))
            } else {
                Ok(kernel_contracts::AuthResult::success(String::new()))
            }
        }
    }

    #[tokio::test]
    async fn auth_gate_blocks_and_login_flow() {
        use bm_assembly::Runtime;
        use std::sync::Arc;

        let db = std::env::temp_dir().join(format!("bm-auth-gate-{}.db", uuid::Uuid::new_v4()));
        // 未装配认证：不门控（旧行为），auth.* 诚实失败。
        let rt = Runtime::headless(db.clone()).unwrap();
        let state = Arc::new(AppState::assemble(rt, vec![], vec![]));
        let r0 = dispatch(&state, "session.list", json!({}), None).await;
        assert_eq!(r0["ok"], true, "no auth = no gate: {r0}");
        let r0b = dispatch(&state, "auth.status", json!({}), None).await;
        assert_eq!(r0b["error"]["code"], "auth-not-available");

        // 装配认证：session.list 未登录 → auth-required；host.describe 放行。
        let mut rt = Runtime::headless(db.clone()).unwrap();
        rt.install_auth(Arc::new(TestAuth));
        let state = Arc::new(AppState::assemble(rt, vec![], vec![]));
        let r1 = dispatch(&state, "session.list", json!({}), None).await;
        assert_eq!(r1["error"]["code"], "auth-required");
        let r1b = dispatch(&state, "host.describe", json!({}), None).await;
        assert_eq!(r1b["ok"], true, "host.describe is auth_methods: {r1b}");

        // 错误密码 → wrong-password；正确密码 → token。
        let r2 = dispatch(&state, "auth.login", json!({ "password": "nope" }), None).await;
        assert_eq!(r2["error"]["code"], "wrong-password");
        let r3 = dispatch(&state, "auth.login", json!({ "password": "secret" }), None).await;
        let token = r3["value"]["token"].as_str().unwrap().to_string();
        assert_eq!(token, "valid-token");

        // 带 token → 放行。
        let r4 = dispatch(&state, "session.list", json!({}), Some(&token)).await;
        assert_eq!(r4["ok"], true, "with token: {r4}");
        // 状态校验。
        let r5 = dispatch(&state, "auth.status", json!({}), Some(&token)).await;
        assert_eq!(r5["value"]["authenticated"], true);

        // 未登录改密 → login-required；登录后改密成功。
        let r6 = dispatch(
            &state,
            "auth.changePassword",
            json!({ "currentPassword": "secret", "newPassword": "newsecret" }),
            None,
        )
        .await;
        assert_eq!(r6["error"]["code"], "login-required");
        let r7 = dispatch(
            &state,
            "auth.changePassword",
            json!({ "currentPassword": "secret", "newPassword": "newsecret" }),
            Some(&token),
        )
        .await;
        assert_eq!(r7["ok"], true, "change with token: {r7}");

        let _ = std::fs::remove_file(db);
    }
}
