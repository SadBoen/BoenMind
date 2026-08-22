//! 目标引擎（万物皆插件②，2026-08-22 从 web-server goal.rs/goal_driver.rs 下沉）。
//!
//! 状态自带（goals map + 续跑排他门），宿主能力经端口消费：
//! - [`SessionDrivePort`]：会话目录解析 + 原子占用 + 回合 spawn（续跑回合）。
//! - [`BroadcastPort`]：goal 投影广播（write_projection，key 恒 "goal"）。
//!
//! 一个引擎同时承载工具面（get/create/update）与 wire 面（CAS edit/相位直置/
//! clear）——语义差异（工具 resume 有额度检查、wire 直置无；缺省轮数工具 8 /
//! wire 1 由调用方显式传参）在端口文档里显式化。续跑驱动（goal-round-driver）
//! 也在引擎内：回合完成点 `maybe_continue`（--goal 装配方才 enable）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bm_ports::{
    BroadcastPort, GoalAction, GoalEnginePort, GoalError, GoalView, SessionDrivePort,
};
use serde_json::json;

/// 创建目标的缺省 max_goal_rounds（工具面缺省；wire 面由宿主显式传 1）。
pub const DEFAULT_MAX_GOAL_ROUNDS: u64 = 8;

/// 内部目标记录（对齐 DSH GoalSnapshot 的 wire 形状；带创建/更新时间戳）。
#[derive(Debug, Clone)]
struct Goal {
    id: String,
    revision: u64,
    objective: String,
    phase: String, // 'active' | 'paused' | 'blocked' | 'complete'
    max_goal_rounds: u64,
    rounds_started: u64,
    created_at: i64,
    updated_at: i64,
}

impl Goal {
    fn to_view(&self) -> GoalView {
        GoalView {
            id: self.id.clone(),
            revision: self.revision,
            objective: self.objective.clone(),
            phase: self.phase.clone(),
            rounds_started: self.rounds_started,
            max_goal_rounds: self.max_goal_rounds,
            blocked_reason: None, // 无 blockedReason 槽位（M3 简化）；blocked 由 phase 承载
            activation: true,
        }
    }

    /// wire 投影值（`GoalProjection`：goal snapshot + roundsStarted + 时间戳）。
    /// 万物皆插件②合并双份投影实现（原 wire 面有值/工具面恒 null 的不一致消除）。
    fn projection(&self) -> serde_json::Value {
        json!({
            "goal": {
                "id": self.id,
                "revision": self.revision,
                "objective": self.objective,
                "phase": self.phase,
                "maxGoalRounds": self.max_goal_rounds,
            },
            "roundsStarted": self.rounds_started,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

/// `<goal_round>` 提示模板（对齐官方：指名目标 + 轮数；要求证据后完成；
/// 若仍有工作保持 active）。
fn round_prompt(objective: &str, round: u64, max: u64) -> String {
    format!(
        "[goal round {round}/{max}] 这是自动化目标回合的续跑。当前目标：\"{objective}\"。\
         请基于当前会话的工作区、工具结果与对话历史继续推进目标。\
         只有在目标确实完成时才调用 goal.update 的 complete；若仍有工作则继续推进（保持目标 active）。"
    )
}

/// 目标引擎（宿主能力经端口；状态自带——每实例独立）。
pub struct GoalEngine {
    host: Arc<dyn SessionDrivePort>,
    broadcast: Arc<dyn BroadcastPort>,
    /// session_id → 当前目标（至多一个）。
    goals: Mutex<HashMap<String, Goal>>,
    /// per-session 续跑中标志（防嵌套；spawn 的续跑任务经完成钩子清除）。
    continuing: Arc<Mutex<HashMap<String, bool>>>,
    /// 续跑驱动开关（--goal 装配方 enable；引擎本体随宿主常驻保 wire 面）。
    driver_enabled: AtomicBool,
}

impl std::fmt::Debug for GoalEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoalEngine").finish_non_exhaustive()
    }
}

impl GoalEngine {
    pub fn new(host: Arc<dyn SessionDrivePort>, broadcast: Arc<dyn BroadcastPort>) -> Self {
        Self {
            host,
            broadcast,
            goals: Mutex::new(HashMap::new()),
            continuing: Arc::new(Mutex::new(HashMap::new())),
            driver_enabled: AtomicBool::new(false),
        }
    }

    /// 目标不存在 / ref 不匹配的统一 CAS 前置检查（返回目标可变引用）。
    fn cas_entry<'a>(
        goals: &'a mut HashMap<String, Goal>,
        session_id: &str,
        goal_id: &str,
        revision: u64,
    ) -> Result<&'a mut Goal, GoalError> {
        let Some(goal) = goals.get_mut(session_id) else {
            return Err(GoalError::NotFound);
        };
        if goal.id != goal_id || goal.revision != revision {
            return Err(GoalError::Conflict);
        }
        Ok(goal)
    }

    /// 推进 revision + 写投影（锁外广播）。
    fn commit(&self, session_id: &str, goal: &Goal) {
        self.broadcast
            .write_projection(session_id, "goal", goal.projection());
    }
}

impl GoalEnginePort for GoalEngine {
    fn resolve_session(&self, session_id: &str) -> Option<String> {
        if !session_id.is_empty() {
            return if self.host.session_exists(session_id) {
                Some(session_id.to_string())
            } else {
                None
            };
        }
        self.host.active_session()
    }

    fn goal_get(&self, session_id: &str) -> Result<Option<GoalView>, GoalError> {
        let goals = self.goals.lock().unwrap();
        Ok(goals.get(session_id).map(Goal::to_view))
    }

    fn goal_create(
        &self,
        session_id: &str,
        objective: &str,
        max_goal_rounds: Option<u64>,
    ) -> Result<GoalView, GoalError> {
        if objective.trim().is_empty() {
            return Err(GoalError::EmptyObjective);
        }
        let now = chrono::Utc::now().timestamp_millis();
        let goal = Goal {
            id: uuid::Uuid::new_v4().to_string(),
            revision: 1,
            objective: objective.to_string(),
            phase: "active".to_string(),
            max_goal_rounds: max_goal_rounds.unwrap_or(DEFAULT_MAX_GOAL_ROUNDS),
            rounds_started: 0,
            created_at: now,
            updated_at: now,
        };
        let view = goal.to_view();
        // 至多一个当前目标：直接覆盖（completed 可被新目标替换；active 覆盖 = 新目标优先）。
        self.goals.lock().unwrap().insert(session_id.to_string(), goal.clone());
        self.commit(session_id, &goal);
        Ok(view)
    }

    fn goal_update(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
        action: GoalAction,
        objective: Option<&str>,
        max_goal_rounds: Option<u64>,
        blocked_reason: Option<&str>,
    ) -> Result<GoalView, GoalError> {
        let mut goals = self.goals.lock().unwrap();
        let goal = Self::cas_entry(&mut goals, session_id, goal_id, revision)?;
        // phase 转换（按 action）。
        match action {
            GoalAction::Edit => {
                if let Some(o) = objective {
                    if o.trim().is_empty() {
                        return Err(GoalError::EmptyObjective);
                    }
                    goal.objective = o.trim().to_string();
                }
                if let Some(n) = max_goal_rounds {
                    if n == 0 {
                        return Err(GoalError::InvalidMaxRounds);
                    }
                    goal.max_goal_rounds = n;
                }
            }
            GoalAction::Pause => goal.phase = "paused".to_string(),
            GoalAction::Resume => {
                if goal.phase == "active" {
                    // redundant resume：容忍（幂等），不报错。
                } else if goal.rounds_started >= goal.max_goal_rounds {
                    return Err(GoalError::ResumeCapExhausted);
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
        let view = goal.to_view();
        let snapshot = goal.clone();
        drop(goals);
        self.commit(session_id, &snapshot);
        Ok(view)
    }

    fn goal_cas_edit(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
        objective: Option<&str>,
        max_goal_rounds: Option<u64>,
    ) -> Result<u64, GoalError> {
        let mut goals = self.goals.lock().unwrap();
        let goal = Self::cas_entry(&mut goals, session_id, goal_id, revision)?;
        if let Some(obj) = objective {
            if obj.trim().is_empty() {
                return Err(GoalError::EmptyObjective);
            }
            goal.objective = obj.to_string();
        }
        if let Some(n) = max_goal_rounds {
            if n == 0 {
                return Err(GoalError::InvalidMaxRounds);
            }
            goal.max_goal_rounds = n;
        }
        goal.revision += 1;
        goal.updated_at = chrono::Utc::now().timestamp_millis();
        let new_rev = goal.revision;
        let snapshot = goal.clone();
        drop(goals);
        self.commit(session_id, &snapshot);
        Ok(new_rev)
    }

    fn goal_cas_phase(
        &self,
        session_id: &str,
        goal_id: &str,
        revision: u64,
        to_phase: &str,
    ) -> Result<u64, GoalError> {
        let mut goals = self.goals.lock().unwrap();
        let goal = Self::cas_entry(&mut goals, session_id, goal_id, revision)?;
        goal.phase = to_phase.to_string();
        goal.revision += 1;
        goal.updated_at = chrono::Utc::now().timestamp_millis();
        let new_rev = goal.revision;
        let snapshot = goal.clone();
        drop(goals);
        self.commit(session_id, &snapshot);
        Ok(new_rev)
    }

    fn goal_clear(&self, session_id: &str, goal_id: &str, revision: u64) -> Result<(), GoalError> {
        let mut goals = self.goals.lock().unwrap();
        Self::cas_entry(&mut goals, session_id, goal_id, revision)?;
        goals.remove(session_id);
        drop(goals);
        // 墓碑：投影置 null（客户端 higher-seq-wins 覆盖到空态）。
        self.broadcast
            .write_projection(session_id, "goal", serde_json::Value::Null);
        Ok(())
    }

    fn maybe_continue(&self, session_id: &str) -> bool {
        if !self.driver_enabled.load(Ordering::Acquire) {
            return false;
        }
        // 单原子排他门：置位（防并发双续跑）。已在续跑中 → 直接跳过。
        {
            let mut cont = self.continuing.lock().unwrap();
            if cont.get(session_id).copied().unwrap_or(false) {
                return false;
            }
            cont.insert(session_id.to_string(), true);
        }
        // 判定 + 推进（CAS）：一次 goals 锁内完成「查询 active + 额度判定 + 自增 +
        // 写回」。无目标 / 非 active / 额度耗尽 → None，guard drop 后走下方统一
        // 释放排他门（回归：早退路径曾直接 return 不释放——标志残留会永久抑制续跑）。
        let admitted: Option<(String, u64, u64, serde_json::Value)> = {
            let mut goals = self.goals.lock().unwrap();
            match goals.get_mut(session_id) {
                None => None,
                Some(g) if g.phase != "active" || g.rounds_started >= g.max_goal_rounds => None,
                Some(g) => {
                    g.rounds_started += 1;
                    g.updated_at = chrono::Utc::now().timestamp_millis();
                    let objective = g.objective.clone();
                    let round = g.rounds_started;
                    let max = g.max_goal_rounds;
                    let projection = g.projection();
                    Some((objective, round, max, projection))
                }
            }
        };
        let Some((objective, round, max, projection)) = admitted else {
            // 无目标 / 非 active / 额度耗尽 / 目标消失：统一在此释放排他门。
            self.continuing.lock().unwrap().remove(session_id);
            return false;
        };
        // 投影广播（roundsStarted 推进可见）。
        self.broadcast.write_projection(session_id, "goal", projection);

        let prompt = round_prompt(&objective, round, max);
        // 会话占用 + 续跑回合（on_finish 释放排他门；忙/消失 → false）。
        let cont_flag = Arc::clone(&self.continuing);
        let sid = session_id.to_string();
        let started = self.host.spawn_turn(
            session_id,
            &prompt,
            Some(Box::new(move || {
                cont_flag.lock().unwrap().insert(sid, false);
            })),
        );
        if !started {
            // 会话消失 / 人类 prompt 已接管：释放排他门，不叠回合。
            self.continuing.lock().unwrap().remove(session_id);
            return false;
        }
        true
    }

    fn set_driver_enabled(&self, enabled: bool) {
        self.driver_enabled.store(enabled, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_ports::TurnFinishHook;
    use std::sync::Mutex as StdMutex;

    /// 桩宿主：记录 spawn 请求；可编程「完成即回调钩子」模拟回合结束。
    #[derive(Debug, Default)]
    struct StubHost {
        spawned: StdMutex<Vec<(String, String)>>,
        exists: StdMutex<Vec<String>>,
    }
    impl SessionDrivePort for StubHost {
        fn session_exists(&self, session_id: &str) -> bool {
            let mut e = self.exists.lock().unwrap();
            let known = !e.is_empty();
            e.push(session_id.to_string());
            known
        }
        fn active_session(&self) -> Option<String> {
            None
        }
        fn spawn_turn(
            &self,
            session_id: &str,
            prompt: &str,
            on_finish: Option<TurnFinishHook>,
        ) -> bool {
            self.spawned
                .lock()
                .unwrap()
                .push((session_id.to_string(), prompt.to_string()));
            if let Some(hook) = on_finish {
                hook(); // 同步完成（模拟回合立即结束 → 释放续跑门）
            }
            true
        }
    }

    /// 桩广播：记录投影写入。
    #[derive(Debug, Default)]
    struct StubBroadcast {
        projections: StdMutex<Vec<(String, String, serde_json::Value)>>,
    }
    impl BroadcastPort for StubBroadcast {
        fn broadcast_host(&self, _method: &str, _payload: serde_json::Value) {}
        fn broadcast_mux(&self, _rpc_id: String, _method: &str, _payload: serde_json::Value) {}
        fn write_projection(&self, session_id: &str, key: &str, value: serde_json::Value) {
            self.projections
                .lock()
                .unwrap()
                .push((session_id.to_string(), key.to_string(), value));
        }
    }

    fn engine() -> (Arc<GoalEngine>, Arc<StubHost>, Arc<StubBroadcast>) {
        let host = Arc::new(StubHost::default());
        let bcast = Arc::new(StubBroadcast::default());
        let engine = Arc::new(GoalEngine::new(
            Arc::clone(&host) as Arc<dyn SessionDrivePort>,
            Arc::clone(&bcast) as Arc<dyn BroadcastPort>,
        ));
        (engine, host, bcast)
    }

    fn seed_active(engine: &GoalEngine, sid: &str, max: u64) -> (String, u64) {
        let v = engine.goal_create(sid, "做一个 fetching 脚本", Some(max)).unwrap();
        (v.id.clone(), v.revision)
    }

    #[test]
    fn create_get_roundtrip_and_projection() {
        let (engine, _h, b) = engine();
        let v = engine.goal_create("s1", "写文档", None).unwrap();
        assert_eq!(v.max_goal_rounds, DEFAULT_MAX_GOAL_ROUNDS, "工具面缺省 8");
        assert_eq!(v.revision, 1);
        let got = engine.goal_get("s1").unwrap().unwrap();
        assert_eq!(got.objective, "写文档");
        // 投影写入（key=goal，createdAt 为真实时间戳——双投影合并后不再恒 null）。
        let projs = b.projections.lock().unwrap();
        assert_eq!(projs.len(), 1);
        assert_eq!(projs[0].1, "goal");
        assert!(projs[0].2["createdAt"].is_i64());
    }

    #[test]
    fn empty_objective_rejected() {
        let (engine, _h, _b) = engine();
        assert_eq!(engine.goal_create("s1", "  ", None), Err(GoalError::EmptyObjective));
    }

    #[test]
    fn cas_mismatch_and_not_found() {
        let (engine, _h, _b) = engine();
        let (id, rev) = seed_active(&engine, "s1", 8);
        assert_eq!(
            engine.goal_cas_phase("s1", &id, rev + 1, "paused"),
            Err(GoalError::Conflict)
        );
        assert_eq!(
            engine.goal_cas_phase("s-none", &id, rev, "paused"),
            Err(GoalError::NotFound)
        );
        let new_rev = engine.goal_cas_phase("s1", &id, rev, "paused").unwrap();
        assert_eq!(new_rev, rev + 1);
        assert_eq!(engine.goal_get("s1").unwrap().unwrap().phase, "paused");
    }

    #[test]
    fn tool_resume_cap_exhausted_but_wire_phase_bypasses() {
        let (engine, _h, _b) = engine();
        let (id, rev) = seed_active(&engine, "s1", 1);
        // 消耗额度：driver 续跑一次。
        engine.set_driver_enabled(true);
        assert!(engine.maybe_continue("s1"));
        engine.set_driver_enabled(false);
        engine.goal_cas_phase("s1", &id, rev, "paused").unwrap();
        // 工具面 resume：额度耗尽 → 错误。
        let v = engine.goal_get("s1").unwrap().unwrap();
        assert_eq!(
            engine.goal_update("s1", &v.id, v.revision, GoalAction::Resume, None, None, None),
            Err(GoalError::ResumeCapExhausted)
        );
        // wire 面相位直置：无额度检查（既有 RPC 语义）。
        let v = engine.goal_get("s1").unwrap().unwrap();
        engine.goal_cas_phase("s1", &v.id, v.revision, "active").unwrap();
        assert_eq!(engine.goal_get("s1").unwrap().unwrap().phase, "active");
    }

    #[test]
    fn clear_writes_tombstone() {
        let (engine, _h, b) = engine();
        let (id, rev) = seed_active(&engine, "s1", 8);
        engine.goal_clear("s1", &id, rev).unwrap();
        assert!(engine.goal_get("s1").unwrap().is_none());
        let projs = b.projections.lock().unwrap();
        assert!(projs.last().unwrap().2.is_null(), "墓碑投影应为 null");
    }

    /// 回归（B-BUG-002）：无目标路径不得泄漏排他门；门释放后 re-arm 可再续跑。
    #[test]
    fn driver_gate_released_and_rearmable() {
        let (engine, host, _b) = engine();
        engine.set_driver_enabled(true);
        // 未启用 driver → 恒 false。
        engine.set_driver_enabled(false);
        assert!(!engine.maybe_continue("s1"));
        engine.set_driver_enabled(true);

        // 无目标经过完成点：不续跑，且门必须释放（标志残留会永久抑制）。
        assert!(!engine.maybe_continue("s1"));
        assert!(
            !engine.continuing.lock().unwrap().get("s1").copied().unwrap_or(false),
            "no-goal early return must release the continuation gate"
        );

        // re-arm：种目标 → 续跑可再次发起（rounds_started 推进 + spawn 记录）。
        seed_active(&engine, "s1", 8);
        assert!(engine.maybe_continue("s1"));
        let g = engine.goal_get("s1").unwrap().unwrap();
        assert_eq!(g.rounds_started, 1);
        let spawned = host.spawned.lock().unwrap();
        assert_eq!(spawned.len(), 1);
        assert!(spawned[0].1.contains("[goal round 1/8]"), "prompt: {}", spawned[0].1);
        // 桩宿主同步回调 on_finish → 门已释放。
        assert!(!engine.continuing.lock().unwrap().get("s1").copied().unwrap_or(false));
    }

    /// 额度耗尽 / 非 active：不续跑、门释放。
    #[test]
    fn driver_suppressed_when_capped_or_not_active() {
        let (engine, host, _b) = engine();
        engine.set_driver_enabled(true);
        let (id, rev) = seed_active(&engine, "s1", 1);
        assert!(engine.maybe_continue("s1")); // 消耗唯一额度
        assert!(!engine.maybe_continue("s1"), "额度耗尽不续跑");
        engine.goal_cas_phase("s1", &id, rev, "paused").unwrap();
        assert!(!engine.maybe_continue("s1"), "paused 不续跑");
        assert_eq!(host.spawned.lock().unwrap().len(), 1);
        assert!(
            !engine.continuing.lock().unwrap().get("s1").copied().unwrap_or(false),
            "gate must be released after suppression paths"
        );
    }
}
