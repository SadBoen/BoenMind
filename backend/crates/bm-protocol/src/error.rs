//! 类型化错误（kernel.chat 风格）：所有错误带机器可读的 code，
//! 禁止裸 string 错误（实现方案 §5-9）。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 机器可读错误码。名称即语义，跨进程传输时保持稳定（snake_case）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 能力模式串不匹配：插件请求了未授予的能力
    CapabilityEscalationDenied,
    /// 资源预算耗尽（token/磁盘/调用次数）
    BudgetExceeded,
    /// 事件 seq 跳号（预期的 next 与实际不一致）
    SeqGap,
    /// 事件 seq 重复（同 (session, branch) 下 seq 已存在）
    SeqDuplicate,
    /// 未知且必需的事件（ignorable=false）——旧版本拒绝重建新日志
    UnknownRequiredEvent,
    /// 事件格式版本不兼容（写者决定 bump；读者拒绝重建，走迁移链）
    FormatVersionMismatch,
    /// 消息面操作违规（如 Replace 区间非法）
    SurfaceViolation,
    /// 存储不可用（打开失败/事务失败）
    StoreUnavailable,
    /// 找不到目标（服务 key/分支/事件）
    NotFound,
    /// 参数非法（类型不匹配/格式错误）
    InvalidArgument,
    /// 服务 key 已注册（重复注册拒绝）
    AlreadyRegistered,
    /// fork 冲突（分支已存在 / 从超头 fork）
    ForkConflict,
    /// 插件依赖未就绪或安装失败
    PluginInstall,
}

/// 契约层统一错误类型：code + 人读 message。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code_str(), self.message)
    }
}

impl std::error::Error for ProtocolError {}

impl ProtocolError {
    fn code_str(&self) -> &'static str {
        match self.code {
            ErrorCode::CapabilityEscalationDenied => "capability_escalation_denied",
            ErrorCode::BudgetExceeded => "budget_exceeded",
            ErrorCode::SeqGap => "seq_gap",
            ErrorCode::SeqDuplicate => "seq_duplicate",
            ErrorCode::UnknownRequiredEvent => "unknown_required_event",
            ErrorCode::FormatVersionMismatch => "format_version_mismatch",
            ErrorCode::SurfaceViolation => "surface_violation",
            ErrorCode::StoreUnavailable => "store_unavailable",
            ErrorCode::NotFound => "not_found",
            ErrorCode::InvalidArgument => "invalid_argument",
            ErrorCode::AlreadyRegistered => "already_registered",
            ErrorCode::ForkConflict => "fork_conflict",
            ErrorCode::PluginInstall => "plugin_install",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_serde_roundtrip() {
        let e = ProtocolError::new(ErrorCode::SeqGap, "expected 5, got 7");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("seq_gap"));
        let back: ProtocolError = serde_json::from_str(&json).unwrap();
        assert_eq!(back.code(), ErrorCode::SeqGap);
        assert_eq!(back.message, "expected 5, got 7");
    }

    #[test]
    fn error_displays_with_code() {
        let e = ProtocolError::new(ErrorCode::UnknownRequiredEvent, "unknown required event type");
        assert!(e.to_string().contains("unknown_required_event"));
    }
}
