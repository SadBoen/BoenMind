//! Wire 命令枚举与不可用应答(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

use super::*;

pub(crate) enum Cmd {
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
    SessionDelete {
        request_id: BmId,
        params: SessionDeleteParams,
        resp: oneshot::Sender<CoreResult<SessionDeleteResult>>,
    },
 /// 会话目录列表(
 /// 只读查询,停机/排空态照常应答,同 EventsAll 口径)。
    SessionList {
        resp: oneshot::Sender<Vec<crate::state::SessionSummary>>,
    },
 /// 会话权限模式变更(ADR-0030;POST /admin/sessions/{sid}/mode):
 /// 服务端会话状态更新 + session.mode.changed 事实事件。
    SessionSetMode {
        request_id: BmId,
        session_id: BmId,
        mode: wire::PermissionMode,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
 /// Provider 熔断健康快照(issue #12;GET /admin/providers/health 读模型;
 /// 只读查询,停机/排空态照常应答,同 SessionList 口径)。
    ProviderHealth {
        resp: oneshot::Sender<Vec<(String, crate::runtime::ProviderHealth)>>,
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
 /// 管理面按 operation_id 直接取消(无需前台凑齐 session_id/agent_id)
    OperationCancel {
        operation_id: BmId,
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
 /// session_id(ADR-0030):回合层模型工具调用标注来源会话,裁决点据此
 /// 读取会话权限模式;None = 无会话上下文(恒按 ask)。
    CapabilityCall {
        request_id: BmId,
        params: wire::CapabilityCallParams,
        session_id: Option<BmId>,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
 /// capability.list(M4):能力发现面 Wire 暴露。
    CapabilityList {
        params: wire::CapabilityListParams,
        resp: oneshot::Sender<CoreResult<wire::CapabilityListResult>>,
    },
 /// approval.list(M4):待裁决审批列表。
    ApprovalList {
        params: wire::ApprovalListParams,
        resp: oneshot::Sender<CoreResult<serde_json::Value>>,
    },
 /// approval.respond(M4):批准(物化 Grant 并重放执行)/拒绝/取消。
 /// source(ADR-0030):裁决来源审计标注,由服务端派生,不信客户端。
    ApprovalRespond {
        request_id: BmId,
        params: wire::ApprovalRespondParams,
        source: crate::approval::ResolvedSource,
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
 /// W2 热装载:运行期追加注册能力(MCP 管理面重载;只增,不改/删仍走重启)。
    CapabilitiesRegister {
        entries: Vec<(
            bm_contract::capability::CapabilityManifest,
            std::sync::Arc<dyn crate::registry::CapabilityProvider>,
        )>,
        resp: oneshot::Sender<CoreResult<Vec<String>>>,
    },
 /// 热拔/重载摘除能力(MCP server 卸载或更新前摘除原能力)。
    CapabilitiesUnregister {
        capabilities: Vec<String>,
        resp: oneshot::Sender<CoreResult<Vec<String>>>,
    },
    Stop {
        reason: String,
        resp: oneshot::Sender<()>,
    },
 /// M8.1:查询异步能力调用结果(诊断端口;非 wire 方法)。
    GetOpResult {
        operation_id: BmId,
        resp: oneshot::Sender<CoreResult<Option<serde_json::Value>>>,
    },
 /// M8.3:能力调用语义取消(在途异步;迟到完成丢弃)。
    CapabilityCancel {
 #[allow(dead_code)] // 信封规范要求请求携带 request_id;回执以 operation 为准
        request_id: BmId,
        params: wire::CapabilityCancelParams,
        resp: oneshot::Sender<CoreResult<wire::CapabilityCancelResult>>,
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
 /// M9-S2:回合模型输出增量(流式开启时逐块回核心循环,单写者落事件)
    ProviderDelta {
        operation_id: BmId,
        delta: String,
    },
    TaskAutorun {
        request_id: BmId,
        params: bm_contract::wire::TaskAutorunParams,
        resp: oneshot::Sender<CoreResult<bm_contract::wire::TaskAutorunResult>>,
    },
    Turn(TurnEvent),
 /// W5:成功回合的对话台账回写(session_chats;历史回喂的数据源)。
    RememberTurn {
        session_id: BmId,
        user: String,
        assistant: String,
    },
 /// W4b 对话内审批:回合任务向 UI 通道推送审批请求卡片
 /// (随 ProviderDelta 进 SSE/事件面,前端据此渲染审批卡片)。
    ApprovalRequested {
        approval_id: String,
        capability: String,
        args: serde_json::Value,
        operation_id: BmId,
    },
}

/// Task 生命周期动作(M5-T1;completed/failed 无 wire 入口——完成判定门禁
/// 在 T8 Observation 核验路径上)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskAction {
    Pause,
    Resume,
    Stop,
}

/// 排空期对非回合命令的统一拒绝(保留应答,不悬挂调用方)。
/// 结构说明():本表与 `core_loop`、`rpc_inner` 合称
/// 「三张表」——它们是**三种不同操作**(统一拒绝 / 真实派发 / 参数解码)在
/// 同一 `Cmd` 变体集上的投影,不是同一张表抄三份。新增变体时三处都会**编译
/// 报错**(无 `_ => {}` 兜底),这是刻意的:穷尽性由编译器强制,不会静默漏臂。
/// 故本结构保留;仅把占多数的同形臂收敛为一行。
pub(crate) fn reply_unavailable(cmd: Cmd) {
 /// 同形臂:应答 `Unavailable`(类型泛化覆盖各变体的不同 Result 载荷)。
    fn refuse<T>(resp: oneshot::Sender<CoreResult<T>>) {
        let _ = resp.send(Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中".into(),
        )));
    }
    match cmd {
        Cmd::SessionCreate { resp, .. } => refuse(resp),
        Cmd::SessionResume { resp, .. } => refuse(resp),
        Cmd::SessionClose { resp, .. } => refuse(resp),
        Cmd::SessionDelete { resp, .. } => refuse(resp),
        Cmd::EventsPoll { resp, .. } => refuse(resp),
        Cmd::SendInput { resp, .. } => refuse(resp),
        Cmd::Cancel { resp, .. } => refuse(resp),
        Cmd::OperationCancel { resp, .. } => refuse(resp),
        Cmd::GetOperation { resp, .. } => refuse(resp),
        Cmd::RecoverySettle { resp, .. } => refuse(resp),
 // M4:裁决后的审批落地在停机态仍应可答;capability.call 是新业务命令
        Cmd::CapabilityCall { resp, .. } => refuse(resp),
        Cmd::CapabilityList { resp, .. } => refuse(resp),
        Cmd::ApprovalList { resp, .. } => refuse(resp),
        Cmd::ApprovalRespond { resp, .. } => refuse(resp),
 // ADR-0030:会话权限模式变更是写命令,排空期拒绝
        Cmd::SessionSetMode { resp, .. } => refuse(resp),
 // M5:task 命令组(停机态一律拒绝;查询面随 M8 只读残存评估)
        Cmd::TaskCreate { resp, .. } => refuse(resp),
        Cmd::TaskLifecycle { resp, .. } => refuse(resp),
        Cmd::TaskList { resp, .. } => refuse(resp),
        Cmd::TaskGet { resp, .. } => refuse(resp),
        Cmd::ButlerRevoke { resp, .. } => refuse(resp),
        Cmd::WorkerCall { resp, .. } => refuse(resp),
        Cmd::TaskSpawnMember { resp, .. } => refuse(resp),
        Cmd::TaskSpawnSubtask { resp, .. } => refuse(resp),
        Cmd::TaskRemoveMember { resp, .. } => refuse(resp),
        Cmd::TaskCollect { resp, .. } => refuse(resp),
        Cmd::TaskBudgetIncrease { resp, .. } => refuse(resp),
        Cmd::WatchdogScan { resp, .. } => refuse(resp),
        Cmd::TaskReportCompletion { resp, .. } => refuse(resp),
        Cmd::TaskAutorun { resp, .. } => refuse(resp),
 // W2 热装载:停机态拒绝(能力注册只在运行态有意义)
        Cmd::CapabilitiesRegister { resp, .. } => refuse(resp),
        Cmd::CapabilitiesUnregister { resp, .. } => refuse(resp),
        Cmd::CapabilityCancel { resp, .. } => refuse(resp),
 // ---- 异形臂(行为各异,刻意逐条) ----------------------------------
        Cmd::EventsAll { resp } => {
            let _ = resp.send(Vec::new());
        }
 // 会话目录只读查询:不可用态应答空列表(不悬挂调用方)
        Cmd::SessionList { resp } => {
            let _ = resp.send(Vec::new());
        }
 // Provider 健康只读查询:不可用态应答空快照(不悬挂调用方)
        Cmd::ProviderHealth { resp } => {
            let _ = resp.send(Vec::new());
        }
        Cmd::GetOpResult { resp, .. } => {
            let _ = resp.send(Ok(None));
        }
        Cmd::Stop { resp, .. } => {
            let _ = resp.send(());
        }
 // 无应答方/自身即回合:静默(进程将终)
        Cmd::ProviderCall { .. } => {}
        Cmd::ProviderProgress { .. } => {}
        Cmd::ProviderDelta { .. } => {}
        Cmd::Turn(_) => {}
        Cmd::RememberTurn { .. } => {}
        Cmd::ApprovalRequested { .. } => {}
    }
}
