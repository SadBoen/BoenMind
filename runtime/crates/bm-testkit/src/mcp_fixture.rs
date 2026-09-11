//! 进程内 MCP server 测试替身(2026-09-12 自 bm-providers/src/mcp.rs 迁出:
//! 测试替身不编译进生产库,与 TestRig/replay 同居测试装配层)。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bm_providers::mcp::{MCP_PROTOCOL_VERSION, McpProgressNote, McpToolDef, McpTransport};
use serde_json::{Value, json};

fn dead_progress_rx() -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
    let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
    rx
}

/// 工具行为脚本(InProc 测试)。
#[derive(Clone, Default)]
pub struct Behavior {
    /// Ok = CallResult(可为 {"isError": true} 形态模拟工具级失败);
    /// Err = JSON-RPC 层错误(传输/协议故障)。
    pub result: Option<Result<Value, String>>,
    /// 真实睡眠(制造进行中窗口;超时测试用)。
    pub delay_ms: u64,
    /// 调用前发送的进度步 (progress, total, message)。
    pub progress: Vec<(u64, Option<u64>, String)>,
}

impl Behavior {
    pub fn done(result: Value) -> Self {
        Self {
            result: Some(Ok(result)),
            ..Default::default()
        }
    }
}

/// 进程内 MCP server 测试替身。进度订阅:单消费者(先到先得)。
pub struct InProcMcpServer {
    tools: Mutex<Vec<McpToolDef>>,
    behaviors: Mutex<HashMap<String, Behavior>>,
    /// tools/call 次数(健康面测试:断言封禁后不再触达执行器)。
    calls: Mutex<HashMap<String, u32>>,
    /// 进度令牌 → 取消标志(M8.3:语义取消的 InProc 贯穿)。
    cancel_flags: Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
    progress_tx: tokio::sync::mpsc::UnboundedSender<McpProgressNote>,
    progress_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<McpProgressNote>>>,
}

impl InProcMcpServer {
    pub fn new(tools: Vec<McpToolDef>) -> Arc<Self> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(Self {
            tools: Mutex::new(tools),
            behaviors: Mutex::new(HashMap::new()),
            calls: Mutex::new(HashMap::new()),
            cancel_flags: Mutex::new(HashMap::new()),
            progress_tx: tx,
            progress_rx: Mutex::new(Some(rx)),
        })
    }

    pub fn set_behavior(&self, tool: &str, behavior: Behavior) {
        self.behaviors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(tool.to_string(), behavior);
    }

    /// 某 tool 的 tools/call 次数(测试断言面)。
    pub fn call_count(&self, tool: &str) -> u32 {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(tool)
            .copied()
            .unwrap_or(0)
    }
}

#[async_trait]
impl McpTransport for InProcMcpServer {
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "inproc", "version": "0.0.1"}
            })),
            "tools/list" => {
                let tools = self
                    .tools
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                Ok(json!({"tools": tools.iter().map(|t| json!({
                    "name": t.name,
                    "inputSchema": t.input_schema,
                    "annotations": t.annotations,
                })).collect::<Vec<_>>()}))
            }
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let token = params
                    .get("_meta")
                    .and_then(|m| m.get("progressToken"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                *self
                    .calls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .entry(name.clone())
                    .or_insert(0) += 1;
                if !token.is_empty() {
                    self.cancel_flags
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .entry(token.clone())
                        .or_insert_with(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));
                }
                let behavior = self
                    .behaviors
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&name)
                    .cloned()
                    .unwrap_or_default();
                for (p, total, msg) in &behavior.progress {
                    let _ = self.progress_tx.send(McpProgressNote {
                        progress_token: token.clone(),
                        progress: *p,
                        total: *total,
                        message: Some(msg.clone()),
                    });
                }
                if behavior.delay_ms > 0 {
                    let flag = self
                        .cancel_flags
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .get(&token)
                        .cloned();
                    let stop_at =
                        tokio::time::Instant::now() + Duration::from_millis(behavior.delay_ms);
                    tokio::select! {
                        _ = tokio::time::sleep_until(stop_at) => {}
                        _ = async {
                            if let Some(f) = flag {
                                while !f.load(std::sync::atomic::Ordering::Relaxed) {
                                    tokio::time::sleep(Duration::from_millis(5)).await;
                                }
                            } else {
                                std::future::pending::<()>().await;
                            }
                        } => {}
                    }
                }
                match behavior.result {
                    Some(Ok(v)) => Ok(v),
                    Some(Err(e)) => Err(e),
                    None => Ok(json!({"content": [{"type": "text", "text": "ok"}]})),
                }
            }
            other => Err(format!("inproc 不支持方法 {other}")),
        }
    }

    async fn notify(&self, _method: &str, _params: Value) -> Result<(), String> {
        Ok(())
    }

    fn subscribe_progress(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpProgressNote> {
        self.progress_rx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .unwrap_or_else(dead_progress_rx)
    }

    fn cancel_by_token(&self, token: &str) {
        if let Some(f) = self
            .cancel_flags
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(token)
        {
            f.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}
