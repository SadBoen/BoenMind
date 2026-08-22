//! M3 收尾 RPC 方法：goal.*、subagent.*、session.{attachment,updateQueue}、
//! settings.openDocument、host.openPath、agentPreset.{select,read,copy,openDocument,remove}。
//!
//! 分层纪律（架构澄清，与 subagent 同逻辑）：goal wire 契约在 web-server、
//! 自动续跑语义归 goal 插件（M3.5）；subagent wire 在 web-server、执行走 team
//! 插件进程、内核不动。无插件装配时的诚实行为：不装死、不假成功。

use serde_json::{json, Value};

use crate::api::{AppState, GoalRecord};
use crate::rpc::{err, err_with_details, ok};

// ---------- session.attachment / session.updateQueue ----------

/// session.attachment（台账 §2）：仅当会话日志引用该 attachmentId 才回
/// `{attachment: ImageAttachmentRef, data: string}`；无引用 → `attachment-error {reason}`。
pub fn session_attachment(state: &AppState, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let Some(attachment_id) = payload.get("attachmentId").and_then(Value::as_str) else {
        return err("bad-request", "missing attachmentId");
    };
    // 当前内核无附件事件——日志引用表（attachmentId → 会话）查询。
    let referenced = state
        .attachments
        .lock()
        .unwrap()
        .get(session_id)
        .map(|ids| ids.contains(&attachment_id.to_string()))
        .unwrap_or(false);
    if !referenced {
        return err_with_details(
            "attachment-error",
            "attachment not referenced by this session's log",
            json!({ "reason": "not-found" }),
        );
    }
    // 引用存在但无存储实现：诚实报错（不假成功）。
    err_with_details(
        "attachment-error",
        "attachment data store not available",
        json!({ "reason": "unavailable" }),
    )
}

/// session.updateQueue（台账 §2）：内核无 queue 语义（prompt 排队 = agent-busy 拒绝）。
/// wire 契约对齐：`{accepted:true}`（空操作）；未知 itemId → `queue-item-not-found`。
pub fn session_update_queue(state: &AppState, payload: Value) -> Value {
    let _session_id = payload.get("sessionId").and_then(Value::as_str);
    let Some(item_id) = payload.get("itemId").and_then(Value::as_str) else {
        return err("bad-request", "missing itemId");
    };
    let _ = state;
    // 队列语义挂后：任何 itemId 都不在队列中。
    err_with_details(
        "queue-item-not-found",
        "queue item not found",
        json!({ "itemId": item_id }),
    )
}

// ---------- settings.openDocument / host.openPath ----------

/// settings.openDocument（特权）：`{opened: true}`（无原生文档可开——hasDocument:false 已声明）。
pub fn settings_open_document() -> Value {
    ok(json!({ "opened": true }))
}

/// host.openPath（特权）：path ≥1 → 调 OS 打开；打开失败 → `{opened:false}`。
/// 唯一真实 OS 副作用方法；验证用无害路径（如临时文件）。
/// Windows 经 `cmd /C start ""` 拼接执行——`&|^<>` 等 cmd 元字符会被解释成
/// 命令语法（SEC-003 命令注入面），一律拒绝；Unix 侧 xdg-open 不经过 shell
/// 无此风险，但为跨平台一致也拒绝控制字符。
pub fn host_open_path(payload: Value) -> Value {
    let Some(path) = payload.get("path").and_then(Value::as_str) else {
        return err("bad-request", "missing path");
    };
    if path.is_empty() {
        return err("bad-request", "path must be at least 1 character");
    }
    // cmd 元字符 + 控制字符拒绝（含 \r\n 防头注入）。
    if path
        .chars()
        .any(|c| c.is_control() || "&|^<>%\"".contains(c))
    {
        return err("bad-request", "path contains characters not allowed");
    }
    #[cfg(windows)]
    {
        let opened = std::process::Command::new("cmd")
            .args(["/C", "start", "", path])
            .spawn()
            .map(|_| true)
            .unwrap_or(false);
        ok(json!({ "opened": opened }))
    }
    #[cfg(not(windows))]
    {
        let opened = std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| true)
            .unwrap_or(false);
        ok(json!({ "opened": opened }))
    }
}

// ---------- goal.*（wire 契约在 web-server，语义属 goal 插件；自动续跑 = 插件职责 M3.5） ----------

/// goal.create：objective ≥1 字符，maxGoalRounds 正整数；建并武装目标。
pub fn goal_create(state: &AppState, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let Some(objective) = payload.get("objective").and_then(Value::as_str) else {
        return err("bad-request", "missing objective");
    };
    if objective.trim().is_empty() {
        return err("bad-request", "objective must be at least 1 character");
    }
    let max_goal_rounds = match payload.get("maxGoalRounds") {
        Some(v) => match v.as_u64() {
            Some(n) if n > 0 => n,
            _ => return err("bad-request", "maxGoalRounds must be a positive integer"),
        },
        None => 1,
    };
    let now = chrono::Utc::now().timestamp_millis();
    let id = uuid::Uuid::new_v4().to_string();
    let goal = GoalRecord {
        id: id.clone(),
        revision: 1,
        objective: objective.to_string(),
        phase: "active".to_string(),
        max_goal_rounds,
        rounds_started: 0,
        created_at: now,
        updated_at: now,
    };
    // 投影先算后插：曾 insert 后再取锁 get().unwrap()，并发 goal_clear 可在
    // 窗口内 remove → unwrap panic。
    let projection = goal.projection();
    state.goals.lock().unwrap().insert(session_id.to_string(), goal);
    // 写 'goal' 投影 + 广播 session/projection 帧（无读方法，客户端走投影）。
    state.write_projection(session_id, "goal", projection);
    ok(json!({ "ref": { "id": id, "revision": 1u64 } }))
}

/// 解析 payload 里的 GoalRef（id + revision）。
fn parse_goal_ref(payload: &Value) -> Option<(String, u64)> {
    let id = payload.get("ref")?.get("id")?.as_str()?;
    let revision = payload.get("ref")?.get("revision")?.as_u64()?;
    Some((id.to_string(), revision))
}

/// CAS 守卫的 goal 变更：ref 不匹配 → 逐字错误码。
/// 返回 (session_id, 新 revision)；错误以 Value 返回。
fn goal_cas_apply<F>(state: &AppState, payload: &Value, apply: F) -> Value
where
    F: FnOnce(&mut GoalRecord) -> Result<(), Value>,
{
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let Some((ref_id, revision)) = parse_goal_ref(payload) else {
        return err("bad-request", "missing or invalid ref");
    };
    let mut goals = state.goals.lock().unwrap();
    let Some(goal) = goals.get_mut(session_id) else {
        return err_with_details(
            "goal-not-found",
            "no goal for this session",
            json!({ "sessionId": session_id }),
        );
    };
    if goal.id != ref_id || goal.revision != revision {
        return err_with_details(
            "goal-conflict",
            "goal ref does not match current goal",
            json!({ "sessionId": session_id, "ref": { "id": ref_id, "revision": revision } }),
        );
    }
    if let Err(e) = apply(goal) {
        return e;
    }
    goal.revision += 1;
    goal.updated_at = chrono::Utc::now().timestamp_millis();
    let new_rev = goal.revision;
    let projection = goal.projection();
    drop(goals);
    state.write_projection(session_id, "goal", projection);
    ok(json!({ "ref": { "id": ref_id, "revision": new_rev } }))
}

/// goal.edit：objective 和/或 maxGoalRounds（至少一个）；不改阶段；revision 递增。
pub fn goal_edit(state: &AppState, payload: Value) -> Value {
    let has_objective = payload.get("objective").and_then(Value::as_str).is_some();
    let has_rounds = payload.get("maxGoalRounds").is_some();
    if !has_objective && !has_rounds {
        return err("bad-request", "goal.edit requires objective or maxGoalRounds");
    }
    goal_cas_apply(state, &payload, |goal| {
        if let Some(obj) = payload.get("objective").and_then(Value::as_str) {
            if obj.trim().is_empty() {
                return Err(err("bad-request", "objective must be at least 1 character"));
            }
            goal.objective = obj.to_string();
        }
        if let Some(v) = payload.get("maxGoalRounds") {
            match v.as_u64() {
                Some(n) if n > 0 => goal.max_goal_rounds = n,
                _ => return Err(err("bad-request", "maxGoalRounds must be a positive integer")),
            }
        }
        Ok(())
    })
}

/// goal.pause：停用自动续跑（phase → paused）。
pub fn goal_pause(state: &AppState, payload: Value) -> Value {
    goal_phase_transition(state, payload, "paused")
}

/// goal.resume：恢复武装（phase → active）。
pub fn goal_resume(state: &AppState, payload: Value) -> Value {
    goal_phase_transition(state, payload, "active")
}

/// goal.complete：完成并解除（phase → complete）。
pub fn goal_complete(state: &AppState, payload: Value) -> Value {
    goal_phase_transition(state, payload, "complete")
}

fn goal_phase_transition(state: &AppState, payload: Value, to_phase: &str) -> Value {
    goal_cas_apply(state, &payload, |goal| {
        goal.phase = to_phase.to_string();
        Ok(())
    })
}

/// goal.clear：留墓碑与历史（投影清空为 null）。
pub fn goal_clear(state: &AppState, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let Some((ref_id, revision)) = parse_goal_ref(&payload) else {
        return err("bad-request", "missing or invalid ref");
    };
    let mut goals = state.goals.lock().unwrap();
    let Some(goal) = goals.get(session_id) else {
        return err_with_details(
            "goal-not-found",
            "no goal for this session",
            json!({ "sessionId": session_id }),
        );
    };
    if goal.id != ref_id || goal.revision != revision {
        return err_with_details(
            "goal-conflict",
            "goal ref does not match current goal",
            json!({ "sessionId": session_id, "ref": { "id": ref_id, "revision": revision } }),
        );
    }
    goals.remove(session_id);
    drop(goals);
    // 墓碑：投影置 null（客户端 higher-seq-wins 覆盖到空态）。
    state.write_projection(session_id, "goal", Value::Null);
    ok(json!({ "cleared": true }))
}

// ---------- subagent.*（wire 契约在 web-server，执行走 team 插件进程，内核不动） ----------

/// 父会话是否可用（live 表或持久化日志）。
fn parent_available(state: &AppState, parent_session_id: &str) -> bool {
    if state.sessions.lock().unwrap().contains_key(parent_session_id) {
        return true;
    }
    // 持久化侧检查（同步兜底：web-server 是 async 上下文，这里只查内存快照；
    // live 表覆盖浏览器会话；持久化孤儿由 list_sessions 语义覆盖——简化判据）。
    futures::executor::block_on(async {
        state
            .runtime
            .persist
            .list_sessions()
            .await
            .map(|ids| ids.contains(&parent_session_id.to_string()))
            .unwrap_or(false)
    })
}

/// subagent.list：`{entries, parentAvailable}`——无插件装配时诚实返回 parentAvailable:false。
pub fn subagent_list(state: &AppState, payload: Value) -> Value {
    let Some(parent_session_id) = payload.get("parentSessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing parentSessionId");
    };
    if !parent_available(state, parent_session_id) {
        return err_with_details(
            "subagent-parent-unavailable",
            "parent session is not available",
            json!({ "parentSessionId": parent_session_id }),
        );
    }
    // 无 team 插件装配：parentAvailable:false + 空 entries（前端据此降级显示，不装死）。
    ok(json!({ "entries": [], "parentAvailable": false }))
}

/// subagent.history：无插件装配 → `subagent-parent-unavailable`（诚实，不假成功）。
pub fn subagent_history(state: &AppState, payload: Value) -> Value {
    let Some(parent_session_id) = payload.get("parentSessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing parentSessionId");
    };
    let _child = payload.get("childSessionId").and_then(Value::as_str);
    let _mode = payload.get("mode").and_then(Value::as_str);
    if !parent_available(state, parent_session_id) {
        return err_with_details(
            "subagent-parent-unavailable",
            "parent session is not available",
            json!({ "parentSessionId": parent_session_id }),
        );
    }
    err_with_details(
        "subagent-parent-unavailable",
        "subagent execution plugin is not assembled",
        json!({ "parentSessionId": parent_session_id }),
    )
}

/// subagent.prompt：同空态诚实路径。
pub fn subagent_prompt(state: &AppState, payload: Value) -> Value {
    let Some(parent_session_id) = payload.get("parentSessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing parentSessionId");
    };
    let _content = payload.get("content");
    if !parent_available(state, parent_session_id) {
        return err_with_details(
            "subagent-parent-unavailable",
            "parent session is not available",
            json!({ "parentSessionId": parent_session_id }),
        );
    }
    err_with_details(
        "subagent-parent-unavailable",
        "subagent execution plugin is not assembled",
        json!({ "parentSessionId": parent_session_id }),
    )
}

/// subagent.interrupt：无插件装配 → 空态诚实错误。
pub fn subagent_interrupt(state: &AppState, payload: Value) -> Value {
    let Some(parent_session_id) = payload.get("parentSessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing parentSessionId");
    };
    if !parent_available(state, parent_session_id) {
        return err_with_details(
            "subagent-parent-unavailable",
            "parent session is not available",
            json!({ "parentSessionId": parent_session_id }),
        );
    }
    err_with_details(
        "subagent-parent-unavailable",
        "subagent execution plugin is not assembled",
        json!({ "parentSessionId": parent_session_id }),
    )
}

// ---------- agentPreset.*（剩余 5 法：select/read/copy/openDocument/remove） ----------

/// agentPreset.select：仅 blank 会话可换（台账：已开跑 → agent-preset-locked）。
/// 无 authoring 预设清单 → agent-preset-not-found。
pub fn agent_preset_select(state: &AppState, payload: Value) -> Value {
    let Some(session_id) = payload.get("sessionId").and_then(Value::as_str) else {
        return err("bad-request", "missing sessionId");
    };
    let Some(agent_preset) = payload.get("agentPreset").and_then(Value::as_str) else {
        return err("bad-request", "missing agentPreset");
    };
    let sessions = state.sessions.lock().unwrap();
    let Some(h) = sessions.get(session_id) else {
        return err("session-not-found", format!("session {session_id} not found"));
    };
    if !h.blank {
        return err_with_details(
            "agent-preset-locked",
            "session has already started; its agent preset is fixed",
            json!({ "sessionId": session_id, "agentPreset": agent_preset }),
        );
    }
    drop(sessions);
    // 无 authoring 预设：请求的 preset 不在清单 → not-found（台账：available[]）。
    err_with_details(
        "agent-preset-not-found",
        "this deployment composes no agent presets",
        json!({ "agentPreset": agent_preset, "available": [] }),
    )
}

/// agentPreset.read（特权）：无 authoring 预设 → 逐字错误码。
pub fn agent_preset_read(state: &AppState, payload: Value) -> Value {
    let _ = state;
    let Some(agent_preset) = payload.get("agentPreset").and_then(Value::as_str) else {
        return err("bad-request", "missing agentPreset");
    };
    err_with_details(
        "agent-preset-not-found",
        "this deployment composes no agent presets",
        json!({ "agentPreset": agent_preset, "available": [] }),
    )
}

/// agentPreset.copy（特权）：无 authoring 预设 → 逐字错误码。
pub fn agent_preset_copy(state: &AppState, payload: Value) -> Value {
    let _ = state;
    let Some(agent_preset) = payload.get("agentPreset").and_then(Value::as_str) else {
        return err("bad-request", "missing agentPreset");
    };
    err_with_details(
        "agent-preset-not-found",
        "this deployment composes no agent presets",
        json!({ "agentPreset": agent_preset, "available": [] }),
    )
}

/// agentPreset.openDocument（特权）：无 authoring 预设 → 逐字错误码。
pub fn agent_preset_open_document(state: &AppState, payload: Value) -> Value {
    let _ = state;
    let Some(agent_preset) = payload.get("agentPreset").and_then(Value::as_str) else {
        return err("bad-request", "missing agentPreset");
    };
    err_with_details(
        "agent-preset-not-found",
        "this deployment composes no agent presets",
        json!({ "agentPreset": agent_preset, "available": [] }),
    )
}

/// agentPreset.remove（特权）：无 authoring 预设 → 逐字错误码。
pub fn agent_preset_remove(state: &AppState, payload: Value) -> Value {
    let _ = state;
    let Some(agent_preset) = payload.get("agentPreset").and_then(Value::as_str) else {
        return err("bad-request", "missing agentPreset");
    };
    err_with_details(
        "agent-preset-not-found",
        "this deployment composes no agent presets",
        json!({ "agentPreset": agent_preset, "available": [] }),
    )
}
