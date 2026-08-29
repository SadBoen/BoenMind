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
                    conn.execute(
                        "INSERT OR REPLACE INTO sessions(id, state, agent_id, created_at)
                         VALUES(?1, 'active', ?2, ?3)",
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
                    conn.execute(
                        "INSERT OR REPLACE INTO operations(
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
