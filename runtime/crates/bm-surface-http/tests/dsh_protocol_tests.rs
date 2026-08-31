//! M10-T2/T3 dsh 宿主协议方法级测试(此前为零):模型目录由生效配置驱动、
//! selectModel 记忆、prompt 懒建真会话 + 事件翻译回流 + history 投影。

use bm_contract::ids::SeqIdGen;
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::mock_model::{MockConnector, Step};
use bm_providers::secret::MemSecretStore;
use bm_surface_http::token;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

struct Rig {
    url: String,
    dir: tempfile::TempDir,
}

async fn rig(script: Vec<Step>) -> Rig {
    let dir = tempfile::tempdir().expect("临时目录");
    let token = Arc::new(token::load_or_create(dir.path()).expect("令牌"));
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(script));
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
        token,
        store,
        shutdown,
        None,
        Some(dir.path().to_path_buf()),
        None,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    Rig { url: format!("http://{addr}"), dir }
}

impl Rig {
    /// dsh unary 调用,返回 result 槽。
    async fn api(&self, method: &str, payload: serde_json::Value) -> serde_json::Value {
        let envelope = json!({
            "type": "client-request",
            "rpcId": format!("t-{}", uuid_like()),
            "method": method,
            "payload": payload,
        });
        let client = reqwest::Client::new();
        let r = client
            .post(format!("{}/api/{}", self.url, method))
            .json(&envelope)
            .send()
            .await
            .expect("请求");
        assert_eq!(r.status().as_u16(), 200, "{method} 恒 200");
        let body: serde_json::Value = r.json().await.expect("信封 JSON");
        body["result"].clone()
    }
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    format!("t{:x}", C.fetch_add(1, Ordering::Relaxed))
}

/// 写模型配置(直接落配置文件,模拟「界面保存 + 重启生效」之后的状态)。
fn seed_model_config(rig: &Rig) {
    let dir = rig.dir.path().join("config");
    std::fs::create_dir_all(&dir).expect("建目录");
    std::fs::write(
        dir.join("model.json"),
        json!({
            "baseUrl": "https://api.example.com/v1",
            "apiKey": "sk-test",
            "modelId": "glm-5.3",
            "displayName": "我的网关"
        })
        .to_string(),
    )
    .expect("写配置");
}

/// M10-T2a:settings.describe 喂 llm-pi-ai 静态 schema——「自定义提供方」
/// 按钮的解锁条件(此前为登记失败项 #10:按钮恒灰)。
#[tokio::test]
async fn t152_settings_describe_feeds_llm_pi_ai_schema() {
    let rig = rig(vec![Step::ok("答", 4, 1)]).await;
    let r = rig.api("settings.describe", json!({})).await;
    assert_eq!(r["ok"], true);
    assert_eq!(r["value"]["writable"], true);
    let namespaces = r["value"]["namespaces"].as_array().expect("namespaces");
    let ns = namespaces
        .iter()
        .find(|n| n["ns"] == "llm-pi-ai")
        .expect("llm-pi-ai 命名空间存在(表单解锁前提)");
    // Schemastery uid/refs 信封:providers 为 dict,档案含 api union
    let schema = &ns["schema"];
    assert!(schema["refs"].is_object(), "uid/refs 信封");
    let providers = &schema["refs"]["1"];
    assert_eq!(providers["type"], "dict");
    let api = &schema["refs"]["4"];
    assert_eq!(api["type"], "union");
    // list 存 uid 引用,wire 上是数字;前端 rehydrate 时按 refs 解引用。
    // 三种协议按 dsh 原版(用户截图),Chat Completions 放首位为默认。
    let first = api["list"][0].as_u64().expect("union 子项为 uid 引用").to_string();
    assert_eq!(schema["refs"][&first]["value"], "Chat Completions (/chat/completions)");
    assert_eq!(api["list"].as_array().map(|a| a.len()), Some(3));
}

/// M10-T2b:配置齐备 → llm.providers/llm.models/session.models 出自定义组;
/// selectModel 记忆并回显;未知选择拒绝。
#[tokio::test]
async fn t153_model_directory_driven_by_config() {
    let rig = rig(vec![Step::ok("答", 4, 1)]).await;
    seed_model_config(&rig);

    let r = rig.api("llm.providers", json!({})).await;
    let providers = r["value"]["providers"].as_array().expect("providers");
    let custom = providers
        .iter()
        .find(|p| p["provider"] == "boenmind-custom")
        .expect("配置驱动的自定义提供方行");
    assert_eq!(custom["displayName"], "我的网关");
    assert_eq!(custom["declared"], true);
    assert_eq!(custom["settingsNs"], "llm-pi-ai");
    // 单一数据源:自定义提供方配置齐备时,目录只出它自己(设置页与模型选择器对齐)
    assert_eq!(providers.len(), 1, "自定义配置优先,env 网关行不再并存");

    let r = rig.api("llm.models", json!({})).await;
    assert_eq!(r["value"]["groups"][0]["id"], "boenmind-custom");
    assert_eq!(r["value"]["groups"][0]["models"][0]["id"], "glm-5.3");

    // session.models:routable 解锁输入框;current 默认指向配置好的模型(同源对齐)
    let r = rig.api("session.models", json!({"sessionId": "sess_1"})).await;
    assert_eq!(r["value"]["routable"], true);
    assert_eq!(r["value"]["current"], json!({"provider": "boenmind-custom", "model": "glm-5.3"}));

    // selectModel:合法选择记忆 + 回显;未知选择拒绝
    let r = rig
        .api("session.selectModel", json!({"sessionId": "sess_1", "provider": "boenmind-custom", "model": "glm-5.3"}))
        .await;
    assert_eq!(r["value"]["selected"]["provider"], "boenmind-custom");
    let r = rig.api("session.models", json!({"sessionId": "sess_1"})).await;
    assert_eq!(r["value"]["current"]["model"], "glm-5.3");
    let r = rig
        .api("session.selectModel", json!({"sessionId": "sess_1", "provider": "boenmind-custom", "model": "nope"}))
        .await;
    assert_eq!(r["ok"], false);
}

/// M10-T3:prompt 懒建真会话 → 用户消息立即上屏 → 模型回复经事件翻译
/// 回流为 assistant/message → history 投影可回放。
#[tokio::test]
async fn t154_prompt_loop_translates_events_to_history() {
    let rig = rig(vec![Step::ok("测试回复内容", 8, 2)]).await;
    seed_model_config(&rig);

    // 建会话(登记层)→ 发消息
    let r = rig.api("session.create", json!({"workspaceId": "ws_1"})).await;
    let sid = r["value"]["sessionId"].as_str().expect("sessionId").to_string();
    let r = rig
        .api(
            "session.prompt",
            json!({"sessionId": sid, "mode": "queue", "content": [{"type": "text", "text": "你好"}]}),
        )
        .await;
    assert_eq!(r["ok"], true, "prompt 接受:{}", r);
    assert_eq!(r["value"]["accepted"], true);

    // 用户消息立即入投影
    let r = rig.api("session.history", json!({"sessionId": sid})).await;
    let events = r["value"]["events"].as_array().expect("events");
    assert!(
        events.iter().any(|e| e["event"]["type"] == "user/message"),
        "用户消息事件在 history"
    );

    // 等模型回合完成并被翻译为 assistant/message(mock 异步回合 + 200ms 轮询)
    let mut settled = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let r = rig.api("session.history", json!({"sessionId": sid})).await;
        let events = r["value"]["events"].as_array().expect("events");
        let reply = events.iter().find(|e| e["event"]["type"] == "assistant/message");
        if let Some(e) = reply {
            assert_eq!(
                e["event"]["data"]["message"]["content"][0]["text"],
                json!("测试回复内容"),
                "终稿文本来自 runtime 事件 content"
            );
            assert_eq!(e["event"]["surfaceOp"], "append", "surface 追加标记");
            settled = true;
            break;
        }
    }
    assert!(settled, "5s 内应收到翻译后的 assistant/message");

    // 懒建映射生效:runtime 侧确有会话,事件已入持久日志(重放非空)
    let r = rig.api("session.history", json!({"sessionId": sid})).await;
    let events = r["value"]["events"].as_array().expect("events");
    assert!(events.iter().any(|e| e["event"]["type"] == "step/start"), "回合开始事件已翻译");
}

/// M10-T3b:未配置任何模型时 prompt 显式拒绝(model-unavailable),不静默。
#[tokio::test]
async fn t155_prompt_without_model_rejects() {
    let rig = rig(vec![Step::ok("答", 4, 1)]).await;
    let r = rig.api("session.create", json!({"workspaceId": "ws_1"})).await;
    let sid = r["value"]["sessionId"].as_str().expect("sessionId").to_string();
    let r = rig
        .api(
            "session.prompt",
            json!({"sessionId": sid, "mode": "queue", "content": [{"type": "text", "text": "你好"}]}),
        )
        .await;
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "model-unavailable");
}
