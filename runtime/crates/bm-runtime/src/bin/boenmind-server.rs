//! boenmind-server:M3 守护进程——持有 L2 Runtime Core,经 HTTP Surface
//! (bm-surface-http)对外提供 Wire API(基线 §14 Surface 与核心解耦)。
//!
//! 用法:
//!   boenmind-server [--data-dir <path>] [--bind <addr>]
//!
//! 默认:数据目录 = 平台数据目录/boenmind;绑定 127.0.0.1:7531;
//! 首启生成访问令牌 <data-dir>/token(auth.v0_1 合同)。

use bm_contract::ids::SeqIdGen;
use bm_core::clock::SystemClock;
use bm_core::ports::ModelConnector;
use bm_core::runtime::{DEFAULT_TURN_TIMEOUT_SECS, RuntimeConfig, RuntimeHandle};
use bm_persist::PersistStore;
use bm_providers::mock_model::{MockConnector, Step};
use bm_providers::secret::MemSecretStore;
use std::path::PathBuf;
use std::sync::Arc;

fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("boenmind"))
        .unwrap_or_else(|| PathBuf::from("boenmind-data"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut data_dir = default_data_dir();
    let mut bind = "127.0.0.1:7531".to_string();
    let mut web_dir: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data-dir" => data_dir = PathBuf::from(args.next().expect("--data-dir 需要值")),
            "--bind" => bind = args.next().expect("--bind 需要值"),
            "--web-dir" => web_dir = Some(PathBuf::from(args.next().expect("--web-dir 需要值"))),
            "--help" | "-h" => {
                println!("boenmind-server [--data-dir <path>] [--bind <addr>] [--web-dir <path>]");
                return Ok(());
            }
            other => return Err(format!("未知参数: {other}").into()),
        }
    }

    std::fs::create_dir_all(&data_dir)?;
    let token = bm_surface_http::token::load_or_create(&data_dir)?;
    let (persist, rebuilt) = PersistStore::open_resilient(&data_dir)?;
    if rebuilt {
        eprintln!("警告:状态库损坏,已自事件日志重建投影(损坏文件已隔离)");
    }

    // ID 防撞:重启后计数器跳过历史已用号段(否则 INSERT OR REPLACE 覆盖旧会话)
    let hint = bm_persist::id_counter_hint(persist.state()).unwrap_or(0);
    let id_gen = Arc::new(SeqIdGen::starting_at(hint));

    // M7(ADR-0010):BOEN_MODEL_BASE_URL + BOEN_MODEL_ID 齐备 → OpenAI 兼容真实网关;
    // 密钥只存加密 Secret Store(FileSecretStore,主密钥 BOEN_SECRET_MASTER_KEY,
    // ≥32 字符),首启可用 BOEN_MODEL_API_KEY 播种一次。缺省仍 mock(测试确定性)。
    let (connector, secrets): (
        Arc<dyn ModelConnector>,
        Arc<dyn bm_core::ports::SecretStore>,
    ) = match (
        std::env::var("BOEN_MODEL_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty()),
        std::env::var("BOEN_MODEL_ID")
            .ok()
            .filter(|s| !s.is_empty()),
    ) {
        (Some(base), Some(model)) => {
            let master = std::env::var("BOEN_SECRET_MASTER_KEY")
                .expect("真实网关模式需要 BOEN_SECRET_MASTER_KEY(至少 32 字符)");
            let path = data_dir.join("secrets.enc");
            let store = bm_providers::secret::FileSecretStore::open(path.clone(), &master)
                .expect("打开加密 Secret Store 失败");
            let store: Arc<dyn bm_core::ports::SecretStore> = Arc::new(store);
            let secret_ref = bm_core::runtime::default_secret_ref(&model);
            if bm_core::ports::SecretStore::get(store.as_ref(), &secret_ref).is_err() {
                let seeded = std::env::var("BOEN_MODEL_API_KEY")
                    .expect("密钥库缺该模型凭据:设 BOEN_MODEL_API_KEY 完成首次播种");
                bm_core::ports::SecretStore::put(store.as_ref(), &secret_ref, &seeded)
                    .expect("播种密钥失败");
                eprintln!("模型凭据已加密写入 {}", path.display());
            }
            eprintln!("真实模型网关 {base}(model {model};凭据走加密 Secret Store)");
            (
                Arc::new(bm_providers::openai_http::OpenAiConnector::new(
                    base,
                    store.clone(),
                )),
                store,
            )
        }
        _ => {
            let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::repeating(Step::ok(
                "mock 模型回答(设 BOEN_MODEL_BASE_URL/BOEN_MODEL_ID 接真实网关)",
                120,
                40,
            )));
            let secrets = Arc::new(MemSecretStore::with(
                &bm_core::runtime::default_secret_ref("zhipu.glm-4-flash"),
                "sk-demo-zhipu-secret-value-001",
            ));
            (connector, secrets)
        }
    };
    let store: Arc<dyn bm_persist::EventStore> = Arc::new(persist);

    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities: bm_providers::builtin::builtin_capability_set(),
        version: format!("{}-server", env!("CARGO_PKG_VERSION")),
        data_dir: Some(data_dir.clone()),
        store: Some(store.clone()),
        connector,
        secret_store: secrets,
        id_gen,
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;

    let shutdown = Arc::new(tokio::sync::Notify::new());
    let app = bm_surface_http::router(
        handle.clone(),
        Arc::new(token.clone()),
        store,
        shutdown.clone(),
        web_dir.clone(),
    );
    if let Some(w) = &web_dir {
        println!("Web Surface 目录 {w:?}(GET / 托管静态界面)");
    }
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    let actual = listener.local_addr()?;
    println!(
        "boenmind-server v{} 监听 http://{actual}",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "数据目录 {};访问令牌 {}/token(auth 合同)",
        data_dir.display(),
        data_dir.display()
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(handle, shutdown))
        .await?;
    Ok(())
}

/// 优雅停机(三入口):Ctrl-C、Unix SIGTERM(M3.6 适配)、应用层 /shutdown。
/// 任一触发 → 排空进行中回合(INV-12)→ 退出。
async fn shutdown_signal(handle: RuntimeHandle, shutdown: Arc<tokio::sync::Notify>) {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let term = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => println!("收到 Ctrl-C,排空中……"),
        _ = term => println!("收到 SIGTERM,排空中……"),
        _ = shutdown.notified() => println!("收到 /shutdown,排空中……"),
    }
    println!("排空进行中回合(不被取消,INV-12)……");
    handle.stop("server_shutdown").await;
    println!("排空完成");
}
