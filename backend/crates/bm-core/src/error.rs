//! 领域层统一错误：按来源分类，路由层据此映射 HTTP 状态码。
//!
//! 业务代码在错误产生点标注分类（`AppError::invalid/upstream/internal`），
//! 路由层用 `api_error_from` 集中映射，不再手工逐个选状态码：
//! - `Invalid` → 400（用户输入/业务规则不合法）
//! - `Upstream` → 502（网络 / 外部服务失败）
//! - `Internal` → 500（本地 IO / 内部状态错误）

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 用户输入或业务规则不合法（非法 id、已存在、未配置、超出范围…）
    #[error("{0}")]
    Invalid(String),
    /// 外部服务失败（下载、HTTP 状态、上游响应异常…）
    #[error("{0}")]
    Upstream(String),
    /// 本地 IO / 内部状态错误
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
    pub fn upstream(msg: impl Into<String>) -> Self {
        Self::Upstream(msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::Internal(err.to_string())
    }
}
