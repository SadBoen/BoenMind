//! M3.1/M3.2 HTTP Surface 端到端(传输合同 surface/transport.v0_1):
//! 鉴权豁免/拒绝、信封逐字节、业务 200、会话-回合-收据全链路、SSE 流。

use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::Method;
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::mock_model::{MockConnector, Step};
use bm_providers::secret::MemSecretStore;
use bm_surface_http::token;
use std::sync::Arc;
use std::time::Duration;

struct Rig {
    url: String,
    token: String,
    handle: RuntimeHandle,
    ids: Arc<SeqIdGen>,
    _dir: tempfile::TempDir,
}

async fn rig(script: Vec<Step>) -> Rig {
    let dir = tempfile::tempdir().expect("临时目录");
    let token = Arc::new(token::load_or_create(dir.path()).expect("令牌"));
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(script));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: Vec::new(),
        version: "0.1.0-m1".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::with(
            &bm_core::runtime::default_secret_ref(bm_testkit_replay::MODEL_A),
            "sk-demo-zhipu-secret-value-001",
        )),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;

    let shutdown = Arc::new(tokio::sync::Notify::new());
    let app = bm_surface_http::router(handle.clone(), token.clone(), store.clone(), shutdown, None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    Rig {
        url: format!("http://{addr}"),
        token: token.to_string(),
        handle,
        ids: Arc::new(SeqIdGen::new()),
        _dir: dir,
    }
}

impl Rig {
    fn client(&self, token: Option<&str>) -> reqwest::Client {
        let mut b = reqwest::Client::builder();
        if let Some(t) = token {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {t}").parse().expect("头"),
            );
            b = b.default_headers(headers);
        }
        b.build().expect("客户端")
    }

    async fn rpc(
        &self,
        client: &reqwest::Client,
        method: Method,
        params: serde_json::Value,
    ) -> (u16, serde_json::Value) {
        let envelope = serde_json::json!({
            "v": "0.1",
            "method": method.as_str(),
            "request_id": IdGen::next_id(self.ids.as_ref(), "req").as_str(),
            "params": params,
        });
        let r = client
            .post(format!("{}/rpc/{}", self.url, method.as_str()))
            .json(&envelope)
            .send()
            .await
            .expect("请求");
        (r.status().as_u16(), r.json().await.expect("信封 JSON"))
    }
}

mod bm_testkit_replay {
    pub const MODEL_A: &str = "zhipu.glm-4-flash";
}

/// 同一步骤重复多次(多回合脚本)。
fn bm_testkit_style_repeat(step: Step, n: usize) -> Vec<Step> {
    vec![step; n]
}

#[tokio::test]
async fn t30_health_auth_and_envelope_end_to_end() {
    let rig = rig(vec![Step::ok("答", 412, 58)]).await;
    let anon = rig.client(None);
    let authed = rig.client(Some(&rig.token));

    // /health 无鉴权
    let r = anon
        .get(format!("{}/health", rig.url))
        .send()
        .await
        .expect("health");
    assert_eq!(r.status().as_u16(), 200);
    let body: serde_json::Value = r.json().await.expect("JSON");
    assert_eq!(body["ok"], true);

    // 无令牌 / 错误令牌 → 401
    let r = anon
        .post(format!("{}/rpc/session.create", rig.url))
        .send()
        .await
        .expect("无令牌请求");
    assert_eq!(r.status().as_u16(), 401);
    let r = rig
        .client(Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"))
        .post(format!("{}/rpc/session.create", rig.url))
        .send()
        .await
        .expect("错令牌请求");
    assert_eq!(r.status().as_u16(), 401);

    // 未知方法 → 404
    let r = authed
        .post(format!("{}/rpc/nope.nope", rig.url))
        .send()
        .await
        .expect("未知方法");
    assert_eq!(r.status().as_u16(), 404);

    // 信封逐字节:ok=true + result 字段;request_id 回显
    let req_id = IdGen::next_id(rig.ids.as_ref(), "req");
    let envelope = serde_json::json!({
        "v": "0.1",
        "method": "session.create",
        "request_id": req_id.as_str(),
        "params": {"agent": {"name": "assistant",
            "model_chain": [bm_testkit_replay::MODEL_A],
            "budget": {"max_tokens": 10000, "max_turns": 10}}},
    });
    let r = authed
        .post(format!("{}/rpc/session.create", rig.url))
        .json(&envelope)
        .send()
        .await
        .expect("session.create");
    assert_eq!(r.status().as_u16(), 200, "业务结果恒 200");
    let body: serde_json::Value = r.json().await.expect("信封");
    assert_eq!(body["ok"], true);
    assert_eq!(body["request_id"], req_id.as_str(), "request_id 回显");
    let sess = body["result"]["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();
    let agent = body["result"]["agent_id"]
        .as_str()
        .expect("agent_id")
        .to_string();

    // 业务错误也走 200 + 信封 error(未知会话 operations.get 的反向用例:
    // events.poll 用合法会话但合法信封 → ok=true;构造一个非法参数请求)
    let bad = serde_json::json!({
        "v": "0.1",
        "method": "operations.get",
        "request_id": IdGen::next_id(rig.ids.as_ref(), "req").as_str(),
        "params": {"operation_id": "nope"},
    });
    let r = authed
        .post(format!("{}/rpc/operations.get", rig.url))
        .json(&bad)
        .send()
        .await
        .expect("非法 id 请求");
    assert_eq!(r.status().as_u16(), 200, "业务错误恒 200");
    let body: serde_json::Value = r.json().await.expect("信封");
    assert_eq!(body["ok"], false, "信封内报错");
    assert_eq!(body["error"]["code"], "validation_failed");

    // events.poll 合法会话:创建期事件在场
    let (status, body) = rig
        .rpc(
            &authed,
            Method::EventsPoll,
            serde_json::json!({"session_id": sess, "since_seq": 0}),
        )
        .await;
    assert_eq!(status, 200);
    assert!(body["ok"] == true);
    assert!(body["result"]["events"].as_array().expect("数组").len() >= 2);

    let _ = (agent, authed);
    rig.handle.stop("test_done").await;
}

#[tokio::test]
async fn t31_turn_via_http_receipt_and_events() {
    let rig = rig(bm_testkit_style_repeat(Step::ok("HTTP 答", 100, 20), 10)).await;
    let authed = rig.client(Some(&rig.token));

    // 建会话
    let (_, body) = rig
        .rpc(
            &authed,
            Method::SessionCreate,
            serde_json::json!({"agent": {"name": "assistant",
                "model_chain": [bm_testkit_replay::MODEL_A],
                "budget": {"max_tokens": 100000, "max_turns": 10}}}),
        )
        .await;
    let sess = body["result"]["session_id"]
        .as_str()
        .expect("sess")
        .to_string();
    let agent = body["result"]["agent_id"]
        .as_str()
        .expect("agent")
        .to_string();

    // 发起回合:收据 state=running
    let (status, body) = rig
        .rpc(
            &authed,
            Method::AgentSendInput,
            serde_json::json!({"session_id": sess, "agent_id": agent,
                "content": "HTTP 上的第一问", "input_trust": "trusted"}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    let op = body["result"]["operation_id"]
        .as_str()
        .expect("op")
        .to_string();
    assert_eq!(body["result"]["state"], "running");

    // 轮询收据至终态
    loop {
        let (status, body) = rig
            .rpc(
                &authed,
                Method::OperationsGet,
                serde_json::json!({"operation_id": op}),
            )
            .await;
        assert_eq!(status, 200);
        if body["result"]["state"].as_str() == Some("succeeded") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // SSE 流:自 0 重连可收到全部历史(含回合事件)
    let r = authed
        .get(format!("{}/events/{sess}?since_seq=0", rig.url))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("SSE 连接");
    assert_eq!(r.status().as_u16(), 200);
    let ct = r
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .expect("content-type")
        .to_string();
    assert!(
        ct.starts_with("text/event-stream"),
        "SSE content-type,实际 {ct}"
    );
    // SSE 是无限流:分块读直到看到目标事件或超时
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    let mut text = String::new();
    let mut received = r;
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "SSE 8s 内未见到 agent.completed"
        );
        let chunk = tokio::time::timeout(Duration::from_secs(3), received.chunk())
            .await
            .expect("分块读取超时")
            .expect("读块")
            .expect("流结束");
        text.push_str(&String::from_utf8_lossy(&chunk));
        if text.contains("event: envelope") && text.contains("\"type\":\"agent.completed\"") {
            break;
        }
    }
    assert!(text.contains("event: envelope"), "SSE 帧格式");
    assert!(
        text.contains("\"type\":\"agent.completed\""),
        "流内含回合完成事件"
    );

    rig.handle.stop("test_done").await;
}

#[tokio::test]
async fn t33_shutdown_endpoint_is_authed_and_notifies() {
    let dir = tempfile::tempdir().expect("临时目录");
    let token = Arc::new(token::load_or_create(dir.path()).expect("令牌"));
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: Vec::new(),
        version: "0.1.0-m1".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::new()),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;

    let shutdown = Arc::new(tokio::sync::Notify::new());
    let assert_notified = shutdown.clone();
    let app = bm_surface_http::router(handle.clone(), token.clone(), store.clone(), shutdown, None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    let url = format!("http://{addr}");

    let authed = reqwest::Client::builder()
        .default_headers(authed_header(&token))
        .build()
        .expect("客户端");

    // 无令牌 → 401(停机是受控操作)
    let r = reqwest::Client::new()
        .post(format!("{url}/shutdown"))
        .send()
        .await
        .expect("无令牌 shutdown");
    assert_eq!(r.status().as_u16(), 401);

    // 有令牌 → 200 draining=true,且 Notify 触发
    let notified = assert_notified.notified();
    let r = authed
        .post(format!("{url}/shutdown"))
        .send()
        .await
        .expect("shutdown");
    assert_eq!(r.status().as_u16(), 200);
    let body: serde_json::Value = r.json().await.expect("JSON");
    assert_eq!(body["draining"], true);
    tokio::time::timeout(Duration::from_secs(2), notified)
        .await
        .expect("停机信号必须触发");

    handle.stop("test_done").await;
}

fn authed_header(token: &str) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {token}").parse().expect("头"),
    );
    headers
}

// ---- M4:审批全链路 HTTP 形态(高风险 → approval_required → 批准 → 执行)----

async fn m4_rig(
    capabilities: Vec<(
        bm_contract::capability::CapabilityManifest,
        std::sync::Arc<dyn bm_core::registry::CapabilityProvider>,
    )>,
) -> Rig {
    let dir = tempfile::tempdir().expect("临时目录");
    let token = Arc::new(token::load_or_create(dir.path()).expect("令牌"));
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities,
        version: "0.1.0-m4".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::with(
            &bm_core::runtime::default_secret_ref(bm_testkit_replay::MODEL_A),
            "sk-demo-zhipu-secret-value-001",
        )),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;

    let shutdown = Arc::new(tokio::sync::Notify::new());
    let app = bm_surface_http::router(handle.clone(), token.clone(), store.clone(), shutdown, None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    Rig {
        url: format!("http://{addr}"),
        token: token.to_string(),
        handle,
        ids: Arc::new(SeqIdGen::new()),
        _dir: dir,
    }
}

fn m4_manifest(name: &str, effect: &str) -> bm_contract::capability::CapabilityManifest {
    serde_json::from_value(serde_json::json!({
        "capability": name, "provider": name, "version": "0.1.0",
        "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "effect": effect, "idempotent": true, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    }))
    .expect("manifest 合法")
}

#[tokio::test]
async fn t34_capability_call_approval_cycle_over_http() {
    let rig = m4_rig(vec![
        (
            m4_manifest("system.echo", "read-only"),
            bm_core::broker::provider_fn(Ok),
        ),
        (
            m4_manifest("system.danger.purge", "high-risk-command"),
            bm_core::broker::provider_fn(|_| Ok(serde_json::json!({"purged": true}))),
        ),
    ])
    .await;
    let client = rig.client(Some(&rig.token));

    // 直通:read-only 经 HTTP 成功
    let (status, body) = rig
        .rpc(
            &client,
            Method::CapabilityCall,
            serde_json::json!({"capability": "system.echo", "args": {"m": "hi"}}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    assert_eq!(body["result"]["state"], "succeeded");

    // 高风险 → approval_required 信封(业务 200)
    let (status, body) = rig
        .rpc(
            &client,
            Method::CapabilityCall,
            serde_json::json!({"capability": "system.danger.purge", "args": {}}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"]["code"], "approval_required");

    // approval.list 可见 waiting_user
    let (status, body) = rig
        .rpc(&client, Method::ApprovalList, serde_json::json!({}))
        .await;
    assert_eq!(status, 200);
    let approvals = body["result"]["approvals"]
        .as_array()
        .expect("approvals 数组");
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0]["state"], "waiting_user");
    let approval_id = approvals[0]["approval_id"]
        .as_str()
        .expect("id")
        .to_string();

    // 批准(scope=once)→ approved
    let (status, body) = rig
        .rpc(
            &client,
            Method::ApprovalRespond,
            serde_json::json!({"approval_id": approval_id, "decision": "approve", "scope": "once"}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["result"]["state"], "approved");
    assert!(body["result"]["grant_id"].is_string());

    // 审计:capability.invoked(ok)与 grant.created 落事件流
    let (status, body) = rig
        .rpc(&client, Method::EventsPoll,
             serde_json::json!({"session_id": rig.ids.next_id("sess").as_str(), "since_seq": 0, "limit": 1000}))
        .await;
    let _ = (status, body);
    let events = rig.handle.events_all().await;
    assert!(events.iter().any(|e| {
        e.event_type == bm_contract::events::EventType::CapabilityInvoked
            && e.payload["outcome"] == serde_json::json!("ok")
    }));
    assert!(
        events
            .iter()
            .any(|e| e.event_type == bm_contract::events::EventType::GrantCreated)
    );

    rig.handle.stop("test_done").await;
}

/// M4-T5 冒烟:server 将实际使用的内置能力集(builtin_capability_set)
/// 经 HTTP 直通/审批链路可用(装配路径与 boenmind-server 完全一致)。
#[tokio::test]
async fn t35_builtin_capability_set_smoke() {
    let rig = m4_rig(bm_providers::builtin::builtin_capability_set()).await;
    let client = rig.client(Some(&rig.token));

    // low-risk 直通:counter.bump 执行并返回内部结果
    let (status, body) = rig
        .rpc(
            &client,
            Method::CapabilityCall,
            serde_json::json!({"capability": "system.counter.bump", "args": {"key": "k"}}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    assert_eq!(body["result"]["result"]["count"], 1);

    // external-side-effect → 升级审批(untrusted 门控无关;effective reversible+)
    let (status, body) = rig
        .rpc(
            &client,
            Method::CapabilityCall,
            serde_json::json!({"capability": "system.mail.mock_send",
                                "args": {"to": "a@x", "subject": "s"},
                                "idempotency_key": "idem-1"}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["error"]["code"], "approval_required");

    // 批准一次 → 执行成功,返回 mock 收据
    let (_, list) = rig
        .rpc(&client, Method::ApprovalList, serde_json::json!({}))
        .await;
    let approval_id = list["result"]["approvals"][0]["approval_id"]
        .as_str()
        .expect("id")
        .to_string();
    let (status, body) = rig
        .rpc(
            &client,
            Method::ApprovalRespond,
            serde_json::json!({"approval_id": approval_id, "decision": "approve", "scope": "once"}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["result"]["state"], "approved");
    rig.handle.stop("test_done").await;
}

/// t58(M5-T2):task 六方法经 HTTP 全链路——create/list/get/pause/resume/stop
/// 同一 Wire 信封;错误信封语义(unavailable 前置校验同源)。
#[tokio::test]
async fn t58_task_methods_via_http() {
    let rig = rig(vec![]).await;
    let client = rig.client(Some(&rig.token));

    // create:即启动(running)
    let (status, resp) = rig
        .rpc(
            &client,
            Method::TaskCreate,
            serde_json::json!({"title": "HTTP 冒烟", "goal": "经 Wire 建 Task"}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(resp["ok"], serde_json::json!(true));
    let task_id = resp["result"]["task_id"].as_str().unwrap().to_string();
    assert_eq!(resp["result"]["state"], serde_json::json!("running"));

    // list / get
    let (status, resp) = rig
        .rpc(&client, Method::TaskList, serde_json::json!({}))
        .await;
    assert_eq!(status, 200);
    assert_eq!(resp["result"]["tasks"].as_array().unwrap().len(), 1);
    let (status, resp) = rig
        .rpc(
            &client,
            Method::TaskGet,
            serde_json::json!({"task_id": task_id}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        resp["result"]["task"]["title"],
        serde_json::json!("HTTP 冒烟")
    );

    // pause → resume → stop
    for (m, extra) in [
        (Method::TaskPause, serde_json::json!({"reason": "http"})),
        (Method::TaskResume, serde_json::json!({})),
        (Method::TaskStop, serde_json::json!({"reason": "http"})),
    ] {
        let mut params = serde_json::json!({ "task_id": task_id });
        if let (Some(obj), Some(e)) = (params.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                obj.insert(k.clone(), v.clone());
            }
        }
        let (status, resp) = rig.rpc(&client, m, params).await;
        assert_eq!(status, 200);
        assert_eq!(resp["ok"], serde_json::json!(true), "{m:?}: {resp}");
    }
    let (status, resp) = rig
        .rpc(
            &client,
            Method::TaskGet,
            serde_json::json!({"task_id": task_id}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        resp["result"]["task"]["state"],
        serde_json::json!("cancelled")
    );

    // 表外拒绝:终态后再 pause → 信封内 validation_failed(业务 200 + ok=false)
    let (status, resp) = rig
        .rpc(
            &client,
            Method::TaskPause,
            serde_json::json!({"task_id": task_id}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(resp["ok"], serde_json::json!(false));
    assert_eq!(
        resp["error"]["code"],
        serde_json::json!("validation_failed")
    );
}
