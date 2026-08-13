//! 消息面操作（SurfaceOp）：仅消息面事件（user/assistant/tool）携带。
//!
//! 语义（实现方案 §2.1，D3/D9 压缩遮蔽）：
//! - `Append`：事件向消息面追加内容；
//! - `Replace { start, end }`：事件（通常是压缩摘要）遮蔽 seq ∈ [start, end]
//!   区间在消息面上的贡献，投影按此重建。区间非法（start > end）→ surface_violation。

use serde::{Deserialize, Serialize};

/// 消息面操作。缺省（None）的事件不参与消息面投影。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SurfaceOp {
    Append,
    Replace { start: u64, end: u64 },
}

/// 投影折叠单个事件后的结果（校验器/投影层共用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceOutcome {
    /// 事件不参与消息面
    Ignored,
    /// 正常折叠
    Applied,
    /// 遮蔽了 [start, end] 区间（投影应移除该区间贡献）
    Replaced { start: u64, end: u64 },
}

impl SurfaceOp {
    /// Replace 的 (start, end) 区间；区间非法时返回 None（校验器据此拒绝）。
    pub fn as_interval(&self) -> Option<(u64, u64)> {
        match *self {
            SurfaceOp::Append => None,
            SurfaceOp::Replace { start, end } if start <= end => Some((start, end)),
            SurfaceOp::Replace { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_op_serde_roundtrip() {
        let cases = [
            (SurfaceOp::Append, r#"{"op":"append"}"#),
            (SurfaceOp::Replace { start: 3, end: 9 }, r#"{"op":"replace","start":3,"end":9}"#),
        ];
        for (op, want) in cases {
            let json = serde_json::to_string(&op).unwrap();
            assert_eq!(json, want);
            let back: SurfaceOp = serde_json::from_str(&json).unwrap();
            assert_eq!(back, op);
        }
    }

    #[test]
    fn replace_interval_validation() {
        // start > end 属于区间非法，由校验器/投影层拒绝（surface_violation）
        let bad = SurfaceOp::Replace { start: 9, end: 3 };
        assert!(bad.as_interval().is_none());
    }
}
