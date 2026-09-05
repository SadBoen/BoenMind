//! 事件物化:确定性 reducer,把一条事件映射为规范状态的行变更。
//!
//! 同一函数服务两条路径(重放确定性的结构保证,ADR-0004 条件 1/混沌③):
//! - 写穿:核心循环每个事件落日志后立即物化(T2);
//! - 重放:从空库/快照自事件日志重建投影(T6)。
//!
//! M1 语义注记:`agent.created` 时核心即时完成 created→starting→running,
//! 且无中间事件——物化按此直接落 running;预算计数自事件可导出:
//! used_tokens = Σ(model.invocation.completed.usage),turns_used = Σ(agent.completed)。

use crate::error::StoreResult;
use crate::sqlite_state::StateDb;
use bm_contract::budget::Budget;
use bm_contract::events::{EventEnvelope, EventType};

impl StateDb {
    /// 物化一条事件(单事务)。非状态类事件是合法 no-op。
    pub fn materialize(&self, event: &EventEnvelope) -> StoreResult<()> {
        let p = &event.payload;
        let ts = event.occurred_at.as_str();
        let conn = self.conn.lock().expect("锁未中毒");
        conn.execute_batch("BEGIN")?;
        let result: rusqlite::Result<usize> = (|| {
            match event.event_type {
                EventType::SessionCreated => {
                    let id = str_field(p, "session_id")?;
                    let agent = str_field(p, "agent_id")?;
                    // 显式列名 + workspace_id 保 NULL:事件载荷不含绑定
                    // (绑定走 save_session_workspace 投影),重放不得抹掉
                    conn.execute(
                        "INSERT OR REPLACE INTO sessions(id, state, agent_id, created_at, workspace_id)
                         VALUES(?1, 'active', ?2, ?3, NULL)",
                        rusqlite::params![id, agent, ts],
                    )?;
                    Ok(1)
                }
                EventType::SessionClosed => {
                    conn.execute(
                        "UPDATE sessions SET state='closed' WHERE id=?1",
                        [str_field(p, "session_id")?],
                    )?;
                    Ok(1)
                }
                EventType::SessionResumed => {
                    conn.execute(
                        "UPDATE sessions SET state='active' WHERE id=?1",
                        [str_field(p, "session_id")?],
                    )?;
                    Ok(1)
                }
                EventType::AgentCreated => {
                    let model_chain = p["model_chain"].clone();
                    let budget: Option<Budget> =
                        serde_json::from_value(p["budget"].clone()).unwrap_or(None);
                    conn.execute(
                        "INSERT OR REPLACE INTO agents(
                            id, session_id, name, model_chain, state,
                            budget_max_tokens, budget_max_turns,
                            budget_used_tokens, budget_turns_used)
                         VALUES(?1, ?2, '', ?3, 'running', ?4, ?5, 0, 0)",
                        rusqlite::params![
                            str_field(p, "agent_id")?,
                            str_field(p, "session_id")?,
                            serde_json::to_string(&model_chain)
                                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
                            budget.as_ref().map(|b| b.max_tokens as i64),
                            budget.as_ref().map(|b| b.max_turns as i64),
                        ],
                    )?;
                    Ok(1)
                }
                EventType::AgentWaitingModel => {
                    conn.execute(
                        "UPDATE agents SET state='waiting_model' WHERE id=?1",
                        [str_field(p, "agent_id")?],
                    )?;
                    Ok(1)
                }
                EventType::AgentCompleted => {
                    conn.execute(
                        "UPDATE agents SET state='running',
                            budget_turns_used = budget_turns_used + 1
                         WHERE id=?1",
                        [str_field(p, "agent_id")?],
                    )?;
                    Ok(1)
                }
                EventType::AgentFailed => {
                    conn.execute(
                        "UPDATE agents SET state='failed' WHERE id=?1",
                        [str_field(p, "agent_id")?],
                    )?;
                    if let Some(op) = opt_str_field(p, "operation_id")? {
                        conn.execute(
                            "UPDATE operations SET error_code=?2 WHERE id=?1",
                            rusqlite::params![op, opt_str_field(p, "error_code")?],
                        )?;
                    }
                    Ok(1)
                }
                EventType::AgentInterrupted => {
                    conn.execute(
                        "UPDATE agents SET state='interrupted' WHERE id=?1",
                        [str_field(p, "agent_id")?],
                    )?;
                    Ok(1)
                }
                EventType::AgentResumed => {
                    conn.execute(
                        "UPDATE agents SET state='running' WHERE id=?1",
                        [str_field(p, "agent_id")?],
                    )?;
                    Ok(1)
                }
                EventType::AgentCancelled => {
                    conn.execute(
                        "UPDATE agents SET state='stopped' WHERE id=?1",
                        [str_field(p, "agent_id")?],
                    )?;
                    Ok(1)
                }
                EventType::AgentTurnStarted => {
                    // OR IGNORE:input_content 在事件之外受保护写入,重放不得覆盖丢失
                    conn.execute(
                        "INSERT OR IGNORE INTO operations(
                            id, session_id, agent_id, request_id, state, turn_index, created_at)
                         VALUES(?1, ?2, ?3, NULL, 'running', ?4, ?5)",
                        rusqlite::params![
                            str_field(p, "operation_id")?,
                            event.session_id.as_ref().map(|i| i.as_str()).unwrap_or(""),
                            str_field(p, "agent_id")?,
                            p["turn_index"].as_i64().unwrap_or(0),
                            ts,
                        ],
                    )?;
                    Ok(1)
                }
                EventType::ModelInvocationCompleted => {
                    let delta =
                        p["usage_in"].as_i64().unwrap_or(0) + p["usage_out"].as_i64().unwrap_or(0);
                    conn.execute(
                        "UPDATE agents SET budget_used_tokens = budget_used_tokens + ?2
                         WHERE id=?1",
                        rusqlite::params![str_field(p, "agent_id")?, delta],
                    )?;
                    Ok(1)
                }
                EventType::OperationStateChanged => {
                    let to = str_field(p, "to")?;
                    let terminal = matches!(
                        to.as_str(),
                        "succeeded" | "failed" | "cancelled" | "timeout" | "outcome_unknown"
                    );
                    conn.execute(
                        "UPDATE operations SET state=?2,
                            completed_at = CASE WHEN ?3 THEN ?4 ELSE completed_at END
                         WHERE id=?1",
                        rusqlite::params![str_field(p, "operation_id")?, to, terminal, ts,],
                    )?;
                    Ok(1)
                }
                // M5 增发:task.* 物化(ADR-0004:Task 规范状态归 L2,行自事件
                // 可重建键列;完整载荷由核心直接落行,重建时键列正确即可)。
                EventType::TaskCreated => {
                    // INSERT OR IGNORE:完整载荷行已由核心先落(直接落行先于
                    // 事件物化),此处仅兜底事件重建路径(重建载荷为键字段形态)。
                    let parent = opt_str_field(p, "parent_task_id")?;
                    conn.execute(
                        "INSERT OR IGNORE INTO tasks(id, title, state, created_by, task_epoch,
                                                    payload, created_at, updated_at,
                                                    parent_task_id, delegation_depth)
                         VALUES(?1, ?2, 'created', ?3, 1, ?4, ?5, ?5, ?6, ?7)",
                        rusqlite::params![
                            str_field(p, "task_id")?,
                            str_field(p, "title")?,
                            str_field(p, "created_by")?,
                            format!(r#"{{"task_id":"{}"}}"#, str_field(p, "task_id")?),
                            ts,
                            parent,
                            match &parent {
                                Some(_) => 1i64,
                                None => 0i64,
                            },
                        ],
                    )?;
                    Ok(1)
                }
                EventType::TaskStateChanged => {
                    conn.execute(
                        "UPDATE tasks SET state=?3, task_epoch=?4, updated_at=?5 WHERE id=?1
                         AND state=?2",
                        rusqlite::params![
                            str_field(p, "task_id")?,
                            str_field(p, "from")?,
                            str_field(p, "to")?,
                            p["task_epoch"].as_i64().unwrap_or(1),
                            ts,
                        ],
                    )?;
                    Ok(1)
                }
                EventType::TaskMemberAdded => {
                    conn.execute(
                        "INSERT OR IGNORE INTO task_members(task_id, agent_id, role, grant_id,
                                                            joined_seq)
                         VALUES(?1, ?2, ?3, ?4, 0)",
                        rusqlite::params![
                            str_field(p, "task_id")?,
                            str_field(p, "agent_id")?,
                            str_field(p, "role")?,
                            opt_str_field(p, "grant_id")?,
                        ],
                    )?;
                    Ok(1)
                }
                // 非状态类事件:合法 no-op
                _ => Ok(0),
            }
        })();
        match result {
            Ok(_) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(crate::error::StoreError::Sql(e));
            }
        }
        Ok(())
    }
}

fn str_field(p: &serde_json::Value, key: &str) -> rusqlite::Result<String> {
    p[key].as_str().map(|s| s.to_string()).ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(0, key.to_string(), rusqlite::types::Type::Null)
    })
}

fn opt_str_field(p: &serde_json::Value, key: &str) -> rusqlite::Result<Option<String>> {
    Ok(p[key].as_str().map(|s| s.to_string()))
}
