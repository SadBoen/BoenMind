//! 工具审批端口（web-server 实现）：把 loop 侧的危险工具调用暂停 + 推审批弹窗 +
//! 等用户裁定。
//!
//! 接线：`ApprovalRouter` 持有 `Arc<AppState>`（PendingRegistry 登记 + mux 帧广播 +
//! approval_waiters 等待表）。loop 执行危险工具前调 `request_approval`：
//! 1. 生成 approval_id（uuid）；
//! 2. 登记进 PendingRegistry（approval/requested 帧 → WS 下行 + 断连重放，带发起
//!    会话 id——供前端豁免表/设置页管理面按会话展示）；
//! 3. 建 oneshot 通道存入 approval_waiters；
//! 4. 等待裁定（最多 [`bm_ports::APPROVAL_TIMEOUT`]，超时 → 拒绝）；
//! 5. respond_dispatch（allowed-once/rejected）经 approval_waiters 唤醒等待者。
//!
//! fail-loud：登记/广播失败 → Err（loop 按拒绝处理，不静默放行危险工具）。

use std::sync::Arc;

use async_trait::async_trait;
use bm_ports::{ApprovalVerdict, APPROVAL_TIMEOUT};

use crate::api::AppState;

/// 工具审批端口实现（经 bm-assembly `install_approval` 装配进 loop）。
/// `Arc<AppState>` 与 SettingsWorkdir 同构：现读 AppState 的 pending/广播/等待表。
/// Debug 手写骨架（AppState 无 Debug；同 ToolRegistry 先例）。
pub struct ApprovalRouter {
    state: Arc<AppState>,
}

impl std::fmt::Debug for ApprovalRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 不依赖 AppState: Debug，只打印内部不含引用的描述。
        f.debug_struct("ApprovalRouter").finish_non_exhaustive()
    }
}

impl ApprovalRouter {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl bm_ports::ToolApprovalPort for ApprovalRouter {
    async fn request_approval(
        &self,
        session_id: &str,
        tool_name: &str,
        call_id: &str,
        reason: Option<String>,
    ) -> Result<ApprovalVerdict, kernel_contracts::ToolError> {
        let approval_id = uuid::Uuid::new_v4().to_string();
        let rpc_id = uuid::Uuid::new_v4().to_string();
        // 审批帧带发起会话 id：前端豁免表按 (sessionId, toolName) 区分会话，
        // 设置页管理面按会话展示。空串 = 无会话上下文（端到端武器化留白）。
        let session_id = session_id.to_string();

        // 1. 登记进 PendingRegistry（approval/requested 帧数据源；mux 断连重放）。
        {
            let mut reg = self.state.pending.lock();
            reg.register_approval(
                rpc_id.clone(),
                session_id,
                approval_id.clone(),
                tool_name.to_string(),
                Some(call_id.to_string()),
                reason.clone(),
            );
        }

        // 2. 建 oneshot 等待通道。
        let (tx, rx) = tokio::sync::oneshot::channel::<ApprovalVerdict>();
        self.state
            .approval_waiters
            .lock()
            .unwrap()
            .insert(approval_id.clone(), tx);

        // 3. 广播 approval/requested 帧（前端弹窗触发）。approval_frame 已有
        //    完整 payload（sessionId/approvalId/toolName/callId/reason），
        //    复用其 rpc_id 与方法名。
        {
            let reg = self.state.pending.lock();
            let pending = reg.approvals.get(&rpc_id).cloned();
            if let Some(p) = pending {
                let frame = reg.approval_frame(&p);
                self.state
                    .broadcast_mux_frame(frame.rpc_id, frame.method, frame.payload);
            }
        }

        // 4. 等待裁定（超时 → 拒绝；通道被 drop/异常 → 拒绝）。
        let verdict: ApprovalVerdict = match tokio::time::timeout(APPROVAL_TIMEOUT, rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => ApprovalVerdict::Rejected,
            Err(_) => {
                // 超时：从审批表移除（不再可应答）。
                let _ = self.state.pending.lock().approvals.remove(&rpc_id);
                ApprovalVerdict::Rejected
            }
        };

        // 5. 无论何种裁定，清理等待表（防泄漏）。
        self.state
            .approval_waiters
            .lock()
            .unwrap()
            .remove(&approval_id);
        Ok(verdict)
    }
}

/// 由 respond 路由（allowed-once/rejected）解析审批：把裁定交还等待中的 loop。
/// 找不到等待者（超时已清理/重复应答）→ 忽略（无等待者即无唤醒需要）。
pub fn resolve_approval_waiter(
    state: &AppState,
    approval_id: &str,
    verdict: ApprovalVerdict,
) {
    if let Some(tx) = state.approval_waiters.lock().unwrap().remove(approval_id) {
        let _ = tx.send(verdict);
    }
}