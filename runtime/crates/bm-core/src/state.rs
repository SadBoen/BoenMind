//! 运行时内部状态记录(Session / Agent / Operation)。
//! 状态迁移一律经由 bm_contract::states 的迁移表,表外迁移是 bug。

use bm_contract::BmTimestamp;
use bm_contract::budget::Budget;
use bm_contract::ids::BmId;
use bm_contract::states::{AgentState, OperationState, SessionState};
use bm_contract::wire::{PermissionMode, ResultReference, WireError};

#[derive(Debug, Clone)]
pub struct Session {
    pub id: BmId,
    pub agent_id: BmId,
    pub state: SessionState,
    pub created_at: BmTimestamp,
    /// W8(ADR-0018):会话绑定的工作区注册表 id(None = 不绑定)。
    /// 进程内作用域:Web 会话指针随重启失效,持久化无用户可见收益。
    pub workspace_id: Option<String>,
    /// 会话目录(2026-09-08 三端一致批):标题 = 首条用户消息首行截断
    /// (None = 未命名/存量待回填)。内容不在事件面(A4),走 core 直写保护。
    pub title: Option<String>,
    /// 会话目录:最近回合边界时间(与 sessions.updated_at 列同源:事件
    /// 物化落库,settle_operation 同步内存投影;None = 旧数据待启动回填)。
    pub updated_at: Option<String>,
    /// 权限模式(ADR-0030):服务端会话状态,权威在此;ask=审批(默认),
    /// yolo=审批类调用服务端自动放行。变更经 SessionSetMode 命令并落
    /// session.mode.changed 事件(物化投影持久)。
    pub permission_mode: PermissionMode,
}

/// 会话目录条目(GET /admin/sessions 读模型;管理面不入合同)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub state: String,
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
    /// ADR-0030:会话权限模式(ask/plan/yolo),服务端权威。
    pub permission_mode: String,
}

impl Session {
    /// 迁移 + 表外断言。
    pub fn transition(&mut self, to: SessionState) {
        assert!(
            SessionState::can_transition(self.state, to),
            "表外迁移: session {:?} -> {:?}",
            self.state,
            to
        );
        self.state = to;
    }
}

#[derive(Debug, Clone)]
pub struct Agent {
    pub id: BmId,
    pub session_id: BmId,
    pub name: String,
    pub model_chain: Vec<String>,
    pub state: AgentState,
    pub budget: crate::budget::BudgetState,
    /// W4b: 会话级指定或创建时继承的角色 system prompt
    pub system_prompt: Option<String>,
    /// ADR-0022 后续批:对话工具白名单(None=全量挂载;Some=仅挂清单内)。
    pub allowed_tools: Option<Vec<String>>,
}

impl Agent {
    pub fn transition(&mut self, to: AgentState) {
        assert!(
            AgentState::can_transition(self.state, to),
            "表外迁移: agent {:?} -> {:?}",
            self.state,
            to
        );
        self.state = to;
    }
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub id: BmId,
    pub request_id: BmId,
    pub session_id: BmId,
    pub agent_id: BmId,
    pub state: OperationState,
    pub turn_index: u32,
    pub created_at: BmTimestamp,
    pub completed_at: Option<BmTimestamp>,
    pub action_summary: String,
    pub result_reference: Option<ResultReference>,
    pub error: Option<WireError>,
}

impl Operation {
    /// dispatch_accepted:not_started→running。按规格 §8.1,此迁移由收据
    /// 本身承载,不发射 operation.state.changed 事件。
    pub fn dispatch(mut self) -> Self {
        assert!(
            OperationState::can_transition(self.state, OperationState::Running),
            "表外迁移: operation {:?} -> running",
            self.state
        );
        self.state = OperationState::Running;
        self
    }

    /// 终态落定:校验边合法性,发 operation.state.changed 事件的调用方
    /// 以返回的 (from, to, reason_code) 为准。
    /// P0(2026-09-07 架构评审):表外迁移不再 panic——返回 Err 交调用方
    /// 收敛为可观测错误(来源可能是恢复/裁决等边界路径,打崩进程不成比例)。
    pub fn settle(
        &mut self,
        to: OperationState,
        error: Option<WireError>,
        now: BmTimestamp,
    ) -> Result<(OperationState, OperationState, &'static str), IllegalTransition> {
        let from = self.state;
        let Some(guard) = OperationState::transitions()
            .iter()
            .find(|t| t.from == from && t.to == to)
            .map(|t| t.guard)
        else {
            return Err(IllegalTransition { from, to });
        };
        self.state = to;
        if to.is_terminal() {
            self.completed_at = Some(now);
        }
        self.error = error;
        Ok((from, to, guard))
    }

    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }
}

/// 表外迁移(状态机不存在的边)。携带迁移两端供调用方记日志。
#[derive(Debug, Clone, Copy)]
pub struct IllegalTransition {
    pub from: OperationState,
    pub to: OperationState,
}

/// 由 AgentSpec 构造预算账本。
pub fn budget_from_spec(budget: Option<&Budget>) -> crate::budget::BudgetState {
    match budget {
        Some(b) => crate::budget::BudgetState::new(b.max_tokens, b.max_turns),
        None => crate::budget::BudgetState::new(u64::MAX, u32::MAX),
    }
}
