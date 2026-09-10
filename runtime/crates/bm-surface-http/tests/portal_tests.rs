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
        limits: bm_core::LimitsCell::with_default(),
        job_board: None,
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

/// issue #47:OIDC 登录全链路(mock IdP)+ state 一次性防伪。
#[tokio::test]
async fn oidc_login_full_flow_and_state_replay_rejected() {
    // --- mock IdP:token 端点回固定 id_token(背通道直取,签名不校验形态) ---
    use base64::Engine as _;
    let b64url = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let seg = |v: serde_json::Value| b64url(v.to_string().as_bytes());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let id_token = format!(
        "{}.{}.{}",
        seg(json!({"alg": "HS256", "typ": "JWT"})),
        seg(json!({"iss": "https://mock-idp", "aud": "boenmind-web",
                    "sub": "user-1", "email": "u@example.com", "exp": now + 300})),
        seg(json!({"sig": "mock"}))
    );
    let app = axum::Router::new()
        .route(
            "/authorize",
            axum::routing::get(
                |axum::extract::Query(q): axum::extract::Query<
                    std::collections::HashMap<String, String>,
                >| async move {
                    let st = q.get("state").cloned().unwrap_or_default();
                    axum::response::Response::builder()
                        .status(302)
                        .header(
                            "Location",
                            format!("/api/portal/oauth/callback?code=the-code&state={st}"),
                        )
                        .body(axum::body::Body::empty())
                        .unwrap()
                },
            ),
        )
        .route(
            "/token",
            axum::routing::post(move || {
                let body = axum::Json(json!({
                    "access_token": "at-1", "token_type": "Bearer",
                    "id_token": id_token,
                }));
                async move { body }
            }),
        );
    let idp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let idp_base = format!("http://{}", idp.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(idp, app).await.unwrap() });

    // --- portal.json:密码 + oauth 指向 mock IdP ---
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(
        dir.path().join("config/portal.json"),
        json!({
            "password_hash": format!("s1${}", bm_surface_http::portal::hash_password("secret1", "s1")),
            "oauth": {
                "issuer": "https://mock-idp",
                "authorization_endpoint": format!("{idp_base}/authorize"),
                "token_endpoint": format!("{idp_base}/token"),
                "client_id": "boenmind-web",
                "client_secret": "sec-1",
                "scopes": ["openid", "email"]
            }
        })
        .to_string(),
    )
    .unwrap();
    let base = spawn(dir.path().to_path_buf()).await;
    let c = client_no_redirect();

    // ① state 端点暴露 oauth 可用
    let (st, body, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/api/portal/state"),
        None,
        None,
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(body["oauth"], json!(true), "{body}");

    // ② 登录入口 302 到 IdP(带 state)
    let resp = c
        .get(format!("{base}/api/portal/oauth/login"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 302);
    let loc = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(loc.starts_with(&format!("{idp_base}/authorize?")), "{loc}");
    let state = loc.split("state=").nth(1).unwrap_or_default().to_string();
    assert!(!state.is_empty());

    // ③ IdP 回跳 callback(带 code+state)→ 换 token → 签发会话
    let resp = c
        .get(format!(
            "{base}/api/portal/oauth/callback?code=the-code&state={state}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 302, "成功后回首页");
    assert_eq!(resp.headers().get("location").unwrap(), "/");
    let cookie = resp
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .to_string();
    assert!(cookie.starts_with("boen_session="), "{cookie}");

    // ④ 会话 cookie 有效:state 端点 authed=true
    let (st, body, _) = send(
        &c,
        reqwest::Method::GET,
        &format!("{base}/api/portal/state"),
        Some(cookie.clone()),
        None,
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(body["authed"], json!(true), "{body}");

    // ⑤ state 重放 = 拒(一次性)
    let resp = c
        .get(format!(
            "{base}/api/portal/oauth/callback?code=the-code&state={state}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401, "state 重放必须拒绝");

    // ⑥ 密码路径回归:oauth 开启不破坏密码登录
    let (st, _body, _) = send(
        &c,
        reqwest::Method::POST,
        &format!("{base}/api/portal/login"),
        None,
        Some(json!({"password": "wrong"})),
    )
    .await;
    assert_eq!(st, 401);
}
