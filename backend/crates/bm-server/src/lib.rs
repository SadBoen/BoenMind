//! BoenMind 后端服务（库入口）。
//!
//! - 独立二进制：`cargo run -p bm-server`
//! - 桌面壳内嵌：Tauri 启动时在独立线程调用 [`serve`]

pub mod bm_engine;
// B6 — 内置工具集（bm 引擎 pi.tool 宿主执行侧：read/write/edit/grep/find/ls/bash）
pub mod builtin_tools;
pub mod chat;
pub mod compat_engine;
pub mod governance;
pub mod pdf_omni;
pub mod permission;
// B6 — 插件权限决策记忆（extension-permissions.json，格式兼容 pi 上游）
pub mod permission_store;
pub mod routes;
pub mod static_files;
pub mod steward;
pub mod subagent_child;
// 专家团队在 bm 引擎的落地：subagent 父侧工具（发现角色 → spawn 子进程 → 摄取
// stdout JSON 事件流；子进程协议 = subagent_child）
pub mod subagent_tool;

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

/// bm 引擎取消条目：(prompt_id, watch::Sender<bool>)——身份匹配纪律：
/// 先结束的只删自己的条目（见 bm_engine.rs）。
pub type BmAbortEntry = (u64, tokio::sync::watch::Sender<bool>);

/// bm 引擎 agent 空闲淘汰阈值（loop_agents 表；agent 状态全在事件日志，
/// 弃置重建零损失）。
pub const AGENT_IDLE_TTL: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);
/// 空闲扫描周期
const AGENT_SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub db: Arc<Db>,
    /// 活跃 prompt 的 SSE 事件通道（key = session_id）。权限询问桥据此把
    /// 询问事件推给前端；prompt 结束时移除。
    /// unbounded：bm 路径的 hooks 是同步回调（try_send 只关心通道是否关闭）。
    pub session_streams: Arc<Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<bm_core::agent::AgentStreamEvent>>>>,
    /// 挂起的权限询问（key = 上游询问请求 id）：等待前端决策（允许/拒绝/总是允许）。
    pub permission_pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<PermissionDecision>>>>,
    /// 阶段 0 事件日志双写器（None = 事件日志不可用，双写静默跳过，
    /// 主链路不受影响——事件日志是渐进式吸收的新家，不是闸门）。
    pub dual_writer: Option<Arc<bm_storage_turso::dual_write::DualWriter>>,
    /// bm 引擎进行中 prompt 的取消通道
    /// （key = session_id，value = (prompt_id, watch::Sender<bool>)）。
    pub bm_aborts: Arc<Mutex<HashMap<String, BmAbortEntry>>>,
    /// bm 引擎会话级 agent（key = session_id）。agent 只是「日志 + 配置 +
    /// 客户端」的壳——状态全在事件日志，换 provider/model 或空闲淘汰时
    /// 弃置重建零损失（见 bm_engine.rs）。
    pub loop_agents: Arc<Mutex<HashMap<String, bm_engine::LoopSessionEntry>>>,
    /// B4 工具方向：QuickJS 插件引擎宿主（None = 启动失败，bm 引擎退化为
    /// 无工具模式）。工具快照在启动加载后固化（compat_engine.rs）。
    pub compat: Option<Arc<compat_engine::CompatEngine>>,
    /// 管家（Steward 轮）：next_wake_at 状态 + 调度器共享句柄
    /// （None = 未启用管家，BM_STEWARD_SESSION 未设置）。
    pub steward: Option<Arc<steward::StewardStore>>,
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
    pub fn new(
        config: AppConfig,
        db: Arc<Db>,
        dual_writer: Option<Arc<bm_storage_turso::dual_write::DualWriter>>,
        compat: Option<Arc<compat_engine::CompatEngine>>,
        session_streams: Arc<
            Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<bm_core::agent::AgentStreamEvent>>>,
        >,
        permission_pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<PermissionDecision>>>>,
        steward: Option<Arc<steward::StewardStore>>,
    ) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            db,
            session_streams,
            permission_pending,
            dual_writer,
            bm_aborts: Arc::new(Mutex::new(HashMap::new())),
            loop_agents: Arc::new(Mutex::new(HashMap::new())),
            compat,
            steward,
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
        .route(
            "/api/sessions/{id}/events",
            get(routes::sessions::events_stream).delete(routes::sessions::clear_session_events),
        )
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
        // Steward 轮（v0.19）：OS 层主动汇报通道 + 管家状态查询
        .route("/api/steward/inject", post(routes::steward::inject))
        .route("/api/steward/status", get(routes::steward::status))
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

/// 阶段 0 双写初始化：打开事件日志存储（与现有 boenmind.db 同文件，
/// WAL 模式多连接），组装 DualWriter。失败返回错误（调用方决定跳过）。
/// A4：启动时补写崩溃遗留的未闭合回合（TurnEnd{reason: Interrupted}）。
async fn init_dual_writer() -> Result<Arc<bm_storage_turso::dual_write::DualWriter>, bm_protocol::ProtocolError> {
    let path = bm_core::config::app_dir()
        .join("boenmind.db")
        .to_str()
        .unwrap_or("boenmind.db")
        .to_string();
    let store = std::sync::Arc::new(bm_storage_turso::TursoEventStore::open(&path).await?);
    let log = bm_kernel::EventLog::new(store.clone());
    let w = std::sync::Arc::new(bm_storage_turso::dual_write::DualWriter::with_turso(
        log,
        store.clone(),
    ));
    // A4 启动恢复：有 TurnStart 无 TurnEnd 的回合显式闭合（dsh 语义）
    match bm_storage_turso::recover_interrupted_turns(store.as_ref(), w.event_log()).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(event = "bm.interrupted_turns_recovered", count = n),
        Err(err) => tracing::warn!(event = "bm.interrupted_turn_recover_failed", error = %err),
    }
    Ok(w)
}

async fn serve_inner(
    port: u16,
    shutdown: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (config, db) = init().await?;
    let db = Arc::new(db);
    tracing::info!("工作文件夹: {}", config.working_dir.display());
    tracing::info!("pi agent 目录: {}", bm_core::config::pi_agent_dir().display());

    // 阶段 0 双写：事件日志与现有表同库（WAL 多连接），打开失败仅告警不阻断
    let dual_writer = match init_dual_writer().await {
        Ok(w) => {
            tracing::info!("事件日志双写已启用（万物皆插件阶段 0）");
            Some(w)
        }
        Err(err) => {
            tracing::warn!(event = "bm.dual_write_disabled", error = %err, "事件日志不可用，双写跳过");
            None
        }
    };

    // B4/B5 工具方向：QuickJS 插件引擎（bm 引擎的工具执行侧）。启动失败不阻断——
    // bm 引擎退化为无工具模式（pi 路径不受影响，其引擎在 legacy 内）。
    // session_streams/permission_pending 先建（CompatEngine 建于 AppState 之前，
    // 只拿这两个组件做权限询问路由），再共享给 AppState。
    let session_streams: Arc<
        Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<bm_core::agent::AgentStreamEvent>>>,
    > = Arc::new(Mutex::new(HashMap::new()));
    let permission_pending: Arc<
        Mutex<HashMap<String, tokio::sync::oneshot::Sender<PermissionDecision>>>,
    > = Arc::new(Mutex::new(HashMap::new()));
    let compat = compat_engine::init_compat(
        &config,
        session_streams.clone(),
        permission_pending.clone(),
        db.clone(),
        // 投影面数据源（getmessagesurface）：与 chat 路径共用同一事件日志
        dual_writer
            .as_ref()
            .map(|d| bm_kernel::EventLog::new(d.event_log().store())),
    )
    .await;

    // Steward 轮（v0.19）：管家状态（next_wake_at 落点 = steward.json）。
    // BM_STEWARD_SESSION env 指定管家会话才启用；未启用 = None（调度器
    // 不启动、set_wake 不进任何工具面——可选项零开销）
    let steward = {
        let store = steward::StewardStore::load(bm_core::config::app_dir());
        match store.session_id().await {
            Some(sid) => {
                tracing::info!(event = "bm.steward_configured", session = %sid);
                Some(Arc::new(store))
            }
            None => None,
        }
    };

    let state = AppState::new(
        config,
        db,
        dual_writer,
        compat,
        session_streams,
        permission_pending,
        steward,
    );
    spawn_agent_sweeper(state.loop_agents.clone());
    // C1 回收站超期清除：孤儿会话（sessions 表已删）事件保留 N 天后物理删除
    spawn_orphan_purger(state.dual_writer.clone());
    // Steward 轮（v0.19）：管家定时唤醒调度器（到点投喂 Goal 回合；
    // store 已创建说明启用了管家；None 时调度器无事可做）
    if let Some(store) = &state.steward {
        let session_id = store.session_id().await;
        tracing::info!(
            event = "bm.steward_enabled",
            session = ?session_id,
            "管家已启用（BM_STEWARD_SESSION）"
        );
        bm_engine::spawn_steward_scheduler(state.clone(), store.clone());
        // v0.20：系统启动汇报（BM_STEWARD_BOOT_REPORT=1 开启）——宿主重启后
        // 内存态丢失，管家需要知道（fire-and-forget：失败仅日志，不阻断启动；
        // 默认关：每次重启烧一次 token 需显式开关）
        if std::env::var("BM_STEWARD_BOOT_REPORT").is_ok_and(|v| v.trim() == "1") {
            tracing::info!(event = "bm.steward_boot_report", "系统启动汇报已投喂");
            let state_boot = state.clone();
            let store_boot = store.clone();
            tokio::spawn(async move {
                bm_engine::dispatch_steward_round(
                    &state_boot,
                    &store_boot,
                    "系统启动汇报：BoenMind 宿主服务已启动，内存态已重置。\
                     请确认当前状态，如有需要处理的事项请执行，回合结束时调用 \
                     set_wake 登记下次唤醒时间。"
                        .to_string(),
                    bm_protocol::UserMsgSource::Inject,
                )
                .await;
            });
        }
    }
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

/// C1 回收站超期清除：孤儿会话（sessions 表已删）的事件保留 N 天后物理删除。
/// N 默认 90 天（实现期调优），可用环境变量 BM_ORPHAN_PURGE_DAYS 覆盖。
/// 每天跑一次；删除会话即入"回收站"（事件仍留 event_log），超期才物理删除。
const ORPHAN_PURGE_DAYS: i64 = 90;
const ORPHAN_PURGE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

fn spawn_orphan_purger(dual: Option<Arc<bm_storage_turso::dual_write::DualWriter>>) {
    let Some(dual) = dual else {
        return;
    };
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(ORPHAN_PURGE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let days = std::env::var("BM_ORPHAN_PURGE_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(ORPHAN_PURGE_DAYS);
            let before_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64 - days * 86_400_000)
                .unwrap_or(0);
            match dual.purge_orphaned_events(before_ms).await {
                Ok(0) => {}
                Ok(n) => tracing::info!(
                    event = "bm.orphan_events_purged",
                    count = n,
                    older_than_days = days
                ),
                Err(err) => tracing::warn!(event = "bm.orphan_purge_failed", error = %err),
            }
        }
    });
}

/// 周期性扫描并释放空闲 bm 引擎 agent 会话句柄（防止长跑服务内存无界增长）。
/// agent 状态全在事件日志，弃置重建零损失。
fn spawn_agent_sweeper(loop_agents: Arc<Mutex<HashMap<String, bm_engine::LoopSessionEntry>>>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(AGENT_SWEEP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            // 第一遍收集候选（不持锁太久）；二次确认时避免与并发 chat 竞争误删
            // agent 锁 = 会话 prompt 串行锁，锁不住才可淘汰
            let bm_candidates: Vec<String> = {
                let map = loop_agents.lock().await;
                map.iter()
                    .filter(|(_, e)| e.last_used.elapsed() > AGENT_IDLE_TTL)
                    .filter(|(_, e)| e.agent.try_lock().is_ok())
                    .map(|(id, _)| id.clone())
                    .collect()
            };
            for id in bm_candidates {
                let mut map = loop_agents.lock().await;
                if let Some(e) = map.get(&id)
                    && e.last_used.elapsed() > AGENT_IDLE_TTL
                    && e.agent.try_lock().is_ok()
                {
                    map.remove(&id);
                    tracing::info!(event = "bm.loop_agent_evicted", session = %id);
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
