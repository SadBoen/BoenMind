//! 自 task_ops.rs 机械移入(内容零改动)。
use super::*;

pub(crate) const AUTORUN_DONE_MARK: &str = "[[AUTORUN_DONE]]";

pub(crate) struct AutorunState {
    pub(crate) session_id: BmId,
    pub(crate) agent_id: BmId,
    pub(crate) turn: u64,
    pub(crate) max_turns: u64,
    pub(crate) in_flight_op: Option<BmId>,
    pub(crate) last_content: Option<String>,
    pub(crate) prev_content: Option<String>,
    pub(crate) repeats: u64,
}
pub(crate) fn autorun_instruction(goal: &str, last: Option<&str>) -> String {
    let mut s = format!("任务目标:{goal}");
    s.push_str("\n请自主推进一步。");
    if let Some(l) = last {
        s.push_str(&format!("\n上一步输出:{l}"));
    }
    s.push_str("\n若任务已全部完成,本条回复以 [[AUTORUN_DONE]] 开头,其后接结果摘要。");
    s
}
pub(crate) fn handle_task_autorun_start(
    w: &mut World,
    request_id: BmId,
    params: bm_contract::wire::TaskAutorunParams,
) -> CoreResult<bm_contract::wire::TaskAutorunResult> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝自主环受理".into(),
        ));
    }
    let task_state = {
        let Some(task) = w.tasks.get(&params.task_id) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("Task 不存在: {}", params.task_id.as_str()),
            ));
        };
        task.state
    };
    if !matches!(task_state, bm_contract::states::TaskState::Running) {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            format!("Task 非运行态,拒绝自主环: {task_state:?}"),
        ));
    }
    if w.autorun.contains_key(&params.task_id) {
        return Err(CoreError::Semantic(
            ErrorCode::ValidationFailed,
            "该 Task 已在自主推进中".into(),
        ));
    }
    // 工作链来源:系统主体(实际部署)/ 任一既有代理 / 兜底(v0;回看裁决)
    let model_chain = w
        .agents
        .get(&w.system_agent)
        .map(|a| a.model_chain.clone())
        .or_else(|| w.agents.values().next().map(|a| a.model_chain.clone()))
        .unwrap_or_else(|| vec!["model.default".to_string()]);
    let created = handle_session_create(
        w,
        request_id,
        bm_contract::wire::SessionCreateParams {
            agent: bm_contract::wire::AgentSpec {
                name: format!("autorun-{}", params.task_id.as_str()),
                model_chain,
                budget: None,
                system_prompt: None,
                allowed_tools: None,
                workspace_id: None,
            },
        },
    )?;
    let max_turns = params
        .max_turns
        .unwrap_or(w.config.limits.get().autorun_default_max_turns as u64)
        .clamp(1, 50);
    w.autorun.insert(
        params.task_id.clone(),
        AutorunState {
            session_id: created.session_id.clone(),
            agent_id: created.agent_id.clone(),
            turn: 0,
            max_turns,
            in_flight_op: None,
            last_content: None,
            prev_content: None,
            repeats: 0,
        },
    );
    emit_autorun(w, &params.task_id, "started", 0, None);
    let goal = w.tasks[&params.task_id].goal.clone();
    let (session_id, agent_id) = {
        let st = &w.autorun[&params.task_id];
        (st.session_id.clone(), st.agent_id.clone())
    };
    let sent = handle_send_input(
        w,
        w.config.id_gen.next_id("req"),
        bm_contract::wire::SendInputParams {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            content: autorun_instruction(&goal, None),
            model_override: None,
            workspace_override: None,
            input_trust: bm_contract::wire::InputTrust::Trusted,
        },
    )?;
    // 防御化(原 expect):send_input 期间 autorun 态被并发清场的健壮性兜底
    let Some(st) = w.autorun.get_mut(&params.task_id) else {
        return Ok(bm_contract::wire::TaskAutorunResult {
            session_id,
            agent_id,
            accepted: false,
        });
    };
    st.in_flight_op = Some(sent.operation_id);
    Ok(bm_contract::wire::TaskAutorunResult {
        session_id,
        agent_id,
        accepted: true,
    })
}
pub(crate) fn autorun_note_completed(w: &mut World, op_id: &BmId, content: &str) {
    for st in w.autorun.values_mut() {
        if st.in_flight_op.as_ref() == Some(op_id) {
            st.last_content = Some(content.to_string());
            st.turn += 1;
        }
    }
}
pub(crate) fn autorun_block(w: &mut World, task_id: &BmId, reason: &str) {
    let now = w.config.clock.now();
    let Some(task) = w.tasks.get_mut(task_id) else {
        return;
    };
    let epoch = task.task_epoch;
    if let Ok((from, to, _g)) = task.transition(bm_contract::states::TaskState::Blocked, None, now)
    {
        w.emit(
            EventType::TaskStateChanged,
            None,
            None,
            None,
            serde_json::json!({
                "task_id": task_id.as_str(),
                "from": from.as_str(),
                "to": to.as_str(),
                "reason_code": reason,
                "task_epoch": epoch,
            }),
        );
        let snapshot = w.tasks[task_id].clone();
        persist_task(w, &snapshot);
    }
}
pub(crate) fn autorun_pump(w: &mut World, op_id: &BmId) {
    let Some(task_id) = w
        .autorun
        .iter()
        .find(|(_, s)| s.in_flight_op.as_ref() == Some(op_id))
        .map(|(k, _)| k.clone())
    else {
        return;
    };
    let receipt_state = w.operations.get(op_id).map(|o| o.state);
    match receipt_state {
        Some(bm_contract::states::OperationState::Succeeded) => {}
        Some(bm_contract::states::OperationState::Cancelled) => {
            emit_autorun(w, &task_id, "finished", 0, Some("cancelled"));
            w.autorun.remove(&task_id);
            return;
        }
        Some(_) => {
            emit_autorun(w, &task_id, "finished", 0, Some("model_failed"));
            w.autorun.remove(&task_id);
            return;
        }
        None => return,
    }
    let (turn, content) = {
        let st = &w.autorun[&task_id];
        (st.turn, st.last_content.clone().unwrap_or_default())
    };
    // 完成哨兵
    if content.trim_start().starts_with(AUTORUN_DONE_MARK) {
        let summary = content.trim_start()[AUTORUN_DONE_MARK.len()..]
            .trim()
            .to_string();
        let _ = handle_task_report_completion(w, task_id.clone(), summary, Some(op_id.clone()));
        emit_autorun(w, &task_id, "finished", turn, Some("done"));
        w.autorun.remove(&task_id);
        return;
    }
    // 停滞:连续两轮完全相同输出(防御化(原 expect):条目被并发清场则放弃推进)
    let Some(st) = w.autorun.get_mut(&task_id) else {
        return;
    };
    let same = st.prev_content.as_deref() == Some(content.as_str());
    if same {
        st.repeats += 1;
    } else {
        st.repeats = 1;
        st.prev_content = Some(content.clone());
    }
    if w.autorun[&task_id].repeats >= 2 {
        autorun_block(w, &task_id, "stalled");
        emit_autorun(w, &task_id, "finished", turn, Some("stalled"));
        w.autorun.remove(&task_id);
        return;
    }
    // 轮数上限
    if turn >= w.autorun[&task_id].max_turns {
        autorun_block(w, &task_id, "max_turns");
        emit_autorun(w, &task_id, "finished", turn, Some("max_turns"));
        w.autorun.remove(&task_id);
        return;
    }
    // 外部状态复检(暂停/停止即时生效)
    let task_state = w.tasks[&task_id].state;
    if !matches!(task_state, bm_contract::states::TaskState::Running) {
        emit_autorun(w, &task_id, "finished", turn, Some(task_state.as_str()));
        w.autorun.remove(&task_id);
        return;
    }
    // 继续下一步
    let (goal, session_id, agent_id) = {
        let st = &w.autorun[&task_id];
        (
            w.tasks[&task_id].goal.clone(),
            st.session_id.clone(),
            st.agent_id.clone(),
        )
    };
    let sent = handle_send_input(
        w,
        w.config.id_gen.next_id("req"),
        bm_contract::wire::SendInputParams {
            session_id,
            agent_id,
            content: autorun_instruction(&goal, Some(&content)),
            model_override: None,
            workspace_override: None,
            input_trust: bm_contract::wire::InputTrust::Trusted,
        },
    );
    match sent {
        Ok(r) => {
            emit_autorun(w, &task_id, "turn_completed", turn, None);
            // 防御化(原 expect):send_input 期间条目被并发清场则放弃回填
            if let Some(st) = w.autorun.get_mut(&task_id) {
                st.in_flight_op = Some(r.operation_id);
            }
        }
        Err(_) => {
            emit_autorun(w, &task_id, "finished", turn, Some("send_failed"));
            w.autorun.remove(&task_id);
        }
    }
}
