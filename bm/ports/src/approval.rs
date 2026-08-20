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
    /// 请求审批一个工具调用。`tool_name` + `call_id` 用于前端展示与匹配；
    /// `reason` 为可选提示文本（如 workdir 外路径、shell 命令文本）。
    /// 返回裁定：Allowed / Rejected（含超时拒绝）。
    /// 实现侧出错（登记失败等）必须 Err——fail-loud，调用方按拒绝处理。
    async fn request_approval(
        &self,
        tool_name: &str,
        call_id: &str,
        reason: Option<String>,
    ) -> Result<ApprovalVerdict, ToolError>;
}