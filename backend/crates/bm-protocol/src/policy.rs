//! 能力模式串与策略评估（A4 能力矩阵的契约形态）。
//!
//! 能力 = glob 模式串，如 `fs:write:/session/**`。策略评估在阶段 2
//! （把关链/权限）才真正接入内核；这里只定义契约形态，保证后续
//! 阶段不需要改协议层。

use serde::{Deserialize, Serialize};

use crate::ErrorCode;

/// 能力模式串：`域:动作:目标glob`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capability(pub String);

impl Capability {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 策略评估结果（阶段 2 把关链接入点）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyEvaluation {
    /// 放行
    Allowed,
    /// 需要用户确认（挂起等待，附理由）
    RequiresApproval { justification: String },
    /// 拒绝（带机器可读错误码）
    Denied(ErrorCode),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_serde_roundtrip() {
        let c = Capability::new("fs:write:/session/**");
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, r#""fs:write:/session/**""#);
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }
}
