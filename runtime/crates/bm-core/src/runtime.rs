//! Runtime 核心循环:全部事件与日志写入只在唯一的循环任务内发生(单写者),
//! 保证 event_seq/log_seq 的全局单调(INV-3/INV-4 的结构前提)。
//! 回合任务只通过内部命令通道回报,不直接改状态。

use bm_persist::EventStore;

use crate::bus::EventBus;
use crate::clock::Clock;
use crate::exec_log::ExecutionLog;
use crate::ports::{ModelConnector, SecretStore};
use crate::state::{Agent, Operation, Session, budget_from_spec};
use crate::{CoreError, CoreResult};
use bm_contract::budget::BudgetScope;
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

enum Cmd {
    SessionCreate {
        request_id: BmId,
        params: SessionCreateParams,
        resp: oneshot::Sender<CoreResult<SessionCreateResult>>,
    },
    SessionResume {
        request_id: BmId,
        params: SessionResumeParams,
        resp: oneshot::Sender<CoreResult<SessionResumeResult>>,
    },
    SessionClose {
        request_id: BmId,
        params: SessionCloseParams,
        resp: oneshot::Sender<CoreResult<SessionCloseResult>>,
    },
    EventsPoll {
        params: EventsPollParams,
        resp: oneshot::Sender<CoreResult<EventsPollResult>>,
    },
    SendInput {
        request_id: BmId,
        params: SendInputParams,
        resp: oneshot::Sender<CoreResult<Receipt>>,
    },
    Cancel {
        params: CancelParams,
        resp: oneshot::Sender<CoreResult<CancelResult>>,
    },
    /// 恢复裁定(M2.6 内部命令,M4 起升级为合同方法;INV-10/11 的用户入口)
    RecoverySettle {
        operation_id: BmId,
        verdict: RecoveryVerdict,
        resp: oneshot::Sender<CoreResult<Receipt>>,
    },
    GetOperation {
        params: GetOperationParams,
        resp: oneshot::Sender<CoreResult<Receipt>>,
    },
    /// 诊断端口(非 Wire 方法):全量事件流,测试/回放用。排空期照常应答。
    EventsAll {
        resp: oneshot::Sender<Vec<EventEnvelope>>,
    },
    Stop {
        reason: String,
        resp: oneshot::Sender<()>,
    },
    Turn(TurnEvent),
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
        // 写穿(M2 规格 §5.1):record 内部固定 ①日志+flush → ②物化 → ③位点。
        // 失败即进入拒写态:内存视图与持久层自此分叉,以持久层为准(重启重建)。
        #[allow(clippy::collapsible_if)] // 三重条件展平反而难读
        if let Some(store) = &self.store {
            if !self.persist_poisoned {
                if let Err(e) = store.record(&event) {
                    tracing::error!(seq = %event.event_seq, error = %e, "持久化失败,Runtime 进入拒写态");
                    self.persist_poisoned = true;
                }
            }
        }
        self.bus.append(event.clone());
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

/// 运行句柄:进程内 Wire API(M1 方法集合)。
#[derive(Clone)]
pub struct RuntimeHandle {
    tx: mpsc::Sender<Cmd>,
}

impl RuntimeHandle {
    /// 启动 Runtime:恢复持久状态(如配置了 store)→ runtime.started →
    /// 中断清点的审计事件 → runtime.recovered,随后进入核心循环。
    pub async fn start(config: RuntimeConfig) -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>(1024);
        let exec_log = Arc::new(ExecutionLog::new(config.data_dir.as_deref()));
        let started_at = config.clock.now();
        let mut world = World {
            bus: EventBus::new(),
            exec_log,
            in_flight: HashMap::new(),
            sessions: HashMap::new(),
            agents: HashMap::new(),
            operations: HashMap::new(),
            started_at,
            started_instant: Instant::now(),
            draining: false,
            stopped: false,
            persist_poisoned: false,
            tx: tx.clone(),
            store: config.store.clone(),
            config,
        };

        // T3 启动恢复:修复窗口 → 行装配 → 中断清点。恢复失败 = 拒绝启动
        // (宁可拒开,不带残缺状态服务)。
        let mut pending_interrupts: Vec<(BmId, BmId, String)> = Vec::new();
        let mut agents_to_resume: Vec<(BmId, Option<BmId>)> = Vec::new();
        let mut report = None;
        if let Some(store) = world.store.clone() {
            let r = store.recover().expect("启动恢复失败,拒绝启动(宁可拒开)");
            let rows = store.load_rows().expect("规范状态行装配失败,拒绝启动");
            world.load_world_rows(rows, &mut pending_interrupts, &mut agents_to_resume);
            // seq 分配器重同步到持久日志末尾之后:跨重启 seq 连续(INV-3)
            let log_last = r.last_applied_seq.max(store.last_log_seq().unwrap_or(0));
            world.bus.resync_to(log_last + 1);
            // 空库首启 = 新启动,不是恢复:不产生 runtime.recovered 噪音事件
            if log_last > 0 {
                report = Some(r);
            }
        }

        world.emit(
            EventType::RuntimeStarted,
            None,
            None,
            None,
            serde_json::json!({
                "pid": std::process::id(),
                "version": world.config.version,
                "started_at": format_ts(world.started_at),
            }),
        );

        // 中断清点(留审计事件,ADR-0004 恢复语义):崩溃时未终态的 operation
        // 走 running→interrupted(runtime_crash_before_terminal);agent 走
        // waiting_model/running→interrupted→resuming→running。
        // T7 claim(M2.6):NoEffect 域且输入原文在受保护存储中时,自动
        // interrupted→running(recovery_replay_ok)并重驱回合 → 幂等续跑;
        // 无输入上下文或 outcome_unknown 者,留给裁定入口(recovery_settle)。
        let mut claim_redrive: Vec<(BmId, BmId, String)> = Vec::new();
        for (op_id, agent_id, op_state) in pending_interrupts.drain(..) {
            if op_state == "running" {
                world.settle_operation(&op_id, OperationState::Interrupted, None);
            }
            world.emit(
                EventType::AgentInterrupted,
                None,
                Some(agent_id.clone()),
                Some(op_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": op_id.as_str(),
                    "reason": "runtime_recovery",
                }),
            );
            {
                let a = world.agents.get_mut(&agent_id).expect("恢复行必有 agent");
                a.transition(AgentState::Interrupted);
                a.transition(AgentState::Resuming);
                a.transition(AgentState::Running);
            }
            world.emit(
                EventType::AgentResumed,
                None,
                Some(agent_id.clone()),
                Some(op_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": op_id.as_str(),
                }),
            );
            // claim 判定:有输入原文才可续跑
            let content = world
                .store
                .as_ref()
                .and_then(|s| s.op_input(op_id.as_str()).ok())
                .flatten();
            if let Some(content) = content {
                claim_redrive.push((op_id, agent_id, content));
            }
        }

        // 无 running op 但停在中间态的 agent:同样走中断恢复(outcome_unknown
        // 场景:op 留给裁定,agent 恢复可接单)
        for (agent_id, latest_op) in agents_to_resume.drain(..) {
            // op 流可能已恢复该 agent:先查内存态,避免重复中断事件
            if world.agents.get(&agent_id).map(|a| a.state) == Some(AgentState::Running) {
                continue;
            }
            let op_hint = latest_op.or_else(|| {
                world
                    .operations
                    .values()
                    .filter(|o| o.agent_id == agent_id)
                    .map(|o| o.id.clone())
                    .next()
            });
            world.emit(
                EventType::AgentInterrupted,
                None,
                Some(agent_id.clone()),
                op_hint.clone(),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": op_hint.as_ref().map(|o| o.as_str()),
                    "reason": "runtime_recovery",
                }),
            );
            {
                let a = world.agents.get_mut(&agent_id).expect("恢复行必有 agent");
                if AgentState::can_transition(a.state, AgentState::Interrupted) {
                    a.transition(AgentState::Interrupted);
                }
                a.transition(AgentState::Resuming);
                a.transition(AgentState::Running);
            }
            world.emit(
                EventType::AgentResumed,
                None,
                Some(agent_id.clone()),
                op_hint.clone(),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": op_hint.as_ref().map(|o| o.as_str()),
                }),
            );
        }

        // claim 续跑:interrupted→running(recovery_replay_ok)后重驱回合
        for (op_id, agent_id, content) in claim_redrive {
            world.settle_operation(&op_id, OperationState::Running, None);
            let agent = world
                .agents
                .get(&agent_id)
                .cloned()
                .expect("恢复行必有 agent");
            // 重驱即再发模型调用:running→waiting_model(与 send_input 同构)
            {
                let a = world.agents.get_mut(&agent_id).expect("存在");
                a.transition(AgentState::WaitingModel);
            }
            world.emit(
                EventType::AgentResumed,
                None,
                Some(agent_id.clone()),
                Some(op_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": op_id.as_str(),
                }),
            );
            world
                .in_flight
                .insert(op_id.clone(), CancellationToken::new());
            spawn_turn(&mut world, &agent, &op_id, content);
        }

        if let Some(r) = report {
            world.emit(
                EventType::RuntimeRecovered,
                None,
                None,
                None,
                serde_json::json!({
                    "last_applied_seq": r.last_applied_seq,
                    "replayed": r.replayed,
                    "interrupted_recovered": r.interrupted_recovered,
                }),
            );
        }

        tokio::spawn(core_loop(world, rx));
        Self { tx }
    }

    pub async fn session_create(
        &self,
        request_id: BmId,
        params: SessionCreateParams,
    ) -> CoreResult<SessionCreateResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::SessionCreate {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn session_resume(
        &self,
        request_id: BmId,
        params: SessionResumeParams,
    ) -> CoreResult<SessionResumeResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::SessionResume {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn session_close(
        &self,
        request_id: BmId,
        params: SessionCloseParams,
    ) -> CoreResult<SessionCloseResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::SessionClose {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn events_poll(&self, params: EventsPollParams) -> CoreResult<EventsPollResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::EventsPoll { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn send_input(
        &self,
        request_id: BmId,
        params: SendInputParams,
    ) -> CoreResult<Receipt> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::SendInput {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    pub async fn agent_cancel(&self, params: CancelParams) -> CoreResult<CancelResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Cancel { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 收据查询幂等(INV-9):终态后任意多次调用结果一致。
    pub async fn operations_get(&self, params: GetOperationParams) -> CoreResult<Receipt> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::GetOperation { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 全量事件流(诊断端口,M1 供回放器使用;M3 由 watch/cursor 取代)。
    #[doc(hidden)]
    pub async fn events_all(&self) -> Vec<EventEnvelope> {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Cmd::EventsAll { resp: tx }).await.is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    /// 恢复裁定(M2.6 内部入口;INV-10:普通重试不得触碰 outcome_unknown)。
    /// 迁移表合法边:outcome_unknown→succeeded/failed(external_verification OR
    /// user_ruling);interrupted→running(claim)/cancelled(user_ruling)。
    #[doc(hidden)]
    pub async fn recovery_settle(
        &self,
        operation_id: BmId,
        verdict: RecoveryVerdict,
    ) -> CoreResult<Receipt> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::RecoverySettle {
                operation_id,
                verdict,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 停机:排空进行中回合(不取消,INV-12),发 stopping/stopped 事件。
    pub async fn stop(&self, reason: impl Into<String>) {
        let (tx, rx) = oneshot::channel();
        if self
            .tx
            .send(Cmd::Stop {
                reason: reason.into(),
                resp: tx,
            })
            .await
            .is_err()
        {
            return;
        }
        let _ = rx.await;
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
        }
    }
}

// ---- 会话与 Agent ----------------------------------------------------------

fn handle_session_create(
    w: &mut World,
    _request_id: BmId,
    params: SessionCreateParams,
) -> CoreResult<SessionCreateResult> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝新会话".into(),
        ));
    }
    let spec = &params.agent;
    if spec.name.is_empty() || spec.name.len() > 64 || spec.model_chain.is_empty() {
        return Err(CoreError::validation("agent 描述不完整"));
    }
    for m in &spec.model_chain {
        bm_contract::connector::validate_model_id(m).map_err(CoreError::validation)?;
    }

    let now = w.now_ts();
    let session_id = w.config.id_gen.next_id("sess");
    let agent_id = w.config.id_gen.next_id("agent");

    let mut session = Session {
        id: session_id.clone(),
        agent_id: agent_id.clone(),
        state: SessionState::Created,
        created_at: now.clone(),
    };
    // created→active(surface_attached):M1 进程内直调即视为已挂接。
    session.transition(SessionState::Active);
    w.sessions.insert(session_id.clone(), session);

    let budget = budget_from_spec(spec.budget.as_ref());
    w.agents.insert(
        agent_id.clone(),
        Agent {
            id: agent_id.clone(),
            session_id: session_id.clone(),
            name: spec.name.clone(),
            model_chain: spec.model_chain.clone(),
            state: AgentState::Created,
            budget,
        },
    );
    // created→starting→running(agent_start + model_binding_ready):无事件(规格 §8.6)。
    {
        let agent = w.agents.get_mut(&agent_id).expect("已插入");
        agent.transition(AgentState::Starting);
        agent.transition(AgentState::Running);
    }

    w.emit(
        EventType::SessionCreated,
        Some(session_id.clone()),
        None,
        None,
        serde_json::json!({
            "session_id": session_id.as_str(),
            "agent_id": agent_id.as_str(),
        }),
    );
    let budget_limits = &w.agents[&agent_id].budget;
    w.emit(
        EventType::AgentCreated,
        Some(session_id.clone()),
        Some(agent_id.clone()),
        None,
        serde_json::json!({
            "agent_id": agent_id.as_str(),
            "session_id": session_id.as_str(),
            "model_chain": spec.model_chain,
            "budget": {"max_tokens": budget_limits.max_tokens, "max_turns": budget_limits.max_turns},
        }),
    );

    Ok(SessionCreateResult {
        session_id,
        agent_id,
        created_at: now,
        resume_cursor: Cursor {
            event_seq: w.bus.last_seq(),
        },
    })
}

fn handle_session_resume(
    w: &mut World,
    _request_id: BmId,
    params: SessionResumeParams,
) -> CoreResult<SessionResumeResult> {
    if w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "持久层故障,Runtime 拒写".into(),
        ));
    }
    let session = w
        .sessions
        .get(&params.session_id)
        .ok_or_else(|| CoreError::validation("session 不存在"))?
        .clone();
    if session.state == SessionState::Closed {
        return Err(CoreError::validation("session 已关闭,不可 resume"));
    }
    let since = params.since_seq.unwrap_or(0);
    let (events, _last, _) = w.events_for_session(&params.session_id, since, u32::MAX);
    let agent_state = w
        .agents
        .get(&session.agent_id)
        .map(|a| a.state)
        .unwrap_or(AgentState::Failed);

    w.emit(
        EventType::SessionResumed,
        Some(params.session_id.clone()),
        None,
        None,
        serde_json::json!({
            "session_id": params.session_id.as_str(),
            "since_seq": since,
            "replayed": events.len(),
        }),
    );

    Ok(SessionResumeResult {
        // M1 无 detached 路径(M3 Surface 断连引入);保持当前态。
        session_state: SessionState::Active,
        agent_state,
        last_event_seq: w.bus.last_seq(),
        events,
    })
}

fn handle_session_close(
    w: &mut World,
    _request_id: BmId,
    params: SessionCloseParams,
) -> CoreResult<SessionCloseResult> {
    if w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "持久层故障,Runtime 拒写".into(),
        ));
    }
    let agent_final_state;
    {
        let session = w
            .sessions
            .get_mut(&params.session_id)
            .ok_or_else(|| CoreError::validation("session 不存在"))?;
        if session.state == SessionState::Closed {
            return Err(CoreError::validation("session 已关闭"));
        }
        session.transition(SessionState::Closed);
        let agent = w.agents.get(&session.agent_id).expect("session 必有 agent");
        agent_final_state = agent.state.as_str().to_string();
    }
    // close 只关会话,不取消进行中的回合(INV-6);in_flight 不动。
    let reason = params.reason.unwrap_or_else(|| "user_request".into());
    w.emit(
        EventType::SessionClosed,
        Some(params.session_id.clone()),
        None,
        None,
        serde_json::json!({
            "session_id": params.session_id.as_str(),
            "reason": reason,
        }),
    );
    Ok(SessionCloseResult {
        closed_at: w.now_ts(),
        agent_final_state,
    })
}

fn handle_events_poll(w: &World, params: EventsPollParams) -> CoreResult<EventsPollResult> {
    let limit = params.limit.unwrap_or(100).clamp(1, 1000);
    let (events, last_seq, has_more) =
        w.events_for_session(&params.session_id, params.since_seq, limit);
    Ok(EventsPollResult {
        events,
        last_seq,
        has_more,
    })
}

// ---- 回合 ------------------------------------------------------------------

fn handle_send_input(
    w: &mut World,
    request_id: BmId,
    params: SendInputParams,
) -> CoreResult<Receipt> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障".into(),
        ));
    }
    if params.content.is_empty() || params.content.len() > 100_000 {
        return Err(CoreError::validation("content 长度越界(1..=100000 字节)"));
    }

    let session = w
        .sessions
        .get(&params.session_id)
        .ok_or_else(|| CoreError::validation("session 不存在"))?
        .clone();
    if session.state == SessionState::Closed {
        return Err(CoreError::validation("session 已关闭"));
    }
    let agent = w
        .agents
        .get(&params.agent_id)
        .ok_or_else(|| CoreError::validation("agent 不存在"))?
        .clone();
    if agent.session_id != session.id {
        return Err(CoreError::validation("agent 不属于该 session"));
    }
    if agent.state != AgentState::Running {
        return Err(CoreError::validation("agent 不在可接单状态"));
    }

    // 强制点①(规格 §8.2):预算拒绝不创建 operation。
    match agent.budget.check(true) {
        crate::budget::Verdict::ExceededTokens | crate::budget::Verdict::ExceededTurns => {
            let msg = match agent.budget.check(false) {
                crate::budget::Verdict::ExceededTokens => "剩余预算不足,回合不发起",
                _ => "回合数已用尽,回合不发起",
            };
            w.emit(
                EventType::BudgetExceeded,
                Some(session.id.clone()),
                Some(agent.id.clone()),
                None,
                serde_json::json!({
                    "agent_id": agent.id.as_str(),
                    "scope": BudgetScope::Agent.as_str(),
                    "used_tokens": agent.budget.used_tokens,
                    "limit_tokens": agent.budget.max_tokens,
                }),
            );
            return Err(CoreError::Semantic(ErrorCode::BudgetExceeded, msg.into()));
        }
        crate::budget::Verdict::Allow => {}
    }

    let now = w.now_ts();
    let operation_id = w.config.id_gen.next_id("op");
    let turn_index = agent.budget.turns_used + 1;

    // not_started→running(dispatch_accepted):由收据承载,不发事件(规格 §8.1)。
    let operation = Operation {
        id: operation_id.clone(),
        request_id: request_id.clone(),
        session_id: session.id.clone(),
        agent_id: agent.id.clone(),
        state: OperationState::NotStarted,
        turn_index,
        created_at: now.clone(),
        completed_at: None,
        action_summary: format!("Agent 回合进行中(第 {turn_index} 回)"),
        result_reference: None,
        error: None,
    }
    .dispatch();
    w.operations.insert(operation_id.clone(), operation);

    let model_id0 = agent.model_chain[0].clone();
    w.emit(
        EventType::AgentTurnStarted,
        Some(session.id.clone()),
        Some(agent.id.clone()),
        Some(operation_id.clone()),
        serde_json::json!({
            "agent_id": agent.id.as_str(),
            "operation_id": operation_id.as_str(),
            "turn_index": turn_index,
        }),
    );
    // Execution Log:agent.turn(输入只留摘要,基线 8.4;A4:载荷原文不入日志)
    {
        let digest = Sha256::digest(params.content.as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        w.exec_log.record(crate::exec_log::LogRecord {
            kind: LogKind::AgentTurn,
            session_id: session.id.clone(),
            agent_id: agent.id.clone(),
            operation_id: operation_id.clone(),
            request_id: Some(request_id.clone()),
            agent_state: AgentState::Running.as_str().to_string(),
            detail: serde_json::json!({
                "turn_index": turn_index,
                "input_digest": format!("sha256:{hex}"),
                "input_bytes": params.content.len(),
            }),
            ts: now.clone(),
        });
    }

    // 输入原文入受保护存储(A4:不进事件/日志),供崩溃后 claim 幂等续跑(M2.6)
    #[allow(clippy::collapsible_if)] // 与写穿主路径同构,保持三段式可读
    if let Some(store) = &w.store {
        if let Err(e) = store.save_op_input(operation_id.as_str(), &params.content) {
            tracing::error!(error = %e, op = %operation_id, "输入持久化失败,进入拒写态");
            w.persist_poisoned = true;
            return Err(CoreError::Semantic(
                ErrorCode::Internal,
                "输入持久化失败".into(),
            ));
        }
    }

    // running→waiting_model(model_invoke_issued)
    {
        let a = w.agents.get_mut(&agent.id).expect("存在");
        a.transition(AgentState::WaitingModel);
    }
    w.emit(
        EventType::AgentWaitingModel,
        Some(session.id.clone()),
        Some(agent.id.clone()),
        Some(operation_id.clone()),
        serde_json::json!({
            "agent_id": agent.id.as_str(),
            "operation_id": operation_id.as_str(),
            "model_id": model_id0,
        }),
    );

    // 强制点②(pre_invoke_check):M1 中与①同账本,防御性保留(基线 9.7)。
    if agent.budget.check(false) != crate::budget::Verdict::Allow {
        w.fail_turn(
            &operation_id,
            ErrorCode::BudgetExceeded,
            "模型调用前预算检查未通过".into(),
        );
        return Err(CoreError::Semantic(
            ErrorCode::BudgetExceeded,
            "模型调用前预算检查未通过".into(),
        ));
    }

    spawn_turn(w, &agent, &operation_id, params.content);

    Ok(w.receipt_of(&w.operations[&operation_id]))
}

fn spawn_turn(w: &mut World, agent: &Agent, operation_id: &BmId, content: String) {
    let cancel = CancellationToken::new();
    w.in_flight.insert(operation_id.clone(), cancel.clone());

    let connector = w.config.connector.clone();
    let clock = w.config.clock.clone();
    let chain = agent.model_chain.clone();
    let agent_id = agent.id.clone();
    let remaining = agent.budget.remaining_tokens();
    let max_attempts = w
        .config
        .max_attempts
        .unwrap_or_else(|| chain.len().min(3) as u32)
        .clamp(1, 3);
    let timeout_secs = w.config.turn_timeout_secs;
    let tx = w.tx.clone();
    let op_id = operation_id.clone();

    tokio::spawn(async move {
        for attempt in 1..=max_attempts {
            let model_id = chain[((attempt - 1) as usize) % chain.len()].clone();
            let req = InvokeRequest {
                model_id: model_id.clone(),
                messages: vec![Message {
                    role: Role::User,
                    content: content.clone(),
                }],
                tools: vec![],
                params: Default::default(),
                secret_ref: default_secret_ref(&model_id),
                budget_ctx: BudgetCtx {
                    operation_id: op_id.clone(),
                    agent_id: agent_id.clone(),
                    remaining_tokens: remaining,
                },
                deadline: format_ts(clock.now() + Duration::seconds(timeout_secs)),
                attempt,
            };

            let resp = tokio::select! {
                _ = cancel.cancelled() => InvokeResponse::Failed {
                    error_code: ErrorCode::Cancelled, retryable: false, attempt, detail_ref: None,
                },
                r = connector.invoke(req, cancel.clone()) => r,
            };

            match resp {
                InvokeResponse::Completed {
                    content,
                    finish_reason: _,
                    usage,
                    model_id: mid,
                    latency_ms,
                    stream_interrupted,
                } => {
                    let _ = tx
                        .send(Cmd::Turn(TurnEvent::Completed {
                            operation_id: op_id.clone(),
                            model_id: mid,
                            attempt,
                            content,
                            usage_in: usage.tokens_in,
                            usage_out: usage.tokens_out,
                            latency_ms,
                            stream_interrupted,
                        }))
                        .await;
                    return;
                }
                InvokeResponse::Failed {
                    error_code,
                    retryable,
                    attempt,
                    detail_ref: _,
                } => {
                    if error_code == ErrorCode::Cancelled {
                        // 显式取消:回合边界落定为 cancelled(INV-12 唯一入口)。
                        let _ = tx
                            .send(Cmd::Turn(TurnEvent::Cancelled {
                                operation_id: op_id.clone(),
                            }))
                            .await;
                        return;
                    }
                    let _ = tx
                        .send(Cmd::Turn(TurnEvent::AttemptFailed {
                            operation_id: op_id.clone(),
                            model_id,
                            attempt,
                            error_code,
                        }))
                        .await;
                    if !retryable || attempt == max_attempts {
                        let _ = tx
                            .send(Cmd::Turn(TurnEvent::ChainExhausted {
                                operation_id: op_id,
                                error_code,
                            }))
                            .await;
                        return;
                    }
                }
            }
        }
    });
}

fn handle_recovery_settle(
    w: &mut World,
    operation_id: BmId,
    verdict: RecoveryVerdict,
) -> CoreResult<Receipt> {
    if w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "持久层故障,Runtime 拒写".into(),
        ));
    }
    let from = {
        let op = w
            .operations
            .get(&operation_id)
            .ok_or_else(|| CoreError::validation("operation 不存在"))?;
        op.state
    };
    // INV-10/11:只允许恢复态被裁定,且只走迁移表合法边
    let target = match (from, verdict) {
        (OperationState::OutcomeUnknown, RecoveryVerdict::Succeeded) => {
            Some(OperationState::Succeeded)
        }
        (OperationState::OutcomeUnknown, RecoveryVerdict::Failed) => Some(OperationState::Failed),
        (OperationState::Interrupted, RecoveryVerdict::ClaimRun) => Some(OperationState::Running),
        (OperationState::Interrupted, RecoveryVerdict::Cancelled) => {
            Some(OperationState::Cancelled)
        }
        (OperationState::OutcomeUnknown, RecoveryVerdict::Cancelled) => {
            return Err(CoreError::validation(
                "outcome_unknown 无 →cancelled 边:只能经核验落 succeeded/failed(INV-11)",
            ));
        }
        (OperationState::Interrupted, RecoveryVerdict::Succeeded)
        | (OperationState::Interrupted, RecoveryVerdict::Failed) => {
            return Err(CoreError::validation(
                "interrupted 无直达 succeeded/failed 的边:claim 续跑或裁定取消",
            ));
        }
        _ => {
            return Err(CoreError::validation(
                "仅恢复态(outcome_unknown/interrupted)可裁定",
            ));
        }
    };
    let target = target.expect("上表已穷尽");

    // claim 续跑:需要受保护存储中的输入原文
    if target == OperationState::Running {
        let content = w
            .store
            .as_ref()
            .and_then(|s| s.op_input(operation_id.as_str()).ok())
            .flatten()
            .ok_or_else(|| CoreError::validation("无输入上下文,不可续跑(裁定取消或核验结论)"))?;
        w.settle_operation(&operation_id, OperationState::Running, None);
        let agent = w
            .agents
            .get(&w.operations[&operation_id].agent_id)
            .cloned()
            .expect("存在");
        {
            let agent_id = agent.id.clone();
            let a = w.agents.get_mut(&agent_id).expect("存在");
            a.transition(AgentState::WaitingModel);
        }
        spawn_turn(w, &agent, &operation_id, content);
        Ok(w.receipt_of(&w.operations[&operation_id]))
    } else {
        let error = match target {
            OperationState::Failed => {
                let mut e =
                    WireError::new(ErrorCode::OutcomeUnknown, "恢复裁定:按失败收口".to_string());
                e.retryable = false;
                Some(e)
            }
            OperationState::Cancelled => Some(WireError::new(
                ErrorCode::Cancelled,
                "恢复裁定:用户裁定取消".to_string(),
            )),
            _ => None,
        };
        w.settle_operation(&operation_id, target, error);
        Ok(w.receipt_of(&w.operations[&operation_id]))
    }
}

fn handle_cancel(w: &World, params: CancelParams) -> CoreResult<CancelResult> {
    let op = w
        .operations
        .get(&params.operation_id)
        .ok_or_else(|| CoreError::validation("operation 不存在"))?;
    if op.session_id != params.session_id || op.agent_id != params.agent_id {
        return Err(CoreError::validation("operation 与 session/agent 不匹配"));
    }
    if op.is_terminal() {
        return Err(CoreError::validation("operation 已到终态,不可取消"));
    }
    // 触发取消令牌;真实落定在 TurnEvent::Cancelled(回合边界)。
    if let Some(token) = w.in_flight.get(&params.operation_id) {
        token.cancel();
    }
    Ok(CancelResult {
        accepted: true,
        operation_id: params.operation_id.clone(),
    })
}

fn handle_get_operation(w: &World, params: GetOperationParams) -> CoreResult<Receipt> {
    let op = w
        .operations
        .get(&params.operation_id)
        .ok_or_else(|| CoreError::validation("operation 不存在"))?;
    Ok(w.receipt_of(op))
}

fn handle_turn_event(w: &mut World, event: TurnEvent) {
    match event {
        TurnEvent::AttemptFailed {
            operation_id,
            model_id,
            attempt,
            error_code,
        } => {
            let (session_id, agent_id, request_id, agent_state) = {
                let op = &w.operations[&operation_id];
                let a = &w.agents[&op.agent_id];
                (
                    op.session_id.clone(),
                    op.agent_id.clone(),
                    op.request_id.clone(),
                    a.state.as_str().to_string(),
                )
            };
            w.emit(
                EventType::ModelInvocationFailed,
                Some(session_id.clone()),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "operation_id": operation_id.as_str(),
                    "agent_id": agent_id.as_str(),
                    "model_id": model_id,
                    "attempt": attempt,
                    "error_code": error_code.as_str(),
                }),
            );
            w.exec_log.record(crate::exec_log::LogRecord {
                kind: LogKind::ModelInvocation,
                session_id,
                agent_id,
                operation_id,
                request_id: Some(request_id),
                agent_state,
                detail: serde_json::json!({
                    "model_id": model_id,
                    "attempt": attempt,
                    "error_code": error_code.as_str(),
                    "stream_interrupted": false,
                }),
                ts: w.now_ts(),
            });
        }
        TurnEvent::ChainExhausted {
            operation_id,
            error_code,
        } => {
            w.fail_turn(
                &operation_id,
                error_code,
                format!("模型降级链耗尽({error_code})"),
            );
            w.in_flight.remove(&operation_id);
        }
        TurnEvent::Cancelled { operation_id } => {
            // operation: running→cancelled(唯一合法入口 = 显式取消,INV-12)
            let (session_id, agent_id) = {
                let op = &w.operations[&operation_id];
                (op.session_id.clone(), op.agent_id.clone())
            };
            {
                let a = w.agents.get_mut(&agent_id).expect("存在");
                // waiting_model→stopping(explicit_cancel)→stopped(turn_boundary_reached)
                a.transition(AgentState::Stopping);
                a.transition(AgentState::Stopped);
            }
            w.settle_operation(
                &operation_id,
                OperationState::Cancelled,
                Some(WireError::new(
                    ErrorCode::Cancelled,
                    "用户显式取消".to_string(),
                )),
            );
            w.emit(
                EventType::AgentCancelled,
                Some(session_id),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": operation_id.as_str(),
                }),
            );
            w.in_flight.remove(&operation_id);
        }
        TurnEvent::Completed {
            operation_id,
            model_id,
            attempt,
            content,
            usage_in,
            usage_out,
            latency_ms,
            stream_interrupted,
        } => {
            let (session_id, agent_id, request_id, agent_state) = {
                let op = &w.operations[&operation_id];
                let a = &w.agents[&op.agent_id];
                (
                    op.session_id.clone(),
                    op.agent_id.clone(),
                    op.request_id.clone(),
                    a.state.as_str().to_string(),
                )
            };
            w.emit(
                EventType::ModelInvocationCompleted,
                Some(session_id.clone()),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "operation_id": operation_id.as_str(),
                    "agent_id": agent_id.as_str(),
                    "model_id": model_id,
                    "attempt": attempt,
                    "usage_in": usage_in,
                    "usage_out": usage_out,
                    "latency_ms": latency_ms,
                    "stream_interrupted": stream_interrupted,
                }),
            );
            w.exec_log.record(crate::exec_log::LogRecord {
                kind: LogKind::ModelInvocation,
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                operation_id: operation_id.clone(),
                request_id: Some(request_id),
                agent_state,
                detail: serde_json::json!({
                    "model_id": model_id,
                    "attempt": attempt,
                    "usage": {"tokens_in": usage_in, "tokens_out": usage_out},
                    "latency_ms": latency_ms,
                    "stream_interrupted": stream_interrupted,
                }),
                ts: w.now_ts(),
            });

            // waiting_model→running(model_response_ok)
            {
                let a = w.agents.get_mut(&agent_id).expect("存在");
                a.transition(AgentState::Running);
            }

            // 强制点③(post_invoke_accounting)
            let turn_index = w.operations[&operation_id].turn_index;
            let (ratio, warn, exceeded) = {
                let a = w.agents.get_mut(&agent_id).expect("存在");
                a.budget.account(usage_in.saturating_add(usage_out))
            };
            let used = w.agents[&agent_id].budget.used_tokens;
            let limit = w.agents[&agent_id].budget.max_tokens;
            w.exec_log.record(crate::exec_log::LogRecord {
                kind: LogKind::BudgetCheck,
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                operation_id: operation_id.clone(),
                request_id: None,
                agent_state: AgentState::Running.as_str().to_string(),
                detail: serde_json::json!({
                    "scope": BudgetScope::Agent.as_str(),
                    "used_tokens": used,
                    "limit_tokens": limit,
                    "ratio": ratio,
                }),
                ts: w.now_ts(),
            });
            if warn {
                w.emit(
                    EventType::BudgetWarning,
                    Some(session_id.clone()),
                    Some(agent_id.clone()),
                    None,
                    serde_json::json!({
                        "agent_id": agent_id.as_str(),
                        "scope": BudgetScope::Agent.as_str(),
                        "used_tokens": used,
                        "limit_tokens": limit,
                        "ratio": ratio,
                    }),
                );
            }
            if exceeded {
                w.emit(
                    EventType::BudgetExceeded,
                    Some(session_id.clone()),
                    Some(agent_id.clone()),
                    None,
                    serde_json::json!({
                        "agent_id": agent_id.as_str(),
                        "scope": BudgetScope::Agent.as_str(),
                        "used_tokens": used,
                        "limit_tokens": limit,
                    }),
                );
            }

            // running→succeeded(result_recorded)+ agent.completed
            {
                let now = w.now_ts();
                let op = w.operations.get_mut(&operation_id).expect("存在");
                op.action_summary =
                    format!("回合 {turn_index} 完成({usage_in} 入 / {usage_out} 出 token)");
                op.result_reference = Some(wire::ResultReference {
                    kind: wire::ResultRefKind::ExecutionLog,
                    r#ref: format!("log:{operation_id}"),
                });
                let _ = now;
            }
            w.settle_operation(&operation_id, OperationState::Succeeded, None);
            w.emit(
                EventType::AgentCompleted,
                Some(session_id),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": operation_id.as_str(),
                    "turn_index": turn_index,
                    "content": content,
                }),
            );
            w.in_flight.remove(&operation_id);
        }
    }
}

async fn handle_stop(
    w: &mut World,
    rx: &mut mpsc::Receiver<Cmd>,
    reason: String,
    resp: oneshot::Sender<()>,
) {
    w.emit(
        EventType::RuntimeStopping,
        None,
        None,
        None,
        serde_json::json!({ "reason": reason }),
    );
    // 排空:不取消进行中回合(INV-12),等它们自然落定。
    w.draining = true;
    while !w.in_flight.is_empty() {
        match rx.recv().await {
            Some(Cmd::Turn(event)) => handle_turn_event(w, event),
            // 收据查询只读幂等,排空期照常应答(INV-6 精神)。
            Some(Cmd::GetOperation { params, resp }) => {
                let _ = resp.send(handle_get_operation(w, params));
            }
            Some(Cmd::EventsAll { resp }) => {
                let events = match &w.store {
                    Some(store) => store.replay_since(0).unwrap_or_default(),
                    None => w.bus.events().to_vec(),
                };
                let _ = resp.send(events);
            }
            Some(other) => reply_unavailable(other),
            None => break,
        }
    }
    let uptime_ms = w.started_instant.elapsed().as_millis() as u64;
    w.emit(
        EventType::RuntimeStopped,
        None,
        None,
        None,
        serde_json::json!({ "uptime_ms": uptime_ms }),
    );
    w.stopped = true;
    let _ = resp.send(());
}

/// 排空期对非回合命令的统一拒绝(保留应答,不悬挂调用方)。
fn reply_unavailable(cmd: Cmd) {
    let err = || CoreError::Semantic(ErrorCode::Unavailable, "Runtime 排空中".into());
    match cmd {
        Cmd::SessionCreate { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::SessionResume { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::SessionClose { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::EventsPoll { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::SendInput { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::Cancel { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::GetOperation { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::RecoverySettle { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::EventsAll { resp } => {
            let _ = resp.send(Vec::new());
        }
        Cmd::Stop { resp, .. } => {
            let _ = resp.send(());
        }
        Cmd::Turn(_) => {}
    }
}
