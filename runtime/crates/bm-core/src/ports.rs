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

// ---- M7.2/M7.5:异步能力执行器端口(MCP 等慢外部 Provider)------------------

/// 异步调用失败类别(内容脱敏:Transport 只携带类别描述,不携带原始报文)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsyncCallError {
    /// deadline 到点(调用未确认完成;副作用对账留给 outbox 语义)。
    Timeout,
    /// 传输/子进程故障(可重试)。
    Transport(String),
    /// 工具明确报告失败(MCP isError=true;不携带工具输出)。
    ToolError,
}

/// 进度通知(单写者回路外产生,经 sink 回注为 capability.progress 事件)。
#[derive(Debug, Clone)]
pub struct ProgressNotice {
    pub operation_id: String,
    pub progress: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
}

/// 异步能力执行器端口。实现方(如 MCP Hub)自行路由 capability → 目标,
/// `operation_id` 兼作进度令牌(MCP progressToken)。
#[async_trait]
pub trait AsyncCapabilityExecutor: Send + Sync {
    async fn call(
        &self,
        operation_id: &str,
        capability: &str,
        args: serde_json::Value,
        deadline: std::time::Duration,
    ) -> Result<serde_json::Value, AsyncCallError>;

    /// 装配期注入进度回注通道(缺省无进度支持,静默忽略)。
    fn set_progress_sink(&self, _sink: Box<dyn Fn(ProgressNotice) + Send + Sync>) {}
}
