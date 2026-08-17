//! web-server 二进制：Rust 协议兼容层服务入口。
//!
//! 用法：web-server [--db <path>] [--dist <dist_root>] [--boot-json <file>] [--port <port>]
//!         [--trusted-host <host>] [--config <toml>]
//!
//! `--config` 指向既有 boenmind 形态的 LLM 配置（minimax/deepseek/custom 三通道，
//! 见 provider_config 模块）。不传时服务保持 mock provider（旧行为不变）。
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
            "--help" | "-h" => {
                println!(
                    "usage: web-server [--db <path>] [--dist <dir>] [--boot-json <file>] [--port <n>] [--trusted-host <host>] [--config <toml>]"
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

    let mut runtime = match Runtime::headless(db.clone()) {
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

