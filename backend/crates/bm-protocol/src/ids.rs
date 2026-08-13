//! typed 标识符（Life Agent OS 风格）：编译期区分不同维度的 ID，
//! 防止 SessionId 与 BranchId 互相传错的低级事故。
//!
//! 格式约定（非强制校验，构造即包装）：
//! - SessionId: `sess_<hex16>`
//! - BranchId:  `main` | `br_<hex16>`
//! - CallId:    任意字符串（工具调用 id）

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }
    };
}

id_type!(SessionId, "会话 ID：`sess_<hex16>`");
id_type!(BranchId, "分支 ID：`main` 或 `br_<hex16>`（fork 产生）");
id_type!(CallId, "工具调用 ID（ToolCall ↔ ToolResult 关联键）");

/// 分支内单调递增的序号（每个 (session, branch) 独立计数，从 1 起）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SeqNo(pub u64);

impl SeqNo {
    pub fn new(n: u64) -> Self {
        Self(n)
    }
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for SeqNo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for SeqNo {
    fn from(n: u64) -> Self {
        Self(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_ids_do_not_confuse() {
        let sid = SessionId::new("sess_abc");
        let bid = BranchId::new("main");
        // 编译期类型不同，无法直接比较/互换；仅包装层共享 Display
        assert_eq!(sid.to_string(), "sess_abc");
        assert_eq!(bid.as_str(), "main");
        assert_eq!(SeqNo::new(1).as_u64(), 1);
    }

    #[test]
    fn ids_serde_roundtrip() {
        let sid = SessionId::new("sess_abc");
        let json = serde_json::to_string(&sid).unwrap();
        assert_eq!(json, "\"sess_abc\"");
        let back: SessionId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sid);

        let seq = SeqNo::new(42);
        let json = serde_json::to_string(&seq).unwrap();
        assert_eq!(json, "42");
        let back: SeqNo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, seq);
    }
}
