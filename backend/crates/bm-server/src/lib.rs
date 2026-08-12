//! BoenMind 后端服务（库入口）。
//!
//! - 独立二进制：`cargo run -p bm-server`
//! - 桌面壳内嵌：Tauri 启动时在独立线程调用 [`serve`]

pub mod chat;
pub mod routes;
pub mod static_files;

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{Method, StatusCode},
    routing::{get, post},
};
use bm_core::{AppConfig, Db};
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::{Any, CorsLayer};

pub const DEFAULT_PORT: u16 = 17321;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 单个聊天会话对应的 pi agent 会话句柄。
/// `Arc<Mutex<..>>`：同一会话的 prompt 串行执行，map 锁不长期占用。
pub struct AgentSessionEntry {
    pub handle: Arc<tokio::sync::Mutex<pi::sdk::AgentSessionHandle>>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub db: Arc<Db>,
    pub agents: Arc<Mutex<HashMap<String, AgentSessionEntry>>>,
}

impl AppState {
    pub fn new(config: AppConfig, db: Db) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            db: Arc::new(db),
            agents: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers(Any);

    let router = Router::new()
        .route("/api/health", get(routes::health))
        .route("/api/config", get(routes::get_config).put(routes::put_config))
        .route("/api/sessions", get(routes::list_sessions).post(routes::create_session))
        .route("/api/sessions/{id}", get(routes::get_session).patch(routes::rename_session).delete(routes::delete_session))
        .route("/api/plugins", get(routes::list_plugins))
        .route("/api/plugins/install", post(routes::install_plugin))
        .route("/api/plugins/{id}", post(routes::set_plugin).delete(routes::uninstall_plugin))
        .route("/api/skills", get(routes::list_skills))
        .route("/api/skills/install", post(routes::install_skill))
        .route("/api/skills/registry/random", get(routes::random_skills))
        .route("/api/skills/{id}", post(routes::set_skill).delete(routes::uninstall_skill))
        .route("/api/chat", post(chat::chat))
        .route("/api/providers/list-models", post(routes::list_provider_models))
        .route("/api/providers/test", post(routes::test_provider))
        .route("/api/workspace/list", get(routes::list_workspace))
        .route("/api/workspace/file", get(routes::read_workspace_file))
        .with_state(state)
        .layer(cors);

    // 服务器版（--features embed）：未命中的 GET 交给内嵌前端（SPA fallback）
    #[cfg(feature = "embed")]
    let router = router.fallback(static_files::handle_static);

    router
}

/// 初始化 BoenMind 环境（配置、工作文件夹、pi agent 目录、数据库）并返回服务状态。
///
/// 供独立二进制与 Tauri 壳共用，保证两端行为一致。
pub fn init() -> Result<(AppConfig, Db), Box<dyn std::error::Error>> {
    // 1. 配置与工作文件夹
    let config = bm_core::config::load();
    if let Err(err) = bm_core::config::ensure_working_dir(&config) {
        eprintln!("[bm-server] 工作文件夹创建失败: {err}");
    }

    // 2. pi agent 全局目录指向我们自己的目录，与用户 ~/.pi 互不干扰
    // 注意：edition 2024 中 set_var 为 unsafe
    let pi_dir = bm_core::config::pi_agent_dir();
    unsafe { std::env::set_var("PI_CODING_AGENT_DIR", &pi_dir) };

    // 3. 同步 models.json（provider baseUrl 覆盖 + 自定义模型注册）
    bm_core::config::sync_pi_models_json(&config)?;

    // 3.5 预装内置示例插件（hello / bookmark）
    if let Err(err) = bm_core::plugins::ensure_builtin_plugins() {
        eprintln!("[bm-server] 预装示例插件失败: {err}");
    }

    // 4. 数据库
    let db = Db::open()?;
    Ok((config, db))
}

/// 启动 HTTP 服务（阻塞直至退出）。
///
/// 注意：本函数不初始化全局日志（避免与宿主进程的日志系统冲突，
/// 例如 Tauri 的 log 插件）。调用方自行初始化 tracing_subscriber。
pub async fn serve(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let (config, db) = init()?;
    tracing::info!("工作文件夹: {}", config.working_dir.display());
    tracing::info!("pi agent 目录: {}", bm_core::config::pi_agent_dir().display());

    let state = AppState::new(config, db);
    let listener = tokio::net::TcpListener::bind(bind_addr(port)).await?;
    let local = listener.local_addr()?;
    tracing::info!("BoenMind 后端已启动: http://{local} (v{VERSION})");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

/// 监听地址：默认 `127.0.0.1`（桌面壳内嵌时只本机访问）；
/// 服务器部署通过 `BOENMIND_BIND` 覆盖，例如 `0.0.0.0:17321`。
pub fn bind_addr(port: u16) -> std::net::SocketAddr {
    let host = std::env::var("BOENMIND_BIND")
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    let ip: std::net::IpAddr = host
        .parse()
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    std::net::SocketAddr::new(ip, port)
}

/// 统一的 API 错误响应。
pub fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, axum::Json<serde_json::Value>) {
    (
        status,
        axum::Json(serde_json::json!({ "error": message.into() })),
    )
}

/// 提取 state 的便捷写法（供 handlers 使用）。
pub type ApiResult<T> = Result<T, (StatusCode, axum::Json<serde_json::Value>)>;
pub type SharedState = State<AppState>;
