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
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data-dir" => data_dir = PathBuf::from(args.next().expect("--data-dir 需要值")),
            "--bind" => bind = args.next().expect("--bind 需要值"),
            "--help" | "-h" => {
                println!("boenmind-server [--data-dir <path>] [--bind <addr>]");
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

    let connector: Arc<dyn ModelConnector> = Arc::new(MockConnector::repeating(Step::ok(
        "M3 演示回答(当前为 mock 模型;真实 GLM 适配器经 --feature glm 接入)",
        120,
        40,
    )));
    let secrets = Arc::new(MemSecretStore::with(
        &bm_core::runtime::default_secret_ref("zhipu.glm-4-flash"),
        "sk-demo-zhipu-secret-value-001",
    ));
    let store: Arc<dyn bm_persist::EventStore> = Arc::new(persist);

    let handle = RuntimeHandle::start(RuntimeConfig {
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

    let app = bm_surface_http::router(handle.clone(), Arc::new(token.clone()), store);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    println!(
        "boenmind-server v{} 监听 http://{bind}",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "数据目录 {};访问令牌 {}/token(auth 合同)",
        data_dir.display(),
        data_dir.display()
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(handle))
        .await?;
    Ok(())
}

/// 优雅停机:Ctrl-C / 终止信号 → 排空进行中回合(INV-12)→ 退出。
async fn shutdown_signal(handle: RuntimeHandle) {
    let _ = tokio::signal::ctrl_c().await;
    println!("收到停机信号,排空中(进行中回合不被取消,INV-12)……");
    handle.stop("server_shutdown").await;
    println!("排空完成");
}
