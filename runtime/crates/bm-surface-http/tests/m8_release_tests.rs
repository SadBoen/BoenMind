//! M8-T5:三平台发行面——Web UI v1(真实资产)服务回归;release 产物
//! 存在性(门控 BOEN_RELEASE=1)。CLI/HTTP 形态回归由 m3/m4/m7 既有
//! e2e 承载;Tauri 壳复用本目录同一前端(shell/tauri,ADR-0009)。

use bm_contract::ids::{IdGen, SeqIdGen};
use bm_contract::wire::Method;
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::builtin::builtin_capability_set;
use bm_providers::mock_model::MockConnector;
use bm_providers::secret::MemSecretStore;
use bm_surface_http::token;
use std::sync::Arc;

fn web_dir() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates
    p.pop(); // runtime
    p.join("web")
}

struct Rig {
    url: String,
    token: String,
    _handle: RuntimeHandle,
    ids: Arc<SeqIdGen>,
    _dir: tempfile::TempDir,
}

async fn rig_with_web() -> Rig {
    let dir = tempfile::tempdir().expect("临时目录");
    let token = Arc::new(token::load_or_create(dir.path()).expect("令牌"));
    let store: Arc<PersistStore> = Arc::new(PersistStore::open(dir.path()).expect("打开"));
    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::new(vec![]));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: builtin_capability_set(),
        async_executor: None,
        model_streaming: false,
        version: "0.1.0-m8".into(),
        data_dir: Some(dir.path().to_path_buf()),
        store: Some(store.clone()),
        connector,
        secret_store: Arc::new(MemSecretStore::with(
            &bm_core::runtime::default_secret_ref("zhipu.glm-4-flash"),
            "sk-demo",
        )),
        id_gen: Arc::new(SeqIdGen::new()),
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;

    let shutdown = Arc::new(tokio::sync::Notify::new());
    let app = bm_surface_http::router(
        handle.clone(),
        token.clone(),
        store.clone(),
        shutdown,
        Some(web_dir()),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定");
    let addr = listener.local_addr().expect("地址");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    Rig {
        url: format!("http://{addr}"),
        token: token.to_string(),
        _handle: handle,
        ids: Arc::new(SeqIdGen::new()),
        _dir: dir,
    }
}

impl Rig {
    fn client(&self, with_auth: bool) -> reqwest::Client {
        let mut b = reqwest::Client::builder();
        if with_auth {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token).parse().expect("头"),
            );
            b = b.default_headers(headers);
        }
        b.build().expect("客户端")
    }

    async fn rpc(
        &self,
        client: &reqwest::Client,
        method: Method,
        params: serde_json::Value,
    ) -> (u16, serde_json::Value) {
        let envelope = serde_json::json!({
            "v": "0.1",
            "method": method.as_str(),
            "request_id": IdGen::next_id(self.ids.as_ref(), "req").as_str(),
            "params": params,
        });
        let r = client
            .post(format!("{}/rpc/{}", self.url, method.as_str()))
            .json(&envelope)
            .send()
            .await
            .expect("请求");
        (r.status().as_u16(), r.json().await.expect("信封 JSON"))
    }
}

/// t120:Web UI v1 服务回归——静态页/健康探针/鉴权 rpc 全部就位;
/// 页面资产含 UI 骨架标记(证明服务的是真实前端而非空目录)。
#[tokio::test]
async fn t120_web_ui_served_and_functional() {
    let rig = rig_with_web().await;
    let anon = rig.client(false);

    // GET / → Web 前端(dsh 官方前端整体复刻,MIT;2026-08-30 起)
    let r = reqwest::get(format!("{}/", rig.url)).await.expect("GET /");
    assert_eq!(r.status().as_u16(), 200);
    let html = r.text().await.expect("正文");
    // 审批面暂离界面(2026-08-30 用户裁决:先对齐 dsh 布局,功能待定回归方案);
    // 后端 approval.* 合同方法不受影响
    assert!(html.contains("__DSH_BOOT__"), "页面含 dsh 启动引导清单");
    assert!(html.contains("/plugins/@deepseek-ai/dsh-client-ui-sidebar/client.js"), "页面加载侧栏模块");
    assert!(html.contains("dsh-typert-registry"), "页面含启动模块清单条目");

    // /health 无鉴权探针
    let r = reqwest::get(format!("{}/health", rig.url))
        .await
        .expect("health");
    assert_eq!(r.status().as_u16(), 200);

    // 鉴权 rpc 可用(页面同款调用路径)
    let authed = rig.client(true);
    let (status, body) = rig
        .rpc(
            &authed,
            Method::CapabilityCall,
            serde_json::json!({"capability": "system.echo", "args": {"msg": "hi"},
                               "idempotency_key": null, "deadline_ms": 1000}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    let _ = anon; // 无鉴权客户端形状验证已在 m3 覆盖
}

/// t120b:release 产物存在性(BOEN_RELEASE=1 门控;产物由
/// `cargo build --release -p bm-runtime -p bm-cli` 产出)。
#[tokio::test]
async fn t120b_release_artifacts_exist() {
    if std::env::var("BOEN_RELEASE").as_deref() != Ok("1") {
        eprintln!("跳过:BOEN_RELEASE 未设(release 构建后运行)");
        return;
    }
    let profile = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/release");
    let server = profile.join("boenmind-server.exe");
    let cli = profile.join("boenmind.exe"); // CLI bin 名为 boenmind([[bin]] name)
    assert!(server.exists(), "server release 产物缺失:{server:?}");
    assert!(cli.exists(), "CLI release 产物缺失:{cli:?}");
    assert!(
        std::fs::metadata(&server).unwrap().len() > 1_000_000,
        "server 产物过小,疑似损坏"
    );
}
