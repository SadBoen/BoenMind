//! 目标路由器（web-server 实现 [`bm_ports::GoalPort`]）。
//!
//! 把现有 goal RPC 状态机（rpc_m3.rs 的 GoalRecord map）暴露成工具消费面：
//! `goal.get/create/update` 经此实现语义——CAS revision 守卫、phase 转换、
//! roundsStarted 自增、projection 广播。激活（activation）= 进程级活观测：
//! 有目标即 active（本进程无独立 disarm 面，goal-round-driver 的抑制
//! 由 phase 判定覆盖）。

use std::sync::Arc;

use async_trait::async_trait;
use bm_ports::{GoalAction, GoalPort, GoalView};
use serde_json::json;

use crate::api::{AppState, GoalRecord};

/// 创建目标缺省 max_goal_rounds（web-server 侧镜像；plugin-goal 有同值常量
/// DEFAULT_MAX_GOAL_ROUNDS——L0 不依赖插件，双处同值 8）。
const DEFAULT_GOAL_ROUNDS: u64 = 8;

/// 目标端口实现（经 bm-assembly `install_goal` 装配进 plugin-goal 全局源）。
pub struct GoalRouter {
    state: Arc<AppState>,
}

impl std::fmt::Debug for GoalRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoalRouter").finish_non_exhaustive()
    }
}

impl GoalRouter {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// 解析目标会话：显式 sid 非空 → 该会话；空 → 当前活跃会话
    /// （running 或非 blank 的第一个）。找不到 → None。
    fn resolve_session(&self, session_id: &str) -> Option<String> {
        if !session_id.is_empty() {
            let exists = self.state.sessions.lock().unwrap().contains_key(session_id);
            return if exists { Some(session_id.to_string()) } else { None };
        }
        let sessions = self.state.sessions.lock().unwrap();
        sessions
            .iter()
            .find(|(_, h)| h.running || !h.blank)
            .map(|(id, _)| id.clone())
    }

    fn to_view(goal: &GoalRecord) -> GoalView {
        GoalView {
            id: goal.id.clone(),
            revision: goal.revision,
            objective: goal.objective.clone(),
            phase: goal.phase.clone(),
            rounds_started: goal.rounds_started,
            max_goal_rounds: goal.max_goal_rounds,
            blocked_reason: None, // GoalRecord 无 blockedReason 槽位（M3 简化）；blocked 由 phase 承载
            activation: true,
        }
    }
}

#[async_trait]
impl GoalPort for GoalRouter {
    async fn goal_get(&self, session_id: &str) -> Result<Option<GoalView>, kernel_contracts::ToolError> {
        let Some(sid) = self.resolve_session(session_id) else {
            return Ok(None);
        };
        let goals = self.state.goals.lock().unwrap();
        Ok(goals.get(&sid).map(Self::to_view))
    }

    async fn goal_create(
        &self,
        session_id: &str,
        objective: &str,
        max_goal_rounds: Option<u64>,
    ) -> Result<GoalView, kernel_contracts::ToolError> {
        let Some(sid) = self.resolve_session(session_id) else {
            return Err(kernel_contracts::ToolError::new(
                "tool error: target session not found",
            ));
        };
        let now = chrono::Utc::now().timestamp_millis();
        let id = uuid::Uuid::new_v4().to_string();
        let goal = GoalRecord {
            id: id.clone(),
            revision: 1,
            objective: objective.to_string(),
            phase: "active".to_string(),
            max_goal_rounds: max_goal_rounds.unwrap_or(DEFAULT_GOAL_ROUNDS),
            rounds_started: 0,
            created_at: now,
            updated_at: now,
        };
        let view = {
            let mut goals = self.state.goals.lock().unwrap();
            // 至多一个当前目标：替换旧目标（completed 可被新目标替换；active 直接覆盖 = 新目标优先）。
            // 投影在锁内从刚插入的 goal 算：曾二次取锁 get().unwrap()，并发
            // clear 可在窗口内 remove → unwrap panic。
            let view = Self::to_view(&goal);
            goals.insert(sid.clone(), goal);
            view
        };
        self.state.write_projection(&sid, "goal", view_projection(&view));
        Ok(view)
    }

    async fn goal_update(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
        action: GoalAction,
        objective: Option<&str>,
        max_goal_rounds: Option<u64>,
        blocked_reason: Option<&str>,
    ) -> Result<GoalView, kernel_contracts::ToolError> {
        let Some(sid) = self.resolve_session(session_id) else {
            return Err(kernel_contracts::ToolError::new(
                "tool error: target session not found",
            ));
        };
        let err = |m: &str| kernel_contracts::ToolError::new(m.to_string());
        let mut goals = self.state.goals.lock().unwrap();
        let Some(goal) = goals.get_mut(&sid) else {
            return Err(err("goal-not-found"));
        };
        if goal.id != goal_id || goal.revision != revision {
            return Err(err("goal-conflict (stale ref; re-read with get_goal)"));
        }
        // phase 转换（按 action）。
        match action {
            GoalAction::Edit => {
                if let Some(o) = objective {
                    if o.trim().is_empty() {
                        return Err(err("objective must be at least 1 character"));
                    }
                    goal.objective = o.trim().to_string();
                }
                if let Some(n) = max_goal_rounds {
                    if n == 0 {
                        return Err(err("maxGoalRounds must be a positive integer"));
                    }
                    goal.max_goal_rounds = n;
                }
            }
            GoalAction::Pause => goal.phase = "paused".to_string(),
            GoalAction::Resume => {
                if goal.phase == "active" {
                    // redundant resume：容忍（幂等），不报错。
                } else if goal.rounds_started >= goal.max_goal_rounds {
                    return Err(err("goal cap exhausted (rounds >= maxGoalRounds)"));
                }
                goal.phase = "active".to_string();
            }
            GoalAction::Complete => goal.phase = "complete".to_string(),
            GoalAction::Blocked => {
                // blockedReason 无独立槽位（M3 简化）：phase=blocked 即信号；原因
                // 由模型在 blocked_reason 参数传递并跳过（不污染 objective）。
                goal.phase = "blocked".to_string();
                let _ = blocked_reason;
            }
        }
        goal.revision += 1;
        goal.updated_at = chrono::Utc::now().timestamp_millis();
        let view = Self::to_view(goal);
        drop(goals);
        self.state.write_projection(&sid, "goal", view_projection(&view));
        Ok(view)
    }
}

/// GoalView → projection wire 形状（对齐 GoalRecord::projection + blockedReason）。
fn view_projection(v: &GoalView) -> serde_json::Value {
    json!({
        "goal": {
            "id": v.id,
            "revision": v.revision,
            "objective": v.objective,
            "phase": v.phase,
            "maxGoalRounds": v.max_goal_rounds,
        },
        "roundsStarted": v.rounds_started,
        "createdAt": null,
        "updatedAt": null,
    })
}

/// goal-round-driver 用：把 session 的当前 goal 投影广播（roundsStarted 推进
/// 后前端可见）。在 driver 里没有 GoalView（只自增 round），从 state 现取。
pub fn broadcast_goal_projection_for_driver(state: &Arc<AppState>, session_id: &str) {
    if let Some(records) = state.goals.lock().unwrap().get(session_id) {
        let view = GoalView {
            id: records.id.clone(),
            revision: records.revision,
            objective: records.objective.clone(),
            phase: records.phase.clone(),
            rounds_started: records.rounds_started,
            max_goal_rounds: records.max_goal_rounds,
            blocked_reason: None,
            activation: true,
        };
        state.write_projection(session_id, "goal", view_projection(&view));
    }
}