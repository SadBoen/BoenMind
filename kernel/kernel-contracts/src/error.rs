//! 跨层统一错误类型。
//!
//! fail-loud 纪律：未注册的能力一律返回 `PortErrorKind::NotAvailable`，
//! 调用方必须显式处理，绝不静默跳过（借鉴 bobleer `PluginRuntimePort` 形状）。

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 端口级错误的类别。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortErrorKind {
    /// 能力未注册/未装配（fail-loud 探针的否定结果）。
    NotAvailable,
    /// 目标不存在。
    NotFound,
    /// 请求不合法。
    InvalidRequest,
    /// 权限不足。
    PermissionDenied,
    /// 操作被取消。
    Cancelled,
    /// 超时。
    Timeout,
    /// 后端/底层错误。
    Backend,
}

/// 端口统一错误。
#[derive(Debug, Clone, Error)]
#[error("{kind:?}: {message}")]
pub struct PortError {
    pub kind: PortErrorKind,
    pub message: String,
}

impl PortError {
    pub fn not_available(what: &str) -> Self {
        Self {
            kind: PortErrorKind::NotAvailable,
            message: format!("{what} is not registered"),
        }
    }

    pub fn not_found(what: &str) -> Self {
        Self {
            kind: PortErrorKind::NotFound,
            message: what.to_string(),
        }
    }

    pub fn invalid_request(what: &str) -> Self {
        Self {
            kind: PortErrorKind::InvalidRequest,
            message: what.to_string(),
        }
    }

    pub fn permission_denied(what: &str) -> Self {
        Self {
            kind: PortErrorKind::PermissionDenied,
            message: what.to_string(),
        }
    }

    pub fn backend(what: impl Into<String>) -> Self {
        Self {
            kind: PortErrorKind::Backend,
            message: what.into(),
        }
    }
}

/// 端口级结果别名。
pub type PortResult<T> = Result<T, PortError>;

/// LLM 调用错误。
#[derive(Debug, Clone, Error)]
#[error("llm failure: {message}")]
pub struct LlmError {
    pub message: String,
    /// 可重试性提示（供上层退避策略参考）。
    pub retryable: bool,
}

impl LlmError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }

    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }
}

/// 工具执行错误。
#[derive(Debug, Clone, Error)]
#[error("tool error: {0}")]
pub struct ToolError(pub String);

impl ToolError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
