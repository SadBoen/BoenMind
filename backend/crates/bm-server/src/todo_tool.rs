//! 活任务清单工具（M2，编程应用核心）：模型在会话内维护"任务清单"。
//!
//! 数据模型 = 事件日志最后一条 `todo/write`（bm-protocol TodoWrite，
//! **全量快照**语义——事件即权威状态，前端投影无需 diff，重放可重建）。
//! 每次变更：读最后快照 → 应用操作 → append 新全量快照（审计链完整）。
//!
//! 入口两个（同一 apply 逻辑，同一事实源）：
//! - 工具面：`todo` 工具（模型回合内动态增删改——用户痛点"任务清单生成
//!   后不会实时插入/删除"的模型侧）；注册进全部会话工具面（通用能力）；
//! - REST 面：`POST /api/sessions/{id}/todos`（前端任务面板手动操作）。

use bm_kernel::EventLog;
use bm_protocol::{BranchId, CoreEvent, EventKind, SessionId, TodoItem};

/// 任务状态白名单（协议注释约定 pending | in_progress | completed）。
const STATUSES: &[&str] = &["pending", "in_progress", "completed"];

/// `todo` 工具定义（模型可见 schema）。
pub fn todo_def() -> bm_loop::model::ToolDef {
    bm_loop::model::ToolDef::new(
        "todo",
        "维护当前会话的活任务清单（编程任务分解/进度跟踪）。action: \
         add=新增条目（content 必填，priority 可选）；update=按 index 修改 \
         条目的 content/status/priority（至少一项）；remove=按 index 删除 \
         条目；list=查看当前清单。index 从 1 开始。每次操作后返回最新完整清单。\
         长任务开始时先分解为清单，每完成一步用 update 推进状态。",
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "update", "remove", "list"],
                    "description": "操作类型"
                },
                "index": {
                    "type": "integer",
                    "description": "条目序号（1 起；update/remove 必填）"
                },
                "content": {
                    "type": "string",
                    "description": "任务内容（add 必填；update 可选覆盖）"
                },
                "status": {
                    "type": "string",
                    "enum": STATUSES,
                    "description": "任务状态（默认 pending）"
                },
                "priority": {
                    "type": "string",
                    "description": "优先级标签（如 high/medium/low，可选）"
                }
            },
            "required": ["action"]
        }),
    )
}

/// 读事件日志最后一条 TodoWrite（会话当前清单；无 → 空清单）。
/// 全量 replay 过滤：起步可接受（会话级规模），长会话按类型倒查留 M3。
async fn load_todos(log: &EventLog, session_id: &str) -> Result<Vec<TodoItem>, String> {
    let sid = SessionId::new(session_id);
    let bid = BranchId::new("main");
    let evs = log
        .replay(&sid, &bid)
        .await
        .map_err(|e| format!("读取事件日志失败: {e}"))?;
    let mut todos: Vec<TodoItem> = Vec::new();
    for ev in evs {
        if let EventKind::Core(CoreEvent::TodoWrite { todos: t }) = ev.kind {
            todos = t;
        }
    }
    Ok(todos)
}

/// 应用一次清单操作并 append 新快照，返回最新清单。
/// 工具面与 REST 面共用（同一事实源、同一校验）。
pub async fn apply_todo_op(
    log: &EventLog,
    session_id: &str,
    action: &str,
    index: Option<usize>,
    content: Option<&str>,
    status: Option<&str>,
    priority: Option<&str>,
) -> Result<Vec<TodoItem>, String> {
    let mut todos = load_todos(log, session_id).await?;
    let i0 = index.map(|i| i.checked_sub(1).ok_or_else(|| "index 必须 >= 1".to_string()))
        .transpose()?;
    match action {
        "add" => {
            let content = content
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "add 需要非空 content".to_string())?;
            let status = status.unwrap_or("pending").to_string();
            if !STATUSES.contains(&status.as_str()) {
                return Err(format!("非法 status: {status}（可选 pending/in_progress/completed）"));
            }
            todos.push(TodoItem {
                content: content.to_string(),
                status,
                priority: priority.map(str::to_string),
            });
        }
        "update" => {
            let idx = i0.ok_or_else(|| "update 需要 index".to_string())?;
            let len = todos.len();
            let item = todos
                .get_mut(idx)
                .ok_or_else(|| format!("index {idx} 越界（清单共 {len} 条）"))?;
            if let Some(c) = content {
                let c = c.trim();
                if c.is_empty() {
                    return Err("update 的 content 不能为空".to_string());
                }
                item.content = c.to_string();
            }
            if let Some(s) = status {
                if !STATUSES.contains(&s) {
                    return Err(format!("非法 status: {s}"));
                }
                item.status = s.to_string();
            }
            if let Some(p) = priority {
                item.priority = Some(p.to_string());
            }
        }
        "remove" => {
            let idx = i0.ok_or_else(|| "remove 需要 index".to_string())?;
            if idx >= todos.len() {
                return Err(format!("index {idx} 越界（清单共 {} 条）", todos.len()));
            }
            todos.remove(idx);
        }
        "list" => {}
        other => return Err(format!("非法 action: {other}（add/update/remove/list）")),
    }
    // 快照落日志（失败即失败——清单变更必须可审计可重放）
    log.append(
        SessionId::new(session_id),
        BranchId::new("main"),
        EventKind::Core(CoreEvent::TodoWrite { todos: todos.clone() }),
        bm_kernel::SurfaceIntent::None,
    )
    .await
    .map_err(|e| format!("清单快照落日志失败: {e}"))?;
    Ok(todos)
}

/// `todo` 工具执行（返回形状对齐内置工具 `{content:[{type:"text"}]}`）。
pub async fn execute_todo(
    log: &EventLog,
    session_id: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let action = args
        .get("action")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "todo 需要字符串参数 action".to_string())?;
    let index = args.get("index").and_then(serde_json::Value::as_u64).map(|i| i as usize);
    let content = args.get("content").and_then(serde_json::Value::as_str);
    let status = args.get("status").and_then(serde_json::Value::as_str);
    let priority = args.get("priority").and_then(serde_json::Value::as_str);
    let todos = apply_todo_op(log, session_id, action, index, content, status, priority).await?;
    let text = if todos.is_empty() {
        "清单为空".to_string()
    } else {
        let lines = todos
            .iter()
            .enumerate()
            .map(|(i, t)| {
                format!(
                    "{}. [{}] {}{}",
                    i + 1,
                    t.status,
                    t.content,
                    t.priority.as_ref().map(|p| format!("（{p}）")).unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("当前清单（共 {} 条）：\n{lines}", todos.len())
    };
    Ok(serde_json::json!({
        "content": [{ "type": "text", "text": text }],
        "details": null,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_kernel::{InMemoryEventStore, SurfaceIntent};
    use std::sync::Arc;

    fn test_log() -> EventLog {
        EventLog::new(Arc::new(InMemoryEventStore::new()))
    }

    #[tokio::test]
    async fn add_update_remove_roundtrip() {
        let log = test_log();
        let mut todos = apply_todo_op(&log, "s1", "add", None, Some("任务甲"), None, Some("high")).await.unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "任务甲");
        assert_eq!(todos[0].status, "pending");
        assert_eq!(todos[0].priority.as_deref(), Some("high"));

        todos = apply_todo_op(&log, "s1", "add", None, Some("任务乙"), None, None).await.unwrap();
        assert_eq!(todos.len(), 2);

        // 按序号更新（第 2 条 → in_progress）
        todos = apply_todo_op(&log, "s1", "update", Some(2), None, Some("in_progress"), None).await.unwrap();
        assert_eq!(todos[1].status, "in_progress");

        // 删除第 1 条
        todos = apply_todo_op(&log, "s1", "remove", Some(1), None, None, None).await.unwrap();
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "任务乙");

        // list 不改状态
        let list = apply_todo_op(&log, "s1", "list", None, None, None, None).await.unwrap();
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    async fn validation_rejects_bad_input() {
        let log = test_log();
        // add 缺 content
        let err = apply_todo_op(&log, "s1", "add", None, None, None, None).await.unwrap_err();
        assert!(err.contains("content"), "{err}");
        // 非法 status
        let err = apply_todo_op(&log, "s1", "add", None, Some("x"), Some("done"), None).await.unwrap_err();
        assert!(err.contains("status"), "{err}");
        // update 越界
        let err = apply_todo_op(&log, "s1", "update", Some(9), Some("y"), None, None).await.unwrap_err();
        assert!(err.contains("越界"), "{err}");
        // remove 越界
        let err = apply_todo_op(&log, "s1", "remove", Some(1), None, None, None).await.unwrap_err();
        assert!(err.contains("越界"), "{err}");
        // 非法 action
        let err = apply_todo_op(&log, "s1", "clear", None, None, None, None).await.unwrap_err();
        assert!(err.contains("action"), "{err}");
    }

    #[tokio::test]
    async fn snapshot_is_replayable() {
        let log = test_log();
        apply_todo_op(&log, "s1", "add", None, Some("甲"), None, None).await.unwrap();
        apply_todo_op(&log, "s1", "add", None, Some("乙"), None, None).await.unwrap();
        // 从日志重放重建（模拟前端 GET /todos 或重启恢复）
        let evs = log
            .replay(&SessionId::new("s1"), &BranchId::new("main"))
            .await
            .unwrap();
        let mut last = None;
        for ev in evs {
            if let EventKind::Core(CoreEvent::TodoWrite { todos }) = ev.kind {
                last = Some(todos);
            }
        }
        let todos = last.unwrap();
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].content, "甲");
        assert_eq!(todos[1].content, "乙");
    }

    #[tokio::test]
    async fn execute_todo_shapes_tool_output() {
        let log = test_log();
        let v = execute_todo(&log, "s1", &serde_json::json!({ "action": "add", "content": "写测试" }))
            .await
            .unwrap();
        assert!(v["content"][0]["text"].as_str().unwrap().contains("写测试"), "{v}");
        // 缺 action → 错误
        let err = execute_todo(&log, "s1", &serde_json::json!({})).await.unwrap_err();
        assert!(err.contains("action"), "{err}");
    }
}
