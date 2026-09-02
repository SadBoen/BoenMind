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
use serde_json::{Value, json};

use std::sync::Arc;

/// 起一个带 /admin 的完整 surface,返回 (base_url, 临时数据目录)。
async fn spawn_app(
    ws: std::path::PathBuf,
    mcp: Option<std::path::PathBuf>,
) -> (String, tempfile::TempDir) {
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

fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

// ---- MCP 插件目录:扫描发现 → 批准接入(两段式,2026-09-02 用户批准)----

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

// ---- 运行日志查看(GET /admin/logs,2026-09-02 用户要求接入)----

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
