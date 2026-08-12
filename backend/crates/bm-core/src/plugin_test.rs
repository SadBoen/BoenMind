//! 插件源连通性测试：按插件 manifest 的 `testSources` 模板对配置的搜索源
//! 发一个轻量探测请求，供设置页「测试」按钮使用（验证 API Key / 端点可达）。
//!
//! 探测逻辑与插件沙箱内的适配器独立（沙箱网络不可从服务端直接调用），
//! 只做连通性判断，不返回搜索结果。模板 `{<settings key>}` 用当前设置值
//! 替换（secret 取明文，仅服务端内部使用）；`custom*` 通配源按实例号展开
//! （source="custom3" → 模板中 `{customN.xxx}` → `{custom3.xxx}`）。
//! 新增搜索源只需在 extension.json 里加模板，无需改本模块。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::http_util::http_agent_global;
use crate::plugin_settings::TestSourceDecl;

/// 单源探测结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub detail: String,
}

/// 解析后的探测请求。
#[derive(Debug, PartialEq)]
struct ResolvedRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Value>,
}

/// 解析探测请求：模板匹配（精确 → `custom*` 通配）+ 设置值替换。
/// 返回 Err 表示未配置依赖或模板非法（不发请求）。
fn resolve_request(
    source: &str,
    settings: &Value,
    test_sources: &HashMap<String, TestSourceDecl>,
) -> Result<ResolvedRequest, String> {
    // 精确匹配（jina/tavily/…）或通配（custom1..N → custom*）
    let instance = source
        .strip_prefix("custom")
        .and_then(|n| n.parse::<usize>().ok());
    let decl = test_sources.get(source).or_else(|| {
        let wildcard = test_sources.get("custom*")?;
        instance?;
        Some(wildcard)
    });
    let Some(decl) = decl else {
        return Err("未知源".into());
    };

    // 模板替换：`{<settings key>}` → 设置值；引用的 key 缺失或为空 = 未配置
    let substitute = |template: &str| -> Result<String, String> {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let Some(end_rel) = rest[start..].find('}') else {
                return Err(format!("模板格式错误（缺少闭合大括号）: {template}"));
            };
            let key = &rest[start + 1..start + end_rel];
            // 通配源：模板里 {customN.xxx} 的 N 替换为实际实例号
            let key = if let Some(n) = instance {
                key.replacen("customN", &format!("custom{n}"), 1)
            } else {
                key.to_string()
            };
            let value = settings.get(&key).and_then(Value::as_str).unwrap_or("");
            if value.is_empty() {
                return Err(format!("未配置 {key}"));
            }
            out.push_str(value);
            rest = &rest[start + end_rel + 1..];
        }
        out.push_str(rest);
        Ok(out)
    };

    let url = substitute(&decl.url)?;
    let mut headers: Vec<(String, String)> = Vec::new();
    for (name, value) in &decl.headers {
        // 头名与头值都可能含模板（自定义源的认证头名是设置值）
        headers.push((substitute(name)?, substitute(value)?));
    }
    let body = decl.body.as_ref().map(|b| substitute_value(b, &substitute));
    Ok(ResolvedRequest {
        method: decl.method.clone(),
        url,
        headers,
        body,
    })
}

/// 探测一个搜索源（source 形如 `jina` / `tavily` / `custom1`）。
/// `test_sources` 为插件 manifest 声明的模板表；`settings` 为明文扁平值。
pub fn test_source(
    source: &str,
    settings: &Value,
    test_sources: &HashMap<String, TestSourceDecl>,
) -> SourceTestResult {
    let started = Instant::now();
    let req = match resolve_request(source, settings, test_sources) {
        Ok(req) => req,
        Err(detail) => return SourceTestResult { ok: false, latency_ms: 0, detail },
    };

    // 整次探测（含读体）限 10s，避免测试按钮长时间无响应
    let agent = http_agent_global(Duration::from_secs(10));
    let result = if req.method == "GET" {
        let mut call = agent.get(&req.url);
        for (k, v) in &req.headers {
            call = call.header(k, v);
        }
        call.call()
    } else {
        let mut call = agent.post(&req.url);
        for (k, v) in &req.headers {
            call = call.header(k, v);
        }
        // 无 body 时发空对象兜底
        call.send_json(req.body.unwrap_or_else(|| serde_json::json!({})))
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

/// 递归替换 JSON 里的字符串模板（body 中的 `{key}` 占位）。
fn substitute_value(
    value: &Value,
    substitute: &impl Fn(&str) -> Result<String, String>,
) -> Value {
    match value {
        Value::String(s) => substitute(s)
            .map(Value::String)
            .unwrap_or_else(|_| Value::String(s.clone())),
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| substitute_value(v, substitute)).collect())
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), substitute_value(v, substitute)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_settings::TestSourceDecl;

    fn decls() -> HashMap<String, TestSourceDecl> {
        serde_json::from_value(serde_json::json!({
            "jina": {
                "method": "GET",
                "url": "https://s.jina.ai/?q=test",
                "headers": { "Authorization": "Bearer {sources.jina.apiKey}" }
            },
            "custom*": {
                "method": "GET",
                "url": "{customN.url}?q=test",
                "headers": { "{customN.apiKeyHeader}": "{customN.apiKey}" }
            }
        }))
        .unwrap()
    }

    #[test]
    fn unknown_source_returns_error() {
        assert_eq!(resolve_request("nope", &serde_json::json!({}), &decls()), Err("未知源".into()));
    }

    #[test]
    fn missing_key_returns_hint() {
        // 模板引用的 key 未配置 → 明确提示（不发出请求）
        assert_eq!(
            resolve_request("jina", &serde_json::json!({}), &decls()),
            Err("未配置 sources.jina.apiKey".into())
        );
    }

    #[test]
    fn custom_requires_url() {
        assert_eq!(
            resolve_request("custom1", &serde_json::json!({}), &decls()),
            Err("未配置 custom1.url".into())
        );
    }

    #[test]
    fn custom_instance_maps_wildcard_and_expands_n() {
        // custom3 → custom* 模板；{customN.xxx} 展开为 custom3.xxx
        let req = resolve_request(
            "custom3",
            &serde_json::json!({
                "custom3.url": "https://s.example.com",
                "custom3.apiKeyHeader": "X-Key",
                "custom3.apiKey": "k-3",
            }),
            &decls(),
        )
        .unwrap();
        assert_eq!(req.url, "https://s.example.com?q=test");
        assert_eq!(req.headers, vec![("X-Key".to_string(), "k-3".to_string())]);
        // custom4 不受 custom3 的值影响（实例隔离）
        let req4 = resolve_request("custom4", &serde_json::json!({"custom4.url": ""}), &decls());
        assert_eq!(req4, Err("未配置 custom4.url".into()));
    }

    #[test]
    fn exact_match_beats_wildcard() {
        // 精确源存在时不落入通配（jina 模板不展开 customN）
        let req = resolve_request(
            "jina",
            &serde_json::json!({"sources.jina.apiKey": "sk-1"}),
            &decls(),
        )
        .unwrap();
        assert_eq!(req.url, "https://s.jina.ai/?q=test");
        assert_eq!(req.headers, vec![("Authorization".to_string(), "Bearer sk-1".to_string())]);
    }
}
