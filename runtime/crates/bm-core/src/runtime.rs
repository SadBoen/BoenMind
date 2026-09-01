//! Runtime 核心循环:全部事件与日志写入只在唯一的循环任务内发生(单写者),
//! 保证 event_seq/log_seq 的全局单调(INV-3/INV-4 的结构前提)。
//! 回合任务只通过内部命令通道回报,不直接改状态。

use bm_persist::EventStore;

use crate::approval::{ApprovalError, ApprovalManager, OpenApproval, RespondDecision};
use crate::broker::{Broker, CallContext, CallOutcome, Decision, DenyReason, GrantLedger};
use crate::bus::EventBus;
use crate::clock::Clock;
use crate::exec_log::ExecutionLog;
use crate::ports::{ModelConnector, SecretStore};
use crate::registry::{CapabilityProvider, CapabilityRegistry};
use crate::state::{Agent, Operation, Session, budget_from_spec};
use crate::{CoreError, CoreResult};
use bm_contract::budget::BudgetScope;
use bm_contract::capability::{Approval, DataTrust, GrantScope};
use bm_contract::connector::{BudgetCtx, InvokeRequest, InvokeResponse, Message, Role};
use bm_contract::error_codes::ErrorCode;
use bm_contract::events::{EventEnvelope, EventType};
use bm_contract::exec_log::LogKind;
use bm_contract::ids::{BmId, IdGen};
use bm_contract::states::{AgentState, OperationState, SessionState};
use bm_contract::timestamp::format_ts;
use bm_contract::wire::{
    self, CancelParams, CancelResult, Cursor, EventsPollParams, EventsPollResult,
    GetOperationParams, Principal, Receipt, SendInputParams, SessionCloseParams,
    SessionCloseResult, SessionCreateParams, SessionCreateResult, SessionResumeParams,
    SessionResumeResult, TaskType, WireError,
};
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// 回合默认超时(GT-A3:deadline = 发起 + 30s)。
pub const DEFAULT_TURN_TIMEOUT_SECS: i64 = 30;

/// 模型 → 凭据引用的默认映射(合同字符集内;实现可注入自己的映射)。
pub fn default_secret_ref(model_id: &str) -> String {
    format!("secret:model.{model_id}")
}

/// 恢复裁定(RecoveryPlan 的落点,基线 9.5/13.3;INV-10/11):
/// - `ClaimRun` = 认领继续(仅 interrupted;NoEffect 域可安全重跑)
/// - `Succeeded`/`Failed` = 外部核验或用户裁定的结论(outcome_unknown 仅此二出口)
/// - `Cancelled` = 用户裁定取消(仅 interrupted;outcome_unknown 无此边)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryVerdict {
    ClaimRun,
    Succeeded,
    Failed,
    Cancelled,
}

pub struct RuntimeConfig {
    pub version: String,
    pub data_dir: Option<std::path::PathBuf>,
    /// 持久层(M2 起);None = 纯内存(M1 兼容形态,测试用)。
    pub store: Option<std::sync::Arc<dyn EventStore>>,
    pub connector: Arc<dyn ModelConnector>,
    pub secret_store: Arc<dyn SecretStore>,
    pub id_gen: Arc<dyn IdGen>,
    pub clock: Arc<dyn Clock>,
    pub turn_timeout_secs: i64,
    /// 降级链最大尝试次数;None = 取链长(合同上限 3)。
    pub max_attempts: Option<u32>,
    /// 内置能力集(M4):启动时注册进 Capability Registry;
    /// 空集 = 无能力面(等价 M3 形态)。
    pub capabilities: Vec<(
        bm_contract::capability::CapabilityManifest,
        Arc<dyn CapabilityProvider>,
    )>,
    /// M7 S4:异步能力执行器(MCP 等慢外部 Provider)。manifest.provider
    /// 以 "mcp." 开头的能力注册时标记 async,dispatch 走本执行器。
    pub async_executor: Option<Arc<dyn crate::ports::AsyncCapabilityExecutor>>,
    /// M9-S2:模型真流式开关(默认关——既有测试/黄金轨迹零变化;
    /// 开启时回合模型输出以 model.content.delta 逐块入事件流)。
    pub model_streaming: bool,
}

/// 回合任务向核心循环回报的内部消息。
enum TurnEvent {
    /// 单次尝试失败(INV-4:每次尝试各产生一条 failed 事件 + 日志)。
    AttemptFailed {
        operation_id: BmId,
        model_id: String,
        attempt: u32,
        error_code: ErrorCode,
    },
    /// 链耗尽(或不可重试错误):回合失败落定。
    ChainExhausted {
        operation_id: BmId,
        error_code: ErrorCode,
    },
    /// 显式取消落定(回合边界)。
    Cancelled { operation_id: BmId },
    /// 单次尝试成功:回合成功落定。
    Completed {
        operation_id: BmId,
        model_id: String,
        attempt: u32,
        content: String,
        usage_in: u64,
        usage_out: u64,
        latency_ms: u64,
        stream_interrupted: bool,
    },
}

/// 运行期执行上下文(核心循环私有)。
struct World {
    config: RuntimeConfig,
    tx: mpsc::Sender<Cmd>,
    store: Option<std::sync::Arc<dyn EventStore>>,
    bus: EventBus,
    exec_log: Arc<ExecutionLog>,
    sessions: HashMap<BmId, Session>,
    agents: HashMap<BmId, Agent>,
    operations: HashMap<BmId, Operation>,
    /// 运行中的回合:operation_id → 取消令牌。
    in_flight: HashMap<BmId, CancellationToken>,
    started_at: DateTime<Utc>,
    started_instant: Instant,
    draining: bool,
    stopped: bool,
    /// 持久层故障拒写态:置位后拒绝一切业务命令;内存视图以持久层为准重建。
    persist_poisoned: bool,
    // ---- M4:Capability / Broker / Approval ---------------------------------
    registry: CapabilityRegistry,
    grants: GrantLedger,
    /// 审批对象(approval_id → 对象);持久化随 T3c 接 SQLite。
    approvals: HashMap<BmId, Approval>,
    /// 待裁决的能力调用:approval_id → 载荷(批准后重放执行用)。
    cap_pending: HashMap<BmId, PendingCapabilityCall>,
    /// 幂等收据仓(key_hash → 原收据;external-side-effect 抑制判据,
    /// ADR-0002 条件 6)。T6c 收紧(M5-T1):落表持久,恢复期装载。
    idem_results: HashMap<String, serde_json::Value>,
    /// capability 操作的系统容器 ID(内存合成;M4 能力调用不依赖 Session/Agent,
    /// operations 表不落行,规范状态由 approvals/grants 承载——回看复核项)。
    system_session: BmId,
    system_agent: BmId,
    /// M5:Task 规范状态(task/task.v0.1;L2 唯一持有,World 内为内存视图)。
    tasks: HashMap<BmId, crate::task::Task>,
    /// M5.4:Task Board 投影(可弃可重建;emit 钩子增量维护)。
    task_board: crate::task::TaskBoard,
    /// M5-T6:Task 包络工具调用记账(task_id → 已用次数;持久于
    /// task_budget_ledger 聚合行,agent_id = "")。
    task_tool_calls: HashMap<BmId, u64>,
    /// M5-T7:Watchdog 监护状态(仅监督,不推断编排下一步)。
    watchdog: crate::watchdog::WatchdogState,
    /// M5-T8:operation → capability(核验证据定位;内存索引,事件可重建)。
    op_capability: HashMap<BmId, String>,
    /// M6:成员结果收集(task_id → 结果流水;来源/状态/关联 Operation)。
    task_results: HashMap<BmId, Vec<serde_json::Value>>,
    /// M7 S4:在途异步能力调用(operation_id → 留档)。
    op_async_meta: HashMap<BmId, AsyncCallMeta>,
    /// M9-S2:在途回合已发 delta 计数(index 单调,0 起;completed 后随审计清理可留)
    model_delta_seq: HashMap<BmId, u64>,
    /// M9-S3:worker 自主环在途状态(task → 状态;终局即移除)
    autorun: HashMap<BmId, AutorunState>,
    /// M7 S4:异步能力调用结果(operation_id → result;内存,随操作同寿命)。
    op_results: HashMap<BmId, serde_json::Value>,
    /// M7 S5:Provider 健康面(provider → 状态;进程内,不入 core-transitions)。
    provider_health: HashMap<String, ProviderHealth>,
    /// M8.3:在途异步能力调用的取消令牌(operation_id → token)。
    cap_in_flight: HashMap<BmId, CancellationToken>,
    /// M7 S1:turn 模型调用 Broker 凭证留档(operation_id 索引;
    /// 授权点在 spawn,审计点在回合模型阶段终态——两段由 call_id 缝合)。
    model_call_audit: HashMap<BmId, ModelCallAudit>,
    /// W5:会话对话台账(session_id → [user, assistant] 对;内存,随进程
    /// 寿命——会话本就不跨进程,openai_compat 重启即「未知会话」)。回合
    /// spawn 时回喂模型(修复「多轮无记忆」),成功落定时回写。
    session_chats: HashMap<BmId, Vec<(String, String)>>,
    /// W5 上下文透视:每次模型调用请求快照(context-log.jsonl;/admin/context)。
    ctx_log: Arc<crate::context_log::ContextLog>,
}

impl World {
    /// 自规范状态行装配内存视图(M2 启动恢复,任务 T3)。
    /// request_id 未持久化(事件流不承载):以 op 的 ULID 段确定性合成 req_ 前缀 ID,
    /// 保证恢复幂等;action_summary/result_reference 为非持久展示字段,恢复后为占位。
    pub fn load_world_rows(
        &mut self,
        rows: bm_persist::WorldRows,
        pending_interrupts: &mut Vec<(BmId, BmId, String)>,
        agents_to_resume: &mut Vec<(BmId, Option<BmId>)>,
    ) {
        for s in rows.sessions {
            let id = BmId::parse(s.id).expect("库内 session id 合法");
            let state = SessionState::from_wire(&s.state).expect("库内 session 状态合法");
            self.sessions.insert(
                id.clone(),
                Session {
                    id: id.clone(),
                    agent_id: BmId::parse(s.agent_id).expect("合法"),
                    state,
                    created_at: s.created_at,
                },
            );
        }
        for a in rows.agents {
            let id = BmId::parse(a.id).expect("库内 agent id 合法");
            let state = AgentState::from_wire(&a.state).expect("库内 agent 状态合法");
            // 崩溃时停在非运行中间态的 agent(starting/waiting_model/stopping/resuming)
            // 需要走 interrupted→resuming→running 恢复(ADR-0003 决策要点 8)
            if matches!(
                state,
                AgentState::Starting
                    | AgentState::WaitingModel
                    | AgentState::Stopping
                    | AgentState::Resuming
            ) {
                agents_to_resume.push((id.clone(), None));
            }
            let chain: Vec<String> =
                serde_json::from_str(&a.model_chain).expect("model_chain 为 JSON 数组");
            let mut budget = crate::budget::BudgetState::new(
                a.budget_max_tokens.map(|v| v as u64).unwrap_or(u64::MAX),
                a.budget_max_turns.map(|v| v as u32).unwrap_or(u32::MAX),
            );
            budget.used_tokens = a.budget_used_tokens as u64;
            budget.turns_used = a.budget_turns_used as u32;
            self.agents.insert(
                id.clone(),
                Agent {
                    id: id.clone(),
                    session_id: BmId::parse(a.session_id).expect("合法"),
                    name: a.name,
                    model_chain: chain,
                    state,
                    budget,
                },
            );
        }
        for o in rows.operations {
            let id = BmId::parse(o.id).expect("库内 operation id 合法");
            let state = OperationState::from_wire(&o.state).expect("库内 operation 状态合法");
            let request_id = match &o.request_id {
                Some(r) => BmId::parse(r).expect("合法"),
                None => BmId::from_parts("req", id.ulid_part()).expect("同段合成合法"),
            };
            let error = o.error_code.as_ref().map(|code| {
                let code = ErrorCode::from_wire(code).unwrap_or(ErrorCode::Internal);
                let mut e = WireError::new(code, "恢复自持久状态".to_string());
                e.retryable = false;
                e
            });
            let running = state == OperationState::Running;
            self.operations.insert(
                id.clone(),
                Operation {
                    id: id.clone(),
                    request_id,
                    session_id: BmId::parse(o.session_id).expect("合法"),
                    agent_id: BmId::parse(o.agent_id).expect("合法"),
                    state,
                    turn_index: o.turn_index as u32,
                    created_at: o.created_at,
                    completed_at: o.completed_at,
                    action_summary: o.action_summary.unwrap_or_default(),
                    result_reference: o.result_reference.map(|r| wire::ResultReference {
                        kind: wire::ResultRefKind::ExecutionLog,
                        r#ref: r,
                    }),
                    error,
                },
            );
            if running {
                let agent_id = self.operations[&id].agent_id.clone();
                pending_interrupts.push((id, agent_id, "running".into()));
            }
        }
        // M5:Task 规范状态装载(tasks 表;成员事实由 task_members 自事件承载)
        for t in rows.tasks {
            let task = crate::task::task_from_row(&t).expect("库内 task 行合法");
            self.tasks.insert(task.id.clone(), task);
        }
    }

    /// 会话相关事件读取:有持久层走日志(跨进程历史完整),否则走内存总线。
    fn events_for_session(
        &self,
        session_id: &BmId,
        since: u64,
        limit: u32,
    ) -> (Vec<EventEnvelope>, u64, bool) {
        if let Some(store) = &self.store {
            let mut evs: Vec<EventEnvelope> = store
                .replay_since(since)
                .unwrap_or_default()
                .into_iter()
                .filter(|e| e.session_id.as_ref() == Some(session_id))
                .collect();
            let last = store.last_log_seq().unwrap_or(0);
            let has_more = evs.len() > limit as usize;
            evs.truncate(limit as usize);
            (evs, last, has_more)
        } else {
            self.bus.poll(session_id, since, limit)
        }
    }

    /// Task 事件流读取(watch 观察面):跨会话按 payload.task_id 过滤。
    fn events_for_task(
        &self,
        task_id: &BmId,
        since: u64,
        limit: u32,
    ) -> (Vec<EventEnvelope>, u64, bool) {
        let mut evs: Vec<EventEnvelope> = if let Some(store) = &self.store {
            store.replay_since(since).unwrap_or_default()
        } else {
            self.bus.events().to_vec()
        };
        let last = match &self.store {
            Some(store) => store.last_log_seq().unwrap_or(0),
            None => self.bus.last_seq(),
        };
        evs.retain(|e| e.payload["task_id"].as_str() == Some(task_id.as_str()));
        let has_more = evs.len() > limit as usize;
        evs.truncate(limit as usize);
        (evs, last, has_more)
    }

    fn now_ts(&self) -> bm_contract::BmTimestamp {
        format_ts(self.config.clock.now())
    }

    /// 唯一的事件发射口:event_seq 分配 + 写穿持久 + 总线追加。
    fn emit(
        &mut self,
        ty: EventType,
        session_id: Option<BmId>,
        agent_id: Option<BmId>,
        operation_id: Option<BmId>,
        payload: serde_json::Value,
    ) -> EventEnvelope {
        // T7 持久前校验(硬约束 3;ADR-0001 条件 3):事件 = 已发生的事实,
        // 命令语义形状在持久化前拒绝并告警(store.write.rejected)。
        let shape_err = validate_event_shape(&ty, &payload);
        let seq = self.bus.next_seq();
        let event = EventEnvelope::new(
            seq,
            ty,
            self.now_ts(),
            session_id,
            agent_id,
            operation_id,
            payload,
        );
        if let Err(reason) = shape_err {
            tracing::warn!(seq = %event.event_seq, %reason, "命令语义事件被拒绝持久化");
            self.bus.append(event.clone());
            // 告警事件(store.write.rejected 载荷形状本身不触发递归)
            let warn_seq = self.bus.next_seq();
            let warn = EventEnvelope::new(
                warn_seq,
                EventType::StoreWriteRejected,
                self.now_ts(),
                None,
                None,
                None,
                serde_json::json!({
                    "key": event.event_seq.to_string(),
                    "reason": reason,
                }),
            );
            if let Some(store) = &self.store
                && !self.persist_poisoned
            {
                let _ = store.record(&warn);
            }
            self.bus.append(warn);
            return event;
        }
        // 写穿(M2 规格 §5.1):record 内部固定 ①日志+flush → ②物化 → ③位点。
        // 失败即进入拒写态:内存视图与持久层自此分叉,以持久层为准(重启重建)。
        #[allow(clippy::collapsible_if)] // 三重条件展平反而难读
        if let Some(store) = &self.store {
            if !self.persist_poisoned {
                if let Err(e) = store.record(&event) {
                    tracing::error!(seq = %event.event_seq, error = %e, "持久化失败,Runtime 进入拒写态");
                    self.persist_poisoned = true;
                    // 降级 B 态可观测(T7 规格 §5.7):持久写路径故障告警
                    // (事件尽力入内存分发;持久恢复 = 重启,M8 部署形态收口)
                    self.bus.append(EventEnvelope::new(
                        self.bus.next_seq(),
                        EventType::BusDegraded,
                        self.now_ts(),
                        None,
                        None,
                        None,
                        serde_json::json!({
                            "reason": format!("persist write failed: {e}"),
                            "component": "event_log",
                        }),
                    ));
                }
            }
        }
        self.bus.append(event.clone());
        // M5.4:task.* 事件增量入 Task Board 投影(与持久化同一单写者时点,
        // 投影永远可丢弃后自事件日志重建——增量与重建两条路径等价有测试)
        self.task_board.apply(&event);
        // M5-T7:任务相关事实事件刷新停滞检测的进度信号
        if matches!(
            event.event_type,
            EventType::TaskCreated
                | EventType::TaskStateChanged
                | EventType::TaskMemberAdded
                | EventType::TaskBudgetIncreased
        ) && let Some(tid) = event.payload["task_id"].as_str()
        {
            self.watchdog
                .mark_progress(tid, self.config.clock.now(), event.event_seq);
        }
        event
    }

    /// operation 终态落定 + operation.state.changed 事件(reason_code = guard 名)。
    fn settle_operation(&mut self, op_id: &BmId, to: OperationState, error: Option<WireError>) {
        let now = self.now_ts();
        let (session_id, agent_id, from, to, reason) = {
            let op = self.operations.get_mut(op_id).expect("operation 必然存在");
            let (from, to, reason) = op.settle(to, error, now);
            (op.session_id.clone(), op.agent_id.clone(), from, to, reason)
        };
        self.emit(
            EventType::OperationStateChanged,
            Some(session_id),
            Some(agent_id),
            Some(op_id.clone()),
            serde_json::json!({
                "operation_id": op_id.as_str(),
                "from": from.as_str(),
                "to": to.as_str(),
                "reason_code": reason,
            }),
        );
    }

    /// 回合失败的统一收口:错误日志 → agent failed → operation failed → agent.failed。
    fn fail_turn(&mut self, operation_id: &BmId, code: ErrorCode, message: String) {
        let now = self.now_ts();
        let (session_id, agent_id, request_id, agent_state) = {
            let op = &self.operations[operation_id];
            let a = &self.agents[&op.agent_id];
            (
                op.session_id.clone(),
                op.agent_id.clone(),
                op.request_id.clone(),
                a.state.as_str().to_string(),
            )
        };
        self.exec_log.record(crate::exec_log::LogRecord {
            kind: LogKind::Error,
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            operation_id: operation_id.clone(),
            request_id: Some(request_id),
            agent_state,
            detail: serde_json::json!({ "error_code": code.as_str(), "message": message }),
            ts: now,
        });
        {
            let a = self.agents.get_mut(&agent_id).expect("存在");
            a.transition(AgentState::Failed);
        }
        let mut err = WireError::new(code, message);
        // 回合已收口,运行时不会再自动重发 → retryable=false(GT-B 信封语义)
        err.retryable = false;
        self.settle_operation(operation_id, OperationState::Failed, Some(err));
        self.emit(
            EventType::AgentFailed,
            Some(session_id),
            Some(agent_id.clone()),
            Some(operation_id.clone()),
            serde_json::json!({
                "agent_id": agent_id.as_str(),
                "operation_id": operation_id.as_str(),
                "error_code": code.as_str(),
            }),
        );
    }

    fn receipt_of(&self, op: &Operation) -> Receipt {
        Receipt {
            operation_id: op.id.clone(),
            request_id: op.request_id.clone(),
            principal: Principal::User,
            task_type: TaskType::AgentTurn,
            state: op.state,
            created_at: op.created_at.clone(),
            completed_at: op.completed_at.clone(),
            action_summary: op.action_summary.clone(),
            result_reference: op.result_reference.clone(),
            error: op.error.clone(),
        }
    }
}

async fn core_loop(mut world: World, mut rx: mpsc::Receiver<Cmd>) {
    while let Some(cmd) = rx.recv().await {
        // 停机后进入只读残存态:事件流与收据仍可查询(INV-6/9 精神),
        // 业务命令一律拒绝。
        if world.stopped {
            match cmd {
                Cmd::EventsAll { resp } => {
                    let events = match &world.store {
                        Some(store) => store.replay_since(0).unwrap_or_default(),
                        None => world.bus.events().to_vec(),
                    };
                    let _ = resp.send(events);
                }
                Cmd::GetOperation { params, resp } => {
                    let _ = resp.send(handle_get_operation(&world, params));
                }
                Cmd::Stop { resp, .. } => {
                    let _ = resp.send(());
                }
                other => reply_unavailable(other),
            }
            continue;
        }
        match cmd {
            Cmd::SessionCreate {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_session_create(&mut world, request_id, params));
            }
            Cmd::SessionResume {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_session_resume(&mut world, request_id, params));
            }
            Cmd::SessionClose {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_session_close(&mut world, request_id, params));
            }
            Cmd::EventsPoll { params, resp } => {
                let _ = resp.send(handle_events_poll(&world, params));
            }
            Cmd::SendInput {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_send_input(&mut world, request_id, params));
            }
            Cmd::TaskAutorun {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_task_autorun_start(&mut world, request_id, params));
            }
            Cmd::CapabilitiesRegister { entries, resp } => {
                let _ = resp.send(handle_capabilities_register(&mut world, entries));
            }
            Cmd::ProviderDelta {
                operation_id,
                delta,
            } => {
                let idx = {
                    let e = world
                        .model_delta_seq
                        .entry(operation_id.clone())
                        .or_insert(0);
                    let v = *e;
                    *e += 1;
                    v
                };
                // 会话归属随收据回填(events.poll 按会话过滤,X-02 隔离纪律)
                let session_id = world
                    .operations
                    .get(&operation_id)
                    .map(|o| o.session_id.clone());
                world.emit(
                    EventType::ModelContentDelta,
                    session_id,
                    None,
                    Some(operation_id.clone()),
                    serde_json::json!({
                        "operation_id": operation_id.as_str(),
                        "index": idx,
                        "delta": content_trunc(&delta),
                    }),
                );
            }
            Cmd::Cancel { params, resp } => {
                let _ = resp.send(handle_cancel(&world, params));
            }
            Cmd::RecoverySettle {
                operation_id,
                verdict,
                resp,
            } => {
                let _ = resp.send(handle_recovery_settle(&mut world, operation_id, verdict));
            }
            Cmd::CapabilityCall {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_capability_call(&mut world, request_id, params));
            }
            Cmd::ApprovalList { params, resp } => {
                let _ = resp.send(handle_approval_list(&mut world, params));
            }
            Cmd::ApprovalRespond {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_approval_respond(&mut world, request_id, params));
            }
            Cmd::TaskCreate {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_task_create(&mut world, request_id, params));
            }
            Cmd::TaskLifecycle {
                request_id,
                action,
                params,
                resp,
            } => {
                let _ = resp.send(handle_task_lifecycle(
                    &mut world, request_id, action, params,
                ));
            }
            Cmd::TaskList { params, resp } => {
                let _ = resp.send(handle_task_list(&world, params));
            }
            Cmd::ButlerRevoke { reason, resp } => {
                let _ = resp.send(handle_butler_revoke(&mut world, reason));
            }
            Cmd::WorkerCall {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_worker_call(&mut world, request_id, params));
            }
            Cmd::TaskSpawnMember { task_id, resp } => {
                let _ = resp.send(handle_task_spawn_member(&mut world, task_id));
            }
            Cmd::TaskSpawnSubtask { params, resp } => {
                let _ = resp.send(handle_task_spawn_subtask(&mut world, params));
            }
            Cmd::TaskRemoveMember { params, resp } => {
                let _ = resp.send(handle_task_remove_member(&mut world, params));
            }
            Cmd::TaskCollect { task_id, resp } => {
                let _ = resp.send(handle_task_collect(&world, task_id));
            }
            Cmd::TaskBudgetIncrease {
                task_id,
                max_tool_calls,
                resp,
            } => {
                let _ = resp.send(handle_task_budget_increase(
                    &mut world,
                    task_id,
                    max_tool_calls,
                ));
            }
            Cmd::WatchdogScan { resp } => {
                let n = world.watchdog_scan_now();
                let _ = resp.send(Ok(n));
            }
            Cmd::ProviderCall {
                operation_id,
                result,
            } => {
                handle_provider_call(&mut world, operation_id, result);
            }
            Cmd::CapabilityCancel {
                request_id: _,
                params,
                resp,
            } => {
                let _ = resp.send(handle_capability_cancel(&mut world, params));
            }
            Cmd::GetOpResult { operation_id, resp } => {
                let _ = resp.send(Ok(world.op_results.get(&operation_id).cloned()));
            }
            Cmd::ProviderProgress {
                operation_id,
                progress,
                total,
                message,
            } => {
                handle_provider_progress(&mut world, operation_id, progress, total, message);
            }
            Cmd::TaskReportCompletion {
                task_id,
                claim_summary,
                operation_id,
                resp,
            } => {
                let _ = resp.send(handle_task_report_completion(
                    &mut world,
                    task_id,
                    claim_summary,
                    operation_id,
                ));
            }
            Cmd::TaskGet { params, resp } => {
                let _ = resp.send(handle_task_get(&world, params));
            }
            Cmd::GetOperation { params, resp } => {
                let _ = resp.send(handle_get_operation(&world, params));
            }
            Cmd::EventsAll { resp } => {
                let events = match &world.store {
                    Some(store) => store.replay_since(0).unwrap_or_default(),
                    None => world.bus.events().to_vec(),
                };
                let _ = resp.send(events);
            }
            Cmd::Stop { reason, resp } => {
                handle_stop(&mut world, &mut rx, reason, resp).await;
            }
            Cmd::Turn(event) => handle_turn_event(&mut world, event),
            // W5:成功回合的对话台账回写(历史回喂的数据源)
            Cmd::RememberTurn {
                session_id,
                user,
                assistant,
            } => crate::runtime::turn::remember_turn(&mut world, session_id, user, assistant),
        }
        // M5-T7:Watchdog 节拍扫描(每条命令处理后检查是否到期;
        // 事实事件产出,不推断编排下一步)
        world.maybe_watchdog_scan();
    }
}

/// T7 事件形状校验(持久前):类型已在 EventEnvelope::new 层锁定注册表;
/// 此处拒绝命令语义形状(事件 = 已发生事实,不是请求;ADR-0001 条件 7 G1)。
/// 禁字段为保守清单:事件 payload 不应出现「请执行」形状的键。
pub(crate) fn validate_event_shape(
    ty: &EventType,
    payload: &serde_json::Value,
) -> Result<(), String> {
    const FORBIDDEN: [&str; 4] = [
        "requested_action",
        "instruction",
        "command",
        "please_execute",
    ];
    let Some(obj) = payload.as_object() else {
        return Ok(());
    };
    for k in FORBIDDEN {
        if obj.contains_key(k) {
            return Err(format!("事件 {ty} 携带命令语义字段 '{k}'"));
        }
    }
    Ok(())
}

// ---- 会话与 Agent ----------------------------------------------------------

// ---- 机械拆分子模块挂载(2026-08-30;路径经 re-export 保持零变化) ----

mod cmd;
mod handle;
mod handlers;
mod provider_health;
mod task_ops;
#[cfg(test)]
mod tests;
mod turn;

pub use handle::RuntimeHandle;
pub use provider_health::ProviderHealth;
pub use task_ops::{RemoveMemberParams, SpawnMemberParams, SpawnSubtaskParams, WorkerCallParams};

use cmd::*;
use handlers::*;
use provider_health::*;
use task_ops::*;
use turn::*;
