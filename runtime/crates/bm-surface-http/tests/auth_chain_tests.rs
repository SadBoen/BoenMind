//! issue #10 断链补立回归:/admin 与 /v1 的统一鉴权口径
//! (auth::require_api_auth)矩阵锁死——
//! ① 本机未设墙(回环):无凭据放行(Playground 未登录可访问);
//! ② 任何场景持**错误** Bearer → 401(严格失败,绝不因墙未开而放行);
//! ③ 有效 Bearer → 放行(程序化访问,墙配置与否皆然);
//! ④ 已设墙:门户 Cookie 放行(浏览器已登录)、无凭据 401。
//! 合同面 /rpc 仍严格 Bearer(Cookie 不放行)。

use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use bm_surface_http::token;
use bm_surface_http::webadmin::AdminConfig;
use serde_json::{Value, json};
use std::sync::Arc;

async fn spawn_inner(data_dir: std::path::PathBuf, public_bind: bool) -> (String, String) {
    let t = token::load_or_create(&data_dir).expect("令牌");
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(&data_dir).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::with_default(),
        job_board: None,
        version: "0.1.0-authchain".into(),
        data_dir: Some(data_dir.clone()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::new()),
        id_gen: Arc::new(bm_contract::ids::SeqIdGen::new()),
        clock: Arc::new(SystemClock),
    })
    .await;
    let admin = AdminConfig {
        data_dir: data_dir.clone(),
        workspace_root: data_dir.join("workspace"),
        mcp_config: None,
        builtin_caps: Arc::new(vec![]),
        mcp_servers: Arc::new(std::sync::RwLock::new(vec![])),
        handle: handle.clone(),
        hub: None,
        secrets: None,
        model_routes: None,
        shutdown: None,
        web_dir: None,
        bundled_plugins_dir: None,
        limits: Default::default(),
        limits_sources: Default::default(),
        jobs: None,
        skills: None,
    };
    let app = bm_surface_http::router(
        handle,
        Arc::new(t.clone()),
        store,
        Arc::new(tokio::sync::Notify::new()),
        None,
        Arc::new("mock-model".into()),
        Some(admin),
        None,
        public_bind,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    (format!("http://{addr}"), t)
}

async fn status_of(
    c: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    auth: Option<&str>,
    cookie: Option<&str>,
    body: Option<Value>,
) -> u16 {
    let mut req = c.request(method, url);
    if let Some(b) = auth {
        req = req.header("Authorization", b);
    }
    if let Some(ck) = cookie {
        req = req.header("Cookie", ck);
    }
    if let Some(b) = body {
        req = req.json(&b);
    }
    req.send().await.expect("请求").status().as_u16()
}

#[tokio::test]
async fn admin_v1_auth_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (base, bearer) = spawn_inner(dir.path().to_path_buf(), false).await;
    let c = reqwest::Client::new();
    let admin_url = format!("{base}/admin/about");
    let models_url = format!("{base}/v1/models");

    // ① 未设墙+回环:无凭据放行(Playground/本地开发零影响)
    assert_eq!(
        status_of(&c, reqwest::Method::GET, &admin_url, None, None, None).await,
        200
    );
    assert_eq!(
        status_of(&c, reqwest::Method::GET, &models_url, None, None, None).await,
        200
    );

    // ② 错误 Bearer → 401(严格失败;墙未开也不放行——本单元核心加固)
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &admin_url,
            Some("Bearer wrong-token"),
            None,
            None,
        )
        .await,
        401,
        "/admin 持错误 Bearer 必须 401"
    );
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &models_url,
            Some("Bearer wrong-token"),
            None,
            None,
        )
        .await,
        401,
        "/v1 持错误 Bearer 必须 401"
    );

    // ③ 有效 Bearer → 放行(程序化访问)
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &admin_url,
            Some(&format!("Bearer {bearer}")),
            None,
            None,
        )
        .await,
        200
    );
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &models_url,
            Some(&format!("Bearer {bearer}")),
            None,
            None,
        )
        .await,
        200
    );

    // 公网裸绑+未设墙:无凭据 401(require_api_auth 独立兜住,不依赖门户墙)
    let dir2 = tempfile::tempdir().unwrap();
    let (pub_base, pub_bearer) = spawn_inner(dir2.path().to_path_buf(), true).await;
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &format!("{pub_base}/admin/about"),
            None,
            None,
            None,
        )
        .await,
        401
    );
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &format!("{pub_base}/v1/models"),
            None,
            None,
            None,
        )
        .await,
        401
    );
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &format!("{pub_base}/v1/models"),
            Some(&format!("Bearer {pub_bearer}")),
            None,
            None,
        )
        .await,
        200,
        "公网裸绑持有效 Bearer 仍放行"
    );
}

#[tokio::test]
async fn wall_on_cookie_passes_admin_and_v1() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(
        dir.path().join("config/portal.json"),
        json!({"password_hash": format!("s1${}", bm_surface_http::portal::hash_password("secret1", "s1"))})
            .to_string(),
    )
    .unwrap();
    let (base, bearer) = spawn_inner(dir.path().to_path_buf(), false).await;
    let c = reqwest::Client::new();

    // 登录取会话 Cookie
    let resp = c
        .post(format!("{base}/api/portal/login"))
        .json(&json!({"password": "secret1"}))
        .send()
        .await
        .expect("登录请求");
    assert_eq!(resp.status().as_u16(), 200);
    let cookie_raw = resp
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .expect("必须下发会话 Cookie");
    let cookie = cookie_raw.split(';').next().unwrap().to_string();

    // 已登录浏览器:Cookie 放行 /admin 与 /v1(Playground 兼顾)
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &format!("{base}/admin/about"),
            None,
            Some(&cookie),
            None,
        )
        .await,
        200
    );
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &format!("{base}/v1/models"),
            None,
            Some(&cookie),
            None,
        )
        .await,
        200
    );

    // 已设墙无凭据:401
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &format!("{base}/admin/about"),
            None,
            None,
            None,
        )
        .await,
        401
    );
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &format!("{base}/v1/models"),
            None,
            None,
            None,
        )
        .await,
        401
    );

    // 已设墙 + 有效 Bearer:程序化访问不受影响
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::GET,
            &format!("{base}/v1/models"),
            Some(&format!("Bearer {bearer}")),
            None,
            None,
        )
        .await,
        200
    );

    // 合同面 /rpc 不认 Cookie(严格 Bearer 不变):401
    assert_eq!(
        status_of(
            &c,
            reqwest::Method::POST,
            &format!("{base}/rpc/session.list"),
            None,
            Some(&cookie),
            Some(json!({})),
        )
        .await,
        401,
        "/rpc 必须保持严格 Bearer,Cookie 不放行"
    );
}
