//! M8-T1:首批真实 App e2e(Wiki=文件域真实写盘;Market=确定性领域)。
//! ADR-0011:App 以 MCP stdio server 接入,全链路过 Runtime/Broker/Task/日志。
//! 门控:BOEN_APPS_E2E=1(真实 python 子进程)。

use bm_contract::error_codes::ErrorCode;
use bm_contract::events::EventType;
use bm_contract::ids::{BmId, IdGen};
use bm_contract::states::OperationState;
use bm_contract::wire::{ApprovalListParams, ApprovalRespondParams, CapabilityCallParams};
use bm_providers::mcp::{McpHub, StdioMcpTransport};
use bm_testkit::replay::TestRig;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates
    p.pop(); // runtime
    p.pop(); // BoenMind
    p
}

fn app_path(name: &str) -> PathBuf {
    repo_root().join("apps").join(name)
}

fn gated() -> bool {
    if std::env::var("BOEN_APPS_E2E").as_deref() == Ok("1") {
        true
    } else {
        eprintln!("跳过:BOEN_APPS_E2E 未设(真实 python 子进程 e2e)");
        false
    }
}

async fn wait_terminal(rig: &TestRig, op_id: &BmId) -> bm_contract::wire::Receipt {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        assert!(tokio::time::Instant::now() < deadline, "调用 15s 未终态");
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

async fn call_and_settle(
    rig: &TestRig,
    capability: &str,
    args: serde_json::Value,
    key: Option<&str>,
) -> bm_contract::wire::Receipt {
    let req = rig.ids.next_id("req");
    let receipt = rig
        .handle
        .capability_call(
            req,
            CapabilityCallParams {
                capability: capability.to_string(),
                args,
                idempotency_key: key.map(String::from),
                deadline_ms: Some(30_000),
            },
        )
        .await
        .expect("派发成功");
    let op_id = BmId::parse(receipt["operation_id"].as_str().expect("op")).expect("BmId");
    wait_terminal(rig, &op_id).await
}

/// 连接 wiki + market 两 App,装配双 App Runtime。
async fn rig_two_apps(wiki_dir: &std::path::Path) -> (TestRig, Arc<McpHub>) {
    let hub = McpHub::new();
    let mut manifests = Vec::new();
    let wiki_transport = StdioMcpTransport::spawn(
        "python",
        &[
            app_path("wiki_server.py").to_string_lossy().to_string(),
            "--dir".into(),
            wiki_dir.to_string_lossy().to_string(),
        ],
        &Default::default(),
    )
    .expect("wiki 子进程启动");
    manifests.extend(
        hub.connect("wiki", wiki_transport, 30_000)
            .await
            .expect("wiki 握手"),
    );
    let market_transport = StdioMcpTransport::spawn(
        "python",
        &[app_path("market_server.py").to_string_lossy().to_string()],
        &Default::default(),
    )
    .expect("market 子进程启动");
    manifests.extend(
        hub.connect("market", market_transport, 30_000)
            .await
            .expect("market 握手"),
    );
    let entries = McpHub::capability_entries(manifests);
    let rig = TestRig::standard_with(vec![], entries, Some(hub.clone())).await;
    (rig, hub)
}

/// t110:Wiki App 真实写盘——page.write(外部副作用,首调审批)→
/// 磁盘文件真实变更 + outbox 对账行 + 审计。
#[tokio::test]
async fn t110_wiki_real_write_with_receipt() {
    if !gated() {
        return;
    }
    let dir = tempfile::tempdir().expect("临时目录");
    let wiki_dir = dir.path().join("wiki");
    let hub = McpHub::new();
    let transport = StdioMcpTransport::spawn(
        "python",
        &[
            app_path("wiki_server.py").to_string_lossy().to_string(),
            "--dir".into(),
            wiki_dir.to_string_lossy().to_string(),
        ],
        &Default::default(),
    )
    .expect("子进程启动");
    let manifests = hub.connect("wiki", transport, 30_000).await.expect("握手");
    let entries = McpHub::capability_entries(manifests);
    let rig = TestRig::standard_with(vec![], entries, Some(hub.clone())).await;

    // 首调 page.write → approval_required(unknown-risk 首调审批,M7.7)
    let req = rig.ids.next_id("req");
    let err = rig
        .handle
        .capability_call(
            req,
            CapabilityCallParams {
                capability: "mcp.wiki.page.write".into(),
                args: json!({"name": "home", "content": "# 我的第一页"}),
                idempotency_key: Some("m8-t110-1".into()),
                deadline_ms: Some(30_000),
            },
        )
        .await
        .expect_err("首调必须审批");
    match &err {
        bm_core::CoreError::Semantic(code, _) => {
            assert_eq!(*code, ErrorCode::ApprovalRequired)
        }
        other => panic!("应为 approval_required,实际 {other:?}"),
    }

    // 批准 once → 重放 → 真实写盘
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
    assert_eq!(done.state, OperationState::Succeeded);

    // 真实世界副作用:磁盘文件真实存在且内容一致
    let page = wiki_dir.join("home.md");
    let content = std::fs::read_to_string(&page).expect("页面已写盘");
    assert_eq!(content, "# 我的第一页");

    // 执行收据:结果(sha256/bytes)可查 + outbox published 对账行
    let result = rig
        .handle
        .operation_result(op_id)
        .await
        .expect("结果查询")
        .expect("成功调用有结果");
    assert_eq!(result["bytes"], 17, "UTF-8 字节数(收据): 1+1+5×3");
    assert_eq!(result["written"], true);
    assert_eq!(result["sha256"].as_str().map(|s| s.len()), Some(64));

    let store_dir = rig.data_dir.clone().expect("data_dir");
    {
        let store = bm_persist::PersistStore::open(store_dir.as_path()).expect("状态库");
        let published =
            bm_persist::EventStore::list_outbox_by_state(&store, "published").expect("outbox");
        assert!(
            published.iter().any(|r| r["payload"]
                .as_str()
                .map(|p| p.contains("mcp.wiki.page.write"))
                .unwrap_or(false)),
            "真实副作用必须有 published 对账行"
        );
    }
    rig.stop().await;
}

/// t111:Wiki 只读路径——直通读取/列表;未读文件 isError → failed(internal)。
#[tokio::test]
async fn t111_wiki_read_paths() {
    if !gated() {
        return;
    }
    let dir = tempfile::tempdir().expect("临时目录");
    let wiki_dir = dir.path().join("wiki");
    std::fs::create_dir_all(&wiki_dir).expect("建目录");
    std::fs::write(wiki_dir.join("a.md"), "alpha").expect("预置页面");
    let hub = McpHub::new();
    let transport = StdioMcpTransport::spawn(
        "python",
        &[
            app_path("wiki_server.py").to_string_lossy().to_string(),
            "--dir".into(),
            wiki_dir.to_string_lossy().to_string(),
        ],
        &Default::default(),
    )
    .expect("子进程启动");
    let manifests = hub.connect("wiki", transport, 30_000).await.expect("握手");
    let entries = McpHub::capability_entries(manifests);
    let rig = TestRig::standard_with(vec![], entries, Some(hub.clone())).await;

    // 只读直通:trusted × read-only × not-required(读已存在页面)
    let done = call_and_settle(&rig, "mcp.wiki.page.read", json!({"name": "a"}), None).await;
    assert_eq!(done.state, OperationState::Succeeded);
    let result = rig
        .handle
        .operation_result(done.operation_id)
        .await
        .expect("结果")
        .expect("有结果");
    assert_eq!(result["bytes"], 5);

    // 列表
    let done = call_and_settle(&rig, "mcp.wiki.page.list", json!({}), None).await;
    assert_eq!(done.state, OperationState::Succeeded);
    let result = rig
        .handle
        .operation_result(done.operation_id)
        .await
        .expect("结果")
        .expect("有结果");
    assert_eq!(result["pages"], json!(["a"]));

    // 读不存在页面 → 工具级失败 → failed(internal,不挂起)
    let done = call_and_settle(&rig, "mcp.wiki.page.read", json!({"name": "ghost"}), None).await;
    assert_eq!(done.state, OperationState::Failed);
    let err = done.error.expect("带错误");
    assert_eq!(err.code.get(), ErrorCode::Internal);
    rig.stop().await;
}

/// t112:Market App 确定性——同查询两次,结果逐字段一致(整数分记账);
/// portfolio 可逆路径:add → value 纯计算反映持仓。
#[tokio::test]
async fn t112_market_determinism_and_portfolio() {
    if !gated() {
        return;
    }
    let hub = McpHub::new();
    let transport = StdioMcpTransport::spawn(
        "python",
        &[app_path("market_server.py").to_string_lossy().to_string()],
        &Default::default(),
    )
    .expect("子进程启动");
    let manifests = hub
        .connect("market", transport, 30_000)
        .await
        .expect("握手");
    let entries = McpHub::capability_entries(manifests);
    let rig = TestRig::standard_with(vec![], entries, Some(hub.clone())).await;

    // 确定性:同查询两次
    let done1 = call_and_settle(
        &rig,
        "mcp.market.quote.get",
        json!({"symbol": "ACME"}),
        None,
    )
    .await;
    assert_eq!(done1.state, OperationState::Succeeded);
    let r1 = rig
        .handle
        .operation_result(done1.operation_id)
        .await
        .expect("结果")
        .expect("有结果");
    let done2 = call_and_settle(
        &rig,
        "mcp.market.quote.get",
        json!({"symbol": "ACME"}),
        None,
    )
    .await;
    let r2 = rig
        .handle
        .operation_result(done2.operation_id)
        .await
        .expect("结果")
        .expect("有结果");
    assert_eq!(
        serde_json::to_string(&r1).unwrap(),
        serde_json::to_string(&r2).unwrap(),
        "同查询必须逐字节同结果"
    );
    assert_eq!(r1["price_cents"], 4217);

    // 可逆路径:add(可逆 → 首调审批)→ value 反映持仓
    let req = rig.ids.next_id("req");
    let err = rig
        .handle
        .capability_call(
            req,
            CapabilityCallParams {
                capability: "mcp.market.portfolio.add".into(),
                args: json!({"symbol": "ACME", "qty": 2}),
                idempotency_key: None,
                deadline_ms: Some(30_000),
            },
        )
        .await
        .expect_err("可逆写首调必须审批");
    match &err {
        bm_core::CoreError::Semantic(code, _) => {
            assert_eq!(*code, ErrorCode::ApprovalRequired)
        }
        other => panic!("应为 approval_required,实际 {other:?}"),
    }
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
                scope: Some("count:5".into()),
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
    assert_eq!(done.state, OperationState::Succeeded);

    // 组合市值:2 × 4217 = 8434 分(纯计算)
    let done = call_and_settle(&rig, "mcp.market.portfolio.value", json!({}), None).await;
    assert_eq!(done.state, OperationState::Succeeded);
    let result = rig
        .handle
        .operation_result(done.operation_id)
        .await
        .expect("结果")
        .expect("有结果");
    assert_eq!(result["value_cents"], 8434);
    rig.stop().await;
}

/// t113:双 App 同 Runtime 共存——命名空间与 scopes 域隔离,
/// 同一套 Broker/Task/日志机制。
#[tokio::test]
async fn t113_two_apps_one_runtime() {
    if !gated() {
        return;
    }
    let dir = tempfile::tempdir().expect("临时目录");
    let (rig, _hub) = rig_two_apps(dir.path()).await;

    // 双 App 均可调用(wiki 只读 + market 只读)
    let done = call_and_settle(&rig, "mcp.wiki.page.list", json!({}), None).await;
    assert_eq!(done.state, OperationState::Succeeded);
    let done = call_and_settle(
        &rig,
        "mcp.market.quote.get",
        json!({"symbol": "GLOBEX"}),
        None,
    )
    .await;
    assert_eq!(done.state, OperationState::Succeeded);

    // 事件流同源:两 App 的调用审计在同一条事件日志(同 seq 空间)
    let events = rig.all_events().await;
    let caps: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == EventType::CapabilityInvoked && e.payload["outcome"] == "ok")
        .map(|e| e.payload["capability"].as_str().unwrap().to_string())
        .collect();
    assert!(
        caps.iter().any(|c| c.starts_with("mcp.wiki."))
            && caps.iter().any(|c| c.starts_with("mcp.market.")),
        "两 App 审计必须同源共存:{caps:?}"
    );
    rig.stop().await;
}

/// t123(外部审计 X-01 验收):Wiki 符号链接越界写/读必须被拒。
/// (Windows 建链可能无权限;无权限环境自动跳过)
#[tokio::test]
async fn t123_wiki_symlink_escape_rejected() {
    if !gated() {
        return;
    }
    let dir = tempfile::tempdir().expect("临时目录");
    let wiki_dir = dir.path().join("wiki");
    std::fs::create_dir_all(&wiki_dir).expect("建目录");
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "secret-outside").expect("外部文件");

    // 建符号链接 wiki/link.md → outside.txt(无权限环境跳过)
    let link = wiki_dir.join("link.md");
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(&outside, &link).is_err() {
            eprintln!("跳过:当前环境无符号链接权限");
            return;
        }
    }
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&outside, &link).expect("建链");

    let hub = McpHub::new();
    let transport = StdioMcpTransport::spawn(
        "python",
        &[
            app_path("wiki_server.py").to_string_lossy().to_string(),
            "--dir".into(),
            wiki_dir.to_string_lossy().to_string(),
        ],
        &Default::default(),
    )
    .expect("子进程启动");
    let manifests = hub.connect("wiki", transport, 30_000).await.expect("握手");
    let entries = McpHub::capability_entries(manifests);
    let rig = TestRig::standard_with(vec![], entries, Some(hub.clone())).await;

    // 写路径需首调审批(破坏性工具)→ 先走一遍审批
    let req = rig.ids.next_id("req");
    let _ = rig
        .handle
        .capability_call(
            req,
            CapabilityCallParams {
                capability: "mcp.wiki.page.write".into(),
                args: json!({"name": "warmup", "content": "x"}),
                idempotency_key: None,
                deadline_ms: Some(30_000),
            },
        )
        .await
        .expect_err("首调必须审批");
    let list = rig
        .handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
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
                scope: Some("count:5".into()),
            },
        )
        .await
        .expect("批准");

    // 越界写:必须失败,且外部文件保持原样
    let done = call_and_settle(
        &rig,
        "mcp.wiki.page.write",
        json!({"name": "link", "content": "PWNED"}),
        None,
    )
    .await;
    assert_eq!(done.state, OperationState::Failed, "越界写必须失败");
    assert_eq!(
        std::fs::read_to_string(&outside).unwrap(),
        "secret-outside",
        "外部文件不得被改写"
    );

    // 越界读:同样拒绝
    let done = call_and_settle(&rig, "mcp.wiki.page.read", json!({"name": "link"}), None).await;
    assert_eq!(done.state, OperationState::Failed, "越界读必须失败");
    rig.stop().await;
}

/// t124(外部审计 X-06 验收):Market qty=true 必须被拒(布尔非整数)。
#[tokio::test]
async fn t124_market_rejects_bool_qty() {
    if !gated() {
        return;
    }
    let hub = McpHub::new();
    let transport = StdioMcpTransport::spawn(
        "python",
        &[app_path("market_server.py").to_string_lossy().to_string()],
        &Default::default(),
    )
    .expect("子进程启动");
    let manifests = hub
        .connect("market", transport, 30_000)
        .await
        .expect("握手");
    let entries = McpHub::capability_entries(manifests);
    let rig = TestRig::standard_with(vec![], entries, Some(hub.clone())).await;

    // 首调 add 需审批 → 批准 once → qty=true 必须工具级失败
    let req = rig.ids.next_id("req");
    let err = rig
        .handle
        .capability_call(
            req,
            CapabilityCallParams {
                capability: "mcp.market.portfolio.add".into(),
                args: json!({"symbol": "ACME", "qty": true}),
                idempotency_key: None,
                deadline_ms: Some(30_000),
            },
        )
        .await
        .expect_err("可逆写首调必须审批");
    assert!(matches!(
        err,
        bm_core::CoreError::Semantic(bm_contract::error_codes::ErrorCode::ApprovalRequired, _)
    ));
    let list = rig
        .handle
        .approval_list(bm_contract::wire::ApprovalListParams { state_filter: None })
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
    assert_eq!(
        done.state,
        OperationState::Failed,
        "qty=true 必须被工具拒绝"
    );
    rig.stop().await;
}
