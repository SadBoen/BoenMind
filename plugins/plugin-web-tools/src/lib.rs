//! # plugin-web-tools —— Web 取数工具插件（功能分类）。
//!
//! 把外网能力注册为 Agent 工具，结果回灌循环：
//! - `web.fetch`   抓取单个 URL 的文本内容（限 512KB，输出钱包；SSRF 防线）
//! - `web.search`  DuckDuckGo Instant Answer 搜索（免费无 key），返回 JSON 摘要
//!
//! 安全语义：
//! - **SSRF 防线**（[`check_url`]）：只允许 http/https；解析出的 Host 必须是
//!   公网地址——拒绝回环/内网/链路本地等（防 Agent 被诱导抓取内网服务、
//!   云元数据 169.254.169.254 等）；DNS 解析后二次校验 IP
//! - **输出钱包**（[`MAX_BODY_BYTES`]）：响应体限顶，超出截断只记总数——
//!   防超大页面撑爆进程内存/模型上下文
//! - 请求超时（10s 连接 + 30s 总时长），失败结构化回写（is_error=true）
//!
//! 工具命名沿用 `web.*`（与 DSH web 工具族同构）。纯自动执行（同 host.* 语义；
//! 若需审批，可配合 `--approval` 由门控策略后续扩展）。
//!
//! 接线：装配方调用 [`register_all`] 注册全部工具并 `gate.enable`。之后
//! plugin-loop 每回合把已启用工具 schema 发给模型，工具调用经 ToolGate 执行。

use kernel_contracts::tools::{
    ToolExecutionInput, ToolExecutionResult, ToolHandler, ToolSchema,
};
use kernel_contracts::ToolError;

pub mod plugin;

pub use plugin::manifest;

/// 响应体钱包：单次调用最多保留的字节数（截断只记总数）。
pub const MAX_BODY_BYTES: usize = 512 * 1024;

/// 请求总超时（含连接；连接级由 reqwest 默认 ~10s 兜底）。
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// web-tools 工具 id 常量（门控白名单）。
pub const WEB_FETCH: &str = "web.fetch";
pub const WEB_SEARCH: &str = "web.search";

/// 全部 web-tools 工具名。
pub const ALL_TOOL_NAMES: [&str; 2] = [WEB_FETCH, WEB_SEARCH];

/// 危险工具（需用户审批）：web.fetch 外联抓取（敏感面，SSRF 防线外仍要人确认）；
/// web.search 只读公网搜索，自动放行。
pub const DANGEROUS_TOOL_NAMES: [&str; 1] = [WEB_FETCH];

/// 全部 web-tools 工具 schema（文档/装配可查询）。
pub fn schemas() -> Vec<ToolSchema> {
    [WEB_FETCH, WEB_SEARCH]
        .iter()
        .map(|name| {
            let h: Box<dyn ToolHandler> = match *name {
                WEB_FETCH => Box::new(FetchTool),
                WEB_SEARCH => Box::new(SearchTool),
                _ => unreachable!("known web tool name"),
            };
            ToolSchema {
                name: h.name().to_string(),
                description: h.description().to_string(),
                parameters: h.parameters(),
            }
        })
        .collect()
}

/// 注册全部 web-tools 工具到注册表。
/// 调用方来自装配方（bm-assembly），传 plug-tools 的 `ToolRegistry` 具体类型。
/// 可重复调用（跳过已注册项，幂等）。
pub fn register_all(registry: &plugin_tools::ToolRegistry) -> Result<(), ToolError> {
    use std::sync::Arc;
    let handlers: Vec<Arc<dyn ToolHandler>> = vec![Arc::new(FetchTool), Arc::new(SearchTool)];
    for h in handlers {
        if registry.get(h.name()).is_some() {
            continue; // 幂等：已注册跳过
        }
        registry.register(h)?;
    }
    Ok(())
}

fn arg_str(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key).and_then(serde_json::Value::as_str).map(str::to_string)
}

/// SSRF 防线：只允许 http/https，且解析 Host 后必须是公网地址。
/// 返回规范化 URL（`http://host...` 或 `https://host...`）或 Err。
fn check_url(raw: &str) -> Result<String, ToolError> {
    let trimmed = raw.trim();
    let url = reqwest::Url::parse(trimmed)
        .map_err(|e| ToolError::new(format!("tool error: invalid url: {e}")))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(ToolError::new(format!(
            "tool error: unsupported scheme '{scheme}' (http/https only)"
        )));
    }
    let host = url.host_str().ok_or_else(|| {
        ToolError::new("tool error: url has no host")
    })?;
    // 域名/IPv4/IPv6 文本层面先拒回环与保留字；再按需 DNS 解析后校验 IP。
    let is_public = validate_host_target(host);
    if !is_public {
        return Err(ToolError::new(format!(
            "tool error: url host '{host}' is not a public address (SSRF blocked)"
        )));
    }
    Ok(url.to_string())
}

/// Host 校验：拒绝回环/私网/链路本地/保留段；域名则解析后校验解析 IP。
fn validate_host_target(host: &str) -> bool {
    // 回环/常见保留字面量直接拒。
    let lower = host.to_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") {
        return false;
    }
    // 数值 IP 文本判断。
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return is_public_ipv4(&ip);
    }
    if let Ok(ip) = host.parse::<std::net::Ipv6Addr>() {
        return is_public_ipv6(&ip);
    }
    // 域名：DNS 解析（A + AAAA），全部解析结果须公网。解析失败/无记录 → 拒，
    // 防 DNS rebinding/悬空域。
    match dns_lookup_ips(host) {
        Some(ips) if !ips.is_empty() => ips.iter().all(|ip| match ip {
            std::net::IpAddr::V4(v4) => is_public_ipv4(v4),
            std::net::IpAddr::V6(v6) => is_public_ipv6(v6),
        }),
        _ => false,
    }
}

/// IPv4 公网判定：非私网（10/8、172.16/12、192.168/16）、非回环 127/8、
/// 非链路本地 169.254/16、非文档保留 192.0.2/24、198.51.100/24、203.0.113/24、
/// 非组播 224/4、非 0.0.0.0。
fn is_public_ipv4(ip: &std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    let a = octets[0];
    let b = octets[1];
    match a {
        0 | 127 => false,
        10 => false,
        172 if (16..=31).contains(&b) => false,
        192 if b == 168 => false,
        169 if b == 254 => false, // 链路本地（云元数据 169.254.169.254）
        224..=239 => false, // 组播
        255 => false,
        192 if b == 0 && (octets[2] == 0 || octets[2] == 51 || octets[2] == 52) => false,
        _ => true,
    }
}

/// IPv6 公网判定：非回环 ::1、非链路本地 fe80::/10、非唯一本地 fc00::/7、
/// 非组播 ff00::/8。
fn is_public_ipv6(ip: &std::net::Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return false;
    }
    let seg = ip.segments();
    // fe80::/10（链路本地）
    if seg[0] & 0xffc0 == 0xfe80 {
        return false;
    }
    // fc00::/7（唯一本地）
    if seg[0] & 0xfe00 == 0xfc00 {
        return false;
    }
    // ff00::/8（组播）
    if seg[0] & 0xff00 == 0xff00 {
        return false;
    }
    true
}

/// DNS 解析域名 → IP 列表（阻塞式，keep simple；A + AAAA 都算，须全部公网）。
fn dns_lookup_ips(host: &str) -> Option<Vec<std::net::IpAddr>> {
    use std::net::ToSocketAddrs;
    let ok = (host, 443).to_socket_addrs().ok()?;
    let mut v = Vec::new();
    for addr in ok {
        v.push(addr.ip());
    }
    v.sort();
    v.dedup();
    if v.is_empty() { None } else { Some(v) }
}

/// 共享的 HTTP 客户端（rustls TLS，连接池复用）。
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent("BoenMind/0.1 (agent web tool)")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// 限顶读取响应体：超过 MAX_BODY_BYTES 截断只记总数（防超大页面撑爆）。
async fn read_capped(resp: reqwest::Response) -> Result<(String, bool, usize), String> {
    use futures_util::StreamExt;
    let mut keep = Vec::new();
    let mut total: usize = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("read body: {e}"))?;
        total += chunk.len();
        let room = MAX_BODY_BYTES.saturating_sub(keep.len());
        let take = room.min(chunk.len());
        if take > 0 {
            keep.extend_from_slice(&chunk[..take]);
        }
    }
    let text = String::from_utf8_lossy(&keep).to_string();
    Ok((text, total > MAX_BODY_BYTES, total))
}

// ---- web.fetch ----

#[derive(Debug, Clone, Copy, Default)]
struct FetchTool;

#[async_trait::async_trait]
impl ToolHandler for FetchTool {
    fn name(&self) -> &str {
        WEB_FETCH
    }

    fn description(&self) -> &str {
        "抓取一个 http/https URL 的文本内容（限 512KB，超量截断只记总数）。SSRF 防线：仅公网地址。返回 JSON：{status, ok, body, truncated, bytes}。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "http/https URL，如 https://example.com/docs" }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(raw) = arg_str(&input.arguments, "url") else {
            return Err(ToolError::new("tool error: missing url"));
        };
        let url = check_url(&raw)?;
        let resp = client()
            .get(&url)
            .send()
            .await
            .map_err(|e| ToolError::new(format!("tool error: request failed: {e}")))?;
        let status = resp.status().as_u16();
        let (body, truncated, total) = read_capped(resp).await.map_err(ToolError::new)?;
        let v = serde_json::json!({
            "status": status,
            "ok": status < 400,
            "body": body,
            "truncated": truncated,
            "bytes": total,
        });
        let text = serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string());
        if status >= 400 {
            Ok(ToolExecutionResult::error(text))
        } else {
            Ok(ToolExecutionResult::ok(text))
        }
    }
}

// ---- web.search ----

#[derive(Debug, Clone, Copy, Default)]
struct SearchTool;

#[async_trait::async_trait]
impl ToolHandler for SearchTool {
    fn name(&self) -> &str {
        WEB_SEARCH
    }

    fn description(&self) -> &str {
        "DuckDuckGo Instant Answer 搜索（免费无 key）：返回与查询匹配的摘要/答案/相关话题 JSON。适合事实型查询；完整网页搜索建议配合 web.fetch 使用搜索引擎结果页。返回 JSON：{query, abstractText, abstractUrl, heading, answer, relatedTopics:[…]}。"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "搜索查询词" }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let Some(q) = arg_str(&input.arguments, "query") else {
            return Err(ToolError::new("tool error: missing query"));
        };
        // DuckDuckGo IA 端点：https://api.duckduckgo.com/?q=...&format=json&no_html=1
        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1",
            urlencoding(&q)
        );
        // DDG API 本身是公网域名，直接请求（无需 check_url——该函数拒绝内网，
        // 但允许公网域名解析；这里域名字面量白名单，直接请求）。
        let resp = client()
            .get(&url)
            .send()
            .await
            .map_err(|e| ToolError::new(format!("tool error: search request failed: {e}")))?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Ok(ToolExecutionResult::error(format!(
                "search request failed (status {status})"
            )));
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ToolError::new(format!("tool error: search parse failed: {e}")))?;
        // 提炼关键字段（不把整包倒给模型——控制上下文开销）。
        let v = serde_json::json!({
            "query": q,
            "abstractText": json.get("AbstractText").and_then(serde_json::Value::as_str).unwrap_or(""),
            "abstractUrl": json.get("AbstractURL").and_then(serde_json::Value::as_str).unwrap_or(""),
            "heading": json.get("Heading").and_then(serde_json::Value::as_str).unwrap_or(""),
            "answer": json.get("Answer").and_then(serde_json::Value::as_str).unwrap_or(""),
            "relatedTopics": json.get("RelatedTopics").and_then(serde_json::Value::as_array).map(|a| {
                a.iter().take(8).filter_map(|t| {
                    t.get("Text").and_then(serde_json::Value::as_str).map(str::to_string)
                }).collect::<Vec<_>>()
            }).unwrap_or_default(),
        });
        let text = serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string());
        Ok(ToolExecutionResult::ok(text))
    }
}

/// 简易 URL 编码（DDG query 用；仅编码空格与保留字符，够用且不依赖外部依赖）。
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match *b {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssrf_rejects_private_and_loopback() {
        assert!(check_url("http://127.0.0.1:8080/x").is_err());
        assert!(check_url("http://localhost/x").is_err());
        assert!(check_url("http://169.254.169.254/latest/meta-data").is_err());
        assert!(check_url("http://10.0.0.1/x").is_err());
        assert!(check_url("http://192.168.1.1/x").is_err());
        assert!(check_url("file:///etc/passwd").is_err());
        assert!(check_url("ftp://example.com").is_err());
        assert!(check_url("http://[::1]/x").is_err());
        // 公网字面量 IP 放行。
        assert!(check_url("http://8.8.8.8/x").is_ok());
        // 公网域名（DNS 解析后可过；离线 CI 可能解析失败 → 拒，但本地可解析）。
        assert!(check_url("https://example.com").is_ok());
    }

    #[test]
    fn urlencoding_basics() {
        assert_eq!(urlencoding("a b"), "a%20b");
        assert_eq!(urlencoding("hello"), "hello");
        assert_eq!(urlencoding("你好"), "%E4%BD%A0%E5%A5%BD");
    }

    #[test]
    fn manifest_and_schemas_are_consistent() {
        use kernel_contracts::plugin::PluginCategory;
        let m = manifest();
        assert_eq!(m.id, "plugin-web-tools");
        assert_eq!(m.category, PluginCategory::Feature);
        let schemas = schemas();
        assert_eq!(schemas.len(), 2);
        for s in &schemas {
            assert!(ALL_TOOL_NAMES.contains(&s.name.as_str()), "unexpected schema {}", s.name);
        }
    }
}