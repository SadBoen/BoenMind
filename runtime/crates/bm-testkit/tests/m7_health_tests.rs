//! M7-T4:Provider 健康面(HTTP 熔断/MCP 崩溃重连/重连上限)、安装信任、
//! 首调审批与 Grant 扩量、App 主体数据域隔离。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{BmId, IdGen};
use bm_contract::wire::{ApprovalListParams, ApprovalRespondParams, CapabilityCallParams};
use bm_providers::mcp::{Behavior, InProcMcpServer, McpHub, McpToolDef};
use bm_providers::mock_model::Step;
use bm_testkit::replay::TestRig;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

fn tool(name: &str, read_only: bool) -> McpToolDef {
    McpToolDef {
        name: name.to_string(),
        description: None,
        input_schema: json!({"type": "object"}),
        annotations: json!({ "readOnlyHint": read_only }),
    }
}

fn tool_destructive(name: &str) -> McpToolDef {
    McpToolDef {
        name: name.to_string(),
        description: None,
        input_schema: json!({"type": "object"}),
        annotations: json!({ "destructiveHint": true }),
    }
}

async fn rig_with_server(server: Arc<InProcMcpServer>) -> (TestRig, Arc<McpHub>) {
    let hub = McpHub::new();
    let manifests = hub
        .connect("notes", server, 30_000)
        .await
        .expect("握手与发现成功");
    let entries = McpHub::capability_entries(manifests);
    let rig = TestRig::standard_with(vec![], entries, Some(hub.clone())).await;
    (rig, hub)
}

async fn wait_terminal(rig: &TestRig, op_id: &BmId) -> bm_contract::wire::Receipt {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "异步调用 10s 未终态"
        );
        let r = rig
            .handle
            .operations_get(bm_contract::wire::GetOperationParams {
                operation_id: op_id.clone(),
            })
            .await
            .expect("收据查询");
        if r.state.is_terminal() {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn call_params(capability: &str, key: Option<&str>) -> CapabilityCallParams {
    CapabilityCallParams {
        capability: capability.to_string(),
        args: json!({}),
        idempotency_key: key.map(String::from),
        deadline_ms: Some(30_000),
    }
}

async fn call_and_settle(
    rig: &TestRig,
    capability: &str,
    key: Option<&str>,
) -> bm_contract::wire::Receipt {
    let req = rig.ids.next_id("req");
    let receipt = rig
        .handle
        .capability_call(req, call_params(capability, key))
        .await
        .expect("派发成功");
    let op_id = BmId::parse(receipt["operation_id"].as_str().expect("op")).expect("BmId");
    wait_terminal(rig, &op_id).await
}

/// t105:HTTP 模型连接器熔断——连续 3 次失败开闸,冷却期内快速失败
/// (不触连接器),冷却后放行半开探测,成功恢复 healthy。
#[tokio::test]
async fn t105_model_provider_circuit_breaker() {
    let fail = || Step::Fail {
        error_code: ErrorCode::Unavailable,
        retryable: true,
    };
    let script = vec![fail(), fail(), fail(), Step::ok("恢复答", 10, 5)];
    let rig = TestRig::standard(script).await;

    // 三模型链:单回合 3 次尝试全部失败 → streak 3,熔断开闸
    let s1 = rig
        .handle
        .session_create(
            rig.ids.next_id("req"),
            bm_contract::wire::SessionCreateParams {
                agent: bm_contract::wire::AgentSpec {
                    name: "a1".into(),
                    model_chain: vec![
                        bm_testkit::replay::MODEL_A.into(),
                        bm_testkit::replay::MODEL_B.into(),
                        bm_testkit::replay::MODEL_A.into(),
                    ],
                    budget: rig.budget(),
                },
            },
        )
        .await
        .expect("三模型链会话");
    let (sess1, agent1) = (s1.session_id, s1.agent_id);
    let r1 = rig.send(&sess1, &agent1, "问1").await.expect("回合发起");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let done = loop {
        let r = rig
            .handle
            .operations_get(bm_contract::wire::GetOperationParams {
                operation_id: r1.operation_id.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            break r;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    assert_eq!(done.state, bm_contract::states::OperationState::Failed);
    let events = rig.all_events().await;
    let opened: Vec<_> = events
        .iter()
        .filter(|e| {
            e.event_type == EventType::ProviderHealthChanged && e.payload["to"] == "unavailable"
        })
        .collect();
    assert_eq!(opened.len(), 1, "恰好一次开闸事件");
    assert_eq!(opened[0].payload["provider"], "mock");
    assert_eq!(opened[0].payload["from"], "healthy");
    let failed_count = |events: &[bm_contract::events::EventEnvelope]| {
        events
            .iter()
            .filter(|e| e.event_type == EventType::ModelInvocationFailed)
            .count()
    };
    assert_eq!(failed_count(&events), 3, "三次尝试各一条失败事件(INV-4)");

    // 新会话发回合:冷却期内 → 快速失败,不触连接器(0 条新的模型失败事件)
    let created = rig
        .handle
        .session_create(
            rig.ids.next_id("req"),
            bm_contract::wire::SessionCreateParams {
                agent: bm_contract::wire::AgentSpec {
                    name: "assistant2".into(),
                    model_chain: vec![
                        bm_testkit::replay::MODEL_A.into(),
                        bm_testkit::replay::MODEL_B.into(),
                    ],
                    budget: rig.budget(),
                },
            },
        )
        .await
        .expect("第二会话");
    let r2 = rig
        .send(&created.session_id, &created.agent_id, "问2")
        .await
        .expect("回合发起");
    let done = loop {
        let r = rig
            .handle
            .operations_get(bm_contract::wire::GetOperationParams {
                operation_id: r2.operation_id.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            break r;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    assert_eq!(done.state, bm_contract::states::OperationState::Failed);
    let err = done.error.expect("带错误");
    assert_eq!(err.code.get(), ErrorCode::Unavailable);
    let events = rig.all_events().await;
    assert_eq!(failed_count(&events), 3, "熔断期内不得产生新的模型调用尝试");

    // 冷却过期 → 半开探测成功 → 恢复 healthy
    rig.clock.advance_ms(31_000);
    let created = rig
        .handle
        .session_create(
            rig.ids.next_id("req"),
            bm_contract::wire::SessionCreateParams {
                agent: bm_contract::wire::AgentSpec {
                    name: "assistant3".into(),
                    model_chain: vec![
                        bm_testkit::replay::MODEL_A.into(),
                        bm_testkit::replay::MODEL_B.into(),
                    ],
                    budget: rig.budget(),
                },
            },
        )
        .await
        .expect("第三会话");
    let r3 = rig
        .send(&created.session_id, &created.agent_id, "问3")
        .await
        .expect("回合发起");
    let done = loop {
        let r = rig
            .handle
            .operations_get(bm_contract::wire::GetOperationParams {
                operation_id: r3.operation_id.clone(),
            })
            .await
            .expect("查询");
        if r.state.is_terminal() {
            break r;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    assert_eq!(done.state, bm_contract::states::OperationState::Succeeded);
    let events = rig.all_events().await;
    let recovered: Vec<_> = events
        .iter()
        .filter(|e| {
            e.event_type == EventType::ProviderHealthChanged && e.payload["to"] == "healthy"
        })
        .collect();
    assert_eq!(recovered.len(), 1, "半开探测成功恢复");
    rig.stop().await;
}

/// t106:MCP 崩溃→unavailable→重连恢复;重连超限(3 次探针)后
/// 快速失败且不再触达执行器(直至重装)。
#[tokio::test]
async fn t106_mcp_crash_reconnect_and_limit() {
    // —— 恢复路径:一次故障后探针成功 ——
    let server = InProcMcpServer::new(vec![tool("search", true)]);
    server.set_behavior(
        "search",
        Behavior {
            result: Some(Err("stdio-closed".into())),
            ..Default::default()
        },
    );
    let (rig, _hub) = rig_with_server(server.clone()).await;

    // 调用 1(健康态):传输故障 → unavailable 立即
    let done = call_and_settle(&rig, "mcp.notes.search", None).await;
    assert_eq!(done.state, bm_contract::states::OperationState::Failed);
    let events = rig.all_events().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::ProviderHealthChanged
                && e.payload["from"] == "healthy"
                && e.payload["to"] == "unavailable")
    );

    // 调用 2(unavailable,探针 1):仍故障
    let done = call_and_settle(&rig, "mcp.notes.search", None).await;
    assert_eq!(done.state, bm_contract::states::OperationState::Failed);

    // 修复工具 → 探针 2 成功 → 恢复 healthy
    server.set_behavior(
        "search",
        Behavior::done(json!({"content": [{"type": "text", "text": "r"}]})),
    );
    let done = call_and_settle(&rig, "mcp.notes.search", None).await;
    assert_eq!(done.state, bm_contract::states::OperationState::Succeeded);
    let events = rig.all_events().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::ProviderHealthChanged
                && e.payload["from"] == "unavailable"
                && e.payload["to"] == "healthy"
                && e.payload["reason"] == "重连握手成功")
    );
    rig.stop().await;

    // —— 封禁路径:探针耗尽后快速失败 ——
    let server2 = InProcMcpServer::new(vec![tool("search", true)]);
    server2.set_behavior(
        "search",
        Behavior {
            result: Some(Err("stdio-closed".into())),
            ..Default::default()
        },
    );
    let (rig2, _hub2) = rig_with_server(server2.clone()).await;
    // 调用 1:开闸(attempts=0);调用 2-4:探针 1-3
    for _ in 0..4 {
        let _ = call_and_settle(&rig2, "mcp.notes.search", None).await;
    }
    assert_eq!(server2.call_count("search"), 4, "4 次真实触达");
    // 调用 5:重连超限 → 快速失败(同步拒绝,不触执行器)
    let req = rig2.ids.next_id("req");
    let err = rig2
        .handle
        .capability_call(req, call_params("mcp.notes.search", None))
        .await
        .expect_err("超限必须快速失败");
    match &err {
        bm_core::CoreError::Semantic(code, _) => assert_eq!(*code, ErrorCode::Unavailable),
        other => panic!("应为 unavailable,实际 {other:?}"),
    }
    assert_eq!(
        server2.call_count("search"),
        4,
        "封禁后不得再触达执行器(直至重装)"
    );
    rig2.stop().await;
}

/// t107:安装信任——配置文件过合同校验、env 只收 secret: 引用并解析;
/// 未安装的能力默认拒绝(M7.7,S6)。
#[tokio::test]
async fn t107_mcp_install_trust_and_config() {
    let dir = tempfile::tempdir().expect("临时目录");
    let cfg_path = dir.path().join("mcp.json");
    std::fs::write(
        &cfg_path,
        json!([{
            "name": "notes",
            "transport": "stdio",
            "command": "python",
            "args": ["-c", "print(1)"],
            "env": {"NOTES_TOKEN": "secret:notes.token"},
            "trust": "explicit-config"
        }])
        .to_string(),
    )
    .expect("写配置");
    let store = bm_providers::secret::MemSecretStore::with("secret:notes.token", "resolved-value");
    let setups = bm_providers::mcp::load_mcp_setups(&cfg_path, &store).expect("装载成功");
    assert_eq!(setups.len(), 1);
    assert_eq!(setups[0].env_resolved["NOTES_TOKEN"], "resolved-value");

    // 不合规 server 名 → 拒
    let bad = dir.path().join("bad.json");
    std::fs::write(
        &bad,
        json!([{"name": "Bad-Name", "transport": "stdio", "command": "x", "args": []}]).to_string(),
    )
    .expect("写");
    assert!(bm_providers::mcp::load_mcp_setups(&bad, &store).is_err());

    // env 明文 → 合同拒绝(只收 secret: 引用)
    let leak = dir.path().join("leak.json");
    std::fs::write(
        &leak,
        json!([{"name": "notes", "transport": "stdio", "command": "x", "args": [],
                "env": {"T": "sk-plaintext"}}])
        .to_string(),
    )
    .expect("写");
    assert!(bm_providers::mcp::load_mcp_setups(&leak, &store).is_err());

    // 未安装能力:默认拒绝
    let server = InProcMcpServer::new(vec![tool("search", true)]);
    let (rig, _hub) = rig_with_server(server.clone()).await;
    let req = rig.ids.next_id("req");
    let err = rig
        .handle
        .capability_call(req, call_params("mcp.other.tool", None))
        .await
        .expect_err("未安装能力必须拒绝");
    match &err {
        bm_core::CoreError::Semantic(code, _) => {
            assert_eq!(*code, ErrorCode::PermissionDenied)
        }
        other => panic!("应为 permission_denied,实际 {other:?}"),
    }
    rig.stop().await;
}

/// t108:首调审批闭环 + Grant 扩量语义——count 批准消费殆尽后
/// 再调回到审批(批准可扩量,不静默放行)。
#[tokio::test]
async fn t108_first_call_approval_and_grant_exhaustion() {
    let server = InProcMcpServer::new(vec![tool_destructive("publish")]);
    server.set_behavior(
        "publish",
        Behavior::done(json!({"content": [{"type": "text", "text": "ok"}]})),
    );
    let (rig, _hub) = rig_with_server(server.clone()).await;

    // 首调 → approval_required(未知风险首调审批)
    let req = rig.ids.next_id("req");
    let err = rig
        .handle
        .capability_call(req, call_params("mcp.notes.publish", None))
        .await
        .expect_err("首调必须审批");
    match &err {
        bm_core::CoreError::Semantic(code, _) => {
            assert_eq!(*code, ErrorCode::ApprovalRequired)
        }
        other => panic!("应为 approval_required,实际 {other:?}"),
    }

    // 批准 count:2 → 重放(消费 1)成功
    let list = rig
        .handle
        .approval_list(ApprovalListParams { state_filter: None })
        .await
        .expect("列表");
    let approval_id = list["approvals"][0]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    rig.handle
        .approval_respond(
            rig.ids.next_id("req"),
            ApprovalRespondParams {
                approval_id: BmId::parse(&approval_id).unwrap(),
                decision: "approve".into(),
                scope: Some("once".into()),
            },
        )
        .await
        .expect("批准");
    let events = rig.all_events().await;
    let requested = events
        .iter()
        .find(|e| e.event_type == EventType::ApprovalRequested)
        .expect("approval.requested");
    let op_id = BmId::parse(requested.payload["operation_id"].as_str().unwrap()).unwrap();
    let done = wait_terminal(&rig, &op_id).await;
    assert_eq!(done.state, bm_contract::states::OperationState::Succeeded);

    // once Grant 已被重放消费 → 同键/新键再调都回到审批(不静默放行)
    let req = rig.ids.next_id("req");
    let err = rig
        .handle
        .capability_call(req, call_params("mcp.notes.publish", Some("k3")))
        .await
        .expect_err("Grant 耗尽必须回到审批");
    match &err {
        bm_core::CoreError::Semantic(code, _) => {
            assert_eq!(*code, ErrorCode::ApprovalRequired)
        }
        other => panic!("应为 approval_required,实际 {other:?}"),
    }
    rig.stop().await;
}

/// t109:数据域隔离——App 主体(surface:app:<name>)不享内建直通,
/// 跨 provider 访问默认拒绝、显式 Grant 后放行(结构面单测;BM 侧
/// broker::m7_tests 同题,这里走装配路径复核)。
#[tokio::test]
async fn t109_app_principal_isolation() {
    use bm_contract::ids::SeqIdGen;
    use bm_core::broker::{Broker, CallContext, Decision, GrantLedger};
    use bm_core::registry::CapabilityRegistry;

    let mut reg = CapabilityRegistry::new();
    let m: bm_contract::capability::CapabilityManifest = serde_json::from_value(json!({
        "capability": "mcp.notes.search", "provider": "mcp.notes",
        "version": "0.1.0", "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": "read-only", "idempotent": false, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    }))
    .unwrap();
    reg.register(m, "mcp.notes@0.1.0", bm_core::broker::provider_fn(Ok))
        .unwrap();
    let clock = bm_core::clock::MockClock::at_ms(1_788_000_000_000);
    let mut ledger = GrantLedger::new();
    let ids = SeqIdGen::new();
    let broker = Broker::new(&reg, &mut ledger, &clock, &ids);

    let app = CallContext::surface("surface:app:wiki");
    assert!(matches!(
        broker.decide(&app, "mcp.notes.search", &json!({})),
        Decision::Denied { .. }
    ));
    // 普通用户不受影响
    let user = CallContext::surface("surface:user");
    assert!(matches!(
        broker.decide(&user, "mcp.notes.search", &json!({})),
        Decision::Allowed { .. }
    ));
}

/// t114:capability.cancel 语义——运行中取消 → 收据 cancelled;
/// 取消令牌贯穿传输层(InProc 睡眠中断),迟到完成被丢弃。
#[tokio::test]
async fn t114_capability_cancel_discards_late_completion() {
    let server = InProcMcpServer::new(vec![tool("search", true)]);
    server.set_behavior(
        "search",
        Behavior {
            delay_ms: 5_000,
            ..Behavior::done(json!({"content": [{"type": "text", "text": "late"}]}))
        },
    );
    let (rig, _hub) = rig_with_server(server.clone()).await;

    let req = rig.ids.next_id("req");
    let receipt = rig
        .handle
        .capability_call(req, call_params("mcp.notes.search", None))
        .await
        .expect("派发");
    let op_id = BmId::parse(receipt["operation_id"].as_str().expect("op")).expect("BmId");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let res = rig
        .handle
        .capability_cancel(
            rig.ids.next_id("req"),
            bm_contract::wire::CapabilityCancelParams {
                operation_id: op_id.clone(),
                reason: Some("不想等了".into()),
            },
        )
        .await
        .expect("取消成功");
    assert_eq!(res.state, "cancelled");

    let r = rig
        .handle
        .operations_get(bm_contract::wire::GetOperationParams {
            operation_id: op_id.clone(),
        })
        .await
        .expect("查询");
    assert_eq!(r.state, bm_contract::states::OperationState::Cancelled);

    // 迟到完成丢弃:取消已令传输提前返回,完成回流时 meta 缺失 → 收据不变
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let r2 = rig
        .handle
        .operations_get(bm_contract::wire::GetOperationParams {
            operation_id: op_id.clone(),
        })
        .await
        .expect("查询");
    assert_eq!(
        r2.state,
        bm_contract::states::OperationState::Cancelled,
        "迟到完成不得改写收据"
    );
    rig.stop().await;
}
