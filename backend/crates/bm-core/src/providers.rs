//! 提供商工具：模型列表拉取与连接测试（面向设置页表单）。
//!
//! 同步 ureq 实现（复用 skills.rs 的 http_agent 模式），调用方应放
//! `tokio::task::spawn_blocking`，见 bm-server routes。
//!
//! 支持的接口方言：
//! - OpenAI 兼容（deepseek/groq/…/custom）：GET /models、POST /chat/completions
//! - Anthropic：GET /v1/models、POST /v1/messages（x-api-key + anthropic-version）
//! - Gemini：GET /v1beta/models、POST /v1beta/models/{model}:generateContent

use std::io::Read;

use serde_json::{Value, json};

use crate::config::ProviderKind;
use crate::error::AppError;
use crate::http_util::http_agent;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TEST_TOKENS: u64 = 128;
const REPLY_TRUNCATE: usize = 500;
const ERROR_BODY_SNIPPET: usize = 200;

/// 官方端点表（单一数据源：新增 kind 只需更新本表 + ProviderKind::ALL，
/// 前端经 `GET /api/providers/presets` 获取，不再各自维护一份端点）。
pub fn official_base_url(kind: ProviderKind) -> Option<&'static str> {
    match kind {
        ProviderKind::Openai => Some("https://api.openai.com/v1"),
        ProviderKind::Anthropic => Some("https://api.anthropic.com"),
        ProviderKind::Gemini => Some("https://generativelanguage.googleapis.com"),
        ProviderKind::Deepseek => Some("https://api.deepseek.com/v1"),
        ProviderKind::Minimax => Some("https://api.minimaxi.com/v1"),
        ProviderKind::Moonshot => Some("https://api.moonshot.cn/v1"),
        ProviderKind::Zhipu => Some("https://open.bigmodel.cn/api/paas/v4"),
        ProviderKind::Qwen => Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
        ProviderKind::Openrouter => Some("https://openrouter.ai/api/v1"),
        ProviderKind::Xai => Some("https://api.x.ai/v1"),
        ProviderKind::Zai => Some("https://api.z.ai/api/paas/v4"),
        ProviderKind::Groq => Some("https://api.groq.com/openai/v1"),
        ProviderKind::Mistral => Some("https://api.mistral.ai/v1"),
        ProviderKind::Together => Some("https://api.together.ai/v1"),
        ProviderKind::Cerebras => Some("https://api.cerebras.ai/v1"),
        ProviderKind::Fireworks => Some("https://api.fireworks.ai/inference"),
        ProviderKind::Huggingface => Some("https://router.huggingface.co/v1"),
        ProviderKind::Nvidia => Some("https://integrate.api.nvidia.com/v1"),
        ProviderKind::Xiaomi => Some("https://api.xiaomimimo.com/v1"),
        ProviderKind::Antling => Some("https://api.ant-ling.com/v1"),
        ProviderKind::Baseten => Some("https://inference.baseten.co/v1"),
        ProviderKind::Ollama => Some("http://127.0.0.1:11434/v1"),
        ProviderKind::Llamacpp => Some("http://127.0.0.1:8080/v1"),
        ProviderKind::Custom => None, // 必须由用户填写端点
    }
}

/// 是否 Anthropic 方言（x-api-key + anthropic-version 头）
fn is_anthropic(kind: ProviderKind) -> bool {
    kind == ProviderKind::Anthropic
}

/// 全量官方端点表（供 `GET /api/providers/presets` 下发前端预填表单）。
pub fn official_base_urls() -> Vec<(ProviderKind, Option<&'static str>)> {
    ProviderKind::ALL
        .iter()
        .map(|k| (*k, official_base_url(*k)))
        .collect()
}

/// 是否 Gemini 方言（v1beta generateContent）
fn is_gemini(kind: ProviderKind) -> bool {
    kind == ProviderKind::Gemini
}

/// 解析最终 base URL：用户填写的优先（去尾部斜杠），否则官方端点；custom 必须填写。
fn resolve_base_url(kind: ProviderKind, base_url: &str) -> Result<String, AppError> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if !trimmed.is_empty() {
        return Ok(trimmed.to_string());
    }
    official_base_url(kind)
        .map(str::to_string)
        .ok_or_else(|| AppError::invalid("该类型必须填写 API 端点（custom 需填写 OpenAI 兼容地址）"))
}

/// SSRF 防护：校验端点必须是指向公网的合法 http(s) URL。
/// ollama / llamacpp 是本地模型服务（官方端点即 127.0.0.1），豁免本校验。
fn validate_base_url(kind: ProviderKind, url: &str) -> Result<(), AppError> {
    if matches!(kind, ProviderKind::Ollama | ProviderKind::Llamacpp) {
        return Ok(());
    }
    let parsed = url::Url::parse(url)
        .map_err(|_| AppError::invalid("API 端点必须是完整的 http(s):// URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::invalid("API 端点只支持 http/https"));
    }
    let Some(host) = parsed.host_str() else {
        return Err(AppError::invalid("API 端点缺少主机名"));
    };
    // IPv6 字面量的 host_str 带方括号（"[::1]"），去掉再判断
    let host = host.trim_start_matches('[').trim_end_matches(']');
    // localhost 是保留域名（只解析到回环），与 IP 字面量同等拦截；
    // 其它域名不做 DNS 解析校验（解析结果取决于发起请求的机器）
    if host.eq_ignore_ascii_case("localhost") {
        return Err(AppError::invalid("API 端点不允许指向本机（localhost）"));
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let blocked = match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local() || v6.is_unspecified()
            }
        };
        if blocked {
            return Err(AppError::invalid(format!("API 端点不允许指向私网/本机地址（{ip}）")));
        }
    }
    Ok(())
}

/// 发送请求并读取响应体（限 64KB），返回 (状态码, body)。
/// 接收尚未发起的请求结果（GET 用 `call()`、POST 用 `send_json(...)`），
/// 统一处理传输层错误并把 4xx/5xx 的 body 透传给调用方展示。
fn collect_response(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<(u16, String), AppError> {
    let resp = result.map_err(|err| match err {
        ureq::Error::Io(e) => AppError::upstream(format!("网络错误: {e}")),
        ureq::Error::Timeout(t) => AppError::upstream(format!("请求超时: {t}")),
        ureq::Error::HostNotFound => AppError::upstream("无法解析主机名"),
        ureq::Error::BadUri(u) => AppError::upstream(format!("无效的 URL: {u}")),
        other => AppError::upstream(format!("请求失败: {other}")),
    })?;
    let status = resp.status().as_u16();
    let mut body = String::new();
    resp.into_body()
        .into_reader()
        .take(64 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| AppError::upstream(format!("读取响应失败: {e}")))?;
    Ok((status, body))
}

/// 状态码非 2xx 时构造错误信息（含服务商返回的 body 摘要，如 401 无效 key）。
fn ensure_success(status: u16, body: &str) -> Result<(), AppError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    let snippet: String = body.chars().take(ERROR_BODY_SNIPPET).collect();
    let detail = if snippet.trim().is_empty() {
        String::new()
    } else {
        format!(": {}", snippet)
    };
    Err(AppError::upstream(format!("HTTP {status}{detail}")))
}

/// URL 路径段编码（模型名可能含 `/`，如 together 的 `Qwen/Qwen3.7-Max`）。
fn encode_path_segment(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => {
                let mut b = [0u8; 4];
                let bytes = c.encode_utf8(&mut b).as_bytes();
                bytes.iter().map(|x| format!("%{x:02X}")).collect()
            }
        })
        .collect()
}

/// 从 OpenAI 兼容 /models 响应提取模型 id。
/// 兼容三种形态：`{data:[{id}]}`、`{models:[{id|name}]}`、字符串数组；
/// Gemini 的 `models[].name`（"models/xxx"）也按 name 处理并去前缀。
fn parse_model_ids(body: &str) -> Vec<String> {
    let value: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let list = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut ids: Vec<String> = Vec::new();
    for item in list {
        match item {
            Value::String(s) => {
                if !s.trim().is_empty() {
                    ids.push(s.trim().to_string());
                }
            }
            Value::Object(map) => {
                let id = map
                    .get("id")
                    .and_then(Value::as_str)
                    .or_else(|| map.get("name").and_then(Value::as_str))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                if let Some(mut id) = id {
                    // Gemini: "models/gemini-2.5-pro" → "gemini-2.5-pro"
                    if let Some(stripped) = id.strip_prefix("models/") {
                        id = stripped.to_string();
                    }
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            _ => {}
        }
    }
    ids
}

/// 拉取模型列表。`base_url` 为空时使用官方端点；API key 为空时（ollama 等）不带头。
pub fn list_provider_models(kind: ProviderKind, base_url: &str, api_key: &str) -> Result<Vec<String>, AppError> {
    let base = resolve_base_url(kind, base_url)?;
    validate_base_url(kind, &base)?;
    let key = api_key.trim();

    let url = if is_gemini(kind) {
        format!("{base}/v1beta/models?pageSize=100")
    } else if is_anthropic(kind) {
        format!("{base}/v1/models")
    } else {
        format!("{base}/models")
    };

    let mut req = http_agent().get(&url);
    if is_anthropic(kind) {
        if key.is_empty() {
            return Err(AppError::invalid("Anthropic 需要 API Key"));
        }
        req = req
            .header("x-api-key", key)
            .header("anthropic-version", ANTHROPIC_VERSION);
    } else if is_gemini(kind) {
        if key.is_empty() {
            return Err(AppError::invalid("Gemini 需要 API Key"));
        }
        req = req.header("Authorization", format!("Bearer {key}"));
    } else if !key.is_empty() {
        req = req.header("Authorization", format!("Bearer {key}"));
    }

    let (status, body) = collect_response(req.call())?;
    ensure_success(status, &body)?;

    let models = parse_model_ids(&body);
    if models.is_empty() {
        return Err(AppError::upstream(format!("模型列表为空（响应格式可能不兼容）: {}", body.chars().take(120).collect::<String>())));
    }
    Ok(models)
}

/// 测试连接：`message` 为空时仅验证连通（拉取模型列表）；
/// 非空时发送真实对话请求并返回模型回复（最多 128 token，截断 500 字符）。
pub fn test_provider_connection(
    kind: ProviderKind,
    base_url: &str,
    api_key: &str,
    model: &str,
    message: &str,
) -> Result<String, AppError> {
    let base = resolve_base_url(kind, base_url)?;
    validate_base_url(kind, &base)?;
    let key = api_key.trim();
    let text = message.trim();

    // 无测试消息：连通测试（模型列表接口能通即视为连接正常）
    if text.is_empty() {
        let models = list_provider_models(kind, base_url, api_key)?;
        return Ok(format!("连接正常，发现 {} 个模型", models.len()));
    }

    if model.trim().is_empty() {
        return Err(AppError::invalid("请先填写模型名称再发送测试消息"));
    }

    let reply = if is_gemini(kind) {
        if key.is_empty() {
            return Err(AppError::invalid("Gemini 需要 API Key"));
        }
        let url = format!("{base}/v1beta/models/{}:generateContent", encode_path_segment(model.trim()));
        let (status, body) = collect_response(
            http_agent()
                .post(&url)
                .header("Authorization", format!("Bearer {key}"))
                .send_json(json!({
                    "contents": [{ "role": "user", "parts": [{ "text": text }] }],
                    "generationConfig": { "maxOutputTokens": MAX_TEST_TOKENS },
                })),
        )?;
        ensure_success(status, &body)?;
        let value: Value = serde_json::from_str(&body).map_err(|e| AppError::upstream(format!("解析响应失败: {e}")))?;
        // candidates[].content.parts[].text
        value
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|p| p.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| AppError::upstream(format!("响应中没有文本内容: {}", body)))
    } else if is_anthropic(kind) {
        if key.is_empty() {
            return Err(AppError::invalid("Anthropic 需要 API Key"));
        }
        let (status, body) = collect_response(
            http_agent()
                .post(&format!("{base}/v1/messages"))
                .header("x-api-key", key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .send_json(json!({
                    "model": model.trim(),
                    "max_tokens": MAX_TEST_TOKENS,
                    "messages": [{ "role": "user", "content": text }],
                })),
        )?;
        ensure_success(status, &body)?;
        let value: Value = serde_json::from_str(&body).map_err(|e| AppError::upstream(format!("解析响应失败: {e}")))?;
        // content: [{type:"text", text:"..."}]
        let texts: Vec<String> = value
            .get("content")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.get("text").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if texts.is_empty() {
            return Err(AppError::upstream(format!("响应中没有文本内容: {}", body)));
        }
        Ok(texts.join("\n"))
    } else {
        // OpenAI 兼容
        let mut builder = http_agent().post(&format!("{base}/chat/completions"));
        if !key.is_empty() {
            builder = builder.header("Authorization", format!("Bearer {key}"));
        }
        let (status, body) = collect_response(
            builder.send_json(json!({
                "model": model.trim(),
                "messages": [{ "role": "user", "content": text }],
                "max_tokens": MAX_TEST_TOKENS,
            })),
        )?;
        ensure_success(status, &body)?;
        let value: Value = serde_json::from_str(&body).map_err(|e| AppError::upstream(format!("解析响应失败: {e}")))?;
        // choices[0].message.content（字符串或分段数组）
        let content = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"));
        match content {
            Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
            Some(Value::Array(parts)) => {
                let texts: Vec<String> = parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(Value::as_str).map(str::to_string))
                    .collect();
                if texts.is_empty() {
                    return Err(AppError::upstream(format!("响应中没有文本内容: {}", body)));
                }
                Ok(texts.join("\n"))
            }
            _ => Err(AppError::upstream(format!("响应中没有文本内容: {}", body))),
        }
    }?;

    Ok(truncate(&reply, REPLY_TRUNCATE))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_endpoints_cover_all_kinds() {
        // 遍历 ProviderKind::ALL 而非手写数组：新增 kind 漏加端点会直接报错
        for kind in ProviderKind::ALL {
            if kind == ProviderKind::Custom {
                assert!(
                    official_base_url(kind).is_none(),
                    "custom 必须由用户填写端点"
                );
            } else {
                assert!(
                    official_base_url(kind).is_some(),
                    "kind {kind:?} 缺官方端点"
                );
            }
        }
        // 全量表与逐项查询一致
        assert_eq!(
            official_base_urls().len(),
            ProviderKind::ALL.len()
        );
    }

    #[test]
    fn resolve_base_url_prefers_user_value() {
        assert_eq!(
            resolve_base_url(ProviderKind::Deepseek, "https://api.deepseek.com/v1/").unwrap(),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(
            resolve_base_url(ProviderKind::Deepseek, "").unwrap(),
            "https://api.deepseek.com/v1"
        );
        assert!(resolve_base_url(ProviderKind::Custom, "").is_err());
    }

    #[test]
    fn validate_base_url_allows_public_endpoints() {
        for ok in [
            "https://api.openai.com/v1",
            "https://api.deepseek.com/v1/",
            "http://example.com:8000/v1",
        ] {
            validate_base_url(ProviderKind::Custom, ok).unwrap_or_else(|e| panic!("应放行 {ok}: {e}"));
        }
    }

    #[test]
    fn validate_base_url_rejects_private_targets() {
        for bad in [
            "http://127.0.0.1:9000/v1",
            "http://localhost:11434/v1",
            "http://192.168.1.10:8000/v1",
            "http://10.0.0.1/v1",
            "http://172.16.0.1/v1",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]:8080/v1",
            "file:///etc/passwd",
            "ftp://example.com/v1",
            "not a url",
        ] {
            assert!(
                validate_base_url(ProviderKind::Custom, bad).is_err(),
                "应拒绝 {bad}"
            );
        }
    }

    #[test]
    fn validate_base_url_exempts_local_servers() {
        // ollama / llamacpp 官方端点就是本机地址，必须放行
        assert!(validate_base_url(ProviderKind::Ollama, "http://127.0.0.1:11434/v1").is_ok());
        assert!(validate_base_url(ProviderKind::Llamacpp, "http://127.0.0.1:8080/v1").is_ok());
        // 但任意 kind 也能显式指向私网：ollama 常部署在局域网主机，属合法场景
        assert!(validate_base_url(ProviderKind::Ollama, "http://192.168.1.5:11434/v1").is_ok());
    }

    #[test]
    fn parse_models_openai_style() {
        let ids = parse_model_ids(r#"{"data":[{"id":"gpt-4o"},{"id":"gpt-4o-mini","reasoning":true}]}"#);
        assert_eq!(ids, vec!["gpt-4o", "gpt-4o-mini"]);
        // 字符串数组形态（部分兼容端点）
        let ids = parse_model_ids(r#"["a","b"]"#);
        assert_eq!(ids, vec!["a", "b"]);
        // 空 / 非法
        assert!(parse_model_ids("not json").is_empty());
        assert!(parse_model_ids(r#"{"data":[]}"#).is_empty());
    }

    #[test]
    fn parse_models_gemini_style() {
        let ids = parse_model_ids(
            r#"{"models":[{"name":"models/gemini-2.5-pro"},{"name":"gemini-2.5-flash"}]}"#,
        );
        assert_eq!(ids, vec!["gemini-2.5-pro", "gemini-2.5-flash"]);
    }

    #[test]
    fn parse_models_deduplicates() {
        let ids = parse_model_ids(r#"{"data":[{"id":"x"},{"id":"x"}]}"#);
        assert_eq!(ids, vec!["x"]);
    }

    #[test]
    fn encode_path_segment_handles_slash() {
        assert_eq!(encode_path_segment("Qwen/Qwen3.7-Max"), "Qwen%2FQwen3.7-Max");
        assert_eq!(encode_path_segment("gemini-2.5-pro"), "gemini-2.5-pro");
    }

    #[test]
    fn truncate_limits_chars() {
        assert_eq!(truncate("abc", 5), "abc");
        assert!(truncate("一二三四五六", 4).ends_with('…'));
    }
}
