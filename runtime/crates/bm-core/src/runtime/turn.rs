//! 回合执行与能力异步引擎(自 runtime.rs 机械移入)。
//!
//! 机械拆分产物:行为零变化,条目与行序保持原样(见审计台账 E3-1/L-08)。

use super::*;

/// turn 模型调用的审计留档(M7 S1)。
pub(crate) struct ModelCallAudit {
    pub(crate) call_id: BmId,
    pub(crate) epoch: u64,
    pub(crate) instance_id: String,
    pub(crate) principal: String,
}

/// 异步能力调用的在途留档(M7 S4):spawn 时捕获,完成回流时落定。
pub(crate) struct AsyncCallMeta {
    pub(crate) capability: String,
    pub(crate) principal: String,
    pub(crate) call_id: BmId,
    pub(crate) epoch: u64,
    pub(crate) instance_id: String,
    pub(crate) key_hash: Option<String>,
    pub(crate) is_side_effect: bool,
    pub(crate) output_schema: String,
    pub(crate) grant_id: Option<String>,
}

/// 待审批的能力调用载荷(approval 对象只存摘要,重放执行需要原 args)。
pub(crate) struct PendingCapabilityCall {
    pub(crate) op_id: BmId,
    pub(crate) capability: String,
    pub(crate) args: serde_json::Value,
    pub(crate) idempotency_key: Option<String>,
    /// 调用方身份(M5 双路径:surface / worker;审批重放归因一致)
    pub(crate) principal: String,
    pub(crate) trust: DataTrust,
}

pub(crate) fn spawn_turn(
    w: &mut World,
    agent: &Agent,
    operation_id: &BmId,
    content: String,
    model_override: Option<String>,
) {
    // W6:回合级模型覆盖(对话热切换)优先——给出则本回合降级链整体
    // 替换为单元素(工具轮/重试同回合同模型);缺省沿用 agent 烤入链。
    let chain: Vec<String> = match model_override {
        Some(m) if !m.trim().is_empty() => vec![m],
        _ => agent.model_chain.clone(),
    };
    // M7 S1:模型调用过 Broker(M4 §5.8 豁免撤销;ADR-0010)。
    // 授权走 Grant 台账:agent 创建即授 model.invoke 永续 Grant,可经
    // grant.revoke 收回(ADR-0006 权力显式化)。不走信任面——内容链构造层
    // 拒绝 trusted(基线 §4.5),而模型调用是回合机器的固定动作。
    let model_call_audit = {
        let ctx = CallContext::content_chain(
            &format!("agent:{}", agent.id.as_str()),
            DataTrust::Untrusted,
        )
        .expect("内容链不得声称 trusted(此处传 untrusted,构造恒成功)");
        let principal = ctx.principal.clone();
        let decision = {
            let broker = Broker::new(
                &w.registry,
                &mut w.grants,
                &*w.config.clock,
                &*w.config.id_gen,
            );
            broker.decide(
                &ctx,
                "model.invoke",
                &serde_json::json!({
                    "model_id": chain.first().cloned().unwrap_or_default()
                }),
            )
        };
        match decision {
            Decision::Allowed { .. } => {
                let (epoch, instance_id) = w
                    .registry
                    .binding_of("model.invoke")
                    .map(|b| (b.epoch, b.provider_instance_id.clone()))
                    .unwrap_or((0, "n/a".to_string()));
                Some(ModelCallAudit {
                    call_id: w.config.id_gen.next_id("call"),
                    epoch,
                    instance_id,
                    principal,
                })
            }
            _ => None,
        }
    };
    let Some(model_call_audit) = model_call_audit else {
        w.fail_turn(
            operation_id,
            ErrorCode::Internal,
            "模型调用权未授予或已收回".into(),
        );
        return;
    };
    w.model_call_audit
        .insert(operation_id.clone(), model_call_audit);

    // M7 S5:模型连接器熔断门——冷却期内快速失败(不触连接器);
    // 冷却已过即本次放行(半开探测,成败都由 TurnEvent 回账)。
    {
        let provider = w.config.connector.provider();
        let now = w.config.clock.now();
        let blocked = w
            .provider_health
            .get(provider)
            .map(|h| {
                h.status == "unavailable" && h.cooldown_until.map(|t| now < t).unwrap_or(false)
            })
            .unwrap_or(false);
        if blocked {
            w.fail_turn(
                operation_id,
                ErrorCode::Unavailable,
                "模型 Provider 熔断冷却中,请稍后重试".into(),
            );
            return;
        }
    }

    let cancel = CancellationToken::new();
    w.in_flight.insert(operation_id.clone(), cancel.clone());

    let connector = w.config.connector.clone();
    let clock = w.config.clock.clone();
    let agent_id = agent.id.clone();
    let remaining = agent.budget.remaining_tokens();
    let max_attempts = w
        .config
        .max_attempts
        .unwrap_or_else(|| chain.len().min(3) as u32)
        .clamp(1, 3);
    let timeout_secs = w.config.turn_timeout_secs;
    let tx = w.tx.clone();
    let op_id = operation_id.clone();
    let streaming = w.config.model_streaming;
    // W4b 对话工具闭环升级:全部 chat 能力(直通+审批类)均暴露给模型;
    // W4b 角色 prompt:会话级指定优先(会话创建时烤入的完整提示词,含技能);
    // 否则每回合现读 roles.json+skills.json 组装(设置页保存即热生效)。
    // 组装逻辑唯一入口 = bm-core::roles::compose_role_prompt(两条路径同口径)。
    let chat_tools: Vec<(String, serde_json::Value, bool)> = w.registry.chat_tools();
    let role_prompt: Option<String> = agent
        .system_prompt
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            w.config
                .data_dir
                .as_ref()
                .and_then(|d| crate::roles::compose_role_prompt(d, None))
        });
    let request_id = w.operations.get(operation_id).map(|o| o.request_id.clone());
    // W5:会话对话台账快照(历史回喂)+ 上下文快照日志句柄 + 回合序号。
    let session_id: Option<BmId> = w.operations.get(operation_id).map(|o| o.session_id.clone());
    let turn_index = w
        .operations
        .get(operation_id)
        .map(|o| o.turn_index)
        .unwrap_or(0);
    let history: Vec<(String, String)> = session_id
        .as_ref()
        .and_then(|sid| w.session_chats.get(sid).cloned())
        .unwrap_or_default();
    // 遗忘轮数 = 会话累计成功回合 − 台账现存活(台账受 20 轮/24K 字符双上限
    // 裁剪)。0 = 历史完整;>0 = 最早若干轮已被丢弃,透视面板如实告警。
    let alive = history.len() as u64;
    let accounted = session_id
        .as_ref()
        .and_then(|sid| w.session_turn_totals.get(sid).copied())
        .unwrap_or(0);
    let evicted_turns: u64 = evicted_turns(accounted, alive);
    // W8(ADR-0018):会话绑定的工作目录回合级注入——追加到 system prompt,
    // 切换工作区下一条消息即生效;注册表缺条目/目录被删时静默降级不注入。
    let workspace_note: Option<String> = session_id.as_ref().and_then(|sid| {
        let wid = w.sessions.get(sid)?.workspace_id.clone()?;
        let ws = crate::workspace::resolve(w.config.data_dir.as_ref()?, &wid)?;
        Some(format!(
            "[工作目录] 本对话的工作目录:{}(用户提到的相对路径与文件均相对此目录)",
            ws.path
        ))
    });
    let role_prompt = match (role_prompt, workspace_note) {
        (Some(sp), Some(note)) => Some(format!("{sp}\n\n{note}")),
        (None, Some(note)) => Some(note),
        (other, None) => other,
    };
    let ctx_log = w.ctx_log.clone();

    tokio::spawn(async move {
        // W4:messages 含角色 prompt + 历史回合 + 本轮输入;tools=直通工具
        // (OpenAI function 格式,capability 名的点映射为双下划线);工具结果
        // 经 CapabilityCall 回核心循环执行(Broker 裁决/审计管道原样),轮询
        // operations 至终态取结果回喂模型。工具轮上限 5,防循环失控。
        const MAX_TOOL_ROUNDS: u32 = 5;
        let mut messages: Vec<Message> = Vec::new();
        if let Some(sp) = &role_prompt {
            messages.push(Message {
                role: Role::System,
                content: sp.clone(),
            });
        }
        // W5(2026-09-02 用户反馈轮):历史回合回喂。此前每轮从零组装,
        // 模型对同会话前情失忆(W1 合同口径「历史由 runtime 侧维护」的实现
        // 缺口);台账在回合成功落定时经 Cmd::RememberTurn 回写。
        for (u, a) in &history {
            messages.push(Message {
                role: Role::User,
                content: u.clone(),
            });
            messages.push(Message {
                role: Role::Assistant,
                content: a.clone(),
            });
        }
        let user_input = content.clone();
        messages.push(Message {
            role: Role::User,
            content: content.clone(),
        });
        let mut name_to_cap: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let tools_json: Vec<serde_json::Value> = chat_tools
            .iter()
            .map(|(cap, schema, needs_approval)| {
                // OpenAI function.name 规范要求 ^[a-zA-Z0-9_-]{1,64}$，不能有点号。
                // 采用单下划线转义(fs.read -> fs_read; mcp.foo.bar -> mcp_foo_bar)，
                // 彻底告别别扭的双下划线；调用返回时由 name_to_cap 原样映射回内核能力名。
                let openai_name = cap.replace('.', "_");
                name_to_cap.insert(openai_name.clone(), cap.clone());
                let desc = match cap.as_str() {
                    "fs.search" => "在工作区中搜索文件内容或文件路径。支持两种模式: 1) mode='content'(默认，类似 ripgrep/grep 在文件正文中检索代码或文本); 2) mode='files'(类似 find/glob，仅按文件名或路径通配符查找文件位置，如 query='*README*' 或 'README|readme')。可传 path_pattern 过滤待查文件(如 '*.rs')。".to_string(),
                    "fs.read" => "读取工作区中指定文件的文本内容。返回带 1-based 行号的格式(类似 cat -n)，支持 offset 起始行与 limit 最大读取行数分页，并返回 total_lines 文件总行数。".to_string(),
                    "fs.write" => "向工作区中的文件写入完整内容(需要用户审批)。若目标文件的父目录不存在会自动递归创建。".to_string(),
                    "fs.edit" => "精确替换文件中的特定代码或文本片段(需要用户审批)。old_string 必须是在文件中唯一匹配的原文(含准确缩进)，new_string 为替换后的文本。若有多处相同匹配可设 replace_all=true。建议在修改前先用 fs.read 确认最新原文与缩进。".to_string(),
                    _ => {
                        if *needs_approval {
                            format!("{cap} — 需要用户审批的业务工具(调用后会弹出审批卡片)")
                        } else {
                            format!("{cap} — 只读直通工具")
                        }
                    }
                };
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": openai_name,
                        "description": desc,
                        "parameters": schema,
                    },
                })
            })
            .collect();

        for attempt in 1..=max_attempts {
            let model_id = chain[((attempt - 1) as usize) % chain.len()].clone();
            let mut tool_rounds: u32 = 0;
            loop {
                let req = InvokeRequest {
                    model_id: model_id.clone(),
                    messages: messages.clone(),
                    tools: tools_json.clone(),
                    params: Default::default(),
                    secret_ref: default_secret_ref(&model_id),
                    budget_ctx: BudgetCtx {
                        operation_id: op_id.clone(),
                        agent_id: agent_id.clone(),
                        remaining_tokens: remaining,
                    },
                    deadline: format_ts(clock.now() + Duration::seconds(timeout_secs)),
                    attempt,
                };

                // W5:请求侧快照(发送前截取;结果侧在 resp 落定后随行落盘)。
                // latency 口径:connector 返回 0 占位(基线 9.7),由调用方按
                // 真实钟测量——此处记墙钟起点,成败两路均落实测耗时。
                let snap_msgs = crate::context_log::snapshot_messages(&req.messages);
                let snap_step = tool_rounds + 1;
                let snap_model = model_id.clone();
                let snap_start = std::time::Instant::now();

                // M9-S2:流式开关开启时走 invoke_stream,增量经 ProviderDelta
                // 回核心循环(单写者落 model.content.delta 事件);通道满则丢弃
                // 单个增量(事件面渐进性降级,不影响终态聚合)。
                // 首字延迟(TTFT):首个增量到达时刻 − 请求发出时刻;仅流式
                // 可测,非流式如实为 None(整响应延迟已测 latency)。
                let first_delta_at: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>> =
                    std::sync::Arc::new(std::sync::Mutex::new(None));
                let resp = if streaming {
                    let delta_tx = tx.clone();
                    let delta_op = op_id.clone();
                    let delta_first = first_delta_at.clone();
                    let on_delta = Box::new(move |d: &str| {
                        if let Ok(mut g) = delta_first.lock()
                            && g.is_none()
                        {
                            *g = Some(std::time::Instant::now());
                        }
                        let _ = delta_tx.try_send(Cmd::ProviderDelta {
                            operation_id: delta_op.clone(),
                            delta: d.to_string(),
                        });
                    });
                    tokio::select! {
                        _ = cancel.cancelled() => InvokeResponse::Failed {
                            error_code: ErrorCode::Cancelled, retryable: false, attempt, detail_ref: None,
                        },
                        r = connector.invoke_stream(req, cancel.clone(), on_delta) => r,
                    }
                } else {
                    tokio::select! {
                        _ = cancel.cancelled() => InvokeResponse::Failed {
                            error_code: ErrorCode::Cancelled, retryable: false, attempt, detail_ref: None,
                        },
                        r = connector.invoke(req, cancel.clone()) => r,
                    }
                };
                let ttft_ms: Option<u64> = if streaming {
                    first_delta_at
                        .lock()
                        .ok()
                        .and_then(|g| *g)
                        .map(|t| t.duration_since(snap_start).as_millis() as u64)
                } else {
                    None
                };

                match resp {
                    InvokeResponse::Completed {
                        content,
                        tool_calls,
                        finish_reason: _,
                        usage,
                        model_id: mid,
                        latency_ms,
                        stream_interrupted,
                    } => {
                        // W5:上下文快照落盘(请求侧+结果侧;诊断面失败静默)
                        ctx_log.record(crate::context_log::ContextRecord {
                            session_id: session_id
                                .as_ref()
                                .map(|s| s.as_str().to_string())
                                .unwrap_or_default(),
                            agent_id: agent_id.as_str().to_string(),
                            operation_id: op_id.as_str().to_string(),
                            turn_index,
                            step: snap_step,
                            attempt,
                            model_id: snap_model,
                            streaming,
                            messages: snap_msgs,
                            tools: tools_json.clone(),
                            status: "ok",
                            error_code: None,
                            tokens_in: Some(usage.tokens_in),
                            tokens_out: Some(usage.tokens_out),
                            tokens_reasoning: usage.tokens_reasoning,
                            tokens_cached: usage.tokens_cached,
                            ttft_ms,
                            evicted_turns: Some(evicted_turns),
                            latency_ms: Some(snap_start.elapsed().as_millis() as u64),
                            ts: format_ts(clock.now()),
                        });
                        // W4 工具轮:模型请求调用直通工具 → 回核心循环执行 →
                        // 结果以 Tool 消息回喂 → 重调模型(上限 MAX_TOOL_ROUNDS)。
                        if !tool_calls.is_empty() && tool_rounds < MAX_TOOL_ROUNDS {
                            tool_rounds += 1;
                            messages.push(Message {
                                role: Role::Assistant,
                                content: content.clone(),
                            });
                            for tc in tool_calls {
                                let _ = tx.try_send(Cmd::ProviderDelta {
                                    operation_id: op_id.clone(),
                                    delta: format!("\n[调用 {}]\n", tc.name),
                                });
                                let capability = name_to_cap
                                    .get(&tc.name)
                                    .cloned()
                                    .unwrap_or_else(|| tc.name.clone());
                                let args: serde_json::Value = serde_json::from_str(&tc.arguments)
                                    .unwrap_or(serde_json::Value::Null);
                                // W9:工具调用事件(轨迹视图数据源)
                                let tool_started = std::time::Instant::now();
                                ctx_log.record_event(
                                    session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                                    op_id.as_str(),
                                    turn_index,
                                    "tool_call",
                                    &format_ts(clock.now()),
                                    serde_json::json!({
                                        "tool": tc.name,
                                        "arguments": args.clone(),
                                    }),
                                );
                                let (rtx, rrx) = tokio::sync::oneshot::channel();
                                let call_req = request_id.clone().unwrap_or_else(|| op_id.clone());
                                let _ = tx
                                    .send(Cmd::CapabilityCall {
                                        request_id: call_req,
                                        params: wire::CapabilityCallParams {
                                            capability: capability.clone(),
                                            args: args.clone(),
                                            // W4b 修复:幂等键必须含回合操作 id——
                                            // 模型不同回合的 tool_call id 会重复,
                                            // 纯 tc.id 会让幂等抑制返回上一回合的
                                            // 旧收据(模型看到旧结果反复重试)
                                            idempotency_key: Some(format!(
                                                "{}:{}",
                                                op_id.as_str(),
                                                tc.id
                                            )),
                                            deadline_ms: None,
                                        },
                                        resp: rtx,
                                    })
                                    .await;
                                // W4b 对话内审批:需审批能力调用返回
                                // ApprovalRequired 错误(审批单已开,operation
                                // 停在 waiting_approval)。此时反查审批单,
                                // 推送审批卡片标记随 SSE 流上屏,并轮询等待
                                // 用户裁决+执行落定(上限 300s=审批 TTL)。
                                let call_resp = rrx.await;
                                let mut approval_id: Option<String> = None;
                                let mut tool_op: Option<bm_contract::ids::BmId> = None;
                                let cap_is_approval = chat_tools
                                    .iter()
                                    .find(|(c, _, _)| *c == capability)
                                    .map(|(_, _, a)| *a)
                                    .unwrap_or(false);
                                if let Ok(Err(_)) = &call_resp {
                                    if cap_is_approval {
                                        let (ltx, lrx) = tokio::sync::oneshot::channel();
                                        let _ = tx
                                            .send(Cmd::ApprovalList {
                                                params: wire::ApprovalListParams {
                                                    state_filter: Some("waiting_user".into()),
                                                },
                                                resp: ltx,
                                            })
                                            .await;
                                        if let Ok(Ok(list)) = lrx.await
                                            && let Some(items) = list["approvals"].as_array()
                                        {
                                            for it in items {
                                                if it["capability"].as_str() == Some(&capability) {
                                                    approval_id = it["approval_id"]
                                                        .as_str()
                                                        .map(|s| s.to_string());
                                                    break;
                                                }
                                            }
                                        }
                                        if let Some(appr) = &approval_id {
                                            let (ptx, prx) = tokio::sync::oneshot::channel();
                                            let _ = tx
                                                .send(Cmd::GetApprovalOp {
                                                    approval_id: appr.clone(),
                                                    resp: ptx,
                                                })
                                                .await;
                                            if let Ok(Some(opid)) = prx.await {
                                                tool_op = Some(opid);
                                            }
                                        }
                                    }
                                } else if let Ok(Ok(receipt_value)) = &call_resp {
                                    tool_op = receipt_value["operation_id"]
                                        .as_str()
                                        .and_then(|s| bm_contract::ids::BmId::parse(s).ok());
                                }

                                if let Some(appr_id) = approval_id.clone() {
                                    // 审批卡片标记:随 ProviderDelta 上屏,
                                    // 前端识别 bm_approval_request 渲染卡片
                                    // (args = 模型本次调用的真实参数,卡片展示用)
                                    let _ = tx
                                        .send(Cmd::ApprovalRequested {
                                            approval_id: appr_id.clone(),
                                            capability: capability.clone(),
                                            args: args.clone(),
                                            operation_id: op_id.clone(),
                                        })
                                        .await;
                                }

                                // 受理/结果:直通能力同步出结果;MCP 异步能力经
                                // operations 轮询至终态(上限 60s);需审批能力
                                // 轮询至审批裁决+执行终态(上限 300s)。
                                let mut tool_result = String::from("工具执行无应答");
                                let wait_secs = if approval_id.is_some() { 300 } else { 60 };
                                // 直通修复(2026-09-03 VPS 实测 P1):同步收据
                                // state=succeeded 且 result 内联时立即回喂——
                                // 同步结果从不写入 op_results(仅异步回单/审批
                                // 重放两路写入),此前一律进 GetOpResult 轮询=
                                // 直通工具必现 60s「工具执行超时」。审批类与
                                // MCP 异步(state=running)仍走轮询不变。
                                let inline_sync = matches!(&call_resp, Ok(Ok(v))
                                    if v["state"].as_str() == Some("succeeded")
                                        && !v["result"].is_null());
                                if inline_sync {
                                    if let Ok(Ok(receipt_value)) = call_resp {
                                        tool_result = receipt_value["result"].to_string();
                                    }
                                } else if let Some(tool_op) = tool_op {
                                    let deadline = std::time::Instant::now()
                                        + std::time::Duration::from_secs(wait_secs);
                                    loop {
                                        if std::time::Instant::now() > deadline {
                                            tool_result = if approval_id.is_some() {
                                                "审批等待超时(用户未及时裁决,审批单已过期)".into()
                                            } else {
                                                "工具执行超时".into()
                                            };
                                            break;
                                        }
                                        tokio::time::sleep(std::time::Duration::from_millis(400))
                                            .await;
                                        // 审批路径先查操作状态(批准→succeeded /
                                        // 拒绝→cancelled),再取结果载荷
                                        if approval_id.is_some() {
                                            let (stx, srx) = tokio::sync::oneshot::channel();
                                            let _ = tx
                                                .send(Cmd::GetOperation {
                                                    params: wire::GetOperationParams {
                                                        operation_id: tool_op.clone(),
                                                    },
                                                    resp: stx,
                                                })
                                                .await;
                                            if let Ok(Ok(receipt)) = srx.await {
                                                match receipt.state {
                                                    bm_contract::states::OperationState::Succeeded => {
                                                        let (rtx2, rrx2) =
                                                            tokio::sync::oneshot::channel();
                                                        let _ = tx
                                                            .send(Cmd::GetOpResult {
                                                                operation_id: tool_op.clone(),
                                                                resp: rtx2,
                                                            })
                                                            .await;
                                                        // 审批类工具回喂附带明确指令:
                                                        // 防模型见到成功结果后重复
                                                        // 发起同一调用(实测 mimo 会)
                                                        let payload = match rrx2.await {
                                                            Ok(Ok(Some(v))) => v.to_string(),
                                                            _ => "{}".into(),
                                                        };
                                                        tool_result = format!(
                                                            "用户已批准,工具执行成功。返回结果: {payload}。该调用已完成,请直接基于此结果回答用户,不要再次调用该工具。"
                                                        );
                                                        break;
                                                    }
                                                    bm_contract::states::OperationState::Cancelled => {
                                                        tool_result = format!(
                                                            "用户拒绝了能力 {capability} 的本次审批请求,工具未执行。请直接向用户说明情况,不要再次调用该工具。"
                                                        );
                                                        break;
                                                    }
                                                    bm_contract::states::OperationState::Failed => {
                                                        tool_result =
                                                            "用户已批准,但工具执行失败,请向用户说明。".into();
                                                        break;
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        } else {
                                            let (otx, orx) = tokio::sync::oneshot::channel();
                                            let _ = tx
                                                .send(Cmd::GetOpResult {
                                                    operation_id: tool_op.clone(),
                                                    resp: otx,
                                                })
                                                .await;
                                            if let Ok(Ok(Some(v))) = orx.await {
                                                tool_result = v.to_string();
                                                break;
                                            }
                                        }
                                    }
                                } else if let Ok(Ok(receipt_value)) = call_resp {
                                    tool_result = receipt_value.to_string();
                                }
                                // W9:工具结果事件(回喂模型的原文+耗时)
                                ctx_log.record_event(
                                    session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                                    op_id.as_str(),
                                    turn_index,
                                    "tool_result",
                                    &format_ts(clock.now()),
                                    serde_json::json!({
                                        "tool": capability,
                                        "result": tool_result,
                                        "elapsed_ms": tool_started.elapsed().as_millis() as u64,
                                    }),
                                );
                                // W9 日常批:成功回喂附带完成指令(实测 mimo
                                // 见到结果后会重复调用同一工具;审批路径同款
                                // 指令已验证有效)。失败/超时/拒绝不加。
                                let tool_failed = tool_result.contains("超时")
                                    || tool_result.contains("无应答")
                                    || tool_result.contains("拒绝");
                                if !tool_failed {
                                    tool_result = format!(
                                        "{tool_result}\n(该调用已完成,请直接基于此结果回答用户,不要再次调用该工具。)"
                                    );
                                }
                                messages.push(Message {
                                    role: Role::Tool,
                                    content: tool_result,
                                });
                            }
                            // 结果回喂后重调模型(仍在同一 attempt 的降级链内)
                            continue;
                        }
                        // 工具轮上限耗尽且本轮只回了工具调用(无文本):
                        // 终稿给显式说明,避免空 content 落库(前端空气泡
                        // 门控下用户看不到任何输出)
                        let content = if !tool_calls.is_empty() && content.trim().is_empty() {
                            format!(
                                "(连续工具调用已达单回合上限 {MAX_TOOL_ROUNDS} 次,回合在此收束;请重发消息继续。)"
                            )
                        } else {
                            content
                        };
                        // W9:终稿与回合边界事件(轨迹视图数据源)
                        ctx_log.record_event(
                            session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                            op_id.as_str(),
                            turn_index,
                            "assistant_final",
                            &format_ts(clock.now()),
                            serde_json::json!({
                                "content": content,
                                "tokens_in": usage.tokens_in,
                                "tokens_out": usage.tokens_out,
                            }),
                        );
                        ctx_log.record_event(
                            session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                            op_id.as_str(),
                            turn_index,
                            "turn_end",
                            &format_ts(clock.now()),
                            serde_json::json!({
                                "outcome": "succeeded",
                                "latency_ms": latency_ms,
                            }),
                        );
                        // W5:对话台账回写(仅终稿成功;工具轮中间态不入账)
                        if let Some(sid) = session_id.clone() {
                            let _ = tx
                                .send(Cmd::RememberTurn {
                                    session_id: sid,
                                    user: user_input,
                                    assistant: content.clone(),
                                })
                                .await;
                        }
                        let _ = tx
                            .send(Cmd::Turn(TurnEvent::Completed {
                                operation_id: op_id.clone(),
                                model_id: mid,
                                attempt,
                                content,
                                usage_in: usage.tokens_in,
                                usage_out: usage.tokens_out,
                                latency_ms,
                                stream_interrupted,
                            }))
                            .await;
                        return;
                    }
                    InvokeResponse::Failed {
                        error_code,
                        retryable,
                        attempt,
                        detail_ref: _,
                    } => {
                        // W5:失败/取消同样落快照(诊断「报错」「卡死」场景)
                        ctx_log.record(crate::context_log::ContextRecord {
                            session_id: session_id
                                .as_ref()
                                .map(|s| s.as_str().to_string())
                                .unwrap_or_default(),
                            agent_id: agent_id.as_str().to_string(),
                            operation_id: op_id.as_str().to_string(),
                            turn_index,
                            step: snap_step,
                            attempt,
                            model_id: snap_model,
                            streaming,
                            messages: snap_msgs,
                            tools: tools_json.clone(),
                            status: if error_code == ErrorCode::Cancelled {
                                "cancelled"
                            } else {
                                "error"
                            },
                            error_code: Some(error_code.as_str().to_string()),
                            tokens_in: None,
                            tokens_out: None,
                            tokens_reasoning: None,
                            tokens_cached: None,
                            ttft_ms,
                            evicted_turns: Some(evicted_turns),
                            latency_ms: Some(snap_start.elapsed().as_millis() as u64),
                            ts: format_ts(clock.now()),
                        });
                        if error_code == ErrorCode::Cancelled {
                            // 显式取消:回合边界落定为 cancelled(INV-12 唯一入口)。
                            ctx_log.record_event(
                                session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                                op_id.as_str(),
                                turn_index,
                                "turn_end",
                                &format_ts(clock.now()),
                                serde_json::json!({
                                    "outcome": "cancelled",
                                    "error_code": error_code.as_str(),
                                }),
                            );
                            let _ = tx
                                .send(Cmd::Turn(TurnEvent::Cancelled {
                                    operation_id: op_id.clone(),
                                }))
                                .await;
                            return;
                        }
                        let _ = tx
                            .send(Cmd::Turn(TurnEvent::AttemptFailed {
                                operation_id: op_id.clone(),
                                model_id,
                                attempt,
                                error_code,
                            }))
                            .await;
                        if !retryable || attempt == max_attempts {
                            // W9:回合失败边界事件(轨迹视图失败红标数据源)
                            ctx_log.record_event(
                                session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                                op_id.as_str(),
                                turn_index,
                                "turn_end",
                                &format_ts(clock.now()),
                                serde_json::json!({
                                    "outcome": "failed",
                                    "error_code": error_code.as_str(),
                                }),
                            );
                            let _ = tx
                                .send(Cmd::Turn(TurnEvent::ChainExhausted {
                                    operation_id: op_id,
                                    error_code,
                                }))
                                .await;
                            return;
                        }
                        // 降级链下一 attempt(退出工具轮)
                        break;
                    }
                }
            }
        }
    });
}

pub(crate) fn handle_recovery_settle(
    w: &mut World,
    operation_id: BmId,
    verdict: RecoveryVerdict,
) -> CoreResult<Receipt> {
    if w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "持久层故障,Runtime 拒写".into(),
        ));
    }
    let from = {
        let op = w
            .operations
            .get(&operation_id)
            .ok_or_else(|| CoreError::validation("operation 不存在"))?;
        op.state
    };
    // INV-10/11:只允许恢复态被裁定,且只走迁移表合法边
    let target = match (from, verdict) {
        (OperationState::OutcomeUnknown, RecoveryVerdict::Succeeded) => {
            Some(OperationState::Succeeded)
        }
        (OperationState::OutcomeUnknown, RecoveryVerdict::Failed) => Some(OperationState::Failed),
        (OperationState::Interrupted, RecoveryVerdict::ClaimRun) => Some(OperationState::Running),
        (OperationState::Interrupted, RecoveryVerdict::Cancelled) => {
            Some(OperationState::Cancelled)
        }
        (OperationState::OutcomeUnknown, RecoveryVerdict::Cancelled) => {
            return Err(CoreError::validation(
                "outcome_unknown 无 →cancelled 边:只能经核验落 succeeded/failed(INV-11)",
            ));
        }
        (OperationState::Interrupted, RecoveryVerdict::Succeeded)
        | (OperationState::Interrupted, RecoveryVerdict::Failed) => {
            return Err(CoreError::validation(
                "interrupted 无直达 succeeded/failed 的边:claim 续跑或裁定取消",
            ));
        }
        _ => {
            return Err(CoreError::validation(
                "仅恢复态(outcome_unknown/interrupted)可裁定",
            ));
        }
    };
    let target = target.expect("上表已穷尽");

    // claim 续跑:需要受保护存储中的输入原文
    if target == OperationState::Running {
        let content = w
            .store
            .as_ref()
            .and_then(|s| s.op_input(operation_id.as_str()).ok())
            .flatten()
            .ok_or_else(|| CoreError::validation("无输入上下文,不可续跑(裁定取消或核验结论)"))?;
        w.settle_operation(&operation_id, OperationState::Running, None);
        let agent = w
            .agents
            .get(&w.operations[&operation_id].agent_id)
            .cloned()
            .expect("存在");
        {
            let agent_id = agent.id.clone();
            let a = w.agents.get_mut(&agent_id).expect("存在");
            a.transition(AgentState::WaitingModel);
        }
        spawn_turn(w, &agent, &operation_id, content, None);
        Ok(w.receipt_of(&w.operations[&operation_id]))
    } else {
        let error = match target {
            OperationState::Failed => {
                let mut e =
                    WireError::new(ErrorCode::OutcomeUnknown, "恢复裁定:按失败收口".to_string());
                e.retryable = false;
                Some(e)
            }
            OperationState::Cancelled => Some(WireError::new(
                ErrorCode::Cancelled,
                "恢复裁定:用户裁定取消".to_string(),
            )),
            _ => None,
        };
        w.settle_operation(&operation_id, target, error);
        Ok(w.receipt_of(&w.operations[&operation_id]))
    }
}

// ---- Capability Broker / Approval(M4;ADR-0001/0002)------------------------

pub(crate) const CAPABILITY_CALLER: &str = "surface:user";

/// 审批对象持久化(payload = 包装 JSON:approval 合同形态;未决时附重放执行
/// 载荷 call,裁决后剥离)。写失败仅告警不阻断:审批对象当次仍在内存可裁决,
/// 重启丢失窗口留 T6 事务性 outbox 统一收紧。
pub(crate) fn persist_approval(
    w: &World,
    approval: &Approval,
    op_id: &BmId,
    pending: Option<(&str, &serde_json::Value, Option<&str>, &str, DataTrust)>,
) {
    if let Some(store) = &w.store {
        let mut wrap = serde_json::json!({ "approval": approval });
        if let Some((capability, args, idempotency_key, principal, trust)) = pending {
            wrap["call"] = serde_json::json!({
                "capability": capability, "args": args,
                "idempotency_key": idempotency_key,
                "principal": principal, "trust": trust.as_str()
            });
        }
        let _ = store.save_approval(bm_persist::sqlite_state::ApprovalRow {
            id: approval.approval_id.as_str(),
            operation_id: op_id.as_str(),
            capability: approval.capability.as_str(),
            principal: approval.principal.as_str(),
            state: approval.state.as_str(),
            payload: &wrap.to_string(),
            created_at: approval.requested_at.as_str(),
            resolved_at: approval.resolved_at.as_deref(),
        });
    }
}

/// Grant 行同步(含消费态:Once 消费即 revoked 落行,T6c 起消费计数随行
/// 持久,重启后 count 类余量不回满)。
pub(crate) fn persist_grant(w: &World, grant_id: &str) {
    if let Some(store) = &w.store
        && let Some(grant) = w.grants.get(grant_id).cloned()
    {
        let (used, revoked) = w.grants.entry_state(grant_id).unwrap_or((0, false));
        let _ = store.save_grant(bm_persist::sqlite_state::GrantRow {
            id: grant.grant_id.as_str(),
            audience: grant.audience.as_str(),
            action: grant.action.as_str(),
            revocation_version: grant.revocation_version,
            revoked: revoked || used >= 1 && matches!(grant.scope, GrantScope::Once),
            used_count: used,
            payload: &serde_json::to_string(&grant).unwrap_or_default(),
            created_at: grant.created_at.as_str(),
        });
    }
}

/// 审批可选范围(GT-02 形态;forever 的收紧策略随 M5 审批 UI,规格 §8.6)。
pub(crate) fn capability_scope_choices() -> Vec<GrantScope> {
    vec![
        GrantScope::Once,
        GrantScope::Count(5),
        GrantScope::Ttl(3_600_000),
    ]
}

pub(crate) fn handle_capability_call(
    w: &mut World,
    request_id: BmId,
    params: wire::CapabilityCallParams,
) -> CoreResult<serde_json::Value> {
    if w.draining || w.persist_poisoned {
        return Err(CoreError::Semantic(
            ErrorCode::Unavailable,
            "Runtime 排空中或持久层故障,拒绝能力调用".into(),
        ));
    }
    // 直路径(Wire Surface):trusted 直调;幂等键随合同参数面挂链
    // (M7-T3 修复:此前仅 worker 路径挂键,Wire 直调的 idempotency_key 被忽略)
    let mut ctx = CallContext::surface(CAPABILITY_CALLER);
    if let Some(k) = &params.idempotency_key {
        ctx = ctx.with_idempotency_key(k);
    }
    capability_call_inner(w, request_id, ctx, params)
}

/// 统一执行体(M5 双路径同构):直路径 surface ctx / Agent 路径 worker ctx
/// 共用同一裁决-执行-审计管道(ADR-0002 条件 4:双路径统一 Grant/幂等/
/// 脱敏/收据合同;收据与事件的 principal 即来源标注)。
pub(crate) fn capability_call_inner(
    w: &mut World,
    request_id: BmId,
    ctx: CallContext,
    params: wire::CapabilityCallParams,
) -> CoreResult<serde_json::Value> {
    // 步 1-4:查表裁决(Broker 为字段级临时借用,用后即还)
    let decision = {
        let broker = Broker::new(
            &w.registry,
            &mut w.grants,
            &*w.config.clock,
            &*w.config.id_gen,
        );
        broker.decide(&ctx, &params.capability, &params.args)
    };
    // operation 载体:系统容器上的内存操作(M4 能力调用不依赖 Session/Agent;
    // 规范状态由 approvals/grants 承载,operations 表不落行——回看复核项)
    let op_id = w.config.id_gen.next_id("op");
    w.op_capability
        .insert(op_id.clone(), params.capability.clone());
    let created_at = w.now_ts();
    let operation = Operation {
        id: op_id.clone(),
        request_id: request_id.clone(),
        session_id: w.system_session.clone(),
        agent_id: w.system_agent.clone(),
        state: bm_contract::states::OperationState::NotStarted,
        turn_index: 0,
        created_at: created_at.clone(),
        completed_at: None,
        action_summary: format!("能力调用 {}", params.capability),
        result_reference: None,
        error: None,
    };
    w.operations.insert(op_id.clone(), operation.dispatch());

    match decision {
        Decision::Allowed { grant_id } => {
            // 统一执行助手:副作用前门禁(intent)+ 幂等抑制 + 结果事件
            let outcome =
                dispatch_capability(w, &ctx, &params.capability, params.args.clone(), &op_id);
            match outcome {
                CallOutcome::Completed {
                    call_id,
                    credential,
                    result,
                    ..
                } => {
                    let completed_at = w.now_ts();
                    w.settle_operation(&op_id, OperationState::Succeeded, None);
                    // Grant 消费态落行(Once 消费即 revoked,重启后不复活)
                    if let Some(gid) = &grant_id {
                        persist_grant(w, gid);
                    }
                    let _ = (call_id, credential);
                    Ok(serde_json::json!({
                        "operation_id": op_id.as_str(),
                        "request_id": request_id.as_str(),
                        "principal": ctx.principal.clone(),
                        "capability": params.capability,
                        "state": "succeeded",
                        "created_at": created_at,
                        "completed_at": completed_at,
                        "action_summary": format!("能力 {} 执行完成", params.capability),
                        "result_reference": null,
                        "error": null,
                        "grant_used": grant_id,
                        "result": result,
                    }))
                }
                CallOutcome::InvalidArgs { message } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::ValidationFailed,
                        &message,
                    );
                    Err(CoreError::Semantic(ErrorCode::ValidationFailed, message))
                }
                CallOutcome::StaleBinding { expected_epoch, .. } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::Unavailable,
                        &format!("binding 已切换(凭证 epoch {expected_epoch}),请重试"),
                    );
                    Err(CoreError::Semantic(
                        ErrorCode::Unavailable,
                        "Provider binding 已切换,请重试".into(),
                    ))
                }
                CallOutcome::ProviderError { message } | CallOutcome::InvalidOutput { message } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::Internal,
                        &message,
                    );
                    Err(CoreError::Internal)
                }
                CallOutcome::ProviderUnavailable { message } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::Unavailable,
                        &message,
                    );
                    Err(CoreError::Semantic(ErrorCode::Unavailable, message))
                }
                CallOutcome::Suppressed { original_result } => {
                    // 幂等抑制:不重复执行,返回原收据(审计已由助手落
                    // outcome=suppressed;ADR-0002 条件 6)
                    let completed_at = w.now_ts();
                    w.settle_operation(&op_id, OperationState::Succeeded, None);
                    if let Some(gid) = &grant_id {
                        persist_grant(w, gid);
                    }
                    Ok(serde_json::json!({
                        "operation_id": op_id.as_str(),
                        "request_id": request_id.as_str(),
                        "principal": ctx.principal.clone(),
                        "capability": params.capability,
                        "state": "succeeded",
                        "created_at": created_at,
                        "completed_at": completed_at,
                        "action_summary": "幂等抑制:等价请求返回原收据",
                        "result_reference": null,
                        "error": null,
                        "grant_used": grant_id,
                        "result": original_result,
                    }))
                }
                CallOutcome::DispatchedAsync => {
                    // M7 S4:已派发异步执行;调用方经 operations.get 轮询终态
                    Ok(serde_json::json!({
                        "operation_id": op_id.as_str(),
                        "request_id": request_id.as_str(),
                        "principal": ctx.principal.clone(),
                        "capability": params.capability,
                        "state": "running",
                        "created_at": created_at,
                        "completed_at": null,
                        "action_summary": format!("能力 {} 异步执行中", params.capability),
                        "result_reference": null,
                        "error": null,
                        "grant_used": grant_id,
                        "result": null,
                    }))
                }
                CallOutcome::Rejected { .. } => {
                    unreachable!("Allowed 分支不会再被拒绝")
                }
            }
        }
        Decision::RequireApproval {
            risk_class,
            effective_risk,
        } => {
            let mut mgr = ApprovalManager::new(&mut w.grants, &*w.config.clock, &*w.config.id_gen);
            let mut approval = mgr.open(OpenApproval {
                capability: &params.capability,
                principal: &ctx.principal,
                risk_class,
                effective_risk,
                input_trust: ctx.trust,
                args: &params.args,
                args_summary: &format!("能力 {} 调用", params.capability),
                scope_choices: capability_scope_choices(),
                ttl_ms: 300_000,
            });
            let approval_id = BmId::parse(approval.approval_id.clone()).expect("appr_ 前缀合法");
            w.settle_operation(&op_id, OperationState::WaitingApproval, None);
            w.emit(
                EventType::ApprovalRequested,
                None,
                None,
                Some(op_id.clone()),
                serde_json::json!({
                    "approval_id": approval.approval_id,
                    "operation_id": op_id.as_str(),
                    "capability": params.capability,
                    "principal": ctx.principal.clone(),
                    "risk_class": risk_class.as_str(),
                    "effective_risk": effective_risk.as_str(),
                    "input_trust": approval.input_trust.as_str(),
                    "expires_at": approval.expires_at,
                }),
            );
            approval.grant_id = None;
            w.approvals.insert(approval_id.clone(), approval.clone());
            persist_approval(
                w,
                &approval,
                &op_id,
                Some((
                    &params.capability,
                    &params.args,
                    params.idempotency_key.as_deref(),
                    ctx.principal.as_str(),
                    ctx.trust,
                )),
            );
            w.cap_pending.insert(
                approval_id,
                PendingCapabilityCall {
                    op_id,
                    capability: params.capability.clone(),
                    args: params.args.clone(),
                    idempotency_key: params.idempotency_key.clone(),
                    principal: ctx.principal.clone(),
                    trust: ctx.trust,
                },
            );
            // GT-02 场景 A2 形态:approval_required 错误信封;operation 停在
            // waiting_approval,由 approval.respond 续行(基线 §9.6)
            Err(CoreError::Semantic(
                ErrorCode::ApprovalRequired,
                format!("能力 {} 需要用户审批", params.capability),
            ))
        }
        Decision::Denied { reason } => {
            let (msg, call_id) = match reason {
                DenyReason::UnknownCapability => (
                    "未知能力,且审批不能补授权(默认拒绝)",
                    w.config.id_gen.next_id("call"),
                ),
                DenyReason::NoGrant => ("无有效授权(默认拒绝)", w.config.id_gen.next_id("call")),
            };
            let reason_code = match reason {
                DenyReason::UnknownCapability => "unknown_capability",
                DenyReason::NoGrant => "no_grant",
            };
            w.settle_operation(
                &op_id,
                OperationState::Failed,
                Some(WireError::new(ErrorCode::PermissionDenied, msg.to_string())),
            );
            w.emit(
                EventType::CapabilityDenied,
                None,
                None,
                Some(op_id.clone()),
                serde_json::json!({
                    "call_id": call_id.as_str(),
                    "capability": params.capability,
                    "principal": ctx.principal.clone(),
                    "input_trust": ctx.trust.as_str(),
                    "reason_code": reason_code,
                }),
            );
            Err(CoreError::Semantic(
                ErrorCode::PermissionDenied,
                msg.to_string(),
            ))
        }
    }
}

/// M7 S4:异步能力调用完成落定(单写者内)。
/// 成功:出参校验 → succeeded + 幂等收据/outbox published + capability.invoked ok;
/// 失败:Timeout/Transport/ToolError 三类映射,副作用 outbox 保持 pending
/// (超时 = 结果未知,对账语义与崩溃窗口一致)。
pub(crate) fn handle_provider_call(
    w: &mut World,
    operation_id: BmId,
    result: Result<serde_json::Value, crate::ports::AsyncCallError>,
) {
    use crate::ports::AsyncCallError;
    let Some(meta) = w.op_async_meta.remove(&operation_id) else {
        return;
    };
    w.cap_in_flight.remove(&operation_id);
    if !w.operations.contains_key(&operation_id) {
        return; // 停机清场后回流的迟到完成:无载体,丢弃(事件已在日志)
    }
    match result {
        Ok(value) => {
            if let Err(e) = bm_contract::schemas::validate(&meta.output_schema, &value) {
                fail_capability_call(
                    w,
                    &operation_id,
                    &meta.capability,
                    &meta.principal,
                    ErrorCode::Internal,
                    &format!("异步结果出参校验失败: {e}"),
                );
                return;
            }
            w.settle_operation(&operation_id, OperationState::Succeeded, None);
            if let (Some(h), true) = (&meta.key_hash, meta.is_side_effect) {
                w.idem_results.insert(h.clone(), value.clone());
                if let Some(store) = &w.store {
                    // F-02(审计台账):投影写失败必须留痕——静默失败会使重启后
                    // 幂等抑制失效(副作用可能重放)
                    if let Err(e) = store.save_idem_receipt(h, &value.to_string(), &w.now_ts()) {
                        eprintln!("[persist] 幂等收据落表失败 key={h}: {e:?}");
                    }
                    let _ = store.outbox_upsert(
                        operation_id.as_str(),
                        "side_effect",
                        "published",
                        &serde_json::json!({
                            "capability": meta.capability,
                            "key_hash": meta.key_hash,
                        })
                        .to_string(),
                        &w.now_ts(),
                    );
                }
            }
            if let Some(gid) = &meta.grant_id {
                persist_grant(w, gid);
            }
            w.op_results.insert(operation_id.clone(), value.clone());
            // M7 S5:成功 -> 恢复 healthy(重连成功/清探针计数)
            note_provider_success(w, &mcp_provider_of(&meta.capability), "重连握手成功");
            emit_capability_invoked_with(
                w,
                &meta.call_id,
                &operation_id,
                &meta.capability,
                &meta.principal,
                Some(meta.epoch),
                Some(&meta.instance_id),
                "ok",
                None,
                meta.key_hash.as_deref(),
            );
        }
        Err(e) => {
            // M7 S5:传输故障 -> MCP unavailable 立即;unavailable 期间的调用
            // 即重连探针(到上限后由 dispatch 门快速失败)
            if matches!(e, AsyncCallError::Transport(_)) {
                let provider = mcp_provider_of(&meta.capability);
                let was = w
                    .provider_health
                    .get(&provider)
                    .map(|h| h.status)
                    .unwrap_or("healthy");
                let entry = w.provider_health.entry(provider.clone()).or_default();
                entry.status = "unavailable";
                if was == "unavailable" {
                    entry.reconnect_attempts += 1;
                }
                if was != "unavailable" {
                    emit_provider_health(w, &provider, "healthy", "unavailable", "子进程/通道故障");
                }
            }
            let (code, msg) = match e {
                AsyncCallError::Timeout => (
                    ErrorCode::Timeout,
                    "异步调用超时(结果未知,对账由 outbox 承载)",
                ),
                AsyncCallError::Transport(_) => (ErrorCode::Unavailable, "Provider 传输故障"),
                AsyncCallError::ToolError => (ErrorCode::Internal, "工具报告执行失败"),
            };
            fail_capability_call(
                w,
                &operation_id,
                &meta.capability,
                &meta.principal,
                code,
                msg,
            );
            if let Some(gid) = &meta.grant_id {
                persist_grant(w, gid);
            }
        }
    }
}

/// M8.3:能力调用语义取消——收据落 cancelled,令牌触发(传输层尽力终止),
/// 迟到完成经 handle_provider_call 的 meta 缺失检查丢弃。
pub(crate) fn handle_capability_cancel(
    w: &mut World,
    params: wire::CapabilityCancelParams,
) -> CoreResult<wire::CapabilityCancelResult> {
    let op = w
        .operations
        .get(&params.operation_id)
        .ok_or_else(|| CoreError::validation("operation 不存在"))?;
    if !matches!(
        op.state,
        OperationState::Running | OperationState::WaitingApproval
    ) {
        return Err(CoreError::validation("operation 不在可取消状态"));
    }
    if let Some(token) = w.cap_in_flight.remove(&params.operation_id) {
        token.cancel();
    }
    // M8.3:语义取消的传输层贯彻(notifications/cancelled;尽力终止)
    if let Some(ex) = &w.config.async_executor {
        ex.cancel_op(params.operation_id.as_str());
    }
    if let Some(meta) = w.op_async_meta.remove(&params.operation_id)
        && let Some(gid) = &meta.grant_id
    {
        persist_grant(w, gid);
    }
    w.settle_operation(
        &params.operation_id,
        OperationState::Cancelled,
        Some(WireError::new(
            ErrorCode::Cancelled,
            params.reason.unwrap_or_else(|| "用户显式取消".into()),
        )),
    );
    // P0(第四轮评审):取消等待审批的操作必须连带撤审。否则用户随后批准
    // 会触发 Cancelled→Running 的表外迁移,状态机 panic,单写者死亡。
    let affected: Vec<BmId> = w
        .cap_pending
        .iter()
        .filter(|(_, p)| p.op_id == params.operation_id)
        .map(|(aid, _)| aid.clone())
        .collect();
    for aid in affected {
        w.cap_pending.remove(&aid);
        if let Some(mut approval) = w.approvals.get(&aid).cloned() {
            let resource = bm_contract::capability::GrantResource {
                capability: approval.capability.clone(),
                args_predicates: Default::default(),
            };
            let respond = {
                let mut mgr = crate::approval::ApprovalManager::new(
                    &mut w.grants,
                    &*w.config.clock,
                    &*w.config.id_gen,
                );
                mgr.respond(
                    &mut approval,
                    crate::approval::RespondDecision::Withdraw,
                    None,
                    resource,
                    "system:cancel",
                )
            };
            if let Ok(None) = respond {
                w.approvals.insert(aid.clone(), approval);
                persist_approval(
                    w,
                    w.approvals.get(&aid).expect("存在"),
                    &params.operation_id,
                    None,
                );
            }
        }
    }
    emit_capability_invoked(
        w,
        &params.operation_id,
        w.op_capability
            .get(&params.operation_id)
            .cloned()
            .unwrap_or_default()
            .as_str(),
        "user:cancel",
        None,
        None,
        "error",
        Some(ErrorCode::Cancelled),
        None,
    );
    Ok(wire::CapabilityCancelResult {
        operation_id: params.operation_id,
        state: "cancelled".into(),
    })
}

/// M7.5:异步能力进度回注 → capability.progress 事件(操作不存在则丢弃)。
pub(crate) fn handle_provider_progress(
    w: &mut World,
    operation_id: String,
    progress: u64,
    total: Option<u64>,
    message: Option<String>,
) {
    let Ok(op_id) = BmId::parse(&operation_id) else {
        return;
    };
    if !w.operations.contains_key(&op_id) {
        return;
    }
    let capability = w.op_capability.get(&op_id).cloned().unwrap_or_default();
    w.emit(
        EventType::CapabilityProgress,
        None,
        None,
        Some(op_id.clone()),
        serde_json::json!({
            "call_id": w.config.id_gen.next_id("call").as_str(),
            "operation_id": op_id.as_str(),
            "capability": capability,
            "progress": progress,
            "total": total,
            "message": message,
        }),
    );
}

/// 执行失败的统一收口:operation → failed + capability.invoked(outcome=error)。
pub(crate) fn fail_capability_call(
    w: &mut World,
    op_id: &BmId,
    capability: &str,
    principal: &str,
    code: ErrorCode,
    message: &str,
) {
    w.settle_operation(
        op_id,
        OperationState::Failed,
        Some(WireError::new(code, message.to_string())),
    );
    w.emit(
        EventType::CapabilityInvoked,
        None,
        None,
        Some(op_id.clone()),
        serde_json::json!({
            "call_id": w.config.id_gen.next_id("call").as_str(),
            "operation_id": op_id.as_str(),
            "capability": capability,
            "principal": principal,
            "binding_epoch": 0,
            "provider_instance_id": "n/a",
            "outcome": "error",
            "error_code": code.as_str(),
            "idempotency_key_hash": null,
        }),
    );
}

/// 统一执行助手(门禁+审计;T6 规格 §5.5/§5.9):副作用类先落 intent 事件
/// (前门禁——intent 落盘后方允许 Provider 执行,ADR-0001 条件 5);幂等键
/// 命中历史收据 → suppressed(不重复执行,ADR-0002 条件 6);结果落 ok/error
/// 事件。operation 终态由调用方落定。
pub(crate) fn dispatch_capability(
    w: &mut World,
    ctx: &CallContext,
    capability: &str,
    args: serde_json::Value,
    op_id: &BmId,
) -> CallOutcome {
    let prepared = {
        let mut broker = Broker::new(
            &w.registry,
            &mut w.grants,
            &*w.config.clock,
            &*w.config.id_gen,
        );
        match broker.prepare(ctx, capability, args.clone()) {
            Ok(p) => p,
            Err(outcome) => {
                emit_capability_invoked(
                    w,
                    op_id,
                    capability,
                    &ctx.principal,
                    None,
                    None,
                    "error",
                    Some(error_code_of(&outcome)),
                    None,
                );
                return outcome;
            }
        }
    };
    let key_hash: Option<String> = ctx.idempotency_key.as_ref().map(|k| {
        sha256_hex(&format!(
            "{k}:{}",
            serde_json::to_string(&args).unwrap_or_default()
        ))
    });
    if prepared.is_side_effect {
        // 幂等抑制:等价请求返回原收据,Provider 不再执行
        if let Some(h) = &key_hash
            && let Some(original) = w.idem_results.get(h).cloned()
        {
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "suppressed",
                None,
                Some(h),
            );
            return CallOutcome::Suppressed {
                original_result: original,
            };
        }
        // 前门禁:intent 落盘后方执行(崩溃窗口 = intent 在而结果不在 →
        // 恢复期以 Provider 幂等查询对账,T6b)。outbox pending 行与 intent
        // 事件同批落盘,是恢复扫描的对账底座。
        emit_capability_invoked(
            w,
            op_id,
            capability,
            &ctx.principal,
            Some(prepared.credential.binding_epoch),
            Some(&prepared.credential.provider_instance_id),
            "intent",
            None,
            key_hash.as_deref(),
        );
        if let Some(store) = &w.store {
            let _ = store.outbox_upsert(
                op_id.as_str(),
                "side_effect",
                "pending",
                &serde_json::json!({
                    "capability": capability,
                    "key_hash": key_hash,
                })
                .to_string(),
                &w.now_ts(),
            );
        }
    }
    // M7 S4:异步 Provider 路径——决策/校验/预扣/intent 门已过,执行交
    // 异步执行器,完成经 Cmd::ProviderCall 回单写者回路落定。
    if w.registry.is_async(capability) {
        // M7 S5:MCP 重连超限 -> 快速失败(不再触执行器,直至重装)
        let provider = mcp_provider_of(capability);
        let blocked = w
            .provider_health
            .get(&provider)
            .map(|h| h.status == "unavailable" && h.reconnect_attempts >= MCP_RECONNECT_LIMIT)
            .unwrap_or(false);
        if blocked {
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "error",
                Some(ErrorCode::Unavailable),
                key_hash.as_deref(),
            );
            return CallOutcome::ProviderUnavailable {
                message: "异步 Provider 重连超限,保持 unavailable 直至重装".into(),
            };
        }
        let Some(executor) = w.config.async_executor.clone() else {
            return CallOutcome::ProviderError {
                message: "异步执行器未装配".into(),
            };
        };
        let meta = AsyncCallMeta {
            capability: capability.to_string(),
            principal: ctx.principal.clone(),
            call_id: BmId::parse(&prepared.credential.call_id)
                .unwrap_or_else(|_| w.config.id_gen.next_id("call")),
            epoch: prepared.credential.binding_epoch,
            instance_id: prepared.credential.provider_instance_id.clone(),
            key_hash: key_hash.clone(),
            is_side_effect: prepared.is_side_effect,
            output_schema: prepared.manifest.output_schema.to_string(),
            grant_id: prepared.grant_id.clone(),
        };
        w.op_async_meta.insert(op_id.clone(), meta);
        // Grant 消费态随 spawn 落行(count 类重启不回满)
        if let Some(gid) = &prepared.grant_id {
            persist_grant(w, gid);
        }
        let deadline_ms = prepared.manifest.timeout_ms.clamp(100, 600_000);
        let cancel = CancellationToken::new();
        w.cap_in_flight.insert(op_id.clone(), cancel.clone());
        let tx = w.tx.clone();
        let op = op_id.clone();
        let cap = capability.to_string();
        let exec_args = args.clone();
        tokio::spawn(async move {
            let result = executor
                .call(
                    op.as_str(),
                    &cap,
                    exec_args,
                    std::time::Duration::from_millis(deadline_ms),
                )
                .await;
            let _ = tx
                .send(Cmd::ProviderCall {
                    operation_id: op,
                    result,
                })
                .await;
        });
        return CallOutcome::DispatchedAsync;
    }
    let outcome = {
        let broker = Broker::new(
            &w.registry,
            &mut w.grants,
            &*w.config.clock,
            &*w.config.id_gen,
        );
        broker.execute(&prepared, args)
    };
    match &outcome {
        CallOutcome::Completed { result, .. } => {
            if let (Some(h), true) = (&key_hash, prepared.is_side_effect) {
                w.idem_results.insert(h.clone(), result.clone());
                // T6c 收紧(M5-T1):幂等收据落表,恢复期抑制判定不依赖内存
                // F-02(审计台账):落表失败必须留痕,不得静默
                if let Some(store) = &w.store
                    && let Err(e) = store.save_idem_receipt(h, &result.to_string(), &w.now_ts())
                {
                    eprintln!("[persist] 幂等收据落表失败 key={h}: {e:?}");
                }
            }
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "ok",
                None,
                key_hash.as_deref(),
            );
            if prepared.is_side_effect
                && let Some(store) = &w.store
            {
                let _ = store.outbox_upsert(
                    op_id.as_str(),
                    "side_effect",
                    "published",
                    &serde_json::json!({
                        "capability": capability,
                        "key_hash": key_hash,
                    })
                    .to_string(),
                    &w.now_ts(),
                );
            }
        }
        CallOutcome::Suppressed { .. } => unreachable!("抑制发生在 execute 前"),
        other => {
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                Some(prepared.credential.binding_epoch),
                Some(&prepared.credential.provider_instance_id),
                "error",
                Some(error_code_of(other)),
                key_hash.as_deref(),
            );
        }
    }
    outcome
}

// ---- M5:Task 生命周期(T1)--------------------------------------------------

/// M7 S1:回合模型阶段终态审计(outcome=error;成功路径见 TurnEvent::Completed)。
/// 回答正文截断(16KB;ISO 边界按字符切割,UTF-8 安全)。
pub(crate) fn content_trunc(content: &str) -> String {
    const LIMIT: usize = 16 * 1024;
    if content.len() <= LIMIT {
        content.to_string()
    } else {
        let mut end = LIMIT;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        content[..end].to_string()
    }
}

pub(crate) fn emit_model_call_error_audit(w: &mut World, operation_id: &BmId, code: ErrorCode) {
    if let Some(a) = w.model_call_audit.remove(operation_id) {
        emit_capability_invoked_with(
            w,
            &a.call_id,
            operation_id,
            "model.invoke",
            &a.principal,
            Some(a.epoch),
            Some(&a.instance_id),
            "error",
            Some(code),
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)] // 审计字段与注册表 payload 键集一一对应
pub(crate) fn emit_capability_invoked(
    w: &mut World,
    op_id: &BmId,
    capability: &str,
    principal: &str,
    epoch: Option<u64>,
    instance: Option<&str>,
    outcome: &str,
    error_code: Option<ErrorCode>,
    key_hash: Option<&str>,
) {
    let call_id = w.config.id_gen.next_id("call");
    emit_capability_invoked_with(
        w, &call_id, op_id, capability, principal, epoch, instance, outcome, error_code, key_hash,
    );
}

/// 带预生成 call_id 的变体(turn 模型调用:授权点与审计点分离,M7 S1)。
#[allow(clippy::too_many_arguments)] // 审计字段与注册表 payload 键集一一对应
pub(crate) fn emit_capability_invoked_with(
    w: &mut World,
    call_id: &BmId,
    op_id: &BmId,
    capability: &str,
    principal: &str,
    epoch: Option<u64>,
    instance: Option<&str>,
    outcome: &str,
    error_code: Option<ErrorCode>,
    key_hash: Option<&str>,
) {
    w.emit(
        EventType::CapabilityInvoked,
        None,
        None,
        Some(op_id.clone()),
        serde_json::json!({
            "call_id": call_id.as_str(),
            "operation_id": op_id.as_str(),
            "capability": capability,
            "principal": principal,
            "binding_epoch": epoch.unwrap_or(0),
            "provider_instance_id": instance.unwrap_or("n/a"),
            "outcome": outcome,
            "error_code": error_code.map(|c| c.as_str()),
            "idempotency_key_hash": key_hash,
        }),
    );
}

pub(crate) fn error_code_of(outcome: &CallOutcome) -> ErrorCode {
    match outcome {
        CallOutcome::InvalidArgs { .. } => ErrorCode::ValidationFailed,
        CallOutcome::StaleBinding { .. } => ErrorCode::Unavailable,
        CallOutcome::ProviderError { .. } | CallOutcome::InvalidOutput { .. } => {
            ErrorCode::Internal
        }
        CallOutcome::ProviderUnavailable { .. } => ErrorCode::Unavailable,
        CallOutcome::Rejected { .. } => ErrorCode::PermissionDenied,
        _ => ErrorCode::Internal,
    }
}

pub(crate) fn sha256_hex(s: &str) -> String {
    use sha2::Digest;
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let out = h.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn handle_turn_event(w: &mut World, event: TurnEvent) {
    // M9-S3:先捕获回执 ID(事件随即被消费),处理完做自主环裁决
    let autorun_op = turn_event_op(&event).cloned();
    match event {
        TurnEvent::AttemptFailed {
            operation_id,
            model_id,
            attempt,
            error_code,
        } => {
            let (session_id, agent_id, request_id, agent_state) = {
                let op = &w.operations[&operation_id];
                let a = &w.agents[&op.agent_id];
                (
                    op.session_id.clone(),
                    op.agent_id.clone(),
                    op.request_id.clone(),
                    a.state.as_str().to_string(),
                )
            };
            w.emit(
                EventType::ModelInvocationFailed,
                Some(session_id.clone()),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "operation_id": operation_id.as_str(),
                    "agent_id": agent_id.as_str(),
                    "model_id": model_id,
                    "attempt": attempt,
                    "error_code": error_code.as_str(),
                }),
            );
            w.exec_log.record(crate::exec_log::LogRecord {
                kind: LogKind::ModelInvocation,
                session_id,
                agent_id,
                operation_id,
                request_id: Some(request_id),
                agent_state,
                detail: serde_json::json!({
                    "model_id": model_id,
                    "attempt": attempt,
                    "error_code": error_code.as_str(),
                    "stream_interrupted": false,
                }),
                ts: w.now_ts(),
            });
            // M7 S5:失败回账(>=3 连续失败 -> 熔断开闸)
            // P1(第四轮评审):仅故障类(Unavailable)计熔断;鉴权/参数错
            // (PermissionDenied/ValidationFailed)是配置错,不烧熔断器。
            if error_code == ErrorCode::Unavailable {
                note_provider_failure(w, w.config.connector.provider(), "模型调用连续失败");
            }
        }
        TurnEvent::ChainExhausted {
            operation_id,
            error_code,
        } => {
            emit_model_call_error_audit(w, &operation_id, error_code);
            w.fail_turn(
                &operation_id,
                error_code,
                format!("模型降级链耗尽({error_code})"),
            );
            w.in_flight.remove(&operation_id);
        }
        TurnEvent::Cancelled { operation_id } => {
            emit_model_call_error_audit(w, &operation_id, ErrorCode::Cancelled);
            // operation: running→cancelled(唯一合法入口 = 显式取消,INV-12)
            let (session_id, agent_id) = {
                let op = &w.operations[&operation_id];
                (op.session_id.clone(), op.agent_id.clone())
            };
            {
                let a = w.agents.get_mut(&agent_id).expect("存在");
                // waiting_model→stopping(explicit_cancel)→stopped(turn_boundary_reached)
                a.transition(AgentState::Stopping);
                a.transition(AgentState::Stopped);
            }
            w.settle_operation(
                &operation_id,
                OperationState::Cancelled,
                Some(WireError::new(
                    ErrorCode::Cancelled,
                    "用户显式取消".to_string(),
                )),
            );
            w.emit(
                EventType::AgentCancelled,
                Some(session_id),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": operation_id.as_str(),
                }),
            );
            w.in_flight.remove(&operation_id);
        }
        TurnEvent::Completed {
            operation_id,
            model_id,
            attempt,
            content,
            usage_in,
            usage_out,
            latency_ms,
            stream_interrupted,
        } => {
            autorun_note_completed(w, &operation_id, &content);
            let (session_id, agent_id, request_id, agent_state) = {
                let op = &w.operations[&operation_id];
                let a = &w.agents[&op.agent_id];
                (
                    op.session_id.clone(),
                    op.agent_id.clone(),
                    op.request_id.clone(),
                    a.state.as_str().to_string(),
                )
            };
            w.emit(
                EventType::ModelInvocationCompleted,
                Some(session_id.clone()),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "operation_id": operation_id.as_str(),
                    "agent_id": agent_id.as_str(),
                    "model_id": model_id,
                    "attempt": attempt,
                    "usage_in": usage_in,
                    "usage_out": usage_out,
                    "latency_ms": latency_ms,
                    "stream_interrupted": stream_interrupted,
                    // M8.1 修复:回答正文入事件(截断 16KB,防日志膨胀;
                    // 截断标记如实)——正文此前无处落地,用户面不可见
                    "content": content_trunc(&content),
                    "content_truncated": content.len() > 16 * 1024,
                }),
            );
            // M7 S5:成功回账(清计数/半开恢复 healthy)
            note_provider_success(w, w.config.connector.provider(), "模型调用成功");
            // M7 S1:模型调用审计(Broker 路径与普通能力调用同享 capability.invoked 面)
            if let Some(a) = w.model_call_audit.remove(&operation_id) {
                emit_capability_invoked_with(
                    w,
                    &a.call_id,
                    &operation_id,
                    "model.invoke",
                    &a.principal,
                    Some(a.epoch),
                    Some(&a.instance_id),
                    "ok",
                    None,
                    None,
                );
            }
            w.exec_log.record(crate::exec_log::LogRecord {
                kind: LogKind::ModelInvocation,
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                operation_id: operation_id.clone(),
                request_id: Some(request_id),
                agent_state,
                detail: serde_json::json!({
                    "model_id": model_id,
                    "attempt": attempt,
                    "usage": {"tokens_in": usage_in, "tokens_out": usage_out},
                    "latency_ms": latency_ms,
                    "stream_interrupted": stream_interrupted,
                }),
                ts: w.now_ts(),
            });

            // waiting_model→running(model_response_ok)
            {
                let a = w.agents.get_mut(&agent_id).expect("存在");
                a.transition(AgentState::Running);
            }

            // 强制点③(post_invoke_accounting)
            let turn_index = w.operations[&operation_id].turn_index;
            let (ratio, warn, exceeded) = {
                let a = w.agents.get_mut(&agent_id).expect("存在");
                a.budget.account(usage_in.saturating_add(usage_out))
            };
            let used = w.agents[&agent_id].budget.used_tokens;
            let limit = w.agents[&agent_id].budget.max_tokens;
            w.exec_log.record(crate::exec_log::LogRecord {
                kind: LogKind::BudgetCheck,
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                operation_id: operation_id.clone(),
                request_id: None,
                agent_state: AgentState::Running.as_str().to_string(),
                detail: serde_json::json!({
                    "scope": BudgetScope::Agent.as_str(),
                    "used_tokens": used,
                    "limit_tokens": limit,
                    "ratio": ratio,
                }),
                ts: w.now_ts(),
            });
            if warn {
                w.emit(
                    EventType::BudgetWarning,
                    Some(session_id.clone()),
                    Some(agent_id.clone()),
                    None,
                    serde_json::json!({
                        "agent_id": agent_id.as_str(),
                        "scope": BudgetScope::Agent.as_str(),
                        "used_tokens": used,
                        "limit_tokens": limit,
                        "ratio": ratio,
                    }),
                );
            }
            if exceeded {
                w.emit(
                    EventType::BudgetExceeded,
                    Some(session_id.clone()),
                    Some(agent_id.clone()),
                    None,
                    serde_json::json!({
                        "agent_id": agent_id.as_str(),
                        "scope": BudgetScope::Agent.as_str(),
                        "used_tokens": used,
                        "limit_tokens": limit,
                    }),
                );
            }

            // running→succeeded(result_recorded)+ agent.completed
            {
                let now = w.now_ts();
                let op = w.operations.get_mut(&operation_id).expect("存在");
                op.action_summary =
                    format!("回合 {turn_index} 完成({usage_in} 入 / {usage_out} 出 token)");
                op.result_reference = Some(wire::ResultReference {
                    kind: wire::ResultRefKind::ExecutionLog,
                    r#ref: format!("log:{operation_id}"),
                });
                let _ = now;
            }
            w.settle_operation(&operation_id, OperationState::Succeeded, None);
            w.emit(
                EventType::AgentCompleted,
                Some(session_id),
                Some(agent_id.clone()),
                Some(operation_id.clone()),
                serde_json::json!({
                    "agent_id": agent_id.as_str(),
                    "operation_id": operation_id.as_str(),
                    "turn_index": turn_index,
                    "content": content,
                }),
            );
            w.in_flight.remove(&operation_id);
        }
    }
    if let Some(op) = autorun_op {
        autorun_pump(w, &op);
    }
    /// M9-S3:TurnEvent → 回执 ID(自主环裁决入口用)。
    fn turn_event_op(e: &TurnEvent) -> Option<&BmId> {
        match e {
            TurnEvent::Completed { operation_id, .. }
            | TurnEvent::Cancelled { operation_id }
            | TurnEvent::AttemptFailed { operation_id, .. }
            | TurnEvent::ChainExhausted { operation_id, .. } => Some(operation_id),
        }
    }
}

// ---- W5:会话对话台账(历史回喂数据源) ------------------------------------

/// 被遗忘轮数(纯计算:累计成功回合 − 台账存活;防借位下溢)。
pub(crate) fn evicted_turns(accounted: u64, alive: u64) -> u64 {
    accounted.saturating_sub(alive)
}

/// 台账轮数上限(超出丢最旧;进程内启发式,不入冻结合同)。
pub(crate) const HISTORY_MAX_TURNS: usize = 20;
/// 台账字符总量上限(user+assistant 合计;防长会话上下文无界膨胀)。
pub(crate) const HISTORY_MAX_CHARS: usize = 24_000;

/// 成功回合终稿入账(工具轮中间态不入;只在 TurnEvent::Completed 路径调)。
/// 存活守卫:close 不取消在途回合(INV-6),迟到的落定不得把已清退的
/// 台账条目复活成孤儿——会话不存在或已 Closed 时丢弃。
pub(crate) fn remember_turn(w: &mut World, session_id: BmId, user: String, assistant: String) {
    let live = w
        .sessions
        .get(&session_id)
        .map(|s| s.state != SessionState::Closed)
        .unwrap_or(false);
    if !live {
        return;
    }
    *w.session_turn_totals.entry(session_id.clone()).or_insert(0) += 1;
    push_capped(
        w.session_chats.entry(session_id).or_default(),
        user,
        assistant,
    );
}

/// 入账并执行双上限:轮数超限丢最旧;字符总量超限从最旧丢到达标
/// (至少保留 1 条——最新一条永不因字符上限被丢)。
fn push_capped(entry: &mut Vec<(String, String)>, user: String, assistant: String) {
    entry.push((user, assistant));
    while entry.len() > HISTORY_MAX_TURNS {
        entry.remove(0);
    }
    let mut total: usize = entry.iter().map(|(u, a)| u.len() + a.len()).sum();
    while total > HISTORY_MAX_CHARS && entry.len() > 1 {
        total -= entry[0].0.len() + entry[0].1.len();
        entry.remove(0);
    }
}

#[cfg(test)]
mod w5_history_tests {
    use super::*;

    #[test]
    fn turn_count_cap_drops_oldest() {
        let mut entry = Vec::new();
        for i in 0..(HISTORY_MAX_TURNS + 5) {
            push_capped(&mut entry, format!("u{i}"), format!("a{i}"));
        }
        assert_eq!(entry.len(), HISTORY_MAX_TURNS);
        assert_eq!(entry[0], ("u5".to_string(), "a5".to_string()));
        assert_eq!(
            entry.last().unwrap().0,
            format!("u{}", HISTORY_MAX_TURNS + 4)
        );
    }

    #[test]
    fn char_cap_drops_oldest_but_keeps_latest() {
        let mut entry = Vec::new();
        let big = "x".repeat(10_000);
        for i in 0..4 {
            push_capped(&mut entry, format!("{big}{i}"), big.clone());
        }
        // 每条 2 万字符,总量 8 万 > 24000:一路丢到只剩最新一条
        assert_eq!(entry.len(), 1);
        assert_eq!(entry[0].0, format!("{big}3"));
    }

    #[test]
    fn char_cap_never_drops_single_latest() {
        let mut entry = Vec::new();
        let big = "y".repeat(HISTORY_MAX_CHARS + 100);
        push_capped(&mut entry, big.clone(), big);
        assert_eq!(entry.len(), 1, "最新一条不因字符上限被丢");
    }

    #[test]
    fn evicted_turns_arithmetic() {
        // 新会话前 3 轮全存活:无遗忘
        assert_eq!(evicted_turns(3, 3), 0);
        // 25 轮历史进 20 轮上限:遗忘最早 5 轮
        assert_eq!(evicted_turns(25, 20), 5);
        // 防御:计数滞后(复活/回放场景)不得借位下溢
        assert_eq!(evicted_turns(2, 5), 0);
        assert_eq!(evicted_turns(0, 0), 0);
    }
}
