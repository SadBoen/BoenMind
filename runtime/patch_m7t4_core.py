# -*- coding: utf-8 -*-
"""M7-T4 内核:Provider 健康状态机 + 熔断门 + 重连上限 + App 主体不直通。"""
import io

P = r'D:\96_CoderWorld\BoenMind\runtime\crates\bm-core\src\runtime.rs'
s = io.open(P, encoding='utf-8').read()

pairs = [
    # 1) World 字段
    ("""    /// M7 S4:异步能力调用结果(operation_id → result;内存,随操作同寿命)。
    op_results: HashMap<BmId, serde_json::Value>,""",
     """    /// M7 S4:异步能力调用结果(operation_id → result;内存,随操作同寿命)。
    op_results: HashMap<BmId, serde_json::Value>,
    /// M7 S5:Provider 健康面(provider → 状态;进程内,不入 core-transitions)。
    provider_health: HashMap<String, ProviderHealth>,"""),
    ("""            op_async_meta: HashMap::new(),
            op_results: HashMap::new(),""",
     """            op_async_meta: HashMap::new(),
            op_results: HashMap::new(),
            provider_health: HashMap::new(),"""),
    # 2) 健康结构与常量(挂在 AsyncCallMeta 定义后)
    ("""/// 异步能力调用的在途留档(M7 S4):spawn 时捕获,完成回流时落定。
struct AsyncCallMeta {""",
     """/// M7 S5:Provider 健康状态(HTTP 熔断/MCP 重连共用;进程内软状态)。
#[derive(Debug, Clone, Default)]
pub struct ProviderHealth {
    pub status: &'static str, // "healthy" | "unavailable"
    /// HTTP:连续失败计数(≥3 开闸);MCP:未用。
    pub fail_streak: u32,
    /// MCP:unavailable 期间的重连探针次数(>3 封禁)。
    pub reconnect_attempts: u32,
    /// HTTP:熔断冷却截止(半开放行探测);MCP:未用。
    pub cooldown_until: Option<chrono::DateTime<chrono::Utc>>,
}

const PROVIDER_FAIL_THRESHOLD: u32 = 3;
const PROVIDER_COOLDOWN_MS: i64 = 30_000;
const MCP_RECONNECT_LIMIT: u32 = 3;

/// "mcp.<server>.<tool>" → "mcp.<server>"(健康面主体;其余原样)。
fn mcp_provider_of(capability: &str) -> String {
    let parts: Vec<&str> = capability.split('.').collect();
    if parts.len() >= 3 && parts[0] == "mcp" {
        format!("mcp.{}", parts[1])
    } else {
        capability.to_string()
    }
}

/// 健康迁移(只在状态变化时发事件;payload 见 registry)。
fn emit_provider_health(
    w: &mut World,
    provider: &str,
    from: &str,
    to: &str,
    reason: &str,
) {
    w.emit(
        EventType::ProviderHealthChanged,
        None,
        None,
        None,
        serde_json::json!({
            "provider": provider,
            "from": from,
            "to": to,
            "reason": reason,
        }),
    );
}

/// HTTP 模型连接器:连续失败计账(≥3 开闸熔断,冷却 30s)。
fn note_provider_failure(w: &mut World, provider: &str, reason: &str) {
    let now = w.config.clock.now();
    let entry = w.provider_health.entry(provider.to_string()).or_default();
    entry.fail_streak += 1;
    if entry.status != "unavailable" && entry.fail_streak >= PROVIDER_FAIL_THRESHOLD {
        entry.status = "unavailable";
        entry.cooldown_until =
            Some(now + chrono::Duration::milliseconds(PROVIDER_COOLDOWN_MS));
        drop(entry);
        emit_provider_health(w, provider, "healthy", "unavailable", reason);
    }
}

/// 成功落定:清计数;若在 unavailable(半开探测/重连成功)则恢复 healthy。
fn note_provider_success(w: &mut World, provider: &str, reason: &str) {
    let Some(entry) = w.provider_health.get_mut(provider) else {
        return;
    };
    entry.fail_streak = 0;
    entry.reconnect_attempts = 0;
    if entry.status == "unavailable" {
        entry.status = "healthy";
        entry.cooldown_until = None;
        drop(entry);
        emit_provider_health(w, provider, "unavailable", "healthy", reason);
    }
}

/// 异步能力调用的在途留档(M7 S4):spawn 时捕获,完成回流时落定。
struct AsyncCallMeta {"""),
    # 3) spawn_turn 熔断门(Broker 审计块之后、cancel 创建之前)
    ("""    let Some(model_call_audit) = model_call_audit else {
        w.fail_turn(
            operation_id,
            ErrorCode::Internal,
            "模型调用权未授予或已收回".into(),
        );
        return;
    };
    w.model_call_audit
        .insert(operation_id.clone(), model_call_audit);

    let cancel = CancellationToken::new();""",
     """    let Some(model_call_audit) = model_call_audit else {
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
                h.status == "unavailable"
                    && h.cooldown_until.map(|t| now < t).unwrap_or(false)
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

    let cancel = CancellationToken::new();"""),
    # 4) AttemptFailed 回账
    ("""            w.exec_log.record(crate::exec_log::LogRecord {
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
        }""",
     """            w.exec_log.record(crate::exec_log::LogRecord {
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
            // M7 S5:失败回账(≥3 连续失败 → 熔断)
            note_provider_failure(
                w,
                w.config.connector.provider(),
                "模型调用连续失败",
            );
        }"""),
    # 5) Completed 回账(成功审计块之后)
    ("""            // M7 S1:模型调用审计(Broker 路径与普通能力调用同享 capability.invoked 面)
            if let Some(a) = w.model_call_audit.remove(&operation_id) {""",
     """            // M7 S5:成功回账(清计数/半开恢复)
            note_provider_success(w, w.config.connector.provider(), "模型调用成功");
            // M7 S1:模型调用审计(Broker 路径与普通能力调用同享 capability.invoked 面)
            if let Some(a) = w.model_call_audit.remove(&operation_id) {"""),
    # 6) dispatch 异步分支:重连超限快速失败门
    ("""    // M7 S4:异步 Provider 路径——决策/校验/预扣/intent 门已过,执行交
    // 异步执行器,完成经 Cmd::ProviderCall 回单写者回路落定。
    if w.registry.is_async(capability) {
        let Some(executor) = w.config.async_executor.clone() else {""",
     """    // M7 S4:异步 Provider 路径——决策/校验/预扣/intent 门已过,执行交
    // 异步执行器,完成经 Cmd::ProviderCall 回单写者回路落定。
    if w.registry.is_async(capability) {
        // M7 S5:MCP 重连超限 → 快速失败(不再触执行器,直至重装)
        let provider = mcp_provider_of(capability);
        let blocked = w
            .provider_health
            .get(&provider)
            .map(|h| {
                h.status == "unavailable" && h.reconnect_attempts >= MCP_RECONNECT_LIMIT
            })
            .unwrap_or(false);
        if blocked {
            emit_capability_invoked(
                w,
                op_id,
                capability,
                &ctx.principal,
                prepared.credential.binding_epoch.into(),
                Some(&prepared.credential.provider_instance_id),
                "error",
                Some(ErrorCode::Unavailable),
                key_hash.as_deref(),
            );
            return CallOutcome::ProviderUnavailable {
                message: "异步 Provider 重连超限,保持 unavailable 直至重装".into(),
            };
        }
        let Some(executor) = w.config.async_executor.clone() else {"""),
    # 7) handle_provider_call:MCP 健康回账
    ("""        Err(e) => {
            let (code, msg) = match e {
                AsyncCallError::Timeout => {
                    (ErrorCode::Timeout, "异步调用超时(结果未知,对账由 outbox 承载)")
                }
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
}""",
     """        Err(e) => {
            // M7 S5:传输故障 → MCP unavailable 立即;unavailable 期间的调用
            // 即重连探针(>上限后由 dispatch 门快速失败)
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
                drop(entry);
                if was != "unavailable" {
                    emit_provider_health(w, &provider, "healthy", "unavailable", "子进程/通道故障");
                }
            }
            let (code, msg) = match e {
                AsyncCallError::Timeout => {
                    (ErrorCode::Timeout, "异步调用超时(结果未知,对账由 outbox 承载)")
                }
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
}"""),
    # 8) handle_provider_call 成功路径:MCP 恢复 healthy
    ("""            w.op_results.insert(operation_id.clone(), value.clone());
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
            );""",
     """            w.op_results.insert(operation_id.clone(), value.clone());
            // M7 S5:成功 → 恢复 healthy(重连成功/清探针计数)
            note_provider_success(
                w,
                &mcp_provider_of(&meta.capability),
                "重连握手成功",
            );
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
            );"""),
]

for old, new in pairs:
    assert s.count(old) == 1, f"runtime anchor: {old[:60]!r} count={s.count(old)}"
    s = s.replace(old, new)
io.open(P, 'w', encoding='utf-8', newline='\n').write(s)
print('runtime.rs patched')

# ---- broker.rs:ProviderUnavailable 变体 + App 主体不直通 ----
P2 = r'D:\96_CoderWorld\BoenMind\runtime\crates\bm-core\src\broker.rs'
s = io.open(P2, encoding='utf-8').read()
pairs = [
    ("""    ProviderError {
        message: String,
    },
    /// M7 S4:已派发异步执行(收据 running;完成经 Cmd::ProviderCall 落定)。
    DispatchedAsync,
}""",
     """    ProviderError {
        message: String,
    },
    /// M7 S5:Provider 熔断/重连超限(unavailable 语义,区别于内部错误)。
    ProviderUnavailable {
        message: String,
    },
    /// M7 S4:已派发异步执行(收据 running;完成经 Cmd::ProviderCall 落定)。
    DispatchedAsync,
}"""),
    ("""        // 步 6:内建直通(仅 trusted × not-required × read-only/low-risk)。
        if ctx.trust == DataTrust::Trusted
            && manifest.approval == ApprovalRequirement::NotRequired
            && matches!(
                manifest.effect,
                RiskClass::ReadOnly | RiskClass::LowRiskCommand
            )
        {
            return Decision::Allowed { grant_id: None };
        }""",
     """        // 步 6:内建直通(仅 trusted × not-required × read-only/low-risk)。
        // M7.6:App 主体(surface:app:<name>)不享内建直通——跨 provider 访问
        // 一律走显式 Grant(默认拒绝,基线 M7 通过条件第五句)。
        if ctx.trust == DataTrust::Trusted
            && !ctx.principal.starts_with("surface:app:")
            && manifest.approval == ApprovalRequirement::NotRequired
            && matches!(
                manifest.effect,
                RiskClass::ReadOnly | RiskClass::LowRiskCommand
            )
        {
            return Decision::Allowed { grant_id: None };
        }"""),
]
for old, new in pairs:
    assert s.count(old) == 1, f"broker anchor: {old[:60]!r} count={s.count(old)}"
    s = s.replace(old, new)
io.open(P2, 'w', encoding='utf-8', newline='\n').write(s)
print('broker.rs patched')

# ---- capability_call_inner / replay / error_code_of 的新变体收口 ----
P3 = r'D:\96_CoderWorld\BoenMind\runtime\crates\bm-core\src\runtime.rs'
s = io.open(P3, encoding='utf-8').read()
pairs = [
    ("""                CallOutcome::ProviderError { message } | CallOutcome::InvalidOutput { message } => {
                    fail_capability_call(
                        w,
                        &op_id,
                        &params.capability,
                        ctx.principal.as_str(),
                        ErrorCode::Internal,
                        &message,
                    );
                    Err(CoreError::Internal)
                }""",
     """                CallOutcome::ProviderError { message } | CallOutcome::InvalidOutput { message } => {
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
                }"""),
    ("""                    other => {
                        let code = match &other {
                            CallOutcome::InvalidArgs { .. } => ErrorCode::ValidationFailed,
                            CallOutcome::StaleBinding { .. } => ErrorCode::Unavailable,
                            _ => ErrorCode::Internal,
                        };""",
     """                    CallOutcome::ProviderUnavailable { message } => {
                        // M7 S5:重连超限在批准重放中同样快速失败(unavailable)
                        fail_capability_call(
                            w,
                            &op_id,
                            &capability,
                            &principal,
                            ErrorCode::Unavailable,
                            message,
                        );
                        w.cap_pending.remove(&params.approval_id);
                    }
                    other => {
                        let code = match &other {
                            CallOutcome::InvalidArgs { .. } => ErrorCode::ValidationFailed,
                            CallOutcome::StaleBinding { .. } => ErrorCode::Unavailable,
                            _ => ErrorCode::Internal,
                        };"""),
    ("""        CallOutcome::ProviderError { .. } | CallOutcome::InvalidOutput { .. } => {
            ErrorCode::Internal
        }""",
     """        CallOutcome::ProviderError { .. } | CallOutcome::InvalidOutput { .. } => {
            ErrorCode::Internal
        }
        CallOutcome::ProviderUnavailable { .. } => ErrorCode::Unavailable,"""),
]
for old, new in pairs:
    assert s.count(old) == 1, f"arm anchor: {old[:60]!r} count={s.count(old)}"
    s = s.replace(old, new)
io.open(P3, 'w', encoding='utf-8', newline='\n').write(s)
print('arms patched')
