//! 错误码注册表镜像(registry/error-codes.v0_1.json,核心码封闭,基线 9.8)。
//!
//! Wire 信封可用码 = 注册表全量 11 码(M1 七码 + M4 四码 permission_denied /
//! approval_required / approval_denied / idempotency_conflict;M4 起信封枚举
//! 同步增发,CI 规则 R6)。

wire_str_enum!(ErrorCode {
    ValidationFailed => "validation_failed",
    Unavailable => "unavailable",
    Timeout => "timeout",
    Cancelled => "cancelled",
    BudgetExceeded => "budget_exceeded",
    OutcomeUnknown => "outcome_unknown",
    Internal => "internal",
    PermissionDenied => "permission_denied",
    ApprovalRequired => "approval_required",
    ApprovalDenied => "approval_denied",
    IdempotencyConflict => "idempotency_conflict",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Since {
    M1,
    M4,
}

impl Since {
    pub fn as_str(self) -> &'static str {
        match self {
            Since::M1 => "M1",
            Since::M4 => "M4",
        }
    }
}

impl ErrorCode {
    /// 该码最早可出现的里程碑(注册表 available_since)。
    pub fn available_since(self) -> Since {
        match self {
            ErrorCode::PermissionDenied
            | ErrorCode::ApprovalRequired
            | ErrorCode::ApprovalDenied
            | ErrorCode::IdempotencyConflict => Since::M4,
            _ => Since::M1,
        }
    }

    /// 注册表 default_retryable。
    pub fn default_retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::Unavailable | ErrorCode::Timeout | ErrorCode::ApprovalRequired
        )
    }

    /// 注册表 cli_exit(M3 起使用,M1 先随注册表固化)。
    pub fn cli_exit(self) -> i32 {
        match self {
            ErrorCode::ValidationFailed => 2,
            ErrorCode::Unavailable | ErrorCode::Timeout => 7,
            ErrorCode::Cancelled => 0,
            ErrorCode::BudgetExceeded | ErrorCode::Internal => 5,
            ErrorCode::OutcomeUnknown => 6,
            ErrorCode::PermissionDenied | ErrorCode::ApprovalDenied => 3,
            ErrorCode::ApprovalRequired => 4,
            ErrorCode::IdempotencyConflict => 2,
        }
    }

    /// 注册表全部码(顺序与 JSON 一致)。
    pub const ALL: [ErrorCode; 11] = [
        ErrorCode::ValidationFailed,
        ErrorCode::Unavailable,
        ErrorCode::Timeout,
        ErrorCode::Cancelled,
        ErrorCode::BudgetExceeded,
        ErrorCode::OutcomeUnknown,
        ErrorCode::Internal,
        ErrorCode::PermissionDenied,
        ErrorCode::ApprovalRequired,
        ErrorCode::ApprovalDenied,
        ErrorCode::IdempotencyConflict,
    ];
}

/// M1 可用码(M1 时期信封枚举的 7 项,顺序一致;保留作历史对照)。
pub const M1_WIRE_CODES: [ErrorCode; 7] = [
    ErrorCode::ValidationFailed,
    ErrorCode::Unavailable,
    ErrorCode::Timeout,
    ErrorCode::Cancelled,
    ErrorCode::BudgetExceeded,
    ErrorCode::OutcomeUnknown,
    ErrorCode::Internal,
];

/// Wire 信封可用码全量(M4 起 = M1 ∪ M4,与 envelope schema error_code 枚举
/// 11 项逐位一致,CI 规则 R6)。
pub const WIRE_CODES: [ErrorCode; 11] = ErrorCode::ALL;

/// Wire 信封错误码:仅允许注册表内的码(M1 ∪ M4 全量),反序列化强制校验。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WireErrorCode(ErrorCode);

impl WireErrorCode {
    pub fn new(code: ErrorCode) -> Option<Self> {
        // 核心码封闭(基线 9.8):注册表全量码自其 available_since 起可用;
        // M4 起全部 11 码均在信封枚举内,故门禁 = 必须是注册表已知码。
        ErrorCode::ALL.contains(&code).then_some(Self(code))
    }

    pub fn get(self) -> ErrorCode {
        self.0
    }
}

impl TryFrom<ErrorCode> for WireErrorCode {
    type Error = &'static str;

    fn try_from(code: ErrorCode) -> Result<Self, Self::Error> {
        Self::new(code).ok_or("未知错误码(不在注册表内)")
    }
}

impl serde::Serialize for WireErrorCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for WireErrorCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let code = ErrorCode::from_wire(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("未知错误码: {s}")))?;
        WireErrorCode::new(code).ok_or_else(|| serde::de::Error::custom(format!("未知错误码: {s}")))
    }
}
