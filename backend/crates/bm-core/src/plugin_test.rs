//! 插件源连通性测试：对插件设置里配置的搜索源发一个轻量探测请求，
//! 供设置页「测试」按钮使用（验证 API Key 是否可用、端点是否可达）。
//!
//! 探测逻辑与插件沙箱内的适配器独立（沙箱网络不可从服务端直接调用），
//! 只做连通性判断，不返回搜索结果。

use serde_json::Value;
use std::time::{Duration, Instant};

use crate::http_util::http_agent_global;

/// 单源探测结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub detail: String,
}

/// 探测一个搜索源（source 形如 `jina` / `tavily` / `custom1`）。
/// settings 为插件设置的明文扁平值（含 apiKey）。
pub fn test_source(source: &str, settings: &Value) -> SourceTestResult {
    let started = Instant::now();
    // 整次探测（含读体）限 10s，避免测试按钮长时间无响应
    let agent = http_agent_global(Duration::from_secs(10));

    // 构造探测请求；返回 (method, url, headers, body_json)
    let req = match source {
        "jina" => {
            let key = str_field(settings, "sources.jina.apiKey");
            if key.is_empty() {
                return SourceTestResult { ok: false, latency_ms: 0, detail: "未配置 API Key".into() };
            }
            ("GET", "https://s.jina.ai/?q=test".to_string(),
             vec![("Authorization".to_string(), format!("Bearer {key}")), ("Accept".to_string(), "text/markdown".into())],
             None)
        }
        "tavily" => {
            let key = str_field(settings, "sources.tavily.apiKey");
            if key.is_empty() {
                return SourceTestResult { ok: false, latency_ms: 0, detail: "未配置 API Key".into() };
            }
            ("POST", "https://api.tavily.com/search".to_string(),
             vec![("Content-Type".to_string(), "application/json".into())],
             Some(serde_json::json!({"api_key": key, "query": "test", "max_results": 1})))
        }
        "exa" => {
            let key = str_field(settings, "sources.exa.apiKey");
            if key.is_empty() {
                return SourceTestResult { ok: false, latency_ms: 0, detail: "未配置 API Key".into() };
            }
            ("POST", "https://api.exa.ai/search".to_string(),
             vec![("x-api-key".to_string(), key), ("Content-Type".to_string(), "application/json".into())],
             Some(serde_json::json!({"query": "test", "numResults": 1})))
        }
        "serper" => {
            let key = str_field(settings, "sources.serper.apiKey");
            if key.is_empty() {
                return SourceTestResult { ok: false, latency_ms: 0, detail: "未配置 API Key".into() };
            }
            ("POST", "https://google.serper.dev/search".to_string(),
             vec![("X-API-KEY".to_string(), key), ("Content-Type".to_string(), "application/json".into())],
             Some(serde_json::json!({"q": "test", "num": 1})))
        }
        "firecrawl" => {
            let key = str_field(settings, "sources.firecrawl.apiKey");
            if key.is_empty() {
                return SourceTestResult { ok: false, latency_ms: 0, detail: "未配置 API Key".into() };
            }
            ("POST", "https://api.firecrawl.dev/v2/scrape".to_string(),
             vec![("Authorization".to_string(), format!("Bearer {key}")), ("Content-Type".to_string(), "application/json".into())],
             Some(serde_json::json!({"url": "https://example.com", "formats": ["markdown"]})))
        }
        custom if custom.starts_with("custom") => {
            let Some(num_part) = custom.strip_prefix("custom") else {
                return SourceTestResult { ok: false, latency_ms: 0, detail: "未知源".into() };
            };
            let Ok(n) = num_part.split('.').next().unwrap_or("").parse::<usize>() else {
                return SourceTestResult { ok: false, latency_ms: 0, detail: "未知源".into() };
            };
            let url = str_field(settings, &format!("custom{n}.url"));
            if url.is_empty() {
                return SourceTestResult { ok: false, latency_ms: 0, detail: "未配置请求 URL".into() };
            }
            let key = str_field(settings, &format!("custom{n}.apiKey"));
            let header = str_field(settings, &format!("custom{n}.apiKeyHeader"));
            let url = url.replace("{query}", "test");
            let headers: Vec<(String, String)> = if !header.is_empty() && !key.is_empty() {
                vec![(header, key)]
            } else {
                Vec::new()
            };
            ("GET", url, headers, None)
        }
        _ => {
            return SourceTestResult { ok: false, latency_ms: 0, detail: "未知源".into() };
        }
    };

    let (method, url, headers, body) = req;
    // GET / POST 的 builder 类型不同（ureq 3），分开构造
    let result = if method == "GET" {
        let mut call = agent.get(&url);
        for (k, v) in &headers {
            call = call.header(k, v);
        }
        call.call()
    } else {
        let mut call = agent.post(&url);
        for (k, v) in &headers {
            call = call.header(k, v);
        }
        // 当前所有 POST 源都带 body；无 body 时发空对象兜底
        call.send_json(body.unwrap_or_else(|| serde_json::json!({})))
    };

    let latency_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(resp) => {
            // 统一 agent 配置 4xx/5xx 不作错误返回，这里显式判定
            let status = resp.status().as_u16();
            SourceTestResult { ok: status < 300, latency_ms, detail: format!("HTTP {status}") }
        }
        Err(err) => {
            let detail = if err.to_string().to_lowercase().contains("timed out") {
                "请求超时（10s）".to_string()
            } else {
                err.to_string()
            };
            SourceTestResult { ok: false, latency_ms, detail }
        }
    }
}

fn str_field(settings: &Value, key: &str) -> String {
    settings
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_source_returns_error() {
        let r = test_source("nope", &serde_json::json!({}));
        assert!(!r.ok);
        assert_eq!(r.detail, "未知源");
    }

    #[test]
    fn missing_key_returns_hint() {
        let r = test_source("jina", &serde_json::json!({"sources": {}}));
        assert!(!r.ok);
        assert_eq!(r.detail, "未配置 API Key");
    }

    #[test]
    fn custom_requires_url() {
        let r = test_source("custom1", &serde_json::json!({}));
        assert!(!r.ok);
        assert_eq!(r.detail, "未配置请求 URL");
    }
}
