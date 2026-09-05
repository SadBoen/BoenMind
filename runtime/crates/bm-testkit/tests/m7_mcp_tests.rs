//! M7-T3:MCP 接入(握手/发现/动态注册/异步收据/进度/超时;InProc 传输)。
//! GT-05 行为面的测试实现;t104 为 stdio 真子进程(#[ignore],环境门控)。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{BmId, IdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::{
    ApprovalListParams, ApprovalRespondParams, CapabilityCallParams, GetOperationParams,
};
use bm_providers::mcp::{Behavior, InProcMcpServer, McpHub, McpToolDef};
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

/// destructiveHint → external-side-effect + required(未知风险首调审批)。
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
            .operations_get(GetOperationParams {
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

fn call_params(capability: &str, args: serde_json::Value) -> CapabilityCallParams {
    CapabilityCallParams {
        capability: capability.to_string(),
        args,
        idempotency_key: None,
        deadline_ms: Some(30_000),
    }
}

/// t100:握手与发现——initialize/tools/list → manifest 动态注册;
/// 不合规工具名拒注册;read-only 工具对 trusted 调用方直通。
#[tokio::test]
async fn t100_mcp_handshake_discovery_and_registration() {
    let server = InProcMcpServer::new(vec![
        tool("search", true),
        tool("write", false),
        McpToolDef {
            name: "bad-Name!".into(),
            description: None,
            input_schema: json!({}),
            annotations: json!({}),
        },
    ]);
    let (rig, _hub) = rig_with_server(server.clone()).await;

    // 发现面:read-only 工具对 trusted 直通(read-only × not-required)
    let req = rig.ids.next_id("req");
    let result = rig
        .handle
        .capability_call(req, call_params("mcp.notes.search", json!({"q": "x"})))
        .await
        .expect("直通成功");
    assert_eq!(result["state"], "running");
    let op_id = BmId::parse(result["operation_id"].as_str().expect("op")).expect("BmId");
    let done = wait_terminal(&rig, &op_id).await;
    assert_eq!(done.state, OperationState::Succeeded);

    // 拒注册面:bad-Name! 未进入能力面 → 默认拒绝
    let req = rig.ids.next_id("req");
    let err = rig
        .handle
        .capability_call(req, call_params("mcp.notes.bad_name", json!({})))
        .await
        .expect_err("未知能力必须拒绝");
    assert_eq!(err.to_wire().code.get(), ErrorCode::PermissionDenied);

    rig.stop().await;
}

/// t101:MCP 工具调用全链路——首调审批 → Grant → 异步完成 →
/// 收据/幂等抑制/outbox published(M4 对账底座在异步路径成立)。
#[tokio::test]
async fn t101_mcp_call_receipt_idempotency_outbox() {
    let server = InProcMcpServer::new(vec![tool("search", true), tool_destructive("publish")]);
    server.set_behavior(
        "publish",
        Behavior::done(json!({"content": [{"type": "text", "text": "published"}]})),
    );
    let (rig, _hub) = rig_with_server(server.clone()).await;

    // 首调 write(reversible × approval=required)→ approval_required
    let req = rig.ids.next_id("req");
    let mut params = call_params("mcp.notes.publish", json!({"topic": "x"}));
    params.idempotency_key = Some("m7-t101-1".into());
    let err = rig
        .handle
        .capability_call(req, params.clone())
        .await
        .expect_err("首调必须审批");
    match &err {
        bm_core::CoreError::ApprovalNeeded { .. } => {}
        other => panic!("应为 approval_required(结构化 ApprovalNeeded),实际 {other:?}"),
    }

    // 批准(scope=count:5,为幂等重放留余量)
    let list = rig
        .handle
        .approval_list(ApprovalListParams { state_filter: None })
        .await
        .expect("审批列表");
    let approvals = list["approvals"].as_array().unwrap();
    let approval_id = approvals[0]["approval_id"].as_str().unwrap().to_string();
    let resp = rig
        .handle
        .approval_respond(
            rig.ids.next_id("req"),
            ApprovalRespondParams {
                approval_id: BmId::parse(&approval_id).expect("appr id"),
                decision: "approve".into(),
                scope: Some("count:5".into()),
            },
        )
        .await
        .expect("批准成功");
    assert_eq!(resp["state"], json!("approved"));

    // 批准后重放即异步派发(InProc 工具完成极快);原操作经
    // Cmd::ProviderCall 落定 succeeded
    let events = rig.all_events().await;
    let requested = events
        .iter()
        .find(|e| e.event_type == EventType::ApprovalRequested)
        .expect("approval.requested 存在");
    let op_id = BmId::parse(requested.payload["operation_id"].as_str().expect("op")).expect("BmId");
    let done = wait_terminal(&rig, &op_id).await;
    assert_eq!(done.state, OperationState::Succeeded);

    // 审计:capability.invoked outcome=ok
    let events = rig.all_events().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::CapabilityInvoked
                && e.payload["capability"] == "mcp.notes.publish"
                && e.payload["outcome"] == "ok")
    );

    // 幂等:同 key 重放 → 抑制返回原结果(不再执行)
    let req = rig.ids.next_id("req");
    let again = rig
        .handle
        .capability_call(req, params)
        .await
        .expect("幂等重放");
    assert_eq!(again["state"], "succeeded");
    assert_eq!(again["action_summary"], "幂等抑制:等价请求返回原收据");
    let events = rig.all_events().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::CapabilityInvoked
                && e.payload["outcome"] == "suppressed"),
        "必须留 suppressed 审计"
    );

    // outbox 对账底座:published 行存在(第二连接只读;运行中查,
    // 停机后临时目录即清理)
    let dir = rig.data_dir.clone().expect("data_dir");
    {
        let store = bm_persist::PersistStore::open(dir.as_path()).expect("打开状态库");
        let published =
            bm_persist::EventStore::list_outbox_by_state(&store, "published").expect("outbox 行");
        assert!(
            published.iter().any(|r| r["payload"]
                .as_str()
                .map(|p| p.contains("mcp.notes.publish"))
                .unwrap_or(false)),
            "MCP 副作用必须有 published 对账行:{published:?}"
        );
    }
    rig.stop().await;
}

/// t102:MCP notifications/progress → capability.progress 事件。
#[tokio::test]
async fn t102_mcp_progress_events() {
    let server = InProcMcpServer::new(vec![tool("search", true)]);
    server.set_behavior(
        "search",
        Behavior {
            progress: vec![(1, Some(2), "scanning".into()), (2, Some(2), "done".into())],
            ..Behavior::done(json!({"content": [{"type": "text", "text": "r"}]}))
        },
    );
    let (rig, _hub) = rig_with_server(server.clone()).await;

    let req = rig.ids.next_id("req");
    let receipt = rig
        .handle
        .capability_call(req, call_params("mcp.notes.search", json!({})))
        .await
        .expect("派发");
    let op_id = BmId::parse(receipt["operation_id"].as_str().expect("op")).expect("BmId");
    let done = wait_terminal(&rig, &op_id).await;
    assert_eq!(done.state, OperationState::Succeeded);

    let events = rig.all_events().await;
    let progress: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::CapabilityProgress)
        .collect();
    assert_eq!(progress.len(), 2, "两条进度通知各落一事件");
    assert_eq!(progress[0].payload["progress"], 1);
    assert_eq!(progress[0].payload["total"], 2);
    assert_eq!(progress[0].payload["message"], "scanning");
    assert_eq!(progress[1].payload["operation_id"], op_id.as_str());

    rig.stop().await;
}

/// t103:MCP 调用超时——deadline 到点 Failed{timeout},不无限等待;
/// 审计 outcome=error error_code=timeout(GT-05 B2 的超时变体)。
#[tokio::test]
async fn t103_mcp_timeout_fails_within_deadline() {
    let server = InProcMcpServer::new(vec![tool("search", true)]);
    server.set_behavior(
        "search",
        Behavior {
            delay_ms: 5_000,
            ..Behavior::done(json!({"content": [{"type": "text", "text": "late"}]}))
        },
    );
    // 连接时声明 300ms 工具超时(manifest.timeout_ms → dispatch deadline)
    let hub = McpHub::new();
    let manifests = hub.connect("notes", server, 300).await.expect("握手成功");
    let entries = McpHub::capability_entries(manifests);
    let rig = TestRig::standard_with(vec![], entries, Some(hub.clone())).await;

    let started = std::time::Instant::now();
    let req = rig.ids.next_id("req");
    let receipt = rig
        .handle
        .capability_call(req, call_params("mcp.notes.search", json!({})))
        .await
        .expect("派发");
    let op_id = BmId::parse(receipt["operation_id"].as_str().expect("op")).expect("BmId");
    let done = wait_terminal(&rig, &op_id).await;
    assert_eq!(done.state, OperationState::Failed);
    let err = done.error.expect("失败收据带错误");
    assert_eq!(err.code.get(), ErrorCode::Timeout);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "不得等满工具延迟"
    );

    let events = rig.all_events().await;
    assert!(
        events
            .iter()
            .any(|e| e.event_type == EventType::CapabilityInvoked
                && e.payload["outcome"] == "error"
                && e.payload["error_code"] == "timeout")
    );

    rig.stop().await;
}

/// t104:stdio 真子进程握手与调用(#[ignore];BOEN_MCP_STDIO_TEST=1 启用)。
/// fixture = fixtures/mini_mcp.py(零外部依赖,零密钥)。
#[tokio::test]
#[ignore = "stdio 子进程测试:BOEN_MCP_STDIO_TEST=1 启用"]
async fn t104_mcp_stdio_real_subprocess() {
    if std::env::var("BOEN_MCP_STDIO_TEST").as_deref() != Ok("1") {
        eprintln!("跳过:BOEN_MCP_STDIO_TEST 未设");
        return;
    }
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/mini_mcp.py");
    let transport = bm_providers::mcp::StdioMcpTransport::spawn(
        "python",
        &[fixture.to_string_lossy().to_string()],
        &Default::default(),
    )
    .expect("子进程启动");
    let hub = McpHub::new();
    let manifests = tokio::time::timeout(
        Duration::from_secs(15),
        hub.connect("mini", transport, 10_000),
    )
    .await
    .expect("stdio 握手 15s 超时(夹具或传输挂起)")
    .expect("stdio 握手成功");
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].capability, "mcp.mini.ping");

    let out = bm_core::ports::AsyncCapabilityExecutor::call(
        hub.as_ref(),
        "op_test",
        "mcp.mini.ping",
        json!({}),
        Duration::from_secs(5),
    )
    .await
    .expect("stdio 工具调用成功");
    assert_eq!(out["text"], "pong");
}
