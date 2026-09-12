//! MCP HTTP/SSE 远程传输(自 mcp.rs 机械移出;ADR-0048)。

use super::*;

// ---- HTTP / SSE 远程传输 (Remote HTTP/SSE Transport) -----------------------

/// 远程 MCP HTTP/SSE 传输层(Streamable HTTP:单 POST + 可选 Bearer +
/// Mcp-Session-Id 会话管理 + SSE 响应流解析;issue #3 完整握手)。
pub struct HttpMcpTransport {
    url: String,
    bearer_token: Option<String>,
    client: reqwest::Client,
    progress_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>>>,
    /// W10(ADR-0024):单请求超时热读单元(缺省 60s)。
    limits: bm_core::limits::LimitsCell,
    /// #3:服务器下发的会话 id(Mcp-Session-Id 响应头),后续请求回带。
    session_id: Mutex<Option<String>>,
    /// #3:initialize 结果(协议版本/capabilities/serverInfo),管理面露出。
    init_result: Mutex<Option<Value>>,
}

impl HttpMcpTransport {
    pub fn new(url: &str, bearer_token: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            url: url.to_string(),
            bearer_token,
            client: reqwest::Client::new(),
            progress_rx: Mutex::new(None),
            limits: bm_core::limits::LimitsCell::with_default(),
            session_id: Mutex::new(None),
            init_result: Mutex::new(None),
        })
    }

    /// W10:注入共享 limits 单元(supervisor 装配用)。
    pub fn with_limits(self: Arc<Self>, limits: bm_core::limits::LimitsCell) -> Arc<Self> {
        let s = Arc::into_inner(self).expect("supervisor 装配期独占");
        let mut s = s;
        s.limits = limits;
        Arc::new(s)
    }
}

/// 从 SSE 文本(text/event-stream)提取 id 匹配的 JSON-RPC 响应:以空行分块,
/// `data:` 行拼合为一条 JSON(#3;POST 作用域的响应流,服务器发完即关)。
fn parse_sse_response(body: &str, id: u64) -> Result<Value, String> {
    for block in body.split(
        "

",
    ) {
        let data: Vec<&str> = block
            .lines()
            .filter(|l| l.starts_with("data:"))
            .map(|l| l.trim_start_matches("data:").trim_start_matches(' '))
            .collect();
        if data.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&data.join(
            "
",
        )) else {
            continue;
        };
        if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
            return Ok(v);
        }
    }
    Err("SSE 流中未找到匹配 id 的响应".to_string())
}

impl HttpMcpTransport {
    /// request/notify 共用的 POST 组装:bearer + Accept 双类型(spec 要求,
    /// 服务端可回 SSE 流)+ 会话头回带(initialize 后服务器下发 Mcp-Session-Id)。
    fn post_rpc(&self, msg: Value) -> reqwest::RequestBuilder {
        let mut req = self.client.post(&self.url).json(&msg);
        if let Some(token) = &self.bearer_token {
            req = req.bearer_auth(token);
        }
        req = req.header("Accept", "application/json, text/event-stream");
        if let Some(sid) = self
            .session_id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            req = req.header("Mcp-Session-Id", sid);
        }
        req
    }
}

#[async_trait]
impl McpTransport for HttpMcpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let msg = rpc::request(1, method, params);
        let req = self.post_rpc(msg);
        // R3(FULL-REVIEW-2026-09-05 §7):裸 send 无超时 = 远端挂起即调用
        // 悬挂;默认 60s 硬顶(W10 走 limits),远端长任务应自行异步化。
        let remote_timeout =
            std::time::Duration::from_millis(self.limits.get().mcp_remote_timeout_ms);
        let resp = tokio::time::timeout(remote_timeout, req.send())
            .await
            // P2:秒数随 limits 热值,不再写死 60。
            .map_err(|_| {
                format!(
                    "远程 MCP 请求超时({}ms)",
                    self.limits.get().mcp_remote_timeout_ms
                )
            })?
            .map_err(|e| format!("远程 MCP 请求失败: {e}"))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND
            && self
                .session_id
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
        {
            return Err("远程 MCP 会话已失效(HTTP 404),请重载该 server".to_string());
        }
        if !resp.status().is_success() {
            return Err(format!("远程 MCP HTTP 状态异常: {}", resp.status()));
        }
        // #3:会话头记录(initialize 响应下发;后续请求回带)
        if let Some(sid) = resp
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
        {
            *self
                .session_id
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sid.to_string());
        }
        let is_sse = resp
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.contains("text/event-stream"));
        let val: Value = if is_sse {
            let body = resp
                .text()
                .await
                .map_err(|e| format!("远程 MCP SSE 流读取失败: {e}"))?;
            parse_sse_response(&body, 1)?
        } else {
            resp.json()
                .await
                .map_err(|e| format!("远程 MCP 响应解析失败: {e}"))?
        };
        if val.get("error").is_some() {
            let code = rpc::error_code(&val).unwrap_or(-1);
            let msg = rpc::error_message(&val).unwrap_or("");
            return Err(format!("rpc-error:{code} {msg}"));
        }
        Ok(rpc::take_result(&val))
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let msg = rpc::notification(method, params);
        let req = self.post_rpc(msg);
        // P1-9: notify 加上 10s 超时,防止半开远端挂死卸载/重载/关闭请求
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), req.send()).await;
        Ok(())
    }

    fn subscribe_progress(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
        take_progress_rx(&self.progress_rx)
    }

    fn remember_init(&self, v: Value) {
        *self
            .init_result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(v);
    }

    fn init_snapshot(&self) -> Option<Value> {
        self.init_result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
mod remote_http_tests {
    use super::*;
    use axum::extract::Request;
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;

    /// 起一个极简 Streamable HTTP MCP mock:initialize 下发会话头,
    /// tools/list 校验会话头,tools/call 以 SSE 流回响应。
    async fn spawn_mock(unknown_version: bool) -> String {
        let app = axum::Router::new().route(
            "/mcp",
            axum::routing::post(move |headers: HeaderMap, req: Request| async move {
                let bytes = axum::body::to_bytes(req.into_body(), 1 << 20)
                    .await
                    .unwrap();
                let msg: Value = serde_json::from_slice(&bytes).unwrap();
                let id = msg.get("id").and_then(|v| v.as_u64());
                let method = msg["method"].as_str().unwrap_or("");
                let session = headers
                    .get("Mcp-Session-Id")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let result = match method {
                    "initialize" => {
                        let version = if unknown_version {
                            "1999-01-01"
                        } else {
                            "2025-03-26"
                        };
                        json!({
                            "protocolVersion": version,
                            "capabilities": {"tools": {"listChanged": false}},
                            "serverInfo": {"name": "mock-remote", "version": "0.1"}
                        })
                    }
                    "tools/list" => {
                        assert_eq!(session.as_deref(), Some("mock-session-1"), "tools/list 必须回带会话头");
                        json!({"tools": [{"name": "echo", "inputSchema": {"type": "object"}}]})
                    }
                    "tools/call" => {
                        assert_eq!(session.as_deref(), Some("mock-session-1"));
                        return (
                            axum::http::StatusCode::OK,
                            [("Content-Type", "text/event-stream"), ("Mcp-Session-Id", "mock-session-1")],
                            format!(
                                "event: message
data: {{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":\"sse-pong\"}}]}}}}

",
                                id.unwrap_or(1)
                            ),
                        )
                            .into_response();
                    }
                    _ => json!({}),
                };
                let body = json!({"jsonrpc": "2.0", "id": id, "result": result});
                (
                    axum::http::StatusCode::OK,
                    [("Mcp-Session-Id", "mock-session-1")],
                    axum::Json(body),
                )
                    .into_response()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/mcp")
    }

    async fn handshake(url: &str, unknown_version: bool) -> Arc<McpHub> {
        let transport = HttpMcpTransport::new(url, None);
        let hub = McpHub::new();
        let manifests = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            hub.connect("remote", transport, 5_000),
        )
        .await
        .expect("握手超时")
        .expect("握手成功");
        assert_eq!(manifests.len(), 1, "tools/list 应发现 1 工具");
        let caps = hub.server_capabilities("remote").expect("握手记录存在");
        assert_eq!(caps["serverInfo"]["name"], "mock-remote");
        let _ = unknown_version;
        hub
    }

    #[tokio::test]
    async fn t_remote_handshake_session_and_capabilities() {
        let url = spawn_mock(false).await;
        let hub = handshake(&url, false).await;
        let caps = hub.server_capabilities("remote").unwrap();
        assert_eq!(caps["protocolVersion"], "2025-03-26", "协商记录响应版本");
        assert_eq!(caps["capabilities"]["tools"]["listChanged"], false);
        // tools/call 全链路:SSE 响应流解析 + 会话头回带
        let out = bm_core::ports::AsyncCapabilityExecutor::call(
            hub.as_ref(),
            "op_remote",
            "mcp.remote.echo",
            json!({"text": "x"}),
            std::time::Duration::from_secs(5),
        )
        .await
        .expect("远程调用成功");
        assert!(out.to_string().contains("sse-pong"), "{out}");
    }

    #[tokio::test]
    async fn t_remote_unknown_protocol_version_tolerated() {
        let url = spawn_mock(true).await;
        handshake(&url, true).await;
    }
}
