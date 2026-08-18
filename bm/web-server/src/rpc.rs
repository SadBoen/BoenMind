//! RPC 信封（契约台账 §1 面 1 + §4 语义）：client-request / server-response /
//! client-response / server-request 四元判别，rpcId 回显，invalid-request 哨兵。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 路径常量（契约台账 §1，源 packages/client/connection/src/api-path.ts）。
pub const API_PATH: &str = "/api";
pub const MUX_EVENTS_PATH: &str = "/api/events.mux";
pub const HOST_EVENTS_PATH: &str = "/api/events.host";
pub const RESPOND_PATH: &str = "/api/respond";
pub const SESSION_EXPORT_PATH: &str = "/api/session.export";
pub const PLUGINS_EVENTS_PATH: &str = "/plugins/events";

/// 信封解析失败的 rpcId 固定哨兵（台账：`INVALID_REQUEST_RPC_ID = RpcId('invalid-request')`）。
pub const INVALID_REQUEST_RPC_ID: &str = "invalid-request";

/// 上行请求信封（client-request）。
#[derive(Debug, Clone, Deserialize)]
pub struct ClientRequest {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "rpcId")]
    pub rpc_id: String,
    pub method: String,
    pub payload: Value,
}

/// 下行响应信封（server-response）。
#[derive(Debug, Clone, Serialize)]
pub struct ServerResponse {
    #[serde(rename = "type")]
    pub type_: &'static str,
    #[serde(rename = "rpcId")]
    pub rpc_id: String,
    pub result: Value,
}

impl ServerResponse {
    pub fn ok(rpc_id: &str, value: Value) -> Self {
        Self {
            type_: "server-response",
            rpc_id: rpc_id.to_string(),
            result: json!({ "ok": true, "value": value }),
        }
    }

    pub fn err(rpc_id: &str, code: &str, message: impl Into<String>) -> Self {
        Self {
            type_: "server-response",
            rpc_id: rpc_id.to_string(),
            result: json!({
                "ok": false,
                "error": { "code": code, "message": message.into(), "details": {} }
            }),
        }
    }

    pub fn bad_request(rpc_id: &str, message: impl Into<String>) -> Self {
        Self::err(rpc_id, "bad-request", message)
    }
}

/// WS 下行帧信封（server-request；method = frame.type）。
#[derive(Debug, Clone, Serialize)]
pub struct ServerRequestFrame {
    #[serde(rename = "type")]
    pub type_: &'static str,
    #[serde(rename = "rpcId")]
    pub rpc_id: String,
    pub method: String,
    pub payload: Value,
}

impl ServerRequestFrame {
    /// 构造下行帧。method 同时作为 payload 的 `type` 判别字段注入——官方前端
    /// `web-api-client.readWebSocket` 用 `frameSchema.parse(full.payload)` 校验
    /// MuxFrame/HostFrame 判别联合（payload.type 必须存在），缺了整帧被丢弃
    /// （对话空白/会话列表不刷新的根因）。外层信封保留官方 `server-request`
    /// 四元判别，两层都要过 schema。
    pub fn new(rpc_id: impl Into<String>, method: impl Into<String>, mut payload: Value) -> Self {
        let method = method.into();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("type".into(), json!(method));
        }
        Self {
            type_: "server-request",
            rpc_id: rpc_id.into(),
            method,
            payload,
        }
    }
}

/// 应答上行信封（client-response，POST /api/respond 用）。
#[derive(Debug, Clone, Deserialize)]
pub struct ClientResponse {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "rpcId")]
    pub rpc_id: String,
    pub result: Value,
}

/// 应答回执（RpcReceipt）。
#[derive(Debug, Clone, Serialize)]
pub struct RpcReceipt {
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl RpcReceipt {
    pub fn accepted() -> Self {
        Self {
            accepted: true,
            reason: None,
        }
    }
    pub fn rejected(reason: &str) -> Self {
        Self {
            accepted: false,
            reason: Some(reason.to_string()),
        }
    }
}

/// 从任意原始体尽力捞取 string rpcId（解析失败的兜底回显）。
pub fn extract_rpc_id(raw: &str) -> String {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.get("rpcId").and_then(|r| r.as_str()).map(str::to_string))
        .unwrap_or_else(|| INVALID_REQUEST_RPC_ID.to_string())
}

/// 校验 channel 段：`/^\/[A-Za-z0-9._~-]+$/`（台账 §1）。
pub fn valid_channel(channel: &str) -> bool {
    let bytes = channel.as_bytes();
    if bytes.is_empty() || bytes[0] != b'/' {
        return false;
    }
    bytes[1..].iter().all(|b| {
        b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-')
    })
}

/// 校验 endpoint 段：`/^[A-Za-z0-9_$.-]+$/`，禁止空段/`.`/`..`（台账 §1）。
pub fn valid_endpoint(endpoint: &str) -> bool {
    !endpoint.is_empty()
        && endpoint != "."
        && endpoint != ".."
        && endpoint
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.' | '-'))
}

/// 便利函数：构造 `{ok:true, value}`。
pub fn ok(value: Value) -> Value {
    json!({ "ok": true, "value": value })
}

/// 便利函数：构造 `{ok:false, error:{code, message, details:{}}}`。
pub fn err(code: &str, message: impl Into<String>) -> Value {
    json!({ "ok": false, "error": { "code": code, "message": message.into(), "details": {} } })
}

/// 便利函数：构造 `{ok:false, error:{code, message, details}}`。
pub fn err_with_details(code: &str, message: impl Into<String>, details: Value) -> Value {
    json!({
        "ok": false,
        "error": { "code": code, "message": message.into(), "details": details }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_shapes_are_verbatim() {
        let r = ServerResponse::ok("abc-123", json!({ "a": 1 }));
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"type":"server-response","rpcId":"abc-123","result":{"ok":true,"value":{"a":1}}}"#
        );
        let e = ServerResponse::err("abc", "session-not-found", "nope");
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            // serde_json Map 按键序（BTreeMap 字母序）序列化：error < ok；code < details < message。
            r#"{"type":"server-response","rpcId":"abc","result":{"error":{"code":"session-not-found","details":{},"message":"nope"},"ok":false}}"#
        );
    }

    #[test]
    fn frame_shape_is_verbatim() {
        let f = ServerRequestFrame::new("r1", "session/event", json!({ "sessionId": "s" }));
        // 外层信封四元判别 + payload 内注入 type 判别字段（官方 frameSchema 要求）。
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&f).unwrap()).unwrap();
        assert_eq!(v["type"], "server-request");
        assert_eq!(v["rpcId"], "r1");
        assert_eq!(v["method"], "session/event");
        assert_eq!(v["payload"]["type"], "session/event");
        assert_eq!(v["payload"]["sessionId"], "s");
    }

    #[test]
    fn rpc_id_fallback() {
        assert_eq!(extract_rpc_id("garbage"), INVALID_REQUEST_RPC_ID);
        assert_eq!(
            extract_rpc_id(r#"{"type":"client-request","rpcId":"x9","method":"a","payload":{}}"#),
            "x9"
        );
    }

    #[test]
    fn channel_endpoint_rules() {
        assert!(valid_channel("/api"));
        assert!(valid_endpoint("session.list"));
        assert!(valid_endpoint("agentPreset.read"));
        assert!(!valid_channel("/api/extra"));
        assert!(!valid_endpoint(""));
        assert!(!valid_endpoint("."));
        assert!(!valid_endpoint("a/b"));
    }
}
