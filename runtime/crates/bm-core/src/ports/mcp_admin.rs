//! MCP 管理面端口(ADR-0046):surface 驱动 MCP 运行期控制与装载的契约。
//!
//! 背景:`bm-surface-http` 的 MCP 管理面(探活/原语请求/stderr/配置热同步)
//! 原先直接持具体 `bm_providers::mcp::McpHub` 与 `supervisor::*`,使 surface 常规
//! 依赖 `bm-providers`。本端口把管理面**实际用到**的操作上移 core,surface 只依赖
//! 端口——与 `ModelRouter`(ADR-0042)、`SkillHost`(ADR-0046)同构。
//!
//! DTO 一并上移:`StderrLine`(遥测)、`SyncOutcome`(同步结果)、
//! `CapabilityRegistrar`(同步期注册回调,本就引用 core 的 `CapabilityProvider`)。

use async_trait::async_trait;
use bm_contract::capability::CapabilityManifest;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

use super::SecretStore;
use crate::limits::LimitsCell;
use crate::registry::CapabilityProvider;

/// 一条子进程 stderr 行(带代标记,跨 respawn 可辨)。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StderrLine {
    pub generation: u64,
    pub text: String,
}

/// MCP 配置同步结果(装载/更新/卸载/失败 + 已装载快照)。
#[derive(Debug, Clone, Default)]
pub struct SyncOutcome {
    pub registered: Vec<String>,
    pub updated: Vec<String>,
    pub uninstalled: Vec<String>,
    pub failed: Vec<Value>,
    /// 已装载快照(热装载写回管理面用;启动路径忽略)。
    pub note_loaded: Vec<Value>,
}

/// 同步期的能力注册回调(启动路径收集成 vec;热路径直接调 handle)。
#[async_trait]
pub trait CapabilityRegistrar: Send + Sync {
    async fn register(
        &self,
        entries: Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)>,
    ) -> Result<(), String>;
    async fn unregister(&self, names: Vec<String>) -> Result<(), String>;
}

/// MCP 管理面端口。实现方 = `bm_providers::mcp::McpHub`。
#[async_trait]
pub trait McpAdmin: Send + Sync {
    /// 探活:返回 (工具数, 工具定义列表)。
    async fn probe_server(&self, server: &str) -> Result<(usize, Vec<Value>), String>;

    /// 原语请求(管理面探针用,如 web_search_test / web_usage)。
    async fn raw_request(&self, server: &str, method: &str, params: Value)
    -> Result<Value, String>;

    /// 子进程 stderr 尾部环形缓冲。
    fn stderr_tail(&self, server: &str, lines: usize) -> Result<Vec<StderrLine>, String>;

    /// 该 server 的握手能力快照。
    fn server_capabilities(&self, server: &str) -> Result<Value, String>;

    /// 断开并摘除该 server 的全部路由,返回被摘除的能力名。
    async fn disconnect_server(&self, server: &str) -> Vec<String>;

    /// 按配置文件同步(装载/更新/卸载)并把能力经 registrar 注册进核心。
    async fn sync(
        &self,
        cfg_path: &Path,
        secrets: Arc<dyn SecretStore>,
        loaded_names: Vec<String>,
        registrar: &dyn CapabilityRegistrar,
        limits: &LimitsCell,
    ) -> SyncOutcome;
}

/// 读 MCP 安装配置(裸 JSON 数组 `[{server},…]`);文件不存在 = 空清单。
/// 语义与 `bm_providers::mcp::supervisor::read_mcp_servers` 逐字一致(ADR-0046
/// 上移单源):**仅 NotFound 视为空**——其他 IO 错误(权限/瞬时故障)必须上抛,
/// 否则热重载会把全部 MCP 能力静默卸载、甚至被整表回写覆盖丢配置(P1-9)。
pub fn read_mcp_servers(path: &Path) -> Result<Vec<Value>, String> {
    // #71:读原语单源(bm_core::json_store);策略不变——仅 NotFound 视为空。
    match crate::json_store::read_json_file(path, "MCP 配置读取失败", "MCP 配置不是 JSON 数组")
    {
        Ok(crate::json_store::JsonRead::Value(v)) => {
            serde_json::from_value(v).map_err(|e| format!("MCP 配置不是 JSON 数组: {e}"))
        }
        Ok(crate::json_store::JsonRead::Missing) => Ok(vec![]),
        Err(e) => Err(e),
    }
}
