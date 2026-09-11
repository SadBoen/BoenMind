//! bm-testkit:黄金轨迹回放器、INV 断言与测试装配。不进生产二进制。

pub mod invariants;
pub mod mcp_fixture;
pub mod memory_fixtures;
pub mod replay;

pub use mcp_fixture::{Behavior, InProcMcpServer};
pub use replay::{Expected, PVal, TestRig, rig, wait_terminal_handle};
