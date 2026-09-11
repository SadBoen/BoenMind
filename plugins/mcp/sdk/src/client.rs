//! 客户端(请求方)协议面:JSON-RPC 2.0 信封构造与响应解析(#60,ADR-0034 §4)。
//!
//! 与 [`run_stdio`](crate::run_stdio) 的服务端面同属协议层——此处的常量与
//! 信封形状是**同一真源**,宿主客户端与插件服务端据此不致漂移。本模块只收
//! 协议,不含任何传输(stdio/HTTP/SSE)、重试或业务语义(那些在宿主)。

use serde_json::{json, Value};

use crate::{JSONRPC_VERSION, MCP_PROTOCOL_VERSION};

/// 构造请求信封 `{jsonrpc,id,method,params}`。
pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "method": method,
        "params": params,
    })
}

/// 构造通知信封 `{jsonrpc,method,params}`(JSON-RPC 通知无 `id`)。
pub fn notification(method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "method": method,
        "params": params,
    })
}

/// 构造 `initialize` 的 params(协议版本 + 客户端身份)。
pub fn initialize_params(client_name: &str, client_version: &str) -> Value {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {"name": client_name, "version": client_version},
    })
}

/// 响应中的 JSON-RPC 错误码;无 `error` 字段时为 `None`。
pub fn error_code(response: &Value) -> Option<i64> {
    response
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(Value::as_i64)
}

/// 响应中的 JSON-RPC 错误消息;无 `error` 字段时为 `None`。
pub fn error_message(response: &Value) -> Option<&str> {
    response
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
}

/// 取响应的 `result`;缺失时返回空对象 `{}`(与宿主既有语义一致)。
pub fn take_result(response: &Value) -> Value {
    response.get("result").cloned().unwrap_or_else(|| json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::INVALID_PARAMS;

    #[test]
    fn request_and_notification_shapes() {
        let req = request(7, "tools/call", json!({"name": "ping"}));
        assert_eq!(req["jsonrpc"], JSONRPC_VERSION);
        assert_eq!(req["id"], 7);
        assert_eq!(req["method"], "tools/call");
        assert_eq!(req["params"]["name"], "ping");

        let note = notification("notifications/initialized", json!({}));
        assert_eq!(note["jsonrpc"], JSONRPC_VERSION);
        assert_eq!(note["method"], "notifications/initialized");
        assert!(note.get("id").is_none(), "通知不得携带 id");
    }

    #[test]
    fn initialize_params_carries_protocol_and_client_info() {
        let p = initialize_params("boenmind", "0.1");
        assert_eq!(p["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(p["clientInfo"]["name"], "boenmind");
        assert_eq!(p["clientInfo"]["version"], "0.1");
    }

    #[test]
    fn response_parsing_named_codes_and_result() {
        let err = json!({"jsonrpc": "2.0", "id": 1, "error": {"code": INVALID_PARAMS, "message": "未知工具"}});
        assert_eq!(error_code(&err), Some(INVALID_PARAMS));
        assert_eq!(error_message(&err), Some("未知工具"));
        assert_eq!(take_result(&err), json!({}), "错误响应无 result → 空对象");

        let ok = json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": []}});
        assert_eq!(error_code(&ok), None);
        assert_eq!(take_result(&ok), json!({"tools": []}));
    }
}
