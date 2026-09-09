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
use bm_core::runtime::{RuntimeConfig, RuntimeHandle};
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
    // 可观测性(2026-09-07 双开毒化排障):此前 main 未装 subscriber,持久层
    // 进入拒写态等 tracing::error! 全部静默蒸发,故障可见性为零。默认 info,
    // 可用 RUST_LOG 覆盖(如 RUST_LOG=info,bm_core=debug)。
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
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
    // W10(ADR-0024):运行时限制 = config/limits.json(缺省=代码默认);
    // env BOEN_TURN_TIMEOUT_SECS 优先级保留(load 内折算并记来源)。
    let (limits_cell, limits_sources) =
        bm_core::limits::load_limits(&data_dir.join("config").join("limits.json"));
    // W10(ADR-0025):后台作业台账(日志落 <data>/jobs/)。
    let job_table = bm_providers::jobs::JobTable::new(&data_dir, limits_cell.clone());
    let token = bm_surface_http::token::load_or_create(&data_dir)?;
    // 双开毒化根治(2026-09-07 本机实测;扩展 W7 2026-09-03 VPS 修复):
    // 绑定必须先于状态库打开——抢端口失败的实例若先开库,会在死前完成
    // 恢复重放+中断回合重驱(含模型调用),与赢家并发写同一状态库(事件
    // 序号撞车+SQLite 写竞争),把赢家推入粘性拒写态,界面表现=「Runtime
    // 排空中或持久层故障,拒绝新会话」且日志零痕迹。升级子进程保留 ≤60s
    // 等待重试;常规启动占用即退(单进程铁律)。
    let listener = if std::env::var("BOEN_UPGRADE_CHILD").as_deref() == Ok("1") {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            match tokio::net::TcpListener::bind(&bind).await {
                Ok(l) => break l,
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                    if std::time::Instant::now() >= deadline {
                        return Err(e.into());
                    }
                    eprintln!("[W7] 等待旧实例退出(端口占用中)……");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    } else {
        tokio::net::TcpListener::bind(&bind).await.map_err(|e| {
            eprintln!(
                "端口 {bind} 绑定失败: {e}(同数据目录仅允许一个实例;端口被占=已有实例或守护残留)"
            );
            e
        })?
    };
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
                &bm_core::runtime::default_secret_ref(bm_core::runtime::DEFAULT_MODEL_ID),
                "sk-demo-zhipu-secret-value-001",
            ));
            (connector, secrets)
        }
    };
    let store: Arc<dyn bm_persist::EventStore> = Arc::new(persist);

    // M7.2/M7.7:--mcp-config 显式安装清单(= 用户批准)→ 握手发现 →
    // 动态注册 + 异步执行器装配;env 明文只进子进程(INV-5)
    // 生产服务仅装载生产级内置能力，移除历史测试桩(mail.mock_send/notes等)防模型误判
    let mut capabilities = bm_providers::builtin::production_builtin_capability_set();
    // W9 日常可用批:system.exec 内置命令执行(审批类,常规 agent 设计)
    capabilities.extend([
        bm_providers::system_exec::exec_capability_entry(),
        bm_providers::system_exec::job_output_capability_entry(),
    ]);
    // ADR-0021:fs.* 文件工具集内置(查/读直通,写/改审批;沙箱=工作区注册表)
    capabilities.extend(bm_providers::fs_tools::fs_capability_entries());
    // issue #2:上下文压缩独立工具(确定性抽取摘要落盘,回合组装面注入前缀)
    capabilities.extend(bm_providers::context_compress::capability_entries(
        data_dir.clone(),
    ));
    // Skill v0.2(ADR-0016 第二步):skills.json 声明 scripts 的技能 →
    // wasmtime 执行面(manifests 进能力面,执行体挂 skill 分道)。
    let (skills, skill_entries) = load_skill_scripts(&data_dir);
    capabilities.extend(skill_entries);
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
    // ADR-0023:官方随包插件默认安装——bundled 目录候选(未登记且不在
    // 墓碑)先按批准同款形状落盘 mcp.json + manifest,再走下方统一装载
    // 一次上线;用户卸载/删除过的官方插件经墓碑永不复活,显式批准即除名。
    if let (Some(cfg_path), Some(exe_dir)) = (
        mcp_config.as_deref(),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf())),
    ) {
        let bundled = exe_dir.join("plugins");
        if bundled.is_dir() {
            let _seeded =
                bm_surface_http::webadmin::seed_bundled_plugins(cfg_path, &bundled, &data_dir)
                    .await;
        }
    }
    if let (Some(cfg_path), Some(hub)) = (mcp_config.as_deref(), hub.as_ref()) {
        // F-07:启动装载与热装载同调 supervisor(消除双写);
        // 启动侧 registrar = 收集 entries 进 capabilities,无注销语义。
        struct CollectRegistrar {
            entries: std::sync::Mutex<
                Vec<(
                    bm_contract::capability::CapabilityManifest,
                    Arc<dyn bm_core::registry::CapabilityProvider>,
                )>,
            >,
        }
        #[async_trait::async_trait]
        impl bm_providers::mcp::supervisor::CapabilityRegistrar for CollectRegistrar {
            async fn register(
                &self,
                entries: Vec<(
                    bm_contract::capability::CapabilityManifest,
                    Arc<dyn bm_core::registry::CapabilityProvider>,
                )>,
            ) -> Result<(), String> {
                self.entries.lock().expect("锁").extend(entries);
                Ok(())
            }
            async fn unregister(&self, _names: Vec<String>) -> Result<(), String> {
                Ok(()) // 启动时无已装载,不触达
            }
        }

        let secrets: Arc<dyn bm_core::ports::SecretStore> = secrets.clone();
        let registrar = CollectRegistrar {
            entries: std::sync::Mutex::new(Vec::new()),
        };
        let outcome = bm_providers::mcp::supervisor::sync_from_config(
            hub,
            cfg_path,
            secrets,
            Vec::new(),
            &registrar,
            &limits_cell,
        )
        .await;
        let collected = registrar.entries.lock().expect("锁").clone();
        capabilities.extend(collected);
        mcp_loaded = outcome.note_loaded;
        for f in &outcome.failed {
            eprintln!(
                "[MCP] 装载失败 (已跳过): {}",
                f["error"].as_str().unwrap_or("")
            );
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
    // W6:对话级模型路由——单连接器插槽装路由器(按 body.model/model_override
    // 分发到各 provider 网关;未命中回落默认连接器)。启动即按 providers.json
    // 建表+播种密钥;此后管理面增删改 provider 免重启热重建(webadmin rebuild)。
    let model_routes = Arc::new(bm_providers::routing::RoutingConnector::new(
        connector.clone(),
    ));
    let handle = RuntimeHandle::start(RuntimeConfig {
        capabilities,
        async_executor: {
            // ADR-0021:fs.* 与 system.exec 走内置异步执行体,其余回落 MCP hub
            let fs = bm_providers::fs_tools::FsExecutor::with_limits(
                data_dir.clone(),
                workspace_root.clone(),
                limits_cell.clone(),
            );
            let exec_inner = Arc::new(bm_providers::system_exec::ExecExecutor::new(
                limits_cell.clone(),
                job_table.clone(),
                data_dir.clone(),
                workspace_root.clone(),
            ));
            let inner: Arc<dyn bm_core::ports::AsyncCapabilityExecutor> = match mcp_executor {
                Some(hub) => hub,
                None => exec_inner.clone(),
            };
            // Skill v0.2(ADR-0016 第二步):装载 skills.json 中带 scripts 的
            // 技能 → 编译 wasm 合成 manifests 注册进能力面;执行体挂 skill 分道。
            let exec: Arc<dyn bm_core::ports::AsyncCapabilityExecutor> =
                Arc::new(bm_providers::system_exec::SplitExecutor {
                    exec: exec_inner,
                    fs,
                    skills,
                    fallback: inner,
                });
            exec.into()
        },
        model_streaming: {
            let on = eff.stream;
            eprintln!("启动配置:模型流式 = {on} (来源: config/model.json stream 优先, BOEN_MODEL_STREAM 环境变量兜底)");
            on
        },
        limits: limits_cell.clone(),
        job_board: Some(job_table.clone()),
        version: format!("{}-server", env!("CARGO_PKG_VERSION")),
        data_dir: Some(data_dir.clone()),
        store: Some(store.clone()),
        connector: model_routes.clone(),
        secret_store: secrets.clone(),
        id_gen,
        clock: Arc::new(SystemClock),
        turn_timeout_secs: limits_cell.get().model_call_timeout_secs as i64,
        max_attempts: None,
    })
    .await;

    // W2 管理面注入(handle 就绪后构造:热装载走 actor 命令)
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let admin = bm_surface_http::webadmin::AdminConfig {
        data_dir: data_dir.clone(),
        workspace_root,
        mcp_config: mcp_config.clone(),
        builtin_caps: Arc::new(builtin_caps),
        mcp_servers: Arc::new(std::sync::RwLock::new(mcp_loaded)),
        handle: handle.clone(),
        hub: hub.clone(),
        secrets: Some(secrets.clone()),
        model_routes: Some(model_routes.clone()),
        shutdown: Some(shutdown.clone()),
        web_dir: web_dir.clone(),
        // 官方随包 MCP 插件(exe 同级 plugins/,v0.0.4 起随包发布;升级换装
        // 会把包内 plugins/ 合并到这里)——扫描/批准与数据目录 mcp/ 同权,
        // 修复「随包插件对在线升级用户不可见」(2026-09-03 VPS 实测触发)
        bundled_plugins_dir: std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("plugins"))),
        limits: limits_cell.clone(),
        limits_sources: Arc::new(std::sync::Mutex::new(limits_sources)),
        jobs: Some(job_table.clone()),
    };
    bm_surface_http::webadmin::rebuild_routes(&admin);

    // P0(第四轮评审):INV-5 脱敏接线——把模型凭据明文注册进 Execution
    // Log 扫描面,此后任何日志条目命中即整条降格,密钥明文禁止落盘。
    // W2:凭据来源 = 生效配置合并值(model.json 或 env)。
    if let Some(key_value) = &eff.api_key {
        handle.register_redaction_value(key_value);
    }

    // W1(ADR-0014):/v1 插座与会话创建的默认模型 = 生效模型(文件>env)
    let default_model = Arc::new(
        eff.model_id
            .clone()
            .unwrap_or_else(|| bm_core::runtime::DEFAULT_MODEL_ID.to_string()),
    );
    // 绑定面判定:非回环 = 公网面(评审 2026-09-03 #9;反代同机回环 Scenario
    // 需操作者自行配置门户密码,启动告警会持续提示)
    let bind_host = bind
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(bind.as_str());
    let public_bind = !matches!(bind_host, "127.0.0.1" | "localhost" | "::1" | "[::1]" | "");
    let app = bm_surface_http::router(
        handle.clone(),
        Arc::new(token.clone()),
        store,
        shutdown.clone(),
        web_dir.clone(),
        default_model.clone(),
        Some(admin),
        Some(model_routes),
        public_bind,
    );
    if let Some(w) = &web_dir {
        println!("Web Surface 目录 {w:?}(GET / 托管静态界面)");
    }
    // 绑定已在最前完成(先于状态库打开,双开毒化根治):升级子进程的
    // ≤60s 重试同样发生在彼处,此处直接复用已持有的 listener。
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

    // W7:带 ConnectInfo(apply-update 端点据此限制仅回环可升)
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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

/// 脚本装载结果:执行器(None=初始化失败)+ 待注册能力对。
type SkillScriptLoad = (
    Option<Arc<bm_providers::skill_wasm::SkillScriptManager>>,
    Vec<(
        bm_contract::capability::CapabilityManifest,
        Arc<dyn bm_core::registry::CapabilityProvider>,
    )>,
);

/// Skill v0.2(ADR-0016 第二步):扫描 <data>/skills/<skill_id>/ 与
/// config/skills.json——声明 scripts 的技能编译注册(wasm → manifests);
/// 纯知识包跳过。失败仅告警不阻断启动。
fn load_skill_scripts(data_dir: &std::path::Path) -> SkillScriptLoad {
    let manager = match bm_providers::skill_wasm::SkillScriptManager::new() {
        Ok(m) => Arc::new(m),
        Err(e) => {
            eprintln!("[Skill] 执行面初始化失败(已跳过): {e}");
            return (None, Vec::new());
        }
    };
    let cfg = data_dir.join("config").join("skills.json");
    let Ok(text) = std::fs::read_to_string(&cfg) else {
        return (Some(manager), Vec::new());
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        eprintln!("[Skill] skills.json 解析失败(已跳过脚本装载)");
        return (Some(manager), Vec::new());
    };
    let Some(list) = v["skills"].as_array() else {
        return (Some(manager), Vec::new());
    };
    let mut entries: Vec<(
        bm_contract::capability::CapabilityManifest,
        Arc<dyn bm_core::registry::CapabilityProvider>,
    )> = Vec::new();
    for sk in list {
        let Some(id) = sk["skill_id"].as_str() else {
            continue;
        };
        if sk.get("scripts").is_none() {
            continue;
        }
        let Ok(def) = serde_json::from_value::<bm_contract::skill::SkillDefinition>(sk.clone())
        else {
            eprintln!("[Skill] 技能 {id} 的 scripts 载荷非法(已跳过)");
            continue;
        };
        let root = data_dir.join("skills").join(id);
        match manager.register_skill(id, &def, &root) {
            Ok(manifests) => {
                eprintln!("[Skill] 技能 {id} 已装载 {} 个脚本", manifests.len());
                entries.extend(
                    bm_providers::skill_wasm::SkillScriptManager::capability_entries(manifests),
                );
            }
            Err(e) => eprintln!("[Skill] 技能 {id} 装载失败(已跳过): {e}"),
        }
    }
    if !entries.is_empty() {
        eprintln!("[Skill] 共注册 {} 个技能脚本能力", entries.len());
    }
    (Some(manager), entries)
}
