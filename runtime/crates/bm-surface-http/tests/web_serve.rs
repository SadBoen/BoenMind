//! T6a:Web Surface 静态托管——GET / 无鉴权回落 ServeDir;API 仍受鉴权保护。

use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::mock_model::MockConnector;
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
