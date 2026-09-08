//! 核心错误 → Wire 错误信封映射。message 一律脱敏:不含输入原文与凭据。

use bm_contract::error_codes::ErrorCode;
use bm_contract::wire::WireError;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    /// 已是合同错误码形态的语义错误(消息由核心生成,天然脱敏)。
    Semantic(ErrorCode, String),
    #[error("{message}")]
    /// approval_required 的结构化形态:开单点持有 approval_id/operation_id,
    /// 回合管线凭此精确绑定审批卡片,不做任何按名/按参反查
    /// (杜绝多会话并发同能力调用时「批准 A 执行 B」错配)。
    ApprovalNeeded {
        message: String,
        approval_id: String,
        operation_id: String,
    },
    #[error("{message}")]
    /// /v1 面(OpenAI 兼容插座,非信封)结构化错误(issue #40):
    /// ext_code 为扩展码(registry/extensions 命名空间声明,如
    /// webui.workspace_unavailable),让前端按码分支替代文案串匹配;
    /// base 为信封映射码——Wire 信封消费方照旧只见核心 11 码(核心码封闭)。
    Extension {
        message: String,
        ext_code: &'static str,
        base: ErrorCode,
    },
    #[error("核心内部错误")]
    Internal,
}

impl CoreError {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Semantic(ErrorCode::ValidationFailed, msg.into())
    }

    /// /v1 面结构化分支码(不在信封 11 核心码内;None = 无扩展码)。
    pub fn ext_code(&self) -> Option<&'static str> {
        match self {
            CoreError::Extension { ext_code, .. } => Some(ext_code),
            _ => None,
        }
    }

    pub fn to_wire(&self) -> WireError {
        match self {
            CoreError::Semantic(code, msg) => WireError::new(*code, msg.clone()),
            CoreError::Extension { message, base, .. } => WireError::new(*base, message.clone()),
            CoreError::ApprovalNeeded { message, .. } => {
                WireError::new(ErrorCode::ApprovalRequired, message.clone())
            }
            CoreError::Internal => WireError::new(ErrorCode::Internal, "核心内部错误"),
        }
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
