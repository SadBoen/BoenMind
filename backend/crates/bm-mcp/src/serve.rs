//! 反向 MCP server（吸收 Claude Code `claude mcp serve` 模式）：把宿主
//! 工具面暴露成 MCP server（stdio），供外部 MCP client（Claude Code /
//! Claude Desktop / Cursor 等）经 `mcpServers` 配置接入调用。
//!
//! 用法：`bm-server --mcp-serve`——spawn 为子进程即成为 MCP server。
//! 权限治理：反向暴露的工具由**外部客户端**的权限系统治理（与
//! subagent 子进程模式同理）；BoenMind 侧不重复设闸。

use std::sync::Arc;

use bm_protocol::BoxFuture;
use rmcp::handler::server::router::tool::ToolRoute;
use rmcp::handler::server::router::Router;
use rmcp::model::{CallToolResult, ContentBlock, ErrorCode, ErrorData, Tool};
use rmcp::transport::stdio;
use rmcp::{ServerHandler, serve_server};

/// 反向 server 的宿主 handler（工具全部经 ToolRouter 注册，无自定义
/// server→client 请求需要处理，空实现即可）。
struct BoenServeHandler;

impl ServerHandler for BoenServeHandler {}

/// 反向暴露的一个工具。
pub struct McpServeTool {
    pub name: String,
    pub description: String,
    /// 输入 JSON Schema（模型可见契约）。
    pub input_schema: serde_json::Value,
    /// 执行器：参数（JSON 对象）→ 结果（JSON 值）或错误文案。
    pub execute: Arc<
        dyn Fn(serde_json::Value) -> BoxFuture<'static, Result<serde_json::Value, String>>
            + Send
            + Sync,
    >,
}

/// 从宿主工具结果里提取模型可读文本；同时判断是否 text_output 包装
/// （`{content:[{type:"text",text}]}`）——是则解包为纯文本，不设
/// structured_content（client 端 structured 优先，会吞掉文本）。
fn extract_text(value: &serde_json::Value) -> (Option<String>, bool) {
    let wrapper = value
        .get("content")
        .and_then(serde_json::Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|b| b.get("text"))
        .and_then(serde_json::Value::as_str);
    match wrapper {
        Some(text) => (Some(text.to_string()), true),
        None => (
            Some(serde_json::to_string(value).unwrap_or_default()),
            false,
        ),
    }
}

/// 以 stdio MCP server 身份运行，直到外部 client 断开或进程被终止。
pub async fn serve_stdio(tools: Vec<McpServeTool>) -> Result<(), String> {
    let mut router = Router::new(BoenServeHandler);
    for tool in tools {
        let name = tool.name.clone();
        let execute = tool.execute.clone();
        let schema: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(tool.input_schema).unwrap_or_default();
        let attr = Tool::new(name.clone(), tool.description, Arc::new(schema));
        router = router.with_tool(ToolRoute::new_dyn(attr, move |ctx| {
            let execute = execute.clone();
            let tool_name = name.clone();
            Box::pin(async move {
                let args = ctx
                    .arguments
                    .map(serde_json::Value::Object)
                    .unwrap_or_else(|| serde_json::json!({}));
                match execute(args).await {
                    Ok(value) => {
                        let (text, is_wrapper) = extract_text(&value);
                        let mut result =
                            CallToolResult::success(vec![ContentBlock::text(text.unwrap_or_default())]);
                        if !is_wrapper {
                            // 结构化结果（如 bash 的 {stdout,stderr,code,killed}）原样保留
                            result.structured_content = Some(value);
                        }
                        Ok(result.into())
                    }
                    Err(err) => Err(ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("工具 `{tool_name}` 执行失败: {err}"),
                        None,
                    )),
                }
            })
        }));
    }

    let (stdin, stdout) = stdio();
    // RunningService 的 DropGuard 在 drop 时取消服务任务——必须持有并
    // 等待，否则 discover 响应后任务即被取消（后续请求无人应答）。
    let running = serve_server(router, (stdin, stdout))
        .await
        .map_err(|e| format!("反向 MCP server 启动失败: {e}"))?;
    running
        .waiting()
        .await
        .map_err(|e| format!("反向 MCP server 运行异常: {e}"))?;
    Ok(())
}
