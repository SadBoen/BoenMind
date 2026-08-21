//! 工具注册表 + 门控端口（产品契约层）。
//!
//! 核心插件 loop 需要"工具注册表 + 门控"能力，但内核契约 `kernel-contracts::tools`
//! 只定义了工具类型（ToolSchema/ToolHandler/…）——注册表/门控是插件实现
//! （plugin-tools）的具体类型。2026-08-20 回收：loop 不得编译期依赖
//! plugin-tools 具体类型，改为依赖本层端口，plugin-tools 实现之，assembly 注入。
//!
//! 本层放的是**产品级所需的最小端口**（loop 只消费 `enabled_schemas` 与
//! `execute_guarded`）；装配面（register/unregister/enable…）保留在
//! plugin-tools 具体类型，组合根调用。

use std::sync::Arc;

use kernel_contracts::tools::{ToolExecutionInput, ToolExecutionResult, ToolHandler, ToolSchema};
use kernel_contracts::ToolError;

/// 工具注册表端口（loop/上层消费面）：schema 清单 + 执行。
/// 装配面（register/unregister）在具体实现（plugin-tools）上，由组合根调用。
#[async_trait::async_trait]
pub trait ToolRegistryPort: Send + Sync + std::fmt::Debug {
    /// 所有已注册工具的 schema（按名称字典序稳定输出）。
    fn schemas(&self) -> Vec<ToolSchema>;
    /// 执行工具（含 schema 校验）。
    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError>;

    /// 该工具是否需要用户审批（危险面声明）。默认 `false` = 自动放行；
    /// 装配面（注册器 mark_dangerous）标记危险工具后覆写。loop 审批点在
    /// 执行前调用——只有 `true` 的工具才弹审批窗（对齐 approval.rs 设计
    /// 注释「仅声明了需要审批的工具会暂停」）。
    fn requires_approval(&self, _name: &str) -> bool {
        false
    }
}

/// 工具**注册面**端口（插件装配用）：插件把 handler 注册进注册表。
/// 2026-08-21 回头看新增：原功能插件 `register_all(registry: &plugin_tools::ToolRegistry)`
/// 编译期依赖核心插件 plugin-tools 的具体类型，违反「插件之间零依赖」纪律——
/// 故把装配面抽象到本层，plugin-tools 的 `ToolRegistry` 实现之，组合根注入。
/// 核心/功能插件只依赖本端口，不再依赖 plugin-tools 具体类型。
#[async_trait::async_trait]
pub trait ToolRegistrarPort: Send + Sync + std::fmt::Debug {
    /// 注册一个工具处理器；重名 → Err（留具体实现语义）。
    fn register(&self, handler: Arc<dyn ToolHandler>) -> Result<(), ToolError>;
    /// 按名取处理器（幂等注册检查用）。
    fn get(&self, name: &str) -> Option<Arc<dyn ToolHandler>>;
    /// 声明工具为危险（需要审批；装配面 mark_dangerous 同义）。
    fn mark_dangerous(&self, name: &str);
}

/// 工具门控端口（fail-closed 消费面）。
/// 装配面（enable/disable）在具体实现（plugin-tools）上，由组合根调用。
#[async_trait::async_trait]
pub trait ToolGatePort: Send + Sync + std::fmt::Debug {
    /// 只返回已启用工具的 schema（发给模型的工具清单）。
    fn enabled_schemas(&self, registry: &dyn ToolRegistryPort) -> Vec<ToolSchema>;
    /// fail-closed 执行：未启用 → Err；启用则经注册表执行。
    async fn execute_guarded(
        &self,
        registry: &dyn ToolRegistryPort,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError>;
}