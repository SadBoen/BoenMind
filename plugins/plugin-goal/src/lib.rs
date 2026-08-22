//! # plugin-goal —— 目标管理工具插件（功能分类）。
//!
//! 模型侧目标控制（对齐 DSH `dsh-tool-goal`）：
//! - `goal.get`     读当前目标（无 → null），含 id/revision/phase/roundsStarted/
//!   maxGoalRounds + activation
//! - `goal.create`  建目标（objective + max_goal_rounds 可选，缺省内部默认）
//! - `goal.update`  更新目标：edit/pause/resume/complete/blocked
//!   （CAS revision 守卫；blocked 需 blocked_reason）
//!
//! 实现侧（web-server）经 [`GoalPort`] 消费面接入（同 WorkdirPort/SchedulePort
//! 模式）：装配方经 [`register_all`] 把源**构造注入**到每个工具 handler，工具
//! 执行时现读；源随 handler 存在于每 Runtime 独立的 ToolRegistry（无进程级全局）。
//! 未装配 → goal 工具返回 "goal not configured"（诚实失败）。
//!
//! 权威拆分：本插件只做工具消费面（model-facing）；goal-round-driver
//! （同会话续跑）在 web-server 回合完成点，负责 roundsStarted 推进与续跑注入
//! ——与官方 `dsh-tool-goal`/`dsh-goal-round-driver` 分工一致。

use std::sync::Arc;

use bm_ports::{GoalAction, GoalPort, GoalView};
use kernel_contracts::tools::{
    ToolExecutionInput, ToolExecutionResult, ToolHandler, ToolSchema,
};
use kernel_contracts::ToolError;

pub mod plugin;

pub use plugin::manifest;

/// 创建目标的缺省 max_goal_rounds（对齐 dsh-goal defaultMaxGoalRounds 的精神；
/// 一个新目标若只给 objective，给合理默认上限）。
pub const DEFAULT_MAX_GOAL_ROUNDS: u64 = 8;

/// goal 工具 id 常量（门控白名单）。
pub const GOAL_GET: &str = "goal.get";
pub const GOAL_CREATE: &str = "goal.create";
pub const GOAL_UPDATE: &str = "goal.update";

/// 全部 goal 工具名。
pub const ALL_TOOL_NAMES: [&str; 3] = [GOAL_GET, GOAL_CREATE, GOAL_UPDATE];

/// 危险工具（需用户审批）：goal.create/update 改变目标状态机（续跑行为副作用）；
/// goal.get 只读投影，自动放行。
pub const DANGEROUS_TOOL_NAMES: [&str; 2] = [GOAL_CREATE, GOAL_UPDATE];



/// session_id：工具参数显式传；缺省 = 当前活跃会话（web-server 侧从
/// state.sessions 取；见 GoalRouter 实现——这里只把参数透传，缺省由实现定）。 
fn session_arg(args: &serde_json::Value) -> Option<String> {
    args.get("session_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn arg_str(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key).and_then(serde_json::Value::as_str).map(str::to_string)
}

/// 全部 goal 工具 schema（文档/装配可查询）。
pub fn schemas() -> Vec<ToolSchema> {
    [GOAL_GET, GOAL_CREATE, GOAL_UPDATE]
        .iter()
        .map(|name| {
            let h: Arc<dyn ToolHandler> = match *name {
                GOAL_GET => Arc::new(GetGoalTool { src: None }),
                GOAL_CREATE => Arc::new(CreateGoalTool { src: None }),
                GOAL_UPDATE => Arc::new(UpdateGoalTool { src: None }),
                _ => unreachable!("known goal tool name"),
            };
            ToolSchema {
                name: h.name().to_string(),
                description: h.description().to_string(),
                parameters: h.parameters(),
            }
        })
        .collect()
}

/// 注册全部 goal 工具到注册表（源构造注入：每个 handler 捕获同一 `src`）。
/// 调用方来自装配方（bm-assembly），传实现 `ToolRegistrarPort` 的注册表
/// （plugin-tools::ToolRegistry）。总是覆盖注册（HashMap 语义）：后装者替换
/// 先前的 handler——终态以最后一次为准。
pub fn register_all(
    registry: &dyn bm_ports::ToolRegistrarPort,
    src: Option<Arc<dyn GoalPort>>,
) -> Result<(), ToolError> {
    let handlers: Vec<Arc<dyn ToolHandler>> = vec![
        Arc::new(GetGoalTool { src: src.clone() }),
        Arc::new(CreateGoalTool { src: src.clone() }),
        Arc::new(UpdateGoalTool { src }),
    ];
    for h in handlers {
        registry.register(h)?;
    }
    Ok(())
}

/// GoalView → 紧凑 JSON（wire 同款；activation 为活观测）。
fn view_json(v: &GoalView) -> serde_json::Value {
    serde_json::json!({
        "goal": {
            "id": v.id,
            "revision": v.revision,
            "objective": v.objective,
            "phase": v.phase,
            "roundsStarted": v.rounds_started,
            "maxGoalRounds": v.max_goal_rounds,
            "blockedReason": v.blocked_reason,
        },
        "activation": v.activation,
    })
}

// ---- goal.get ----

struct GetGoalTool {
    /// goal 源（构造注入；None = 未装配，执行诚实报错）。
    src: Option<Arc<dyn GoalPort>>,
}

#[async_trait::async_trait]
impl ToolHandler for GetGoalTool {
    fn name(&self) -> &str {
        GOAL_GET
    }

    fn description(&self) -> &str {
        "读取当前目标（无 → {goal:null}）。返回 JSON：{goal:{id, revision, objective, phase, roundsStarted, maxGoalRounds, blockedReason}, activation}。更新前先读并复制确切 goal_id 与 revision。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "目标会话 id；缺省 = 当前活跃会话" }
            },
            "required": [],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(src) = self.src.as_ref() else {
            return Err(ToolError::new("tool error: goal not configured"));
        };
        let sid = session_arg(&input.arguments).unwrap_or_default();
        match src.goal_get(&sid).await? {
            Some(v) => {
                let text = serde_json::to_string(&view_json(&v)).unwrap_or_else(|_| "{}".to_string());
                Ok(ToolExecutionResult::ok(text))
            }
            None => Ok(ToolExecutionResult::ok("{\"goal\":null}".to_string())),
        }
    }
}

// ---- goal.create ----

struct CreateGoalTool {
    /// goal 源（构造注入；None = 未装配，执行诚实报错）。
    src: Option<Arc<dyn GoalPort>>,
}

#[async_trait::async_trait]
impl ToolHandler for CreateGoalTool {
    fn name(&self) -> &str {
        GOAL_CREATE
    }

    fn description(&self) -> &str {
        "为一个长期完成的单一目标创建目标（objective 非空；max_goal_rounds 可选，缺省 8）。常规单回合工作不要建目标。返回新目标 JSON（同 get_goal 形状）。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "objective": { "type": "string", "description": "目标描述（非空，≥1 字符）" },
                "max_goal_rounds": { "type": "integer", "minimum": 1, "description": "最大自动续跑轮数；缺省 8" },
                "session_id": { "type": "string", "description": "目标会话 id；缺省 = 当前活跃会话" }
            },
            "required": ["objective"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(src) = self.src.as_ref() else {
            return Err(ToolError::new("tool error: goal not configured"));
        };
        let Some(objective) = arg_str(&input.arguments, "objective") else {
            return Ok(ToolExecutionResult::error("missing objective"));
        };
        if objective.trim().is_empty() {
            return Ok(ToolExecutionResult::error(
                "objective must be at least 1 character",
            ));
        }
        let sid = session_arg(&input.arguments).unwrap_or_default();
        let max_rounds = input
            .arguments
            .get("max_goal_rounds")
            .and_then(serde_json::Value::as_u64)
            .filter(|n| *n > 0);
        match src.goal_create(&sid, objective.trim(), max_rounds).await {
            Ok(v) => {
                let text = serde_json::to_string(&view_json(&v)).unwrap_or_else(|_| "{}".to_string());
                Ok(ToolExecutionResult::ok(text))
            }
            Err(e) => Ok(ToolExecutionResult::error(format!("goal create failed: {e}"))),
        }
    }
}

// ---- goal.update ----

struct UpdateGoalTool {
    /// goal 源（构造注入；None = 未装配，执行诚实报错）。
    src: Option<Arc<dyn GoalPort>>,
}

#[async_trait::async_trait]
impl ToolHandler for UpdateGoalTool {
    fn name(&self) -> &str {
        GOAL_UPDATE
    }

    fn description(&self) -> &str {
        "更新目标（CAS revision 守卫，先 get_goal 复制确切 id/revision）。action：edit（改 objective/max_goal_rounds）、pause、resume、complete（目标确实达成才标）、blocked（同一阻塞持续 ≥3 轮且 blocked_reason 描述具体条件）。返回新目标 JSON。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "goal_id": { "type": "string", "description": "目标 id（来自 get_goal）" },
                "revision": { "type": "integer", "description": "目标 revision（来自 get_goal）" },
                "action": { "type": "string", "enum": ["edit", "pause", "resume", "complete", "blocked"], "description": "操作" },
                "objective": { "type": "string", "description": "仅 edit：新目标描述" },
                "max_goal_rounds": { "type": "integer", "minimum": 1, "description": "仅 edit：新轮数上限" },
                "blocked_reason": { "type": "string", "description": "仅 blocked：具体阻塞条件" },
                "session_id": { "type": "string", "description": "目标会话 id；缺省 = 当前活跃会话" }
            },
            "required": ["goal_id", "revision", "action"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(src) = self.src.as_ref() else {
            return Err(ToolError::new("tool error: goal not configured"));
        };
        let Some(goal_id) = arg_str(&input.arguments, "goal_id") else {
            return Ok(ToolExecutionResult::error("missing goal_id"));
        };
        let Some(rev) = input.arguments.get("revision").and_then(serde_json::Value::as_u64) else {
            return Ok(ToolExecutionResult::error("missing revision"));
        };
        let Some(action_str) = arg_str(&input.arguments, "action") else {
            return Ok(ToolExecutionResult::error("missing action"));
        };
        let action = match action_str.as_str() {
            "edit" => GoalAction::Edit,
            "pause" => GoalAction::Pause,
            "resume" => GoalAction::Resume,
            "complete" => GoalAction::Complete,
            "blocked" => GoalAction::Blocked,
            other => {
                return Ok(ToolExecutionResult::error(format!(
                    "unknown action: {other} (edit|pause|resume|complete|blocked)"
                )))
            }
        };
        // blocked 必填 blocked_reason（权威：模型报告的条件）。
        let blocked_reason = arg_str(&input.arguments, "blocked_reason");
        if action == GoalAction::Blocked && blocked_reason.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true) {
            return Ok(ToolExecutionResult::error(
                "blocked action requires blocked_reason",
            ));
        }
        let sid = session_arg(&input.arguments).unwrap_or_default();
        let objective = arg_str(&input.arguments, "objective");
        let max_rounds = input
            .arguments
            .get("max_goal_rounds")
            .and_then(serde_json::Value::as_u64)
            .filter(|n| *n > 0);
        match src
            .goal_update(
                &sid,
                &goal_id,
                rev,
                action,
                objective.as_deref(),
                max_rounds,
                blocked_reason.as_deref(),
            )
            .await
        {
            Ok(v) => {
                let text = serde_json::to_string(&view_json(&v)).unwrap_or_else(|_| "{}".to_string());
                Ok(ToolExecutionResult::ok(text))
            }
            Err(e) => Ok(ToolExecutionResult::error(format!("goal update failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel_contracts::plugin::PluginCategory;

    #[test]
    fn manifest_and_schemas_are_consistent() {
        let m = manifest();
        assert_eq!(m.id, "plugin-goal");
        assert_eq!(m.category, PluginCategory::Feature);
        let schemas = schemas();
        assert_eq!(schemas.len(), 3);
        for s in &schemas {
            assert!(ALL_TOOL_NAMES.contains(&s.name.as_str()), "unexpected schema {}", s.name);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unconfigured_source_fails_loud() {
        // 未装配 goal 源 → 工具异常（诚实失败，不假成功）。
        let tool = GetGoalTool { src: None };
        let r = tool
            .execute(ToolExecutionInput {
                name: GOAL_GET.to_string(),
                arguments: serde_json::json!({}),
            })
            .await
            .unwrap_err();
        assert!(r.0.contains("not configured"));
    }
}