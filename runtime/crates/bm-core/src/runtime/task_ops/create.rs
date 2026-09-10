//! 自 task_ops.rs 机械移入(内容零改动)。
use super::*;

pub(crate) fn handle_task_create(
    w: &mut World,
    _request_id: BmId,
    params: wire::TaskCreateParams,
) -> CoreResult<wire::TaskCreateResult> {
    w.gate_writes("创建 Task")?;
    // 协调权门禁(M5.1):task.create 是 Butler 的 mutation 协调动词——
    // bootstrap Grant 被撤销后此命令拒绝(重授走审批,撤销不影响既有 Task)
    if w.grants
        .active_for(
            crate::butler::BUTLER_PRINCIPAL,
            "task.create",
            w.config.clock.now(),
        )
        .is_empty()
    {
        return Err(CoreError::Semantic(
            ErrorCode::PermissionDenied,
            "Butler 协调权(task.create)已被撤销,重授需用户批准".into(),
        ));
    }
    // Task 授权校验:动词 ⊆ Butler 协调清单(上界);mutation 动词必须显式
    // 标记 klass=mutation(ADR-0002 §11.2 二分;领域动词不可授权)
    let authorization = params.authorization.unwrap_or_default();
    for entry in &authorization {
        let Some(class) = crate::butler::verb_class(&entry.verb) else {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!("非协调动词不可授权: {}", entry.verb),
            ));
        };
        let ok = match class {
            crate::butler::CoordinationClass::Mutation => {
                entry.klass.as_deref() == Some("mutation")
            }
            crate::butler::CoordinationClass::Safe => {
                matches!(entry.klass.as_deref(), None | Some("safe"))
            }
        };
        if !ok {
            return Err(CoreError::Semantic(
                ErrorCode::ValidationFailed,
                format!(
                    "授权分级与动词默认分级不一致: {}(mutation 动词必须显式 klass=mutation)",
                    entry.verb
                ),
            ));
        }
    }
    let now = w.config.clock.now();
    // wire task.create 恒为根 Task(委派走 spawn_subtask 内核 API,M6 规格 §8-4)
    let mut task = crate::task::Task::create(
        &*w.config.id_gen,
        params.title,
        params.goal,
        authorization,
        params.budget,
        params.deadline,
        None,
        0,
        now,
    );
    // task.created(事实)+ created→running(task_started)
    w.emit(
        EventType::TaskCreated,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task.id.as_str(),
            "title": task.title,
            "created_by": task.created_by,
            "parent_task_id": null,
        }),
    );
    let (from, to, guard) = task
        .transition(bm_contract::states::TaskState::Running, None, now)
        .expect("created→running 是迁移表边");
    w.emit(
        EventType::TaskStateChanged,
        None,
        None,
        None,
        serde_json::json!({
            "task_id": task.id.as_str(),
            "from": from.as_str(),
            "to": to.as_str(),
            "reason_code": guard,
            "task_epoch": task.task_epoch,
        }),
    );

    // M5-T4/T5:协调链自举——三方交集物化为 task:<id> Grant(ADR-0002 §11.3)
    // + Coordinator/单 Worker 成员事实(GT-03 场景 A2 形态)
    {
        let task_id_str = task.id.as_str().to_string();
        // M6:per-task principal 命名空间(跨 Task 访问在 Grant 查表层结构性不命中)
        let coord_aud = crate::team::coord_principal(&task_id_str);
        let worker_aud = crate::team::worker_principal(&task_id_str);
        // 分阶段作用域:butler 上界查证闭包借用 w.grants,产出后即释放
        let (coord_grants, worker_grants) = {
            let mut butler_lookup = |verb: &str| {
                w.grants
                    .active_for(crate::butler::BUTLER_PRINCIPAL, verb, now)
                    .into_iter()
                    .next()
            };
            crate::coordinator::intersection_grants(
                &*w.config.id_gen,
                &task_id_str,
                &coord_aud,
                &worker_aud,
                &task.authorization,
                now,
                &mut butler_lookup,
            )
        };
        for g in coord_grants.iter().chain(worker_grants.iter()) {
            w.grants.record(g.clone());
            persist_grant(w, &g.grant_id);
            w.emit_grant_created(g, None, None);
        }
        // 成员事实:Coordinator(必有)+ Worker(仅当任务声明了能力资源)
        let member_event = |w: &mut World, agent_id: &str, role: &str, grant_id: Option<&str>| {
            w.emit(
                EventType::TaskMemberAdded,
                None,
                None,
                None,
                serde_json::json!({
                    "task_id": task_id_str,
                    "agent_id": agent_id,
                    "role": role,
                    "grant_id": grant_id,
                }),
            )
        };
        let coord_member_id = w.config.id_gen.next_id("agent");
        let coord_grant_id = coord_grants
            .iter()
            .find(|g| g.action == "agent.spawn")
            .or_else(|| coord_grants.first())
            .map(|g| g.grant_id.clone());
        let ev = member_event(
            w,
            coord_member_id.as_str(),
            "coordinator",
            coord_grant_id.as_deref(),
        );
        task.add_member(crate::task::TaskMember {
            agent_id: coord_member_id,
            role: crate::task::MemberRole::Coordinator,
            grant_id: coord_grant_id,
            joined_seq: ev.event_seq,
        });
        if !worker_grants.is_empty() {
            let worker_member_id = w.config.id_gen.next_id("agent");
            let worker_grant_id = worker_grants[0].grant_id.clone();
            let ev = member_event(
                w,
                worker_member_id.as_str(),
                "worker",
                Some(worker_grant_id.as_str()),
            );
            task.add_member(crate::task::TaskMember {
                agent_id: worker_member_id,
                role: crate::task::MemberRole::Worker,
                grant_id: Some(worker_grant_id),
                joined_seq: ev.event_seq,
            });
        }
    }
    persist_task(w, &task);
    let result = wire::TaskCreateResult {
        task_id: task.id.clone(),
        state: task.state,
        created_at: task.created_at.clone(),
    };
    w.tasks.insert(task.id.clone(), task);
    Ok(result)
}
