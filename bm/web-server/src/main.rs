//! web-server 二进制：Rust 协议兼容层服务入口。
//!
//! 用法：web-server [--db <path>] [--dist <dist_root>] [--boot-json <file>] [--port <port>]
//!         [--bind <host>]
//!         [--trusted-host <host>] [--config <toml>] [--max-steps <n>] [--plugins-dir <dir>]
//!         [--update-dir <dir>] [--auth] [--compact]
//!
//! `--config` 指向既有 boenmind 形态的 LLM 配置（minimax/deepseek/custom 三通道，
//! 见 provider_config 模块）。不传时服务保持 mock provider（旧行为不变）。
//! `--max-steps` 覆盖单回合最大 step 数（默认 32）。
//! `--plugins-dir` 指向 JS 插件目录（QuickJS 桥 §6）：扫描 plugin.json 逐个按
//! manifest 最小权限授面建引擎，注册进 PluginRuntimePort（探针变 Ready）；
//! 不传 = 探针 Unavailable（fail-loud，旧行为不变）。
//! `--update-dir` 指向热更新托管目录（自建 Update Server）：挂载
//! `GET /update/{*path}` 静态服务 latest.json / 更新包，桌面壳（Tauri updater）
//! 从该端点拉取版本与签名；不传 = 不挂该路由（旧行为不变）。
//! `--compact` 装配上下文压缩插件（功能面）：长会话按水线自动摘要压缩
//! （运行态视图变换，前端无感）；不传 = 未装配（透传，旧行为不变）。
//!
//! 默认 `--dist` 指向自研前端构建产物 `frontend/dist/`（React 19 + dockview 布局 +
//! 应用层登录页，已替代 dsh 官方快照）。

use std::path::PathBuf;
use std::sync::Arc;

use bm_assembly::config::load_llm_config;
use bm_assembly::provider::ProviderRuntime;
use bm_assembly::Runtime;
use web_server::api::AppState;
use web_server::rpc::API_PATH;

/// 解析 home 范围的匿名用户 id（归因头 `x-deepseek-harness-user-id`）。
/// 镜像 DSH `.anonymous-user-id` 语义：文件 `~/.boenmind/.anonymous-user-id`
/// 存一行 UUID v4（`wx` 独占创建防并发双写；读失败/写失败 best-effort 保持
/// 内存 id——归因永不阻塞请求，也永不从主机名/网络/远端派生）。
fn resolve_anonymous_user_id() -> String {
    use std::io::Write;
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = home.join(".boenmind");
    let file = dir.join(".anonymous-user-id");
    // 已存在且合法 → 复用。
    if let Ok(text) = std::fs::read_to_string(&file) {
        let id = text.trim();
        if is_uuid_v4(id) {
            return id.to_string();
        }
    }
    let created = uuid::Uuid::new_v4().to_string();
    let _ = std::fs::create_dir_all(&dir);
    let mut wrote = false;
    if let Ok(mut f) = std::fs::OpenOptions::new().create_new(true).write(true).open(&file) {
        if f.write_all(format!("{created}\n").as_bytes()).is_ok() {
            wrote = true;
        }
    }
    if !wrote {
        // 并发输家/只读 home：best-effort 重读胜者 id，否则保留内存 id。
        if let Ok(text) = std::fs::read_to_string(&file) {
            let id = text.trim();
            if is_uuid_v4(id) {
                return id.to_string();
            }
        }
    }
    created
}

fn is_uuid_v4(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[14] == b'4' // version 4
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes.iter().all(|&b| {
            b.is_ascii_hexdigit() || b == b'-'
        })
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let mut db = PathBuf::from("boenmind.db");
    let mut dist = PathBuf::from("frontend/dist");
    let mut boot_json: Option<String> = None;
    let mut port: u16 = 3080;
    let mut bind_host = "127.0.0.1".to_string();
    // BOENMIND_BIND 环境变量（Linux 服务器版 systemd 注入，历史打包方案先例）
    if let Ok(h) = std::env::var("BOENMIND_BIND") {
        if !h.trim().is_empty() {
            bind_host = h.trim().to_string();
        }
    }
    let mut trusted_hosts: Vec<String> = vec![];
    let mut config: Option<PathBuf> = None;
    let mut max_steps: u64 = kernel_session::DEFAULT_MAX_STEPS;
    let mut plugins_dir: Option<PathBuf> = None;
    let mut update_dir: Option<PathBuf> = None;
    let mut auth_enabled = false;
    let mut compact_enabled = false;
    let mut code_runtime_enabled = false;
    let mut approval_enabled = false;
    let mut web_tools_enabled = false;
    let mut schedule_enabled = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                i += 1;
                db = PathBuf::from(&args[i]);
            }
            "--dist" => {
                i += 1;
                dist = PathBuf::from(&args[i]);
            }
            "--boot-json" => {
                i += 1;
                let path = &args[i];
                boot_json = Some(
                    std::fs::read_to_string(path)
                        .unwrap_or_else(|e| panic!("cannot read boot json {path}: {e}")),
                );
            }
            "--port" => {
                i += 1;
                port = args[i].parse().expect("port must be a number");
            }
            "--bind" => {
                i += 1;
                bind_host = args[i].clone();
            }
            "--trusted-host" => {
                i += 1;
                trusted_hosts.push(args[i].clone());
            }
            "--config" => {
                i += 1;
                config = Some(PathBuf::from(&args[i]));
            }
            "--max-steps" => {
                i += 1;
                max_steps = args[i].parse().expect("max-steps must be a number");
                if max_steps == 0 {
                    eprintln!("max-steps must be at least 1");
                    std::process::exit(2);
                }
            }
            "--plugins-dir" => {
                i += 1;
                plugins_dir = Some(PathBuf::from(&args[i]));
            }
            "--update-dir" => {
                i += 1;
                update_dir = Some(PathBuf::from(&args[i]));
            }
            "--auth" => {
                auth_enabled = true;
            }
            "--compact" => {
                compact_enabled = true;
            }
            "--code-runtime" => {
                code_runtime_enabled = true;
            }
            "--approval" => {
                approval_enabled = true;
            }
            "--web-tools" => {
                web_tools_enabled = true;
            }
            "--schedule" => {
                schedule_enabled = true;
            }
            "--help" | "-h" => {
                println!(
                    "usage: web-server [--db <path>] [--dist <dir>] [--boot-json <file>] [--port <n>] [--bind <host>] [--trusted-host <host>] [--config <toml>] [--max-steps <n>] [--plugins-dir <dir>] [--update-dir <dir>] [--auth] [--compact] [--code-runtime] [--approval] [--web-tools] [--schedule]"
                );
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let mut runtime = match Runtime::headless_with_max_steps(db.clone(), max_steps) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runtime init failed: {e}");
            std::process::exit(1);
        }
    };

    // M3：真 provider 装配（--config），经组合根唯一装配点（bm_assembly::apply_llm）。
    // 无配置 → mock provider（旧行为）。web-server 不再直接依赖 plugin-llm——
    // 装配具体 provider 是组合根职责，main 只做配置读取与结果消费。
    let mut provider_runtimes: Vec<ProviderRuntime> = Vec::new();
    // [compaction] 段（--config 文件内）：--compact 装配时消费；None = 用默认策略参数。
    let mut compaction_cfg: Option<bm_assembly::config::CompactionConfig> = None;
    if let Some(cfg_path) = &config {
        let llm_cfg = match load_llm_config(cfg_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("config error: {e}");
                std::process::exit(1);
            }
        };
        if llm_cfg.providers.is_empty() {
            eprintln!("config has no usable providers (need id + api_key + models)");
            std::process::exit(1);
        }
        let (runtimes, default_provider, default_model) =
            match runtime.apply_llm(&llm_cfg, resolve_anonymous_user_id()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("config error: {e}");
                    std::process::exit(1);
                }
            };
        provider_runtimes = runtimes;
        runtime.provider = default_provider;
        runtime.model = default_model;
        compaction_cfg = llm_cfg.compaction;
        tracing::info!(
            "real providers assembled: {} (default {}/{}), from {}",
            provider_runtimes
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
                .join(","),
            runtime.provider,
            runtime.model,
            cfg_path.display()
        );
    }

    // §6 JS 插件装配（--plugins-dir）：扫描 → 逐插件按 manifest 最小权限授面建引擎
    // → 注册进 PluginRuntimePort（探针变 Ready）。fail-loud：扫描/任一插件装配失败
    // 直接退出（不静默跳过损坏插件）；不传 = 探针 Unavailable（旧行为不变）。
    if let Some(dir) = &plugins_dir {
        match runtime.load_js_plugins_dir(dir) {
            Ok(n) => {
                tracing::info!(
                    "js plugins assembled: {n} plugin(s) from {} (probe={:?})",
                    dir.display(),
                    runtime.plugin_availability()
                );
            }
            Err(e) => {
                eprintln!("plugins-dir error ({}) : {e}", dir.display());
                std::process::exit(1);
            }
        }
    }

    // 认证插件装配（--auth）：启用登录门——敏感方法（credentials.set /
    // settings.update 等）与全部 RPC 需会话 token。密码文件 ~/.boenmind/auth.json
    // （默认密码 adminadmin，首登后设置中心改）+ 会话持久化 sessions.jsonl（重启
    // 不再全员登出）；不传 --auth = 无登录门（旧行为）。
    // L0 不直接依赖 plugin-auth（边界守卫）：经 bm-assembly 组合根入口装配。
    if auth_enabled {
        let auth_dir = std::env::home_dir().map(|h| h.join(".boenmind"));
        runtime.install_default_auth(auth_dir);
        tracing::info!(
            "auth plugin installed (login gate on; default password {})",
            bm_assembly::DEFAULT_PASSWORD
        );
    }

    // 上下文压缩插件装配（--compact）：长会话按水线自动摘要压缩（运行态
    // 视图变换，前端无感——聊天记录照常显示全部历史）。不传 = 未装配
    // （透传，旧行为不变）。功能插件 = 用户可加装/可关闭的可选面。
    // 参数优先取 --config 内 [compaction] 段（缺省回落默认策略 0.5/10%/4000/512）。
    if compact_enabled && !matches!(compaction_cfg.as_ref().and_then(|c| c.enabled), Some(false)) {
        let def = bm_assembly::DefaultCompactor::default();
        let compactor = bm_assembly::DefaultCompactor {
            watermark: compaction_cfg
                .as_ref()
                .and_then(|c| c.watermark)
                .unwrap_or(def.watermark),
            keep_recent_ratio: compaction_cfg
                .as_ref()
                .and_then(|c| c.keep_recent_ratio)
                .unwrap_or(def.keep_recent_ratio),
            keep_recent_floor: compaction_cfg
                .as_ref()
                .and_then(|c| c.keep_recent_floor)
                .unwrap_or(def.keep_recent_floor),
            min_middle_tokens: compaction_cfg
                .as_ref()
                .and_then(|c| c.min_middle_tokens)
                .unwrap_or(def.min_middle_tokens),
        };
        let watermark = compactor.watermark;
        let keep_recent_ratio = compactor.keep_recent_ratio;
        let keep_recent_floor = compactor.keep_recent_floor;
        let min_middle_tokens = compactor.min_middle_tokens;
        runtime.install_compactor(Arc::new(compactor));
        tracing::info!(
            "context compactor plugin installed (watermark {}, tail {:.0}% / floor {}, min-middle {})",
            watermark,
            keep_recent_ratio * 100.0,
            keep_recent_floor,
            min_middle_tokens
        );
    } else if compact_enabled {
        tracing::info!("context compactor plugin skipped: [compaction] enabled=false");
    }

    // settings/credentials 持久化文件（P2-C：重启恢复配置与凭据；
    // 无 home 场景回落内存态）。
    let settings_path = std::env::home_dir()
        .map(|h| h.join(".boenmind").join("settings.json"));
    let state = Arc::new(AppState::assemble_with_settings_path(
        runtime,
        trusted_hosts.clone(),
        provider_runtimes,
        settings_path,
    ));
    // 宿主文件工具插件装配（核心）：注册 host.* 工具 + 注入 workdir 源
    // （从 settings host.workdir 现读——设置页改工作目录后工具即时生效）。
    // 经 bm-assembly 组合根装配（L0 不直接依赖 plugin-host-tools，守卫规则 2）。
    state.install_host_tools();
    // 代码执行沙箱插件装配（--code-runtime）：注册 code.compile/python/shell 工具
    // （workdir 作用域 + 超时 kill + 输出钱包）。不传 = 未装配（旧行为不变）。
    if code_runtime_enabled {
        state.install_code_runtime();
        tracing::info!("code runtime plugin installed (code.compile/code.python/code.shell)");
    }
    // 工具审批端口装配（--approval）：危险工具调用执行前推前端审批弹窗、等用户
    // 裁定（allowed-once 执行 / rejected 记拒绝结果回写日志）。不传 = 审批面禁用
    // （既有自动执行语义，旧行为不变）。
    if approval_enabled {
        let router = web_server::approval::ApprovalRouter::new(Arc::clone(&state));
        state.runtime.install_approval(Arc::new(router));
        tracing::info!("tool approval port installed (dangerous tool calls require approval)");
    }
    // Web 取数工具插件装配（--web-tools）：注册 web.fetch/web.search 工具
    // （SSRF 防线 + 输出钱包，纯自动执行同 host.*）。不传 = 未装配（旧行为不变）。
    if web_tools_enabled {
        state.runtime.install_web_tools();
        tracing::info!("web tools plugin installed (web.fetch/web.search)");
    }
    // 定时任务插件装配（--schedule）：创建 Scheduler（实现 SchedulePort）
    // → 经 bm-assembly install_schedule（注入源 + 注册/启用 schedule.* 工具）
    // → 启动后台驱动循环。不传 = 未装配（旧行为不变）。
    if schedule_enabled {
        state.install_scheduler();
        tracing::info!("schedule plugin installed (schedule.create/list/cancel)");
    }
    // 启动恢复：把持久化会话全部 restore 进 live 表（kill -9 恢复语义，
    // restore_session 内部自动做 interrupted-turn 修复）。blank/running 按日志判定。
    // 必须先于 attach_event_bus：attach 按 live 表历史播种实时 seq 游标。
    {
        let ids = futures::executor::block_on(state.runtime.persist.list_sessions())
            .unwrap_or_default();
        for id in ids {
            match futures::executor::block_on(state.runtime.restore_session(&id)) {
                Ok(agent) => {
                    let events = agent.session().events();
                    let has_turn_start = events
                        .iter()
                        .any(|r| matches!(&r.event, kernel_contracts::session::SessionEvent::Turn(kernel_contracts::session::TurnEvent::Started { .. })));
                    let running = false;
                    state.sessions.lock().unwrap().insert(
                        id.clone(),
                        web_server::api::SessionHandle {
                            agent,
                            running,
                            blank: !has_turn_start,
                            title: None,
                            selected: None,
                        },
                    );
                    tracing::info!("restored session {id} (blank={})", !has_turn_start);
                }
                Err(e) => tracing::warn!("skip restore of session {id}: {e}"),
            }
        }
    }
    // 持有总线监听器句柄到进程结束（drop 即注销，实时事件流依赖它）。
    let _bus_listener = state.attach_event_bus();

    let app = web_server::router(Arc::clone(&state), dist.clone(), boot_json, update_dir);

    let addr = format!("{bind_host}:{port}");
    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap_or_else(|e| {
                eprintln!("bind {addr} failed: {e}");
                std::process::exit(1);
            });
            tracing::info!(
                "web-server listening on http://{addr} (db={}, dist={}, api={API_PATH}, trusted={:?}, providers={})",
                db.display(),
                dist.display(),
                trusted_hosts,
                state.providers.len()
            );
            axum::serve(listener, app).await.unwrap();
        });
}

