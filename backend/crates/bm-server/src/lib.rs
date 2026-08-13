//! BoenMind 后端服务（库入口）。
//!
//! - 独立二进制：`cargo run -p bm-server`
//! - 桌面壳内嵌：Tauri 启动时在独立线程调用 [`serve`]

pub mod chat;
pub mod pdf_omni;
pub mod permission;
pub mod routes;
pub mod static_files;
pub mod subagent_child;

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use bm_core::{AppConfig, Db};
use tokio::sync::{Mutex, RwLock};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

pub const DEFAULT_PORT: u16 = 17321;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 单个聊天会话对应的 pi agent 会话句柄。
/// `Arc<Mutex<..>>`：同一会话的 prompt 串行执行，map 锁不长期占用。
pub struct AgentSessionEntry {
    pub handle: Arc<tokio::sync::Mutex<pi::sdk::AgentSessionHandle>>,
    /// 最近一次使用时间（chat 请求时刷新），空闲淘汰用
    pub last_used: std::time::Instant,
}

/// agent 会话句柄空闲淘汰阈值：超过该时长无对话且无进行中 prompt 即释放
/// （句柄持有完整会话上下文，长期运行的服务不释放会无界增长内存）。
pub const AGENT_IDLE_TTL: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);
/// 空闲扫描周期
const AGENT_SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub db: Arc<Db>,
    pub agents: Arc<Mutex<HashMap<String, AgentSessionEntry>>>,
    /// 进行中 prompt 的取消句柄（key = session_id，value = (prompt_id, AbortHandle)）。
    /// prompt_id 用于清理时身份匹配：同会话连续两个 prompt 时，先结束的
    /// 只能删除自己的条目，不能把后一个的取消句柄误删（见 chat.rs）。
    pub aborts: Arc<Mutex<HashMap<String, (u64, pi::sdk::AbortHandle)>>>,
    /// 活跃 prompt 的 SSE 事件通道（key = session_id）。权限询问桥据此把
    /// 询问事件推给前端；prompt 结束时移除。
    pub session_streams: Arc<Mutex<HashMap<String, tokio::sync::mpsc::Sender<bm_core::agent::AgentStreamEvent>>>>,
    /// 挂起的权限询问（key = 上游询问请求 id）：等待前端决策（允许/拒绝/总是允许）。
    pub permission_pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<PermissionDecision>>>>,
}

/// 前端对一次权限询问的决策。
#[derive(Debug, Clone)]
pub struct PermissionDecision {
    pub allow: bool,
    /// 总是允许/总是拒绝。上游会把任何决策持久化到
    /// extension-permissions.json（跨会话生效），"总是"只是用户的显式表达，
    /// 后端不再自建白名单存储——上游缓存即权威。
    pub always: bool,
}

impl AppState {
    pub fn new(config: AppConfig, db: Db) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            db: Arc::new(db),
            agents: Arc::new(Mutex::new(HashMap::new())),
            aborts: Arc::new(Mutex::new(HashMap::new())),
            session_streams: Arc::new(Mutex::new(HashMap::new())),
            permission_pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn router(state: AppState) -> Router {
    // CORS：仅放行本机/桌面壳来源，防止任意网页跨源读取本地 API（明文密钥、
    // 聊天记录）。无 Origin 头的请求（同源、curl 等非浏览器客户端）不经此判断。
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| cors_origin_allowed(origin)))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers(Any);

    let router = Router::new()
        .route("/api/health", get(routes::health))
        .route("/api/config", get(routes::config::get_config).put(routes::config::put_config))
        .route("/api/sessions", get(routes::sessions::list_sessions).post(routes::sessions::create_session))
        .route("/api/sessions/{id}", get(routes::sessions::get_session).patch(routes::sessions::rename_session).delete(routes::sessions::delete_session))
        .route("/api/sessions/{id}/tasks", get(routes::sessions::list_session_tasks))
        .route("/api/plugins", get(routes::plugins::list_plugins))
        .route("/api/plugins/install", post(routes::plugins::install_plugin))
        .route("/api/plugins/install-source", post(routes::plugins::install_plugin_from_source))
        .route("/api/plugins/{id}", post(routes::plugins::set_plugin).delete(routes::plugins::uninstall_plugin))
        .route("/api/plugins/{id}/settings", get(routes::plugins::get_plugin_settings).put(routes::plugins::put_plugin_settings))
        .route("/api/plugins/{id}/test-source", post(routes::plugins::test_plugin_source))
        .route("/api/skills", get(routes::skills::list_skills))
        .route("/api/skills/install", post(routes::skills::install_skill))
        .route("/api/skills/registry/random", get(routes::skills::random_skills))
        .route("/api/skills/{id}", post(routes::skills::set_skill).delete(routes::skills::uninstall_skill))
        .route("/api/chat", post(chat::chat))
        .route("/api/chat/stop", post(chat::stop_chat))
        .route("/api/chat/permission-response", post(chat::respond_permission))
        .route("/api/refinement-suggestions", get(routes::refine::list_refinement_suggestions))
        .route("/api/refinement-suggestions/{id}/approve", post(routes::refine::approve_suggestion))
        .route("/api/refinement-suggestions/{id}/reject", post(routes::refine::reject_suggestion))
        .route("/api/refinement-suggestions/{id}/rollback", post(routes::refine::rollback_suggestion))
        .route("/api/providers/presets", get(routes::providers::presets))
        .route("/api/providers/list-models", post(routes::providers::list_provider_models))
        .route("/api/thinking-levels", get(routes::providers::thinking_levels))
        .route("/api/providers/test", post(routes::providers::test_provider))
        .route("/api/plugins/pdf-omni/parse", post(routes::pdf_omni::parse_pdf))
        .route("/api/plugins/pdf-omni/probe", post(routes::pdf_omni::probe))
        .route("/api/updates/check", get(routes::updates::check_update))
        .route("/api/updates/apply", post(routes::updates::apply_update))
        .route("/api/updates/restart", post(routes::updates::restart_update))
        .route("/api/workspace/list", get(routes::workspace::list_workspace))
        .route("/api/workspace/file", get(routes::workspace::read_workspace_file))
        .with_state(state)
        // 注意层顺序：CORS 最外层（跨域预检 OPTIONS 不能被鉴权挡住），
        // 鉴权在 CORS 之内、路由之外
        .layer(axum::middleware::from_fn(auth_middleware))
        .layer(cors);

    // 服务器版（--features embed）：未命中的 GET 交给内嵌前端（SPA fallback）
    #[cfg(feature = "embed")]
    let router = router.fallback(static_files::handle_static);

    router
}

/// 跨源 Origin 白名单：仅本机来源（浏览器本机页面 / Tauri 桌面壳 webview）。
///
/// - `http(s)://localhost:*`、`http(s)://127.0.0.1:*`、`http(s)://[::1]:*`：本地开发与桌面内嵌
/// - `tauri://localhost`、`http(s)://tauri.localhost:*`：Tauri 2 webview（macOS/Linux 与 Windows）
/// - 其余 Origin 一律拒绝：浏览器跨源响应不带 `Access-Control-Allow-Origin` 头，
///   恶意网页读不到本地 API 响应（DNS rebinding / 同源场景由 BOENMIND_TOKEN 兜底）
fn cors_origin_allowed(origin: &axum::http::HeaderValue) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    if origin == "tauri://localhost" {
        return true;
    }
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    // host 提取：IPv6 字面量形如 `[::1]:5173`，需取到 `]` 为止
    let host = if let Some(end) = rest.find(']') {
        &rest[..=end]
    } else {
        rest.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "tauri.localhost")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn allows_local_sources() {
        for ok in [
            "http://localhost:5173",
            "http://127.0.0.1:5173",
            "http://[::1]:17321",
            "http://localhost:17321",
            "tauri://localhost",
            "http://tauri.localhost",
        ] {
            assert!(
                cors_origin_allowed(&HeaderValue::from_str(ok).unwrap()),
                "应放行: {ok}"
            );
        }
    }

    #[test]
    fn rejects_remote_origins() {
        for bad in [
            "http://evil.example.com",
            "https://evil.example.com:17321",
            "http://192.168.1.10:17321",
            "file:///tmp/x.html",
            "null",
        ] {
            assert!(
                !cors_origin_allowed(&HeaderValue::from_str(bad).unwrap()),
                "应拒绝: {bad}"
            );
        }
    }

    #[test]
    fn app_error_maps_to_http_status() {
        use bm_core::AppError;
        // 分类 → 状态码集中映射
        assert_eq!(
            api_error_from(AppError::Invalid("参数错误".into())).0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            api_error_from(AppError::Upstream("网络错误".into())).0,
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            api_error_from(AppError::Internal("IO 错误".into())).0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        // 错误消息透传（前端 toast 直接展示）
        let (_, body) = api_error_from(AppError::Invalid("参数错误".into()));
        assert_eq!(body.0, serde_json::json!({ "error": "参数错误" }));
    }
}

/// 可选的访问令牌守卫：`BOENMIND_TOKEN` 环境变量设置后，所有 /api 请求必须带
/// `Authorization: Bearer <token>`，否则 401（body 为 `{"error":"unauthorized"}`）。
/// 桌面壳（Tauri 内嵌）不设置该变量，行为与之前完全一致。
async fn auth_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(expected) = std::env::var("BOENMIND_TOKEN").ok().filter(|t| !t.is_empty()) else {
        return next.run(request).await;
    };
    let authorized = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected);
    if !authorized {
        return api_error(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    next.run(request).await
}

/// 初始化 BoenMind 环境（配置、工作文件夹、pi agent 目录、数据库）并返回服务状态。
///
/// 供独立二进制与 Tauri 壳共用，保证两端行为一致。
pub async fn init() -> Result<(AppConfig, Db), Box<dyn std::error::Error>> {
    // 1. 配置与工作文件夹
    let config = bm_core::config::load();
    if let Err(err) = bm_core::config::ensure_working_dir(&config) {
        eprintln!("[bm-server] 工作文件夹创建失败: {err}");
    }

    // 2. pi agent 全局目录指向我们自己的目录，与用户 ~/.pi 互不干扰
    // 注意：edition 2024 中 set_var 为 unsafe
    let pi_dir = bm_core::config::pi_agent_dir();
    unsafe { std::env::set_var("PI_CODING_AGENT_DIR", &pi_dir) };

    // 2.25 开启 pi 会话热路径性能计数器（原子计数，零 UI 暴露；排障时读内部快照）
    unsafe { std::env::set_var("PI_PERF_TELEMETRY", "1") };

    // 2.5 pdf-omni 插件经 loopback 调宿主解析端点：放行纯 http 的 127.0.0.1
    // （上游 http connector 对 loopback 的明文 http 需显式 opt-in；只影响本机端点）
    unsafe { std::env::set_var("PI_HTTP_ALLOW_LOOPBACK", "1") };

    // 3. 同步 models.json（provider baseUrl 覆盖 + 自定义模型注册）
    bm_core::config::sync_pi_models_json(&config)?;

    // 3.25 预置子代理角色定义（agents/*.md），让 subagent 工具开箱可用
    if let Err(err) = bm_core::config::ensure_builtin_agents() {
        eprintln!("[bm-server] 预置子代理角色定义失败: {err}");
    }

    // 3.5 预装内置插件（hello / bookmark / ctx-compactor；用户已卸载的不再恢复）
    if let Err(err) = bm_core::plugins::ensure_builtin_plugins(&config) {
        eprintln!("[bm-server] 预装示例插件失败: {err}");
    }

    // 3.75 默认提供商注入环境变量：subagent 子进程据此解析 provider
    //（多会话各自选择 provider 时子代理仍用全局默认——见 docs/expert-team.md 阶段 1）
    if let Some(default_id) = bm_core::config::resolve_provider(&config, None) {
        unsafe { std::env::set_var("PI_SUBAGENT_PROVIDER_ID", default_id.id.clone()) };
    }

    // 4. 数据库
    let db = Db::open().await?;
    Ok((config, db))
}

/// 启动 HTTP 服务（阻塞直至退出）。
///
/// 注意：本函数不初始化全局日志（避免与宿主进程的日志系统冲突，
/// 例如 Tauri 的 log 插件）。调用方自行初始化 tracing_subscriber。
pub async fn serve(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    serve_inner(port, None).await
}

/// 桌面壳托管版：`serve` + 可优雅关闭（壳在热更新后向 shutdown 发送信号，
/// axum graceful shutdown 结束后本函数返回，壳随即拉起新版本子进程）。
/// 仅桌面壳（managed 模式）使用；standalone 的升级走 exec，不需要优雅关闭。
pub async fn serve_managed(
    port: u16,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    serve_inner(port, Some(shutdown)).await
}

async fn serve_inner(
    port: u16,
    shutdown: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (config, db) = init().await?;
    tracing::info!("工作文件夹: {}", config.working_dir.display());
    tracing::info!("pi agent 目录: {}", bm_core::config::pi_agent_dir().display());

    let state = AppState::new(config, db);
    spawn_agent_sweeper(state.agents.clone());
    let listener = tokio::net::TcpListener::bind(bind_addr(port)).await?;
    let local = listener.local_addr()?;
    let has_token = std::env::var("BOENMIND_TOKEN").is_ok_and(|t| !t.trim().is_empty());
    if !local.ip().is_loopback() && !has_token {
        tracing::warn!(
            "监听 {local} 且未设置 BOENMIND_TOKEN：API 密钥与聊天记录对网络内任何人可见，请设置 BOENMIND_TOKEN 或经反向代理加访问控制"
        );
    }
    tracing::info!("BoenMind 后端已启动: http://{local} (v{VERSION})");

    let server = axum::serve(listener, router(state));
    match shutdown {
        Some(mut rx) => {
            server
                .with_graceful_shutdown(async move {
                    // 壳发来关闭信号（热更新换新版）：等一等正在进行的请求收尾
                    let _ = rx.changed().await;
                })
                .await?;
            tracing::info!("BoenMind 后端已优雅关闭");
        }
        None => {
            server.await?;
        }
    }
    Ok(())
}

/// 周期性扫描并释放空闲 agent 会话句柄（防止长跑服务内存无界增长）。
fn spawn_agent_sweeper(agents: Arc<Mutex<HashMap<String, AgentSessionEntry>>>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(AGENT_SWEEP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            // 第一遍收集候选（不持锁太久）；二次确认时避免与并发 chat 竞争误删
            let candidates: Vec<String> = {
                let map = agents.lock().await;
                map.iter()
                    .filter(|(_, e)| e.last_used.elapsed() > AGENT_IDLE_TTL)
                    .filter(|(_, e)| e.handle.try_lock().is_ok()) // 有进行中 prompt 的跳过
                    .map(|(id, _)| id.clone())
                    .collect()
            };
            for id in candidates {
                let mut map = agents.lock().await;
                if let Some(e) = map.get(&id)
                    && e.last_used.elapsed() > AGENT_IDLE_TTL
                    && e.handle.try_lock().is_ok()
                {
                    map.remove(&id);
                    tracing::info!(event = "bm.agent_evicted", session = %id);
                }
            }
        }
    });
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

/// 以当前二进制替换自身进程（PID 不变，systemd `Restart=always` 无感知）。
/// exec 成功即不返回；仅 Unix（standalone 部署）可用。
#[cfg(unix)]
pub fn exec_self() -> Result<(), std::io::Error> {
    use std::os::unix::process::CommandExt as _;
    let exe = std::env::current_exe()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let err = std::process::Command::new(&exe).args(&args).exec();
    Err(err)
}
#[cfg(not(unix))]
pub fn exec_self() -> Result<(), std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "非 Unix 平台不支持 exec 自重启",
    ))
}

/// 启动时检测自更新残留：`.update-pending` 标记存在（apply 已替换自身但
/// 进程未及重启，如崩溃/断电）→ 删除标记并 exec 自身完成升级。
/// 仅独立二进制入口（main.rs）调用；桌面壳（managed）由壳管理，不调用。
pub fn consume_pending_update() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        let marker = bm_core::updates::runtime_dir().join(bm_core::updates::UPDATE_PENDING_FILE);
        if marker.exists() {
            // 先删标记：新进程启动后不再重复 exec
            let _ = std::fs::remove_file(&marker);
            eprintln!("[bm-server] 检测到待完成的自更新，正在重启为新版本…");
            exec_self().map_err(|e| format!("自更新 exec 失败: {e}"))?;
        }
    }
    Ok(())
}

/// 统一的 API 错误响应。
pub fn api_error(status: StatusCode, message: impl Into<String>) -> (StatusCode, axum::Json<serde_json::Value>) {
    (
        status,
        axum::Json(serde_json::json!({ "error": message.into() })),
    )
}

/// 领域层错误 → HTTP 响应：按分类集中映射状态码（不再在路由层手工选）。
/// Invalid → 400、Upstream → 502、Internal → 500。
pub fn api_error_from(err: bm_core::AppError) -> (StatusCode, axum::Json<serde_json::Value>) {
    match err {
        bm_core::AppError::Invalid(msg) => api_error(StatusCode::BAD_REQUEST, msg),
        bm_core::AppError::Upstream(msg) => api_error(StatusCode::BAD_GATEWAY, msg),
        bm_core::AppError::Internal(msg) => api_error(StatusCode::INTERNAL_SERVER_ERROR, msg),
    }
}

/// 提取 state 的便捷写法（供 handlers 使用）。
pub type ApiResult<T> = Result<T, (StatusCode, axum::Json<serde_json::Value>)>;
pub type SharedState = State<AppState>;
