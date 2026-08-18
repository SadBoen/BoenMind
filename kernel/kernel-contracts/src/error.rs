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

/// LLM 失败的结构化事实（对齐 DSH `LlmFailure`：message/code/status/
/// providerRetryAfterMs/requestId）。可选字段 None 时不上 wire（缺失字段省略，
/// 对齐 adapter.spec 的 toEqual 精确形状断言）。code 永远非空（归一化兜底 UNKNOWN）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureInfo {
    pub message: String,
    pub code: String,
    pub status: Option<u16>,
    pub provider_retry_after_ms: Option<u64>,
    pub request_id: Option<String>,
}

/// LLM 调用错误。
#[derive(Debug, Clone, Error)]
#[error("llm failure: {message}")]
pub struct LlmError {
    pub message: String,
    /// 可重试性提示（供上层退避策略参考）。
    pub retryable: bool,
    /// 稳定机器可路由错误码（None = 归一化回退 UNKNOWN，对齐
    /// `normalizeLlmFailure` 的终态契约）。
    pub code: Option<String>,
    /// HTTP 状态码（适配器在带状态的事实上保留）。
    pub status: Option<u16>,
    /// 提供商重试等待（毫秒；来自 Retry-After 头，0/非法/过去省略）。
    pub provider_retry_after_ms: Option<u64>,
    /// 提供商请求 id（`x-request-id` 回退 `x-deepseek-request-id`）。
    pub request_id: Option<String>,
}

impl LlmError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
            code: None,
            status: None,
            provider_retry_after_ms: None,
            request_id: None,
        }
    }

    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
            code: None,
            status: None,
            provider_retry_after_ms: None,
            request_id: None,
        }
    }

    /// 带结构化事实的错误（对齐 DSH `LlmError(message, code, {status, ...})`）。
    pub fn structured(
        message: impl Into<String>,
        code: impl Into<String>,
        status: Option<u16>,
        provider_retry_after_ms: Option<u64>,
        request_id: Option<String>,
    ) -> Self {
        Self {
            message: message.into(),
            retryable: false,
            code: Some(code.into()),
            status,
            provider_retry_after_ms,
            request_id,
        }
    }

    /// 归一化为 DSH `LlmFailure` 终态形状：message 空 → "LLM adapter failed"、
    /// code 缺失 → "UNKNOWN"（对齐 `normalizeLlmFailure` 的两条兜底路径；
    /// 恶意 coercion/访问器字段在 Rust 侧无等价物，类型系统已隔离）。
    pub fn to_failure(&self) -> FailureInfo {
        let message = if self.message.trim().is_empty() {
            "LLM adapter failed".to_string()
        } else {
            self.message.clone()
        };
        FailureInfo {
            message,
            code: self
                .code
                .clone()
                .unwrap_or_else(|| "UNKNOWN".to_string()),
            status: self.status,
            provider_retry_after_ms: self.provider_retry_after_ms,
            request_id: self.request_id.clone(),
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
