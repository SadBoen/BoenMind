//! bm-persist:M2 持久层。事件日志(JSONL,追加写)是可重建的事实史,
//! SQLite 规范状态是快路径载体;二者互为完整性校验(M2 规格 §5.1)。
//! 写序纪律:**先日志后状态**——崩溃只会留下「日志有、状态未及」的单向窗口。

pub mod error;
pub mod event_log;
pub mod materialize;
pub mod recovery;
pub mod sqlite_state;
pub mod store;

pub use error::StoreError;
pub use event_log::JsonlEventLog;
pub use recovery::{
    RecoveryReport, WorldRows, dump_all, id_counter_hint, load_rows, pending_operations,
    rebuild_projection, repair_tail,
};
pub use sqlite_state::StateDb;
pub use store::{EventStore, META_LAST_APPLIED, META_SNAPSHOT_SEQ, PersistStore};
// 落盘小工具所有权在内核端口层(F-12);此处 re-export 保持旧路径。
pub use bm_core::ports::persist::{atomic_write, filter_lines_atomic};

/// 平台默认数据目录(`<data_dir>/boenmind`,无平台目录时回落到 `boenmind-data`)。
/// 运行时状态(事件日志/配置/密钥)的缺省根;`boenmind-server` 与 `bm-cli` 共用,
/// 。置于本 crate:两边都已依赖它,且属"数据目录"归属。
pub fn default_data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .map(|d| d.join("boenmind"))
        .unwrap_or_else(|| std::path::PathBuf::from("boenmind-data"))
}
