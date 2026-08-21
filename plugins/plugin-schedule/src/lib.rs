//! # plugin-schedule —— 定时任务工具插件（功能分类）。
//!
//! 把周期驱动会话的能力注册为 Agent 工具（经 [`SchedulePort`] 消费面）：
//! - `schedule.create`  创建定时任务（interval 秒级循环 或 cron 表达式简化匹配），
//!   到期时对目标会话发一条 user message（驱动 run_turn）
//! - `schedule.list`    列出全部活动任务（下次触发时间）
//! - `schedule.cancel`  取消任务
//!
//! 实现侧（web-server Scheduler）注入全局 [`SchedulePort`] 源（同 WorkdirPort
//! 模式）：装配方经 [`set_schedule_source`] 注入；工具执行时现读。未装配 →
//! schedule 工具返回 "schedule not configured"（诚实失败，不假成功）。
//!
//! 接线：装配方调用 [`register_all`] 注册全部工具并 `gate.enable`。之后
//! plugin-loop 每回合把已启用工具 schema 发给模型，工具调用经 ToolGate 执行。

use std::sync::{Arc, Mutex};

use bm_ports::{SchedulePort, ScheduleSpec, ScheduleTrigger};
use kernel_contracts::tools::{
    ToolExecutionInput, ToolExecutionResult, ToolHandler, ToolSchema,
};
use kernel_contracts::ToolError;

pub mod plugin;

pub use plugin::manifest;

/// schedule 工具 id 常量（门控白名单）。
pub const SCHEDULE_CREATE: &str = "schedule.create";
pub const SCHEDULE_LIST: &str = "schedule.list";
pub const SCHEDULE_CANCEL: &str = "schedule.cancel";

/// 全部 schedule 工具名。
pub const ALL_TOOL_NAMES: [&str; 3] = [SCHEDULE_CREATE, SCHEDULE_LIST, SCHEDULE_CANCEL];

/// 危险工具（需用户审批）：schedule.create 后台自动驱动会话回合（副作用面）；
/// list/cancel 只读或撤销，自动放行。
pub const DANGEROUS_TOOL_NAMES: [&str; 1] = [SCHEDULE_CREATE];

/// 全局 schedule 源（装配方经 [`set_schedule_source`] 注入；工具执行时现读）。
static SCHEDULE_SOURCE: Mutex<Option<Arc<dyn SchedulePort>>> = Mutex::new(None);

/// 注入 schedule 源（bm-assembly 装配点；web-server 实现 SchedulePort 并经组合根传入）。
pub fn set_schedule_source(src: Arc<dyn SchedulePort>) {
    *SCHEDULE_SOURCE.lock().unwrap() = Some(src);
}

/// 当前 schedule 源（未装配 → None）。
fn schedule() -> Option<Arc<dyn SchedulePort>> {
    SCHEDULE_SOURCE.lock().unwrap().clone()
}

/// 全部 schedule 工具 schema（文档/装配可查询）。
pub fn schemas() -> Vec<ToolSchema> {
    [SCHEDULE_CREATE, SCHEDULE_LIST, SCHEDULE_CANCEL]
        .iter()
        .map(|name| {
            let h: Arc<dyn ToolHandler> = match *name {
                SCHEDULE_CREATE => Arc::new(CreateScheduleTool),
                SCHEDULE_LIST => Arc::new(ListSchedulesTool),
                SCHEDULE_CANCEL => Arc::new(CancelScheduleTool),
                _ => unreachable!("known schedule tool name"),
            };
            ToolSchema {
                name: h.name().to_string(),
                description: h.description().to_string(),
                parameters: h.parameters(),
            }
        })
        .collect()
}

/// 注册全部 schedule 工具到注册表。
/// 调用方来自装配方（bm-assembly），传 plug-tools 的 `ToolRegistry` 具体类型。
/// 可重复调用（跳过已注册项，幂等）。
pub fn register_all(registry: &plugin_tools::ToolRegistry) -> Result<(), ToolError> {
    let handlers: Vec<Arc<dyn ToolHandler>> = vec![
        Arc::new(CreateScheduleTool),
        Arc::new(ListSchedulesTool),
        Arc::new(CancelScheduleTool),
    ];
    for h in handlers {
        if registry.get(h.name()).is_some() {
            continue; // 幂等：已注册跳过
        }
        registry.register(h)?;
    }
    Ok(())
}

fn arg_str(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key).and_then(serde_json::Value::as_str).map(str::to_string)
}

// ---- schedule.create ----

#[derive(Debug, Clone, Copy, Default)]
struct CreateScheduleTool;

#[async_trait::async_trait]
impl ToolHandler for CreateScheduleTool {
    fn name(&self) -> &str {
        SCHEDULE_CREATE
    }

    fn description(&self) -> &str {
        "创建定时任务：trigger=interval（interval_secs 秒级循环）或 trigger=cron（cron 表达式简化匹配：5 段 分 时 日 月 周）。到期时对目标 session（session_id，缺省当前活跃）发送 prompt 文本驱动 Agent 回合。返回 JSON：{id, trigger, nextAt}。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "trigger": { "type": "string", "enum": ["interval", "cron"], "description": "触发方式" },
                "interval_secs": { "type": "integer", "minimum": 1, "description": "interval 触发：间隔秒数" },
                "cron": { "type": "string", "description": "cron 触发：5 段表达式，如 \"0 */30 * * *\"（每 30 分钟）" },
                "prompt": { "type": "string", "description": "到期发给目标会话的提示文本" },
                "session_id": { "type": "string", "description": "目标会话 id；缺省 = 当前活跃会话" }
            },
            "required": ["trigger", "prompt"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(src) = schedule() else {
            return Err(ToolError::new("tool error: schedule not configured"));
        };
        let Some(trigger_kind) = arg_str(&input.arguments, "trigger") else {
            return Ok(ToolExecutionResult::error(
                "missing trigger (interval|cron)",
            ));
        };
        let Some(prompt) = arg_str(&input.arguments, "prompt") else {
            return Ok(ToolExecutionResult::error("missing prompt"));
        };
        let session_id = arg_str(&input.arguments, "session_id");
        let trigger = match trigger_kind.as_str() {
            "interval" => {
                let secs = input
                    .arguments
                    .get("interval_secs")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                if secs < 1 {
                    return Ok(ToolExecutionResult::error(
                        "interval_secs must be >= 1",
                    ));
                }
                ScheduleTrigger::Interval { secs }
            }
            "cron" => {
                let Some(expr) = arg_str(&input.arguments, "cron") else {
                    return Ok(ToolExecutionResult::error(
                        "cron expression missing for trigger=cron",
                    ));
                };
                ScheduleTrigger::Cron { expr }
            }
            other => {
                return Ok(ToolExecutionResult::error(format!(
                    "unknown trigger: {other} (interval|cron)"
                )))
            }
        };
        let id = src
            .schedule_create(ScheduleSpec {
                trigger,
                prompt,
                session_id,
            })
            .await?;
        Ok(ToolExecutionResult::ok(format!("created schedule {id}")))
    }
}

// ---- schedule.list ----

#[derive(Debug, Clone, Copy, Default)]
struct ListSchedulesTool;

#[async_trait::async_trait]
impl ToolHandler for ListSchedulesTool {
    fn name(&self) -> &str {
        SCHEDULE_LIST
    }

    fn description(&self) -> &str {
        "列出全部活动定时任务。返回 JSON 数组：[{id, trigger, prompt, sessionId, nextAtMs}]。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        _input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(src) = schedule() else {
            return Err(ToolError::new("tool error: schedule not configured"));
        };
        let views = src.schedule_list().await?;
        let arr: Vec<serde_json::Value> = views
            .iter()
            .map(|v| {
                serde_json::json!({
                    "id": v.id,
                    "trigger": v.trigger,
                    "prompt": v.prompt,
                    "sessionId": v.session_id,
                    "nextAtMs": v.next_at_ms,
                })
            })
            .collect();
        let text = serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string());
        Ok(ToolExecutionResult::ok(text))
    }
}

// ---- schedule.cancel ----

#[derive(Debug, Clone, Copy, Default)]
struct CancelScheduleTool;

#[async_trait::async_trait]
impl ToolHandler for CancelScheduleTool {
    fn name(&self) -> &str {
        SCHEDULE_CANCEL
    }

    fn description(&self) -> &str {
        "取消一个定时任务（id 来自 schedule.create 或 schedule.list）。返回确认文本。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "任务 id" }
            },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(src) = schedule() else {
            return Err(ToolError::new("tool error: schedule not configured"));
        };
        let Some(id) = arg_str(&input.arguments, "id") else {
            return Ok(ToolExecutionResult::error("missing id"));
        };
        src.schedule_cancel(&id).await?;
        Ok(ToolExecutionResult::ok(format!("cancelled schedule {id}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel_contracts::plugin::PluginCategory;

    #[test]
    fn manifest_and_schemas_are_consistent() {
        let m = manifest();
        assert_eq!(m.id, "plugin-schedule");
        assert_eq!(m.category, PluginCategory::Feature);
        let schemas = schemas();
        assert_eq!(schemas.len(), 3);
        for s in &schemas {
            assert!(ALL_TOOL_NAMES.contains(&s.name.as_str()), "unexpected schema {}", s.name);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_missing_trigger_is_business_error() {
        // 未装配 schedule 源 → 工具异常（诚实失败）；装配后缺 trigger → 业务错误。
        let tool = CreateScheduleTool;
        let r = tool
            .execute(ToolExecutionInput {
                name: SCHEDULE_CREATE.to_string(),
                arguments: serde_json::json!({ "prompt": "hello" }),
            })
            .await
            .unwrap_err();
        assert!(r.0.contains("not configured"));
    }
}