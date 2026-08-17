//! 工具契约：schema、处理器 trait 与执行输入/输出。

use serde::{Deserialize, Serialize};

use crate::error::ToolError;

/// 工具对外 schema（发给模型的 JSON Schema）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    /// JSON Schema 对象（`{"type":"object","properties":{...}}`）。
    pub parameters: serde_json::Value,
}

/// 工具执行模式：独占或可并行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    Exclusive,
    Parallel,
}

/// 工具执行输入。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolExecutionInput {
    pub name: String,
    /// 模型给出的参数（已按 schema 校验）。
    pub arguments: serde_json::Value,
}

/// 工具执行结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    pub output: String,
    pub is_error: bool,
}

impl ToolExecutionResult {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: false,
        }
    }

    pub fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: true,
        }
    }
}

/// 工具处理器 trait。
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Exclusive
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError>;
}

impl<T> From<&T> for ToolSchema
where
    T: ToolHandler,
{
    fn from(h: &T) -> Self {
        ToolSchema {
            name: h.name().to_string(),
            description: h.description().to_string(),
            parameters: h.parameters(),
        }
    }
}
