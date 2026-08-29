//! 持久层错误。IO/SQL 故障一律包装为致命类(调用方须拒绝命令,不可静默)。

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("事件日志 IO 失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite 失败: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("事件日志损坏于 seq {seq}: {reason}")]
    Corrupt { seq: u64, reason: String },
    #[error("CAS 不匹配: key={key} expect={expect}")]
    CasMismatch { key: String, expect: String },
    #[error("目录未初始化: {0}")]
    NotOpen(String),
}

pub type StoreResult<T> = Result<T, StoreError>;
