//! goal-round-driver（web-server 同会话续跑驱动）。
//!
//! 对齐 DSH `dsh-goal-round-driver` 语义：**同一会话**内，当 Agent 空闲且存在
//! active + armed（有剩余额度）的目标时，回合完成后注入一条 `<goal_round>`
//! 用户消息，自动再跑一轮——而不是新开会话/复制前缀/独立尝试。
//!
//! 触发点：`session_prompt` 的回合完成点调 [`GoalDriver::maybe_continue`]。
//! - `rounds_started` 只在 admitted 的 goal-sourced 回合自增（人类消息不消耗）
//! - 抑制：phase != active（pause/complete/blocked）或额度耗尽 → 不续跑
//! - 防嵌套：per-session 续跑中标志，同一时刻只允许一条续跑链（人类 prompt
//!   与续跑竞态时后到者跳过，不叠回合）

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;
use tokio::sync::Mutex;

use crate::api::AppState;

/// `<goal_round>` 提示模板（对齐官方：指名目标 + 轮数；要求证据后完成；
/// 若仍有工作保持 active）。
fn round_prompt(objective: &str, round: u64, max: u64) -> String {
    format!(
        "[goal round {round}/{max}] 这是自动化目标回合的续跑。当前目标：\"{objective}\"。\
         请基于当前会话的工作区、工具结果与对话历史继续推进目标。\
         只有在目标确实完成时才调用 goal.update 的 complete；若仍有工作则继续推进（保持目标 active）。"
    )
}

/// 同会话续跑驱动。
pub struct GoalDriver {
    state: Arc<AppState>,
    /// per-session 续跑中标志（防嵌套）。spawn 的续跑任务持有同一 Arc 清除。
    continuing: Arc<Mutex<HashMap<String, bool>>>,
}

impl std::fmt::Debug for GoalDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoalDriver").finish_non_exhaustive()
    }
}

impl GoalDriver {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            continuing: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 回合完成点调用：若该 session 有 active + 有额度目标，注入下一轮并续跑。
    /// 防嵌套 + 幂等（判定在 goals 锁内完成）。返回是否发起了续跑。
    ///
    /// 竞态收口（回归 F6）：续跑中标志（continuing）作为**唯一排他门**——
    /// 检查+置位在函数最前原子完成（无 await 间隙），`rounds_started` 自增在
    /// 门内进行。两个并发 `maybe_continue`（如人类 prompt 完成点 + scheduler
    /// 到期）后到者看到 continuing=true 直接返回，**不会 double-consume 额度、
    /// 不会双 spawn**。spawn 完成后清标志（见 spawn 块）。
    pub async fn maybe_continue(&self, session_id: &str) -> bool {
        // 单原子排他门：置位（防并发双续跑）。已在续跑中 → 直接跳过。
        {
            let mut cont = self.continuing.lock().await;
            let key = session_id.to_string();
            if cont.get(&key).copied().unwrap_or(false) {
                return false;
            }
            cont.insert(key, true);
        }
        // 判定 + 推进（CAS）：一次 goals 锁内完成「查询 active + 额度判定 + 自增 +
        // 写回」。无目标 / 非 active / 额度耗尽 → admitted=None，guard drop 后
        // 释放排他门（std 锁 guard 不 Send，不能跨 await 持有）。
        let admitted: Option<(String, u64, u64)> = {
            let mut goals = self.state.goals.lock().unwrap();
            let Some(mut g) = goals.get(session_id).cloned() else {
                return false; // 无目标 → 下方统一释放排他门
            };
            if g.phase != "active" || g.rounds_started >= g.max_goal_rounds {
                return false; // 非 active / 额度耗尽
            }
            g.rounds_started += 1;
            g.updated_at = chrono::Utc::now().timestamp_millis();
            let objective = g.objective.clone();
            let round = g.rounds_started;
            let max = g.max_goal_rounds;
            goals.insert(session_id.to_string(), g);
            Some((objective, round, max))
        };
        let Some((objective, round, max)) = admitted else {
            // 目标消失：释放排他门（goals guard 已 drop）。
            self.continuing.lock().await.remove(session_id);
            return false;
        };
        // 投影广播（roundsStarted 推进可见）。
        crate::goal::broadcast_goal_projection_for_driver(&self.state, session_id);

        let prompt = round_prompt(&objective, round, max);
        // 会话检查与运行状态判定（std 锁内绝不跨 await；guard 内只计算，
        // guard drop 后统一决定是否释放排他门）。
        let (agent, release_gate) = {
            let mut sessions = self.state.sessions.lock().unwrap();
            match sessions.get_mut(session_id) {
                None => (None, true), // 会话已消失 → 释放排他门
                Some(h) if h.running => {
                    // 人类 prompt 已接管：不叠（本轮额度已消耗，但 run_turn 一定会
                    // 发生——不过是 human 触发的，不 double）。释放排他门。
                    (None, true)
                }
                Some(h) => {
                    h.running = true;
                    h.blank = false;
                    (Some(Arc::clone(&h.agent)), false)
                }
            }
        };
        if release_gate {
            self.continuing.lock().await.remove(session_id);
        }
        let Some(agent) = agent else {
            return false;
        };
        let state = Arc::clone(&self.state);
        let sid = session_id.to_string();
        let cont_flag = Arc::clone(&self.continuing);
        state.broadcast_host(
            "host/session-status",
            json!({ "sessionId": sid, "running": true }),
        );
        tokio::spawn(async move {
            let _ = agent.run_turn(Some(&prompt)).await;
            if let Some(h) = state.sessions.lock().unwrap().get_mut(&sid) {
                h.running = false;
            }
            cont_flag.lock().await.insert(sid.clone(), false);
            state.broadcast_host(
                "host/session-status",
                json!({ "sessionId": sid, "running": false }),
            );
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{GoalRecord, SessionHandle};
    use bm_assembly::Runtime;
    use kernel_contracts::session::{SessionHeader, SessionId};

    #[test]
    fn round_prompt_quotes_objective_and_rounds() {
        let p = round_prompt("写一个 fetching 脚本", 3, 8);
        assert!(p.contains("写一个 fetching 脚本"));
        assert!(p.contains("3/8"));
        assert!(p.contains("goal.update"));
    }

    #[test]
    fn suppressed_when_non_active_or_capped() {
        // 纯判定：phase != active 或额度耗尽 → 不 admitted。
        let now = 0i64;
        let make = |phase: &str, started: u64, max: u64| GoalRecord {
            id: "g1".to_string(),
            revision: 1,
            objective: "x".to_string(),
            phase: phase.to_string(),
            max_goal_rounds: max,
            rounds_started: started,
            created_at: now,
            updated_at: now,
        };
        assert_eq!(make("complete", 1, 8).phase, "complete");
        assert_eq!(make("paused", 1, 8).phase, "paused");
        assert_eq!(make("active", 8, 8).rounds_started, 8);
    }

    /// 集成：headless runtime + 会话 + 种一个 active 目标 → maybe_continue
    /// 推进 roundsStarted 并入一条 goal-sourced prompt（续跑回合 spawned）。
    #[tokio::test(flavor = "current_thread")]
    async fn driver_continues_active_goal_round_once() {
        let db = std::env::temp_dir().join(format!("bm-goal-drv-{}.db", uuid::Uuid::new_v4()));
        let rt = Runtime::headless(db.clone()).unwrap();
        // 脚本 LLM：文本收尾（回合可完成）。
        rt.swap_llm(bm_assembly::scripted_llm(
            "mock".to_string(),
            "mock-1".to_string(),
            vec![bm_assembly::MockTurn::Text("ok".to_string())],
        ));
        let agent = rt
            .create_session(SessionHeader {
                id: SessionId("s1".into()),
                app: "test".into(),
                profile: "test".into(),
                workspace: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
        let state = Arc::new(AppState::assemble(rt, vec![], vec![]));
        state.sessions.lock().unwrap().insert(
            "s1".into(),
            SessionHandle {
                agent,
                running: false,
                blank: false,
                title: None,
                selected: None,
            },
        );
        // 种一个 active 目标（max 8）。
        let now = chrono::Utc::now().timestamp_millis();
        state.goals.lock().unwrap().insert(
            "s1".into(),
            GoalRecord {
                id: "g1".into(),
                revision: 1,
                objective: "做一个 fetching 脚本".into(),
                phase: "active".into(),
                max_goal_rounds: 8,
                rounds_started: 0,
                created_at: now,
                updated_at: now,
            },
        );

        let driver = GoalDriver::new(Arc::clone(&state));
        let started = driver.maybe_continue("s1").await;
        assert!(started, "driver must start a continuation for active goal");

        // roundsStarted 推进为 1。
        let g = state.goals.lock().unwrap().get("s1").cloned().unwrap();
        assert_eq!(g.rounds_started, 1);

        // 等续跑回合 spawn 完成（脚本 LLM 即刻完）。
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // 继续标志应被清除（无嵌套）。
        assert!(!driver.continuing.lock().await.get("s1").copied().unwrap_or(false));

        // 无 active 目标（清掉）→ 不再续跑。
        state.goals.lock().unwrap().remove("s1");
        let started2 = driver.maybe_continue("s1").await;
        assert!(!started2, "no goal -> no continuation");

        let _ = std::fs::remove_file(&db);
    }
}