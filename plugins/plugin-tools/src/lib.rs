//! # kernel-tools
//!
//! 工具注册表 + 门控（enabled 名单 + fail-closed）。
//!
//! `ToolRegistry` 持有 name → handler 映射，`execute` 先按 handler 的
//! `parameters()` JSON Schema 校验 arguments，通过后再调用 handler 本体。
//! `ToolGate` 维护 enabled 名单（默认 fail-closed，全禁用）：未启用的工具
//! 不会出现在 `enabled_schemas` 里，也一律被 `execute_guarded` 拒绝。
//!
//! 工具插件化：注册表运行期可装卸（`register` / `unregister`），
//! 对应 dsh 官方 `@deepseek-ai/dsh-tools` 的工具注册面。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use kernel_contracts::{ToolError, ToolExecutionInput, ToolExecutionResult, ToolHandler, ToolSchema};
use parking_lot::RwLock;

pub mod plugin;

/// 工具注册表：name → handler。
pub struct ToolRegistry {
    handlers: RwLock<HashMap<String, Arc<dyn ToolHandler>>>,
    /// 危险工具名单（装配面 mark_dangerous 注入）：`requires_approval(name)`
    /// 返回 true → loop 执行前弹审批窗。默认空 = 全自动放行（审批面存在但
    /// 不拦安全工具）。
    dangerous: RwLock<HashSet<String>>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 不依赖 ToolHandler: Debug（契约层只要求 Send+Sync）；只打印已注册工具名。
        let mut names: Vec<String> = self.handlers.read().keys().cloned().collect();
        names.sort();
        f.debug_struct("ToolRegistry").field("handlers", &names).finish()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
            dangerous: RwLock::new(HashSet::new()),
        }
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 声明工具为危险（需要审批）。装配面调用——组合根在各插件 install_*
    /// 里 mark。未注册的工具名静默忽略（宽松，避免装配顺序耦合）。
    pub fn mark_dangerous(&self, name: &str) {
        self.dangerous.write().insert(name.to_string());
    }

    /// 批量声明危险工具。
    pub fn mark_dangerous_many<'a>(&self, names: impl IntoIterator<Item = &'a str>) {
        let mut set = self.dangerous.write();
        for n in names {
            set.insert(n.to_string());
        }
    }

    /// 注册一个工具处理器；重名返回 `Err("tool '{name}' already registered")`。
    pub fn register(&self, handler: Arc<dyn ToolHandler>) -> Result<(), ToolError> {
        let name = handler.name().to_string();
        let mut handlers = self.handlers.write();
        if handlers.contains_key(&name) {
            return Err(ToolError::new(format!("tool '{name}' already registered")));
        }
        handlers.insert(name, handler);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        self.handlers.read().get(name).cloned()
    }

    /// 卸载一个工具（热插拔：注册表运行期装卸）。不存在返回 Err。
    pub fn unregister(&self, name: &str) -> Result<(), ToolError> {
        let mut handlers = self.handlers.write();
        if handlers.remove(name).is_none() {
            return Err(ToolError::new(format!("tool '{name}' not registered")));
        }
        Ok(())
    }

    /// 所有已注册工具的 schema（按名称字典序稳定输出）。
    pub fn schemas(&self) -> Vec<ToolSchema> {
        let mut schemas: Vec<ToolSchema> = self
            .handlers
            .read()
            .values()
            .map(|h| ToolSchema {
                name: h.name().to_string(),
                description: h.description().to_string(),
                parameters: h.parameters(),
            })
            .collect();
        schemas.sort_by(|a, b| a.name.cmp(&b.name));
        schemas
    }

    /// 执行工具：先用 `handler.parameters()` 做 jsonschema 校验（draft 2020-12），
    /// 校验失败返回 Err；通过后调用 handler 本体。
    pub async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let handler = self
            .get(&input.name)
            .ok_or_else(|| ToolError::new(format!("tool '{}' not found", input.name)))?;
        #[cfg(feature = "schema-validation")]
        {
            let schema = ToolSchema {
                name: handler.name().to_string(),
                description: handler.description().to_string(),
                parameters: handler.parameters(),
            };
            let validator = jsonschema::validator_for(&schema.parameters).map_err(|e| {
                ToolError::new(format!("invalid schema for tool '{}': {e}", input.name))
            })?;
            validator.validate(&input.arguments).map_err(|e| {
                ToolError::new(format!("invalid arguments for tool '{}': {e}", input.name))
            })?;
        }
        handler.execute(input).await
    }
}

// 端口实现（bm-ports）：loop 等上层消费面经 trait object 注入，组合根装配。
#[async_trait::async_trait]
impl bm_ports::ToolRegistryPort for ToolRegistry {
    fn schemas(&self) -> Vec<ToolSchema> {
        ToolRegistry::schemas(self)
    }
    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        ToolRegistry::execute(self, input).await
    }
    fn requires_approval(&self, name: &str) -> bool {
        self.dangerous.read().contains(name)
    }
}

/// 工具门控：enabled 名单 + fail-closed（空名单默认全部禁用）。
#[derive(Debug, Default)]
pub struct ToolGate {
    enabled: RwLock<HashSet<String>>,
}

impl ToolGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enable(&self, name: &str) {
        self.enabled.write().insert(name.to_string());
    }

    pub fn disable(&self, name: &str) {
        self.enabled.write().remove(name);
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.read().contains(name)
    }

    /// 只返回已启用工具的 schema（发给模型的工具清单）。
    pub fn enabled_schemas(&self, registry: &ToolRegistry) -> Vec<ToolSchema> {
        registry
            .schemas()
            .into_iter()
            .filter(|schema| self.is_enabled(&schema.name))
            .collect()
    }

    /// fail-closed 执行：未启用 → `Err("tool '{name}' is disabled (fail-closed)")`，不落执行。
    pub async fn execute_guarded(
        &self,
        registry: &ToolRegistry,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        if !self.is_enabled(&input.name) {
            return Err(ToolError::new(format!(
                "tool '{}' is disabled (fail-closed)",
                input.name
            )));
        }
        registry.execute(input).await
    }
}

// 端口实现（bm-ports）：enabled_schemas / execute_guarded 经 trait object 消费。
#[async_trait::async_trait]
impl bm_ports::ToolGatePort for ToolGate {
    fn enabled_schemas(&self, registry: &dyn bm_ports::ToolRegistryPort) -> Vec<ToolSchema> {
        registry
            .schemas()
            .into_iter()
            .filter(|schema| self.is_enabled(&schema.name))
            .collect()
    }
    async fn execute_guarded(
        &self,
        registry: &dyn bm_ports::ToolRegistryPort,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        if !self.is_enabled(&input.name) {
            return Err(ToolError::new(format!(
                "tool '{}' is disabled (fail-closed)",
                input.name
            )));
        }
        registry.execute(input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct EchoTool;

    #[async_trait]
    impl ToolHandler for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "echo the given text back"
        }

        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                },
                "required": ["text"],
                "additionalProperties": false
            })
        }

        async fn execute(
            &self,
            input: ToolExecutionInput,
        ) -> Result<ToolExecutionResult, ToolError> {
            let text = input
                .arguments
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Ok(ToolExecutionResult::ok(format!("echo:{text}")))
        }
    }

    fn echo_handler() -> Arc<dyn ToolHandler> {
        Arc::new(EchoTool)
    }

    fn echo_input(text: &str) -> ToolExecutionInput {
        ToolExecutionInput {
            name: "echo".to_string(),
            arguments: serde_json::json!({ "text": text }),
        }
    }

    #[test]
    fn register_duplicate_rejected() {
        let registry = ToolRegistry::new();
        registry.register(echo_handler()).unwrap();
        let err = registry.register(echo_handler()).unwrap_err();
        assert!(err.0.contains("already registered"));
        assert_eq!(registry.schemas().len(), 1);
    }

    #[cfg(feature = "schema-validation")]
    #[tokio::test]
    async fn execute_validates_arguments_against_schema() {
        let registry = ToolRegistry::new();
        registry.register(echo_handler()).unwrap();

        // 非法参数：缺少 required 的 text
        let err = registry
            .execute(ToolExecutionInput {
                name: "echo".to_string(),
                arguments: serde_json::json!({ "wrong": 1 }),
            })
            .await
            .unwrap_err();
        assert!(err.0.contains("invalid arguments"));

        // 非法参数：类型错误
        let err = registry
            .execute(ToolExecutionInput {
                name: "echo".to_string(),
                arguments: serde_json::json!({ "text": 42 }),
            })
            .await
            .unwrap_err();
        assert!(err.0.contains("invalid arguments"));

        // 合法参数：通过校验并执行
        let res = registry.execute(echo_input("hi")).await.unwrap();
        assert!(!res.is_error);
        assert_eq!(res.output, "echo:hi");
    }

    #[test]
    fn gate_fail_closed_by_default_and_enabled_schemas() {
        let registry = ToolRegistry::new();
        registry.register(echo_handler()).unwrap();

        let gate = ToolGate::new();
        // 空名单：默认 fail-closed，全部禁用
        assert!(!gate.is_enabled("echo"));
        assert!(gate.enabled_schemas(&registry).is_empty());

        gate.enable("echo");
        assert!(gate.is_enabled("echo"));
        let schemas = gate.enabled_schemas(&registry);
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "echo");

        gate.disable("echo");
        assert!(!gate.is_enabled("echo"));
        assert!(gate.enabled_schemas(&registry).is_empty());
    }

    #[tokio::test]
    async fn execute_guarded_rejects_disabled_tool() {
        let registry = ToolRegistry::new();
        registry.register(echo_handler()).unwrap();
        let gate = ToolGate::new();

        let err = gate
            .execute_guarded(&registry, echo_input("hi"))
            .await
            .unwrap_err();
        assert!(err.0.contains("is disabled (fail-closed)"));
    }

    #[tokio::test]
    async fn execute_guarded_runs_when_enabled() {
        let registry = ToolRegistry::new();
        registry.register(echo_handler()).unwrap();
        let gate = ToolGate::new();
        gate.enable("echo");

        let res = gate
            .execute_guarded(&registry, echo_input("hi"))
            .await
            .unwrap();
        assert_eq!(res.output, "echo:hi");
    }
}
