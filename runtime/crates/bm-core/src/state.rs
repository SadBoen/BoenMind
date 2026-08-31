//! 运行时内部状态记录(Session / Agent / Operation)。
//! 状态迁移一律经由 bm_contract::states 的迁移表,表外迁移是 bug。

use bm_contract::BmTimestamp;
use bm_contract::budget::Budget;
use bm_contract::ids::BmId;
use bm_contract::states::{AgentState, OperationState, SessionState};
use bm_contract::wire::{ResultReference, WireError};

#[derive(Debug, Clone)]
pub struct Session {
    pub id: BmId,
    pub agent_id: BmId,
    pub state: SessionState,
    pub created_at: BmTimestamp,
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
    pub fn settle(
        &mut self,
        to: OperationState,
        error: Option<WireError>,
        now: BmTimestamp,
    ) -> (OperationState, OperationState, &'static str) {
        let from = self.state;
        let guard = OperationState::transitions()
            .iter()
            .find(|t| t.from == from && t.to == to)
            .map(|t| t.guard)
            .unwrap_or_else(|| panic!("表外迁移: operation {from:?} -> {to:?}"));
        self.state = to;
        if to.is_terminal() {
            self.completed_at = Some(now);
        }
        self.error = error;
        (from, to, guard)
    }

    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }
}

/// 由 AgentSpec 构造预算账本。
pub fn budget_from_spec(budget: Option<&Budget>) -> crate::budget::BudgetState {
    match budget {
        Some(b) => crate::budget::BudgetState::new(b.max_tokens, b.max_turns),
        None => crate::budget::BudgetState::new(u64::MAX, u32::MAX),
    }
}
