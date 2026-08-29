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
    /// capability.call(M4):统一入口裁决 + 执行;需审批时停在 waiting_approval。
    CapabilityCall {
        request_id: BmId,
        params: wire::CapabilityCallParams,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
    /// approval.list(M4):待裁决审批列表。
    ApprovalList {
        params: wire::ApprovalListParams,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
    /// approval.respond(M4):批准(物化 Grant 并重放执行)/拒绝/取消。
    ApprovalRespond {
        request_id: BmId,
        params: wire::ApprovalRespondParams,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
    /// task.create(M5):Task 创建并启动(created→running)。
    TaskCreate {
        request_id: BmId,
        params: wire::TaskCreateParams,
        resp: oneshot::Sender<CoreResult<wire::TaskCreateResult>>,
    },
    /// task.pause / task.resume / task.stop(M5):生命周期命令。
    TaskLifecycle {
        request_id: BmId,
        action: TaskAction,
        params: wire::TaskLifecycleParams,
        resp: oneshot::Sender<CoreResult<wire::TaskStateResult>>,
    },
    /// task.list(M5):Task Board 列表(确定性序)。
    TaskList {
        params: wire::TaskListParams,
        resp: oneshot::Sender<CoreResult<wire::TaskListResult>>,
    },
    /// task.get(M5):Task 规范对象 + 监护态投影。
    TaskGet {
        params: wire::TaskGetParams,
        resp: oneshot::Sender<CoreResult<wire::TaskGetResult>>,
    },
    /// Task 预算扩容(M5-T6;用户批准面:单用户 M5 下命令即批准)
    TaskBudgetIncrease {
        task_id: BmId,
        max_tool_calls: u64,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
    /// Worker 声称任务完成(M5-T8;Observation 核验门禁入口)
    TaskReportCompletion {
        task_id: BmId,
        claim_summary: String,
        operation_id: Option<BmId>,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
    /// Watchdog 手动扫描(测试与运维诊断入口;自动扫描随核心循环节拍)
    WatchdogScan {
        resp: oneshot::Sender<CoreResult<usize>>,
    },
    /// 追加 Worker 成员(M6.3;并发门禁)
    TaskSpawnMember {
        task_id: BmId,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
    /// 委派子任务(M6.3/M6.5;深度/子集/预算/并发四门禁)
    TaskSpawnSubtask {
        params: SpawnSubtaskParams,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
    /// 成员移除(M6.3;替换留痕)
    TaskRemoveMember {
        params: RemoveMemberParams,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
    /// 结果收集(M6.6;来源/状态/关联 Operation)
    TaskCollect {
        task_id: BmId,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
    /// Worker 能力调用(M5 Agent 路径;task:<id> Grant 直通,无授权走审批)
    WorkerCall {
        request_id: BmId,
        params: WorkerCallParams,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
    /// Butler 协调权撤销(M5.1;核心 API,wire 撤销面随 M8 审批 UI)。
    ButlerRevoke {
        reason: String,
        resp: oneshot::Sender<CoreResult<usize>>,
    },
    Stop {
        reason: String,
        resp: oneshot::Sender<()>,
    },
    /// M7 S4:异步能力调用完成回流(单写者落定收据/审计/outbox)。
    ProviderCall {
        operation_id: BmId,
        result: Result<serde_json::Value, crate::ports::AsyncCallError>,
    },
    /// M7.5:异步能力进度回注(capability.progress 事件)。
    ProviderProgress {
        operation_id: String,
        progress: u64,
        total: Option<u64>,
        message: Option<String>,
    },
    Turn(TurnEvent),
}

/// Task 生命周期动作(M5-T1;completed/failed 无 wire 入口——完成判定门禁
/// 在 T8 Observation 核验路径上)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskAction {
    Pause,
    Resume,
    Stop,
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
    /// M7 S4:异步能力调用结果(operation_id → result;内存,随操作同寿命)。
    op_results: HashMap<BmId, serde_json::Value>,
    /// M7 S5:Provider 健康面(provider → 状态;进程内,不入 core-transitions)。
    provider_health: HashMap<String, ProviderHealth>,
    /// M7 S1:turn 模型调用 Broker 凭证留档(operation_id 索引;
    /// 授权点在 spawn,审计点在回合模型阶段终态——两段由 call_id 缝合)。
    model_call_audit: HashMap<BmId, ModelCallAudit>,
}

/// turn 模型调用的审计留档(M7 S1)。
struct ModelCallAudit {
    call_id: BmId,
    epoch: u64,
    instance_id: String,
    principal: String,
}

/// M7 S5:Provider 健康状态(HTTP 熔断/MCP 重连共用;进程内软状态)。
#[derive(Debug, Clone, Default)]
pub struct ProviderHealth {
    pub status: &'static str, // "healthy" | "unavailable"
    /// HTTP:连续失败计数(>=3 开闸);MCP:未用。
    pub fail_streak: u32,
    /// MCP:unavailable 期间的重连探针次数(>=3 封禁)。
    pub reconnect_attempts: u32,
    /// HTTP:熔断冷却截止(半开放行探测);MCP:未用。
    pub cooldown_until: Option<chrono::DateTime<chrono::Utc>>,
}

const PROVIDER_FAIL_THRESHOLD: u32 = 3;
const PROVIDER_COOLDOWN_MS: i64 = 30_000;
const MCP_RECONNECT_LIMIT: u32 = 3;

/// "mcp.<server>.<tool>" -> "mcp.<server>"(健康面主体;其余原样)。
fn mcp_provider_of(capability: &str) -> String {
    let parts: Vec<&str> = capability.split('.').collect();
    if parts.len() >= 3 && parts[0] == "mcp" {
        format!("mcp.{}", parts[1])
    } else {
        capability.to_string()
    }
}

/// 健康迁移(只在状态变化时发事件;payload 见 registry)。
fn emit_provider_health(w: &mut World, provider: &str, from: &str, to: &str, reason: &str) {
    w.emit(
        EventType::ProviderHealthChanged,
        None,
        None,
        None,
        serde_json::json!({
            "provider": provider,
            "from": from,
            "to": to,
            "reason": reason,
        }),
    );
}

/// HTTP 模型连接器:连续失败计账(>=3 开闸熔断,冷却 30s)。
fn note_provider_failure(w: &mut World, provider: &str, reason: &str) {
    let now = w.config.clock.now();
    let entry = w.provider_health.entry(provider.to_string()).or_default();
    entry.fail_streak += 1;
    if entry.status != "unavailable" && entry.fail_streak >= PROVIDER_FAIL_THRESHOLD {
        entry.status = "unavailable";
        entry.cooldown_until = Some(now + chrono::Duration::milliseconds(PROVIDER_COOLDOWN_MS));
        emit_provider_health(w, provider, "healthy", "unavailable", reason);
    }
}

/// 成功落定:清计数;若在 unavailable(半开探测/重连成功)则恢复 healthy。
fn note_provider_success(w: &mut World, provider: &str, reason: &str) {
    let Some(entry) = w.provider_health.get_mut(provider) else {
        return;
    };
    entry.fail_streak = 0;
    entry.reconnect_attempts = 0;
    if entry.status == "unavailable" {
        entry.status = "healthy";
        entry.cooldown_until = None;
        emit_provider_health(w, provider, "unavailable", "healthy", reason);
    }
}

/// 异步能力调用的在途留档(M7 S4):spawn 时捕获,完成回流时落定。
struct AsyncCallMeta {
    capability: String,
    principal: String,
    call_id: BmId,
    epoch: u64,
    instance_id: String,
    key_hash: Option<String>,
    is_side_effect: bool,
    output_schema: String,
    grant_id: Option<String>,
}

/// 追加成员入参(M6.3)。
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnMemberParams {
    pub task_id: BmId,
}

/// 子任务委派入参(M6.3/M6.5;四门禁:深度/子集/预算/并发)。
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnSubtaskParams {
    pub parent_task_id: BmId,
    pub title: String,
    pub goal: String,
    pub authorization: Vec<wire::TaskAuthorizationEntry>,
    pub budget: Option<bm_contract::budget::Budget>,
}

/// 成员移除入参(替换/退出留痕;墓碑语义)。
#[derive(Debug, Clone, PartialEq)]
pub struct RemoveMemberParams {
    pub task_id: BmId,
    pub agent_id: BmId,
    pub reason: String,
}

/// Worker 能力调用入参(Agent 路径;非 Wire 面——GT-03 场景 A3,worker 以
/// task:<id> Grant 直通;worker agent 的自主回合循环随 M7 真实 Provider)。
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerCallParams {
    pub task_id: BmId,
    pub capability: String,
    pub args: serde_json::Value,
    pub idempotency_key: Option<String>,
    pub deadline_ms: Option<u64>,
}

/// 待审批的能力调用载荷(approval 对象只存摘要,重放执行需要原 args)。
struct PendingCapabilityCall {
    op_id: BmId,
    capability: String,
    args: serde_json::Value,
    idempotency_key: Option<String>,
    /// 调用方身份(M5 双路径:surface / worker;审批重放归因一致)
    principal: String,
    trust: DataTrust,
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
        // capability 操作的系统容器(内存合成;不与任何 Session/Agent 关联)
        let system_session = config.id_gen.next_id("sess");
        let system_agent = config.id_gen.next_id("agent");
        let config = config;
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
            registry: CapabilityRegistry::new(),
            grants: GrantLedger::new(),
            approvals: HashMap::new(),
            cap_pending: HashMap::new(),
            idem_results: HashMap::new(),
            system_session,
            system_agent,
            tasks: HashMap::new(),
            task_board: crate::task::TaskBoard::default(),
            task_tool_calls: HashMap::new(),
            watchdog: crate::watchdog::WatchdogState::default(),
            op_capability: HashMap::new(),
            task_results: HashMap::new(),
            op_async_meta: HashMap::new(),
            op_results: HashMap::new(),
            provider_health: HashMap::new(),
            model_call_audit: HashMap::new(),
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
        // M4:内置能力注册(启动面)+ 持久 binding/审批/授权恢复
        let caps = std::mem::take(&mut world.config.capabilities);
        let mut registered: Vec<(String, Arc<dyn CapabilityProvider>)> = Vec::new();
        for (manifest, provider) in caps {
            let instance = format!("{}@{}", manifest.capability, manifest.version);
            let manifest_json = serde_json::to_string(&manifest).unwrap_or_default();
            let capability = manifest.capability.clone();
            let is_async = manifest.provider.starts_with("mcp.");
            world
                .registry
                .register(manifest, &instance, provider.clone())
                .expect("内置能力首次注册不得冲突");
            if is_async {
                world.registry.mark_async(&capability);
            }
            registered.push((capability.clone(), provider));
            if let Some(store) = &world.store {
                let _ = store.save_capability_binding(bm_persist::sqlite_state::CapabilityRow {
                    capability: &capability,
                    provider_instance_id: &instance,
                    epoch: 1,
                    status: "active",
                    manifest: &manifest_json,
                    updated_at: &format_ts(world.started_at),
                });
            }
        }
        // M4 启动恢复:binding epoch 取持久 max(不回退,ADR-0001 条件 2);
        // Grant 台账与审批对象重建——「审批中断后可以恢复」(基线 M4 通过条件)。
        if let Some(store) = &world.store {
            for row in store.list_capability_bindings().unwrap_or_default() {
                let cap = row["capability"].as_str().unwrap_or("");
                let epoch = row["epoch"].as_u64().unwrap_or(0);
                let instance = row["provider_instance_id"].as_str().unwrap_or("");
                if let Some(m) = world.registry.manifest_of(cap).cloned() {
                    world.registry.restore_binding(m, instance, epoch);
                }
            }
            // restore_binding 清空可丢失运行时缓存(T1 语义):句柄由注册流程
            // 重新 attach——否则恢复后执行入口拿不到 Provider。
            for (cap, provider) in &registered {
                let _ = world.registry.attach_handle(cap, provider.clone());
            }
            for row in store.list_grants().unwrap_or_default() {
                let payload = row["payload"].as_str().unwrap_or("null");
                let Ok(grant) = serde_json::from_str::<bm_contract::capability::Grant>(payload)
                else {
                    continue;
                };
                let revoked = row["revoked"].as_i64().unwrap_or(0) == 1;
                // T6c 收紧(M5-T1):消费计数随行恢复,count 类余量重启不回满
                let used = row["used_count"].as_u64().unwrap_or(0);
                world.grants.restore(grant, used, revoked);
            }
            // T6c 收紧(M5-T1):幂等收据仓自持久层装载
            for row in store.list_idem_receipts().unwrap_or_default() {
                if let (Some(h), Some(payload)) =
                    (row["key_hash"].as_str(), row["payload"].as_str())
                    && let Ok(v) = serde_json::from_str::<serde_json::Value>(payload)
                {
                    world.idem_results.insert(h.to_string(), v);
                }
            }
            // M5-T6:Task 包络记账恢复(聚合行 agent_id = "")
            for row in store.list_task_budget().unwrap_or_default() {
                if row["agent_id"].as_str() == Some("")
                    && let (Some(tid), Some(used)) =
                        (row["task_id"].as_str(), row["used_tool_calls"].as_u64())
                    && let Ok(id) = BmId::parse(tid.to_string())
                {
                    world.task_tool_calls.insert(id, used);
                }
            }
            // M5.4:Task Board 投影启动重建——重放事件日志折叠(ADR-0004 条件 1);
            // 已被压实的前缀自 L2 行补齐(行即同一事件流的快照态,键列确定性)
            let events = store.replay_since(0).unwrap_or_default();
            world.task_board = crate::task::TaskBoard::rebuild(&events);
            for row in store.list_tasks().unwrap_or_default() {
                let id = row["id"].as_str().unwrap_or_default().to_string();
                if world.task_board.entry(&id).is_none() {
                    world.task_board.restore_row(
                        &id,
                        row["title"].as_str().unwrap_or_default(),
                        row["state"].as_str().unwrap_or("created"),
                        row["task_epoch"].as_u64().unwrap_or(1),
                    );
                }
            }
            for row in store.list_approvals().unwrap_or_default() {
                let payload = row["payload"].as_str().unwrap_or("null");
                let Ok(wrap) = serde_json::from_str::<serde_json::Value>(payload) else {
                    continue;
                };
                let Ok(approval) = serde_json::from_value::<Approval>(wrap["approval"].clone())
                else {
                    continue;
                };
                let state = approval.state;
                let approval_id =
                    BmId::parse(approval.approval_id.clone()).expect("库内 approval id 合法");
                let Ok(op_id) = BmId::parse(row["operation_id"].as_str().unwrap_or("")) else {
                    continue;
                };
                if state == bm_contract::capability::ApprovalState::WaitingUser {
                    // waiting_approval 的 operation 内存重建(从未离开该状态)
                    let request_id =
                        BmId::from_parts("req", op_id.ulid_part()).expect("同段合成合法");
                    let created_at = approval.requested_at.clone();
                    world.operations.insert(
                        op_id.clone(),
                        Operation {
                            id: op_id.clone(),
                            request_id,
                            session_id: world.system_session.clone(),
                            agent_id: world.system_agent.clone(),
                            state: OperationState::WaitingApproval,
                            turn_index: 0,
                            created_at,
                            completed_at: None,
                            action_summary: "能力调用(恢复)".to_string(),
                            result_reference: None,
                            error: None,
                        },
                    );
                    if let Some(call) = wrap.get("call") {
                        let capability = call["capability"].as_str().unwrap_or("").to_string();
                        let args = call["args"].clone();
                        if !capability.is_empty() {
                            world.cap_pending.insert(
                                approval_id.clone(),
                                PendingCapabilityCall {
                                    op_id,
                                    capability,
                                    args,
                                    idempotency_key: call["idempotency_key"]
                                        .as_str()
                                        .map(|s| s.to_string()),
                                    principal: call["principal"]
                                        .as_str()
                                        .unwrap_or(CAPABILITY_CALLER)
                                        .to_string(),
                                    trust: call["trust"]
                                        .as_str()
                                        .and_then(DataTrust::from_wire)
                                        .unwrap_or(DataTrust::Trusted),
                                },
                            );
                        }
                    }
                }
                world.approvals.insert(approval_id, approval);
            }
            // T6b 恢复三路:pending outbox = intent 在而结果不在。Provider
            // 幂等查询属 M7(外部核验);M4 落地 = operation 重建为
            // outcome_unknown,等待裁定入口(recovery_settle:external_
            // verification OR user_ruling),禁止自动重放(ADR-0004)。
            for row in store.list_outbox_by_state("pending").unwrap_or_default() {
                let Ok(op_id) = BmId::parse(row["operation_id"].as_str().unwrap_or("")) else {
                    continue;
                };
                if world.operations.contains_key(&op_id) {
                    continue;
                }
                let request_id = BmId::from_parts("req", op_id.ulid_part()).expect("同段合成合法");
                world.operations.insert(
                    op_id.clone(),
                    Operation {
                        id: op_id.clone(),
                        request_id,
                        session_id: world.system_session.clone(),
                        agent_id: world.system_agent.clone(),
                        state: OperationState::OutcomeUnknown,
                        turn_index: 0,
                        created_at: world.now_ts(),
                        completed_at: None,
                        action_summary: "外部副作用结果未知(恢复)".to_string(),
                        result_reference: None,
                        error: None,
                    },
                );
                tracing::warn!(
                    op = %op_id,
                    "outbox pending:外部副作用结果未知,等待裁定(recovery_settle)"
                );
            }
        }

        // M5-T3:Butler bootstrap 协调权物化(基线 §10.1 全集;幂等——已有
        // (含已撤销)不重发;撤销是持久事实,重授走审批随 M8 UI)
        {
            let mut existing_pairs: Vec<(String, String)> = Vec::new();
            if let Some(store) = &world.store {
                for row in store.list_grants().unwrap_or_default() {
                    if row["audience"].as_str() == Some(crate::butler::BUTLER_PRINCIPAL)
                        && let Some(action) = row["action"].as_str()
                    {
                        existing_pairs.push((
                            crate::butler::BUTLER_PRINCIPAL.to_string(),
                            action.to_string(),
                        ));
                    }
                }
            }
            let issued = crate::butler::materialize_missing(
                &mut world.grants,
                &existing_pairs,
                &*world.config.id_gen,
                &*world.config.clock,
            );
            for g in issued {
                persist_grant(&world, &g.grant_id);
                world.emit(
                    EventType::GrantCreated,
                    None,
                    None,
                    None,
                    serde_json::json!({
                        "grant_id": g.grant_id,
                        "approval_id": null,
                        "audience": g.audience,
                        "action": g.action,
                        "scope": g.scope.to_wire(),
                        "delegation_depth": g.delegation_depth,
                        "expires_at": null,
                        "parent_hash": g.parent_grant_hash,
                    }),
                );
            }
        }

        // M7.5:异步执行器进度回注(回路外 → Cmd 回流单写者)
        if let Some(ex) = world.config.async_executor.clone() {
            let tx = world.tx.clone();
            ex.set_progress_sink(Box::new(move |n| {
                let _ = tx.try_send(Cmd::ProviderProgress {
                    operation_id: n.operation_id,
                    progress: n.progress,
                    total: n.total,
                    message: n.message,
                });
            }));
        }

        world.watchdog.schedule_next(world.config.clock.now());
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

    /// capability.call(M4.2):统一入口;需审批时返回 approval_required 错误,
    /// operation 停在 waiting_approval,经 approval.respond 续行。
    pub async fn capability_call(
        &self,
        request_id: BmId,
        params: wire::CapabilityCallParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::CapabilityCall {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// approval.list(M4.7):审批对象列表(默认全部,waiting_user 在前)。
    pub async fn approval_list(
        &self,
        params: wire::ApprovalListParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::ApprovalList { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// approval.respond(M4.7):批准(物化 Grant 并重放执行)/拒绝/取消。
    pub async fn approval_respond(
        &self,
        request_id: BmId,
        params: wire::ApprovalRespondParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::ApprovalRespond {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// task.create(M5.2):Task 创建并启动(L2 规范状态;wire/task.v0.1)。
    pub async fn task_create(
        &self,
        request_id: BmId,
        params: wire::TaskCreateParams,
    ) -> CoreResult<wire::TaskCreateResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskCreate {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// task.pause(M5.5):暂停 Task 及其成员编排推进。
    pub async fn task_pause(
        &self,
        request_id: BmId,
        params: wire::TaskLifecycleParams,
    ) -> CoreResult<wire::TaskStateResult> {
        self.task_lifecycle(request_id, TaskAction::Pause, params)
            .await
    }

    /// task.resume(M5.5):恢复 = 编排重启触发者之一(ADR-0004 条件 6)。
    pub async fn task_resume(
        &self,
        request_id: BmId,
        params: wire::TaskLifecycleParams,
    ) -> CoreResult<wire::TaskStateResult> {
        self.task_lifecycle(request_id, TaskAction::Resume, params)
            .await
    }

    /// task.stop(M5.5):取消 Task(进行中副作用按 §9.5 收敛,成员级联停止)。
    pub async fn task_stop(
        &self,
        request_id: BmId,
        params: wire::TaskLifecycleParams,
    ) -> CoreResult<wire::TaskStateResult> {
        self.task_lifecycle(request_id, TaskAction::Stop, params)
            .await
    }

    /// task.list(M5.4):任务列表(L2 规范状态投影为合同对象;确定性序)。
    pub async fn task_list(
        &self,
        params: wire::TaskListParams,
    ) -> CoreResult<wire::TaskListResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskList { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 追加 Worker 成员(M6.3):并发门禁内再签发一枚 Worker(授权链同自举)。
    pub async fn task_spawn_member(&self, task_id: BmId) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskSpawnMember { task_id, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 委派子任务(M6.3/M6.5):深度/授权子集/预算/并发四门禁后建子 Task
    /// 并完成其协调链自举(委派 = 子任务,规格 §5.2)。
    pub async fn task_spawn_subtask(
        &self,
        params: SpawnSubtaskParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskSpawnSubtask { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 成员移除(M6.3):替换/退出留痕(task.member.removed,墓碑语义)。
    pub async fn task_remove_member(
        &self,
        params: RemoveMemberParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskRemoveMember { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// 结果收集(M6.6):聚合成员结果(来源/状态/关联 Operation)+ 子任务概览。
    pub async fn task_collect(&self, task_id: BmId) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskCollect { task_id, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// Worker 能力调用(M5 Agent 路径):worker 以 task:<id> Grant 经
    /// Broker 统一裁决(Grant 命中直通,无授权 untrusted 100% 升级审批)。
    pub async fn worker_capability_call(
        &self,
        request_id: BmId,
        params: WorkerCallParams,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::WorkerCall {
                request_id,
                params,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// Task 预算扩容(M5-T6):仅用户可发起(Broker/Coordinator 不可扩容,
    /// ADR-0002 要点 5);blocked(budget_exhausted)的任务扩容后恢复运行。
    pub async fn task_budget_increase(
        &self,
        task_id: BmId,
        max_tool_calls: u64,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskBudgetIncrease {
                task_id,
                max_tool_calls,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// Worker 声称任务完成(M5-T8):Observation 消费 verification 钩子做
    /// 确定性核验——verified → completed;unverified → blocked(outcome_
    /// unknown_pending)等用户裁定,禁止自动标成功(完成判定门禁)。
    pub async fn task_report_completion(
        &self,
        task_id: BmId,
        claim_summary: impl Into<String>,
        operation_id: Option<BmId>,
    ) -> CoreResult<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskReportCompletion {
                task_id,
                claim_summary: claim_summary.into(),
                operation_id,
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// Watchdog 手动扫描(诊断入口):返回本次产生的事件数。
    pub async fn watchdog_scan(&self) -> CoreResult<usize> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::WatchdogScan { resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// Butler 协调权撤销(M5.1):撤销 bootstrap Grant 集(全部 mutation+
    /// safe 动词),撤销后仅剩只读查询面;重授走审批(交互随 M8)。
    /// 返回撤销的 Grant 数。
    pub async fn butler_revoke(&self, reason: impl Into<String>) -> CoreResult<usize> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::ButlerRevoke {
                reason: reason.into(),
                resp: tx,
            })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    /// task.get(M5.2/M5.4):规范对象 + 监护态(guard_states 随 T7 填充)。
    pub async fn task_get(&self, params: wire::TaskGetParams) -> CoreResult<wire::TaskGetResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskGet { params, resp: tx })
            .await
            .map_err(|_| CoreError::Internal)?;
        rx.await.map_err(|_| CoreError::Internal)?
    }

    async fn task_lifecycle(
        &self,
        request_id: BmId,
        action: TaskAction,
        params: wire::TaskLifecycleParams,
    ) -> CoreResult<wire::TaskStateResult> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Cmd::TaskLifecycle {
                request_id,
                action,
                params,
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
            Cmd::CapabilityCall {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_capability_call(&mut world, request_id, params));
            }
            Cmd::ApprovalList { params, resp } => {
                let _ = resp.send(handle_approval_list(&world, params));
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

    // M7 S1:模型调用权显式授权(Grant 台账;ADR-0006)——创建即授 Forever,
    // 可被 Butler revoke 收回;持久行保证重启后权利不丢。
    let mg =
        crate::butler::model_grant_for(&*w.config.id_gen, agent_id.as_str(), w.config.clock.now());
    w.grants.record(mg.clone());
    persist_grant(w, &mg.grant_id);
    w.emit(
        EventType::GrantCreated,
        None,
        None,
        None,
        serde_json::json!({
            "grant_id": mg.grant_id,
            "approval_id": null,
            "audience": mg.audience,
            "action": mg.action,
            "scope": mg.scope.to_wire(),
            "delegation_depth": mg.delegation_depth,
            "expires_at": null,
            "parent_hash": mg.parent_grant_hash,
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
    // M5 增发:task_id 过滤(watch 观察面;task 事件不携带 session 关联,
    // 过滤在事件信封 payload.task_id 上执行,wire/session 合同语义)
    if let Some(task_id) = &params.task_id {
        let (events, last_seq, has_more) = w.events_for_task(task_id, params.since_seq, limit);
        return Ok(EventsPollResult {
            events,
            last_seq,
            has_more,
        });
    }
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
    // M7 S1:模型调用过 Broker(M4 §5.8 豁免撤销;ADR-0010)。
    // 授权走 Grant 台账:agent 创建即授 model.invoke 永续 Grant,可经
    // grant.revoke 收回(ADR-0006 权力显式化)。不走信任面——内容链构造层
    // 拒绝 trusted(基线 §4.5),而模型调用是回合机器的固定动作。
    let model_call_audit = {
        let ctx = CallContext::content_chain(
            &format!("agent:{}", agent.id.as_str()),
            DataTrust::Untrusted,
        )
        .expect("内容链不得声称 trusted(此处传 untrusted,构造恒成功)");
        let principal = ctx.principal.clone();
        let decision = {
            let broker = Broker::new(
                &w.registry,
                &mut w.grants,
                &*w.config.clock,
                &*w.config.id_gen,
            );
            broker.decide(
                &ctx,
                "model.invoke",
                &serde_json::json!({
                    "model_id": agent.model_chain.first().cloned().unwrap_or_default()
                }),
            )
        };
        match decision {
            Decision::Allowed { .. } => {
                let (epoch, instance_id) = w
                    .registry
                    .binding_of("model.invoke")
                    .map(|b| (b.epoch, b.provider_instance_id.clone()))
                    .unwrap_or((0, "n/a".to_string()));
                Some(ModelCallAudit {
                    call_id: w.config.id_gen.next_id("call"),
                    epoch,
                    instance_id,
                    principal,
                })
            }
            _ => None,
        }
    };
    let Some(model_call_audit) = model_call_audit else {
        w.fail_turn(
            operation_id,
            ErrorCode::Internal,
            "模型调用权未授予或已收回".into(),
        );
        return;
    };
    w.model_call_audit
        .insert(operation_id.clone(), model_call_audit);

    // M7 S5:模型连接器熔断门——冷却期内快速失败(不触连接器);
    // 冷却已过即本次放行(半开探测,成败都由 TurnEvent 回账)。
    {
        let provider = w.config.connector.provider();
        let now = w.config.clock.now();
        let blocked = w
            .provider_health
            .get(provider)
            .map(|h| {
                h.status == "unavailable" && h.cooldown_until.map(|t| now < t).unwrap_or(false)
            })
            .unwrap_or(false);
        if blocked {
            w.fail_turn(
                operation_id,
                ErrorCode::Unavailable,
                "模型 Provider 熔断冷却中,请稍后重试".into(),
            );
            return;
        }
    }

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

// ---- Capability Broker / Approval(M4;ADR-0001/0002)------------------------

const CAPABILITY_CALLER: &str = "surface:user";

/// 审批对象持久化(payload = 包装 JSON:approval 合同形态;未决时附重放执行
/// 载荷 call,裁决后剥离)。写失败仅告警不阻断:审批对象当次仍在内存可裁决,
/// 重启丢失窗口留 T6 事务性 outbox 统一收紧。
fn persist_approval(
    w: &World,
    approval: &Approval,
    op_id: &BmId,
    pending: Option<(&str, &serde_json::Value, Option<&str>, &str, DataTrust)>,
) {
    if let Some(store) = &w.store {
        let mut wrap = serde_json::json!({ "approval": approval });
        if let Some((capability, args, idempotency_key, principal, trust)) = pending {
            wrap["call"] = serde_json::json!({
                "capability": capability, "args": args,
                "idempotency_key": idempotency_key,
                "principal": principal, "trust": trust.as_str()
            });
        }
        let _ = store.save_approval(bm_persist::sqlite_state::ApprovalRow {
            id: approval.approval_id.as_str(),
            operation_id: op_id.as_str(),
            capability: approval.capability.as_str(),
            principal: approval.principal.as_str(),
            state: approval.state.as_str(),
            payload: &wrap.to_string(),
            created_at: approval.requested_at.as_str(),
            resolved_at: approval.resolved_at.as_deref(),
        });
    }
}

/// Grant 行同步(含消费态:Once 消费即 revoked 落行,T6c 起消费计数随行
/// 持久,重启后 count 类余量不回满)。
fn persist_grant(w: &World, grant_id: &str) {
    if let Some(store) = &w.store
        && let Some(grant) = w.grants.get(grant_id).cloned()
    {
        let (used, revoked) = w.grants.entry_state(grant_id).unwrap_or((0, false));
        let _ = store.save_grant(bm_persist::sqlite_state::GrantRow {
            id: grant.grant_id.as_str(),
            audience: grant.audience.as_str(),
            action: grant.action.as_str(),
            revocation_version: grant.revocation_version,
            revoked: revoked || used >= 1 && matches!(grant.scope, GrantScope::Once),
            used_count: used,
            payload: &serde_json::to_string(&grant).unwrap_or_default(),
            created_at: grant.created_at.as_str(),
        });
    }
}

/// Task 行同步(M5-T1):完整合同载荷落 tasks 表;先于事件物化(与
/// persist_approval 同款顺序,事件 INSERT OR IGNORE 不覆盖完整行)。
fn persist_task(w: &World, task: &crate::task::Task) {
    if let Some(store) = &w.store {
        let payload = task_contract_json(task);
        let _ = store.save_task(bm_persist::sqlite_state::TaskRow {
            id: task.id.as_str(),
            title: &task.title,
            state: task.state.as_str(),
            created_by: &task.created_by,
            task_epoch: task.task_epoch,
            payload: &payload,
            created_at: task.created_at.as_str(),
            updated_at: task.updated_at.as_str(),
            parent_task_id: task.parent_task_id.as_ref().map(|p| p.as_str()),
            delegation_depth: task.delegation_depth,
        });
    }
}

/// Task 合同形态 JSON(task/task.v0.1;members 在行级表 + 事件承载,不入载荷)。
/// budget None = 运行时默认包络(落为显式对象,合同 budget 必为对象)。
fn task_contract_json(task: &crate::task::Task) -> String {
    const DEFAULT_MAX_TOKENS: i64 = 1_000_000;
    const DEFAULT_MAX_TURNS: i64 = 1_000;
    let budget_json = match &task.budget {
        Some(b) => serde_json::to_value(b).unwrap_or(serde_json::json!({})),
        None => serde_json::json!({
            "max_tokens": DEFAULT_MAX_TOKENS, "max_turns": DEFAULT_MAX_TURNS
        }),
    };
    serde_json::json!({
        "task_id": task.id.as_str(),
        "title": task.title,
        "goal": task.goal,
        "state": task.state.as_str(),
        "created_by": task.created_by,
        "task_epoch": task.task_epoch,
        "authorization": task.authorization,
        "budget": budget_json,
        "deadline": task.deadline.as_deref(),
        "members": [],
        "parent_task_id": task.parent_task_id.as_ref().map(|p| p.as_str()),
        "delegation_depth": task.delegation_depth,
        "created_at": task.created_at.as_str(),
        "updated_at": task.updated_at.as_str(),
    })
    .to_string()
}

/// 审批可选范围(GT-02 形态;forever 的收紧策略随 M5 审批 UI,规格 §8.6)。
fn capability_scope_choices() -> Vec<GrantScope> {
    vec![
        GrantScope::Once,
        GrantScope::Count(5),
        GrantScope::Ttl(3_600_000),
    ]
}

fn handle_capability_call(
    w: &mut World,
    request_id: BmId,
    params: wire::CapabilityCallParams,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝能力调用".into(),
        ));
    }
    // 直路径(Wire Surface):trusted 直调;幂等键随合同参数面挂链
    // (M7-T3 修复:此前仅 worker 路径挂键,Wire 直调的 idempotency_key 被忽略)
    let mut ctx = CallContext::surface(CAPABILITY_CALLER);
    if let Some(k) = &params.idempotency_key {
        ctx = ctx.with_idempotency_key(k);
    }
    capability_call_inner(w, request_id, ctx, params)
}

/// 统一执行体(M5 双路径同构):直路径 surface ctx / Agent 路径 worker ctx
/// 共用同一裁决-执行-审计管道(ADR-0002 条件 4:双路径统一 Grant/幂等/
/// 脱敏/收据合同;收据与事件的 principal 即来源标注)。
fn capability_call_inner(
    w: &mut World,
    request_id: BmId,
    ctx: CallContext,
    params: wire::CapabilityCallParams,
) -> CoreResult<serde_json::Value> {
    // 步 1-4:查表裁决(Broker 为字段级临时借用,用后即还)
    let decision = {
        let broker = Broker::new(
            &w.registry,
            &mut w.grants,
            &*w.config.clock,
            &*w.config.id_gen,
        );
        broker.decide(&ctx, &params.capability, &params.args)
    };
    // operation 载体:系统容器上的内存操作(M4 能力调用不依赖 Session/Agent;
    // 规范状态由 approvals/grants 承载,operations 表不落行——回看复核项)
    let op_id = w.config.id_gen.next_id("op");
    w.op_capability
        .insert(op_id.clone(), params.capability.clone());
    let created_at = w.now_ts();
    let operation = Operation {
        id: op_id.clone(),
        request_id: request_id.clone(),
        session_id: w.system_session.clone(),
        agent_id: w.system_agent.clone(),
        state: bm_contract::states::OperationState::NotStarted,
        turn_index: 0,
        created_at: created_at.clone(),
        completed_at: None,
        action_summary: format!("能力调用 {}", params.capability),
        result_reference: None,
        error: None,
    };
    w.operations.insert(op_id.clone(), operation.dispatch());

    match decision {
        Decision::Allowed { grant_id } => {
            // 统一执行助手:副作用前门禁(intent)+ 幂等抑制 + 结果事件
            let outcome =
                dispatch_capability(w, &ctx, &params.capability, params.args.clone(), &op_id);
            match outcome {
                CallOutcome::Completed {
                    call_id,
                    credential,
                    result,
                    ..
                } => {
                    let completed_at = w.now_ts();
                    w.settle_operation(&op_id, OperationState::Succeeded, None);
                    // Grant 消费态落行(Once 消费即 revoked,重启后不复活)
                    if let Some(gid) = &grant_id {
                        persist_grant(w, gid);
                    }
                    let _ = (call_id, credential);
                    Ok(serde_json::json!({
                        "operation_id": op_id.as_str(),
                        "request_id": request_id.as_str(),
                        "principal": ctx.principal.clone(),
                        "capability": params.capability,
                        "state": "succeeded",
                        "created_at": created_at,
                        "completed_at": completed_at,
                        "action_summary": format!("能力 {} 执行完成", params.capability),
                        "result_reference": null,
                        "error": null,
                        "grant_used": grant_id,
                        "result": result,
                    }))
                }
                CallOutcome::InvalidArgs { message } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::ValidationFailed,
                        &message,
                    );
                    Err(CoreError::Semantic(ErrorCode::ValidationFailed, message))
                }
                CallOutcome::StaleBinding { expected_epoch, .. } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::Unavailable,
                        &format!("binding 已切换(凭证 epoch {expected_epoch}),请重试"),
                    );
                    Err(CoreError::Semantic(
                        ErrorCode::Unavailable,
                        "Provider binding 已切换,请重试".into(),
                    ))
                }
                CallOutcome::ProviderError { message } | CallOutcome::InvalidOutput { message } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::Internal,
                        &message,
                    );
                    Err(CoreError::Internal)
                }
                CallOutcome::ProviderUnavailable { message } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::Unavailable,
                        &message,
                    );
                    Err(CoreError::Semantic(ErrorCode::Unavailable, message))
                }
                CallOutcome::Suppressed { original_result } => {
                    // 幂等抑制:不重复执行,返回原收据(审计已由助手落
                    // outcome=suppressed;ADR-0002 条件 6)
                    let completed_at = w.now_ts();
                    w.settle_operation(&op_id, OperationState::Succeeded, None);
                    if let Some(gid) = &grant_id {
                        persist_grant(w, gid);
                    }
                    Ok(serde_json::json!({
                        "operation_id": op_id.as_str(),
                        "request_id": request_id.as_str(),
                        "principal": ctx.principal.clone(),
                        "capability": params.capability,
                        "state": "succeeded",
                        "created_at": created_at,
                        "completed_at": completed_at,
                        "action_summary": "幂等抑制:等价请求返回原收据",
                        "result_reference": null,
                        "error": null,
                        "grant_used": grant_id,
                        "result": original_result,
                    }))
                }
                CallOutcome::DispatchedAsync => {
                    // M7 S4:已派发异步执行;调用方经 operations.get 轮询终态
                    Ok(serde_json::json!({
                        "operation_id": op_id.as_str(),
                        "request_id": request_id.as_str(),
                        "principal": ctx.principal.clone(),
                        "capability": params.capability,
                        "state": "running",
                        "created_at": created_at,
                        "completed_at": null,
                        "action_summary": format!("能力 {} 异步执行中", params.capability),
                        "result_reference": null,
                        "error": null,
                        "grant_used": grant_id,
                        "result": null,
                    }))
                }
                CallOutcome::Rejected { .. } => {
                    unreachable!("Allowed 分支不会再被拒绝")
                }
            }
        }
        Decision::RequireApproval {
            risk_class,
            effective_risk,
        } => {
            let mut mgr = ApprovalManager::new(&mut w.grants, &*w.config.clock, &*w.config.id_gen);
            let mut approval = mgr.open(OpenApproval {
                capability: &params.capability,
                principal: &ctx.principal,
                risk_class,
                effective_risk,
                input_trust: ctx.trust,
                args: &params.args,
                args_summary: &format!("能力 {} 调用", params.capability),
                scope_choices: capability_scope_choices(),
                ttl_ms: 300_000,
            });
            let approval_id = BmId::parse(approval.approval_id.clone()).expect("appr_ 前缀合法");
            w.settle_operation(&op_id, OperationState::WaitingApproval, None);
            w.emit(
                EventType::ApprovalRequested,
                None,
                None,
                Some(op_id.clone()),
                serde_json::json!({
                    "approval_id": approval.approval_id,
                    "operation_id": op_id.as_str(),
                    "capability": params.capability,
                    "principal": ctx.principal.clone(),
                    "risk_class": risk_class.as_str(),
                    "effective_risk": effective_risk.as_str(),
                    "input_trust": approval.input_trust.as_str(),
                    "expires_at": approval.expires_at,
                }),
            );
            approval.grant_id = None;
            w.approvals.insert(approval_id.clone(), approval.clone());
            persist_approval(
                w,
                &approval,
                &op_id,
                Some((
                    &params.capability,
                    &params.args,
                    params.idempotency_key.as_deref(),
                    ctx.principal.as_str(),
                    ctx.trust,
                )),
            );
            w.cap_pending.insert(
                approval_id,
                PendingCapabilityCall {
                    op_id,
                    capability: params.capability.clone(),
                    args: params.args.clone(),
                    idempotency_key: params.idempotency_key.clone(),
                    principal: ctx.principal.clone(),
                    trust: ctx.trust,
                },
            );
            // GT-02 场景 A2 形态:approval_required 错误信封;operation 停在
            // waiting_approval,由 approval.respond 续行(基线 §9.6)
            Err(CoreError::Semantic(
                ErrorCode::ApprovalRequired,
                format!("能力 {} 需要用户审批", params.capability),
            ))
        }
        Decision::Denied { reason } => {
            let (msg, call_id) = match reason {
                DenyReason::UnknownCapability => (
                    "未知能力,且审批不能补授权(默认拒绝)",
                    w.config.id_gen.next_id("call"),
                ),
                DenyReason::NoGrant => ("无有效授权(默认拒绝)", w.config.id_gen.next_id("call")),
            };
            let reason_code = match reason {
                DenyReason::UnknownCapability => "unknown_capability",
                DenyReason::NoGrant => "no_grant",
            };
            w.settle_operation(
                &op_id,
                OperationState::Failed,
                Some(WireError::new(ErrorCode::PermissionDenied, msg.to_string())),
            );
            w.emit(
                EventType::CapabilityDenied,
                None,
                None,
                Some(op_id.clone()),
                serde_json::json!({
                    "call_id": call_id.as_str(),
                    "capability": params.capability,
                    "principal": ctx.principal.clone(),
                    "input_trust": ctx.trust.as_str(),
                    "reason_code": reason_code,
                }),
            );
            Err(CoreError::Semantic(
                ErrorCode::PermissionDenied,
                msg.to_string(),
            ))
        }
    }
}

/// M7 S4:异步能力调用完成落定(单写者内)。
/// 成功:出参校验 → succeeded + 幂等收据/outbox published + capability.invoked ok;
/// 失败:Timeout/Transport/ToolError 三类映射,副作用 outbox 保持 pending
/// (超时 = 结果未知,对账语义与崩溃窗口一致)。
fn handle_provider_call(
    w: &mut World,
    operation_id: BmId,
    result: Result<serde_json::Value, crate::ports::AsyncCallError>,
) {
    use crate::ports::AsyncCallError;
    let Some(meta) = w.op_async_meta.remove(&operation_id) else {
        return;
    };
    if !w.operations.contains_key(&operation_id) {
        return; // 停机清场后回流的迟到完成:无载体,丢弃(事件已在日志)
    }
    match result {
        Ok(value) => {
            if let Err(e) = bm_contract::schemas::validate(&meta.output_schema, &value) {
                fail_capability_call(
                    w,
                    &operation_id,
                    &meta.capability,
                    &meta.principal,
                    ErrorCode::Internal,
                    &format!("异步结果出参校验失败: {e}"),
                );
                return;
            }
            w.settle_operation(&operation_id, OperationState::Succeeded, None);
            if let (Some(h), true) = (&meta.key_hash, meta.is_side_effect) {
                w.idem_results.insert(h.clone(), value.clone());
                if let Some(store) = &w.store {
                    let _ = store.save_idem_receipt(h, &value.to_string(), &w.now_ts());
                    let _ = store.outbox_upsert(
                        operation_id.as_str(),
                        "side_effect",
                        "published",
                        &serde_json::json!({
                            "capability": meta.capability,
                            "key_hash": meta.key_hash,
                        })
                        .to_string(),
                        &w.now_ts(),
                    );
                }
            }
            if let Some(gid) = &meta.grant_id {
                persist_grant(w, gid);
            }
            w.op_results.insert(operation_id.clone(), value.clone());
            // M7 S5:成功 -> 恢复 healthy(重连成功/清探针计数)
            note_provider_success(w, &mcp_provider_of(&meta.capability), "重连握手成功");
            emit_capability_invoked_with(
                w,
                &meta.call_id,
                &operation_id,
                &meta.capability,
                &meta.principal,
                Some(meta.epoch),
                Some(&meta.instance_id),
                "ok",
                None,
                meta.key_hash.as_deref(),
            );
        }
        Err(e) => {
            // M7 S5:传输故障 -> MCP unavailable 立即;unavailable 期间的调用
            // 即重连探针(到上限后由 dispatch 门快速失败)
            if matches!(e, AsyncCallError::Transport(_)) {
                let provider = mcp_provider_of(&meta.capability);
                let was = w
                    .provider_health
                    .get(&provider)
                    .map(|h| h.status)
                    .unwrap_or("healthy");
                let entry = w.provider_health.entry(provider.clone()).or_default();
                entry.status = "unavailable";
                if was == "unavailable" {
                    entry.reconnect_attempts += 1;
                }
                if was != "unavailable" {
                    emit_provider_health(w, &provider, "healthy", "unavailable", "子进程/通道故障");
                }
            }
            let (code, msg) = match e {
                AsyncCallError::Timeout => (
                    ErrorCode::Timeout,
                    "异步调用超时(结果未知,对账由 outbox 承载)",
                ),
                AsyncCallError::Transport(_) => (ErrorCode::Unavailable, "Provider 传输故障"),
                AsyncCallError::ToolError => (ErrorCode::Internal, "工具报告执行失败"),
            };
            fail_capability_call(
                w,
                &operation_id,
                &meta.capability,
                &meta.principal,
                code,
                msg,
            );
            if let Some(gid) = &meta.grant_id {
                persist_grant(w, gid);
            }
        }
    }
}

/// M7.5:异步能力进度回注 → capability.progress 事件(操作不存在则丢弃)。
fn handle_provider_progress(
    w: &mut World,
    operation_id: String,
    progress: u64,
    total: Option<u64>,
    message: Option<String>,
) {
    let Ok(op_id) = BmId::parse(&operation_id) else {
        return;
    };
    if !w.operations.contains_key(&op_id) {
        return;
    }
    let capability = w.op_capability.get(&op_id).cloned().unwrap_or_default();
    w.emit(
        EventType::CapabilityProgress,
        None,
        None,
        Some(op_id.clone()),
        serde_json::json!({
            "call_id": w.config.id_gen.next_id("call").as_str(),
            "operation_id": op_id.as_str(),
            "capability": capability,
            "progress": progress,
            "total": total,
            "message": message,
        }),
    );
}

/// 执行失败的统一收口:operation → failed + capability.invoked(outcome=error)。
fn fail_capability_call(
    w: &mut World,
    op_id: &BmId,
    capability: &str,
    principal: &str,
    code: ErrorCode,
    message: &str,
) {
    w.settle_operation(
        op_id,
        OperationState::Failed,
        Some(WireError::new(code, message.to_string())),
    );
    w.emit(
        EventType::CapabilityInvoked,
        None,
        None,
        Some(op_id.clone()),
        serde_json::json!({
            "call_id": w.config.id_gen.next_id("call").as_str(),
            "operation_id": op_id.as_str(),
            "capability": capability,
            "principal": principal,
            "binding_epoch": 0,
            "provider_instance_id": "n/a",
            "outcome": "error",
            "error_code": code.as_str(),
            "idempotency_key_hash": null,
        }),
    );
}

fn handle_approval_list(
    w: &World,
    params: wire::ApprovalListParams,
) -> CoreResult<serde_json::Value> {
    // 缺省 = 待裁决队列(waiting_user):审批工作面只关心未决项;
    // 显式 --state 过滤任意状态(wire/capability 合同 description)。
    let state_filter = params
        .state_filter
        .as_deref()
        .unwrap_or(bm_contract::capability::ApprovalState::WaitingUser.as_str());
    let mut rows: Vec<&Approval> = w
        .approvals
        .values()
        .filter(|a| a.state.as_str() == state_filter)
        .collect();
    rows.sort_by(|a, b| a.requested_at.cmp(&b.requested_at));
    let mut approvals = Vec::new();
    for a in rows {
        approvals.push(serde_json::to_value(a).map_err(|_| CoreError::Internal)?);
    }
    Ok(serde_json::json!({ "approvals": approvals }))
}

fn handle_approval_respond(
    w: &mut World,
    request_id: BmId,
    params: wire::ApprovalRespondParams,
) -> CoreResult<serde_json::Value> {
    let decision = match params.decision.as_str() {
        "approve" => RespondDecision::Approve,
        "deny" => RespondDecision::Deny,
        "withdraw" => RespondDecision::Withdraw,
        other => return Err(CoreError::validation(format!("非法 decision: {other}"))),
    };
    let scope = params
        .scope
        .as_deref()
        .map(|s| GrantScope::from_wire(s).ok_or_else(|| CoreError::validation("非法 scope")))
        .transpose()?;
    // M5 解读条款 4 兑现:task:<id> scope 自 Task 对象落地起启用;校验面
    // 仅拒绝引用不存在 Task 的情形(M4 期恒拒的过渡语义移除)
    if let Some(GrantScope::Task(task_id)) = &scope {
        let exists = BmId::parse(task_id.clone())
            .map(|id| w.tasks.contains_key(&id))
            .unwrap_or(false);
        if !exists {
            return Err(CoreError::validation(format!(
                "task scope 引用不存在的 Task: {task_id}"
            )));
        }
    }
    let pending = w.cap_pending.get(&params.approval_id).map(|p| {
        (
            p.op_id.clone(),
            p.capability.clone(),
            p.args.clone(),
            p.idempotency_key.clone(),
            p.principal.clone(),
            p.trust,
        )
    });
    let resource = bm_contract::capability::GrantResource {
        capability: w
            .approvals
            .get(&params.approval_id)
            .map(|a| a.capability.clone())
            .ok_or_else(|| CoreError::validation("未知审批对象"))?,
        args_predicates: Default::default(),
    };
    let respond_result = {
        let approval = w
            .approvals
            .get_mut(&params.approval_id)
            .ok_or_else(|| CoreError::validation("未知审批对象"))?;
        let mut mgr = ApprovalManager::new(&mut w.grants, &*w.config.clock, &*w.config.id_gen);
        mgr.respond(approval, decision, scope, resource, CAPABILITY_CALLER)
    };
    // 裁决后同步审批行(非 waiting 态剥离重放载荷)
    let op_row_id = pending.as_ref().map(|(op_id, ..)| op_id.clone());
    if let (Some(a), Some(op_id)) = (w.approvals.get(&params.approval_id), op_row_id.as_ref()) {
        persist_approval(w, a, op_id, None);
    }
    let op = pending;
    match respond_result {
        Ok(Some(grant)) => {
            let op_key = op.as_ref().map(|(op_id, ..)| op_id.clone());
            w.emit(
                EventType::GrantCreated,
                None,
                None,
                op_key.clone(),
                serde_json::json!({
                    "grant_id": grant.grant_id,
                    "approval_id": params.approval_id.as_str(),
                    "audience": grant.audience,
                    "action": grant.action,
                    "scope": grant.scope.to_wire(),
                    "delegation_depth": grant.delegation_depth,
                    "expires_at": grant.expires_at,
                    "parent_hash": grant.parent_grant_hash,
                }),
            );
            // approval.resolved 键集:[approval_id, operation_id, outcome, scope, grant_id]
            w.emit(
                EventType::ApprovalResolved,
                None,
                None,
                op_key.clone(),
                serde_json::json!({
                    "approval_id": params.approval_id.as_str(),
                    "operation_id": op_key.as_ref().map(|o| o.as_str()),
                    "outcome": "approved",
                    "scope": grant.scope.to_wire(),
                    "grant_id": grant.grant_id,
                }),
            );
            // 批准:operation 续行(waiting_approval→running→统一执行助手)
            persist_grant(w, &grant.grant_id);
            if let Some((op_id, capability, args, idem, principal, trust)) = op {
                w.settle_operation(&op_id, OperationState::Running, None);
                // 重放按原始调用方身份归因(M5 双路径:surface / worker)
                let mut ctx = CallContext::content_chain(&principal, trust)
                    .unwrap_or_else(|_| CallContext::surface(CAPABILITY_CALLER));
                if let Some(k) = idem {
                    ctx = ctx.with_idempotency_key(k);
                }
                let outcome = dispatch_capability(w, &ctx, &capability, args, &op_id);
                match outcome {
                    CallOutcome::Completed { .. } | CallOutcome::Suppressed { .. } => {
                        w.settle_operation(&op_id, OperationState::Succeeded, None);
                        persist_grant(w, &grant.grant_id);
                        w.cap_pending.remove(&params.approval_id);
                    }
                    CallOutcome::DispatchedAsync => {
                        // M7 S4:异步执行中;完成经 Cmd::ProviderCall 落定
                        // (收据/Grant 消费态/outbox 均在完成处理器收口)
                        w.cap_pending.remove(&params.approval_id);
                    }
                    CallOutcome::ProviderUnavailable { message } => {
                        // M7 S5:重连超限在批准重放中同样快速失败(unavailable)
                        fail_capability_call(
                            w,
                            &op_id,
                            &capability,
                            &principal,
                            ErrorCode::Unavailable,
                            &message,
                        );
                        w.cap_pending.remove(&params.approval_id);
                    }
                    other => {
                        let code = match &other {
                            CallOutcome::InvalidArgs { .. } => ErrorCode::ValidationFailed,
                            CallOutcome::StaleBinding { .. } => ErrorCode::Unavailable,
                            _ => ErrorCode::Internal,
                        };
                        let message = match &other {
                            CallOutcome::InvalidArgs { message }
                            | CallOutcome::ProviderError { message }
                            | CallOutcome::InvalidOutput { message } => message.clone(),
                            CallOutcome::StaleBinding { .. } => "binding 已切换".into(),
                            _ => "批准后执行失败".into(),
                        };
                        w.settle_operation(
                            &op_id,
                            OperationState::Failed,
                            Some(WireError::new(code, message)),
                        );
                    }
                }
            }
            Ok(serde_json::json!({
                "approval_id": params.approval_id.as_str(),
                "state": "approved",
                "grant_id": grant.grant_id,
                "request_id": request_id.as_str(),
            }))
        }
        Ok(None) => {
            let outcome_str = match decision {
                RespondDecision::Deny => "denied",
                RespondDecision::Withdraw => "withdrawn",
                RespondDecision::Approve => unreachable!("approve 必然返回 Some/Err"),
            };
            let op_key = op.as_ref().map(|(op_id, ..)| op_id.clone());
            w.emit(
                EventType::ApprovalResolved,
                None,
                None,
                op_key.clone(),
                serde_json::json!({
                    "approval_id": params.approval_id.as_str(),
                    "operation_id": op_key.as_ref().map(|o| o.as_str()),
                    "outcome": outcome_str,
                    "scope": null,
                    "grant_id": null,
                }),
            );
            // denied/expired/withdrawn → operation cancelled(基线 §9.6)
            if let Some((op_id, ..)) = op {
                w.settle_operation(&op_id, OperationState::Cancelled, None);
                w.cap_pending.remove(&params.approval_id);
            }
            Ok(serde_json::json!({
                "approval_id": params.approval_id.as_str(),
                "state": outcome_str,
                "grant_id": null,
                "request_id": request_id.as_str(),
            }))
        }
        Err(ApprovalError::Expired) => {
            let op_key = op.as_ref().map(|(op_id, ..)| op_id.clone());
            w.emit(
                EventType::ApprovalExpired,
                None,
                None,
                op_key.clone(),
                serde_json::json!({
                    "approval_id": params.approval_id.as_str(),
                    "operation_id": op_key.as_ref().map(|o| o.as_str()),
                    "expired_at": w.now_ts(),
                }),
            );
            if let Some((op_id, ..)) = op {
                w.settle_operation(&op_id, OperationState::Cancelled, None);
                w.cap_pending.remove(&params.approval_id);
            }
            Err(CoreError::Semantic(
                ErrorCode::ApprovalDenied,
                "审批窗口已过期(等价拒绝)".into(),
            ))
        }
        Err(e) => Err(CoreError::validation(format!("审批裁决失败: {e:?}"))),
    }
}

/// 统一执行助手(门禁+审计;T6 规格 §5.5/§5.9):副作用类先落 intent 事件
/// (前门禁——intent 落盘后方允许 Provider 执行,ADR-0001 条件 5);幂等键
/// 命中历史收据 → suppressed(不重复执行,ADR-0002 条件 6);结果落 ok/error
/// 事件。operation 终态由调用方落定。
fn dispatch_capability(
    w: &mut World,
    ctx: &CallContext,
    capability: &str,
    args: serde_json::Value,
    op_id: &BmId,
) -> CallOutcome {
    let prepared = {
        let mut broker = Broker::new(
            &w.registry,
            &mut w.grants,
            &*w.config.clock,
            &*w.config.id_gen,
        );
        match broker.prepare(ctx, capability, args.clone()) {
            Ok(p) => p,
            Err(outcome) => {
                emit_capability_invoked(
                    w,
                    op_id,
                    capability,
                    &ctx.principal,
                    None,
                    None,
                    "error",
                    Some(error_code_of(&outcome)),
                    None,
                );
                return outcome;
            }
        }
    };
    let key_hash: Option<String> = ctx.idempotency_key.as_ref().map(|k| {
        sha256_hex(&format!(
            "{k}:{}",
            serde_json::to_string(&args).unwrap_or_default()
        ))
    });
    if prepared.is_side_effect {
        // 幂等抑制:等价请求返回原收据,Provider 不再执行
        if let Some(h) = &key_hash
            && let Some(original) = w.idem_results.get(h).cloned()
        {
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "suppressed",
                None,
                Some(h),
            );
            return CallOutcome::Suppressed {
                original_result: original,
            };
        }
        // 前门禁:intent 落盘后方执行(崩溃窗口 = intent 在而结果不在 →
        // 恢复期以 Provider 幂等查询对账,T6b)。outbox pending 行与 intent
        // 事件同批落盘,是恢复扫描的对账底座。
        emit_capability_invoked(
            w,
            op_id,
            capability,
            &ctx.principal,
            Some(prepared.credential.binding_epoch),
            Some(&prepared.credential.provider_instance_id),
            "intent",
            None,
            key_hash.as_deref(),
        );
        if let Some(store) = &w.store {
            let _ = store.outbox_upsert(
                op_id.as_str(),
                "side_effect",
                "pending",
                &serde_json::json!({
                    "capability": capability,
                    "key_hash": key_hash,
                })
                .to_string(),
                &w.now_ts(),
            );
        }
    }
    // M7 S4:异步 Provider 路径——决策/校验/预扣/intent 门已过,执行交
    // 异步执行器,完成经 Cmd::ProviderCall 回单写者回路落定。
    if w.registry.is_async(capability) {
        // M7 S5:MCP 重连超限 -> 快速失败(不再触执行器,直至重装)
        let provider = mcp_provider_of(capability);
        let blocked = w
            .provider_health
            .get(&provider)
            .map(|h| h.status == "unavailable" && h.reconnect_attempts >= MCP_RECONNECT_LIMIT)
            .unwrap_or(false);
        if blocked {
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "error",
                Some(ErrorCode::Unavailable),
                key_hash.as_deref(),
            );
            return CallOutcome::ProviderUnavailable {
                message: "异步 Provider 重连超限,保持 unavailable 直至重装".into(),
            };
        }
        let Some(executor) = w.config.async_executor.clone() else {
            return CallOutcome::ProviderError {
                message: "异步执行器未装配".into(),
            };
        };
        let meta = AsyncCallMeta {
            capability: capability.to_string(),
            principal: ctx.principal.clone(),
            call_id: BmId::parse(&prepared.credential.call_id)
                .unwrap_or_else(|_| w.config.id_gen.next_id("call")),
            epoch: prepared.credential.binding_epoch,
            instance_id: prepared.credential.provider_instance_id.clone(),
            key_hash: key_hash.clone(),
            is_side_effect: prepared.is_side_effect,
            output_schema: prepared.manifest.output_schema.to_string(),
            grant_id: prepared.grant_id.clone(),
        };
        w.op_async_meta.insert(op_id.clone(), meta);
        // Grant 消费态随 spawn 落行(count 类重启不回满)
        if let Some(gid) = &prepared.grant_id {
            persist_grant(w, gid);
        }
        let deadline_ms = prepared.manifest.timeout_ms.clamp(100, 600_000);
        let tx = w.tx.clone();
        let op = op_id.clone();
        let cap = capability.to_string();
        let exec_args = args.clone();
        tokio::spawn(async move {
            let result = executor
                .call(
                    op.as_str(),
                    &cap,
                    exec_args,
                    std::time::Duration::from_millis(deadline_ms),
                )
                .await;
            let _ = tx
                .send(Cmd::ProviderCall {
                    operation_id: op,
                    result,
                })
                .await;
        });
        return CallOutcome::DispatchedAsync;
    }
    let outcome = {
        let broker = Broker::new(
            &w.registry,
            &mut w.grants,
            &*w.config.clock,
            &*w.config.id_gen,
        );
        broker.execute(&prepared, args)
    };
    match &outcome {
        CallOutcome::Completed { result, .. } => {
            if let (Some(h), true) = (&key_hash, prepared.is_side_effect) {
                w.idem_results.insert(h.clone(), result.clone());
                // T6c 收紧(M5-T1):幂等收据落表,恢复期抑制判定不依赖内存
                if let Some(store) = &w.store {
                    let _ = store.save_idem_receipt(h, &result.to_string(), &w.now_ts());
                }
            }
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "ok",
                None,
                key_hash.as_deref(),
            );
            if prepared.is_side_effect
                && let Some(store) = &w.store
            {
                let _ = store.outbox_upsert(
                    op_id.as_str(),
                    "side_effect",
                    "published",
                    &serde_json::json!({
                        "capability": capability,
                        "key_hash": key_hash,
                    })
                    .to_string(),
                    &w.now_ts(),
                );
            }
        }
        CallOutcome::Suppressed { .. } => unreachable!("抑制发生在 execute 前"),
        other => {
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "error",
                Some(error_code_of(other)),
                key_hash.as_deref(),
            );
        }
    }
    outcome
}

// ---- M5:Task 生命周期(T1)--------------------------------------------------

/// task.create:Task 对象入 L2(tasks 表 + task.created 事件)并即启动
/// (created→running,GT-03 场景 A1 语义:Butler 接单即开跑)。
/// request_id 预留审计(M5 规格归因链随 T3 Butler 接线启用)。
fn handle_task_create(
    w: &mut World,
    _request_id: BmId,
    params: wire::TaskCreateParams,
) -> CoreResult<wire::TaskCreateResult> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝创建 Task".into(),
        ));
    }
    // 协调权门禁(M5.1):task.create 是 Butler 的 mutation 协调动词——
    // bootstrap Grant 被撤销后此命令拒绝(重授走审批,撤销不影响既有 Task)
    if w.grants
        .active_for(
            crate::butler::BUTLER_PRINCIPAL,
            "task.create",
            w.config.clock.now(),
        )
        .is_empty()
    {
        return Err(CoreError::Semantic(
            ErrorCode::PermissionDenied,
            "Butler 协调权(task.create)已被撤销,重授需用户批准".into(),
        ));
    }
    // Task 授权校验:动词 ⊆ Butler 协调清单(上界);mutation 动词必须显式
    // 标记 klass=mutation(ADR-0002 §11.2 二分;领域动词不可授权)
    let authorization = params.authorization.unwrap_or_default();
    for entry in &authorization {
        let Some(class) = crate::butler::verb_class(&entry.verb) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("非协调动词不可授权: {}", entry.verb),
            ));
        };
        let ok = match class {
            crate::butler::CoordinationClass::Mutation => {
                entry.klass.as_deref() == Some("mutation")
            }
            crate::butler::CoordinationClass::Safe => {
                matches!(entry.klass.as_deref(), None | Some("safe"))
            }
        };
        if !ok {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!(
                    "授权分级与动词默认分级不一致: {}(mutation 动词必须显式 klass=mutation)",
                    entry.verb
                ),
            ));
        }
    }
    let now = w.config.clock.now();
    // wire task.create 恒为根 Task(委派走 spawn_subtask 内核 API,M6 规格 §8-4)
    let mut task = crate::task::Task::create(
        &*w.config.id_gen,
        params.title,
        params.goal,
        authorization,
        params.budget,
        params.deadline,
        None,
        0,
        now,
    );
    // task.created(事实)+ created→running(task_started)
    w.emit(
        EventType::TaskCreated,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task.id.as_str(),
            "title": task.title,
            "created_by": task.created_by,
            "parent_task_id": null,
        }),
    );
    let (from, to, guard) = task
        .transition(bm_contract::states::TaskState::Running, None, now)
        .expect("created→running 是迁移表边");
    w.emit(
        EventType::TaskStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task.id.as_str(),
            "from": from.as_str(),
            "to": to.as_str(),
            "reason_code": guard,
            "task_epoch": task.task_epoch,
        }),
    );

    // M5-T4/T5:协调链自举——三方交集物化为 task:<id> Grant(ADR-0002 §11.3)
    // + Coordinator/单 Worker 成员事实(GT-03 场景 A2 形态)
    {
        let task_id_str = task.id.as_str().to_string();
        // M6:per-task principal 命名空间(跨 Task 访问在 Grant 查表层结构性不命中)
        let coord_aud = crate::team::coord_principal(&task_id_str);
        let worker_aud = crate::team::worker_principal(&task_id_str);
        // 分阶段作用域:butler 上界查证闭包借用 w.grants,产出后即释放
        let (coord_grants, worker_grants) = {
            let mut butler_lookup = |verb: &str| {
                w.grants
                    .active_for(crate::butler::BUTLER_PRINCIPAL, verb, now)
                    .into_iter()
                    .next()
            };
            crate::coordinator::intersection_grants(
                &*w.config.id_gen,
                &task_id_str,
                &coord_aud,
                &worker_aud,
                &task.authorization,
                now,
                &mut butler_lookup,
            )
        };
        for g in coord_grants.iter().chain(worker_grants.iter()) {
            w.grants.record(g.clone());
            persist_grant(w, &g.grant_id);
            w.emit(
                EventType::GrantCreated,
                None,
                None,
                None,
                serde_json::json!({
                    "grant_id": g.grant_id,
                    "approval_id": null,
                    "audience": g.audience,
                    "action": g.action,
                    "scope": g.scope.to_wire(),
                    "delegation_depth": g.delegation_depth,
                    "expires_at": null,
                    "parent_hash": g.parent_grant_hash,
                }),
            );
        }
        // 成员事实:Coordinator(必有)+ Worker(仅当任务声明了能力资源)
        let member_event = |w: &mut World, agent_id: &str, role: &str, grant_id: Option<&str>| {
            w.emit(
                EventType::TaskMemberAdded,
                None,
                None,
                None,
                serde_json::json!({
                    "task_id": task_id_str,
                    "agent_id": agent_id,
                    "role": role,
                    "grant_id": grant_id,
                }),
            )
        };
        let coord_member_id = w.config.id_gen.next_id("agent");
        let coord_grant_id = coord_grants
            .iter()
            .find(|g| g.action == "agent.spawn")
            .or_else(|| coord_grants.first())
            .map(|g| g.grant_id.clone());
        let ev = member_event(
            w,
            coord_member_id.as_str(),
            "coordinator",
            coord_grant_id.as_deref(),
        );
        task.add_member(crate::task::TaskMember {
            agent_id: coord_member_id,
            role: crate::task::MemberRole::Coordinator,
            grant_id: coord_grant_id,
            joined_seq: ev.event_seq,
        });
        if !worker_grants.is_empty() {
            let worker_member_id = w.config.id_gen.next_id("agent");
            let worker_grant_id = worker_grants[0].grant_id.clone();
            let ev = member_event(
                w,
                worker_member_id.as_str(),
                "worker",
                Some(worker_grant_id.as_str()),
            );
            task.add_member(crate::task::TaskMember {
                agent_id: worker_member_id,
                role: crate::task::MemberRole::Worker,
                grant_id: Some(worker_grant_id),
                joined_seq: ev.event_seq,
            });
        }
    }
    persist_task(w, &task);
    let result = wire::TaskCreateResult {
        task_id: task.id.clone(),
        state: task.state,
        created_at: task.created_at.clone(),
    };
    w.tasks.insert(task.id.clone(), task);
    Ok(result)
}

/// task.pause / task.resume / task.stop:生命周期命令(表内边 + epoch 门禁 +
/// 事实事件 + 行同步)。wire 面不暴露 epoch 参数(与 input_trust 同款收权):
/// wire 命令恒以当前 epoch 出示;stale 语义由编排器内部路径与测试行使。
fn handle_task_lifecycle(
    w: &mut World,
    _request_id: BmId,
    action: TaskAction,
    params: wire::TaskLifecycleParams,
) -> CoreResult<wire::TaskStateResult> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝 Task 生命周期命令".into(),
        ));
    }
    let Some(task) = w.tasks.get_mut(&params.task_id) else {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 不存在: {}", params.task_id.as_str()),
        ));
    };
    // epoch 门禁:wire 路径出示当前 epoch(接管权变更后旧命令在核心面拒绝)
    task.require_epoch(task.task_epoch)
        .map_err(|e| task_error_to_core(&params.task_id, e))?;
    let now = w.config.clock.now();
    let to = match action {
        TaskAction::Pause => bm_contract::states::TaskState::Paused,
        TaskAction::Resume => bm_contract::states::TaskState::Running,
        TaskAction::Stop => bm_contract::states::TaskState::Cancelled,
    };
    // 分阶段作用域(跨字段借用):迁移与取值完成后即释放 task 借用
    let (transition, state_result, task_snapshot) = {
        let (from, to, guard) = task
            .transition(to, None, now)
            .map_err(|e| task_error_to_core(&params.task_id, e))?;
        let result = wire::TaskStateResult {
            task_id: task.id.clone(),
            state: task.state,
        };
        (Some((from, to, guard)), result, task.clone())
    };
    let (from, to, guard) = transition.expect("迁移已成功");
    w.emit(
        EventType::TaskStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task_snapshot.id.as_str(),
            "from": from.as_str(),
            "to": to.as_str(),
            "reason_code": guard,
            "task_epoch": task_snapshot.task_epoch,
        }),
    );
    persist_task(w, &task_snapshot);
    // Task 结束即失效(ADR-0002 要点 1):终态时该 Task 的全部 task:<id>
    // Grant 撤销(审计 grant.revoked,持久行同步,重启不复活)
    if to == bm_contract::states::TaskState::Cancelled {
        let gids: Vec<String> = w
            .grants
            .grants_scoped_to(task_snapshot.id.as_str())
            .into_iter()
            .filter(|g| {
                w.grants
                    .entry_state(&g.grant_id)
                    .map(|(_, revoked)| !revoked)
                    .unwrap_or(false)
            })
            .map(|g| g.grant_id)
            .collect();
        for gid in gids {
            let version = w.grants.revoke(&gid).map_err(|_| CoreError::Internal)?;
            w.emit(
                EventType::GrantRevoked,
                None,
                None,
                None,
                serde_json::json!({
                    "grant_id": gid,
                    "revocation_version": version,
                    "reason": "task_cancelled",
                }),
            );
            persist_grant(w, &gid);
        }
    }
    Ok(state_result)
}

/// Watchdog 扫描(运行时执行面):判定 → 事实事件/状态变更。
/// 仅监督,不推断编排下一步(G4);blocked 后不再自动重启(ADR-0004 条件 6)。
fn watchdog_scan_run(w: &mut World) -> usize {
    let now = w.config.clock.now();
    let mut events = 0;
    // 分阶段作用域:先收集 Running 任务与判定,再逐个变更
    let mut decisions: Vec<(BmId, crate::watchdog::ScanDecision)> = Vec::new();
    for t in w.tasks.values() {
        if t.state != bm_contract::states::TaskState::Running {
            continue;
        }
        let created = crate::watchdog::parse_or(t.created_at.as_str(), now);
        if let Some(d) = w.watchdog.decide(t.id.as_str(), created, now) {
            decisions.push((t.id.clone(), d));
        }
    }
    for (tid, d) in decisions {
        match d {
            crate::watchdog::ScanDecision::Stall => {
                let elapsed_ms = {
                    let watch = w.watchdog.watches.get(tid.as_str());
                    watch
                        .map(|x| (now - x.last_progress_at).num_milliseconds())
                        .unwrap_or(0)
                };
                let last_seq = w
                    .watchdog
                    .watches
                    .get(tid.as_str())
                    .map(|x| x.last_progress_seq)
                    .unwrap_or(0);
                w.watchdog.mark_stall_notified(tid.as_str());
                w.emit(
                    EventType::TaskStalled,
                    None,
                    None,
                    None,
                    serde_json::json!({
                        "task_id": tid.as_str(),
                        "stalled_ms": elapsed_ms,
                        "last_progress_seq": last_seq,
                    }),
                );
                w.emit(
                    EventType::WatchdogReorchestrationTriggered,
                    None,
                    None,
                    None,
                    serde_json::json!({
                        "task_id": tid.as_str(),
                        "trigger": "watchdog",
                        "reason": "stalled_after_default_window",
                    }),
                );
                events += 2;
            }
            crate::watchdog::ScanDecision::HardLimit => {
                // 分阶段作用域:迁移完成后即释放 task 借用
                let Some((from, to, guard, snapshot)) = (|| {
                    let task = w.tasks.get_mut(&tid)?;
                    let (from, to, guard) = task
                        .transition(bm_contract::states::TaskState::Blocked, None, now)
                        .ok()?;
                    Some((from, to, guard, task.clone()))
                })() else {
                    continue;
                };
                w.emit(
                    EventType::TaskStateChanged,
                    None,
                    None,
                    None,
                    serde_json::json!({
                        "task_id": tid.as_str(),
                        "from": from.as_str(),
                        "to": to.as_str(),
                        "reason_code": guard,
                        "task_epoch": snapshot.task_epoch,
                    }),
                );
                persist_task(w, &snapshot);
                events += 1;
            }
        }
    }
    events
}

impl World {
    /// 到期自动扫描(核心循环节拍)。
    fn maybe_watchdog_scan(&mut self) {
        let now = self.config.clock.now();
        if !self.watchdog.due(now) {
            return;
        }
        watchdog_scan_run(self);
        self.watchdog.schedule_next(now);
    }

    /// 手动扫描(诊断/测试入口):忽略节拍,直接执行并重排下次。
    fn watchdog_scan_now(&mut self) -> usize {
        let now = self.config.clock.now();
        let n = watchdog_scan_run(self);
        self.watchdog.schedule_next(now);
        n
    }
}

/// Worker 声称完成 → Observation 核验 → 状态机终局(完成判定门禁):
/// - 声称所涉 Operation 的能力声明了 verification 钩子 → 执行 query 能力
///   (observation principal,read-only trusted 直通)做确定性核验;
/// - verified → completed(verified_completion);证据不足/无核验钩子 →
///   unverified → blocked(outcome_unknown_pending)等用户裁定;
/// - 观测写 Observation Log(observation.recorded 事件镜像)。
fn handle_task_report_completion(
    w: &mut World,
    task_id: BmId,
    claim_summary: String,
    operation_id: Option<BmId>,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝完成报告".into(),
        ));
    }
    let Some(task) = w.tasks.get(&task_id) else {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 不存在: {}", task_id.as_str()),
        ));
    };
    if !matches!(
        task.state,
        bm_contract::states::TaskState::Running | bm_contract::states::TaskState::Paused
    ) {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 状态 {} 不可受理完成报告", task.state.as_str()),
        ));
    }
    // 核验证据:声称所涉 Operation 的能力 verification 钩子
    let mut evidence: Vec<(String, String)> = Vec::new();
    let mut verdict = "unverified";
    if let Some(op_id) = &operation_id {
        evidence.push(("receipt".into(), op_id.as_str().to_string()));
        let capability = w.op_capability.get(op_id).cloned();
        if let Some(cap) = capability
            && let Some(manifest) = w.registry.manifest_of(&cap)
            && let Some(hook) = &manifest.verification
        {
            let query = hook["query"].as_str().unwrap_or_default().to_string();
            let expect = hook["expect"].as_str().unwrap_or("exists").to_string();
            if !query.is_empty() {
                // 观测查询:observation principal × read-only trusted → 直通
                let req = w.config.id_gen.next_id("req");
                let ctx = CallContext::surface("system:observation");
                let outcome = capability_call_inner(
                    w,
                    req,
                    ctx,
                    wire::CapabilityCallParams {
                        capability: query.clone(),
                        args: serde_json::json!({"subject": task_id.as_str()}),
                        idempotency_key: None,
                        deadline_ms: Some(2000),
                    },
                );
                match outcome {
                    Ok(result) => {
                        let satisfied = crate::observation::expect_satisfied(&result, &expect);
                        evidence.push(("state_check".into(), format!("{query} expect={expect}")));
                        verdict = match satisfied {
                            Some(true) => "verified",
                            _ => "unverified",
                        };
                    }
                    Err(_) => {
                        evidence.push(("state_check".into(), format!("{query} 不可得")));
                        verdict = "unverified";
                    }
                }
            }
        }
    }
    let now = w.config.clock.now();
    let now_ts = format_ts(now);
    // 状态机终局(门禁在 Task::transition:verified=false 不得 completed)
    let (from, to, guard, verified_flag) = {
        let Some(task) = w.tasks.get_mut(&task_id) else {
            return Err(CoreError::Internal);
        };
        if verdict == "verified" {
            let r = task
                .transition(bm_contract::states::TaskState::Completed, Some(true), now)
                .expect("verified → completed 是迁移表边");
            (r.0, r.1, r.2, true)
        } else {
            let r = task
                .transition(bm_contract::states::TaskState::Blocked, None, now)
                .map_err(|e| task_error_to_core(&task_id, e))?;
            (r.0, r.1, r.2, false)
        }
    };
    let guard_state = if verdict == "verified" {
        "completed"
    } else {
        "outcome_unknown"
    };
    // Observation Log 行 + observation.recorded 事件
    let entry = crate::observation::ObservationEntry {
        log_seq: 0,
        task_id: task_id.as_str().to_string(),
        agent_id: None,
        operation_id: operation_id.as_ref().map(|o| o.as_str().to_string()),
        claim_summary: claim_summary.clone(),
        evidence: evidence.clone(),
        verdict: if verdict == "verified" {
            "verified"
        } else {
            "unverified"
        },
        guard_state,
        observed_at: now_ts.clone(),
    };
    if let Some(store) = &w.store {
        let seq = store
            .save_observation(
                task_id.as_str(),
                entry.verdict,
                guard_state,
                &entry.to_contract_json(),
                &now_ts,
            )
            .unwrap_or(0);
        w.emit(
            EventType::ObservationRecorded,
            None,
            None,
            None,
            serde_json::json!({
                "task_id": task_id.as_str(),
                "log_seq": seq,
                "verdict": entry.verdict,
                "guard_state": guard_state,
            }),
        );
    }
    w.emit(
        EventType::TaskStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task_id.as_str(),
            "from": from.as_str(),
            "to": to.as_str(),
            "reason_code": guard,
            "task_epoch": w.tasks[&task_id].task_epoch,
        }),
    );
    let snapshot = w.tasks[&task_id].clone();
    persist_task(w, &snapshot);
    Ok(serde_json::json!({
        "task_id": task_id.as_str(),
        "verdict": entry.verdict,
        "verified": verified_flag,
        "state": snapshot.state.as_str(),
        "claim_digest": crate::observation::claim_digest(&claim_summary),
    }))
}

/// Task 预算扩容:更新包络 + 事实事件 + blocked 恢复(用户批准面)。
fn handle_task_budget_increase(
    w: &mut World,
    task_id: BmId,
    max_tool_calls: u64,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝扩容".into(),
        ));
    }
    // 分阶段作用域:包络更新与迁移完成后即释放 task 借用
    let (old_limit, snapshot, transition) = {
        let Some(task) = w.tasks.get_mut(&task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 不存在: {}", task_id.as_str()),
            ));
        };
        if task.is_terminal() {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                "终态 Task 不可扩容".into(),
            ));
        }
        let old_limit = task
            .budget
            .as_ref()
            .and_then(|b| b.extra.get("max_tool_calls"))
            .and_then(|v| match v {
                bm_contract::budget::ExtraValue::Int(n) => u64::try_from(*n).ok(),
                _ => None,
            })
            .unwrap_or(0);
        let budget = task
            .budget
            .get_or_insert_with(|| bm_contract::budget::Budget {
                max_tokens: u64::MAX,
                max_turns: u32::MAX,
                extra: Default::default(),
            });
        budget.extra.insert(
            "max_tool_calls".into(),
            bm_contract::budget::ExtraValue::Int(max_tool_calls as i64),
        );
        // blocked(budget_exhausted)的任务:扩容即用户裁定 → 恢复运行
        let mut transition = None;
        if task.state == bm_contract::states::TaskState::Blocked {
            let now = w.config.clock.now();
            let (from, to, guard) = task
                .transition(bm_contract::states::TaskState::Running, None, now)
                .expect("blocked→running 是迁移表边(user_resolved)");
            transition = Some((from, to, guard));
        }
        (old_limit, task.clone(), transition)
    };
    w.emit(
        EventType::TaskBudgetIncreased,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task_id.as_str(),
            "key": "max_tool_calls",
            "old_limit": old_limit,
            "new_limit": max_tool_calls,
            "approval_id": null,
        }),
    );
    if let Some((from, to, guard)) = transition {
        w.emit(
            EventType::TaskStateChanged,
            None,
            None,
            None,
            serde_json::json!({
                "task_id": task_id.as_str(),
                "from": from.as_str(),
                "to": to.as_str(),
                "reason_code": guard,
                "task_epoch": snapshot.task_epoch,
            }),
        );
    }
    persist_task(w, &snapshot);
    let state_after = snapshot.state;
    Ok(serde_json::json!({
        "task_id": task_id.as_str(),
        "max_tool_calls": max_tool_calls,
        "state": state_after.as_str(),
    }))
}

// ---- M6:Team 编队与委派(T2)----------------------------------------------

/// 追加 Worker 成员(M6.3):并发门禁(存活 worker ≤ 5)→ 授权链签发
/// (与自举同构,per-task principal)→ member.added。
fn handle_task_spawn_member(w: &mut World, task_id: BmId) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝成员追加".into(),
        ));
    }
    // 分阶段作用域:读任务与并发计数,门禁通过后即释放借用
    let (coord_aud, worker_aud, authorization) = {
        let Some(task) = w.tasks.get(&task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 不存在: {}", task_id.as_str()),
            ));
        };
        if task.state != bm_contract::states::TaskState::Running {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 状态 {} 不可追加成员", task.state.as_str()),
            ));
        }
        let alive_workers = task
            .members
            .iter()
            .filter(|m| m.role == crate::task::MemberRole::Worker)
            .count() as u64;
        if alive_workers >= crate::team::MAX_CONCURRENT_WORKERS {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!(
                    "并发上限:存活 worker {} 已达 {}",
                    alive_workers,
                    crate::team::MAX_CONCURRENT_WORKERS
                ),
            ));
        }
        (
            crate::team::coord_principal(task_id.as_str()),
            crate::team::worker_principal(task_id.as_str()),
            task.authorization.clone(),
        )
    };
    let now = w.config.clock.now();
    let (_coord_grants, worker_grants) = {
        let mut butler_lookup = |verb: &str| {
            w.grants
                .active_for(crate::butler::BUTLER_PRINCIPAL, verb, now)
                .into_iter()
                .next()
        };
        crate::coordinator::intersection_grants(
            &*w.config.id_gen,
            task_id.as_str(),
            &coord_aud,
            &worker_aud,
            &authorization,
            now,
            &mut butler_lookup,
        )
    };
    for g in worker_grants.iter() {
        w.grants.record(g.clone());
        persist_grant(w, &g.grant_id);
        w.emit(
            EventType::GrantCreated,
            None,
            None,
            None,
            serde_json::json!({
                "grant_id": g.grant_id,
                "approval_id": null,
                "audience": g.audience,
                "action": g.action,
                "scope": g.scope.to_wire(),
                "delegation_depth": g.delegation_depth,
                "expires_at": null,
                "parent_hash": g.parent_grant_hash,
            }),
        );
    }
    let member_id = w.config.id_gen.next_id("agent");
    let grant_id = worker_grants.first().map(|g| g.grant_id.clone());
    let ev = w.emit(
        EventType::TaskMemberAdded,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task_id.as_str(),
            "agent_id": member_id.as_str(),
            "role": "worker",
            "grant_id": grant_id,
        }),
    );
    {
        let Some(task) = w.tasks.get_mut(&task_id) else {
            return Err(CoreError::Internal);
        };
        task.add_member(crate::task::TaskMember {
            agent_id: member_id.clone(),
            role: crate::task::MemberRole::Worker,
            grant_id: grant_id.clone(),
            joined_seq: ev.event_seq,
        });
    }
    let snapshot = w.tasks[&task_id].clone();
    persist_task(w, &snapshot);
    Ok(serde_json::json!({
        "task_id": task_id.as_str(),
        "agent_id": member_id.as_str(),
        "grant_id": grant_id,
    }))
}

/// 委派子任务(M6.3/M6.5):四门禁(深度/授权子集/预算/并发)→ 子 Task
/// 创建 + 协调链自举。委派 = 子任务(规格 §5.2);权限只减不增。
fn handle_task_spawn_subtask(
    w: &mut World,
    params: SpawnSubtaskParams,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝委派".into(),
        ));
    }
    // 门禁(分阶段作用域:校验后即释放借用)
    let (parent_snapshot, coord_aud, _worker_aud, child_authorization) = {
        let Some(parent) = w.tasks.get(&params.parent_task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("父 Task 不存在: {}", params.parent_task_id.as_str()),
            ));
        };
        if !crate::team::depth_ok(parent.delegation_depth) {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!(
                    "委派深度超限:父深度 {} + 1 > {}",
                    parent.delegation_depth,
                    crate::team::MAX_DELEGATION_DEPTH
                ),
            ));
        }
        if !crate::team::authorization_subset(&params.authorization, &parent.authorization) {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                "委派授权必须为父授权的子集(成员权限只减不增)".into(),
            ));
        }
        let parent_max = crate::team::max_tool_calls_of(parent.budget.as_ref());
        let parent_used = *w.task_tool_calls.get(&params.parent_task_id).unwrap_or(&0);
        let child_max = crate::team::max_tool_calls_of(params.budget.as_ref());
        if !crate::team::budget_ok(child_max, parent_max, parent_used) {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                "子任务预算超父包络剩余(预算子分配门禁)".into(),
            ));
        }
        (
            parent.clone(),
            crate::team::coord_principal(params.parent_task_id.as_str()),
            crate::team::worker_principal(params.parent_task_id.as_str()),
            params.authorization.clone(),
        )
    };
    let now = w.config.clock.now();
    let parent_id_str = params.parent_task_id.as_str().to_string();
    // 子 Task 创建(wire 之外的内核委派路径;created_by = 父 Coordinator)
    let mut child = crate::task::Task::create(
        &*w.config.id_gen,
        params.title,
        params.goal,
        child_authorization,
        params.budget,
        None,
        Some(params.parent_task_id.clone()),
        parent_snapshot.delegation_depth + 1,
        now,
    );
    child.created_by = coord_aud.clone();
    w.emit(
        EventType::TaskCreated,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": child.id.as_str(),
            "title": child.title,
            "created_by": child.created_by,
            "parent_task_id": child.parent_task_id.as_ref().map(|p| p.as_str()),
        }),
    );
    let (from, to, guard) = child
        .transition(bm_contract::states::TaskState::Running, None, now)
        .expect("created→running 是迁移表边");
    w.emit(
        EventType::TaskStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": child.id.as_str(),
            "from": from.as_str(),
            "to": to.as_str(),
            "reason_code": guard,
            "task_epoch": child.task_epoch,
        }),
    );
    // 子任务协调链自举(per-child principal;Grant 链仍回溯 Butler 上界)
    let child_id_str = child.id.as_str().to_string();
    let child_coord_aud = crate::team::coord_principal(&child_id_str);
    let child_worker_aud = crate::team::worker_principal(&child_id_str);
    let (coord_grants, worker_grants) = {
        let mut butler_lookup = |verb: &str| {
            w.grants
                .active_for(crate::butler::BUTLER_PRINCIPAL, verb, now)
                .into_iter()
                .next()
        };
        crate::coordinator::intersection_grants(
            &*w.config.id_gen,
            &child_id_str,
            &child_coord_aud,
            &child_worker_aud,
            &child.authorization,
            now,
            &mut butler_lookup,
        )
    };
    for g in coord_grants.iter().chain(worker_grants.iter()) {
        w.grants.record(g.clone());
        persist_grant(w, &g.grant_id);
        w.emit(
            EventType::GrantCreated,
            None,
            None,
            None,
            serde_json::json!({
                "grant_id": g.grant_id,
                "approval_id": null,
                "audience": g.audience,
                "action": g.action,
                "scope": g.scope.to_wire(),
                "delegation_depth": g.delegation_depth,
                "expires_at": null,
                "parent_hash": g.parent_grant_hash,
            }),
        );
    }
    let coord_member_id = w.config.id_gen.next_id("agent");
    let coord_grant_id = coord_grants.first().map(|g| g.grant_id.clone());
    let ev = w.emit(
        EventType::TaskMemberAdded,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": child_id_str,
            "agent_id": coord_member_id.as_str(),
            "role": "coordinator",
            "grant_id": coord_grant_id,
        }),
    );
    child.add_member(crate::task::TaskMember {
        agent_id: coord_member_id,
        role: crate::task::MemberRole::Coordinator,
        grant_id: coord_grant_id,
        joined_seq: ev.event_seq,
    });
    if !worker_grants.is_empty() {
        let worker_member_id = w.config.id_gen.next_id("agent");
        let worker_grant_id = worker_grants[0].grant_id.clone();
        let ev = w.emit(
            EventType::TaskMemberAdded,
            None,
            None,
            None,
            serde_json::json!({
                "task_id": child_id_str,
                "agent_id": worker_member_id.as_str(),
                "role": "worker",
                "grant_id": worker_grant_id,
            }),
        );
        child.add_member(crate::task::TaskMember {
            agent_id: worker_member_id,
            role: crate::task::MemberRole::Worker,
            grant_id: Some(worker_grant_id),
            joined_seq: ev.event_seq,
        });
    }
    persist_task(w, &child);
    let result = serde_json::json!({
        "task_id": child.id.as_str(),
        "parent_task_id": parent_id_str,
        "delegation_depth": child.delegation_depth,
        "state": child.state.as_str(),
    });
    w.tasks.insert(child.id.clone(), child);
    Ok(result)
}

/// 成员移除(M6.3):member.removed 留痕 + 成员列表移除(墓碑语义)。
fn handle_task_remove_member(
    w: &mut World,
    params: RemoveMemberParams,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝成员移除".into(),
        ));
    }
    let removed = {
        let Some(task) = w.tasks.get_mut(&params.task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 不存在: {}", params.task_id.as_str()),
            ));
        };
        let before = task.members.len();
        task.members
            .retain(|m| m.agent_id.as_str() != params.agent_id.as_str());
        before != task.members.len()
    };
    if !removed {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("成员不存在: {}", params.agent_id.as_str()),
        ));
    }
    w.emit(
        EventType::TaskMemberRemoved,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": params.task_id.as_str(),
            "agent_id": params.agent_id.as_str(),
            "reason": params.reason,
        }),
    );
    let snapshot = w.tasks[&params.task_id].clone();
    persist_task(w, &snapshot);
    Ok(serde_json::json!({
        "task_id": params.task_id.as_str(),
        "agent_id": params.agent_id.as_str(),
        "removed": true,
    }))
}

/// 结果收集(M6.6):来源/状态/关联 Operation 三要素 + 子任务概览。
fn handle_task_collect(w: &World, task_id: BmId) -> CoreResult<serde_json::Value> {
    let Some(task) = w.tasks.get(&task_id) else {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 不存在: {}", task_id.as_str()),
        ));
    };
    let results = w.task_results.get(&task_id).cloned().unwrap_or_default();
    let children: Vec<serde_json::Value> = w
        .tasks
        .values()
        .filter(|t| t.parent_task_id.as_ref() == Some(&task_id))
        .map(|t| {
            serde_json::json!({
                "task_id": t.id.as_str(),
                "title": t.title,
                "state": t.state.as_str(),
                "delegation_depth": t.delegation_depth,
            })
        })
        .collect();
    Ok(serde_json::json!({
        "task_id": task_id.as_str(),
        "state": task.state.as_str(),
        "results": results,
        "children": children,
    }))
}

/// Worker 能力调用(Agent 路径):Task 必须在运行态;worker principal +
/// untrusted 上下文走统一执行体(Grant 命中直通 / 无授权 100% 升级审批)。
fn handle_worker_call(
    w: &mut World,
    request_id: BmId,
    params: WorkerCallParams,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝成员调用".into(),
        ));
    }
    // 分阶段作用域:状态检查完成后即释放 task 借用
    let state = {
        let Some(task) = w.tasks.get(&params.task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 不存在: {}", params.task_id.as_str()),
            ));
        };
        task.state
    };
    match state {
        bm_contract::states::TaskState::Running => {}
        bm_contract::states::TaskState::Paused => {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                "Task 暂停中,成员调用挂起".into(),
            ));
        }
        bm_contract::states::TaskState::Blocked => {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                "Task blocked,等待用户裁定".into(),
            ));
        }
        other => {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 状态 {} 不可执行成员调用", other.as_str()),
            ));
        }
    }
    // M5-T6:Task 包络「工具调用前」强制点(Broker 路径唯一执行出口:
    // 绕过 Broker 无预算执行出口——G 断言的结构面)
    let max_tool_calls = w
        .tasks
        .get(&params.task_id)
        .and_then(|t| t.budget.as_ref())
        .and_then(|b| b.extra.get("max_tool_calls"))
        .and_then(|v| match v {
            bm_contract::budget::ExtraValue::Int(n) => u64::try_from(*n).ok(),
            bm_contract::budget::ExtraValue::Float(f) => u64::try_from(*f as i64).ok(),
            _ => None,
        });
    let used = *w.task_tool_calls.entry(params.task_id.clone()).or_insert(0);
    if let Some(max) = max_tool_calls {
        // 软限 80%:budget.warning(基线 §9.7;逐次逼近即告警)
        if (used + 1) as f64 >= 0.8 * max as f64 && used < max {
            w.emit(
                EventType::BudgetWarning,
                None,
                None,
                None,
                serde_json::json!({
                    "agent_id": w.system_agent.as_str(),
                    "scope": format!("task:{}", params.task_id.as_str()),
                    "used_tokens": used + 1,
                    "limit_tokens": max,
                    "ratio": bm_contract::budget::round_ratio((used + 1) as f64, max as f64),
                }),
            );
        }
        // 硬限:拒绝 + Task blocked(budget_exhausted)等待用户裁定
        if used + 1 > max {
            w.emit(
                EventType::BudgetExceeded,
                None,
                None,
                None,
                serde_json::json!({
                    "agent_id": w.system_agent.as_str(),
                    "scope": format!("task:{}", params.task_id.as_str()),
                    "used_tokens": used,
                    "limit_tokens": max,
                }),
            );
            let now = w.config.clock.now();
            let (from, to, epoch) = {
                let Some(task) = w.tasks.get_mut(&params.task_id) else {
                    return Err(CoreError::Internal);
                };
                let epoch = task.task_epoch;
                let (from, to, _g) = task
                    .transition(bm_contract::states::TaskState::Blocked, None, now)
                    .expect("running→blocked 是迁移表边");
                (from, to, epoch)
            };
            w.emit(
                EventType::TaskStateChanged,
                None,
                None,
                None,
                serde_json::json!({
                    "task_id": params.task_id.as_str(),
                    "from": from.as_str(),
                    "to": to.as_str(),
                    "reason_code": "budget_exhausted",
                    "task_epoch": epoch,
                }),
            );
            let snapshot = w.tasks[&params.task_id].clone();
            persist_task(w, &snapshot);
            return Err(CoreError::Semantic(
                ErrorCode::BudgetExceeded,
                format!(
                    "Task {} 预算包络已耗尽(max_tool_calls={max}),转 blocked 等待用户裁定",
                    params.task_id.as_str()
                ),
            ));
        }
    }
    // Agent 路径信任归因:worker 上下文 = agent-derived/untrusted(内容
    // 来源链随任务传递,不可自报降级);Grant 命中优先,无授权则 100% 升级。
    // M6:per-task principal(跨 Task 结构性隔离)
    let ctx = CallContext::content_chain(
        crate::team::worker_principal(params.task_id.as_str()).as_str(),
        DataTrust::Untrusted,
    )
    .map_err(|_| CoreError::Internal)?;
    let outcome = capability_call_inner(
        w,
        request_id,
        ctx,
        wire::CapabilityCallParams {
            capability: params.capability.clone(),
            args: params.args.clone(),
            idempotency_key: params.idempotency_key.clone(),
            deadline_ms: params.deadline_ms,
        },
    );
    // 「返回后记账」+ 重复检测 + 进度信号(waiting_approval 豁免:等人的
    // 时间不算停滞,进度随审批挂起刷新)
    let outcome_str = match &outcome {
        Ok(_) => "ok",
        Err(CoreError::Semantic(ErrorCode::ApprovalRequired, _)) => "approval",
        Err(_) => "error",
    };
    let now = w.config.clock.now();
    let sig = crate::watchdog::call_sig(&params.capability, &params.args, outcome_str);
    let repeat_count = if outcome_str == "approval" {
        w.watchdog.mark_waiting(params.task_id.as_str(), now, 0);
        0
    } else {
        w.watchdog.note_call(params.task_id.as_str(), sig, now, 0)
    };
    if outcome_str != "approval" {
        *w.task_tool_calls.entry(params.task_id.clone()).or_insert(0) += 1;
        let used_now = w.task_tool_calls[&params.task_id];
        if let Some(store) = &w.store {
            let _ = store.save_task_budget(params.task_id.as_str(), "", used_now, 0, &w.now_ts());
        }
        // M6.6:结果流水(来源/状态/关联 Operation;collect 聚合面)
        let summary = match &outcome {
            Ok(r) => r["action_summary"].as_str().unwrap_or_default().to_string(),
            Err(_) => String::new(),
        };
        w.task_results
            .entry(params.task_id.clone())
            .or_default()
            .push(serde_json::json!({
                "agent_id": crate::team::worker_principal(params.task_id.as_str()),
                "operation_id": w.op_capability.keys().last().map(|k| k.as_str()).unwrap_or(""),
                "capability": params.capability,
                "state": if outcome.is_ok() { "succeeded" } else { "failed" },
                "action_summary": summary,
            }));
    }
    if repeat_count == crate::watchdog::REPEAT_THRESHOLD {
        w.emit(
            EventType::TaskRepeating,
            None,
            None,
            None,
            serde_json::json!({
                "task_id": params.task_id.as_str(),
                "agent_id": w.system_agent.as_str(),
                "capability": params.capability,
                "repeat_count": repeat_count,
            }),
        );
    }
    outcome
}

/// Butler 协调权撤销:撤销 bootstrap Grant 集(审计 grant.revoked),持久行
/// 同步(重启后不复活——materialize 尊重已撤销行)。
fn handle_butler_revoke(w: &mut World, reason: String) -> CoreResult<usize> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝撤销操作".into(),
        ));
    }
    let mut revoked = 0;
    for (verb, _) in crate::butler::COORDINATION_VERBS {
        // 分阶段作用域:收集后逐个撤销(避免跨字段借用)
        let gids: Vec<String> = w
            .grants
            .active_for(crate::butler::BUTLER_PRINCIPAL, verb, w.config.clock.now())
            .into_iter()
            .map(|g| g.grant_id)
            .collect();
        for gid in gids {
            let version = w.grants.revoke(&gid).map_err(|_| CoreError::Internal)?;
            w.emit(
                EventType::GrantRevoked,
                None,
                None,
                None,
                serde_json::json!({
                    "grant_id": gid,
                    "revocation_version": version,
                    "reason": reason,
                }),
            );
            persist_grant(w, &gid);
            revoked += 1;
        }
    }
    Ok(revoked)
}

/// task.list:全量任务投影为合同对象;(created_at, task_id) 字典序确定性。
/// state_filter 未识别的状态串 = 空列表(宽松过滤,合同未约束枚举面)。
fn handle_task_list(w: &World, params: wire::TaskListParams) -> CoreResult<wire::TaskListResult> {
    let mut tasks: Vec<&crate::task::Task> = w
        .tasks
        .values()
        .filter(|t| match &params.state_filter {
            Some(f) => t.state.as_str() == f,
            None => true,
        })
        .collect();
    tasks.sort_by(|a, b| (&a.created_at, a.id.as_str()).cmp(&(&b.created_at, b.id.as_str())));
    Ok(wire::TaskListResult {
        tasks: tasks
            .iter()
            .map(|t| serde_json::from_str(&task_contract_json(t)).unwrap_or_default())
            .collect(),
    })
}

/// task.get:规范对象 + 监护态投影(guard_states 随 T7 Watchdog 填充)。
fn handle_task_get(w: &World, params: wire::TaskGetParams) -> CoreResult<wire::TaskGetResult> {
    let Some(task) = w.tasks.get(&params.task_id) else {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 不存在: {}", params.task_id.as_str()),
        ));
    };
    Ok(wire::TaskGetResult {
        task: serde_json::from_str(&task_contract_json(task)).unwrap_or_default(),
        guard_states: None,
    })
}

/// TaskError → 统一错误信封(脱敏;epoch 拒绝带 reason_code 语义)。
fn task_error_to_core(task_id: &BmId, e: crate::task::TaskError) -> CoreError {
    let msg = match e {
        crate::task::TaskError::IllegalTransition { from, to } => format!(
            "Task {} 表外迁移: {} -> {}",
            task_id.as_str(),
            from.as_str(),
            to.as_str()
        ),
        crate::task::TaskError::UnverifiedCompletion => {
            format!("Task {} 无核验结论不得终局(完成判定门禁)", task_id.as_str())
        }
        crate::task::TaskError::StaleEpoch { current, presented } => format!(
            "Task {} 命令携带过期 epoch({presented},当前 {current}),Stale 拒绝",
            task_id.as_str()
        ),
    };
    CoreError::Semantic(ErrorCode::ValidationFailed, msg)
}

/// M7 S1:回合模型阶段终态审计(outcome=error;成功路径见 TurnEvent::Completed)。
fn emit_model_call_error_audit(w: &mut World, operation_id: &BmId, code: ErrorCode) {
    if let Some(a) = w.model_call_audit.remove(operation_id) {
        emit_capability_invoked_with(
            w,
            &a.call_id,
            operation_id,
            "model.invoke",
            &a.principal,
            Some(a.epoch),
            Some(&a.instance_id),
            "error",
            Some(code),
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)] // 审计字段与注册表 payload 键集一一对应
fn emit_capability_invoked(
    w: &mut World,
    op_id: &BmId,
    capability: &str,
    principal: &str,
    epoch: Option<u64>,
    instance: Option<&str>,
    outcome: &str,
    error_code: Option<ErrorCode>,
    key_hash: Option<&str>,
) {
    let call_id = w.config.id_gen.next_id("call");
    emit_capability_invoked_with(
        w, &call_id, op_id, capability, principal, epoch, instance, outcome, error_code, key_hash,
    );
}

/// 带预生成 call_id 的变体(turn 模型调用:授权点与审计点分离,M7 S1)。
#[allow(clippy::too_many_arguments)] // 审计字段与注册表 payload 键集一一对应
fn emit_capability_invoked_with(
    w: &mut World,
    call_id: &BmId,
    op_id: &BmId,
    capability: &str,
    principal: &str,
    epoch: Option<u64>,
    instance: Option<&str>,
    outcome: &str,
    error_code: Option<ErrorCode>,
    key_hash: Option<&str>,
) {
    w.emit(
        EventType::CapabilityInvoked,
        None,
        None,
        Some(op_id.clone()),
        serde_json::json!({
            "call_id": call_id.as_str(),
            "operation_id": op_id.as_str(),
            "capability": capability,
            "principal": principal,
            "binding_epoch": epoch.unwrap_or(0),
            "provider_instance_id": instance.unwrap_or("n/a"),
            "outcome": outcome,
            "error_code": error_code.map(|c| c.as_str()),
            "idempotency_key_hash": key_hash,
        }),
    );
}

fn error_code_of(outcome: &CallOutcome) -> ErrorCode {
    match outcome {
        CallOutcome::InvalidArgs { .. } => ErrorCode::ValidationFailed,
        CallOutcome::StaleBinding { .. } => ErrorCode::Unavailable,
        CallOutcome::ProviderError { .. } | CallOutcome::InvalidOutput { .. } => {
            ErrorCode::Internal
        }
        CallOutcome::ProviderUnavailable { .. } => ErrorCode::Unavailable,
        CallOutcome::Rejected { .. } => ErrorCode::PermissionDenied,
        _ => ErrorCode::Internal,
    }
}

fn sha256_hex(s: &str) -> String {
    use sha2::Digest;
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let out = h.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
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
            // M7 S5:失败回账(>=3 连续失败 -> 熔断开闸)
            note_provider_failure(w, w.config.connector.provider(), "模型调用连续失败");
        }
        TurnEvent::ChainExhausted {
            operation_id,
            error_code,
        } => {
            emit_model_call_error_audit(w, &operation_id, error_code);
            w.fail_turn(
                &operation_id,
                error_code,
                format!("模型降级链耗尽({error_code})"),
            );
            w.in_flight.remove(&operation_id);
        }
        TurnEvent::Cancelled { operation_id } => {
            emit_model_call_error_audit(w, &operation_id, ErrorCode::Cancelled);
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
            // M7 S5:成功回账(清计数/半开恢复 healthy)
            note_provider_success(w, w.config.connector.provider(), "模型调用成功");
            // M7 S1:模型调用审计(Broker 路径与普通能力调用同享 capability.invoked 面)
            if let Some(a) = w.model_call_audit.remove(&operation_id) {
                emit_capability_invoked_with(
                    w,
                    &a.call_id,
                    &operation_id,
                    "model.invoke",
                    &a.principal,
                    Some(a.epoch),
                    Some(&a.instance_id),
                    "ok",
                    None,
                    None,
                );
            }
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
        // M4:裁决后的审批落地在停机态仍应可答;capability.call 是新业务命令
        Cmd::CapabilityCall { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::ApprovalList { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::ApprovalRespond { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        // M5:task 命令组(停机态一律拒绝;查询面随 M8 只读残存评估)
        Cmd::TaskCreate { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::TaskLifecycle { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::TaskList { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::TaskGet { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::ButlerRevoke { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::WorkerCall { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::TaskSpawnMember { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::TaskSpawnSubtask { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::TaskRemoveMember { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::TaskCollect { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::TaskBudgetIncrease { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::WatchdogScan { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::TaskReportCompletion { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::EventsAll { resp } => {
            let _ = resp.send(Vec::new());
        }
        Cmd::Stop { resp, .. } => {
            let _ = resp.send(());
        }
        Cmd::ProviderCall { .. } => {}
        Cmd::ProviderProgress { .. } => {}
        Cmd::Turn(_) => {}
    }
}

#[cfg(test)]
mod t7_event_shape_tests {
    use super::*;

    /// T7 硬约束 3:命令语义形状在持久化前拒绝(G1 Bus 不得当 RPC)。
    #[test]
    fn command_semantic_payloads_are_rejected_before_persist() {
        let ty = EventType::SessionCreated;
        for bad_key in [
            "requested_action",
            "instruction",
            "command",
            "please_execute",
        ] {
            let payload = serde_json::json!({ bad_key: {"op": "mail.send"} });
            assert!(
                validate_event_shape(&ty, &payload).is_err(),
                "{bad_key} 形状必须被拒"
            );
        }
        // 正常事实载荷照常通过
        assert!(validate_event_shape(&ty, &serde_json::json!({"session_id": "x"})).is_ok());
    }
}
