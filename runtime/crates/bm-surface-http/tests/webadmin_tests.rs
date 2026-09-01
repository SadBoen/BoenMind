//! W2 管理面测试(webadmin.rs):provider CRUD/打码/校验、连通探针
//! (本地 stub 网关)、MCP 配置管理(过合同 schema)、工作区文件浏览
//! (X-01 路径防护)、当前生效模型落盘。
//!
//! 合同裁决(2026-09-01):/admin/* 壳子私用 REST 面,暂不入冻结库;
//! 本测试文件即该面的行为规格(稳定后入册时由此翻译 schema)。

use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use bm_surface_http::token;
use bm_surface_http::webadmin::AdminConfig;
use serde_json::{json, Value};

use std::sync::Arc;

/// 起一个带 /admin 的完整 surface,返回 (base_url, 临时数据目录)。
async fn spawn_app(ws: std::path::PathBuf, mcp: Option<std::path::PathBuf>) -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("临时目录");
    let t = token::load_or_create(dir.path()).expect("令牌");
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        version: "0.1.0-w2".into(),
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
    let admin = AdminConfig {
        data_dir: dir.path().to_path_buf(),
        workspace_root: ws,
        mcp_config: mcp,
        builtin_caps: Arc::new(vec![json!({"name": "system.echo", "provider": "system.echo", "effect": "read-only", "idempotent": true})]),
        mcp_servers: Arc::new(std::sync::RwLock::new(vec![json!({"name": "demo", "tools": 2})])),
        handle: handle.clone(),
        hub: None,
        secrets: Some(Arc::new(MemSecretStore::new()) as Arc<dyn bm_core::ports::SecretStore>),
    };
    let app = bm_surface_http::router(
        handle,
        Arc::new(t),
        store,
        Arc::new(tokio::sync::Notify::new()),
        None,
        Arc::new("mock-model".into()),
        Some(admin),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    (format!("http://{addr}"), dir)
}

async fn get(url: &str) -> (u16, Value) {
    let resp = reqwest::get(url).await.expect("GET");
    let status = resp.status().as_u16();
    (status, resp.json::<Value>().await.unwrap_or(Value::Null))
}

async fn send_json(method: reqwest::Method, url: &str, body: Value) -> (u16, Value) {
    let client = reqwest::Client::new();
    let resp = client
        .request(method, url)
        .json(&body)
        .send()
        .await
        .expect("请求");
    let status = resp.status().as_u16();
    (status, resp.json::<Value>().await.unwrap_or(Value::Null))
}

// ---- provider CRUD -------------------------------------------------------

#[tokio::test]
async fn t_w2_provider_crud_roundtrip_masking_and_delete() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;

    // 增:回显打码,secretSet=true
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/providers"),
        json!({"name": "OpenCode Go", "baseUrl": "https://opencode.ai/zen/go/v1", "apiKey": "sk-live-1", "models": ["mimo-v2.5"], "defaultModel": "mimo-v2.5"}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    let p = &r["provider"];
    let id = p["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("prov_"));
    assert_eq!(p["name"], json!("OpenCode Go"));
    assert!(p["apiKey"].is_null(), "apiKey 回显必须打码");
    assert_eq!(p["secretSet"], json!(true));

    // 查:列表同样打码,文件里确有明文(与 dev.env 同级口径)
    let (_, list) = get(&format!("{base}/admin/providers")).await;
    assert_eq!(list["providers"].as_array().unwrap().len(), 1);
    assert!(list["providers"][0]["apiKey"].is_null());
    let raw = std::fs::read_to_string(_dir.path().join("config/providers.json")).unwrap();
    assert!(raw.contains("sk-live-1"), "明文只落配置文件");

    // 改:apiKey 留空 = 保持不变;其余字段更新
    let (st, r) = send_json(
        reqwest::Method::PUT,
        &format!("{base}/admin/providers/{id}"),
        json!({"name": "OpenCode Go·改名", "baseUrl": "https://opencode.ai/zen/go/v1", "apiKey": "", "models": ["mimo-v2.5", "gpt-5.6"]}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["provider"]["name"], json!("OpenCode Go·改名"));
    assert_eq!(r["provider"]["secretSet"], json!(true), "留空 = 保持不变");
    assert_eq!(r["provider"]["models"].as_array().unwrap().len(), 2);
    let raw = std::fs::read_to_string(_dir.path().join("config/providers.json")).unwrap();
    assert!(raw.contains("sk-live-1"), "密钥未被清掉");

    // 删:密钥一并没
    let client = reqwest::Client::new();
    let st = client
        .delete(format!("{base}/admin/providers/{id}"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(st, 200);
    let (_, list) = get(&format!("{base}/admin/providers")).await;
    assert_eq!(list["providers"].as_array().unwrap().len(), 0);
    let raw = std::fs::read_to_string(_dir.path().join("config/providers.json")).unwrap();
    assert!(!raw.contains("sk-live-1"), "删 provider = 密钥一并清除");
}

#[tokio::test]
async fn t_w2_provider_validation_and_404() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;

    // baseUrl 非 http(s) 拒;name 空 拒
    let (st, _) = send_json(reqwest::Method::POST, &format!("{base}/admin/providers"),
        json!({"name": "x", "baseUrl": "ftp://nope"})).await;
    assert_eq!(st, 400);
    let (st, _) = send_json(reqwest::Method::POST, &format!("{base}/admin/providers"),
        json!({"name": "", "baseUrl": "https://ok.example.com"})).await;
    assert_eq!(st, 400);

    // 改/删不存在的 id → 404
    let (st, _) = send_json(reqwest::Method::PUT, &format!("{base}/admin/providers/prov_nope"),
        json!({"name": "x", "baseUrl": "https://ok.example.com"})).await;
    assert_eq!(st, 404);
    let client = reqwest::Client::new();
    let st = client.delete(format!("{base}/admin/providers/prov_nope")).send().await.unwrap().status().as_u16();
    assert_eq!(st, 404);
}

// ---- 连通探针(本地 stub 网关)--------------------------------------------

#[tokio::test]
async fn t_w2_probe_ok_parses_models_and_down_reports_error() {
    // stub:OpenAI 兼容 /models
    let stub = axum::Router::new()
        .route(
            "/v1/models",
            axum::routing::get(|| async {
                axum::Json(json!({"data": [{"id": "mimo-v2.5"}, {"id": "gpt-5.6"}]}))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let stub_addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });

    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;

    let (st, r) = send_json(reqwest::Method::POST, &format!("{base}/admin/providers/probe"),
        json!({"baseUrl": format!("http://{stub_addr}/v1"), "apiKey": "sk-x"})).await;
    assert_eq!(st, 200);
    assert_eq!(r["ok"], json!(true), "{r}");
    assert!(r["latencyMs"].as_u64().is_some());
    assert_eq!(r["models"], json!(["mimo-v2.5", "gpt-5.6"]), "模型清单真实解析");

    // 不可达端口 → ok:false + error 摘要(不 500)
    let (st, r) = send_json(reqwest::Method::POST, &format!("{base}/admin/providers/probe"),
        json!({"baseUrl": "http://127.0.0.1:9"})).await;
    assert_eq!(st, 200);
    assert_eq!(r["ok"], json!(false));
    assert!(r["error"].as_str().is_some());

    // baseUrl 非法 → 400
    let (st, _) = send_json(reqwest::Method::POST, &format!("{base}/admin/providers/probe"),
        json!({"baseUrl": "notaurl"})).await;
    assert_eq!(st, 400);
}

// ---- 当前生效模型(config/model.json,重启生效)--------------------------

#[tokio::test]
async fn t_w2_model_active_set_writes_config_file() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;

    // 没 provider 时 404
    let (st, _) = send_json(reqwest::Method::PUT, &format!("{base}/admin/model/active"),
        json!({"providerId": "prov_x"})).await;
    assert_eq!(st, 404);

    // 建 provider(无模型清单)→ 设为当前 → 400(要求先拉清单)
    let (_, r) = send_json(reqwest::Method::POST, &format!("{base}/admin/providers"),
        json!({"name": "Go", "baseUrl": "https://opencode.ai/zen/go/v1", "apiKey": "sk-1"})).await;
    let id = r["provider"]["id"].as_str().unwrap();
    let (st, _) = send_json(reqwest::Method::PUT, &format!("{base}/admin/model/active"),
        json!({"providerId": id})).await;
    assert_eq!(st, 400, "无可用模型必须先拉清单");

    // 拉清单后设为当前 → model.json 落盘 + restartRequired
    let client = reqwest::Client::new();
    let (st, _) = client.request(reqwest::Method::PUT, format!("{base}/admin/providers/{id}"))
        .json(&json!({"name": "Go", "baseUrl": "https://opencode.ai/zen/go/v1", "models": ["mimo-v2.5"], "defaultModel": "mimo-v2.5"}))
        .send().await.map(|x| (x.status().as_u16(), ())).unwrap();
    assert_eq!(st, 200);
    let (st, r) = send_json(reqwest::Method::PUT, &format!("{base}/admin/model/active"),
        json!({"providerId": id})).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["restartRequired"], json!(true));
    let raw = std::fs::read_to_string(_dir.path().join("config/model.json")).unwrap();
    assert!(raw.contains("opencode.ai"));
    assert!(raw.contains("mimo-v2.5"));
    assert!(raw.contains("sk-1"), "密钥随「设为当前」写入 model.json(重启播种)");

    // GET /admin/model/active:投影打码
    let (_, r) = get(&format!("{base}/admin/model/active")).await;
    assert_eq!(r["values"]["modelId"], json!("mimo-v2.5"));
    assert!(r["values"]["apiKey"].is_null());
    assert_eq!(r["secret_set"]["apiKey"], json!(true));
}

// ---- MCP 配置管理 ---------------------------------------------------------

#[tokio::test]
async fn t_w2_mcp_crud_with_contract_schema() {
    let ws = tempfile::tempdir().unwrap();
    let mcfile = tempfile::tempdir().unwrap();
    let mcp_path = mcfile.path().join("mcp.json");
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), Some(mcp_path.clone())).await;

    // 非法条目(name 大写)被合同 schema 拒
    let (st, r) = send_json(reqwest::Method::POST, &format!("{base}/admin/mcp"),
        json!({"name": "BadName", "command": "uvx"})).await;
    assert_eq!(st, 400, "{r}");

    // 合法条目:transport 自动补 stdio;文件落盘
    let (st, r) = send_json(reqwest::Method::POST, &format!("{base}/admin/mcp"),
        json!({"name": "wiki", "command": "uvx", "args": ["mcp-wiki"], "env": {"WIKI_TOKEN": "secret:wiki-token"}})).await;
    assert_eq!(st, 200, "{r}");
    let raw = std::fs::read_to_string(&mcp_path).unwrap();
    assert!(raw.contains("\"transport\": \"stdio\""));
    assert!(raw.contains("secret:wiki-token"), "env 引用形态透传(明文不入配置)");

    // 重名 → 409;编辑 → 替换;删除 → 移除
    let (st, _) = send_json(reqwest::Method::POST, &format!("{base}/admin/mcp"),
        json!({"name": "wiki", "command": "uvx"})).await;
    assert_eq!(st, 409);
    let (st, _) = send_json(reqwest::Method::PUT, &format!("{base}/admin/mcp/wiki"),
        json!({"name": "wiki", "command": "node", "args": ["wiki.js"], "tool_timeout_ms": 5000})).await;
    assert_eq!(st, 200);
    let raw = std::fs::read_to_string(&mcp_path).unwrap();
    assert!(raw.contains("wiki.js"));
    let client = reqwest::Client::new();
    let st = client.delete(format!("{base}/admin/mcp/wiki")).send().await.unwrap().status().as_u16();
    assert_eq!(st, 200);
    let (_, list) = get(&format!("{base}/admin/mcp")).await;
    assert_eq!(list["servers"].as_array().unwrap().len(), 0);
    assert_eq!(list["note"], json!("增删改只落配置文件,重启或「重载」后生效"));
}

#[tokio::test]
async fn t_w2_mcp_disabled_without_config_file() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;
    let (st, r) = get(&format!("{base}/admin/mcp")).await;
    assert_eq!(st, 400, "{r}");
    assert!(r["error"]["message"].as_str().unwrap().contains("--mcp-config"));
}

// ---- 插件(能力)清单 ------------------------------------------------------

#[tokio::test]
async fn t_w2_capabilities_builtin_and_mcp() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;
    let (_, r) = get(&format!("{base}/admin/capabilities")).await;
    assert_eq!(r["builtin"][0]["name"], json!("system.echo"));
    assert_eq!(r["mcp"][0]["name"], json!("demo"));
}

// ---- 工作区文件浏览(X-01 路径防护)---------------------------------------

#[tokio::test]
async fn t_w2_fs_list_and_read_file() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("hello.md"), "# 你好\nBoenMind").unwrap();
    std::fs::create_dir_all(ws.path().join("sub")).unwrap();
    std::fs::write(ws.path().join("sub").join("nested.txt"), "deep").unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;

    // 根列表:目录在前
    let (_, r) = get(&format!("{base}/admin/fs/list?path=")).await;
    let entries = r["entries"].as_array().unwrap();
    assert_eq!(entries[0]["name"], json!("sub"));
    assert_eq!(entries[0]["kind"], json!("dir"));

    // 子目录
    let (_, r) = get(&format!("{base}/admin/fs/list?path=sub")).await;
    assert_eq!(r["entries"][0]["name"], json!("nested.txt"));
    assert_eq!(r["entries"][0]["size"], json!(4));

    // 读文件(中文内容 UTF-8 直读)
    let (_, r) = get(&format!("{base}/admin/fs/file?path=hello.md")).await;
    assert_eq!(r["content"], json!("# 你好\nBoenMind"));

    // 目录当文件读 → 400
    let (st, _) = get(&format!("{base}/admin/fs/file?path=sub")).await;
    assert_eq!(st, 400);
}

#[tokio::test]
async fn t_w2_fs_blocks_traversal_and_absolute_and_symlink() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("ok.txt"), "safe").unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "top-secret").unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;

    // .. 穿越、绝对路径、URL 编码变体:全拒
    for bad in ["..", "a/../..", "../secret.txt", "sub/../../x", "C:/Windows", "/etc/passwd"] {
        let (st, _) = get(&format!("{base}/admin/fs/file?path={}", urlencode(bad))).await;
        assert_eq!(st, 400, "必须拒绝 {bad}");
        let (st, _) = get(&format!("{base}/admin/fs/list?path={}", urlencode(bad))).await;
        assert_eq!(st, 400, "list 必须拒绝 {bad}");
    }

    // 符号链接:拒链(X-01;Windows 无特权建链失败则跳过该子项)
    #[cfg(unix)]
    let link_ok = std::os::unix::fs::symlink(outside.path().join("secret.txt"), ws.path().join("leak.txt")).is_ok();
    #[cfg(windows)]
    let link_ok = std::os::windows::fs::symlink_file(outside.path().join("secret.txt"), ws.path().join("leak.txt")).is_ok();
    if link_ok {
        let (st, r) = get(&format!("{base}/admin/fs/file?path=leak.txt")).await;
        assert_eq!(st, 400, "{r}");
        assert!(r["error"]["message"].as_str().unwrap().contains("符号链接"));
    } else {
        println!("(跳过 symlink 用例:当前环境无建链特权)");
    }

    // 正常路径仍可用
    let (_, r) = get(&format!("{base}/admin/fs/file?path=ok.txt")).await;
    assert_eq!(r["content"], json!("safe"));
}

fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}
