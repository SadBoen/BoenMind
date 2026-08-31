//! M10-T1 配置管理 API(ADR-0012):/rpc 四方法(Bearer)与 /api 喂食口
//! (公开挂载,已登记欠账)行为一致;打码、留空不改、删除、非法值拒绝。

use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::Method;
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::mock_model::{MockConnector, Step};
use bm_providers::secret::MemSecretStore;
use bm_surface_http::token;
use serde_json::json;
use std::sync::Arc;

struct Rig {
    url: String,
    token: String,
    dir: tempfile::TempDir,
}

async fn rig() -> Rig {
    let dir = tempfile::tempdir().expect("临时目录");
    let token = Arc::new(token::load_or_create(dir.path()).expect("令牌"));
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![Step::ok("答", 4, 1)]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        version: "0.1.0-m1".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::with("secret:model.x", "sk-x")),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let app = bm_surface_http::router(
        handle,
        token.clone(),
        store,
        shutdown,
        None,
        Some(dir.path().to_path_buf()),
        None,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    Rig { url: format!("http://{addr}"), token: token.to_string(), dir }
}

fn authed(rig: &Rig) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {}", rig.token).parse().expect("头"),
    );
    reqwest::Client::builder().default_headers(headers).build().expect("客户端")
}

fn anon() -> reqwest::Client {
    reqwest::Client::new()
}

async fn rpc_config(rig: &Rig, method: Method, params: serde_json::Value) -> serde_json::Value {
    let ids = SeqIdGen::new();
    let envelope = json!({
        "v": "0.1",
        "method": method.as_str(),
        "request_id": IdGen::next_id(&ids, "req").as_str(),
        "params": params,
    });
    let r = authed(rig)
        .post(format!("{}/rpc/{}", rig.url, method.as_str()))
        .json(&envelope)
        .send()
        .await
        .expect("请求");
    assert_eq!(r.status().as_u16(), 200, "业务错误走信封不走状态码");
    r.json().await.expect("信封 JSON")
}

/// dsh 界面喂食口:POST /api/{method},body = client-request 信封。
async fn api_call(rig: &Rig, method: &str, payload: serde_json::Value) -> serde_json::Value {
    let envelope = json!({
        "type": "client-request",
        "rpcId": format!("t-{method}"),
        "method": method,
        "payload": payload,
    });
    let r = anon()
        .post(format!("{}/api/{}", rig.url, method))
        .json(&envelope)
        .send()
        .await
        .expect("请求");
    assert_eq!(r.status().as_u16(), 200);
    let body: serde_json::Value = r.json().await.expect("信封 JSON");
    body["result"].clone()
}

/// M10-T1a:/rpc config CRUD 全流程——list 注册表、set 落盘、get 打码、
/// 留空不改、delete 字段/整节。
#[tokio::test]
async fn t150_config_rpc_crud_masking_and_delete() {
    let rig = rig().await;

    // list:v0 只有 model 节,apiKey 标记 secret
    let r = rpc_config(&rig, Method::ConfigList, json!({})).await;
    assert_eq!(r["ok"], true);
    let sections = r["result"]["sections"].as_array().expect("sections");
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0]["ns"], "model");
    assert_eq!(sections[0]["file"], "config/model.json");
    let fields = sections[0]["fields"].as_array().expect("fields");
    assert!(fields.iter().any(|f| f["name"] == "apiKey" && f["secret"] == true));

    // get 初始:无配置无 env → 全空,密钥未设置
    let r = rpc_config(&rig, Method::ConfigGet, json!({"ns": "model"})).await;
    assert_eq!(r["ok"], true);
    assert_eq!(r["result"]["values"]["apiKey"], json!(null), "回显恒打码");
    assert_eq!(r["result"]["secret_set"]["apiKey"], false);

    // set:写入 + 密钥不回显
    let r = rpc_config(
        &rig,
        Method::ConfigSet,
        json!({"ns": "model", "values": {
            "baseUrl": "https://api.example.com/v1", "apiKey": "sk-secret-1", "modelId": "glm-5.3"
        }}),
    )
    .await;
    assert_eq!(r["ok"], true);
    assert_eq!(r["result"]["values"]["modelId"], "glm-5.3");
    assert_eq!(r["result"]["values"]["apiKey"], json!(null), "写入后回显仍打码");
    assert_eq!(r["result"]["secret_set"]["apiKey"], true);
    // 人可读文件确已落盘
    let raw = std::fs::read_to_string(rig.dir.path().join("config/model.json")).expect("文件");
    assert!(raw.contains("https://api.example.com/v1"));

    // set 留空密钥 = 保持不变;非法值拒绝且不落盘
    let r = rpc_config(
        &rig,
        Method::ConfigSet,
        json!({"ns": "model", "values": {"apiKey": null, "modelId": "glm-5-air"}}),
    )
    .await;
    assert_eq!(r["result"]["secret_set"]["apiKey"], true, "留空 = 不改");
    let r = rpc_config(&rig, Method::ConfigSet, json!({"ns": "model", "values": {"baseUrl": "ftp://x"}})).await;
    assert_eq!(r["ok"], false, "非法 baseUrl 拒绝");
    assert_eq!(r["error"]["code"], "validation_failed");

    // delete 字段 → 密钥清除;delete 整节 → 文件复位
    let r = rpc_config(&rig, Method::ConfigDelete, json!({"ns": "model", "field": "apiKey"})).await;
    assert_eq!(r["result"]["secret_set"]["apiKey"], false);
    let r = rpc_config(&rig, Method::ConfigDelete, json!({"ns": "model"})).await;
    assert_eq!(r["ok"], true);
    assert_eq!(r["result"]["values"]["modelId"], json!(null));
    assert!(!rig.dir.path().join("config/model.json").exists(), "整节复位 = 删文件");
}

/// M10-T1b:/api 喂食口与 /rpc 行为一致(同一 ConfigStore 实现);
/// 无鉴权可写是**已登记欠账**(公网部署前必须补,ADR-0009 T-13/T-14)。
#[tokio::test]
async fn t151_config_api_glue_matches_rpc() {
    let rig = rig().await;

    let r = api_call(
        &rig,
        "config.set",
        json!({"ns": "model", "values": {"baseUrl": "https://glue.example.com/v1", "modelId": "m-1"}}),
    )
    .await;
    assert_eq!(r["ok"], true);
    assert_eq!(r["value"]["values"]["modelId"], "m-1");

    // /rpc 读到同一份文件
    let r = rpc_config(&rig, Method::ConfigGet, json!({"ns": "model"})).await;
    assert_eq!(r["result"]["values"]["baseUrl"], "https://glue.example.com/v1");

    // 未知配置节拒绝
    let r = api_call(&rig, "config.get", json!({"ns": "nope"})).await;
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "bad-request");
}
