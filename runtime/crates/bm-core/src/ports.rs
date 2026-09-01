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

    /// 流式调用(M9-S2):增量经 `on_delta` 逐块回调;默认实现退化为
    /// 非流式 `invoke`(整段作为单个 delta 回调)——既有连接器零改动兼容,
    /// 真 SSE 由连接器按需覆写。返回值与非流式同一合同(聚合全量)。
    async fn invoke_stream(
        &self,
        req: InvokeRequest,
        cancel: CancellationToken,
        mut on_delta: Box<dyn for<'a> FnMut(&'a str) + Send + 'static>,
    ) -> InvokeResponse {
        let r = self.invoke(req, cancel).await;
        if let InvokeResponse::Completed { content, .. } = &r
            && !content.is_empty()
        {
            (on_delta)(content.as_str());
        }
        drop(on_delta); // 先丢回调(析构借用)再归还结果
        r
    }

    /// 连接器实现标识(model descriptor 的 provider 字段)。
    fn provider(&self) -> &'static str;
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret 不存在: {0}")]
    NotFound(String),
    /// A-13(审计台账):写路径 fail-fast——引用须过合同字符集
    /// (`connector::validate_secret_ref`,`^secret:[A-Za-z0-9_.-]{1,64}$`)。
    #[error("非法 secret_ref: {0}")]
    InvalidRef(String),
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
/// 外部慢路径执行器端口(异步;MCP hub 实现)。与 `registry::CapabilityProvider`
/// (进程内同步快路径)共用同一条 Broker 决策管线,仅执行步分道:`is_async()`
/// 为真的能力由运行期 spawn 走本端口,超时按 manifest.timeout_ms 钳制、可取消,
/// 完成经 Cmd::ProviderCall 回单写者回路落定(turn.rs M7 S4/S5)。
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

    /// 语义取消的传输层贯彻(尽力终止;M8.3)。缺省无操作。
    fn cancel_op(&self, _operation_id: &str) {}
}
