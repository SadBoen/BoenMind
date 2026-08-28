//! bm-testkit:黄金轨迹回放器、INV 断言与测试装配。不进生产二进制。

pub mod invariants;
pub mod replay;

pub use replay::{Expected, PVal, TestRig, rig};
