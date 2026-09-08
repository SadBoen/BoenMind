//! T6a:Web Surface 静态托管——GET / 无鉴权回落 ServeDir;API 仍受鉴权保护。

use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::mock_model::{MockConnector, Step};
use bm_providers::secret::MemSecretStore;
use bm_surface_http::token;
use std::sync::Arc;

#[tokio::test]
async fn t34_web_root_served_without_auth_api_still_guarded() {
    let dir = tempfile::tempdir().expect("临时目录");
    let web_dir = tempfile::tempdir().expect("web 目录");
    std::fs::write(
        web_dir.path().join("index.html"),
        "<!DOCTYPE html><html><body>boenmind-surface-ok</body></html>",
    )
    .expect("写页面");

    let t = token::load_or_create(dir.path()).expect("令牌");
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::with_default(),
        job_board: None,
        version: "0.1.0-m1".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::new()),
        id_gen: Arc::new(bm_contract::ids::SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;

    let app = bm_surface_http::router(
        handle.clone(),
        Arc::new(t.clone()),
        store.clone(),
        Arc::new(tokio::sync::Notify::new()),
        Some(web_dir.path().to_path_buf()),
        Arc::new("mock-model".into()),
        None,
        None,
        false,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

    let base = format!("http://{addr}");
    // 静态页面:无鉴权可取
    let r = reqwest::get(format!("{base}/")).await.expect("GET /");
    assert_eq!(r.status().as_u16(), 200);
    let html = r.text().await.expect("正文");
    assert!(html.contains("boenmind-surface-ok"), "index.html 内容");

    // API 仍受鉴权:无令牌 401
    let r = reqwest::Client::new()
        .post(format!("{base}/rpc/session.create"))
        .send()
        .await
        .expect("无令牌 rpc");
    assert_eq!(r.status().as_u16(), 401);

    let _ = handle;
}

/// 会话目录(2026-09-08 三端一致批):GET /admin/sessions 返回服务端权威
/// 列表——建会话即出现,删除即消失;三端一致的根本,此前只有浏览器
/// localStorage 各记各账。注:harness 未配门户墙,/admin 面免 Bearer
/// (与 webadmin_tests 同口径);/rpc 仍须 Bearer。
#[tokio::test]
async fn t35_admin_session_list_is_server_authoritative() {
    use bm_contract::ids::{IdGen, SeqIdGen};
    use bm_surface_http::webadmin::AdminConfig;

    let dir = tempfile::tempdir().expect("临时目录");
    let ws = tempfile::tempdir().expect("工作区目录");
    let t = token::load_or_create(dir.path()).expect("令牌");
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::with_default(),
        job_board: None,
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

    let admin = AdminConfig {
        data_dir: dir.path().to_path_buf(),
        workspace_root: ws.path().to_path_buf(),
        mcp_config: None,
        builtin_caps: Arc::new(vec![]),
        mcp_servers: Arc::new(std::sync::RwLock::new(vec![])),
        handle: handle.clone(),
        hub: None,
        secrets: Some(Arc::new(MemSecretStore::new()) as Arc<dyn bm_core::ports::SecretStore>),
        model_routes: None,
        shutdown: None,
        web_dir: None,
        bundled_plugins_dir: None,
        limits: Default::default(),
        limits_sources: Default::default(),
        jobs: None,
    };
    let app = bm_surface_http::router(
        handle.clone(),
        Arc::new(t.clone()),
        store.clone(),
        Arc::new(tokio::sync::Notify::new()),
        None,
        Arc::new("mock-model".into()),
        Some(admin),
        None,
        false,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // 未建会话:空目录
    let r = client
        .get(format!("{base}/admin/sessions"))
        .send()
        .await
        .expect("GET");
    assert_eq!(r.status().as_u16(), 200);
    let body: serde_json::Value = r.json().await.expect("信封");
    assert_eq!(body["ok"], true);
    assert_eq!(body["sessions"].as_array().unwrap().len(), 0);

    // 建会话(/rpc 须 Bearer)→ 目录出现该会话
    let req_id = IdGen::next_id(&SeqIdGen::new(), "req");
    let envelope = serde_json::json!({
        "v": "0.1",
        "method": "session.create",
        "request_id": req_id.as_str(),
        "params": {"agent": {"name": "assistant", "model_chain": ["mock-model"]}},
    });
    let r = client
        .post(format!("{base}/rpc/session.create"))
        .header("Authorization", format!("Bearer {t}"))
        .json(&envelope)
        .send()
        .await
        .expect("session.create");
    let body: serde_json::Value = r.json().await.expect("信封");
    assert_eq!(body["ok"], true, "{body}");
    let sess = body["result"]["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();

    let r = client
        .get(format!("{base}/admin/sessions"))
        .send()
        .await
        .expect("GET");
    let body: serde_json::Value = r.json().await.expect("信封");
    let items = body["sessions"].as_array().expect("数组");
    assert_eq!(items.len(), 1, "建会话即入目录");
    assert_eq!(items[0]["id"], serde_json::json!(sess));
    assert_eq!(items[0]["state"], serde_json::json!("active"));
    assert!(items[0]["created_at"].is_string());
    assert!(
        items[0]["updated_at"].is_string(),
        "新建会话 updated_at = created_at"
    );

    // 删除 → 目录同步消失
    let r = client
        .delete(format!("{base}/admin/sessions/{sess}"))
        .send()
        .await
        .expect("DELETE 会话");
    assert_eq!(r.status().as_u16(), 200);
    let r = client
        .get(format!("{base}/admin/sessions"))
        .send()
        .await
        .expect("GET");
    let body: serde_json::Value = r.json().await.expect("信封");
    assert_eq!(
        body["sessions"].as_array().unwrap().len(),
        0,
        "删除即出目录"
    );

    let _ = handle;
}

/// 会话目录分页(issue #15):limit/skip 裁剪 + total/truncated 增量字段。
/// 房规同 session_messages(limit+skip 游标);默认页 500,硬顶 1000。
#[tokio::test]
async fn t35b_admin_session_list_paging() {
    use bm_contract::ids::{IdGen, SeqIdGen};
    use bm_surface_http::webadmin::AdminConfig;

    let dir = tempfile::tempdir().expect("临时目录");
    let ws = tempfile::tempdir().expect("工作区目录");
    let t = token::load_or_create(dir.path()).expect("令牌");
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::with_default(),
        job_board: None,
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

    let admin = AdminConfig {
        data_dir: dir.path().to_path_buf(),
        workspace_root: ws.path().to_path_buf(),
        mcp_config: None,
        builtin_caps: Arc::new(vec![]),
        mcp_servers: Arc::new(std::sync::RwLock::new(vec![])),
        handle: handle.clone(),
        hub: None,
        secrets: Some(Arc::new(MemSecretStore::new()) as Arc<dyn bm_core::ports::SecretStore>),
        model_routes: None,
        shutdown: None,
        web_dir: None,
        bundled_plugins_dir: None,
        limits: Default::default(),
        limits_sources: Default::default(),
        jobs: None,
    };
    let app = bm_surface_http::router(
        handle.clone(),
        Arc::new(t.clone()),
        store.clone(),
        Arc::new(tokio::sync::Notify::new()),
        None,
        Arc::new("mock-model".into()),
        Some(admin),
        None,
        false,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // 建 7 个会话
    for _ in 0..7 {
        let req_id = IdGen::next_id(&SeqIdGen::new(), "req");
        let envelope = serde_json::json!({
            "v": "0.1",
            "method": "session.create",
            "request_id": req_id.as_str(),
            "params": {"agent": {"name": "assistant", "model_chain": ["mock-model"]}},
        });
        let r = client
            .post(format!("{base}/rpc/session.create"))
            .header("Authorization", format!("Bearer {t}"))
            .json(&envelope)
            .send()
            .await
            .expect("session.create");
        assert_eq!(
            r.json::<serde_json::Value>().await.expect("信封")["ok"],
            true
        );
    }

    // 默认(无参):全量返回,无截断
    let body: serde_json::Value = client
        .get(format!("{base}/admin/sessions"))
        .send()
        .await
        .expect("GET")
        .json()
        .await
        .expect("信封");
    assert_eq!(body["total"], serde_json::json!(7));
    assert_eq!(body["truncated"], serde_json::json!(false));
    assert_eq!(body["sessions"].as_array().unwrap().len(), 7);

    // limit=3:截断可见
    let body: serde_json::Value = client
        .get(format!("{base}/admin/sessions?limit=3"))
        .send()
        .await
        .expect("GET")
        .json()
        .await
        .expect("信封");
    assert_eq!(body["total"], serde_json::json!(7));
    assert_eq!(body["truncated"], serde_json::json!(true));
    let page1: Vec<String> = body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(page1.len(), 3);

    // skip=3:第二页与第一页零重叠
    let body: serde_json::Value = client
        .get(format!("{base}/admin/sessions?limit=3&skip=3"))
        .send()
        .await
        .expect("GET")
        .json()
        .await
        .expect("信封");
    let page2: Vec<String> = body["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(page2.len(), 3);
    assert!(body["truncated"].as_bool().unwrap());
    assert!(page1.iter().all(|id| !page2.contains(id)), "页间不得重叠");

    // 末页:skip 越过剩余量 → 只剩余项,无截断
    let body: serde_json::Value = client
        .get(format!("{base}/admin/sessions?limit=3&skip=6"))
        .send()
        .await
        .expect("GET")
        .json()
        .await
        .expect("信封");
    assert_eq!(body["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(body["truncated"], serde_json::json!(false));

    // limit 硬顶 1000(传入 99999 被钳制)
    let body: serde_json::Value = client
        .get(format!("{base}/admin/sessions?limit=99999"))
        .send()
        .await
        .expect("GET")
        .json()
        .await
        .expect("信封");
    assert_eq!(body["limit"], serde_json::json!(1000));

    let _ = handle;
}

/// issue #12:Provider 熔断健康快照接口——GET /admin/providers/health。
/// 空脚本 mock 连接器每次调用必败;3 次失败达阈值(默认 3)即熔断,
/// 健康端点应可见 unavailable + 冷却截止;新 runtime 则为空快照。
#[tokio::test]
async fn t35c_admin_provider_health_snapshot() {
    use bm_contract::ids::SeqIdGen;
    use bm_surface_http::webadmin::AdminConfig;

    let dir = tempfile::tempdir().expect("临时目录");
    let ws = tempfile::tempdir().expect("工作区目录");
    let t = token::load_or_create(dir.path()).expect("令牌");
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    // 熔断只计 Unavailable 类失败(内部错/鉴权错是配置错,不烧熔断器);
    // retryable=false 保证每次 /v1 调用恰好一次 attempt(不进重试循环)。
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::repeating(Step::Fail {
        error_code: bm_contract::error_codes::ErrorCode::Unavailable,
        retryable: false,
    }));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::with_default(),
        job_board: None,
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

    let admin = AdminConfig {
        data_dir: dir.path().to_path_buf(),
        workspace_root: ws.path().to_path_buf(),
        mcp_config: None,
        builtin_caps: Arc::new(vec![]),
        mcp_servers: Arc::new(std::sync::RwLock::new(vec![])),
        handle: handle.clone(),
        hub: None,
        secrets: Some(Arc::new(MemSecretStore::new()) as Arc<dyn bm_core::ports::SecretStore>),
        model_routes: None,
        shutdown: None,
        web_dir: None,
        bundled_plugins_dir: None,
        limits: Default::default(),
        limits_sources: Default::default(),
        jobs: None,
    };
    let app = bm_surface_http::router(
        handle.clone(),
        Arc::new(t.clone()),
        store.clone(),
        Arc::new(tokio::sync::Notify::new()),
        None,
        Arc::new("mock-model".into()),
        Some(admin),
        None,
        false,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // 新 runtime:无任何失败记录 = 空快照
    let body: serde_json::Value = client
        .get(format!("{base}/admin/providers/health"))
        .send()
        .await
        .expect("GET")
        .json()
        .await
        .expect("信封");
    assert_eq!(body["ok"], serde_json::json!(true));
    assert_eq!(body["health"].as_array().unwrap().len(), 0, "{body}");

    // 3 次必败模型调用(≥ provider_fail_threshold 默认 3)→ 熔断
    for i in 0..3 {
        let _ = client
            .post(format!("{base}/v1/chat/completions"))
            .json(&serde_json::json!({
                "model": "mock-model",
                "messages": [{"role": "user", "content": format!("msg{i}")}],
            }))
            .send()
            .await
            .expect("调用");
    }

    let body: serde_json::Value = client
        .get(format!("{base}/admin/providers/health"))
        .send()
        .await
        .expect("GET")
        .json()
        .await
        .expect("信封");
    let health = body["health"].as_array().expect("health 数组");
    assert_eq!(health.len(), 1, "{body}");
    assert_eq!(health[0]["provider"], serde_json::json!("mock"));
    assert_eq!(
        health[0]["status"],
        serde_json::json!("unavailable"),
        "{body}"
    );
    assert!(health[0]["fail_streak"].as_u64().unwrap() >= 3);
    assert!(
        health[0]["cooldown_until"].is_string(),
        "熔断必须有冷却截止"
    );

    let _ = handle;
}
