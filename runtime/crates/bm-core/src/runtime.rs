//! Runtime 核心循环:全部事件与日志写入只在唯一的循环任务内发生(单写者),
//! 保证 event_seq/log_seq 的全局单调(INV-3/INV-4 的结构前提)。
//! 回合任务只通过内部命令通道回报,不直接改状态。

use crate::ports::persist::EventStore;

use crate::approval::{ApprovalError, ApprovalManager, OpenApproval, RespondDecision};
use crate::broker::{Broker, CallContext, CallOutcome, Decision, DenyReason, GrantLedger};
use crate::bus::EventBus;
use crate::clock::Clock;
use crate::exec_log::ExecutionLog;
use crate::limits::LimitsCell;
use crate::ports::{ModelConnector, SecretStore};
use crate::registry::{CapabilityProvider, CapabilityRegistry};
use crate::state::{Agent, Operation, Session, budget_from_spec};
use crate::{CoreError, CoreResult};
use bm_contract::budget::BudgetScope;
use bm_contract::capability::{Approval, DataTrust, GrantScope};
use bm_contract::connector::{
    BudgetCtx, InvokeRequest, InvokeResponse, Message, Role, ToolCallPayload,
};
use bm_contract::error_codes::ErrorCode;
use bm_contract::events::{EventEnvelope, EventType};
use bm_contract::exec_log::LogKind;
use bm_contract::ids::{BmId, IdGen};
use bm_contract::states::{AgentState, OperationState, SessionState};
use bm_contract::timestamp::format_ts;
use bm_contract::wire::{
    self, CancelParams, CancelResult, Cursor, EventsPollParams, EventsPollResult,
    GetOperationParams, PermissionMode, Principal, Receipt, SendInputParams, SessionCloseParams,
    SessionCloseResult, SessionCreateParams, SessionCreateResult, SessionDeleteParams,
    SessionDeleteResult, SessionResumeParams, SessionResumeResult, TaskType, WireError,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// 回合默认超时(每次模型调用的 deadline,秒)。GT-A3 原 30s——2026-09-03
/// VPS 实测:mimo 等真实网关常规调用 12~29s,30s 必现撞顶整回合失败,
/// 上调至 120;可用 BOEN_TURN_TIMEOUT_SECS(>0 整数秒)覆盖,由装配方
/// 经 limits 折算进 Cell(W10;此常量现为测试装配口径)。
pub const DEFAULT_TURN_TIMEOUT_SECS: i64 = 120;

/// 缺省模型 id(P2,2026-09-07 架构评审:此前 server/cli 三处魔法串散落)。
/// 语义 = 零配置时的模型标识占位;真实接入以 model.json/env 为准。
pub const DEFAULT_MODEL_ID: &str = "zhipu.glm-4-flash";

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
    /// W10(ADR-0024):运行时限制配置面。共享快照单元,消费点读时取值
    /// (热生效);env 覆盖由装配方在启动期折算进 Cell。
    pub limits: LimitsCell,
    /// W10(ADR-0025):后台作业台账门面(providers JobTable 实现);
    /// None = 无作业面(既有测试/纯内存形态零变化)。
    pub job_board: Option<Arc<dyn crate::ports::JobBoard>>,
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
        /// ADR-0029:脱敏后的 provider 错误原文(可空)——用户与日志
        /// 终于能看到「回合执行失败」背后的真实死因。
        detail: Option<String>,
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
    /// M11/ADR-0031:Task 公告栏投影(share.published 事件增量维护;可重建)。
    share_board: crate::share::TaskShareBoard,
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
    /// 会话累计成功回合计数(不裁剪)。台账受双上限裁剪,光靠存活条数
    /// 无法区分「新会话」与「旧轮已被遗忘」——存活数与累计数之差即被
    /// 遗忘轮数,context-inspector 的遗忘健康度以此为真实数据源。
    session_turn_totals: HashMap<BmId, u64>,
    /// W5 上下文透视:每次模型调用请求快照(context-log.jsonl;/admin/context)。
    ctx_log: Arc<crate::context_log::ContextLog>,
    /// #14 Turn 内调试日志(turn-debug.jsonl;默认关,管理面热开关)。
    turn_debug: Arc<crate::turn_debug::TurnDebugLog>,
}

impl World {
    /// 库内行不变量断言(issue #37 可选小改):fail-fast 语义不变,报错从
    /// 裸 expect 升级为「哪类行 + 哪个 id + 什么错」——恢复失败拒开时的
    /// 第一现场即可定位坏行,无需再开库排查。
    fn inv<T, E: std::fmt::Display>(kind: &str, id: &str, r: Result<T, E>) -> T {
        r.unwrap_or_else(|e| panic!("load_world_rows: 库内{kind}行不合法(id={id}): {e}"))
    }

    /// 同上,适配 from_wire 返回 Option 的形态。
    fn inv_opt<T>(kind: &str, id: &str, r: Option<T>) -> T {
        r.unwrap_or_else(|| panic!("load_world_rows: 库内{kind}行不合法(id={id})"))
    }

    /// 自规范状态行装配内存视图(M2 启动恢复,任务 T3)。
    /// request_id 未持久化(事件流不承载):以 op 的 ULID 段确定性合成 req_ 前缀 ID,
    /// 保证恢复幂等;action_summary/result_reference 为非持久展示字段,恢复后为占位。
    pub fn load_world_rows(
        &mut self,
        rows: crate::ports::persist::WorldRows,
        pending_interrupts: &mut Vec<(BmId, BmId, String)>,
        agents_to_resume: &mut Vec<(BmId, Option<BmId>)>,
    ) {
        for s in rows.sessions {
            let id = Self::inv("session", &s.id, BmId::parse(&s.id));
            let state = Self::inv_opt("session", &s.id, SessionState::from_wire(&s.state));
            self.sessions.insert(
                id.clone(),
                Session {
                    id: id.clone(),
                    agent_id: Self::inv("session→agent", &s.id, BmId::parse(&s.agent_id)),
                    state,
                    created_at: s.created_at,
                    // W8+重启续聊(2026-09-06):绑定随行持久装载
                    workspace_id: s.workspace_id,
                    // 会话目录(2026-09-08 三端一致批):标题/活跃时间随行装载
                    //(旧行为 None,启动期自 context-log 回填写平)
                    title: s.title,
                    updated_at: s.updated_at,
                    // ADR-0030:权限模式随行装载(迁移前旧行 None → ask)
                    permission_mode: s
                        .permission_mode
                        .as_deref()
                        .and_then(PermissionMode::from_wire)
                        .unwrap_or(PermissionMode::Ask),
                },
            );
        }
        for a in rows.agents {
            let id = Self::inv("agent", &a.id, BmId::parse(&a.id));
            let state = Self::inv_opt("agent", &a.id, AgentState::from_wire(&a.state));
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
                Self::inv("agent", &a.id, serde_json::from_str(&a.model_chain));
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
                    session_id: Self::inv("agent→session", &a.id, BmId::parse(&a.session_id)),
                    name: a.name,
                    model_chain: chain,
                    state,
                    budget,
                    system_prompt: None,
                    // 与 system_prompt 同语义:会话/角色进程内作用域,恢复为 None
                    allowed_tools: None,
                },
            );
        }
        for o in rows.operations {
            let id = Self::inv("operation", &o.id, BmId::parse(&o.id));
            let state = Self::inv_opt("operation", &o.id, OperationState::from_wire(&o.state));
            let request_id = match &o.request_id {
                Some(r) => Self::inv("operation→request", &o.id, BmId::parse(r)),
                None => Self::inv(
                    "operation→request 合成",
                    &o.id,
                    BmId::from_parts("req", id.ulid_part()),
                ),
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
                    session_id: Self::inv("operation→session", &o.id, BmId::parse(&o.session_id)),
                    agent_id: Self::inv("operation→agent", &o.id, BmId::parse(&o.agent_id)),
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
            let task = Self::inv(
                "task",
                &t.id,
                crate::task::task_from_row(&t).map_err(|e| e.to_string()),
            );
            self.tasks.insert(task.id.clone(), task);
        }
    }

    /// 会话相关事件读取:有持久层走日志(跨进程历史完整),否则走内存总线。
    fn events_for_session(
        &self,
        session_id: &BmId,
        since: u64,
        limit: u32,
    ) -> CoreResult<(Vec<EventEnvelope>, u64, bool)> {
        if let Some(store) = &self.store {
            // R1 收口(FULL-REVIEW-2026-09-05 §7):持久读失败如实上抛——
            // 此前折叠为空 = 回放假空历史,故障消音。
            let evs_all = store.replay_since(since).map_err(|e| {
                tracing::error!(error = %e, session = %session_id, "事件日志读取失败");
                CoreError::Semantic(
                    ErrorCode::Internal,
                    "持久事件日志读取失败,请检查数据目录或重启".into(),
                )
            })?;
            let last = store.last_log_seq().map_err(|e| {
                tracing::error!(error = %e, "事件日志位点读取失败");
                CoreError::Semantic(ErrorCode::Internal, "持久事件日志读取失败".into())
            })?;
            let mut evs: Vec<EventEnvelope> = evs_all
                .into_iter()
                .filter(|e| e.session_id.as_ref() == Some(session_id))
                .collect();
            let has_more = evs.len() > limit as usize;
            evs.truncate(limit as usize);
            Ok((evs, last, has_more))
        } else {
            Ok(self.bus.poll(session_id, since, limit))
        }
    }

    /// Task 事件流读取(watch 观察面):跨会话按 payload.task_id 过滤。
    fn events_for_task(
        &self,
        task_id: &BmId,
        since: u64,
        limit: u32,
    ) -> CoreResult<(Vec<EventEnvelope>, u64, bool)> {
        let mut evs: Vec<EventEnvelope> = if let Some(store) = &self.store {
            // R1 收口:持久读失败如实上抛(同 events_for_session)
            store.replay_since(since).map_err(|e| {
                tracing::error!(error = %e, "事件日志读取失败(task 流)");
                CoreError::Semantic(ErrorCode::Internal, "持久事件日志读取失败".into())
            })?
        } else {
            self.bus.events().to_vec()
        };
        let last = match &self.store {
            Some(store) => store.last_log_seq().map_err(|e| {
                tracing::error!(error = %e, "事件日志位点读取失败(task 流)");
                CoreError::Semantic(ErrorCode::Internal, "持久事件日志读取失败".into())
            })?,
            None => self.bus.last_seq(),
        };
        evs.retain(|e| e.payload["task_id"].as_str() == Some(task_id.as_str()));
        let has_more = evs.len() > limit as usize;
        evs.truncate(limit as usize);
        Ok((evs, last, has_more))
    }

    /// 写命令统一门禁:排空中或持久层故障时拒绝业务写命令(`what` 为"拒绝"的宾语)。
    fn gate_writes(&self, what: &str) -> CoreResult<()> {
        if self.draining || self.persist_poisoned {
            return Err(CoreError::Semantic(
                ErrorCode::Unavailable,
                format!("Runtime 排空中或持久层故障,拒绝{what}"),
            ));
        }
        Ok(())
    }

    /// Grant 签发事实事件:各签发路径共用的 GrantCreated 载荷。
    /// `approval_id` None → null;`expires_at` 取 Grant 自身(签发路径均为永不过期 → null)。
    fn emit_grant_created(
        &mut self,
        g: &bm_contract::capability::Grant,
        approval_id: Option<&str>,
        operation_id: Option<BmId>,
    ) -> EventEnvelope {
        self.emit(
            EventType::GrantCreated,
            None,
            None,
            operation_id,
            serde_json::json!({
                "grant_id": g.grant_id,
                "approval_id": approval_id,
                "audience": g.audience,
                "action": g.action,
                "scope": g.scope.to_wire(),
                "delegation_depth": g.delegation_depth,
                "expires_at": g.expires_at,
                "parent_hash": g.parent_grant_hash,
                "resource": serde_json::to_value(&g.resource).expect("resource 序列化"),
            }),
        )
    }

    /// 撤销单条 Grant 三件套:台账 revoke + GrantRevoked 事件 + 持久行。
    fn revoke_grant_and_emit(&mut self, gid: &str, reason: &str) -> CoreResult<()> {
        let version = self.grants.revoke(gid).map_err(|_| CoreError::Internal)?;
        self.emit(
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
        turn::persist_grant(self, gid);
        Ok(())
    }

    fn now_ts(&self) -> bm_contract::BmTimestamp {
        format_ts(self.config.clock.now())
    }

    /// 校验工作区是否已登记(W8 ADR-0018:注册表 = config/workspaces.json)
    pub(crate) fn validate_workspace(&self, wid: &str) -> CoreResult<()> {
        let ok = self
            .config
            .data_dir
            .as_ref()
            .map(|d| crate::workspace::is_registered(d, wid))
            .unwrap_or(false);
        if !ok {
            // 扩展码结构化(issue #40):前端按 code 分支,不再串匹配文案
            return Err(CoreError::Extension {
                message: format!("工作区「{wid}」未登记或已删除(设置 → 常规 里维护)"),
                ext_code: "webui.workspace_unavailable",
                base: ErrorCode::ValidationFailed,
            });
        }
        Ok(())
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
        if let Err(reason) = shape_err {
            // R2(FULL-REVIEW-2026-09-05 §7,INV-3):坏形状事件此前「占 seq 但
            // 只进内存总线不落盘」,存储侧自此永久跳号。改为 tombstone 占位:
            // 原 seq 槽落 StoreWriteRejected(持久+总线),日志保持连续
            // (Judge contiguous 可验);坏事件本体不再进总线(T7:非事实不分发)。
            let tombstone = EventEnvelope::new(
                seq,
                EventType::StoreWriteRejected,
                self.now_ts(),
                None,
                None,
                None,
                // 键集须与合同注册表精确一致(key/reason);类型信息已在
                // reason 文案内(「事件 xxx 携带…」),不扩键
                serde_json::json!({
                    "key": seq.to_string(),
                    "reason": reason,
                }),
            );
            if let Some(store) = &self.store
                && !self.persist_poisoned
                && let Err(e) = store.record(&tombstone)
            {
                // tombstone 自身写失败:与正常事件写失败同口径处理(见下)
                tracing::error!(error = %e, seq = %seq, "拒写 tombstone 落盘失败,Runtime 进入拒写态");
                self.persist_poisoned = true;
            }
            self.bus.append(tombstone.clone());
            return tombstone;
        }
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
        // M11/ADR-0031:share.published 事件增量入公告栏投影(与 task_board
        // 同一单写者时点;增量与重建两条路径等价有测试)
        self.share_board.apply(&event);
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
    /// P0(2026-09-07 架构评审):表外迁移收敛为可观测错误——记 exec_log 后
    /// 原样返回,不再 panic 打崩进程(状态保持原样,终态由已落定的一方为准)。
    fn settle_operation(&mut self, op_id: &BmId, to: OperationState, error: Option<WireError>) {
        let now = self.now_ts();
        let (session_id, agent_id, from, to, reason) = {
            let Some(op) = self.operations.get_mut(op_id) else {
                tracing::warn!(operation = %op_id.as_str(), "settle_operation: operation 已不存在(可能随会话删除被清理),跳过");
                return;
            };
            let (from, to, reason) = match op.settle(to, error, now.clone()) {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!(
                        operation = %op_id.as_str(),
                        from = ?e.from,
                        to = ?e.to,
                        "表外迁移被拒绝(不落定,保持原状态)"
                    );
                    self.exec_log.record(crate::exec_log::LogRecord {
                        kind: LogKind::Error,
                        session_id: op.session_id.clone(),
                        agent_id: op.agent_id.clone(),
                        operation_id: op_id.clone(),
                        request_id: Some(op.request_id.clone()),
                        agent_state: "n/a".into(),
                        detail: serde_json::json!({
                            "message": "表外迁移被拒绝",
                            "from": e.from.as_str(),
                            "to": e.to.as_str(),
                        }),
                        ts: now,
                    });
                    return;
                }
            };
            (op.session_id.clone(), op.agent_id.clone(), from, to, reason)
        };
        // 会话目录 updated_at 内存投影(2026-09-08 三端一致批):与
        // materialize 对本事件的 sessions.updated_at 物化同源同刻;系统容器
        // 操作无 session 行,get_mut 为 None 自然跳过
        if let Some(s) = self.sessions.get_mut(&session_id) {
            s.updated_at = Some(now.clone());
        }
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
            let Some(op) = self.operations.get(operation_id) else {
                tracing::warn!(operation = %operation_id.as_str(), "fail_turn: operation 已不存在,跳过");
                return;
            };
            let Some(a) = self.agents.get(&op.agent_id) else {
                tracing::warn!(operation = %operation_id.as_str(), agent = %op.agent_id.as_str(), "fail_turn: agent 已不存在,跳过");
                return;
            };
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
            if let Some(a) = self.agents.get_mut(&agent_id) {
                if AgentState::can_transition(a.state, AgentState::Failed) {
                    a.transition(AgentState::Failed);
                } else {
                    tracing::warn!(agent = %agent_id.as_str(), state = ?a.state, "fail_turn: agent 无法迁移至 Failed,跳过");
                }
            }
        }
        // 强制点③补充(2026-09-05 回看):失败回合占回合配额,失败重试
        // 不得绕过 max_turns 烧钱(网关对失败调用同样可能计费)
        let turns_exhausted = {
            if let Some(a) = self.agents.get_mut(&agent_id) {
                a.budget.account_failed_turn()
            } else {
                false
            }
        };
        if turns_exhausted {
            let (used, limit) = {
                let a = &self.agents[&agent_id];
                (a.budget.used_tokens, a.budget.max_tokens)
            };
            self.emit(
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
                        Some(store) => match store.replay_since(0) {
                            Ok(events) => events,
                            Err(e) => {
                                tracing::warn!(error = %e, "事件流重放失败,轨迹视图降级为空");
                                Vec::new()
                            }
                        },
                        None => world.bus.events().to_vec(),
                    };
                    let _ = resp.send(events);
                }
                Cmd::GetOperation { params, resp } => {
                    let _ = resp.send(handle_get_operation(&world, params));
                }
                // 会话目录只读查询(2026-09-08):停机残存态照常应答
                Cmd::SessionList { resp } => {
                    let _ = resp.send(handle_session_list(&world));
                }
                // Provider 健康只读查询(issue #12):停机残存态照常应答
                Cmd::ProviderHealth { resp } => {
                    let _ = resp.send(handle_provider_health(&world));
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
            // 会话目录列表(2026-09-08 三端一致批;GET /admin/sessions)
            Cmd::SessionList { resp } => {
                let _ = resp.send(handle_session_list(&world));
            }
            // 会话权限模式变更(ADR-0030;POST /admin/sessions/{sid}/mode)
            Cmd::SessionSetMode {
                request_id: _request_id, // 审计以 session.mode.changed 事件为准
                session_id,
                mode,
                resp,
            } => {
                let _ = resp.send(handle_session_set_mode(&mut world, session_id, mode));
            }
            // Provider 健康快照(issue #12;GET /admin/providers/health)
            Cmd::ProviderHealth { resp } => {
                let _ = resp.send(handle_provider_health(&world));
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
            Cmd::SessionDelete {
                request_id,
                params,
                resp,
            } => {
                let _ = resp.send(handle_session_delete(&mut world, request_id, params));
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
            Cmd::CapabilitiesUnregister { capabilities, resp } => {
                let _ = resp.send(handle_capabilities_unregister(&mut world, capabilities));
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
                        "delta": content_trunc_with(&delta, world.config.limits.get().audit_entry_max_chars),
                    }),
                );
            }
            Cmd::Cancel { params, resp } => {
                let _ = resp.send(handle_cancel(&mut world, params));
            }
            Cmd::OperationCancel { operation_id, resp } => {
                let _ = resp.send(handle_operation_cancel(&mut world, operation_id));
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
                session_id,
                resp,
            } => {
                let _ = resp.send(handle_capability_call(
                    &mut world, request_id, params, session_id,
                ));
            }
            Cmd::CapabilityList { params, resp } => {
                let _ = resp.send(handle_capability_list(&world, params));
            }
            Cmd::ApprovalList { params, resp } => {
                let _ = resp.send(handle_approval_list(&mut world, params));
            }
            Cmd::ApprovalRespond {
                request_id,
                params,
                source,
                resp,
            } => {
                let _ = resp.send(handle_approval_respond(
                    &mut world, request_id, params, source,
                ));
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
                    Some(store) => match store.replay_since(0) {
                        Ok(events) => events,
                        Err(e) => {
                            tracing::warn!(error = %e, "事件流重放失败,轨迹视图降级为空");
                            Vec::new()
                        }
                    },
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
            // W4b 对话内审批:审批请求经 ProviderDelta 形态进入事件面
            // (前端按标记渲染审批卡片);此处仅透传,不改变核心状态。
            Cmd::ApprovalRequested {
                approval_id,
                capability,
                args,
                operation_id,
            } => {
                let marker = serde_json::json!({
                    "bm_approval_request": {
                        "approval_id": approval_id,
                        "capability": capability,
                        "args": args,
                        "operation_id": operation_id.as_str(),
                    }
                });
                let _ = world.tx.try_send(Cmd::ProviderDelta {
                    operation_id,
                    delta: format!("\n[BM_APPROVAL:{}]\n", marker),
                });
            }
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
