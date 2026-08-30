//! M8-T2:多 Surface 协作 e2e——同一 Runtime 上,「Web 形态」连接发起
//! 能力调用、「CLI 形态」第二连接取消;审计链同源(基线 M8.3)。

use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::Method;
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::mcp::{Behavior, InProcMcpServer, McpHub, McpToolDef};
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use bm_surface_http::token;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

struct Rig {
    url: String,
    token: String,
    handle: RuntimeHandle,
    ids: Arc<SeqIdGen>,
    _dir: tempfile::TempDir,
}

/// 双 App?单 mcp server(InProc 慢工具)装配,mcp hub 为异步执行器。
async fn rig_with_slow_mcp() -> Rig {
    let dir = tempfile::tempdir().expect("临时目录");
    let token = Arc::new(token::load_or_create(dir.path()).expect("令牌"));
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));

    let server = InProcMcpServer::new(vec![McpToolDef {
        name: "slow".into(),
        description: None,
        input_schema: json!({"type": "object"}),
        annotations: json!({"readOnlyHint": true}),
    }]);
    server.set_behavior(
        "slow",
        Behavior {
            delay_ms: 5_000,
            ..Behavior::done(json!({"content": [{"type": "text", "text": "late"}]}))
        },
    );
    let hub = McpHub::new();
    let manifests = hub
        .connect(
            "slow",
            server as Arc<dyn bm_providers::mcp::McpTransport>,
            60_000,
        )
        .await
        .expect("握手");
    let entries = McpHub::capability_entries(manifests);

    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: [vec![bm_providers::builtin::model_invoke_cap()], entries].concat(),
        async_executor: Some(hub),
        version: "0.1.0-m8".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::with(
            &bm_core::runtime::default_secret_ref("zhipu.glm-4-flash"),
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

/// t115:多 Surface 协作——同一 Runtime,连接 A(Web 形态)发起慢调用,
/// 连接 B(CLI 形态)取消;两 Surface 的操作在同一事件日志同源可溯,
/// 收据落 cancelled,后续轮询一致。
#[tokio::test]
async fn t115_multi_surface_cancel_collaboration() {
    let rig = rig_with_slow_mcp().await;
    let web_like = rig.client(Some(&rig.token)); // Surface A:Web 形态
    let cli_like = rig.client(Some(&rig.token)); // Surface B:CLI 形态(第二连接)

    // Surface A 发起在途调用
    let (status, body) = rig
        .rpc(
            &web_like,
            Method::CapabilityCall,
            json!({"capability": "mcp.slow.slow", "args": {}, "idempotency_key": null,
                   "deadline_ms": 60000}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["result"]["state"], "running");
    let op_id = body["result"]["operation_id"].as_str().unwrap().to_string();

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Surface B 取消
    let (status, body) = rig
        .rpc(
            &cli_like,
            Method::CapabilityCancel,
            json!({"operation_id": op_id, "reason": "另一端用户不想等了"}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["result"]["state"], "cancelled");

    // Surface A 轮询:cancelled(且保持)
    let (_, body) = rig
        .rpc(
            &web_like,
            Method::OperationsGet,
            json!({"operation_id": op_id}),
        )
        .await;
    assert_eq!(body["result"]["state"], "cancelled");
    tokio::time::sleep(Duration::from_millis(800)).await;
    let (_, body) = rig
        .rpc(
            &web_like,
            Method::OperationsGet,
            json!({"operation_id": op_id}),
        )
        .await;
    assert_eq!(body["result"]["state"], "cancelled", "迟到完成不得改写");

    // 审计同源:取消审计在事件流(caller capability.invoked error/cancelled)
    let events = rig.handle.events_all().await;
    assert!(events.iter().any(|e| {
        e.event_type == bm_contract::events::EventType::CapabilityInvoked
            && e.payload["operation_id"].as_str() == Some(op_id.as_str())
            && e.payload["error_code"] == "cancelled"
    }));
}
