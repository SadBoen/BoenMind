//! 核心错误 → Wire 错误信封映射。message 一律脱敏:不含输入原文与凭据。

use bm_contract::error_codes::ErrorCode;
use bm_contract::wire::WireError;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    /// 已是合同错误码形态的语义错误(消息由核心生成,天然脱敏)。
    Semantic(ErrorCode, String),
    #[error("核心内部错误")]
    Internal,
}

impl CoreError {
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Semantic(ErrorCode::ValidationFailed, msg.into())
    }

    pub fn to_wire(&self) -> WireError {
        match self {
            CoreError::Semantic(code, msg) => WireError::new(*code, msg.clone()),
            CoreError::Internal => WireError::new(ErrorCode::Internal, "核心内部错误"),
        }
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
