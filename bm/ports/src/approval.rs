//! 工具审批契约（产品契约层扩展）。
//!
//! 插件 #2：危险工具调用在 loop 执行前经此端口暂停、推到前端审批弹窗，
//! 用户 allowed-once / rejected 后再继续。契约只留**消费面**（loop 侧）——
//! 装配面（PendingRegistry 登记、oneshot 等待、approval/resolved 帧广播、
//! respond 路由回拨）由 web-server 的 `ApprovalRouter` 具体实现承载。
//!
//! 可选装配（`Option`）：未装配 = 审批面禁用（不暂停，直接放行）——与既有
//! host.* 工具的默认自动执行语义一致；装配后仅 **声明了需要审批的工具**
//! 会暂停。fail-loud：已装配但审批本身出错 → 按拒绝处理（不静默放行危险工具）。
//!
//! 依赖纪律同 compactor/WorkdirPort：本 crate 只依赖 kernel-contracts（纯契约）。

use std::time::Duration;

use kernel_contracts::ToolError;

/// 审批默认超时（用户长时间未应答 → 自动拒绝，防审批悬挂卡死回合）。
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(600);

/// 审批裁定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalVerdict {
    /// 用户允许本次调用（allowed-once）。
    Allowed,
    /// 用户拒绝 / 超时未应答 / 审批面错误。
    Rejected,
}

/// 工具审批端口（loop 消费面）：把一次待审批工具调用登记并等待用户裁定。
#[async_trait::async_trait]
pub trait ToolApprovalPort: Send + Sync + std::fmt::Debug {
    /// 请求审批一个工具调用。`session_id` 为发起调用的会话（供前端豁免表
    /// 按会话区分、设置页管理面按会话展示）；`tool_name` + `call_id` 用于
    /// 前端展示与匹配；`reason` 为可选提示文本（如 workdir 外路径、shell
    /// 命令文本）。返回裁定：Allowed / Rejected（含超时拒绝）。
    /// 实现侧出错（登记失败等）必须 Err——fail-loud，调用方按拒绝处理。
    async fn request_approval(
        &self,
        session_id: &str,
        tool_name: &str,
        call_id: &str,
        reason: Option<String>,
    ) -> Result<ApprovalVerdict, ToolError>;
}
/// 中立 mux 帧（插件产出；宿主转自家 server-request 信封下行）。
#[derive(Debug, Clone)]
pub struct MuxFrameOut {
    pub rpc_id: String,
    pub method: String,
    pub payload: serde_json::Value,
}

/// 审批中心面（万物皆插件②，2026-08-22）：pending 表与 respond 路由住在
/// plugin-approval；宿主的 `POST /api/respond` 路由、mux 断连重放与测试钩子
/// 经本端口委托。回执 reason 逐字（"bad-response" / "not-pending"）。
pub trait ApprovalFacePort: Send + Sync + std::fmt::Debug {
    /// /api/respond 路由（approval 表先、question 表后）：校验应答负载、
    /// 移除 pending、唤醒等待者、广播 resolved 帧。返回 (accepted, reason)。
    fn respond(&self, rpc_id: &str, result: &serde_json::Value) -> (bool, Option<&'static str>);

    /// mux 重放帧（仍 pending 的 approval/question requested；rpcId 原样复用）。
    fn pending_frames(&self) -> Vec<MuxFrameOut>;

    /// 测试钩子登记（BM_TEST_HOOKS 门在宿主侧）：登记 + 广播 requested 帧。
    fn register_test_approval(
        &self,
        rpc_id: String,
        session_id: String,
        approval_id: String,
        tool_name: String,
        call_id: Option<String>,
        reason: Option<String>,
    );

    /// 测试钩子登记（question 表）。
    fn register_test_question(&self, rpc_id: String, session_id: String, questions: serde_json::Value);
}
