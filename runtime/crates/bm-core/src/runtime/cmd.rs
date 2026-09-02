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
    /// W2 热装载:运行期追加注册能力(MCP 管理面重载;只增,不改/删仍走重启)。
    CapabilitiesRegister {
        entries: Vec<(
            bm_contract::capability::CapabilityManifest,
            std::sync::Arc<dyn crate::registry::CapabilityProvider>,
        )>,
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
    /// W4b 对话内审批:反查审批单对应的 operation(等待轮询用)。
    GetApprovalOp {
        approval_id: String,
        resp: oneshot::Sender<Option<BmId>>,
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
pub(crate) fn reply_unavailable(cmd: Cmd) {
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
        Cmd::ProviderDelta { .. } => {}
        Cmd::TaskAutorun { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        // W2 热装载:停机态拒绝(能力注册只在运行态有意义)
        Cmd::CapabilitiesRegister { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::CapabilityCancel { resp, .. } => {
            let _ = resp.send(Err(err()));
        }
        Cmd::GetOpResult { resp, .. } => {
            let _ = resp.send(Ok(None));
        }
        Cmd::Turn(_) => {}
        // W5:台账回写无应答方;排空期与 Turn 同口径静默应用即可(进程将终)
        Cmd::RememberTurn { .. } => {}
        // W4b:排空期审批请求/反查按排空口径静默
        Cmd::ApprovalRequested { .. } => {}
        Cmd::GetApprovalOp { resp, .. } => {
            let _ = resp.send(None);
        }
    }
}
