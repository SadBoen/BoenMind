//! 可替换端口:模型连接器与 Secret Store(基线 5.4:内核不持有 Provider 特权)。

use async_trait::async_trait;
use bm_contract::connector::InvokeRequest;
use bm_contract::connector::InvokeResponse;
use tokio_util::sync::CancellationToken;

/// 模型连接器端口。实现方自行从 Secret Store 解析 `req.secret_ref`;
/// 凭据明文不得进入事件/日志/错误(INV-5,基线 4.6)。
#[async_trait]
pub trait ModelConnector: Send + Sync {
    async fn invoke(&self, req: InvokeRequest, cancel: CancellationToken) -> InvokeResponse;

    /// 连接器实现标识(model descriptor 的 provider 字段)。
    fn provider(&self) -> &'static str;
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret 不存在: {0}")]
    NotFound(String),
    #[error("secret 后端故障")]
    Backend(String),
}

/// Secret Store 端口:凭据本体唯一存放地(基线 4.6)。
/// `expose_for_scan` 服务于 INV-5 泄漏扫描:返回本进程已知/经手的凭据明文。
pub trait SecretStore: Send + Sync {
    fn get(&self, secret_ref: &str) -> Result<String, SecretError>;
    fn put(&self, secret_ref: &str, value: &str) -> Result<(), SecretError>;
    fn delete(&self, secret_ref: &str) -> Result<(), SecretError>;
    fn expose_for_scan(&self) -> Vec<String>;
}
