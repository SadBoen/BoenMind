//! web-server 二进制：Rust 协议兼容层服务入口。
//!
//! 用法：web-server [--db <path>] [--dist <dist_root>] [--boot-json <file>] [--port <port>]
//!         [--trusted-host <host>] [--config <toml>] [--max-steps <n>]
//!
//! `--config` 指向既有 boenmind 形态的 LLM 配置（minimax/deepseek/custom 三通道，
//! 见 provider_config 模块）。不传时服务保持 mock provider（旧行为不变）。
//! `--max-steps` 覆盖单回合最大 step 数（默认 32）。
//!
//! 默认 `--dist` 指向内置前端快照 `kernel/web-server/frontend/`（dsh rc.6 壳层 +
//! 真实 boot 清单 + 38 插件 client bundle，见同目录 README）。快照 index.html 已含
//! `window.__DSH_BOOT__` 注入，无需再注入；对自备 dist 可用 `--boot-json` 提供清单。

use std::path::PathBuf;
use std::sync::Arc;

use kernel_assembly::Runtime;
use kernel_contracts::llm::{LlmModelInfo, LlmPort};
use kernel_llm::{ModelListEndpoint, MultiProviderLlm, OpenAiProviderConfig, OpenAICompatLlm};
use web_server::api::{AppState, ProviderRuntime};
use web_server::provider_config::load_llm_config;
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
    let mut dist = PathBuf::from("kernel/web-server/frontend");
    let mut boot_json: Option<String> = None;
    let mut port: u16 = 3080;
    let mut trusted_hosts: Vec<String> = vec![];
    let mut config: Option<PathBuf> = None;
    let mut max_steps: u64 = kernel_loop::DEFAULT_MAX_STEPS;

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
            "--help" | "-h" => {
                println!(
                    "usage: web-server [--db <path>] [--dist <dir>] [--boot-json <file>] [--port <n>] [--trusted-host <host>] [--config <toml>] [--max-steps <n>]"
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

    // M3：真 provider 装配（--config）。无配置 → mock provider（旧行为）。
    let mut provider_runtimes: Vec<ProviderRuntime> = Vec::new();
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
        let mut ports: Vec<(String, Arc<dyn LlmPort>)> = Vec::new();
        for p in &llm_cfg.providers {
            if p.models.is_empty() {
                tracing::warn!("provider {}: no models declared, skipped", p.id);
                continue;
            }
            let list_endpoint = ModelListEndpoint::Standard;
            let models: Vec<LlmModelInfo> = p
                .models
                .iter()
                .map(|m| LlmModelInfo {
                    id: m.clone(),
                    label: None,
                    supports_tools: true,
                    context_window: None,
                    max_tokens: None,
                    reasoning: None,
                })
                .collect();
            let adapter = Arc::new(OpenAICompatLlm::new(OpenAiProviderConfig {
                id: p.id.clone(),
                display_name: p.name.clone(),
                settings_ns: format!("llm.{}", p.id),
                base_url: p.base_url.clone(),
                api_key: p.api_key.clone(),
                models,
                list_endpoint,
                user_id: resolve_anonymous_user_id(),
            }));
            provider_runtimes.push(ProviderRuntime {
                id: p.id.clone(),
                display_name: p.name.clone(),
                settings_ns: format!("llm.{}", p.id),
                base_url: p.base_url.clone(),
                models: adapter.models().to_vec(),
                adapter: Some(Arc::clone(&adapter)),
            });
            ports.push((p.id.clone(), adapter as Arc<dyn LlmPort>));
        }
        if ports.is_empty() {
            eprintln!("config has no usable providers (all skipped)");
            std::process::exit(1);
        }
        // 默认 provider/model：config 顶层优先，否则首个 provider 的 default_model，否则其首模型。
        let default_provider = llm_cfg
            .default_provider
            .clone()
            .unwrap_or_else(|| ports[0].0.clone());
        let default_model = llm_cfg
            .default_model
            .clone()
            .or_else(|| {
                llm_cfg
                    .providers
                    .iter()
                    .find(|p| p.id == default_provider)
                    .and_then(|p| p.default_model.clone())
            })
            .or_else(|| {
                llm_cfg
                    .providers
                    .iter()
                    .find(|p| p.id == default_provider)
                    .and_then(|p| p.models.first().cloned())
            })
            .unwrap_or_default();
        runtime.llm = Arc::new(MultiProviderLlm::new(ports));
        runtime.provider = default_provider;
        runtime.model = default_model;
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

    let state = Arc::new(AppState::assemble(runtime, trusted_hosts.clone(), provider_runtimes));
    // 持有总线监听器句柄到进程结束（drop 即注销，实时事件流依赖它）。
    let _bus_listener = state.attach_event_bus();

    // 启动恢复：把持久化会话全部 restore 进 live 表（kill -9 恢复语义，
    // restore_session 内部自动做 interrupted-turn 修复）。blank/running 按日志判定。
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
                            agent: Arc::new(agent),
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

    let app = web_server::router(Arc::clone(&state), dist.clone(), boot_json);

    let addr = format!("127.0.0.1:{port}");
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

