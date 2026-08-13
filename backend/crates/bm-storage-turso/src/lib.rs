//! BoenMind 存储后端（bm-storage-turso）。
//!
//! - [`event_log`]：EventStorePort 的 turso 实现（单写者 Mutex）；
//! - [`checkpoint`]：请求边界 fsync + 崩溃 interrupted 恢复；
//! - [`dual_write`]：阶段 0 双写过渡（bm-server chat 路由在现有落库
//!   的同时 append 事件流）。
//!
//! **seq 分配修正**（对实现方案 Schema 的一处实现期修正）：`seq` 列
//! 不用 AUTOINCREMENT（全局计数与"分支内连续"矛盾，且事务回滚会留
//! 空洞），改为应用层分配：读分支 head → head+1 → INSERT，唯一约束
//! (session_id, branch_id, seq) 兜底。

pub mod checkpoint;
pub mod dual_write;
pub mod event_log;

pub use checkpoint::{CheckpointStore, CheckpointState};
pub use event_log::TursoEventStore;
