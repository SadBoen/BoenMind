//! 合同同步测试:Rust 投影 ↔ 冻结合同文本,任何漂移立即变红。
//! 对应合同库 CI 规则 R2/R3/R4/R6 的实现侧镜像。

use bm_contract::budget::{AccountingRecord, Budget};
use bm_contract::connector::{InvokeRequest, InvokeResponse, Message, Role, Usage};
use bm_contract::error_codes::{ErrorCode, Since, WIRE_CODES, WireErrorCode};
use bm_contract::events::{EventEnvelope, EventType};
use bm_contract::exec_log::LogEntry;
use bm_contract::ids::BmId;
use bm_contract::registries;
use bm_contract::schemas::{validate, validate_by_pointer};
use bm_contract::states::{AgentState, OperationState, SessionState, TaskState};
use bm_contract::timestamp;
use bm_contract::wire::{
    AgentSpec, CancelResult, EventsPollResult, GetOperationParams, Receipt, RequestEnvelope,
    ResponseEnvelope, SendInputParams, SessionCloseResult, SessionCreateParams,
    SessionCreateResult, SessionResumeResult, WIRE_VERSION,
};
use serde_json::json;

// ---- R6:envelope 错误码枚举 ↔ 注册表 -------------------------------------

#[test]
fn error_code_enum_matches_registry() {
    let registry = registries::error_codes();
    assert_eq!(
        ErrorCode::ALL.len(),
        registry.len(),
        "枚举与注册表条数不一致"
    );
    for (variant, reg) in ErrorCode::ALL.iter().zip(&registry) {
        assert_eq!(variant.as_str(), reg.code, "枚举顺序/名称与注册表不一致");
        assert_eq!(
            variant.cli_exit(),
            reg.cli_exit,
            "{} cli_exit 漂移",
            reg.code
        );
        assert_eq!(
            variant.default_retryable(),
            reg.default_retryable,
            "{} default_retryable 漂移",
            reg.code
        );
        let since = match variant.available_since() {
            Since::M1 => "M1",
            Since::M4 => "M4",
        };
        assert_eq!(
            since, reg.available_since,
            "{} available_since 漂移",
            reg.code
        );
    }
}

#[test]
fn envelope_error_code_enum_matches_wire_codes() {
    let doc: serde_json::Value = serde_json::from_str(registries::ENVELOPE_SCHEMA).unwrap();
    let enum_codes: Vec<&str> = doc
        .pointer("/definitions/error_code/enum")
        .and_then(|e| e.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .expect("envelope schema 应有 error_code 枚举");
    let wire: Vec<&str> = WIRE_CODES.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        enum_codes, wire,
        "envelope 枚举 ≠ Wire 可用码(M1∪M4,CI 规则 R6)"
    );
}

#[test]
fn wire_error_code_accepts_all_registry_codes() {
    let json = serde_json::to_string(&WireErrorCode::new(ErrorCode::Timeout).unwrap()).unwrap();
    let back: WireErrorCode = serde_json::from_str(&json).unwrap();
    assert_eq!(back.get(), ErrorCode::Timeout);

    // M4 起:四码进入信封枚举(Minor 增发),全量注册表码可序列化往返
    for code in [
        ErrorCode::PermissionDenied,
        ErrorCode::ApprovalRequired,
        ErrorCode::ApprovalDenied,
        ErrorCode::IdempotencyConflict,
    ] {
        let ser = serde_json::to_string(&WireErrorCode::new(code).unwrap()).unwrap();
        let back: WireErrorCode = serde_json::from_str(&ser).unwrap();
        assert_eq!(back.get(), code, "{code:?} 应可进 Wire 信封(M4)");
    }

    // 未知码仍然拒绝(核心码封闭,基线 9.8)
    assert!(
        serde_json::from_str::<WireErrorCode>("\"not_a_code\"").is_err(),
        "未注册码不得进 Wire 信封"
    );
}

// ---- R3:事件类型 ⊆ 注册表,键集一致 --------------------------------------

#[test]
fn event_type_enum_matches_registry() {
    let registry = registries::runtime_events();
    assert_eq!(
        registry.len(),
        45,
        "注册表事件数漂移(M1 20 + M2 增发 2 + M4 增发 10 + M5 增发 8 + M6 增发 1 + M7 增发 2 + M9 增发 2)"
    );
    for reg in &registry {
        let t = EventType::from_wire(&reg.type_).unwrap_or_else(|| panic!("枚举缺 {}", reg.type_));
        let mut expected: Vec<String> = t.payload_keys().iter().map(|s| s.to_string()).collect();
        expected.sort();
        let mut actual: Vec<String> = reg
            .payload
            .as_object()
            .expect("payload 描述是对象")
            .keys()
            .cloned()
            .collect();
        actual.sort();
        assert_eq!(expected, actual, "{} payload 键集漂移", reg.type_);
    }
}

// ---- R4:迁移表 ↔ core-transitions JSON -----------------------------------

#[test]
fn state_machines_match_transitions_json() {
    let machines = registries::core_transitions().machines;

    let op = &machines["operation"];
    assert_eq!(OperationState::ALL_LEN, op.states.len());
    for s in &op.states {
        let v = OperationState::from_wire(s).unwrap_or_else(|| panic!("operation 缺状态 {s}"));
        assert_eq!(
            v.is_terminal(),
            op.terminal.contains(s),
            "{s} terminal 漂移"
        );
    }
    assert_eq!(OperationState::transitions().len(), op.transitions.len());
    for tr in OperationState::transitions() {
        assert!(
            op.transitions.iter().any(|r| r.from == tr.from.as_str()
                && r.to == tr.to.as_str()
                && r.guard == tr.guard),
            "operation 迁移 {:?}->{:?} 在 JSON 中无对应或 guard 漂移",
            tr.from,
            tr.to
        );
    }

    let sess = &machines["session"];
    assert_eq!(SessionState::ALL_LEN, sess.states.len());
    assert_eq!(SessionState::transitions().len(), sess.transitions.len());

    let agent = &machines["agent"];
    assert_eq!(AgentState::ALL_LEN, agent.states.len());
    assert_eq!(AgentState::transitions().len(), agent.transitions.len());
    for tr in AgentState::transitions() {
        assert!(
            agent.transitions.iter().any(|r| r.from == tr.from.as_str()
                && r.to == tr.to.as_str()
                && r.guard == tr.guard),
            "agent 迁移 {:?}->{:?} 漂移",
            tr.from,
            tr.to
        );
    }

    // M5 增发:task 状态机镜像
    let task = &machines["task"];
    assert_eq!(TaskState::ALL_LEN, task.states.len());
    for s in &task.states {
        let v = TaskState::from_wire(s).unwrap_or_else(|| panic!("task 缺状态 {s}"));
        assert_eq!(
            v.is_terminal(),
            task.terminal.contains(s),
            "task {s} terminal 漂移"
        );
    }
    assert_eq!(TaskState::transitions().len(), task.transitions.len());
    for tr in TaskState::transitions() {
        assert!(
            task.transitions.iter().any(|r| r.from == tr.from.as_str()
                && r.to == tr.to.as_str()
                && r.guard == tr.guard),
            "task 迁移 {:?}->{:?} 在 JSON 中无对应或 guard 漂移",
            tr.from,
            tr.to
        );
    }
}

// ---- R2(镜像):类型序列化后过对应 schema ---------------------------------

fn sample_request(method: &str, params: serde_json::Value) -> serde_json::Value {
    json!({
        "v": WIRE_VERSION,
        "method": method,
        "request_id": "req_01J9Z8G3K2X7M4Q6B8WD5RNYVT",
        "idempotency_key": null,
        "params": params
    })
}

#[test]
fn wire_requests_validate_against_envelope() {
    let cases = vec![
        (
            "session.create",
            json!({"agent": {"name": "assistant", "model_chain": ["zhipu.glm-4-flash"]}}),
        ),
        (
            "session.resume",
            json!({"session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX"}),
        ),
        (
            "session.close",
            json!({"session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX", "reason": "user_request"}),
        ),
        (
            "events.poll",
            json!({"session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX", "since_seq": 0}),
        ),
        (
            "agent.send_input",
            json!({
            "session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
            "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
            "content": "用一句话解释什么是幂等性",
            "input_trust": "trusted"}),
        ),
        (
            "agent.cancel",
            json!({
            "session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
            "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
            "operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM"}),
        ),
        (
            "operations.get",
            json!({"operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM"}),
        ),
        // M4 增发三方法(GT-02 形态)
        (
            "capability.call",
            json!({"capability": "system.echo", "args": {"message": "ping"},
                   "idempotency_key": null, "deadline_ms": 1000}),
        ),
        ("approval.list", json!({})),
        (
            "approval.respond",
            json!({"approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAA04", "decision": "deny"}),
        ),
        // M5 增发六方法(GT-03 形态;params 合法性在 wire_task 方法级测试细化)
        (
            "task.create",
            json!({"title": "整理读书笔记", "goal": "把 inbox 笔记归档到 notes 并复核"}),
        ),
        ("task.list", json!({})),
        (
            "task.get",
            json!({"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2"}),
        ),
        (
            "task.pause",
            json!({"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2", "reason": "我先看看"}),
        ),
        (
            "task.resume",
            json!({"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2", "note": "继续"}),
        ),
        (
            "task.stop",
            json!({"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2", "reason": null}),
        ),
        // M5 增发:events.poll 可选 task_id 过滤(同名方法第二形态)
        (
            "events.poll",
            json!({"session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX", "since_seq": 0,
                   "task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2"}),
        ),
        // M8 增发:capability.cancel(语义取消)
        (
            "capability.cancel",
            json!({"operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAB2"}),
        ),
        // M4 补齐:capability.list(能力发现)
        ("capability.list", json!({"provider": null})),
    ];
    for (method, params) in cases {
        let req = sample_request(method, params);
        validate_by_pointer(registries::ENVELOPE_SCHEMA, "#/request", &req)
            .unwrap_or_else(|e| panic!("{method} 请求应合法: {e}"));
    }

    // 方法名与序列化形态交叉验证
    let env = RequestEnvelope::new(
        bm_contract::wire::Method::SessionCreate,
        "req_01J9Z8G3K2X7M4Q6B8WD5RNYVT".parse().unwrap(),
        json!({}),
    );
    let ser = serde_json::to_value(&env).unwrap();
    assert_eq!(ser["method"], "session.create");
    assert_eq!(ser["v"], "0.1");
    assert!(ser.get("idempotency_key").is_none(), "None 不应序列化");
}

#[test]
fn wire_responses_validate_against_envelope() {
    let ok = json!({
        "v": WIRE_VERSION,
        "request_id": "req_01J9Z8G3K2X7M4Q6B8WD5RNYVT",
        "ok": true,
        "result": {"session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX"}
    });
    validate_by_pointer(registries::ENVELOPE_SCHEMA, "#/response", &ok).expect("成功响应合法");

    let err = json!({
        "v": WIRE_VERSION,
        "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
        "ok": false,
        "error": {
            "code": "timeout",
            "message": "模型降级链耗尽:2 次尝试均超时",
            "retryable": false,
            "retry_after_ms": null,
            "detail_ref": null
        }
    });
    validate_by_pointer(registries::ENVELOPE_SCHEMA, "#/response", &err)
        .expect("错误响应合法(GT 场景 B 形态)");

    let resp: ResponseEnvelope = serde_json::from_value(err).unwrap();
    match &resp {
        ResponseEnvelope::Failure { error, .. } => {
            assert_eq!(error.code.get(), ErrorCode::Timeout);
            assert!(!error.retryable);
        }
        _ => panic!("应为失败分支"),
    }
    let ser = serde_json::to_value(&resp).unwrap();
    assert_eq!(
        ser["error"]["retry_after_ms"],
        serde_json::Value::Null,
        "None 序列化为显式 null(GT 形态)"
    );
}

#[test]
fn session_and_agent_payloads_validate() {
    let create = serde_json::to_value(SessionCreateParams {
        agent: AgentSpec {
            name: "assistant".into(),
            model_chain: vec!["zhipu.glm-4-flash".into(), "openai.gpt-4o-mini".into()],
            budget: Some(Budget {
                max_tokens: 50000,
                max_turns: 10,
                extra: Default::default(),
            }),
            system_prompt: None,
            workspace_id: None,
        },
    })
    .unwrap();
    validate_by_pointer(
        registries::SESSION_SCHEMA,
        "#/session.create/params",
        &create,
    )
    .expect("session.create params 合法");

    let create_result = serde_json::to_value(SessionCreateResult {
        session_id: "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX".parse().unwrap(),
        agent_id: "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP".parse().unwrap(),
        created_at: "2026-08-29T09:30:00.220Z".into(),
        resume_cursor: bm_contract::wire::Cursor { event_seq: 3 },
    })
    .unwrap();
    validate_by_pointer(
        registries::SESSION_SCHEMA,
        "#/session.create/result",
        &create_result,
    )
    .expect("session.create result 合法");

    let resume_result = serde_json::to_value(SessionResumeResult {
        session_state: SessionState::Active,
        agent_state: AgentState::Running,
        last_event_seq: 11,
        events: vec![],
    })
    .unwrap();
    validate_by_pointer(
        registries::SESSION_SCHEMA,
        "#/session.resume/result",
        &resume_result,
    )
    .expect("session.resume result 合法");

    let poll_result = serde_json::to_value(EventsPollResult {
        events: vec![],
        last_seq: 0,
        has_more: false,
    })
    .unwrap();
    validate_by_pointer(
        registries::SESSION_SCHEMA,
        "#/events.poll/result",
        &poll_result,
    )
    .expect("events.poll result 合法");

    let close_result = serde_json::to_value(SessionCloseResult {
        closed_at: "2026-08-29T09:30:08.500Z".into(),
        agent_final_state: "running".into(),
    })
    .unwrap();
    validate_by_pointer(
        registries::SESSION_SCHEMA,
        "#/session.close/result",
        &close_result,
    )
    .expect("session.close result 合法");

    let cancel_result = serde_json::to_value(CancelResult {
        accepted: true,
        operation_id: "op_01J9Z8G56BX7M4Q6B8WD5RV6QM".parse().unwrap(),
    })
    .unwrap();
    validate_by_pointer(
        registries::AGENT_SCHEMA,
        "#/agent.cancel/result",
        &cancel_result,
    )
    .expect("agent.cancel result 合法");

    let get_params = serde_json::to_value(GetOperationParams {
        operation_id: "op_01J9Z8G56BX7M4Q6B8WD5RV6QM".parse().unwrap(),
    })
    .unwrap();
    validate_by_pointer(
        registries::AGENT_SCHEMA,
        "#/operations.get/params",
        &get_params,
    )
    .expect("operations.get params 合法");
}

#[test]
fn gt_receipts_validate_against_agent_schema() {
    // 黄金轨迹 A2 的执行收据
    let running: Receipt = serde_json::from_value(json!({
        "operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
        "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
        "principal": "user",
        "task_type": "agent.turn",
        "state": "running",
        "created_at": "2026-08-29T09:30:05.012Z",
        "completed_at": null,
        "action_summary": "Agent 回合:解释幂等性",
        "result_reference": null,
        "error": null
    }))
    .unwrap();
    let v = serde_json::to_value(&running).unwrap();
    validate_by_pointer(registries::AGENT_SCHEMA, "#/definitions/receipt", &v)
        .expect("GT-A2 收据应过 receipt schema");

    // 黄金轨迹 A4 的终态收据
    let done = json!({
        "operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
        "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
        "principal": "user",
        "task_type": "agent.turn",
        "state": "succeeded",
        "created_at": "2026-08-29T09:30:05.012Z",
        "completed_at": "2026-08-29T09:30:07.110Z",
        "action_summary": "已回答幂等性问题(412 入 / 58 出 token)",
        "result_reference": {"kind": "execution_log", "ref": "log:op_01J9Z8G56BX7M4Q6B8WD5RV6QM"},
        "error": null
    });
    validate_by_pointer(registries::AGENT_SCHEMA, "#/definitions/receipt", &done)
        .expect("GT-A4 收据应过 receipt schema");
}

#[test]
fn gt_exec_log_entries_validate() {
    let entries = vec![
        json!({
            "log_seq": 1, "ts": "2026-08-29T09:30:05.012Z", "kind": "agent.turn",
            "session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
            "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
            "operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
            "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
            "state": "running", "secret_scan": "passed",
            "detail": {"turn_index": 1, "input_digest": "sha256:9f2c", "input_bytes": 42}
        }),
        json!({
            "log_seq": 2, "ts": "2026-08-29T09:30:06.890Z", "kind": "model.invocation",
            "session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
            "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
            "operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
            "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
            "state": "waiting_model", "secret_scan": "passed",
            "detail": {"model_id": "zhipu.glm-4-flash", "attempt": 1,
                       "usage": {"tokens_in": 412, "tokens_out": 58},
                       "latency_ms": 1873, "stream_interrupted": false}
        }),
        json!({
            "log_seq": 3, "ts": "2026-08-29T09:30:07.108Z", "kind": "budget.check",
            "session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
            "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
            "operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
            "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
            "state": "running", "secret_scan": "passed",
            "detail": {"scope": "agent", "used_tokens": 470, "limit_tokens": 50000, "ratio": 0.0094}
        }),
    ];
    for e in &entries {
        let parsed: LogEntry = serde_json::from_value(e.clone()).unwrap();
        let ser = serde_json::to_value(&parsed).unwrap();
        validate(registries::EXEC_LOG_SCHEMA, &ser).expect("GT 日志条目应过 schema");
    }

    // P0(第四轮评审):secret_scan 增 "failed"(fail-closed 降格态,Minor);
    // 未知值仍拒绝。
    let mut ok_failed = entries[0].clone();
    ok_failed["secret_scan"] = json!("failed");
    assert!(
        serde_json::from_value::<LogEntry>(ok_failed).is_ok(),
        "failed 态(fail-closed 降格)应合法"
    );
    let mut bad = entries[0].clone();
    bad["secret_scan"] = json!("bogus");
    assert!(
        serde_json::from_value::<LogEntry>(bad).is_err(),
        "SecretScan 未知值仍拒绝"
    );
}

#[test]
fn connector_invoke_validates() {
    let req = InvokeRequest {
        model_id: "zhipu.glm-4-flash".into(),
        messages: vec![Message {
            role: Role::User,
            content: "用一句话解释什么是幂等性".into(),
        }],
        tools: vec![],
        params: Default::default(),
        secret_ref: "secret:model.zhipu".into(),
        budget_ctx: bm_contract::connector::BudgetCtx {
            operation_id: "op_01J9Z8G56BX7M4Q6B8WD5RV6QM".parse().unwrap(),
            agent_id: "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP".parse().unwrap(),
            remaining_tokens: 50000,
        },
        deadline: "2026-08-29T09:30:35.012Z".into(),
        attempt: 1,
    };
    let ser = serde_json::to_value(&req).unwrap();
    validate_by_pointer(
        registries::CONNECTOR_SCHEMA,
        "#/definitions/invoke_request",
        &ser,
    )
    .expect("invoke_request 应过 schema");

    let ok_resp = InvokeResponse::Completed {
        content: "幂等性是指同一操作执行多次与执行一次的效果相同。".into(),
        tool_calls: Vec::new(),
        finish_reason: bm_contract::connector::FinishReason::Stop,
        usage: Usage {
            tokens_in: 412,
            tokens_out: 58,
            ..Default::default()
        },
        model_id: "zhipu.glm-4-flash".into(),
        latency_ms: 1873,
        stream_interrupted: false,
    };
    let ser = serde_json::to_value(&ok_resp).unwrap();
    validate_by_pointer(
        registries::CONNECTOR_SCHEMA,
        "#/definitions/invoke_response",
        &ser,
    )
    .expect("invoke_response(ok) 应过 schema");

    // W4 回归:finish_reason = tool_calls + 携带 tool_calls 必须合法过 schema
    let tool_call_resp = InvokeResponse::Completed {
        content: String::new(),
        tool_calls: vec![bm_contract::connector::ToolCallPayload {
            id: "call_123".into(),
            name: "fs_search".into(),
            arguments: r#"{"pattern":"test"}"#.into(),
        }],
        finish_reason: bm_contract::connector::FinishReason::ToolCalls,
        usage: Usage {
            tokens_in: 300,
            tokens_out: 40,
            tokens_reasoning: Some(10),
            tokens_cached: Some(50),
        },
        model_id: "zhipu.glm-4-flash".into(),
        latency_ms: 1200,
        stream_interrupted: false,
    };
    let ser = serde_json::to_value(&tool_call_resp).unwrap();
    validate_by_pointer(
        registries::CONNECTOR_SCHEMA,
        "#/definitions/invoke_response",
        &ser,
    )
    .expect("invoke_response(tool_calls) 应过 schema");

    // W4 回归:role = tool 消息必须合法过 invoke_request schema
    let req_with_tool_msg = bm_contract::connector::InvokeRequest {
        model_id: "zhipu.glm-4-flash".into(),
        messages: vec![
            bm_contract::connector::Message {
                role: bm_contract::connector::Role::User,
                content: "查一下文件".into(),
            },
            bm_contract::connector::Message {
                role: bm_contract::connector::Role::Assistant,
                content: "".into(),
            },
            bm_contract::connector::Message {
                role: bm_contract::connector::Role::Tool,
                content: "[]".into(),
            },
        ],
        tools: vec![],
        params: Default::default(),
        secret_ref: "secret:zhipu-api-key".into(),
        budget_ctx: bm_contract::connector::BudgetCtx {
            operation_id: BmId::generate("op"),
            agent_id: BmId::generate("agent"),
            remaining_tokens: 10000,
        },
        deadline: timestamp::now(),
        attempt: 1,
    };
    let ser = serde_json::to_value(&req_with_tool_msg).unwrap();
    validate_by_pointer(
        registries::CONNECTOR_SCHEMA,
        "#/definitions/invoke_request",
        &ser,
    )
    .expect("invoke_request(with role:tool) 应过 schema");

    let fail_resp = InvokeResponse::Failed {
        error_code: ErrorCode::Timeout,
        retryable: true,
        attempt: 1,
        detail_ref: None,
    };
    let ser = serde_json::to_value(&fail_resp).unwrap();
    validate_by_pointer(
        registries::CONNECTOR_SCHEMA,
        "#/definitions/invoke_response",
        &ser,
    )
    .expect("invoke_response(failed) 应过 schema");
}

#[test]
fn budget_validates() {
    let b = Budget {
        max_tokens: 50000,
        max_turns: 10,
        extra: Default::default(),
    };
    validate_by_pointer(
        registries::BUDGET_SCHEMA,
        "#/definitions/budget",
        &serde_json::to_value(&b).unwrap(),
    )
    .expect("budget 合法");

    // 开放键值:未知标量键被保留
    let b2: Budget = serde_json::from_value(json!({
        "max_tokens": 100, "max_turns": 1, "max_cost": 0.5, "owner": "boen"
    }))
    .unwrap();
    assert_eq!(
        b2.extra.get("owner"),
        Some(&bm_contract::budget::ExtraValue::Str("boen".into()))
    );

    let rec = AccountingRecord {
        scope: bm_contract::budget::BudgetScope::Agent,
        operation_id: "op_01J9Z8G56BX7M4Q6B8WD5RV6QM".parse().unwrap(),
        used_tokens: 470,
        limit_tokens: 50000,
        ratio: bm_contract::budget::round_ratio(470.0, 50000.0),
        at: "2026-08-29T09:30:07.108Z".into(),
    };
    assert_eq!(serde_json::to_value(&rec).unwrap()["ratio"], json!(0.0094));
    validate_by_pointer(
        registries::BUDGET_SCHEMA,
        "#/definitions/accounting_record",
        &serde_json::to_value(&rec).unwrap(),
    )
    .expect("accounting_record 合法");
}

#[test]
fn event_envelope_roundtrip_and_validation() {
    let env = EventEnvelope::new(
        1,
        EventType::RuntimeStarted,
        "2026-08-29T09:30:00.100Z".into(),
        None,
        None,
        None,
        json!({"pid": 43121, "version": "0.1.0-m1", "started_at": "2026-08-29T09:30:00.098Z"}),
    );
    let ser = serde_json::to_value(&env).unwrap();
    validate_by_pointer(registries::ENVELOPE_SCHEMA, "#/event_envelope", &ser)
        .expect("事件信封应过 envelope schema");
    let back: EventEnvelope = serde_json::from_value(ser).unwrap();
    assert_eq!(back, env);
    assert_eq!(back.event_type, EventType::RuntimeStarted);

    // 带 correlation 的事件
    let env2 = EventEnvelope::new(
        4,
        EventType::AgentTurnStarted,
        "2026-08-29T09:30:05.012Z".into(),
        Some("sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX".parse().unwrap()),
        Some("agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP".parse().unwrap()),
        Some("op_01J9Z8G56BX7M4Q6B8WD5RV6QM".parse().unwrap()),
        json!({"agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
               "operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM", "turn_index": 1}),
    );
    validate_by_pointer(
        registries::ENVELOPE_SCHEMA,
        "#/event_envelope",
        &serde_json::to_value(&env2).unwrap(),
    )
    .expect("带关联字段的事件应过 schema");
}

#[test]
fn send_input_params_roundtrip() {
    let p: SendInputParams = serde_json::from_value(json!({
        "session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
        "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
        "content": "你好",
        "input_trust": "trusted"
    }))
    .unwrap();
    assert_eq!(p.input_trust, bm_contract::wire::InputTrust::Trusted);
    // M1 不接受 untrusted(schema enum 只冻了 trusted)
    assert!(
        serde_json::from_value::<SendInputParams>(json!({
            "session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
            "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
            "content": "x", "input_trust": "untrusted"
        }))
        .is_err()
    );
}

// ---- M4 增发(GT-02 形态):capability.* 合同自检 ---------------------------

#[test]
fn capability_manifest_validates() {
    let m = json!({
        "capability": "system.echo",
        "provider": "system.echo",
        "version": "0.1.0",
        "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": "read-only",
        "idempotent": true,
        "cancellable": true,
        "timeout_ms": 1000,
        "approval": "not-required",
        "scopes": ["system.echo"],
        "verification": {"query": "system.echo", "within_ms": 2000},
        "undo": null,
        "retry": {"max_attempts": 1, "backoff_ms": 100, "retry_on": []},
        "deprecated_by": null,
        "mutation_class": "safe"
    });
    validate(registries::CAPABILITY_MANIFEST_SCHEMA, &m).expect("manifest 合法");

    let mut bad = m.clone();
    bad["effect"] = json!("ultra-risky");
    assert!(
        validate(registries::CAPABILITY_MANIFEST_SCHEMA, &bad).is_err(),
        "未知风险等级必须被拒"
    );
}

#[test]
fn capability_grant_validates() {
    let g = json!({
        "grant_id": "grant_01JAAAAAAAAAAAAAAAAAAAAA0C",
        "audience": "agent:note_bot",
        "action": "system.notes.write",
        "resource": {"capability": "system.notes.write",
                     "args_predicates": {"path": "notes/inbox.md"}},
        "scope": "once",
        "delegation_depth": 0,
        "expires_at": "2026-08-29T10:30:00.000Z",
        "revocation_version": 0,
        "parent_grant_hash": "9b1dec3f2a6c47d5b8e0f1a2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a",
        "issued_by": "surface:user",
        "created_at": "2026-08-29T10:02:09.500Z"
    });
    validate(registries::CAPABILITY_GRANT_SCHEMA, &g).expect("grant 合法");

    // delegation_depth 恒 0(不可再转授,ADR-0002)
    let mut bad = g.clone();
    bad["delegation_depth"] = json!(1);
    assert!(
        validate(registries::CAPABILITY_GRANT_SCHEMA, &bad).is_err(),
        "delegation_depth > 0 必须被拒"
    );

    // scope 枚举形态
    let mut bad = g;
    bad["scope"] = json!("whenever");
    assert!(
        validate(registries::CAPABILITY_GRANT_SCHEMA, &bad).is_err(),
        "非法 scope 必须被拒"
    );
}

#[test]
fn capability_approval_and_lease_validate() {
    let a = json!({
        "approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAA04",
        "capability": "system.danger.purge",
        "args_digest": "9b1dec3f2a6c47d5b8e0f1a2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a",
        "args_summary": "清除 notes 域全部内容(target=notes)",
        "principal": "surface:user",
        "risk_class": "high-risk-command",
        "effective_risk": "high-risk-command",
        "input_trust": "trusted",
        "state": "waiting_user",
        "scope_choices": ["once", "count:5", "ttl:1h"],
        "requested_at": "2026-08-29T10:00:00.220Z",
        "expires_at": "2026-08-29T10:05:00.220Z",
        "resolved_at": null,
        "grant_id": null
    });
    validate(registries::CAPABILITY_APPROVAL_SCHEMA, &a).expect("approval 合法");

    let mut bad = a;
    bad["input_trust"] = json!("very-trusted");
    assert!(
        validate(registries::CAPABILITY_APPROVAL_SCHEMA, &bad).is_err(),
        "未知信任级别必须被拒"
    );

    let l = json!({
        "lease_id": "lease_01JAAAAAAAAAAAAAAAAAAAAA0G",
        "binding_epoch": 1,
        "policy_version": "policy-1",
        "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAA03",
        "provider_instance_id": "system.echo@0.1.0",
        "deadline": "2026-08-29T10:00:10.000Z",
        "byte_budget": 1048576
    });
    validate(registries::CAPABILITY_LEASE_SCHEMA, &l).expect("lease 合法");
}

#[test]
fn wire_capability_params_and_result_validate() {
    let call_params = json!({
        "capability": "system.echo", "args": {"message": "ping"},
        "idempotency_key": null, "deadline_ms": 1000
    });
    validate_by_pointer(
        registries::WIRE_CAPABILITY_SCHEMA,
        "#/capability.call/params",
        &call_params,
    )
    .expect("capability.call params 合法");

    // input_trust 不在参数面(调用方不可自报信任级别,规格 §5.4)
    let mut bad = call_params.clone();
    bad["input_trust"] = json!("trusted");
    assert!(
        validate_by_pointer(
            registries::WIRE_CAPABILITY_SCHEMA,
            "#/capability.call/params",
            &bad
        )
        .is_err(),
        "params 不得携带 input_trust(additionalProperties=false)"
    );

    let call_result = json!({
        "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAA03",
        "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAA06",
        "principal": "surface:user",
        "capability": "system.echo",
        "state": "succeeded",
        "created_at": "2026-08-29T10:00:00.150Z",
        "completed_at": "2026-08-29T10:00:00.180Z",
        "action_summary": "echo 回显完成",
        "result_reference": null,
        "error": null
    });
    validate_by_pointer(
        registries::WIRE_CAPABILITY_SCHEMA,
        "#/capability.call/result",
        &call_result,
    )
    .expect("capability.call result(执行收据)合法");

    let respond_params = json!({
        "approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAA0B",
        "decision": "approve", "scope": "once"
    });
    validate_by_pointer(
        registries::WIRE_CAPABILITY_SCHEMA,
        "#/approval.respond/params",
        &respond_params,
    )
    .expect("approval.respond params 合法");
}

// ---- M5 增发(GT-03 形态):task / memory / observation 合同自检 ------------

#[test]
fn task_object_validates() {
    let task = json!({
        "task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2",
        "title": "整理读书笔记",
        "goal": "把 inbox 笔记归档到 notes 并复核",
        "state": "running",
        "created_by": "butler:system",
        "task_epoch": 1,
        "delegation_depth": 0,
        "authorization": [
            {"verb": "task.collect", "klass": "safe"},
            {"verb": "agent.spawn", "klass": "mutation"}
        ],
        "budget": {"max_tokens": 100000, "max_tool_calls": 50},
        "deadline": null,
        "members": [
            {"agent_id": "agent_01JAAAAAAAAAAAAAAAAAAAAAB5",
             "role": "worker", "grant_id": "grant_01JAAAAAAAAAAAAAAAAAAAAAB6",
             "joined_seq": 5}
        ],
        "parent_task_id": null,
        "created_at": "2026-08-29T11:00:01.000Z",
        "updated_at": "2026-08-29T11:00:02.000Z"
    });
    validate(registries::TASK_SCHEMA, &task).expect("Task 对象合法");

    // 非法状态必须被拒(状态机外状态)
    let mut bad = task.clone();
    bad["state"] = json!("stalled");
    assert!(
        validate(registries::TASK_SCHEMA, &bad).is_err(),
        "stalled 是监护态非状态机状态,不得入 Task.state"
    );

    // M6 启用:parent_task_id 放宽为 task 引用;delegation_depth 必填且 ≤3
    let mut with_parent = task.clone();
    with_parent["parent_task_id"] = json!("task_01JAAAAAAAAAAAAAAAAAAAAAB1");
    with_parent["delegation_depth"] = json!(1);
    assert!(
        validate(registries::TASK_SCHEMA, &with_parent).is_ok(),
        "M6:parent_task_id + delegation_depth 合法"
    );
    let mut bad = task;
    bad["delegation_depth"] = json!(4);
    assert!(
        validate(registries::TASK_SCHEMA, &bad).is_err(),
        "委派深度上限 3(M6.5)"
    );
}

#[test]
fn wire_task_params_and_results_validate() {
    let create_params = json!({
        "title": "整理读书笔记", "goal": "把 inbox 笔记归档到 notes 并复核",
        "authorization": [{"verb": "task.collect", "klass": "safe"},
                          {"verb": "agent.stop", "klass": "mutation"}],
        "budget": {"max_tokens": 100000}, "deadline": null
    });
    validate_by_pointer(
        registries::WIRE_TASK_SCHEMA,
        "#/task.create/params",
        &create_params,
    )
    .expect("task.create params 合法");

    // 超长 title 必须被拒(合同 maxLength)
    let mut bad = create_params.clone();
    bad["title"] = json!("x".repeat(201));
    assert!(
        validate_by_pointer(registries::WIRE_TASK_SCHEMA, "#/task.create/params", &bad).is_err(),
        "title 超长必须被拒"
    );

    let create_result = json!({
        "task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2",
        "state": "created", "created_at": "2026-08-29T11:00:01.000Z"
    });
    validate_by_pointer(
        registries::WIRE_TASK_SCHEMA,
        "#/task.create/result",
        &create_result,
    )
    .expect("task.create result 合法");

    for m in ["task.pause", "task.resume", "task.stop"] {
        let r = json!({"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2", "state": "running"});
        validate_by_pointer(registries::WIRE_TASK_SCHEMA, &format!("#/{m}/result"), &r)
            .unwrap_or_else(|e| panic!("{m} result 应合法: {e}"));
    }
}

#[test]
fn memory_entry_validates() {
    let entry = json!({
        "entry_id": "mem_01JAAAAAAAAAAAAAAAAAAAAAC5",
        "scope": "memory:task:task_01JAAAAAAAAAAAAAAAAAAAAAB2",
        "content_ref": "protected://mem/01JAAAAAAAAAAAAAAAAAAAAAC5",
        "content_digest_preview": "归档偏好:按年目录",
        "source_trust": "agent-derived",
        "source_ref": "op_01JAAAAAAAAAAAAAAAAAAAAAB8",
        "content_hash": "sha256:1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
        "correction_of": null,
        "created_at": "2026-08-29T11:00:05.000Z",
        "tombstoned": false
    });
    validate(registries::MEMORY_ENTRY_SCHEMA, &entry).expect("memory 条目合法");

    // scope 枚举面:非法域必须被拒
    let mut bad = entry.clone();
    bad["scope"] = json!("memory:global");
    assert!(
        validate(registries::MEMORY_ENTRY_SCHEMA, &bad).is_err(),
        "scope 即权限边界,未定义域必须被拒"
    );

    // 未知信任级别必须被拒
    let mut bad = entry;
    bad["source_trust"] = json!("very-trusted");
    assert!(
        validate(registries::MEMORY_ENTRY_SCHEMA, &bad).is_err(),
        "trust 三级之外必须被拒"
    );
}

#[test]
fn observation_log_entry_validates() {
    let entry = json!({
        "log_seq": 1,
        "task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2",
        "agent_id": "agent_01JAAAAAAAAAAAAAAAAAAAAAB5",
        "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAB8",
        "claim_summary": "Worker 声称归档完成",
        "evidence": [
            {"kind": "receipt", "ref": "op_01JAAAAAAAAAAAAAAAAAAAAAB8"},
            {"kind": "state_check", "ref": "notes/2026/archived.md exists"}
        ],
        "verdict": "verified",
        "guard_state": "completed",
        "verification_hook": {"query": "system.notes.read", "expect": "exists", "within_ms": 2000},
        "observed_at": "2026-08-29T11:00:04.000Z"
    });
    validate(registries::OBSERVATION_LOG_SCHEMA, &entry).expect("observation 条目合法");

    // 完成判定门禁面:unverified 是合法记录,但不得没有证据
    let mut bad = entry.clone();
    bad["evidence"] = json!([]);
    assert!(
        validate(registries::OBSERVATION_LOG_SCHEMA, &bad).is_err(),
        "无证据的观察记录必须被拒(minItems=1)"
    );

    // 非法 verdict 必须被拒
    let mut bad = entry;
    bad["verdict"] = json!("agent-said-so");
    assert!(
        validate(registries::OBSERVATION_LOG_SCHEMA, &bad).is_err(),
        "verdict 三值之外必须被拒"
    );
}

#[test]
fn task_state_machine_terminal_and_paused_edges() {
    // M5 增发面:完成判定门禁——无 verified guard 不得进 completed
    assert!(TaskState::can_transition(
        TaskState::Running,
        TaskState::Completed
    ));
    assert!(
        !TaskState::can_transition(TaskState::Blocked, TaskState::Completed),
        "blocked 无直达 completed 边(须先 user_resolved)"
    );
    assert!(
        !TaskState::can_transition(TaskState::Created, TaskState::Completed),
        "created 无直达 completed 边"
    );
    // blocked 自动出口封死(ADR-0004 条件 6:硬顶后等待用户)
    assert!(TaskState::can_transition(
        TaskState::Blocked,
        TaskState::Running
    ));
    assert!(TaskState::can_transition(
        TaskState::Blocked,
        TaskState::Cancelled
    ));
    // agent paused 四边
    assert!(AgentState::can_transition(
        AgentState::Running,
        AgentState::Paused
    ));
    assert!(AgentState::can_transition(
        AgentState::Paused,
        AgentState::Running
    ));
    assert!(AgentState::can_transition(
        AgentState::Paused,
        AgentState::Stopping
    ));
    assert!(AgentState::can_transition(
        AgentState::Paused,
        AgentState::Cancelled
    ));
    // paused 非终态
    assert!(!AgentState::Paused.is_terminal());
    // M8 序列化形态:paused 与 task 状态字符串与合同一致
    assert_eq!(AgentState::Paused.as_str(), "paused");
    assert_eq!(TaskState::Blocked.as_str(), "blocked");
}

// ---- M7:MCP server 配置合同 -----------------------------------------------

#[test]
fn mcp_server_config_schema_accepts_minimal_and_rejects_bad() {
    let ok = json!({
        "name": "notes", "transport": "stdio",
        "command": "python", "args": ["-m", "notes_mcp"],
        "env": {"NOTES_TOKEN": "secret:notes.token"},
        "tool_timeout_ms": 20000, "restart_limit": 3,
        "trust": "explicit-config"
    });
    validate(registries::MCP_SERVER_SCHEMA, &ok).expect("mcp 配置合法");

    let bad = json!({
        "name": "Bad-Name", "transport": "stdio",
        "command": "x", "args": []
    });
    validate(registries::MCP_SERVER_SCHEMA, &bad).expect_err("server 名字符集必须拒连字符");

    let leak = json!({
        "name": "notes", "transport": "stdio",
        "command": "x", "args": [],
        "env": {"NOTES_TOKEN": "sk-plaintext"}
    });
    validate(registries::MCP_SERVER_SCHEMA, &leak).expect_err("env 明文必须拒绝,只收 secret: 引用");
}

// ---- M8:评估报告合同 -------------------------------------------------------

#[test]
fn evaluation_report_schema_accepts_minimal_and_rejects_bad() {
    let ok = json!({
        "report_id": "rep_01JAAAAAAAAAAAAAAAAAAAAAB2",
        "range": {"from_seq": 1, "to_seq": 42},
        "checks": [
            {"check_id": "inv.single_terminal", "verdict": "pass", "evidence": "ops=7"},
            {"check_id": "receipt.side_effect", "verdict": "fail", "evidence": "seq=31 无 published 行"}
        ],
        "summary": {"passed": 1, "failed": 1, "skipped": 0},
        "judge_version": "0.1.0",
        "generated_at": "2026-08-30T12:00:00.000Z"
    });
    validate(registries::EVALUATION_REPORT_SCHEMA, &ok).expect("评估报告合法");

    let bad = json!({
        "report_id": "rep_01JAAAAAAAAAAAAAAAAAAAAAB2",
        "range": {"from_seq": 1, "to_seq": 42},
        "checks": [],
        "summary": {"passed": 0, "failed": 0, "skipped": 0},
        "judge_version": "0.1.0",
        "generated_at": "2026-08-30T12:00:00.000Z",
        "notes": "自由文本字段不允许(脱敏纪律,合同结构阻止)"
    });
    validate(registries::EVALUATION_REPORT_SCHEMA, &bad).expect_err("自由文本字段必须拒");
}
