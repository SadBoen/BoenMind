//! 门户登录墙(2026-09-03 用户令)回归:未设密码=全放行(既有测试与
//! 本地开发零影响);设密码后整站(含 /admin)必须持 Cookie/Bearer;
//! bootstrap 仅首次可用;改密作废全部会话;/health 与 /login 豁免。
//! 2026-09-03 评审 #9 收紧:公网绑定+未配置密码 → 仅健康/设置口可达。

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

async fn spawn(data_dir: std::path::PathBuf) -> String {
    spawn_inner(data_dir, false).await.0
}

/// 公网面变体:返回 (base, Bearer 令牌)。
async fn spawn_public(data_dir: std::path::PathBuf) -> (String, String) {
    spawn_inner(data_dir, true).await
}

async fn spawn_inner(data_dir: std::path::PathBuf, public_bind: bool) -> (String, String) {
    let t = token::load_or_create(&data_dir).expect("令牌");
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(&data_dir).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        version: "0.1.0-portal".into(),
        data_dir: Some(data_dir.clone()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::new()),
        id_gen: Arc::new(bm_contract::ids::SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: 120,
        max_attempts: None,
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

fn client_no_redirect() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("客户端")
}

/// 返回 (状态码, JSON 体或原始文本, Set-Cookie 头)。
async fn send(
    c: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    cookie: Option<String>,
    json_body: Option<Value>,
) -> (u16, Value, Option<String>) {
    let mut req = c.request(method, url);
    if let Some(ck) = cookie {
        req = req.header("Cookie", ck);
    }
    if let Some(b) = json_body {
        req = req.json(&b);
    }
    let resp = req.send().await.expect("请求");
    let status = resp.status().as_u16();
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let text = resp.text().await.unwrap_or_default();
    let v = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    (status, v, set_cookie)
}

#[tokio::test]
async fn portal_wall_lifecycle() {
    let dir = tempfile::tempdir().unwrap();

    // ① 未设密码:墙未启用,一切放行(既有行为零影响)
    let base = spawn(dir.path().to_path_buf()).await;
    let c = client_no_redirect();
    let (st, _, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/health"),
        None,
        None,
    )
    .await;
    assert_eq!(st, 200);
    let (st, _, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/admin/about"),
        None,
        None,
    )
    .await;
    assert_eq!(st, 200, "未设密码时 /admin 必须放行");

    // ② 设密码(等价重启后读到 portal.json)
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(
        dir.path().join("config/portal.json"),
        json!({"password_hash": format!("s1${}", bm_surface_http::portal::hash_password("secret1", "s1"))})
            .to_string(),
    )
    .unwrap();
    let base = spawn(dir.path().to_path_buf()).await;
    let c = client_no_redirect();

    // 豁免面:/health 通;未认证 HTML 导航 302 /login
    let (st, _, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/health"),
        None,
        None,
    )
    .await;
    assert_eq!(st, 200, "/health 必须豁免");
    let resp = c
        .get(format!("{base}/"))
        .header("Accept", "text/html")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        302,
        "未认证 HTML 导航必须 302 /login"
    );
    assert_eq!(resp.headers().get("location").unwrap(), "/login");

    // 管理面未认证:401
    let (st, _, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/admin/about"),
        None,
        None,
    )
    .await;
    assert_eq!(st, 401, "设密码后 /admin 未认证必须 401");

    // bootstrap 已配置 → 409;错误密码 → 401;正确密码 → 会话 Cookie
    let (st, _, _) = send(
        &c,
        reqwest::Method::POST,
        &format!("{base}/api/portal/bootstrap"),
        None,
        Some(json!({"password": "another1"})),
    )
    .await;
    assert_eq!(st, 409, "bootstrap 仅首次可用");
    let (st, _, _) = send(
        &c,
        reqwest::Method::POST,
        &format!("{base}/api/portal/login"),
        None,
        Some(json!({"password": "wrong-pass"})),
    )
    .await;
    assert_eq!(st, 401);
    let (st, _, set_cookie) = send(
        &c,
        reqwest::Method::POST,
        &format!("{base}/api/portal/login"),
        None,
        Some(json!({"password": "secret1"})),
    )
    .await;
    assert_eq!(st, 200);
    let cookie = set_cookie.expect("必须下发会话 Cookie");
    let bare = cookie.split(';').next().unwrap().to_string();

    // 带 Cookie:管理面放行
    let (st, _, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/admin/about"),
        Some(bare.clone()),
        None,
    )
    .await;
    assert_eq!(st, 200, "会话 Cookie 必须放行 /admin");

    // ③ 改密:旧密码错 → 401;正确 → 200 且旧会话作废、新密码可登录
    let (st, _, _) = send(
        &c,
        reqwest::Method::POST,
        &format!("{base}/api/portal/password"),
        Some(bare.clone()),
        Some(json!({"old": "bad-old", "new": "newpass1"})),
    )
    .await;
    assert_eq!(st, 401);
    let (st, _, _) = send(
        &c,
        reqwest::Method::POST,
        &format!("{base}/api/portal/password"),
        Some(bare.clone()),
        Some(json!({"old": "secret1", "new": "newpass1"})),
    )
    .await;
    assert_eq!(st, 200);
    let (st, _, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/admin/about"),
        Some(bare),
        None,
    )
    .await;
    assert_eq!(st, 401, "改密后旧会话必须作废");
    let (st, _, _) = send(
        &c,
        reqwest::Method::POST,
        &format!("{base}/api/portal/login"),
        None,
        Some(json!({"password": "newpass1"})),
    )
    .await;
    assert_eq!(st, 200, "新密码必须可登录");
}

/// 外部评审 2026-09-03 #9:公网绑定+未配置密码 → 仅健康/设置口可达,
/// /v1、/admin 与静态一律拒绝;持 Bearer 令牌不受影响。
#[tokio::test]
async fn public_bind_unconfigured_denies_public_surface() {
    let dir = tempfile::tempdir().unwrap();
    let (base, bearer) = spawn_public(dir.path().to_path_buf()).await;
    let c = client_no_redirect();

    // 未认证:静态页 302 /login,API 面 401
    let (st, _, _) = send(&c, reqwest::Method::GET, &format!("{base}/"), None, None).await;
    assert_eq!(st, 302, "未认证 HTML 导航应 302 /login");
    let (st, _, _) = send(
        &c,
        reqwest::Method::POST,
        &format!("{base}/v1/chat/completions"),
        None,
        Some(json!({"model": "mock-model", "messages": []})),
    )
    .await;
    assert_eq!(st, 401, "/v1 未认证必须 401");
    let (st, _, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/admin/capabilities"),
        None,
        None,
    )
    .await;
    assert_eq!(st, 401, "/admin 未认证必须 401");

    // 设置口与健康检查保持可达(首次使用流程不破)
    let (st, _, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/login"),
        None,
        None,
    )
    .await;
    // 测试台无 web_dir(login.html 不存在)时页面本体 404 属正常,可达性由豁免放行保证
    assert!(
        st == 200 || st == 404,
        "/login 豁免放行,不应被墙拦成 401/302: {st}"
    );
    let (st, _, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/api/portal/state"),
        None,
        None,
    )
    .await;
    assert_eq!(st, 200);
    let (st, _, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/health"),
        None,
        None,
    )
    .await;
    assert_eq!(st, 200);

    // 持 Bearer 令牌:不受墙影响(本机 CLI/脚本场景)
    let resp = c
        .request(reqwest::Method::GET, format!("{base}/admin/capabilities"))
        .header("Authorization", format!("Bearer {bearer}"))
        .send()
        .await
        .expect("请求");
    assert_eq!(resp.status().as_u16(), 200, "Bearer 令牌必须放行");
}
