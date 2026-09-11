//! BoenMind 插件协议最小 SDK(ADR-0034,issue #56)。
//!
//! 统一 MCP 2024-11-05 / JSON-RPC 2.0 over stdio 的协议面:逐行帧、通知语义、
//! 错误码与 `--self-describe` 声明。插件只需实现 [`McpService`](工具表 + 调用)
//! 并调用 [`run_stdio`],协议不变量由本 crate 单处守门。
//!
//! 边界:本 SDK 只收协议,不收业务与宿主。宿主客户端的传输(stdio/HTTP/SSE)、
//! 重试与能力注册不在本 SDK 范围;其**信封构造与响应解析**共用 [`client`]
//! 模块(#60),使请求方与响应方的协议常量单源。

use std::future::Future;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub mod client;

/// JSON-RPC 版本号。
pub const JSONRPC_VERSION: &str = "2.0";
/// MCP 协议版本(握手 `protocolVersion`)。
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC 2.0 标准错误码。
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// 握手 `serverInfo`。`extra` 承载插件自定义附加字段(如 market 的数据版本)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl ServerInfo {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            extra: serde_json::Map::new(),
        }
    }

    /// 追加一个自定义字段。
    pub fn with_extra(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }
}

/// `tools/list` 的单个工具定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}

impl ToolDef {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            annotations: None,
        }
    }

    /// 标注 MCP `annotations`(如 `readOnlyHint`)。
    pub fn with_annotations(mut self, annotations: Value) -> Self {
        self.annotations = Some(annotations);
        self
    }

    /// 只读工具快捷构造:`annotations.readOnlyHint = true`。
    pub fn read_only(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self::new(name, description, input_schema).with_annotations(json!({"readOnlyHint": true}))
    }
}

/// `tools/call` 的返回:结构化成功(应用层)或工具执行错误(`isError:true`)。
#[derive(Debug, Clone)]
pub enum ToolOutput {
    /// 成功:序列化为 `content[].text`(pretty JSON)+ `structuredContent` + `isError:false`。
    Structured(Value),
    /// 工具执行失败(MCP 应用层错误,`isError:true`)。
    Error(String),
}

impl ToolOutput {
    /// 展开为 MCP `tools/call` 的 `result` 负载。
    pub fn into_result(self) -> Value {
        match self {
            ToolOutput::Structured(data) => json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&data).unwrap_or_default(),
                }],
                "structuredContent": data,
                "isError": false,
            }),
            ToolOutput::Error(message) => json!({
                "content": [{"type": "text", "text": message}],
                "isError": true,
            }),
        }
    }
}

/// 插件协议面实现:工具表 + 调用处理 + 可选自定义方法。
///
/// 协议循环、握手、错误码、通知语义由 [`run_stdio`] 统一处理,实现方不必关心。
pub trait McpService: Send + Sync {
    /// 握手返回的 `serverInfo`。**`name` 须与 `--self-describe` 声明名一致**。
    fn server_info(&self) -> ServerInfo;

    /// `tools/list` 返回的工具表;其 `name` 集合同时决定未知工具的 `-32602` 判定。
    fn tools(&self) -> Vec<ToolDef>;

    /// 调用一个已知工具(仅当 `name` 在 [`McpService::tools`] 内时才被调用)。
    fn call_tool(&self, name: &str, args: Value) -> impl Future<Output = ToolOutput> + Send;

    /// 处理非 MCP 标准的自定义方法;返回 `None` 表示方法不存在(→ `-32601`)。
    fn call_custom(
        &self,
        method: &str,
        params: Value,
    ) -> impl Future<Output = Option<Value>> + Send;
}

/// `--self-describe` 声明(合同 `mcp-self-describe.v0_1`)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescribe {
    pub name: String,
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<Vec<Value>>,
    pub suggested_entry: SuggestedEntry,
}

impl SelfDescribe {
    pub fn new(
        name: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
        suggested_entry: SuggestedEntry,
    ) -> Self {
        Self {
            name: name.into(),
            title: title.into(),
            description: description.into(),
            config_schema: None,
            suggested_entry,
        }
    }

    pub fn with_config_schema(mut self, config_schema: Vec<Value>) -> Self {
        self.config_schema = Some(config_schema);
        self
    }
}

/// `--self-describe` 的建议接入条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedEntry {
    pub transport: String,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_limit: Option<u32>,
}

impl SuggestedEntry {
    /// stdio 接入(自描述发现面当前唯一传输)。
    pub fn stdio(args: Vec<String>) -> Self {
        Self {
            transport: "stdio".to_string(),
            args,
            tool_timeout_ms: None,
            restart_limit: None,
        }
    }

    pub fn with_tool_timeout_ms(mut self, ms: u64) -> Self {
        self.tool_timeout_ms = Some(ms);
        self
    }

    pub fn with_restart_limit(mut self, limit: u32) -> Self {
        self.restart_limit = Some(limit);
        self
    }
}

/// 若命令行含 `--self-describe`,打印声明并退出(0);否则正常返回。
///
/// 须在解析自身参数、建立运行时之前调用。
pub fn emit_self_describe_if_requested(decl: &SelfDescribe) {
    if std::env::args().any(|a| a == "--self-describe") {
        let text = serde_json::to_string(decl).expect("self-describe 序列化失败");
        println!("{text}");
        std::process::exit(0);
    }
}

/// 运行 stdio 协议循环直至 EOF。
///
/// 集中处理:坏 JSON → `-32700`;通知(id 缺省/null)不回复;`initialize`/`ping`/
/// `tools/list`/`tools/call` 分发;未知工具 → `-32602`;未知方法 → `-32601`;
/// `shutdown`/`exit` → `result:null` 后退出。
pub async fn run_stdio<S: McpService>(service: &S) {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[plugin-sdk] 无法解析的输入行:{e}");
                let err = error_response(&Value::Null, PARSE_ERROR, "JSON 解析失败");
                if write_line(&mut stdout, &err).await.is_err() {
                    break;
                }
                continue;
            }
        };

        let id = match msg.get("id") {
            Some(Value::Null) | None => continue, // 通知:无应答
            Some(other) => other.clone(),
        };
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        // shutdown/exit:MCP 生命周期方法,回 result:null 后收尾
        if method == "shutdown" || method == "exit" {
            let ok = json!({"jsonrpc": JSONRPC_VERSION, "id": id, "result": Value::Null});
            let _ = write_line(&mut stdout, &ok).await;
            break;
        }

        let response = dispatch(service, &id, &method, params).await;
        if write_line(&mut stdout, &response).await.is_err() {
            break;
        }
    }
}

async fn dispatch<S: McpService>(service: &S, id: &Value, method: &str, params: Value) -> Value {
    match method {
        "initialize" => ok_response(
            id,
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": service.server_info(),
            }),
        ),
        "ping" => ok_response(id, json!({})),
        "tools/list" => ok_response(id, json!({"tools": service.tools()})),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !service.tools().iter().any(|t| t.name == name) {
                return error_response(id, INVALID_PARAMS, &format!("未知工具:{name}"));
            }
            ok_response(id, service.call_tool(&name, args).await.into_result())
        }
        other => match service.call_custom(other, params).await {
            Some(result) => ok_response(id, result),
            None => error_response(id, METHOD_NOT_FOUND, &format!("方法不存在:{other}")),
        },
    }
}

fn ok_response(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": JSONRPC_VERSION, "id": id, "result": result})
}

fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "error": {"code": code, "message": message},
    })
}

async fn write_line(stdout: &mut tokio::io::Stdout, value: &Value) -> std::io::Result<()> {
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    text.push('\n');
    stdout.write_all(text.as_bytes()).await?;
    stdout.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        fail: bool,
    }

    impl McpService for Fake {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("fake_plugin", "1.2.3")
        }

        fn tools(&self) -> Vec<ToolDef> {
            vec![
                ToolDef::read_only("echo", "回显", json!({"type": "object"})),
                ToolDef::new("boom", "失败", json!({"type": "object"})),
            ]
        }

        fn call_tool(&self, name: &str, args: Value) -> impl Future<Output = ToolOutput> + Send {
            let fail = self.fail && name == "boom";
            let args = args.clone();
            async move {
                if fail {
                    ToolOutput::Error("工具执行失败".to_string())
                } else {
                    ToolOutput::Structured(args)
                }
            }
        }

        fn call_custom(
            &self,
            method: &str,
            _params: Value,
        ) -> impl Future<Output = Option<Value>> + Send {
            let known = method == "plugin_status";
            async move { known.then(|| json!({"ok": true})) }
        }
    }

    async fn roundtrip(svc: &Fake, msg: Value) -> Value {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg["method"].as_str().unwrap().to_string();
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        dispatch(svc, &id, &method, params).await
    }

    #[tokio::test]
    async fn initialize_reports_version_and_server_info() {
        let svc = Fake { fail: false };
        let resp = roundtrip(&svc, json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).await;
        assert_eq!(resp["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(resp["result"]["serverInfo"]["name"], "fake_plugin");
        assert_eq!(resp["result"]["capabilities"]["tools"], json!({}));
    }

    #[tokio::test]
    async fn tools_list_serializes_input_schema_and_annotations() {
        let svc = Fake { fail: false };
        let resp = roundtrip(&svc, json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await;
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "echo");
        assert!(tools[0]["inputSchema"].is_object());
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
    }

    #[tokio::test]
    async fn tool_call_success_wraps_structured_content() {
        let svc = Fake { fail: false };
        let resp = roundtrip(
            &svc,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
                   "params":{"name":"echo","arguments":{"v":7}}}),
        )
        .await;
        assert_eq!(resp["result"]["isError"], false);
        assert_eq!(resp["result"]["structuredContent"]["v"], 7);
        assert!(resp["result"]["content"][0]["text"].is_string());
    }

    #[tokio::test]
    async fn tool_execution_failure_is_application_error() {
        let svc = Fake { fail: true };
        let resp = roundtrip(
            &svc,
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
                   "params":{"name":"boom","arguments":{}}}),
        )
        .await;
        assert_eq!(resp["result"]["isError"], true);
    }

    #[tokio::test]
    async fn unknown_tool_is_invalid_params() {
        let svc = Fake { fail: false };
        let resp = roundtrip(
            &svc,
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call",
                   "params":{"name":"nope","arguments":{}}}),
        )
        .await;
        assert_eq!(resp["error"]["code"], INVALID_PARAMS);
    }

    #[tokio::test]
    async fn unknown_method_is_method_not_found() {
        let svc = Fake { fail: false };
        let resp = roundtrip(
            &svc,
            json!({"jsonrpc":"2.0","id":6,"method":"resources/list"}),
        )
        .await;
        assert_eq!(resp["error"]["code"], METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn custom_method_is_routed() {
        let svc = Fake { fail: false };
        let resp = roundtrip(
            &svc,
            json!({"jsonrpc":"2.0","id":7,"method":"plugin_status"}),
        )
        .await;
        assert_eq!(resp["result"]["ok"], true);
    }

    #[test]
    fn self_describe_serializes_to_contract_shape() {
        let decl = SelfDescribe::new(
            "fake_plugin",
            "假插件",
            "协议单测用",
            SuggestedEntry::stdio(vec!["--config".into(), "{config_file}".into()])
                .with_tool_timeout_ms(15_000)
                .with_restart_limit(3),
        )
        .with_config_schema(vec![json!({"key": "x", "label": "X", "type": "string"})]);
        let v: Value = serde_json::to_value(&decl).unwrap();
        assert_eq!(v["name"], "fake_plugin");
        assert_eq!(v["suggested_entry"]["transport"], "stdio");
        assert_eq!(v["suggested_entry"]["args"][1], "{config_file}");
        assert_eq!(v["suggested_entry"]["tool_timeout_ms"], 15_000);
        assert_eq!(v["config_schema"][0]["key"], "x");
        // 未设 config_schema 时不应出现该键(合同可选)
        let bare = serde_json::to_value(SelfDescribe::new(
            "p",
            "t",
            "d",
            SuggestedEntry::stdio(vec![]),
        ))
        .unwrap();
        assert!(bare.get("config_schema").is_none());
    }

    #[test]
    fn error_response_shape() {
        let v = error_response(&Value::Null, PARSE_ERROR, "bad");
        assert_eq!(v["jsonrpc"], JSONRPC_VERSION);
        assert_eq!(v["id"], Value::Null);
        assert_eq!(v["error"]["code"], PARSE_ERROR);
    }
}
