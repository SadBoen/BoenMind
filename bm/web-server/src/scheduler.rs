//! 定时任务调度器（web-server 实现 [`bm_ports::SchedulePort`]）。
//!
//! 后台循环每秒唤醒：检查到期任务 → 对目标会话驱动一个回合（复用
//! session.prompt 的 run_turn 语义：置 running、tokio::spawn、广播状态）→
//! interval 任务重排 next_at；cron 任务触发一次后按表达式重排（简化匹配）。
//!
//! 目标会话缺省 = 当前活跃会话（state.sessions 里 running=true 的；若同时多个
//! 取第一个；无活跃会话则跳过本次触发——诚实失败，不假成功）。
//!
//! 与 goal 的关系：goal 自动续跑（M3.5）可复用本调度器驱动会话回合；
//! 独立语义（目标完成判定在 agent 侧）见 HANDOFF 待办。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bm_ports::{SchedulePort, ScheduleSpec, ScheduleTrigger, ScheduleView};
use serde_json::json;
use tokio::sync::Mutex;

use crate::api::AppState;

/// 后台循环唤醒周期。
const TICK: Duration = Duration::from_secs(1);

/// 一个已登记的任务（内部状态）。
#[derive(Debug, Clone)]
struct Entry {
    id: String,
    spec: ScheduleSpec,
    next_at_ms: i64,
    /// cron 表达式上次匹配的分钟键（防同分钟重复触发）。
    last_match_minutes: Option<i64>,
}

/// 定时任务调度器。
pub struct Scheduler {
    state: Arc<AppState>,
    /// id → 条目（Mutex：后台循环与工具调用并发访问）。
    entries: Mutex<HashMap<String, Entry>>,
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler").finish_non_exhaustive()
    }
}

impl Scheduler {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// 启动后台驱动循环（web-server main 在 serve 前调用一次；spawn 后返回）。
    pub fn start(self: &Arc<Self>) {
        let sched = Arc::clone(self);
        tokio::spawn(async move {
            sched.run_loop().await;
        });
    }

    async fn run_loop(self: &Arc<Self>) {
        loop {
            tokio::time::sleep(TICK).await;
            let now = chrono::Utc::now().timestamp_millis();
            // 到期条目移出表待触发，未到期保留（触发后 interval/cron 重排再插回）。
            let due = {
                let mut entries = self.entries.lock().await;
                collect_due(&mut entries, now)
            };
            for e in due {
                self.fire(&e).await;
            }
        }
    }

    /// 触发一次：驱动目标会话回合，然后重排（interval 循环 / cron 简化重排）。
    async fn fire(&self, e: &Entry) {
        let prompt = e.spec.prompt.clone();
        let session_id = self.resolve_target(e).await;
        if let Some(sid) = session_id {
            self.drive_session(&sid, &prompt).await;
        }
        // 重排：interval → next = now + secs；cron → 触发后按 cron 简化重排。
        let next = match &e.spec.trigger {
            ScheduleTrigger::Interval { secs } => {
                chrono::Utc::now().timestamp_millis() + (*secs as i64) * 1000
            }
            ScheduleTrigger::Cron { expr } => cron_next_ms(expr, e.last_match_minutes).unwrap_or(i64::MAX),
        };
        if next < i64::MAX {
            let mut entries = self.entries.lock().await;
            entries.insert(
                e.id.clone(),
                Entry {
                    id: e.id.clone(),
                    spec: e.spec.clone(),
                    next_at_ms: next,
                    last_match_minutes: self.last_match_minutes(e).await,
                },
            );
        }
    }

    /// cron 本次匹配的分钟键（下次避免重复）。
    async fn last_match_minutes(&self, _e: &Entry) -> Option<i64> {
        Some(
            (chrono::Utc::now().timestamp() / 60) * 60,
        )
    }

    /// 解析目标会话：spec.session_id 指定 → 该会话若存在；缺省 → 当前活跃会话
    /// （state.sessions 中 running 或非 blank 的第一个）；找不到 → None（跳过）。
    async fn resolve_target(&self, e: &Entry) -> Option<String> {
        if let Some(sid) = &e.spec.session_id {
            let exists = self.state.sessions.lock().unwrap().contains_key(sid);
            return if exists { Some(sid.clone()) } else { None };
        }
        let sessions = self.state.sessions.lock().unwrap();
        sessions
            .iter()
            .find(|(_, h)| h.running || !h.blank)
            .map(|(id, _)| id.clone())
    }

    /// 驱动会话回合（复用 session.prompt 语义：置 running、spawn run_turn、广播状态）。
    async fn drive_session(&self, session_id: &str, prompt: &str) {
        let agent = {
            let mut sessions = self.state.sessions.lock().unwrap();
            let Some(h) = sessions.get_mut(session_id) else {
                return;
            };
            if h.running {
                return; // 忙：跳过本次触发（不排队，防叠加）。
            }
            h.running = true;
            h.blank = false;
            Arc::clone(&h.agent)
        };
        let state = Arc::clone(&self.state);
        let sid = session_id.to_string();
        let text = prompt.to_string();
        state.broadcast_host(
            "host/session-status",
            json!({ "sessionId": sid, "running": true }),
        );
        tokio::spawn(async move {
            let _ = agent.run_turn(Some(&text)).await;
            if let Some(h) = state.sessions.lock().unwrap().get_mut(&sid) {
                h.running = false;
            }
            state.broadcast_host(
                "host/session-status",
                json!({ "sessionId": sid, "running": false }),
            );
        });
    }
}

/// 取出到期条目（`next_at_ms <= now`）并从表中移除，未到期保留。
/// 独立函数便于单测（run_loop 是无限循环，测试不可达）。
/// 回归：曾把谓词写反（保留已到期、删除未到期），任何未来任务下个 tick
/// 即被清除——定时任务整体失效。
fn collect_due(entries: &mut HashMap<String, Entry>, now: i64) -> Vec<Entry> {
    let mut due = Vec::new();
    entries.retain(|_, e| {
        if e.next_at_ms <= now {
            due.push(e.clone());
            false
        } else {
            true
        }
    });
    due
}

/// cron 简化匹配：5 段（分 时 日 月 周），只处理固定数值与 `*` 与
/// `*/n` 步进。返回下次触发毫秒；表达式不合法 → None（任务失效）。
/// 实现为"当前分钟是否匹配 + next minute 扫描"（最多扫 24h 防死循环）。
fn cron_next_ms(expr: &str, _last_match: Option<i64>) -> Option<i64> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return None;
    }
    let minute = parts[0];
    let hour = parts[1];
    let _day = parts[2];
    let _month = parts[3];
    let _weekday = parts[4];
    // 简化：只实现 分 时 两层（日/月/周按 `*` 处理；非 `*` 视为不可匹配 → None）。
    if _day != "*" || _month != "*" || _weekday != "*" {
        return None;
    }
    enum Field {
        Any,              // `*`
        Set(Vec<u32>),    // 固定值 / */n 步进
        Invalid,          // 非法（*/x、*/0、非数字）
    }
    let field_spec = |spec: &str| -> Field {
        if spec == "*" {
            Field::Any
        } else if let Some(stripped) = spec.strip_prefix("*/") {
            match stripped.parse::<u32>() {
                Ok(step) if step > 0 => {
                    let mut v = Vec::new();
                    let mut cur = 0;
                    loop {
                        v.push(cur);
                        cur += step;
                        if cur > 59 {
                            break;
                        }
                    }
                    Field::Set(v)
                }
                _ => Field::Invalid, // */x 或 */0
            }
        } else {
            match spec.parse::<u32>() {
                Ok(x) => Field::Set(vec![x]),
                Err(_) => Field::Invalid,
            }
        }
    };
    let minutes = field_spec(minute);
    let hours = field_spec(hour);
    use chrono::Timelike;
    let now = chrono::Utc::now();
    // 扫描未来最多 24h 的分钟，找第一个匹配。
    for off in 1..=(24 * 60) {
        let t = now + chrono::Duration::minutes(off as i64);
        let min = t.minute();
        let hr = t.hour();
        let min_ok = match &minutes {
            Field::Any => true,
            Field::Set(v) => v.contains(&min),
            Field::Invalid => return None, // 非法表达式 → 任务失效
        };
        let hr_ok = match &hours {
            Field::Any => true,
            Field::Set(v) => v.contains(&hr),
            Field::Invalid => return None,
        };
        if min_ok && hr_ok {
            return Some(t.timestamp_millis());
        }
    }
    None
}

#[async_trait]
impl SchedulePort for Scheduler {
    async fn schedule_create(&self, spec: ScheduleSpec) -> Result<String, kernel_contracts::ToolError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let next = match &spec.trigger {
            ScheduleTrigger::Interval { secs } => now + (*secs as i64) * 1000,
            ScheduleTrigger::Cron { expr } => cron_next_ms(expr, None).ok_or_else(|| {
                kernel_contracts::ToolError::new(format!(
                    "tool error: invalid cron expression '{expr}' (5 fields, minute/hour/`*/n` supported)"
                ))
            })?,
        };
        let mut entries = self.entries.lock().await;
        entries.insert(
            id.clone(),
            Entry {
                id: id.clone(),
                spec,
                next_at_ms: next,
                last_match_minutes: None,
            },
        );
        Ok(id)
    }

    async fn schedule_list(&self) -> Result<Vec<ScheduleView>, kernel_contracts::ToolError> {
        let entries = self.entries.lock().await;
        let mut views: Vec<ScheduleView> = entries
            .values()
            .map(|e| ScheduleView {
                id: e.id.clone(),
                trigger: match &e.spec.trigger {
                    ScheduleTrigger::Interval { secs } => format!("interval:{secs}s"),
                    ScheduleTrigger::Cron { expr } => format!("cron:{expr}"),
                },
                prompt: e.spec.prompt.clone(),
                session_id: e.spec.session_id.clone(),
                next_at_ms: Some(e.next_at_ms),
            })
            .collect();
        views.sort_by_key(|a| a.next_at_ms);
        Ok(views)
    }

    async fn schedule_cancel(&self, id: &str) -> Result<(), kernel_contracts::ToolError> {
        let mut entries = self.entries.lock().await;
        if entries.remove(id).is_none() {
            return Err(kernel_contracts::ToolError::new(format!(
                "tool error: schedule {id} not found"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_next_ms_basic() {
        // "*/5 * * * *" → 每 5 分钟，必有下次。
        let n = cron_next_ms("*/5 * * * *", None);
        assert!(n.is_some());
        // "* * * * *" → 每分钟 → 必有。
        assert!(cron_next_ms("* * * * *", None).is_some());
        // 日/月/周非 `*` → 不支持 → None（任务失效，诚实）。
        assert!(cron_next_ms("0 * * 1 *", None).is_none());
        // 段数不对 → None。
        assert!(cron_next_ms("* * *", None).is_none());
        // 非法步进 → None。
        assert!(cron_next_ms("*/x * * * *", None).is_none());
    }

    #[test]
    fn cron_next_ms_hourly() {
        // "0 */2 * * *"（每 2 小时 0 分）应有 next（24h 内必然命中）。
        assert!(cron_next_ms("0 */2 * * *", None).is_some());
    }

    /// 调度循环取到期逻辑：到期移出待触发、未到期保留（回归 retain 反转）。
    #[test]
    fn collect_due_removes_expired_and_keeps_future() {
        let mk = |id: &str, next: i64| Entry {
            id: id.to_string(),
            spec: ScheduleSpec {
                trigger: ScheduleTrigger::Interval { secs: 60 },
                prompt: "p".to_string(),
                session_id: None,
            },
            next_at_ms: next,
            last_match_minutes: None,
        };
        let mut entries = HashMap::new();
        entries.insert("past".to_string(), mk("past", 900));
        entries.insert("future".to_string(), mk("future", 1100));
        entries.insert("now".to_string(), mk("now", 1000));
        let due = collect_due(&mut entries, 1000);
        assert_eq!(due.len(), 2, "past + now are due");
        assert!(due.iter().all(|e| e.next_at_ms <= 1000));
        // 未到期必须仍在表里（原 bug 会把它删掉 → 定时任务整体失效）。
        assert!(entries.contains_key("future"), "future entry must survive the tick");
        assert!(!entries.contains_key("past") && !entries.contains_key("now"));
    }
}