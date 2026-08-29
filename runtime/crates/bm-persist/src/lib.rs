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
    RecoveryReport, WorldRows, dump_all, load_rows, pending_operations, rebuild_projection,
    repair_tail,
};
pub use sqlite_state::StateDb;
pub use store::{EventStore, META_LAST_APPLIED, META_SNAPSHOT_SEQ, PersistStore};
