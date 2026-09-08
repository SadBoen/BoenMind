//! W2 管理面(ADR-0014 W 序列):provider 库增删改查 + 连通探针/模型清单、
//! MCP 接入配置管理、插件(能力)清单、工作区文件浏览(只读)。
//!
//! 形态裁决(2026-09-01,登记于 W2 规格 §5):
//! - 本面是 webapp 壳子私用的 REST 端点,不走 Wire 信封协议(dsh 协议
//!   已随 ADR-0013 归档);**整批暂不入 boenmind-contracts 冻结库**,
//!   以本模块 + W2 实现规格约束,W 序列稳定后一次性评估入册
//!   (合同只增不破,晚入不亏);
//! - 公开挂载 = W1 同款**已登记欠账**(单机 localhost 口径;公网部署前
//!   补 Bearer,沿 ADR-0009 T-13/T-14);
//! - 「插件」对象语义 = 运行时能力提供方(用户裁决 2026-09-01 视同确认,
//!   选项已按推荐执行):清单 = 内置能力(系统类,禁卸载)+ MCP 服务器组
//!   (卸载 = 移出 MCP 配置文件,重启生效);PIN 是壳子本地偏好,不入后端;
//! - 变更生效时机沿 ADR-0012 口径:落盘后**下次启动生效**(v0 诚实边界,
//!   前端明示「重启生效」)。
//!
//! 安全:
//! - provider apiKey 回显恒打码(与 config_store 同口径,INV-5 面);
//! - 文件浏览限 workspace_root 内:路径组件白名单(Normal 段)+ 逐级
//!   拒符号链接 + realpath 包含校验(X-01 先例:lstat 拒链 + realpath
//!   包含校验);文件读取只读、上限走 limits、非 UTF-8 拒(二进制不预览)。
//!
//! 模块地图(2026-09-07 拆分:纯机械移动,零语义变化;对外符号路径不变):
//! - [`providers`] provider 库 CRUD/探针/当前模型 + 路由重建
//! - [`mcp`]        MCP 配置/探活/候选批准/播种/墓碑/热同步/插件清单
//! - [`roles`]      多角色管理
//! - [`skills`]     技能库
//! - [`approvals`]  审批裁决与列表
//! - [`fs`]         工作区文件浏览/全盘目录浏览/改名/下载/删除/新建
//! - [`logs`]       运行日志尾部直读
//! - [`context`]    上下文透视/检索/会话历史回放/会话删除
//! - [`limits`]     W10 运行时限制面
//! - [`jobs`]       W10 后台作业列表

mod approvals;
mod context;
mod fs;
mod jobs;
mod limits;
mod logs;
mod mcp;
mod providers;
mod roles;
mod skills;
mod tail;

pub use mcp::seed_bundled_plugins;
pub use providers::rebuild_routes;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

/// 管理面配置(服务器启动时装配注入)。
#[derive(Clone)]
pub struct AdminConfig {
    /// 数据目录(config/ 文件根:providers.json / model.json)。
    pub data_dir: PathBuf,
    /// 文件浏览根(BOEN_WORKSPACE_DIR env > <data_dir>/workspace)。
    pub workspace_root: PathBuf,
    /// MCP 配置文件路径(--mcp-config;None = 未启用 MCP 接入)。
    pub mcp_config: Option<PathBuf>,
    /// 内置能力清单摘要(server 启动时从 capability 注册集提取:
    /// [{name, provider, effect, idempotent}])。
    pub builtin_caps: Arc<Vec<Value>>,
    /// 已装载的 MCP 服务器([{name, tools}];reload 会追加,可变共享)。
    pub mcp_servers: Arc<std::sync::RwLock<Vec<Value>>>,
    /// 热装载句柄:运行期把新 MCP server 的能力注册进核心(actor 命令)。
    pub handle: bm_core::runtime::RuntimeHandle,
    /// MCP hub(与启动装载共用同一实例;None = 启动未配 --mcp-config)。
    pub hub: Option<Arc<bm_providers::mcp::McpHub>>,
    /// MCP env secret: 引用解析用加密库(与启动装载同一实例)。
    pub secrets: Option<Arc<dyn bm_core::ports::SecretStore>>,
    /// W6:对话级模型路由表(providers 写后重建;None = 未装配,如测试态)。
    pub model_routes: Option<Arc<bm_providers::routing::RoutingConnector>>,
    /// W7 在线升级:应用层停机信号(apply 后排空本进程);None = 测试态。
    pub shutdown: Option<Arc<tokio::sync::Notify>>,
    /// W7 在线升级:Web 静态目录(--web-dir,升级时覆盖 dist);None = 未挂载。
    pub web_dir: Option<PathBuf>,
    /// 官方随包 MCP 插件目录(exe 同级 plugins/,v0.0.4 起随包发布)。扫描与
    /// 批准同数据目录 mcp/ 对待,同名候选以数据目录优先;None = 测试态或
    /// 无法定位(开发态 cargo run 无此目录,静默跳过)。
    pub bundled_plugins_dir: Option<PathBuf>,
    /// W10(ADR-0024):运行时限制共享单元(缺省 = 代码默认;测试态零变化)。
    pub limits: bm_core::limits::LimitsCell,
    /// W10:来源追踪(env/file 徽标;PUT 后随写更新)。
    pub limits_sources: Arc<std::sync::Mutex<bm_core::limits::LimitsSources>>,
    /// W10(ADR-0025):后台作业台账(/admin/jobs;None = 未装配,测试态)。
    pub jobs: Option<Arc<bm_providers::jobs::JobTable>>,
}

// W10:预览/下载/删除/浏览上限走 cfg.limits
// (原 FILE_PREVIEW_LIMIT=512KB 等常量已收编进 limits,默认值同前)。

/// 管理面统一错误形状(壳子私用 REST 惯例,非 Wire 信封)。
pub(crate) fn admin_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({ "error": { "message": message.into() } })),
    )
        .into_response()
}

/// 管理面子路由(挂载于 /admin;公开 = W1 同款已登记欠账)。
pub fn admin_routes(cfg: AdminConfig) -> axum::Router {
    use approvals::*;
    use axum::routing::{delete, get, post, put};
    use context::*;
    use fs::*;
    use jobs::*;
    use limits::*;
    use logs::*;
    use mcp::*;
    use providers::*;
    use roles::*;
    use skills::*;
    let cfg = Arc::new(cfg);
    axum::Router::new()
        .route("/providers", get(providers_list).post(providers_create))
        .route("/providers/probe", post(providers_probe))
        .route(
            "/providers/{id}",
            put(providers_update).delete(providers_delete),
        )
        .route("/model/active", get(model_active_get).put(model_active_set))
        .route("/mcp", get(mcp_list).post(mcp_create))
        .route("/mcp/reload", post(mcp_reload))
        .route("/mcp/candidates", post(mcp_candidates))
        .route("/mcp/approve", post(mcp_approve))
        .route("/mcp/test/{name}", post(mcp_test))
        .route("/mcp/search-test/{name}", post(mcp_search_test))
        .route("/mcp/usage/{name}", get(mcp_usage))
        .route("/mcp/status", get(mcp_status))
        .route(
            "/mcp-config/{name}",
            get(mcp_config_get).put(mcp_config_set),
        )
        .route("/mcp/{name}", put(mcp_update).delete(mcp_delete))
        // ADR-0023:卸载并物理删除插件文件(警告栏确认后调用)
        .route("/mcp/{name}/purge", post(mcp_purge))
        .route("/capabilities", get(capabilities_list))
        .route("/roles", get(roles_get).post(roles_set).put(roles_set))
        .route("/roles/{id}", put(roles_set).delete(roles_delete))
        .route("/roles/active/{id}", put(roles_set_active))
        .route("/approvals", get(approvals_list))
        .route("/approvals/{id}/respond", post(approval_respond))
        .route("/skills", get(skills_get).post(skills_set))
        .route("/skills/{id}", delete(skills_delete))
        .route("/logs", get(logs_tail))
        .route("/context", get(context_tail))
        .route("/context/search", get(context_search))
        // W10(ADR-0024/0025):限制配置面 + 后台作业列表
        .route("/limits", get(limits_get).put(limits_put))
        .route("/jobs", get(jobs_list))
        // 会话目录(2026-09-08 三端一致批):服务端权威列表,前端启动即拉
        .route("/sessions", get(session_list))
        // 会话历史回放(2026-09-06):切会话/刷新后前端按此拉历史消息
        .route("/sessions/{session_id}/messages", get(session_messages))
        // 会话删除(2026-09-06 A+B):墓碑+原文擦除,经核心单写者执行
        .route(
            "/sessions/{session_id}",
            axum::routing::delete(session_delete),
        )
        // P1-5: 取消在途操作
        .route(
            "/operations/{operation_id}/cancel",
            post(context::operation_cancel),
        )
        .route("/fs/list", get(fs_list))
        // 工作目录选择器:全盘只读目录浏览(仅目录名,零内容)
        .route("/fs/browse", get(fs_browse))
        .route("/fs/file", get(fs_file))
        // W7 目录树右键菜单:重命名 / 下载(文件)与打包下载(文件夹 zip)
        .route("/fs/rename", post(fs_rename))
        .route("/fs/download", get(fs_download))
        // 2026-09-07 目录树批次:新建目录(选择器全盘)与删除(工作区多选)
        .route("/fs/mkdir", post(fs_mkdir))
        .route("/fs/delete", post(fs_delete))
        // W7 关于与在线升级(apply 仅回环;铁规矩:绝不由此触发发布)
        .route("/about", get(crate::about::about))
        .route("/about/check-update", post(crate::about::check_update))
        .route("/about/apply-update", post(crate::about::apply_update))
        // W8 常规:工作区注册表 CRUD/探测 + 运行环境探针(ADR-0018)
        .route(
            "/workspaces",
            get(crate::workspace_admin::workspaces_list)
                .post(crate::workspace_admin::workspaces_create),
        )
        .route(
            "/workspaces/{id}",
            axum::routing::put(crate::workspace_admin::workspaces_update)
                .delete(crate::workspace_admin::workspaces_delete),
        )
        .route(
            "/workspaces/{id}/check",
            post(crate::workspace_admin::workspaces_check),
        )
        .route("/runtime/env", get(crate::workspace_admin::runtime_env))
        .with_state((*cfg).clone())
}
