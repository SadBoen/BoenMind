//! spawn_turn:回合发起与工具闭环(自 turn.rs 机械移入;#19 深度拆分批次)。
//!
//! 本文件结构(自上而下):纯函数(wire 名映射/防空转熔断判定,#34 一批)→
//! 工具目录组装 build_tool_catalog(纯逻辑,#19 二批外置)→ 回合状态体
//! TurnState(#19 二批打包)→ 审批/异步工具等待环 await_tool_settlement
//! (#19 二批外置独立 async fn)→ 装配主循环 spawn_turn → 直测单测。
//! 降级链阶段函数化仍留 #19 跟踪。
use super::*;

/// OpenAI function.name 规范要求 ^[a-zA-Z0-9_-]{1,64}$,不能有点号。
/// 默认单下划线转义(fs.read -> fs_read; mcp.foo.bar -> mcp_foo_bar);
/// 内置五件走短名表;短名被占则回落长名,保唯一性。(#19 拆分:纯逻辑出回合体)
fn wire_name_of(cap: &str, taken: &std::collections::HashSet<String>) -> String {
    #[cfg(windows)]
    const EXEC_WIRE_NAME: &str = "powershell";
    #[cfg(not(windows))]
    const EXEC_WIRE_NAME: &str = "bash";
    const SHORT_WIRE_NAMES: &[(&str, &str)] = &[
        ("fs.read", "read"),
        ("fs.write", "write"),
        ("fs.edit", "edit"),
        ("fs.search", "rgrep"),
        ("system.exec", EXEC_WIRE_NAME),
    ];
    let default_name = cap.replace('.', "_");
    SHORT_WIRE_NAMES
        .iter()
        .find(|(c, _)| *c == cap)
        .map(|(_, short)| short.to_string())
        .filter(|short| !taken.contains(short))
        .unwrap_or(default_name)
}

/// 防空转熔断判定(#19 拆分:纯函数,单测直测)——窗口内已有连续 n-1 个
/// 与 sig 相同的签名(加上本次恰好 n 次)= 熔断命中。
fn breaker_hit(recent: &[(String, String)], sig: &(String, String), n: usize) -> bool {
    if n < 2 {
        return false;
    }
    let w = n - 1;
    recent.len() >= w && recent[recent.len() - w..].iter().all(|s| s == sig)
}

/// #19 拆分第二批:工具目录组装(纯逻辑自异步块外置,语句逐字保留,直测)。
/// 输入 registry.chat_tools() 快照与 agent 工具白名单;输出 wire 名→能力名
/// 映射与 OpenAI function 格式工具清单。
fn build_tool_catalog(
    chat_tools: &[(String, serde_json::Value, bool, Option<String>)],
    allowed_tools: Option<&[String]>,
) -> (
    std::collections::HashMap<String, String>,
    Vec<serde_json::Value>,
) {
    let mut name_to_cap: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    // ADR-0022 后续批(用户实测反馈):内置能力出主流短名,模型对
    // read/write/edit/rgrep/powershell/bash 有训练亲和;合同能力名不动,
    // 返回调用经 name_to_cap 映射回内核能力名。短名被占(理论边界)则
    // 回落单下划线长名,保唯一性。search 命名用户裁决弃用(易误读为
    // 网络查询)→ rgrep;exec 按平台呈现实际 shell 名(Windows=powershell,
    // 其余=bash),模型见名即知语法。
    let taken: std::collections::HashSet<String> = chat_tools
        .iter()
        .map(|(cap, ..)| cap.replace('.', "_"))
        .collect();
    let mut tools_json: Vec<serde_json::Value> = chat_tools
        .iter()
        .map(|(cap, schema, needs_approval, manifest_desc)| {
            let openai_name = wire_name_of(cap, &taken);
            name_to_cap.insert(openai_name.clone(), cap.clone());
            // ADR-0022 描述治理:描述随 manifest 走(fs.*/system.exec 内置
            // 能力与 MCP 工具均自描述);缺省按审批语义给最小兜底,不再
            // 把「弹出审批卡片」等前端 UI 行为写进模型视野。
            let desc = manifest_desc
                .as_deref()
                .filter(|d| !d.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if *needs_approval {
                        format!("{cap} — 该工具需用户批准后执行")
                    } else {
                        format!("{cap} 工具")
                    }
                });
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

    // F1(ADR-0022 后续批):工具白名单——Some(非空) 只挂清单内工具。
    // 匹配口径:能力名 fs.read / 单下划线 fs_read / wire 短名 read 三种
    // 写法均认;清单写了不存在的工具 = 忽略该项(白名单语义从宽)。
    if let Some(allowed) = allowed_tools {
        let allow: std::collections::HashSet<&str> = allowed.iter().map(String::as_str).collect();
        tools_json.retain(|t| {
            let wire = t["function"]["name"].as_str().unwrap_or("");
            match name_to_cap.get(wire) {
                Some(cap) => {
                    allow.contains(cap.as_str())
                        || allow.contains(cap.replace('.', "_").as_str())
                        || allow.contains(wire)
                }
                None => false,
            }
        });
        let kept: std::collections::HashSet<String> = tools_json
            .iter()
            .filter_map(|t| t["function"]["name"].as_str().map(String::from))
            .collect();
        name_to_cap.retain(|wire, _| kept.contains(wire));
    }
    (name_to_cap, tools_json)
}

/// #19 拆分第二批:回合状态体打包——异步块内散落的回合级可变状态集中成
/// 结构体,审批/取消/熔断状态机的状态面显式化。只收全回合生命周期的状态;
/// 批次作用域的 batch_denied(#26 同批拒绝联动)与每 attempt 清零的
/// tool_rounds 留在装配层/reset 就地处,语义就地可见。
struct TurnState {
    /// 模型消息序列(system/压缩摘要/历史回合/本轮输入/工具闭环追加)。
    messages: Vec<Message>,
    /// 防空转熔断窗口:最近工具签名 (name, arguments),超窗裁最旧。
    recent_tool_signatures: Vec<(String, String)>,
    /// 当前 attempt 内已发生的工具轮数(每批 tool_calls 记 1 轮)。
    tool_rounds: u32,
    /// 防空转熔断命中(同命令同参连续 N 次)→ 本回合不再执行工具。
    loop_broken: bool,
    /// 总轮数安全网(limits.tool_rounds_max)触发 → 本回合不再执行工具。
    round_cap_hit: bool,
}

impl TurnState {
    /// 起始消息序列:角色 prompt → 压缩摘要(#2)→ 历史回合回喂(W5)→ 本轮
    /// 输入。语句逐字保留自原异步块装配段。
    fn init(
        role_prompt: Option<&str>,
        compress_summary: Option<String>,
        history: &[(String, String)],
        user_input: &str,
    ) -> Self {
        let mut messages: Vec<Message> = Vec::new();
        if let Some(sp) = role_prompt {
            messages.push(Message {
                role: Role::System,
                content: sp.to_string(),
                tool_call_id: None,
                tool_calls: None,
            });
        }
        // #2:会话压缩摘要注入——context.compress 的产物文件存在即前置
        // (System 消息;历史原文不改写,删除文件即回退)
        if let Some(summary) = compress_summary {
            messages.push(Message {
                role: Role::System,
                content: format!(
                    "【会话压缩摘要(context.compress 生成;历史原文未改写,以下摘要把关前情)】
{summary}"
                ),
                tool_call_id: None,
                tool_calls: None,
            });
        }
        // W5(2026-09-02 用户反馈轮):历史回合回喂。此前每轮从零组装,
        // 模型对同会话前情失忆(W1 合同口径「历史由 runtime 侧维护」的实现
        // 缺口);台账在回合成功落定时经 Cmd::RememberTurn 回写。
        for (u, a) in history {
            messages.push(Message {
                role: Role::User,
                content: u.clone(),
                tool_call_id: None,
                tool_calls: None,
            });
            messages.push(Message {
                role: Role::Assistant,
                content: a.clone(),
                tool_call_id: None,
                tool_calls: None,
            });
        }
        messages.push(Message {
            role: Role::User,
            content: user_input.to_string(),
            tool_call_id: None,
            tool_calls: None,
        });
        Self {
            messages,
            recent_tool_signatures: Vec::new(),
            tool_rounds: 0,
            loop_broken: false,
            round_cap_hit: false,
        }
    }

    /// 防空转熔断窗口扫描(W10):逐个记录本批工具签名,任一签名使窗口内
    /// 连续同参次数达标即置 loop_broken;breaker_n<2 = 熔断关闭不扫描。
    fn sweep_breaker(
        &mut self,
        tool_calls: &[ToolCallPayload],
        breaker_n: usize,
        breaker_window: usize,
    ) {
        if breaker_n < 2 {
            return;
        }
        for tc in tool_calls {
            let sig = (tc.name.clone(), tc.arguments.clone());
            if breaker_hit(&self.recent_tool_signatures, &sig, breaker_n) {
                self.loop_broken = true;
                break;
            }
            self.recent_tool_signatures.push(sig);
            if self.recent_tool_signatures.len() > breaker_window {
                self.recent_tool_signatures.remove(0);
            }
        }
    }

    /// ADR-0029(2026-09-08 用户裁决):熔断/触顶拦截的调用不凭空蒸发——
    /// 逐个回喂事实性结果(未执行+原因),因果链对模型与日志完整。
    fn feed_unexecuted(&mut self, tool_calls: &[ToolCallPayload]) {
        if tool_calls.is_empty() {
            return;
        }
        let reason = if self.loop_broken {
            "已触发防空转熔断(连续相同命令与入参)"
        } else if self.round_cap_hit {
            "单回合工具轮数已达上限"
        } else {
            "回合收束"
        };
        for tc in tool_calls {
            self.messages.push(Message {
                role: Role::Tool,
                content: format!("本调用未执行:{reason}。"),
                tool_call_id: Some(tc.id.clone()),
                tool_calls: None,
            });
        }
    }
}

/// #19 拆分第二批:审批/异步工具等待环外置为独立 async fn(语句逐字保留,
/// 行为零变化)。直通同步收据不进本环(inline_sync 特判在装配层);此处只
/// 轮询 operations 至终态:需审批能力等到审批裁决+执行终态,MCP 异步能力
/// 等到执行终态。超时:审批单主动 Withdraw 撤销(P1-3,防迟到批准执行无主
/// 副作用),普通工具如实回报超时。返回 (tool_result, batch_denied)——
/// 后者仅「用户拒绝」路径置 true(#26 同批联动)。
async fn await_tool_settlement(
    tx: &tokio::sync::mpsc::Sender<Cmd>,
    wait_secs: Option<u64>,
    approval_id: Option<String>,
    tool_op: BmId,
    capability: &str,
    mut batch_denied: bool,
) -> (String, bool) {
    let deadline = wait_secs.map(|s| std::time::Instant::now() + std::time::Duration::from_secs(s));
    // 延迟初始化:环内每条 break 边都先赋 tool_result,初始占位值已无读点。
    let tool_result;
    loop {
        if deadline.is_some_and(|dl| std::time::Instant::now() > dl) {
            if let Some(appr_id) = &approval_id {
                // P1-3: 审批等待超时后主动发送 Withdraw 撤销审批单,
                // 防止后续用户迟到点击批准引发无主的真实副作用执行
                if let Ok(appr_bm_id) = BmId::parse(appr_id) {
                    let (wtx, _wrx) = tokio::sync::oneshot::channel();
                    let _ = tx
                        .send(Cmd::ApprovalRespond {
                            request_id: BmId::generate("req"),
                            params: wire::ApprovalRespondParams {
                                approval_id: appr_bm_id,
                                decision: "withdraw".to_string(),
                                scope: None,
                            },
                            resp: wtx,
                        })
                        .await;
                }
            }
            tool_result = if approval_id.is_some() {
                "审批等待超时:用户未在等待期内裁决,审批单已撤销过期,工具未执行".into()
            } else {
                format!(
                    "工具执行超时:等待 {} 秒未收到执行结果,工具未执行",
                    wait_secs.unwrap_or(0)
                )
            };
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        // 审批路径先查操作状态(批准→succeeded / 拒绝→cancelled),再取结果载荷
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
                        let (rtx2, rrx2) = tokio::sync::oneshot::channel();
                        let _ = tx
                            .send(Cmd::GetOpResult {
                                operation_id: tool_op.clone(),
                                resp: rtx2,
                            })
                            .await;
                        // 审批类工具回喂如实转述审批
                        // 结论(ADR-0022:不再附加
                        // 「不要再次调用」类禁令)
                        let payload = match rrx2.await {
                            Ok(Ok(Some(v))) => v.to_string(),
                            _ => "{}".into(),
                        };
                        tool_result = format!("用户已批准,工具执行成功。返回结果: {payload}");
                        break;
                    }
                    bm_contract::states::OperationState::Cancelled => {
                        tool_result =
                            format!("用户拒绝了能力 {capability} 的本次审批请求,工具未执行。");
                        // #26:用户驳回 → 同批余下联动取消
                        batch_denied = true;
                        break;
                    }
                    bm_contract::states::OperationState::Failed => {
                        // ADR-0029:如实回喂,不带「请向
                        // 用户说明」类教练话术。
                        let detail = receipt
                            .error
                            .as_ref()
                            .map(|e| format!("(error_code={:?}) {}", e.code.get(), e.message))
                            .unwrap_or_default();
                        tool_result = format!("用户已批准,但工具执行失败{detail}");
                        break;
                    }
                    _ => {}
                }
            }
        } else {
            // ADR-0028 修复:等待时限 0=不限时后,
            // 本循环必须对终态失败/取消即时脱身——
            // 失败操作从不写 op_results,只查
            // GetOpResult 会无限空转挂死回合。
            // 每拍先查状态:内核单写者循环保证
            // settle(Succeeded) 与载荷写入同一
            // 命令处理器内完成,观测到成功后单次
            // GetOpResult 即是结论——有则取结果,
            // 无则如实回报,零宽限零竞态。
            // 回喂纪律(ADR-0022 同源,2026-09-08
            // 用户重申):只如实转述事实,不附加
            // 任何「该怎么办」的教练话术。
            let (stx, srx) = tokio::sync::oneshot::channel();
            let _ = tx
                .send(Cmd::GetOperation {
                    params: wire::GetOperationParams {
                        operation_id: tool_op.clone(),
                    },
                    resp: stx,
                })
                .await;
            let Ok(Ok(receipt)) = srx.await else {
                continue;
            };
            match receipt.state {
                bm_contract::states::OperationState::Succeeded => {
                    let (rtx2, rrx2) = tokio::sync::oneshot::channel();
                    let _ = tx
                        .send(Cmd::GetOpResult {
                            operation_id: tool_op.clone(),
                            resp: rtx2,
                        })
                        .await;
                    tool_result = match rrx2.await {
                        Ok(Ok(Some(v))) => v.to_string(),
                        _ => "工具执行成功,但无返回结果载荷".into(),
                    };
                    break;
                }
                bm_contract::states::OperationState::Failed => {
                    let mut detail = receipt
                        .error
                        .as_ref()
                        .map(|e| format!("(error_code={:?})", e.code.get()))
                        .unwrap_or_default();
                    if let Some(e) = receipt.error.as_ref() {
                        detail.push_str(&format!(" {}", e.message));
                    }
                    tool_result = format!("工具执行失败{detail}");
                    break;
                }
                bm_contract::states::OperationState::Cancelled => {
                    tool_result = "工具执行已取消。".into();
                    break;
                }
                _ => {}
            }
        }
    }
    (tool_result, batch_denied)
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
    // M7 S1:模型调用权裁决(批9 F-05:提取为 audit_model_invoke)
    let Some(model_call_audit) = audit_model_invoke(w, agent, &chain) else {
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
    // ADR-0028:model_max_attempts=0 = 不限重试(None,链内按序循环);
    // 显式 RuntimeConfig.max_attempts 兼容保留(仍钳 1..=3)。
    let max_attempts: Option<u32> = match w.config.max_attempts {
        Some(n) => Some(n.clamp(1, 3)),
        None => {
            let lim = w.config.limits.get().model_max_attempts;
            if lim == 0 {
                None
            } else {
                Some((chain.len().min(lim as usize) as u32).clamp(1, 3))
            }
        }
    };
    // W10(ADR-0024):模型调用超时走 limits 热生效(env 覆盖已由装配方
    // 折算进 Cell;RuntimeConfig.turn_timeout_secs 保留为兼容字段不再读)。
    // ADR-0028:0 = 不限时——合同 InvokeRequest.deadline 为必填时间戳,
    // 以 100 年远期哨兵表达「无 deadline」(remaining_until 折出巨大预算)。
    let timeout_secs = w.config.limits.get().model_call_timeout_secs as i64;
    let unlimited_deadline = (timeout_secs <= 0)
        .then(|| format_ts(clock.now() + Duration::seconds(100 * 365 * 24 * 3600)));
    let tx = w.tx.clone();
    let op_id = operation_id.clone();
    let streaming = w.config.model_streaming;
    // W4b 对话工具闭环升级:全部 chat 能力(直通+审批类)均暴露给模型;
    // W4b 角色 prompt:会话级指定优先(会话创建时烤入的完整提示词,含技能);
    // 否则每回合现读 roles.json+skills.json 组装(设置页保存即热生效)。
    // 组装逻辑唯一入口 = bm-core::roles::compose_role_prompt(两条路径同口径)。
    let chat_tools: Vec<(String, serde_json::Value, bool, Option<String>)> =
        w.registry.chat_tools();
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
    // W10(ADR-0025):在跑后台作业摘要回合级注入——模型据此知道该用
    // system.job_output 收哪个 job(不做完成主动推注入,见 ADR-0025 §3)。
    let jobs_note = w
        .config
        .job_board
        .as_ref()
        .map(|b| b.summary())
        .unwrap_or_default();
    let notes: Option<String> = {
        let mut parts: Vec<String> = Vec::new();
        if let Some(n) = workspace_note {
            parts.push(n);
        }
        if !jobs_note.is_empty() {
            parts.push(jobs_note);
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(
                "

",
            ))
        }
    };
    let role_prompt = match (role_prompt, notes) {
        (Some(sp), Some(note)) => Some(format!("{sp}\n\n{note}")),
        (None, Some(note)) => Some(note),
        (other, None) => other,
    };
    let ctx_log = w.ctx_log.clone();
    // #14:Turn 内调试日志(默认关;管理面热开关)
    let turn_debug = w.turn_debug.clone();
    // #2:会话压缩摘要注入(存在即读;无 data_dir 的纯内存测试不触达)
    let compress_data_dir = w.config.data_dir.clone();
    // #1:意图硬门控参数(0=关)——触发输入过短的回合禁用一切工具派发
    let intent_gate_min = w.config.limits.get().intent_gate_min_chars;
    let limits_cell = w.config.limits.clone();

    let allowed_tools = agent.allowed_tools.clone();
    tokio::spawn(async move {
        // W4:messages 含角色 prompt + 历史回合 + 本轮输入;tools=直通工具
        // (OpenAI function 格式,capability 名的点映射为单下划线);工具结果
        // 经 CapabilityCall 回核心循环执行(Broker 裁决/审计管道原样),轮询
        // operations 至终态取结果回喂模型。
        // 轮数防线上说明:同命令同参死循环由 loop_breaker 熔断(v0.0.10);
        // 2026-09-07 架构评审 P0-1 补总轮数安全网(limits.tool_rounds_max,
        // 默认 64、0=关)——变参/换工具式轮转此前无界,烧 token 无收敛点。
        // #19 拆分第二批:起始消息序列与工具目录组装外置(纯逻辑+直测),
        // 回合级可变状态集中进 TurnState(见本文件上方结构体注释)。
        let user_input = content.clone();
        // #2:会话压缩摘要注入(context.compress 的产物文件存在即前置;
        // 无 data_dir 的纯内存测试不触达)
        let compress_summary: Option<String> =
            if let (Some(sid), Some(ddir)) = (session_id.as_ref(), compress_data_dir.as_deref()) {
                crate::context_log::load_compress_summary(ddir, sid.as_str())
            } else {
                None
            };
        let mut st = TurnState::init(
            role_prompt.as_deref(),
            compress_summary,
            &history,
            &user_input,
        );
        let (name_to_cap, tools_json) = build_tool_catalog(&chat_tools, allowed_tools.as_deref());

        // (2026-09-08 用户裁决,ADR-0029:内核不在系统提示里硬编码任何行为
        // 指导——原「工具纪律」软防线段已删,同类话术如需存在只能经内/外置
        // 插件通道以可区分方式注入,SETTLED §2-10 口径由本条取代。)

        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            if max_attempts.is_some_and(|m| attempt > m) {
                break;
            }
            let model_id = chain[((attempt - 1) as usize) % chain.len()].clone();
            // 工具轮数按 attempt 清零(原装配段局部变量语义,现落在状态体上
            // 显式 reset——降级链切模型后重新计轮)。
            st.tool_rounds = 0;
            loop {
                let req = InvokeRequest {
                    model_id: model_id.clone(),
                    messages: st.messages.clone(),
                    tools: tools_json.clone(),
                    params: Default::default(),
                    secret_ref: default_secret_ref(&model_id),
                    budget_ctx: BudgetCtx {
                        operation_id: op_id.clone(),
                        agent_id: agent_id.clone(),
                        remaining_tokens: remaining,
                    },
                    deadline: match &unlimited_deadline {
                        Some(ts) => ts.clone(),
                        None => format_ts(clock.now() + Duration::seconds(timeout_secs)),
                    },
                    attempt,
                };

                // W5:请求侧快照(发送前截取;结果侧在 resp 落定后随行落盘)。
                // latency 口径:connector 返回 0 占位(基线 9.7),由调用方按
                // 真实钟测量——此处记墙钟起点,成败两路均落实测耗时。
                let snap_msgs = crate::context_log::snapshot_messages(&req.messages);
                let snap_step = st.tool_rounds + 1;
                let snap_model = model_id.clone();
                let snap_start = std::time::Instant::now();
                // #14:调试面——请求侧全量(消息序列+工具数;开关关时零成本)
                turn_debug.record(
                    "model_request",
                    session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                    agent_id.as_str(),
                    op_id.as_str(),
                    serde_json::json!({
                        "step": snap_step,
                        "attempt": attempt,
                        "model_id": snap_model.clone(),
                        "streaming": streaming,
                        "messages": snap_msgs,
                        "tools_count": tools_json.len(),
                    }),
                );

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
                            error_code: ErrorCode::Cancelled, retryable: false, attempt, detail_ref: None, detail: None,
                        },
                        r = connector.invoke_stream(req, cancel.clone(), on_delta) => r,
                    }
                } else {
                    tokio::select! {
                        _ = cancel.cancelled() => InvokeResponse::Failed {
                            error_code: ErrorCode::Cancelled, retryable: false, attempt, detail_ref: None, detail: None,
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
                        finish_reason,
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
                            model_id: snap_model.clone(),
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
                        // #14:调试面——模型响应原文(比 context-log 厚:含回复
                        // 内容、finish_reason 与工具调用全参;开关关时零成本)
                        turn_debug.record(
                            "model_response",
                            session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                            agent_id.as_str(),
                            op_id.as_str(),
                            serde_json::json!({
                                "step": snap_step,
                                "attempt": attempt,
                                "model_id": snap_model.clone(),
                                "streaming": streaming,
                                "content": content,
                                "finish_reason": finish_reason,
                                "tool_calls": tool_calls,
                                "tokens_in": usage.tokens_in,
                                "tokens_out": usage.tokens_out,
                                "ttft_ms": ttft_ms,
                                "latency_ms": snap_start.elapsed().as_millis() as u64,
                            }),
                        );
                        // W4 工具轮:模型请求调用直通工具 → 回核心循环执行 →
                        // 结果以 Tool 消息回喂 → 重调模型。
                        if !tool_calls.is_empty() && !st.loop_broken && !st.round_cap_hit {
                            st.tool_rounds += 1;
                            // P0-1 总轮数安全网(limits 热生效,0=关):
                            // 超限不再执行工具,回合就地收束并告知用户。
                            let cap = limits_cell.get().tool_rounds_max;
                            if cap > 0 && st.tool_rounds > cap {
                                st.round_cap_hit = true;
                                let _ = tx.try_send(Cmd::ProviderDelta {
                                    operation_id: op_id.clone(),
                                    delta: format!(
                                        "\n(本回合工具调用已达 {cap} 轮上限,为防失控烧钱在此收束;如需继续请发新消息。)\n"
                                    ),
                                });
                            }
                            // W10:防空转熔断阈值/窗口走 limits(0=关闭)。
                            // 检测是否连续 N 次调用完全相同工具与参数(N=limits)。
                            let lim = limits_cell.get();
                            let breaker_n = lim.loop_breaker_consecutive as usize;
                            let breaker_window = lim.loop_breaker_window;
                            st.sweep_breaker(&tool_calls, breaker_n, breaker_window);

                            if st.loop_broken {
                                let _ = tx.try_send(Cmd::ProviderDelta {
                                    operation_id: op_id.clone(),
                                    delta: format!(
                                        "\n(检测到连续 {breaker_n} 次调用相同工具与完全一致的入参，已触发防空转熔断保护。)\n"
                                    ),
                                });
                            } else {
                                // ADR-0022:assistant 消息原样携带 tool_calls 回喂,
                                // 模型才能把下一轮的工具结果对齐回自己发起的调用
                                // (此前只回 content,调用结构丢失 = 模型「失忆」)。
                                st.messages.push(Message {
                                    role: Role::Assistant,
                                    tool_call_id: None,
                                    tool_calls: Some(tool_calls.clone()),
                                    content: content.clone(),
                                });
                                // #26:同批拒绝联动——本批任一调用被用户驳回后,
                                // 余下调用不再派发(策略开关走 limits,0=回退独立执行)
                                let mut batch_denied = false;
                                for tc in tool_calls {
                                    // #1:意图硬门控——触发输入过短的回合禁用一切工具
                                    // 派发(如实回喂;0=关)。在 #26 联动判定之前,
                                    // 被门控拦截的调用不产生审批单/不触达提供者
                                    if intent_gate_min > 0
                                        && (user_input.chars().count() as u32) < intent_gate_min
                                    {
                                        st.messages.push(Message {
                                            role: Role::Tool,
                                            content: "本调用未执行:意图门控——本轮触发输入过短,无操作意图,工具已禁用;请直接以文字回应用户或请用户补充需求。".into(),
                                            tool_call_id: Some(tc.id.clone()),
                                            tool_calls: None,
                                        });
                                        turn_debug.record(
                                            "tool_cancelled",
                                            session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                                            agent_id.as_str(),
                                            op_id.as_str(),
                                            serde_json::json!({
                                                "tool": tc.name,
                                                "reason": "intent_gate",
                                                "input_chars": user_input.chars().count(),
                                            }),
                                        );
                                        continue;
                                    }
                                    if batch_denied
                                        && limits_cell.get().tool_batch_cancel_on_deny == 1
                                    {
                                        st.messages.push(Message {
                                            role: Role::Tool,
                                            content: "本调用未执行:同批已有调用被用户驳回,按策略联动取消余下调用。".into(),
                                            tool_call_id: Some(tc.id.clone()),
                                            tool_calls: None,
                                        });
                                        turn_debug.record(
                                            "tool_cancelled",
                                            session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                                            agent_id.as_str(),
                                            op_id.as_str(),
                                            serde_json::json!({
                                                "tool": tc.name,
                                                "reason": "batch_deny_linkage"
                                            }),
                                        );
                                        continue;
                                    }
                                    let args: serde_json::Value =
                                        serde_json::from_str(&tc.arguments)
                                            .unwrap_or(serde_json::Value::Null);

                                    // 提取核心目标参数(如 path, file_path, command, query)用于前端清晰呈现
                                    let target_summary = args
                                        .get("path")
                                        .or_else(|| args.get("file_path"))
                                        .or_else(|| args.get("command"))
                                        .or_else(|| args.get("query"))
                                        .or_else(|| args.get("pattern"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");

                                    let target_display = if !target_summary.is_empty() {
                                        format!(" {}", target_summary)
                                    } else {
                                        String::new()
                                    };

                                    let _ = tx.try_send(Cmd::ProviderDelta {
                                        operation_id: op_id.clone(),
                                        delta: format!("\n[调用 {}{}]\n", tc.name, target_display),
                                    });
                                    let capability = name_to_cap
                                        .get(&tc.name)
                                        .cloned()
                                        .unwrap_or_else(|| tc.name.clone());
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
                                    // #14:调试面——工具调用全参
                                    turn_debug.record(
                                        "tool_call",
                                        session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                                        agent_id.as_str(),
                                        op_id.as_str(),
                                        serde_json::json!({
                                            "step": snap_step,
                                            "tool": tc.name,
                                            "capability": capability,
                                            "arguments": args.clone(),
                                        }),
                                    );
                                    let (rtx, rrx) = tokio::sync::oneshot::channel();
                                    let call_req =
                                        request_id.clone().unwrap_or_else(|| op_id.clone());
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
                                    match &call_resp {
                                        // W4b+ 加固:ApprovalRequired 错误自带开单点的
                                        // approval_id/operation_id(CoreError::ApprovalNeeded),
                                        // 回合侧零反查——杜绝多会话/并发调用同能力时
                                        // 「批准 A 执行 B」的错配缺陷
                                        Ok(Err(CoreError::ApprovalNeeded {
                                            approval_id: aid,
                                            operation_id: opid,
                                            ..
                                        })) => {
                                            approval_id = Some(aid.clone());
                                            tool_op = bm_contract::ids::BmId::parse(opid).ok();
                                        }
                                        Ok(Ok(receipt_value)) => {
                                            tool_op =
                                                receipt_value["operation_id"].as_str().and_then(
                                                    |s| bm_contract::ids::BmId::parse(s).ok(),
                                                );
                                        }
                                        _ => {}
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
                                    // operations 轮询至终态;需审批能力轮询至审批
                                    // 裁决+执行终态。
                                    let mut tool_result = String::from("工具执行无应答");
                                    // ADR-0029:调用层直接失败(如能力校验拒绝)
                                    // 如实回喂真实死因,不以占位文案掩盖。
                                    if let Ok(Err(e)) = &call_resp {
                                        tool_result = match e {
                                            CoreError::Semantic(code, msg) => {
                                                format!("工具调用失败({}): {}", code.as_str(), msg)
                                            }
                                            other => format!("工具调用失败: {other}"),
                                        };
                                    }
                                    // W10:等待时限走 limits;ADR-0028:0 = 不限时(None)。
                                    let lim_wait = limits_cell.get();
                                    let wait_secs: Option<u64> = if approval_id.is_some() {
                                        (lim_wait.approval_wait_ms > 0)
                                            .then(|| (lim_wait.approval_wait_ms / 1000).max(1))
                                    } else {
                                        (lim_wait.tool_wait_ms > 0)
                                            .then(|| (lim_wait.tool_wait_ms / 1000).max(1))
                                    };
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
                                            // ADR-0029:幂等抑制如实告知——等价请求
                                            // 返回的是旧结果,模型必须知道本次没有
                                            // 真实执行。
                                            let suppressed = receipt_value["action_summary"]
                                                .as_str()
                                                .is_some_and(|s| s.contains("幂等抑制"));
                                            tool_result = if suppressed {
                                                format!(
                                                    "本次未重复执行(等价请求幂等返回原结果): {}",
                                                    receipt_value["result"]
                                                )
                                            } else {
                                                receipt_value["result"].to_string()
                                            };
                                        }
                                    } else if let Some(tool_op) = tool_op {
                                        // #19 拆分第二批:审批/异步工具等待环
                                        // 外置为独立 async fn(本文件上方)——
                                        // 轮询 operations 至终态,审批超时撤单
                                        // (P1-3),用户拒绝联动 batch_denied(#26)。
                                        let (result, denied) = await_tool_settlement(
                                            &tx,
                                            wait_secs,
                                            approval_id,
                                            tool_op,
                                            &capability,
                                            batch_denied,
                                        )
                                        .await;
                                        tool_result = result;
                                        batch_denied = denied;
                                    } else if let Ok(Ok(receipt_value)) = call_resp {
                                        tool_result = receipt_value.to_string();
                                    }
                                    // W9:工具结果事件(回喂模型的原文+耗时)
                                    let elapsed_ms = tool_started.elapsed().as_millis() as u64;
                                    ctx_log.record_event(
                                        session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                                        op_id.as_str(),
                                        turn_index,
                                        "tool_result",
                                        &format_ts(clock.now()),
                                        serde_json::json!({
                                            "tool": capability,
                                            "result": tool_result,
                                            "elapsed_ms": elapsed_ms,
                                        }),
                                    );
                                    // #14:调试面——工具结果全文(与回喂同文)
                                    turn_debug.record(
                                        "tool_result",
                                        session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                                        agent_id.as_str(),
                                        op_id.as_str(),
                                        serde_json::json!({
                                            "step": snap_step,
                                            "tool": tc.name,
                                            "capability": capability,
                                            "result": tool_result,
                                            "elapsed_ms": elapsed_ms,
                                        }),
                                    );
                                    // 前端轻量反馈:向前端推一条工具执行耗时与成败标记
                                    let _ = tx.try_send(Cmd::ProviderDelta {
                                        operation_id: op_id.clone(),
                                        delta: format!(
                                            "\n[工具完成 {} 耗时 {}ms]\n",
                                            tc.name, elapsed_ms
                                        ),
                                    });
                                    // ADR-0022:工具结果原生 role=tool + tool_call_id
                                    // 回喂,对齐模型因果链。不再强贴「不要再次调用」
                                    // 类负向禁令——链式调用(搜→读→改→测)是模型的
                                    // 正常工作方式;失控防线=同参熔断 + limits.
                                    // tool_rounds_max 总轮数安全网(本文件上方)。
                                    st.messages.push(Message {
                                        role: Role::Tool,
                                        content: tool_result,
                                        tool_call_id: Some(tc.id.clone()),
                                        tool_calls: None,
                                    });
                                }
                            }
                            // 结果回喂后重调模型(仍在同一 attempt 的降级链内)
                            continue;
                        }
                        // ADR-0029(2026-09-08 用户裁决):熔断/触顶拦截的调用
                        // 不再凭空蒸发——逐个回喂事实性结果(未执行+原因),
                        // 因果链对模型与日志完整;回合就此收束,不再重调模型。
                        // 原内核代写的 assistant 终稿废除:收束原因只在触发点
                        // 经 ProviderDelta 上屏(UI-only),不入台账、不冒充
                        // 模型发言;content 保持模型原文(可能为空)。
                        st.feed_unexecuted(&tool_calls);
                        // W9:终稿与回合边界事件(轨迹视图数据源)。
                        // ADR-0029:assistant_final 只在模型真有话时记录——
                        // 内核不再生产终稿内容,空终稿不落轨迹。
                        if !content.trim().is_empty() {
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
                        }
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
                        // #14:调试面——回合终态(成功)
                        turn_debug.record(
                            "turn_end",
                            session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                            agent_id.as_str(),
                            op_id.as_str(),
                            serde_json::json!({
                                "outcome": "succeeded",
                                "attempt": attempt,
                                "tool_rounds": st.tool_rounds,
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
                        detail,
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
                            model_id: snap_model.clone(),
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
                        // #14:调试面——模型调用失败(含脱敏后细节)
                        turn_debug.record(
                            "model_failed",
                            session_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
                            agent_id.as_str(),
                            op_id.as_str(),
                            serde_json::json!({
                                "step": snap_step,
                                "attempt": attempt,
                                "model_id": snap_model.clone(),
                                "error_code": error_code.as_str(),
                                "retryable": retryable,
                                "detail": detail,
                            }),
                        );
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
                        let exhausted = max_attempts.is_some_and(|m| attempt >= m);
                        if !retryable || exhausted {
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
                                    detail,
                                }))
                                .await;
                            return;
                        }
                        // ADR-0028:不限重试时加 1s 退避,防对僵死网关热循环打点
                        if max_attempts.is_none() {
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                        // 降级链下一 attempt(退出工具轮)
                        break;
                    }
                }
            }
        }
    });
}

/// M7 S1:模型调用权裁决(批9 F-05:自 spawn_turn 提取,语句逐字保留)。
fn audit_model_invoke(w: &mut World, agent: &Agent, chain: &[String]) -> Option<ModelCallAudit> {
    {
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
    }
}

/// #19 拆分第二批直测:工具目录组装 / 回合状态体(TurnState init/熔断扫描/
/// 未执行回喂)。等待环 await_tool_settlement 需活体命令回路,由 chat 系列
/// 集成测试(chat_batch_deny/chat_direct_tool/chat_intent_gate)与黄金轨迹护航。
#[cfg(test)]
mod turn_state_tests {
    use super::TurnState;
    use super::build_tool_catalog;
    use bm_contract::connector::{Role, ToolCallPayload};

    fn tc(id: &str, name: &str, arguments: &str) -> ToolCallPayload {
        ToolCallPayload {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }
    }

    #[test]
    fn init_seeds_history_and_user_without_extras() {
        let st = TurnState::init(None, None, &[("问".into(), "答".into())], "你好");
        assert_eq!(st.messages.len(), 3, "历史 user+assistant + 本轮 user");
        assert!(matches!(st.messages[0].role, Role::User));
        assert!(matches!(st.messages[1].role, Role::Assistant));
        assert_eq!(st.messages[2].content, "你好");
        assert!(
            st.messages
                .iter()
                .all(|m| m.tool_call_id.is_none() && m.tool_calls.is_none())
        );
        assert_eq!(st.tool_rounds, 0);
        assert!(!st.loop_broken && !st.round_cap_hit);
        assert!(st.recent_tool_signatures.is_empty());
    }

    #[test]
    fn init_prepends_role_prompt_and_compress_summary() {
        let st = TurnState::init(Some("系统提示"), Some("前情摘要".into()), &[], "hi");
        assert_eq!(st.messages.len(), 3);
        assert!(matches!(st.messages[0].role, Role::System));
        assert_eq!(st.messages[0].content, "系统提示");
        assert!(matches!(st.messages[1].role, Role::System));
        assert!(
            st.messages[1].content.contains("会话压缩摘要")
                && st.messages[1].content.contains("前情摘要")
        );
    }

    #[test]
    fn sweep_breaker_trips_only_after_n_consecutive_same() {
        let batch = [tc("1", "fs.read", r#"{"path":"a"}"#)];
        let mut st = TurnState::init(None, None, &[], "x");
        st.sweep_breaker(&batch, 3, 10);
        st.sweep_breaker(&batch, 3, 10);
        assert!(!st.loop_broken, "前 n-1 次不命中");
        st.sweep_breaker(&batch, 3, 10);
        assert!(st.loop_broken, "第 n 次同签名命中");

        // 穿插不同签名打断连续性
        let other = [tc("2", "fs.read", r#"{"path":"b"}"#)];
        let mut st2 = TurnState::init(None, None, &[], "x");
        st2.sweep_breaker(&batch, 3, 10);
        st2.sweep_breaker(&other, 3, 10);
        st2.sweep_breaker(&batch, 3, 10);
        assert!(!st2.loop_broken, "同签名连续性被打断后不命中");

        // breaker_n<2 = 熔断关闭不扫描
        let mut st3 = TurnState::init(None, None, &[], "x");
        for _ in 0..5 {
            st3.sweep_breaker(&batch, 1, 10);
        }
        assert!(!st3.loop_broken);
        assert!(st3.recent_tool_signatures.is_empty(), "关闭时不记签名");
    }

    #[test]
    fn feed_unexecuted_replies_every_call_with_reason() {
        let batch = [tc("1", "fs.read", "{}"), tc("2", "mcp.x.y", "{}")];
        let mut st = TurnState::init(None, None, &[], "x");
        st.loop_broken = true;
        st.feed_unexecuted(&batch);
        assert_eq!(st.messages.len(), 3, "起始 user 输入 + 2 条未执行回喂");
        assert!(st.messages[1].content.contains("防空转熔断"));
        assert_eq!(st.messages[1].tool_call_id.as_deref(), Some("1"));
        assert_eq!(st.messages[2].tool_call_id.as_deref(), Some("2"));
        // 空批次零回喂
        let mut st2 = TurnState::init(None, None, &[], "x");
        st2.feed_unexecuted(&[]);
        assert_eq!(st2.messages.len(), 1, "空批次零回喂,只剩起始 user 输入");
    }

    #[test]
    fn tool_catalog_maps_wire_names_and_honors_allowlist() {
        let chat_tools = vec![
            (
                "fs.read".to_string(),
                serde_json::json!({"type": "object"}),
                false,
                Some("读文件".to_string()),
            ),
            (
                "mcp.srv.tool".to_string(),
                serde_json::json!({}),
                true,
                None,
            ),
        ];
        let (name_to_cap, tools_json) = build_tool_catalog(&chat_tools, None);
        assert_eq!(name_to_cap["read"], "fs.read", "内置五件出 wire 短名");
        assert_eq!(name_to_cap["mcp_srv_tool"], "mcp.srv.tool", "点转单下划线");
        assert_eq!(tools_json.len(), 2);
        assert_eq!(tools_json[0]["function"]["description"], "读文件");
        assert_eq!(
            tools_json[1]["function"]["description"], "mcp.srv.tool — 该工具需用户批准后执行",
            "无 manifest 描述按审批语义兜底"
        );

        // 白名单:wire 短名写法命中;清单外工具忽略;映射同步裁剪
        let allow = vec!["read".to_string(), "nonexistent".to_string()];
        let (n2c, tj) = build_tool_catalog(&chat_tools, Some(&allow));
        assert_eq!(tj.len(), 1, "白名单外(mcp.srv.tool)被过滤");
        assert_eq!(tj[0]["function"]["name"], "read");
        assert_eq!(n2c.len(), 1, "name_to_cap 与保留工具同步");
        assert_eq!(n2c["read"], "fs.read");

        // 能力名写法亦认(从宽口径)
        let (n2c2, tj2) = build_tool_catalog(&chat_tools, Some(&["fs.read".to_string()]));
        assert_eq!(tj2.len(), 1);
        assert_eq!(n2c2["read"], "fs.read");
    }
}

/// #19 拆分第一步:回合体纯逻辑外置后的直测(防空转熔断判定 + wire 名映射)。
#[cfg(test)]
mod pure_helpers_tests {
    use super::breaker_hit;
    use super::wire_name_of;

    fn sig(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn breaker_hits_only_after_n_consecutive_same() {
        let n = 3;
        let window: Vec<(String, String)> = vec![sig("a", "1"), sig("a", "1")];
        assert!(
            breaker_hit(&window, &sig("a", "1"), n),
            "窗口已有 n-1=2 个同签名 → 本次命中"
        );
        let mixed: Vec<(String, String)> = vec![sig("a", "1"), sig("b", "2")];
        assert!(!breaker_hit(&mixed, &sig("a", "1"), n), "签名不同不命中");
        let short: Vec<(String, String)> = vec![sig("a", "1")];
        assert!(
            !breaker_hit(&short, &sig("a", "1"), n),
            "窗口不足 n-1 不命中"
        );
        assert!(!breaker_hit(&[], &sig("a", "1"), n), "空窗口不命中");
        // n<2 = 熔断关闭
        assert!(!breaker_hit(&window, &sig("a", "1"), 1));
        assert!(!breaker_hit(&window, &sig("a", "1"), 0));
    }

    #[test]
    fn wire_names_escape_dots_and_prefer_free_short_names() {
        let taken: std::collections::HashSet<String> =
            ["mcp_x_y".to_string()].into_iter().collect();
        assert_eq!(wire_name_of("fs.read", &taken), "read", "内置五件走短名");
        assert_eq!(
            wire_name_of("system.exec", &taken),
            if cfg!(windows) { "powershell" } else { "bash" }
        );
        assert_eq!(wire_name_of("mcp.x.y", &taken), "mcp_x_y", "点转单下划线");
        // 短名被占 → 回落长名
        let taken2: std::collections::HashSet<String> = ["read".to_string()].into_iter().collect();
        assert_eq!(
            wire_name_of("fs.read", &taken2),
            "fs_read",
            "短名被占回落长名"
        );
    }
}
