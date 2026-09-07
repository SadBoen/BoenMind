//! 持久层错误(所有权已收归 bm_core::ports::persist;此路径 re-export 兼容)。
pub use bm_core::ports::persist::{StoreError, StoreResult};

/// rusqlite::Error → StoreError 的边界转换(F-12 依赖倒置收口):内核端口层
/// 不再 `#[from] rusqlite::Error`,孤儿规则又不允许在本 crate 为外族
/// StoreError 实现 From,故以扩展 trait `.sql()` 统一收口全部转换点。
pub(crate) trait SqlResultExt<T> {
    fn sql(self) -> StoreResult<T>;
}

impl<T> SqlResultExt<T> for Result<T, rusqlite::Error> {
    fn sql(self) -> StoreResult<T> {
        self.map_err(|e| StoreError::Sql(e.to_string()))
    }
}
