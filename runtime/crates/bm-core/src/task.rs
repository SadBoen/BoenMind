//! Task 规范对象与状态机(M5.2,基线 §2.2/§10.3;ADR-0004 三层归属)。
//!
//! Task 的规范状态、生命周期、成员关系、预算与截止时间唯一由 L2 持有;
//! 任何 Task Board 与 Surface 视图仅为投影。迁移一律经由 bm_contract::states
//! 的 task 迁移表,表外迁移是 bug。两道结构门禁在本模块落地:
//! - **完成判定门禁**:completed/failed 必须 verified(无 Observation 核验
//!   不得完成,基线 M5 通过条件第 4 条;M5 规格 §5.7)——表内 guard 是
//!   verified_completion/verified_failure,调用方须出示核验结论;
//! - **task_epoch 写入门禁**(ADR-0004 条件 3):取得接管权时递增,携带过期
//!   epoch 的编排命令一律 Stale 拒绝(M5-T1 在核心面执行;wire 面不暴露
//!   epoch 参数——与 input_trust 同款收权,M5 规格 §9)。

use bm_contract::BmTimestamp;
use bm_contract::budget::Budget;
use bm_contract::ids::IdGen;
use bm_contract::states::TaskState;
use bm_contract::timestamp::format_ts;
use bm_contract::wire::TaskAuthorizationEntry;
use chrono::DateTime;

/// 成员事实(task.member.added 事件承载;M5 = coordinator + 单 worker 闭环)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMember {
    pub agent_id: bm_contract::ids::BmId,
    pub role: MemberRole,
    pub grant_id: Option<String>,
    pub joined_seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberRole {
    Coordinator,
    Worker,
}

impl MemberRole {
    pub fn as_str(self) -> &'static str {
        match self {
            MemberRole::Coordinator => "coordinator",
            MemberRole::Worker => "worker",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskError {
    /// 表外迁移(迁移表中无边)。
    IllegalTransition { from: TaskState, to: TaskState },
    /// 完成判定门禁:出示的核验结论不支持目标态(基线 M5 通过条件第 4 条)。
    UnverifiedCompletion,
    /// 过期 epoch 的命令(ADR-0004 条件 3):Stale 拒绝。
    StaleEpoch { current: u64, presented: u64 },
}

/// Task 规范对象(L2 持有;内存视图 = World.tasks,持久 = tasks 表)。
#[derive(Debug, Clone)]
pub struct Task {
    pub id: bm_contract::ids::BmId,
    pub title: String,
    pub goal: String,
    pub state: TaskState,
    pub created_by: String,
    pub task_epoch: u64,
    /// 三方交集的 Task 分量(ADR-0002 §11.3):协调动词白名单 + 资源谓词。
    pub authorization: Vec<TaskAuthorizationEntry>,
    /// Task 预算包络(基线 §9.7;None = 运行时默认包络)。
    pub budget: Option<Budget>,
    pub deadline: Option<BmTimestamp>,
    pub members: Vec<TaskMember>,
    /// M5 恒 None(M6 子任务预留,合同 const null)。
    pub parent_task_id: Option<bm_contract::ids::BmId>,
    pub created_at: BmTimestamp,
    pub updated_at: BmTimestamp,
}

impl Task {
    /// 创建(created 态;启动迁移由调用方推进并各自落事件)。
    pub fn create(
        ids: &dyn IdGen,
        title: impl Into<String>,
        goal: impl Into<String>,
        authorization: Vec<TaskAuthorizationEntry>,
        budget: Option<Budget>,
        deadline: Option<BmTimestamp>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        let now_ts = format_ts(now);
        Self {
            id: ids.next_id("task"),
            title: title.into(),
            goal: goal.into(),
            state: TaskState::Created,
            created_by: "butler:system".into(),
            task_epoch: 1,
            authorization,
            budget,
            deadline,
            members: Vec::new(),
            parent_task_id: None,
            created_at: now_ts.clone(),
            updated_at: now_ts,
        }
    }

    /// task_epoch 写入门禁(ADR-0004 条件 3):命令须出示当前 epoch。
    pub fn require_epoch(&self, presented: u64) -> Result<(), TaskError> {
        if presented == self.task_epoch {
            Ok(())
        } else {
            Err(TaskError::StaleEpoch {
                current: self.task_epoch,
                presented,
            })
        }
    }

    /// 取得接管权:epoch 单调递增(跨 Surface 接管/编排重启语义;
    /// 持久化随调用方 save_task,重启不回退)。
    pub fn takeover(&mut self) -> u64 {
        self.task_epoch += 1;
        self.task_epoch
    }

    /// 状态迁移(表内边) + 完成判定门禁。返回 (from, to, guard) 供事件
    /// reason_code;失败不改状态。
    ///
    /// `verified`:目标为 completed/failed 时必须出示 Observation 核验结论
    /// (true = verified;false/None = unverified,一律拒绝)。guard 文本
    /// verified_completion/verified_failure 的机器检查点即此参数。
    pub fn transition(
        &mut self,
        to: TaskState,
        verified: Option<bool>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(TaskState, TaskState, &'static str), TaskError> {
        let from = self.state;
        let guard = TaskState::transitions()
            .iter()
            .find(|t| t.from == from && t.to == to)
            .map(|t| t.guard)
            .ok_or(TaskError::IllegalTransition { from, to })?;
        if matches!(to, TaskState::Completed | TaskState::Failed) && verified != Some(true) {
            return Err(TaskError::UnverifiedCompletion);
        }
        self.state = to;
        self.updated_at = format_ts(now);
        Ok((from, to, guard))
    }

    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    /// 成员加入(调用方发 task.member.added 事件;jointed_seq 由事件 seq 回填)。
    pub fn add_member(&mut self, member: TaskMember) {
        self.members.push(member);
        self.updated_at = self.updated_at.clone();
    }
}

/// 解析暂停/恢复原因码为状态迁移的语义类别(T1 仅供测试与 T7 复用)。
pub fn reason_is_pause(reason: &str) -> bool {
    reason == "task_paused"
}

/// 恢复装载:行 → Task(载荷合同 JSON 优先,行级键列为兜底)。
pub fn task_from_row(row: &bm_persist::recovery::TaskStateRow) -> Result<Task, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "snake_case")]
    struct Payload {
        task_id: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        goal: Option<String>,
        #[serde(default)]
        authorization: Vec<TaskAuthorizationEntry>,
        #[serde(default)]
        budget: Option<Budget>,
        #[serde(default)]
        deadline: Option<BmTimestamp>,
        #[serde(default)]
        parent_task_id: Option<String>,
        #[serde(default)]
        created_at: Option<String>,
    }
    let p: Payload =
        serde_json::from_str(&row.payload).map_err(|e| format!("task payload 解析失败: {e}"))?;
    let id = bm_contract::ids::BmId::parse(p.task_id).map_err(|e| e.to_string())?;
    let state =
        TaskState::from_wire(&row.state).ok_or_else(|| format!("task 状态非法: {}", row.state))?;
    Ok(Task {
        id,
        title: p.title.unwrap_or_else(|| row.title.clone()),
        goal: p.goal.unwrap_or_default(),
        state,
        created_by: row.created_by.clone(),
        task_epoch: row.task_epoch.max(1) as u64,
        authorization: p.authorization,
        budget: p.budget,
        deadline: p.deadline,
        members: Vec::new(),
        parent_task_id: match p.parent_task_id {
            Some(s) => Some(bm_contract::ids::BmId::parse(s).map_err(|e| e.to_string())?),
            None => None,
        },
        created_at: p.created_at.unwrap_or_else(|| row.created_at.clone()),
        updated_at: row.updated_at.clone(),
    })
}

/// 时间换算辅助(与 approval.rs 同款语义;解析失败按安全侧处理)。
pub fn parse_or_epoch(ts: &BmTimestamp) -> Option<DateTime<chrono::Utc>> {
    bm_contract::timestamp::parse_ts(ts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{Clock, MockClock};
    use bm_contract::ids::SeqIdGen;

    const BASE_MS: u128 = 1_788_000_000_000; // 2026-08-29T10:40:00.000Z

    fn new_task() -> Task {
        let ids = SeqIdGen::new();
        let clock = MockClock::at_ms(BASE_MS);
        Task::create(
            &ids,
            "整理读书笔记",
            "把 inbox 归档",
            vec![],
            None,
            None,
            clock.now(),
        )
    }

    #[test]
    fn create_starts_in_created_state_with_epoch_1() {
        let t = new_task();
        assert_eq!(t.state, TaskState::Created);
        assert_eq!(t.task_epoch, 1);
        assert_eq!(t.created_by, "butler:system");
        assert!(t.parent_task_id.is_none(), "M5 恒 None(M6 预留)");
        assert_eq!(t.created_at.as_str(), "2026-08-29T10:40:00.000Z");
    }

    #[test]
    fn happy_path_lifecycle_follows_transition_table() {
        let mut t = new_task();
        let clock = MockClock::at_ms(BASE_MS);
        // created→running→paused→running→cancelled(GT-03 场景 B 主链)
        let (f, to, g) = t.transition(TaskState::Running, None, clock.now()).unwrap();
        assert_eq!(
            (f.as_str(), to.as_str(), g),
            ("created", "running", "task_started")
        );
        let (f, to, g) = t.transition(TaskState::Paused, None, clock.now()).unwrap();
        assert_eq!(
            (f.as_str(), to.as_str(), g),
            ("running", "paused", "task_paused")
        );
        let (f, to, g) = t.transition(TaskState::Running, None, clock.now()).unwrap();
        assert_eq!(
            (f.as_str(), to.as_str(), g),
            ("paused", "running", "task_resumed")
        );
        let (f, to, g) = t
            .transition(TaskState::Cancelled, None, clock.now())
            .unwrap();
        assert_eq!(
            (f.as_str(), to.as_str(), g),
            ("running", "cancelled", "task_cancelled")
        );
        assert!(t.is_terminal());
        // 终态不可迁出
        assert_eq!(
            t.transition(TaskState::Running, None, clock.now()),
            Err(TaskError::IllegalTransition {
                from: TaskState::Cancelled,
                to: TaskState::Running
            })
        );
    }

    #[test]
    fn completion_gate_requires_verified_verdict() {
        let mut t = new_task();
        let clock = MockClock::at_ms(BASE_MS);
        t.transition(TaskState::Running, None, clock.now()).unwrap();

        // 无核验结论(None)→ 拒绝,状态不变
        assert_eq!(
            t.transition(TaskState::Completed, None, clock.now()),
            Err(TaskError::UnverifiedCompletion)
        );
        assert_eq!(t.state, TaskState::Running, "拒绝后状态不变");
        // 核验结论 = unverified(false)→ 拒绝(声称完成不算,基线 §20)
        assert_eq!(
            t.transition(TaskState::Completed, Some(false), clock.now()),
            Err(TaskError::UnverifiedCompletion)
        );
        // 核验结论 = verified(true)→ 放行(guard = verified_completion)
        let (_, to, g) = t
            .transition(TaskState::Completed, Some(true), clock.now())
            .unwrap();
        assert_eq!((to.as_str(), g), ("completed", "verified_completion"));
    }

    #[test]
    fn blocked_has_no_direct_exit_to_completed() {
        let mut t = new_task();
        let clock = MockClock::at_ms(BASE_MS);
        t.transition(TaskState::Running, None, clock.now()).unwrap();
        t.transition(TaskState::Blocked, None, clock.now()).unwrap();
        // blocked 直达 completed 无边(硬顶后必须先 user_resolved)
        assert_eq!(
            t.transition(TaskState::Completed, Some(true), clock.now()),
            Err(TaskError::IllegalTransition {
                from: TaskState::Blocked,
                to: TaskState::Completed
            })
        );
        // user_resolved 回 running 后才可核验完成
        t.transition(TaskState::Running, None, clock.now()).unwrap();
        assert!(
            t.transition(TaskState::Completed, Some(true), clock.now())
                .is_ok()
        );
    }

    #[test]
    fn epoch_gate_rejects_stale_commands_and_takeover_is_monotonic() {
        let mut t = new_task();
        t.require_epoch(1).expect("当前 epoch 命令放行");
        // 接管:epoch 递增
        assert_eq!(t.takeover(), 2);
        assert_eq!(t.takeover(), 3, "接管权可连续取得,单调递增");
        // 过期 epoch 命令:Stale 拒绝(ADR-0004 条件 3)
        assert_eq!(
            t.require_epoch(1),
            Err(TaskError::StaleEpoch {
                current: 3,
                presented: 1
            })
        );
        assert_eq!(
            t.require_epoch(2),
            Err(TaskError::StaleEpoch {
                current: 3,
                presented: 2
            })
        );
        t.require_epoch(3).expect("最新 epoch 放行");
    }

    #[test]
    fn task_from_row_roundtrips_payload_and_state() {
        let t = new_task();
        let row = bm_persist::recovery::TaskStateRow {
            id: t.id.as_str().to_string(),
            title: t.title.clone(),
            state: "paused".into(),
            created_by: t.created_by.clone(),
            task_epoch: 4,
            payload: serde_json::json!({
                "task_id": t.id.as_str(),
                "title": t.title,
                "goal": t.goal,
                "budget": {"max_tokens": 100000, "max_turns": 1000}
            })
            .to_string(),
            created_at: t.created_at.as_str().to_string(),
            updated_at: t.updated_at.as_str().to_string(),
        };
        let back = task_from_row(&row).expect("行可解析");
        assert_eq!(back.id.as_str(), t.id.as_str());
        assert_eq!(back.state, TaskState::Paused);
        assert_eq!(back.task_epoch, 4, "epoch 自行级恢复(不回退)");
        assert_eq!(
            back.budget.as_ref().map(|b| b.max_tokens),
            Some(100000),
            "包络自载荷恢复"
        );
    }
}
