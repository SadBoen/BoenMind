//! W1(ADR-0014):OpenAI 兼容插座协议测试——流式帧形状、非流式聚合、
//! 会话续接(X-Bm-Session)、错误形状。壳子对话闭环的后端合同面。

use bm_contract::ids::SeqIdGen;
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use bm_surface_http::token;
use std::sync::Arc;

async fn rig(connector: Arc<dyn ModelConnector>) -> (String, reqwest::Client, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("临时目录");
    let t = token::load_or_create(dir.path()).expect("令牌");
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let ids = Arc::new(SeqIdGen::new());
    let config = RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        async_executor: None,
        model_streaming: true,
        version: "0.1.0-w1".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::with(
            &bm_core::runtime::default_secret_ref("mock-model"),
            "sk-demo",
        )),
        id_gen: ids,
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    };
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let app = bm_surface_http::router(
        RuntimeHandle::start(config).await,
        Arc::new(t),
        store,
        shutdown,
        None,
        Arc::new("mock-model".into()),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (
        format!("http://{addr}"),
        reqwest::Client::builder().build().expect("客户端"),
        dir,
    )
}

fn body(text: &str, stream: bool) -> serde_json::Value {
    serde_json::json!({
        "model": "mock-model",
        "stream": stream,
        "messages": [{"role": "user", "content": text}]
    })
}

// W1 合同:非流式 = 聚合 OpenAI chat.completion 形状 + X-Bm-Session 续聊头
#[tokio::test]
async fn t150_non_stream_completion_and_session_header() {
    let connector = Arc::new(MockConnector::repeating(bm_providers::mock_model::Step::ok(
        "回复内容", 10, 5,
    )));
    let (url, client, _dir) = rig(connector).await;

    let r = client
        .post(format!("{url}/v1/chat/completions"))
        .json(&body("你好", false))
        .send()
        .await
        .expect("请求");
    let status = r.status().as_u16();
    if status != 200 {
        panic!("实际响应 {}: {}", status, r.text().await.unwrap_or_default());
    }
    let session = r
        .headers()
        .get("x-bm-session")
        .expect("应带 X-Bm-Session 响应头")
        .to_str()
        .unwrap()
        .to_string();
    let v: serde_json::Value = r.json().await.expect("JSON");
    assert_eq!(v["object"], serde_json::json!("chat.completion"));
    assert_eq!(v["choices"][0]["message"]["role"], serde_json::json!("assistant"));
    assert_eq!(v["choices"][0]["message"]["content"], serde_json::json!("回复内容"));
    assert_eq!(v["choices"][0]["finish_reason"], serde_json::json!("stop"));
    assert_eq!(v["model"], serde_json::json!("mock-model"));

    // 续聊:同 X-Bm-Session 第二次请求成功(会话复用)
    let r = client
        .post(format!("{url}/v1/chat/completions"))
        .header("x-bm-session", session)
        .json(&body("第二句", false))
        .send()
        .await
        .expect("请求");
    assert_eq!(r.status().as_u16(), 200);
}

// W1 合同:流式 = SSE(role 起手 → delta → finish → [DONE]),内容完整
#[tokio::test]
async fn t151_stream_sse_shape_and_content() {
    let connector = Arc::new(MockConnector::repeating(bm_providers::mock_model::Step::ok(
        "流式回复正文", 10, 5,
    )));
    let (url, client, _dir) = rig(connector).await;

    let r = client
        .post(format!("{url}/v1/chat/completions"))
        .json(&body("讲讲你自己", true))
        .send()
        .await
        .expect("请求");
    assert_eq!(r.status().as_u16(), 200);
    let text = r.text().await.expect("正文");

    // 帧序列:全 data: 前缀,含 role 起手、finish_reason:stop、[DONE] 收口
    // (按 JSON 解析断言,不依赖字段序列化顺序)
    let mut saw_role_start = false;
    let mut saw_finish = false;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data: ") else { continue };
        if data == "[DONE]" { break; }
        let v: serde_json::Value = serde_json::from_str(data).expect("帧为合法 JSON");
        let delta = &v["choices"][0]["delta"];
        if delta["role"] == serde_json::json!("assistant") { saw_role_start = true; }
        if v["choices"][0]["finish_reason"] == serde_json::json!("stop") { saw_finish = true; }
    }
    assert!(saw_role_start, "应有 role 起手帧:{text}");
    assert!(saw_finish, "应有 finish 帧:{text}");
    assert!(text.trim_end().ends_with("data: [DONE]"), "应以 [DONE] 收口");

    // 内容完整:起手空串 + delta/completion 余量拼接 = 全文(字符级不丢不重)
    let mut content = String::new();
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data: ") else { continue };
        if data == "[DONE]" { break; }
        let v: serde_json::Value = serde_json::from_str(data).expect("帧为合法 JSON");
        if let Some(d) = v["choices"][0]["delta"]["content"].as_str() {
            content.push_str(d);
        }
    }
    assert_eq!(content, "流式回复正文", "delta 拼接必须等于全文(不丢字不重字)");
}

// W1 合同:错误形状(OpenAI error 对象)——空消息 / 非法会话
#[tokio::test]
async fn t152_error_shapes() {
    let connector = Arc::new(MockConnector::repeating(bm_providers::mock_model::Step::ok(
        "x", 5, 5,
    )));
    let (url, client, _dir) = rig(connector).await;

    // 空 user 消息
    let r = client
        .post(format!("{url}/v1/chat/completions"))
        .json(&serde_json::json!({"messages": []}))
        .send()
        .await
        .expect("请求");
    assert_eq!(r.status().as_u16(), 400);
    let v: serde_json::Value = r.json().await.expect("JSON");
    assert!(v["error"]["message"].is_string(), "应为 OpenAI error 形状");

    // 未知会话
    let r = client
        .post(format!("{url}/v1/chat/completions"))
        .header("x-bm-session", "sess_00000000000000000000000099")
        .json(&body("hi", false))
        .send()
        .await
        .expect("请求");
    assert_eq!(r.status().as_u16(), 400);
    let v: serde_json::Value = r.json().await.expect("JSON");
    assert!(v["error"]["message"].as_str().unwrap().contains("未知会话"));
}

// W1 合同:GET /v1/models 返回服务器配置模型
#[tokio::test]
async fn t153_models_list() {
    let connector = Arc::new(MockConnector::repeating(bm_providers::mock_model::Step::ok(
        "x", 5, 5,
    )));
    let (url, client, _dir) = rig(connector).await;
    let r = client.get(format!("{url}/v1/models")).send().await.expect("请求");
    assert_eq!(r.status().as_u16(), 200);
    let v: serde_json::Value = r.json().await.expect("JSON");
    assert_eq!(v["data"][0]["id"], serde_json::json!("mock-model"));
}
