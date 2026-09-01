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
use serde_json::json;
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
    let mut mcp_config: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data-dir" => data_dir = PathBuf::from(args.next().expect("--data-dir 需要值")),
            "--bind" => bind = args.next().expect("--bind 需要值"),
            "--web-dir" => web_dir = Some(PathBuf::from(args.next().expect("--web-dir 需要值"))),
            "--mcp-config" => {
                let v = args.next().expect("--mcp-config 需要值");
                mcp_config = Some(PathBuf::from(v));
            }
            "--help" | "-h" => {
                println!(
                    "boenmind-server [--data-dir <path>] [--bind <addr>] [--web-dir <path>] [--mcp-config <path>]"
                );
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

    // M7(ADR-0010):生效模型接入 = config/model.json > 启动 env(W2 从归档
    // 恢复接线,ADR-0012);base+model 齐备 → OpenAI 兼容真实网关。密钥只存
    // 加密 Secret Store(FileSecretStore,主密钥 BOEN_SECRET_MASTER_KEY,
    // ≥32 字符),首启可用 BOEN_MODEL_API_KEY(env 或 model.json)播种一次。
    // 缺省仍 mock(测试确定性)。
    let eff = bm_surface_http::config_store::effective_model(&data_dir);
    let (connector, secrets): (
        Arc<dyn ModelConnector>,
        Arc<dyn bm_core::ports::SecretStore>,
    ) = match (&eff.base_url, &eff.model_id) {
        (Some(base), Some(model)) => {
            let master = std::env::var("BOEN_SECRET_MASTER_KEY")
                .expect("真实网关模式需要 BOEN_SECRET_MASTER_KEY(至少 32 字符)");
            let path = data_dir.join("secrets.enc");
            let store = bm_providers::secret::FileSecretStore::open(path.clone(), &master)
                .expect("打开加密 Secret Store 失败");
            let store: Arc<dyn bm_core::ports::SecretStore> = Arc::new(store);
            let secret_ref = bm_core::runtime::default_secret_ref(model);
            if bm_core::ports::SecretStore::get(store.as_ref(), &secret_ref).is_err() {
                let seeded = eff.api_key.clone().expect(
                    "密钥库缺该模型凭据:设 BOEN_MODEL_API_KEY(或在设置页保存 provider 密钥)完成首次播种",
                );
                bm_core::ports::SecretStore::put(store.as_ref(), &secret_ref, &seeded)
                    .expect("播种密钥失败");
                eprintln!("模型凭据已加密写入 {}", path.display());
            }
            eprintln!("真实模型网关 {base}(model {model};凭据走加密 Secret Store)");
            (
                Arc::new(bm_providers::openai_http::OpenAiConnector::new(
                    base.clone(),
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

    // M7.2/M7.7:--mcp-config 显式安装清单(= 用户批准)→ 握手发现 →
    // 动态注册 + 异步执行器装配;env 明文只进子进程(INV-5)
    let mut capabilities = bm_providers::builtin::builtin_capability_set();
    // W2 管理面注入面:内置能力摘要(= mcp 注入前的 capabilities)
    let builtin_caps: Vec<serde_json::Value> = capabilities
        .iter()
        .filter_map(|(m, _)| serde_json::to_value(m).ok())
        .map(|v| {
            json!({
                "name": v["capability"], "provider": v["provider"],
                "effect": v["effect"], "idempotent": v["idempotent"],
                "approval": v["approval"],
            })
        })
        .collect();
    let mut mcp_loaded: Vec<serde_json::Value> = Vec::new();
    let mut mcp_executor: Option<Arc<dyn bm_core::ports::AsyncCapabilityExecutor>> = None;
    // McpHub::new() 自返回 Arc(内部 OnceLock 全局共享)
    let hub: Option<Arc<bm_providers::mcp::McpHub>> = mcp_config
        .as_ref()
        .map(|_| bm_providers::mcp::McpHub::new());
    if let Some(cfg) = &mcp_config {
        let hub = hub.as_ref().expect("hub 已构造");
        let setups = bm_providers::mcp::load_mcp_setups(cfg, secrets.as_ref())?;
        for setup in setups {
            let transport = bm_providers::mcp::StdioMcpTransport::spawn(
                &setup.command,
                &setup.args,
                &setup.env_resolved,
            )?;
            let manifests = hub
                .connect(&setup.name, transport, setup.tool_timeout_ms)
                .await?;
            println!(
                "MCP server {} 已接入:{} 个工具",
                setup.name,
                manifests.len()
            );
            mcp_loaded.push(json!({ "name": setup.name, "tools": manifests.len() }));
            capabilities.extend(bm_providers::mcp::McpHub::capability_entries(manifests));
        }
        mcp_executor = Some(hub.clone() as Arc<dyn bm_core::ports::AsyncCapabilityExecutor>);
    }

    // W2 管理面:工作区根(BOEN_WORKSPACE_DIR > <data-dir>/workspace)
    let workspace_root = std::env::var("BOEN_WORKSPACE_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("workspace"));
    std::fs::create_dir_all(&workspace_root)?;
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities,
        async_executor: mcp_executor,
        model_streaming: {
            let on = std::env::var("BOEN_MODEL_STREAM").as_deref() == Ok("1");
            eprintln!("启动配置:模型流式 = {on}");
            on
        },
        version: format!("{}-server", env!("CARGO_PKG_VERSION")),
        data_dir: Some(data_dir.clone()),
        store: Some(store.clone()),
        connector,
        secret_store: secrets.clone(),
        id_gen,
        clock: Arc::new(SystemClock),
        turn_timeout_secs: DEFAULT_TURN_TIMEOUT_SECS,
        max_attempts: None,
    })
    .await;

    std::fs::create_dir_all(&workspace_root)?;
    // W2 管理面注入(handle 就绪后构造:热装载走 actor 命令)
    let admin = bm_surface_http::webadmin::AdminConfig {
        data_dir: data_dir.clone(),
        workspace_root,
        mcp_config: mcp_config.clone(),
        builtin_caps: Arc::new(builtin_caps),
        mcp_servers: Arc::new(std::sync::RwLock::new(mcp_loaded)),
        handle: handle.clone(),
        hub: hub.clone(),
        secrets: Some(secrets.clone()),
    };

    // P0(第四轮评审):INV-5 脱敏接线——把模型凭据明文注册进 Execution
    // Log 扫描面,此后任何日志条目命中即整条降格,密钥明文禁止落盘。
    // W2:凭据来源 = 生效配置合并值(model.json 或 env)。
    if let Some(key_value) = &eff.api_key {
        handle.register_redaction_value(key_value);
    }

    let shutdown = Arc::new(tokio::sync::Notify::new());
    // W1(ADR-0014):/v1 插座与会话创建的默认模型 = 生效模型(文件>env)
    let default_model = Arc::new(
        eff.model_id
            .clone()
            .unwrap_or_else(|| "zhipu.glm-4-flash".to_string()),
    );
    let app = bm_surface_http::router(
        handle.clone(),
        Arc::new(token.clone()),
        store,
        shutdown.clone(),
        web_dir.clone(),
        default_model.clone(),
        Some(admin),
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
