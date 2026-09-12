//! W2 管理面测试(webadmin.rs):provider CRUD/打码/校验、连通探针
//! (本地 stub 网关)、MCP 配置管理(过合同 schema)、工作区文件浏览
//! (X-01 路径防护)、当前生效模型落盘。
//!
//! 合同裁决():/admin/* 壳子私用 REST 面,暂不入冻结库;
//! 本测试文件即该面的行为规格(稳定后入册时由此翻译 schema)。

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

/// 起一个带 /admin 的完整 surface,返回 (base_url, 临时数据目录)。
async fn spawn_app(
    ws: std::path::PathBuf,
    mcp: Option<std::path::PathBuf>,
) -> (String, tempfile::TempDir) {
    spawn_app_with(ws, mcp, None).await
}

/// 同上,可注入官方随包插件目录(bundled_plugins_dir,)。
async fn spawn_app_with(
    ws: std::path::PathBuf,
    mcp: Option<std::path::PathBuf>,
    bundled: Option<std::path::PathBuf>,
) -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("临时目录");
    let t = token::load_or_create(dir.path()).expect("令牌");
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        limits: bm_core::LimitsCell::with_default(),
        job_board: None,
        version: "0.1.0-w2".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::new()),
        id_gen: Arc::new(bm_contract::ids::SeqIdGen::new()),
        clock: Arc::new(SystemClock),
    })
    .await;
    let admin = AdminConfig {
        data_dir: dir.path().to_path_buf(),
        workspace_root: ws,
        mcp_config: mcp,
        builtin_caps: Arc::new(vec![
            json!({"name": "system.echo", "provider": "system.echo", "effect": "read-only", "idempotent": true}),
        ]),
        mcp_servers: Arc::new(std::sync::RwLock::new(vec![
            json!({"name": "demo", "tools": 2}),
        ])),
        handle: handle.clone(),
        hub: None,
        secrets: Some(Arc::new(MemSecretStore::new()) as Arc<dyn bm_core::ports::SecretStore>),
        model_routes: None,
        shutdown: None,
        web_dir: None,
        bundled_plugins_dir: bundled,
        limits: Default::default(),
        limits_sources: Default::default(),
        jobs: None,
        skills: None,
    };
    let app = bm_surface_http::router(
        handle,
        Arc::new(t),
        store,
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

/// issue #13:软删除——删除条目移入历史墓碑(deleted_at),可查可恢复。
#[tokio::test]
async fn t_w2b_provider_soft_delete_history_and_restore() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;

    let (_, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/providers"),
        json!({"name": "gw-a", "baseUrl": "https://a.example.com/v1", "apiKey": "sk-live-1", "models": ["m1"], "defaultModel": "m1"}),
    )
    .await;
    let id = r["provider"]["id"].as_str().unwrap().to_string();

 // 删:活跃清单空,历史 1 条(打码 + 墓碑),明文随条目入历史文件
    let (st, _) = send_json(
        reqwest::Method::DELETE,
        &format!("{base}/admin/providers/{id}"),
        json!({}),
    )
    .await;
    assert_eq!(st, 200);
    let (_, list) = get(&format!("{base}/admin/providers")).await;
    assert_eq!(list["providers"].as_array().unwrap().len(), 0);
    let (_, hist) = get(&format!("{base}/admin/providers/history")).await;
    let entries = hist["history"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], json!(id));
    assert!(entries[0]["deleted_at"].is_u64(), "墓碑时刻必须存在");
    assert!(entries[0]["apiKey"].is_null(), "历史回显同样打码");
    assert_eq!(entries[0]["secretSet"], json!(true));
    let raw = std::fs::read_to_string(_dir.path().join("config/providers.history.json")).unwrap();
    assert!(raw.contains("sk-live-1"), "历史条目保留原文(恢复即全功能)");

 // 恢复:移回活跃库,墓碑摘除,历史清空
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/providers/history/restore"),
        json!({"id": id}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["provider"]["id"], json!(id));
    let (_, list) = get(&format!("{base}/admin/providers")).await;
    assert_eq!(list["providers"].as_array().unwrap().len(), 1);
    assert_eq!(list["providers"][0]["id"], json!(id));
    let (_, hist) = get(&format!("{base}/admin/providers/history")).await;
    assert_eq!(hist["history"].as_array().unwrap().len(), 0);

 // 再恢复 = 404;活跃清单确有原文
    let (st, _) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/providers/history/restore"),
        json!({"id": id}),
    )
    .await;
    assert_eq!(st, 404);
    let raw = std::fs::read_to_string(_dir.path().join("config/providers.json")).unwrap();
    assert!(raw.contains("sk-live-1"));
}

#[tokio::test]
async fn t_w2_provider_validation_and_404() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;

 // baseUrl 非 http(s) 拒;name 空 拒
    let (st, _) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/providers"),
        json!({"name": "x", "baseUrl": "ftp://nope"}),
    )
    .await;
    assert_eq!(st, 400);
    let (st, _) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/providers"),
        json!({"name": "", "baseUrl": "https://ok.example.com"}),
    )
    .await;
    assert_eq!(st, 400);

 // 改/删不存在的 id → 404
    let (st, _) = send_json(
        reqwest::Method::PUT,
        &format!("{base}/admin/providers/prov_nope"),
        json!({"name": "x", "baseUrl": "https://ok.example.com"}),
    )
    .await;
    assert_eq!(st, 404);
    let client = reqwest::Client::new();
    let st = client
        .delete(format!("{base}/admin/providers/prov_nope"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(st, 404);
}

// ---- 连通探针(本地 stub 网关)--------------------------------------------

#[tokio::test]
async fn t_w2_probe_ok_parses_models_and_down_reports_error() {
 // stub:OpenAI 兼容 /models
    let stub = axum::Router::new().route(
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

    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/providers/probe"),
        json!({"baseUrl": format!("http://{stub_addr}/v1"), "apiKey": "sk-x"}),
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(r["ok"], json!(true), "{r}");
    assert!(r["latencyMs"].as_u64().is_some());
    assert_eq!(
        r["models"],
        json!(["mimo-v2.5", "gpt-5.6"]),
        "模型清单真实解析"
    );

 // 不可达端口 → ok:false + error 摘要(不 500)
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/providers/probe"),
        json!({"baseUrl": "http://127.0.0.1:9"}),
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(r["ok"], json!(false));
    assert!(r["error"].as_str().is_some());

 // baseUrl 非法 → 400
    let (st, _) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/providers/probe"),
        json!({"baseUrl": "notaurl"}),
    )
    .await;
    assert_eq!(st, 400);
}

// ---- 当前生效模型(config/model.json,重启生效)--------------------------

#[tokio::test]
async fn t_w2_model_active_set_writes_config_file() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;

 // 没 provider 时 404
    let (st, _) = send_json(
        reqwest::Method::PUT,
        &format!("{base}/admin/model/active"),
        json!({"providerId": "prov_x"}),
    )
    .await;
    assert_eq!(st, 404);

 // 建 provider(无模型清单)→ 设为当前 → 400(要求先拉清单)
    let (_, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/providers"),
        json!({"name": "Go", "baseUrl": "https://opencode.ai/zen/go/v1", "apiKey": "sk-1"}),
    )
    .await;
    let id = r["provider"]["id"].as_str().unwrap();
    let (st, _) = send_json(
        reqwest::Method::PUT,
        &format!("{base}/admin/model/active"),
        json!({"providerId": id}),
    )
    .await;
    assert_eq!(st, 400, "无可用模型必须先拉清单");

 // 拉清单后设为当前 → model.json 落盘 + restartRequired
    let client = reqwest::Client::new();
    let (st, _) = client.request(reqwest::Method::PUT, format!("{base}/admin/providers/{id}"))
        .json(&json!({"name": "Go", "baseUrl": "https://opencode.ai/zen/go/v1", "models": ["mimo-v2.5"], "defaultModel": "mimo-v2.5"}))
        .send().await.map(|x| (x.status().as_u16(), ())).unwrap();
    assert_eq!(st, 200);
    let (st, r) = send_json(
        reqwest::Method::PUT,
        &format!("{base}/admin/model/active"),
        json!({"providerId": id}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["restartRequired"], json!(true));
    let raw = std::fs::read_to_string(_dir.path().join("config/model.json")).unwrap();
    assert!(raw.contains("opencode.ai"));
    assert!(raw.contains("mimo-v2.5"));
    assert!(
        raw.contains("sk-1"),
        "密钥随「设为当前」写入 model.json(重启播种)"
    );

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
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp"),
        json!({"name": "BadName", "command": "uvx"}),
    )
    .await;
    assert_eq!(st, 400, "{r}");

 // 合法条目:transport 自动补 stdio;文件落盘
    let (st, r) = send_json(reqwest::Method::POST, &format!("{base}/admin/mcp"),
        json!({"name": "wiki", "command": "uvx", "args": ["mcp-wiki"], "env": {"WIKI_TOKEN": "secret:wiki-token"}})).await;
    assert_eq!(st, 200, "{r}");
    let raw = std::fs::read_to_string(&mcp_path).unwrap();
    assert!(raw.contains("\"transport\": \"stdio\""));
    assert!(
        raw.contains("secret:wiki-token"),
        "env 引用形态透传(明文不入配置)"
    );

 // 重名 → 409;编辑 → 替换;删除 → 移除
    let (st, _) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp"),
        json!({"name": "wiki", "command": "uvx"}),
    )
    .await;
    assert_eq!(st, 409);
    let (st, _) = send_json(
        reqwest::Method::PUT,
        &format!("{base}/admin/mcp/wiki"),
        json!({"name": "wiki", "command": "node", "args": ["wiki.js"], "tool_timeout_ms": 5000}),
    )
    .await;
    assert_eq!(st, 200);
    let raw = std::fs::read_to_string(&mcp_path).unwrap();
    assert!(raw.contains("wiki.js"));
    let client = reqwest::Client::new();
    let st = client
        .delete(format!("{base}/admin/mcp/wiki"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(st, 200);
    let (_, list) = get(&format!("{base}/admin/mcp")).await;
    assert_eq!(list["servers"].as_array().unwrap().len(), 0);
    assert_eq!(
        list["note"],
        json!("增删改只落配置文件,重启或「重载」后生效")
    );
}

#[tokio::test]
async fn t_w2_mcp_disabled_without_config_file() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;
    let (st, r) = get(&format!("{base}/admin/mcp")).await;
    assert_eq!(st, 400, "{r}");
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--mcp-config")
    );
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
    for bad in [
        "..",
        "a/../..",
        "../secret.txt",
        "sub/../../x",
        "C:/Windows",
        "/etc/passwd",
    ] {
        let (st, _) = get(&format!("{base}/admin/fs/file?path={}", urlencode(bad))).await;
        assert_eq!(st, 400, "必须拒绝 {bad}");
        let (st, _) = get(&format!("{base}/admin/fs/list?path={}", urlencode(bad))).await;
        assert_eq!(st, 400, "list 必须拒绝 {bad}");
    }

 // 符号链接:拒链(X-01;Windows 无特权建链失败则跳过该子项)
 #[cfg(unix)]
    let link_ok = std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        ws.path().join("leak.txt"),
    )
    .is_ok();
 #[cfg(windows)]
    let link_ok = std::os::windows::fs::symlink_file(
        outside.path().join("secret.txt"),
        ws.path().join("leak.txt"),
    )
    .is_ok();
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

// ---- 任意目录浏览(工作目录选择器;只读、仅目录名、绝对路径)--------------

#[tokio::test]
async fn t_w2_fs_browse_lists_dirs_only() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(ws.path().join("beta")).unwrap();
    std::fs::create_dir_all(ws.path().join("alpha")).unwrap();
    std::fs::write(ws.path().join("plain.txt"), "x").unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;

 // 断言对 canonicalize 后(剥 \\?\ 前缀)的形态做:tempdir 平台形差异见 CI 坑
    let canon = std::fs::canonicalize(ws.path()).unwrap();
    let canon_text = {
        let t = canon.display().to_string();
        t.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(t)
    };

 // 根视图:path 为空 → 盘符/根条目非空
    let (_, r) = get(&format!("{base}/admin/fs/browse?path=")).await;
    assert!(
        !r["entries"].as_array().unwrap().is_empty(),
        "根视图必须非空"
    );

 // 目录浏览:仅目录、按名排序;文件不出现;返回规范路径与上级
    let (_, r) = get(&format!(
        "{base}/admin/fs/browse?path={}",
        urlencode(&canon_text)
    ))
    .await;
    assert_eq!(r["path"], json!(canon_text));
    assert!(r["parent"].is_string(), "非根目录必须有上级");
    let entries = r["entries"].as_array().unwrap();
    let names: Vec<&str> = entries
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["alpha", "beta"], "仅目录且按名排序: {names:?}");
    assert!(
        entries
            .iter()
            .all(|e| e["path"].as_str().unwrap().starts_with(&canon_text)),
        "条目路径必须是绝对路径"
    );

 // 文件当目录浏览 → 400;不存在的路径 → 400
    let file_text = {
        let t = canon.join("plain.txt").display().to_string();
        t.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(t)
    };
    let (st, _) = get(&format!(
        "{base}/admin/fs/browse?path={}",
        urlencode(&file_text)
    ))
    .await;
    assert_eq!(st, 400, "文件路径必须拒绝");
    let missing = {
        let t = canon.join("no_such_dir_42").display().to_string();
        t.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(t)
    };
    let (st, _) = get(&format!(
        "{base}/admin/fs/browse?path={}",
        urlencode(&missing)
    ))
    .await;
    assert_eq!(st, 400, "不存在的路径必须拒绝");
}

// ---- 新建目录(选择器「新建文件夹」配套;全盘、单级、重名 409)--------------

#[tokio::test]
async fn t_w9_fs_mkdir_creates_and_validates() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;
    let canon = std::fs::canonicalize(ws.path()).unwrap();
    let parent = {
        let t = canon.display().to_string();
        t.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(t)
    };
    let url = format!("{base}/admin/fs/mkdir");

 // 正常创建:返回剥前缀规范路径,磁盘真实可见
    let (st, r) = send_json(
        reqwest::Method::POST,
        &url,
        json!({ "parent": parent, "name": "新目录" }),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    let expected = {
        let t = canon.join("新目录").display().to_string();
        t.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(t)
    };
    assert_eq!(r["path"], json!(expected));
    assert!(canon.join("新目录").is_dir());

 // 重名 → 409
    let (st, _) = send_json(
        reqwest::Method::POST,
        &url,
        json!({ "parent": parent, "name": "新目录" }),
    )
    .await;
    assert_eq!(st, 409);

 // 非法 name(`..`/含分隔符/空)与坏 parent(空/不存在/是文件)→ 全 400
    std::fs::write(canon.join("plain.txt"), "x").unwrap();
    let file_parent = {
        let t = canon.join("plain.txt").display().to_string();
        t.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(t)
    };
    let missing_parent = {
        let t = canon.join("no_such").display().to_string();
        t.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(t)
    };
    for bad in [
        json!({ "parent": parent, "name": ".." }),
        json!({ "parent": parent, "name": "a/b" }),
        json!({ "parent": parent, "name": "" }),
        json!({ "parent": "", "name": "x" }),
        json!({ "parent": missing_parent, "name": "x" }),
        json!({ "parent": file_parent, "name": "x" }),
    ] {
        let (st, _) = send_json(reqwest::Method::POST, &url, bad).await;
        assert_eq!(st, 400, "非法 body 必须拒绝");
    }
}

// ---- 删除(工作区沙箱;多选批量/目录递归/防逃逸/部分失败逐条结果)----------

#[tokio::test]
async fn t_w9_fs_delete_multi_recursive_and_traversal() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("file.txt"), "x").unwrap();
    std::fs::create_dir_all(ws.path().join("dir").join("inner")).unwrap();
    std::fs::write(ws.path().join("dir").join("inner").join("n.txt"), "y").unwrap();
    std::fs::create_dir_all(ws.path().join("dir2")).unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;
    let url = format!("{base}/admin/fs/delete");

 // 多选批量:文件 + 空目录一起删
    let (st, r) = send_json(
        reqwest::Method::POST,
        &url,
        json!({ "paths": ["file.txt", "dir2"] }),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["ok"], json!(true));
    assert_eq!(r["deleted"], json!(2));
    assert!(!ws.path().join("file.txt").exists());
    assert!(!ws.path().join("dir2").exists());

 // 目录整棵递归删
    let (st, r) = send_json(reqwest::Method::POST, &url, json!({ "paths": ["dir"] })).await;
    assert_eq!(st, 200, "{r}");
    assert!(!ws.path().join("dir").exists());

 // 防逃逸与守门:`..`/绝对路径/空串(=根)/不存在 → 逐条报错不拖累其余;
 // 部分失败整体 ok=false 但仍 200
    let (st, r) = send_json(
        reqwest::Method::POST,
        &url,
        json!({ "paths": ["..", "C:/Windows", "", "gone.txt"] }),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["ok"], json!(false));
    assert_eq!(r["deleted"], json!(0));
    let results = r["results"].as_array().unwrap();
    assert_eq!(results.len(), 4);
    assert!(results.iter().all(|x| x["ok"].as_bool() == Some(false)));
    assert!(results[0]["error"].as_str().unwrap().contains("非法"));

 // 坏 body:paths 缺失 / 空数组 → 400
    let (st, _) = send_json(reqwest::Method::POST, &url, json!({ "paths": [] })).await;
    assert_eq!(st, 400);
    let (st, _) = send_json(reqwest::Method::POST, &url, json!({})).await;
    assert_eq!(st, 400);
}

// 只删一次,结果不混入必然失败的 NotFound;完全重复项同理收口。
#[tokio::test]
async fn t_fs_delete_merges_ancestors_and_duplicates() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(ws.path().join("dirA").join("sub")).unwrap();
    std::fs::write(ws.path().join("dirA").join("sub").join("n.txt"), "y").unwrap();
    std::fs::write(ws.path().join("dup.txt"), "x").unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;
    let url = format!("{base}/admin/fs/delete");

 // 祖孙同批(子项在前、父目录在后):归并后只剩父目录一条删除记录
    let (st, r) = send_json(
        reqwest::Method::POST,
        &url,
        json!({ "paths": ["dirA/sub/n.txt", "dirA"] }),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["deleted"], json!(1), "{r}");
    assert_eq!(r["results"].as_array().unwrap().len(), 1, "{r}");
    assert!(!ws.path().join("dirA").exists());

 // 完全重复项:只删一次,单条成功记录
    let (st, r) = send_json(
        reqwest::Method::POST,
        &url,
        json!({ "paths": ["dup.txt", "dup.txt"] }),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["deleted"], json!(1), "{r}");
    assert_eq!(r["results"].as_array().unwrap().len(), 1, "{r}");
    assert!(!ws.path().join("dup.txt").exists());
}

fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

// ---- MCP 插件目录:扫描发现 → 批准接入(两段式,)----

#[tokio::test]
async fn t_w2_mcp_scan_candidates_and_approve() {
    let ws = tempfile::tempdir().unwrap();
    let mcfile = tempfile::tempdir().unwrap();
    let mcp_path = mcfile.path().join("mcp.json");
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), Some(mcp_path.clone())).await;
    let plugins_dir = mcfile.path().join("mcp");
    std::fs::create_dir_all(&plugins_dir).unwrap();

 // 假候选:回显单行声明 JSON(平台门控;声明用纯 ASCII——cmd 按系统代码页
 // 解释脚本文件,非 ASCII 会被 GBK 等弄坏引号结构;生产 exe 由 Rust 直写
 // UTF-8 无此问题)
    let decl = r#"{"name":"fake_plugin","title":"Fake Plugin","description":"test candidate","config_schema":[{"key":"k","label":"K","type":"string","default":""}],"suggested_entry":{"transport":"stdio","args":["--config","{config_file}"],"tool_timeout_ms":12345,"restart_limit":3}}"#;
    let candidate = {
 #[cfg(windows)]
        {
            let p = plugins_dir.join("fake-plugin.cmd");
            std::fs::write(&p, format!("@echo {decl}")).unwrap();
            p
        }
 #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = plugins_dir.join("fake-plugin.sh");
            std::fs::write(&p, format!("#!/bin/sh\necho '{decl}'\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }
    };
 // 非可执行/非候选文件不进清单
    std::fs::write(plugins_dir.join("readme.txt"), "not a plugin").unwrap();

 // 扫描:发现 fake_plugin,registered=false
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp/candidates"),
        json!({}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    let cands = r["candidates"].as_array().unwrap();
    assert_eq!(cands.len(), 1, "{r}");
    assert_eq!(cands[0]["name"], json!("fake_plugin"));
    assert_eq!(cands[0]["registered"], json!(false));
    assert!(r["dir"].as_str().unwrap().ends_with("mcp"));

 // 批准:落盘 mcp.json 条目(args 模板替换 {config_file})+ manifest 双写
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp/approve"),
        json!({"name": "fake_plugin"}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    let entry = &r["entry"];
    assert_eq!(entry["name"], json!("fake_plugin"));
    assert_eq!(entry["transport"], json!("stdio"));
    assert_eq!(entry["tool_timeout_ms"], json!(12345));
    let args = entry["args"].as_array().unwrap();
    assert_eq!(args[0], json!("--config"));
    assert!(
        args[1].as_str().unwrap().contains("mcp-fake_plugin.json"),
        "{args:?}"
    );
    assert!(args[1].as_str().unwrap().contains("config"), "{args:?}");

    let raw = std::fs::read_to_string(&mcp_path).unwrap();
    assert!(raw.contains("fake_plugin"));
    let manifest =
        std::fs::read_to_string(mcfile.path().join("manifests/fake_plugin.manifest.json")).unwrap();
    assert!(manifest.contains("Fake Plugin"));
    assert!(manifest.contains("config_schema"));

 // 重名批准 → 409;重扫 → registered=true
    let (st, _) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp/approve"),
        json!({"name": "fake_plugin"}),
    )
    .await;
    assert_eq!(st, 409);
    let (_, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp/candidates"),
        json!({}),
    )
    .await;
    assert_eq!(r["candidates"][0]["registered"], json!(true));

 // 目录外的同声明文件不可被 approve(路径限定在插件目录)
    let _ = candidate; // 候选路径仅用于落盘条目;approve 只在插件目录内搜索
}

// 写一个 --self-describe 假候选(平台门控;声明用纯 ASCII,理由同上)
fn write_fake_candidate(dir: &std::path::Path, name: &str) {
    let decl = format!(
        r#"{{"name":"{name}","title":"{name}","description":"candidate","suggested_entry":{{"transport":"stdio","args":["--config","{{config_file}}"]}}}}"#
    );
 #[cfg(windows)]
    {
        let p = dir.join(format!("{name}.cmd"));
        std::fs::write(&p, format!("@echo {decl}")).unwrap();
    }
 #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(format!("{name}.sh"));
        std::fs::write(&p, format!("#!/bin/sh\necho '{decl}'\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[tokio::test]
async fn t_w2_mcp_bundled_plugins_scan_approve_and_dedupe() {
 // 官方随包目录(exe 同级 plugins/)候选可被扫描发现、可批准落盘;
 // 同名候选以数据目录 mcp/ 优先()。
    let ws = tempfile::tempdir().unwrap();
    let mcfile = tempfile::tempdir().unwrap();
    let mcp_path = mcfile.path().join("mcp.json");
    let bundled = tempfile::tempdir().unwrap();
    write_fake_candidate(bundled.path(), "bundled_plugin");
    let (base, _dir) = spawn_app_with(
        ws.path().to_path_buf(),
        Some(mcp_path.clone()),
        Some(bundled.path().to_path_buf()),
    )
    .await;

 // 扫描:随包候选在场且 source=bundled;响应带 bundled_dir
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp/candidates"),
        json!({}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["candidates"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["candidates"][0]["name"], json!("bundled_plugin"));
    assert_eq!(r["candidates"][0]["source"], json!("bundled"));
    assert!(r["bundled_dir"].as_str().is_some(), "{r}");

 // 批准:command 落在随包目录(候选实际路径),条目照常落盘
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp/approve"),
        json!({"name": "bundled_plugin"}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    let cmd = r["entry"]["command"].as_str().unwrap();
    assert!(cmd.contains("bundled"), "command 应指向随包目录: {cmd}");
    assert!(
        std::fs::read_to_string(&mcp_path)
            .unwrap()
            .contains("bundled_plugin")
    );

 // 同名去重:数据目录再放同名候选,扫描只出一条且 source=data
    let data_dir = mcfile.path().join("mcp");
    std::fs::create_dir_all(&data_dir).unwrap();
    write_fake_candidate(&data_dir, "bundled_plugin");
    let (_, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp/candidates"),
        json!({}),
    )
    .await;
    let cands = r["candidates"].as_array().unwrap();
    let rows: Vec<&Value> = cands
        .iter()
        .filter(|c| c["name"] == json!("bundled_plugin"))
        .collect();
    assert_eq!(rows.len(), 1, "同名候选必须去重: {r}");
    assert_eq!(rows[0]["source"], json!("data"));
}

// ---- 运行日志查看(GET /admin/logs,)----

#[tokio::test]
async fn t_w2_logs_tail_reads_data_dir_jsonl() {
    let ws = tempfile::tempdir().unwrap();
    let (base, dir) = spawn_app(ws.path().to_path_buf(), None).await;
 // 预置两份日志(各 3 行,验证尾部读取与透传)
    std::fs::write(
        dir.path().join("execution-log.jsonl"),
        "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("events.jsonl"), "{\"e\":1}\n{\"e\":2}\n").unwrap();

    let (_, r) = get(&format!("{base}/admin/logs")).await;
    assert_eq!(r["ok"], json!(true), "{r}");
    let exec = r["exec"].as_array().unwrap();
    assert_eq!(exec.len(), 3, "{r}");
    assert_eq!(exec[2], json!("{\"a\":3}"));
    let events = r["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);

 // 文件不存在 = 空数组,不报错(起第二个 app,数据目录天然无日志文件)
    let ws2 = tempfile::tempdir().unwrap();
    let (base2, _dir2) = spawn_app(ws2.path().to_path_buf(), None).await;
    let (_, r2) = get(&format!("{base2}/admin/logs")).await;
    assert_eq!(r2["ok"], json!(true));
    assert_eq!(r2["exec"].as_array().unwrap().len(), 0);
}

// ---- W8:常规设置(工作区注册表 + 运行环境探针,ADR-0018)--------------------

#[tokio::test]
async fn t_w8_workspaces_seeded_crud_and_guards() {
    let ws = tempfile::tempdir().unwrap();
    let proj = tempfile::tempdir().unwrap(); // 另一个真实目录
    let (base, data_dir) = spawn_app(ws.path().to_path_buf(), None).await;

 // 首次 GET 播种 default = 文件浏览根
    let (_, r) = get(&format!("{base}/admin/workspaces")).await;
    let list = r["workspaces"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{r}");
    assert_eq!(list[0]["id"], json!("default"));
    assert_eq!(list[0]["isDefault"], json!(true));
    assert_eq!(list[0]["exists"], json!(true));
 // 播种已落盘
    let raw = std::fs::read_to_string(data_dir.path().join("config/workspaces.json")).unwrap();
    assert!(raw.contains("default"));

 // 增:合法目录
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/workspaces"),
        json!({"name": "项目甲", "path": proj.path().display().to_string()}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    let wid = r["workspace"]["id"].as_str().unwrap().to_string();
    assert!(wid.starts_with("ws_"));

 // 增:重复路径拒;不存在路径拒;文件路径拒;空名称拒
    let (st, _) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/workspaces"),
        json!({"name": "重复", "path": proj.path().display().to_string()}),
    )
    .await;
    assert_eq!(st, 409);
    let (st, _) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/workspaces"),
        json!({"name": "不存在", "path": "Z:/no/such/dir"}),
    )
    .await;
    assert_eq!(st, 400);
    let (st, _) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/workspaces"),
        json!({"name": "", "path": proj.path().display().to_string()}),
    )
    .await;
    assert_eq!(st, 400);

 // 改:改名生效
    let (st, r) = send_json(
        reqwest::Method::PUT,
        &format!("{base}/admin/workspaces/{wid}"),
        json!({"name": "项目甲·改名"}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["workspace"]["name"], json!("项目甲·改名"));

 // 检测:存在目录 ok
    let (_, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/workspaces/{wid}/check"),
        json!({}),
    )
    .await;
    assert_eq!(r["ok"], json!(true), "{r}");

 // 删:default 拒;其他可删
    let client = reqwest::Client::new();
    let st = client
        .delete(format!("{base}/admin/workspaces/default"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(st, 400, "default 必须拒删");
    let st = client
        .delete(format!("{base}/admin/workspaces/{wid}"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(st, 200);
    let (_, r) = get(&format!("{base}/admin/workspaces")).await;
    assert_eq!(r["workspaces"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn t_w8_runtime_env_shape() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;
    let (_, r) = get(&format!("{base}/admin/runtime/env")).await;
 // 形状断言(不假设测试机装没装 Python/Node):installed/version/program 键在
    for key in ["python", "node"] {
        assert!(r[key]["installed"].is_boolean(), "{r}");
        if r[key]["installed"] == json!(true) {
            assert!(r[key]["version"].is_string(), "{r}");
            assert!(r[key]["program"].is_string(), "{r}");
        } else {
            assert!(r[key]["error"].is_string(), "{r}");
        }
    }
}

/// 会话历史回放端点():按 session 过滤
/// user_message/assistant_final,文件序(真实时序)返回;坏行跳过;其他
/// 会话不串;skip/limit 分页与 has_more 正确。
#[tokio::test]
async fn t_session_messages_replay_filters_and_orders() {
    let ws = tempfile::tempdir().expect("工作区");
    let (base, dir) = spawn_app(ws.path().to_path_buf(), None).await;

 // 手工落 context-log.jsonl(文件序=时序):目标会话 4 行 + 其他会话 1 行 + 坏行
    let log_path = dir.path().join("context-log.jsonl");
    std::fs::write(
        &log_path,
        concat!(
            r#"{"seq":1,"ts":"T1","session_id":"sess_A","operation_id":"op_1","turn_index":1,"kind":"user_message","data":{"content":"第一问"}}"#,
            "
",
            r#"{"seq":2,"ts":"T2","session_id":"sess_A","operation_id":"op_1","turn_index":1,"kind":"tool_call","data":{"tool":"fs_search"}}"#,
            "
",
            r#"{"seq":3,"ts":"T3","session_id":"sess_A","operation_id":"op_1","turn_index":1,"kind":"assistant_final","data":{"content":"第一轮回复"}}"#,
            "
",
            r#"{"seq":4,"ts":"T4","session_id":"sess_B","operation_id":"op_9","turn_index":1,"kind":"user_message","data":{"content":"别的会话不串"}}"#,
            "
",
            "not-a-json-line
",
            r#"{"seq":5,"ts":"T5","session_id":"sess_A","operation_id":"op_1","turn_index":2,"kind":"user_message","data":{"content":"第二问"}}"#,
            "
",
            r#"{"seq":6,"ts":"T6","session_id":"sess_A","operation_id":"op_1","turn_index":2,"kind":"assistant_final","data":{"content":"第二轮回复2"}}"#,
            "
",
        ),
    )
    .expect("写日志");

    let (st, v) = get(&format!("{base}/admin/sessions/sess_A/messages")).await;
    assert_eq!(st, 200);
    assert_eq!(v["ok"], json!(true));
    let msgs = v["messages"].as_array().expect("消息数组");
    let roles: Vec<&str> = msgs
        .iter()
        .map(|m| m["role"].as_str().expect("role"))
        .collect();
    assert_eq!(
        roles,
        vec!["user", "assistant", "user", "assistant"],
        "过滤+时序角色映射"
    );
    assert_eq!(msgs[0]["content"], json!("第一问"));
    assert_eq!(msgs[1]["content"], json!("第一轮回复"));
    assert_eq!(msgs[2]["content"], json!("第二问"));
    assert_eq!(msgs[3]["content"], json!("第二轮回复2"));
 // 全量页(4 条 ≤ 默认 50):无更早
    assert_eq!(v["has_more"], json!(false));

 // 分页:skip=2 跳过最新 2 条 → 第一轮一问一答;还有更早?没有
    let (st2, v2) = get(&format!(
        "{base}/admin/sessions/sess_A/messages?limit=2&skip=2"
    ))
    .await;
    assert_eq!(st2, 200);
    let page = v2["messages"].as_array().expect("分页数组");
    let page_roles: Vec<&str> = page.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(page_roles, vec!["user", "assistant"], "skip=2 取最早 2 条");
    assert_eq!(page[0]["content"], json!("第一问"));
    assert_eq!(v2["has_more"], json!(false), "已到最早");

 // 分页:skip=3,limit=1 → 跳过最新 3 条,剩最早 1 条(matched=4 ≤ 3+1,无更早)
    let (_, v3) = get(&format!(
        "{base}/admin/sessions/sess_A/messages?limit=1&skip=3"
    ))
    .await;
    let page3 = v3["messages"].as_array().expect("数组");
    assert_eq!(page3.len(), 1);
    assert_eq!(page3[0]["content"], json!("第一问"));
    assert_eq!(v3["has_more"], json!(false));

 // 其他会话互不串
    let (st4, v4) = get(&format!("{base}/admin/sessions/sess_B/messages")).await;
    assert_eq!(st4, 200);
    assert_eq!(v4["messages"].as_array().expect("数组").len(), 1);
    assert_eq!(v4["messages"][0]["content"], json!("别的会话不串"));

 // 不存在的会话 = 空数组(非错误)
    let (_, v5) = get(&format!("{base}/admin/sessions/sess_X/messages")).await;
    assert_eq!(v5["messages"].as_array().expect("数组").len(), 0);
}

// ---- ADR-0023:官方默认安装 / 墓碑 / purge / 弃用标记 ---------------------

#[tokio::test]
async fn t_w2_mcp_bundled_seeding_tombstone_and_approve_revival() {
    let ws = tempfile::tempdir().unwrap();
    let mcfile = tempfile::tempdir().unwrap();
    let mcp_path = mcfile.path().join("mcp.json");
    let bundled = tempfile::tempdir().unwrap();
    write_fake_candidate(bundled.path(), "official_plugin");
 // 同声明名的第二个文件(换装拷贝场景)不得重复播种
 #[cfg(windows)]
    std::fs::copy(
        bundled.path().join("official_plugin.cmd"),
        bundled.path().join("official_plugin_copy.cmd"),
    )
    .unwrap();
 #[cfg(unix)]
    std::fs::copy(
        bundled.path().join("official_plugin.sh"),
        bundled.path().join("official_plugin_copy.sh"),
    )
    .unwrap();
    let (base, dir) = spawn_app_with(
        ws.path().to_path_buf(),
        Some(mcp_path.clone()),
        Some(bundled.path().to_path_buf()),
    )
    .await;
    let data_dir = dir.path().to_path_buf();

 // 播种:未登记+无墓碑 → 默认安装(mcp.json + manifest 双写)
    let seeded =
        bm_surface_http::webadmin::seed_bundled_plugins(&mcp_path, bundled.path(), &data_dir).await;
    assert_eq!(
        seeded,
        vec!["official_plugin".to_string()],
        "应播种官方插件"
    );
    assert!(
        std::fs::read_to_string(&mcp_path)
            .unwrap()
            .contains("official_plugin")
    );
    assert!(
        mcfile
            .path()
            .join("manifests/official_plugin.manifest.json")
            .exists(),
        "manifest 应双写"
    );

 // 卸载(bundled 来源)→ 墓碑在册 → 再播种不复活
    let (st, r) = send_json(
        reqwest::Method::DELETE,
        &format!("{base}/admin/mcp/official_plugin"),
        json!({}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["origin"], json!("bundled"), "{r}");
    assert_eq!(r["tombstoned"], json!(true), "{r}");
    let seeded2 =
        bm_surface_http::webadmin::seed_bundled_plugins(&mcp_path, bundled.path(), &data_dir).await;
    assert!(seeded2.is_empty(), "墓碑在册不得复活: {seeded2:?}");
    assert!(
        !std::fs::read_to_string(&mcp_path)
            .unwrap()
            .contains("official_plugin")
    );

 // 显式批准 → 清墓碑 + 恢复安装(恢复唯一正路)
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp/approve"),
        json!({"name": "official_plugin"}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert!(
        std::fs::read_to_string(&mcp_path)
            .unwrap()
            .contains("official_plugin")
    );
    let tomb = std::fs::read_to_string(data_dir.join("config/mcp-removed.json")).unwrap();
    assert!(!tomb.contains("official_plugin"), "墓碑应已清除: {tomb}");
}

#[tokio::test]
async fn t_w2_mcp_purge_deletes_files_and_tombstones() {
    let ws = tempfile::tempdir().unwrap();
    let mcfile = tempfile::tempdir().unwrap();
    let mcp_path = mcfile.path().join("mcp.json");
    let (base, dir) = spawn_app(ws.path().to_path_buf(), Some(mcp_path.clone())).await;
 // 手工登记一个数据目录来源插件:exe/manifest/config 三件齐全
    let plugins_dir = mcfile.path().join("mcp");
    std::fs::create_dir_all(&plugins_dir).unwrap();
    let exe = plugins_dir.join("purge-me.bin");
    std::fs::write(&exe, b"fake exe bytes").unwrap();
    let config_dir = mcfile.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config_file = config_dir.join("mcp-purge_me.json");
    std::fs::write(&config_file, "{}").unwrap();
    let manifests_dir = mcfile.path().join("manifests");
    std::fs::create_dir_all(&manifests_dir).unwrap();
    let manifest_file = manifests_dir.join("purge_me.manifest.json");
    std::fs::write(&manifest_file, "{}").unwrap();
    std::fs::write(
        &mcp_path,
        format!(
            r#"[{{"name":"purge_me","transport":"stdio","command":"{}","args":["--config","{}"]}}]"#,
            exe.display().to_string().replace('\\', "\\\\"),
            config_file.display().to_string().replace('\\', "\\\\"),
        ),
    )
    .unwrap();

    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp/purge_me/purge"),
        json!({}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(r["deleted"].as_array().unwrap().len(), 3, "{r}");
    assert!(!exe.exists(), "exe 应被物理删除");
    assert!(!config_file.exists(), "每插件配置应被删除");
    assert!(!manifest_file.exists(), "manifest 应被删除");
    assert!(
        !std::fs::read_to_string(&mcp_path)
            .unwrap()
            .contains("purge_me"),
        "mcp.json 条目应摘除"
    );
    let tomb = std::fs::read_to_string(dir.path().join("config/mcp-removed.json")).unwrap();
    assert!(tomb.contains("purge_me"), "应写墓碑: {tomb}");

 // 再 purge → 404(未登记无从定位文件)
    let (st, _) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/mcp/purge_me/purge"),
        json!({}),
    )
    .await;
    assert_eq!(st, 404);
}

#[tokio::test]
async fn t_w2_mcp_list_marks_bundled_deprecated() {
    let ws = tempfile::tempdir().unwrap();
    let mcfile = tempfile::tempdir().unwrap();
    let mcp_path = mcfile.path().join("mcp.json");
    let bundled = tempfile::tempdir().unwrap();
 // 登记一个 command 指向随包目录的插件
    let cmd = bundled.path().join("retired-plugin.exe");
    std::fs::write(&cmd, b"fake").unwrap();
    std::fs::write(
        &mcp_path,
        format!(
            r#"[{{"name":"retired_plugin","transport":"stdio","command":"{}","args":[]}}]"#,
            cmd.display().to_string().replace('\\', "\\\\")
        ),
    )
    .unwrap();
 // 官方清单不含它 → deprecated=true;含 → false;清单缺失 → 不标记
    let official = bundled.path().join(".official.json");
    std::fs::write(&official, r#"{"plugins":["other_plugin"]}"#).unwrap();
    let (base, _dir) = spawn_app_with(
        ws.path().to_path_buf(),
        Some(mcp_path.clone()),
        Some(bundled.path().to_path_buf()),
    )
    .await;
    let (_, r) = get(&format!("{base}/admin/mcp")).await;
    assert_eq!(r["entries"][0]["origin"], json!("bundled"), "{r}");
    assert_eq!(r["entries"][0]["deprecated"], json!(true), "{r}");

    std::fs::write(&official, r#"{"plugins":["retired_plugin"]}"#).unwrap();
    let (_, r) = get(&format!("{base}/admin/mcp")).await;
    assert_eq!(r["entries"][0]["deprecated"], json!(false), "{r}");

    std::fs::remove_file(&official).unwrap();
    let (_, r) = get(&format!("{base}/admin/mcp")).await;
    assert_eq!(
        r["entries"][0]["deprecated"],
        json!(false),
        "清单缺失=不标记"
    );
}

// ---- 配置损坏防护()------------------------------
// skills/roles 损坏 JSON 必须拒绝加载与覆写(500),不得静默回落空库/默认
// 文档后被下一次保存整库覆写清盘(与 providers 同口径)。

#[tokio::test]
async fn t_w2_skills_roundtrip_and_corrupt_rejects_overwrite() {
    let ws = tempfile::tempdir().unwrap();
    let (base, dir) = spawn_app(ws.path().to_path_buf(), None).await;
 // 新库:空清单可读
    let (st, r) = get(&format!("{base}/admin/skills")).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["skills"], json!([]), "{r}");
 // 正常写入→读回
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/skills"),
        json!({"skill_id": "skill_t1", "name": "T1", "instruction": "do"}),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    let (_, r) = get(&format!("{base}/admin/skills")).await;
    assert_eq!(r["skills"][0]["skill_id"], json!("skill_t1"), "{r}");
 // 人为写坏后:读取拒绝(500)
    let skills_file = dir.path().join("config").join("skills.json");
    let corrupt = r#"{"skills": [{"skill_id": "keep", "name": "K""#; // 半截 JSON
    std::fs::write(&skills_file, corrupt).unwrap();
    let (st, r) = get(&format!("{base}/admin/skills")).await;
    assert_eq!(st, 500, "{r}");
 // 删除同样拒绝(先于 NOT_FOUND 语义触达读取)
    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{base}/admin/skills/keep"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 500);
 // 盘上损坏文件原样保留(未被覆写清盘)
    assert_eq!(
        std::fs::read_to_string(&skills_file).unwrap(),
        corrupt,
        "盘上损坏文件不得被覆写"
    );
}

#[tokio::test]
async fn t_w2_corrupt_roles_json_rejects_load_and_overwrite() {
    let ws = tempfile::tempdir().unwrap();
    let (base, dir) = spawn_app(ws.path().to_path_buf(), None).await;
    let roles_file = dir.path().join("config").join("roles.json");
    std::fs::create_dir_all(roles_file.parent().unwrap()).unwrap();
    let corrupt = r#"{"roles": [{"id": "keep""#; // 半截 JSON
    std::fs::write(&roles_file, corrupt).unwrap();

 // 读:损坏拒绝(500)
    let (st, r) = get(&format!("{base}/admin/roles")).await;
    assert_eq!(st, 500, "{r}");
 // 保存(单角色形态)同样拒绝
    let (st, r) = send_json(
        reqwest::Method::POST,
        &format!("{base}/admin/roles"),
        json!({"id": "x", "name": "X", "system_prompt": "p"}),
    )
    .await;
    assert_eq!(st, 500, "{r}");
 // 盘上损坏文件原样保留
    assert_eq!(
        std::fs::read_to_string(&roles_file).unwrap(),
        corrupt,
        "盘上损坏文件不得被覆写"
    );
 // 合法 JSON 但缺 roles 数组(且非旧版单 system_prompt 形态)同属损坏口径
    std::fs::write(&roles_file, r#"{"foo": 1}"#).unwrap();
    let (st, _) = get(&format!("{base}/admin/roles")).await;
    assert_eq!(st, 500);
}

// ---- issue #18 裁决:管理面响应形状夹具 ----
// 前端 w2/api.ts 的类型是手抄的,本组夹具锁住各 GET 端点的顶层键集:
// 后端改动任何顶层字段,此处先红 = w2/api.ts 必须显式同步。
// 新增顶层字段 = 有意变更:更新期望值 + 同步 api.ts 类型 + 本注释。

async fn top_keys(c: &reqwest::Client, url: &str) -> Vec<String> {
    let body: serde_json::Value = c
        .get(url)
        .send()
        .await
        .expect("GET")
        .json()
        .await
        .expect("JSON body");
    let mut keys: Vec<String> = body
        .as_object()
        .expect("顶层必须是对象")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

#[tokio::test]
async fn t_admin_response_shape_anchors() {
    let ws = tempfile::tempdir().unwrap();
    let (base, _dir) = spawn_app(ws.path().to_path_buf(), None).await;
    let c = reqwest::Client::new();
    let cases: &[(&str, &[&str])] = &[
        (
            "/admin/sessions",
            &["limit", "ok", "sessions", "skip", "total", "truncated"],
        ),
        ("/admin/skills", &["ok", "skills"]),
        ("/admin/roles", &["active_id", "ok", "roles"]),
        ("/admin/limits", &["keys", "ok"]),
        ("/admin/jobs", &["jobs", "ok"]),
 // /admin/mcp 不入夹具:rig 未接 --mcp-config,该端点在 rig 下走
 // error 降级形状;成功形状待 MCP e2e rig 就绪后补锁。
        ("/admin/workspaces", &["workspaces"]),
        ("/admin/context", &["ok", "steps"]),
        ("/admin/providers/health", &["health", "ok"]),
        ("/admin/logs", &["context", "events", "exec", "ok"]),
        ("/admin/capabilities", &["builtin", "mcp", "note"]),
        ("/admin/approvals", &["approvals"]),
    ];
    for (path, want) in cases {
        let got = top_keys(&c, &format!("{base}{path}")).await;
        assert_eq!(
            &got, want,
            "{path} 顶层键漂移——若为有意变更:同步 w2/api.ts 类型 + 本夹具期望值"
        );
    }
}
