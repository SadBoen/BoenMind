//! 供应商注册表 + 通用搜索引擎 — web_multisearch 的「可扩展供应商」底座。
//!
//! 设计目标(2026-09-04 用户裁决):不再把供应商写死在代码里,而是把每家
//! 「怎么发请求、怎么拆结果」变成一份可编辑的 Provider 模板。内置 12 家
//! 默认预填好(值沿用原 sources.rs 各家的正确默认);用户可在设置页新增
//! 全新供应商(接口地址 / 方式 / key 传法 / 参数名 / 结果路径 / 字段映射)。
//!
//! 解析类型约定:`parse` 字段是内置特例的调度键(searxng/ddg/jina/marginalia),
//! 其余内置与全部自定义一律 `"std"` → 通用 JSON 适配器 `run_generic`。
//! 内置每家**不是写死**——endpoint/key 等字段都可由配置覆盖,只是默认正确。

use serde_json::{json, Value};

use crate::config::Config;
use crate::keys::{split_keys, with_key_rotation, HttpErr};

/// 单条源结果(全模块共用)。
#[derive(Debug, Clone)]
pub struct Item {
    pub title: String,
    pub url: String,
    pub description: String,
}

/// 一家供应商的可编辑描述(对应设置页下拉选中后展开的那份表单)。
#[derive(Debug, Default, Clone)]
pub struct Provider {
    /// 稳定标识(内置=英文名,如 "serper";自定义=uuid 或随机短串)。
    pub id: String,
    /// 显示名(内置中文名或英文名;自定义由用户起)。
    pub name: String,
    /// 是否出厂内置(内置可编辑字段,但不可删除)。
    pub builtin: bool,
    /// 接口地址。
    pub endpoint: String,
    /// 请求方式 "GET" | "POST"。
    pub method: String,
    /// key 传法:"header" | "bearer" | "query" | "none"。
    pub auth: String,
    /// auth=header/query 时的参数名(如 "X-API-KEY" / "api_key")。
    pub auth_name: String,
    /// key 值(secret;"," 逗号分隔多把,内置自动轮换)。
    pub key: String,
    /// query 用的参数名(如 "q" / "query")。
    pub query_param: String,
    /// limit 用的参数名(可空,空则不发 limit)。
    pub limit_param: String,
    /// 结果数组的 JSON 路径(pointer 语法,如 "/organic"、"/data/webPages/value")。
    pub results_path: String,
    /// 标题字段名。
    pub title_field: String,
    /// 链接字段名。
    pub url_field: String,
    /// 摘要字段名。
    pub desc_field: String,
    /// 解析特例:"std"(通用 JSON)或内置调度键(jina/ddg/searxng/marginalia)。
    pub parse: String,
    /// 月度配额(0 = 不限;进度条右侧分子/分母)。
    pub quota: i64,
}

/// 内置提供的 id → 默认模板。endpoint/key 等皆可被配置覆盖;parse 只在
/// 内置集合里才有意义(自定义必为 "std")。
pub fn builtin_templates() -> Vec<Provider> {
    vec![
        Provider {
            id: "searxng".into(),
            name: "SearXNG".into(),
            builtin: true,
            endpoint: "".into(),
            method: "GET".into(),
            auth: "none".into(),
            auth_name: "".into(),
            key: "".into(),
            query_param: "q".into(),
            limit_param: "".into(),
            results_path: "/results".into(),
            title_field: "title".into(),
            url_field: "url".into(),
            desc_field: "content".into(),
            parse: "searxng".into(),
            quota: 0,
        },
        Provider {
            id: "ddgs".into(),
            name: "DuckDuckGo".into(),
            builtin: true,
            endpoint: "https://html.duckduckgo.com/html/".into(),
            method: "POST".into(),
            auth: "none".into(),
            auth_name: "".into(),
            key: "".into(),
            query_param: "q".into(),
            limit_param: "".into(),
            results_path: "/".into(),
            title_field: "title".into(),
            url_field: "url".into(),
            desc_field: "description".into(),
            parse: "ddg".into(),
            quota: 0,
        },
        Provider {
            id: "jina".into(),
            name: "Jina".into(),
            builtin: true,
            endpoint: "https://s.jina.ai".into(),
            method: "GET".into(),
            auth: "bearer".into(),
            auth_name: "Authorization".into(),
            key: "".into(),
            query_param: "q".into(),
            limit_param: "".into(),
            results_path: "/".into(),
            title_field: "title".into(),
            url_field: "url".into(),
            desc_field: "description".into(),
            parse: "jina".into(),
            quota: 0,
        },
        Provider {
            id: "marginalia".into(),
            name: "Marginalia".into(),
            builtin: true,
            endpoint: "https://api2.marginalia-search.com/search".into(),
            method: "GET".into(),
            auth: "none".into(),
            auth_name: "".into(),
            key: "".into(),
            query_param: "query".into(),
            limit_param: "count".into(),
            results_path: "/results".into(),
            title_field: "title".into(),
            url_field: "url".into(),
            desc_field: "description".into(),
            parse: "marginalia".into(),
            quota: 0,
        },
        Provider {
            id: "serper".into(),
            name: "Serper".into(),
            builtin: true,
            endpoint: "https://google.serper.dev/search".into(),
            method: "POST".into(),
            auth: "header".into(),
            auth_name: "X-API-KEY".into(),
            key: "".into(),
            query_param: "q".into(),
            limit_param: "num".into(),
            results_path: "/organic".into(),
            title_field: "title".into(),
            url_field: "link".into(),
            desc_field: "snippet".into(),
            parse: "std".into(),
            quota: 0,
        },
        Provider {
            id: "tavily".into(),
            name: "Tavily".into(),
            builtin: true,
            endpoint: "https://api.tavily.com/search".into(),
            method: "POST".into(),
            // 2026-09-05 修正:旧默认 "none" 走通用适配器时 Key 永不随请求
            // 发出,填真 Key 也 401;Tavily 现行 API 支持 Bearer 头。
            auth: "bearer".into(),
            auth_name: "Authorization".into(),
            key: "".into(),
            query_param: "query".into(),
            limit_param: "max_results".into(),
            results_path: "/results".into(),
            title_field: "title".into(),
            url_field: "url".into(),
            desc_field: "content".into(),
            parse: "tavily".into(),
            quota: 0,
        },
        Provider {
            id: "exa".into(),
            name: "Exa".into(),
            builtin: true,
            endpoint: "https://api.exa.ai/search".into(),
            method: "POST".into(),
            auth: "header".into(),
            auth_name: "x-api-key".into(),
            key: "".into(),
            query_param: "query".into(),
            limit_param: "numResults".into(),
            results_path: "/results".into(),
            title_field: "title".into(),
            url_field: "url".into(),
            desc_field: "text".into(),
            parse: "std".into(),
            quota: 0,
        },
        Provider {
            id: "brave".into(),
            name: "Brave".into(),
            builtin: true,
            endpoint: "https://api.search.brave.com/res/v1/web/search".into(),
            method: "GET".into(),
            auth: "header".into(),
            auth_name: "X-Subscription-Token".into(),
            key: "".into(),
            query_param: "q".into(),
            limit_param: "count".into(),
            results_path: "/web/results".into(),
            title_field: "title".into(),
            url_field: "url".into(),
            desc_field: "description".into(),
            parse: "std".into(),
            quota: 0,
        },
        Provider {
            id: "langsearch".into(),
            name: "LangSearch".into(),
            builtin: true,
            endpoint: "https://api.langsearch.com/v1/web-search".into(),
            method: "POST".into(),
            auth: "bearer".into(),
            auth_name: "Authorization".into(),
            key: "".into(),
            query_param: "query".into(),
            limit_param: "count".into(),
            results_path: "/data/webPages/value".into(),
            title_field: "name".into(),
            url_field: "url".into(),
            desc_field: "snippet".into(),
            parse: "std".into(),
            quota: 0,
        },
        Provider {
            id: "linkup".into(),
            name: "Linkup".into(),
            builtin: true,
            endpoint: "https://api.linkup.so/v1/search".into(),
            method: "POST".into(),
            auth: "bearer".into(),
            auth_name: "Authorization".into(),
            key: "".into(),
            query_param: "q".into(),
            // 2026-09-05 修正:旧默认 "depth" 恒发数字 depth:3,上游只认
            // "standard"/"deep" 必 400;清空则不发 limit。
            limit_param: "".into(),
            results_path: "/results".into(),
            title_field: "name".into(),
            url_field: "url".into(),
            desc_field: "content".into(),
            parse: "std".into(),
            quota: 0,
        },
        Provider {
            id: "you".into(),
            name: "You.com".into(),
            builtin: true,
            endpoint: "https://ydc-index.io/v1/search".into(),
            method: "GET".into(),
            auth: "header".into(),
            auth_name: "x-api-key".into(),
            key: "".into(),
            query_param: "query".into(),
            limit_param: "count".into(),
            results_path: "/results".into(),
            title_field: "title".into(),
            url_field: "url".into(),
            desc_field: "description".into(),
            parse: "std".into(),
            quota: 0,
        },
        Provider {
            id: "websearchapi".into(),
            name: "WebSearchAPI".into(),
            builtin: true,
            endpoint: "https://api.websearchapi.ai/ai-search".into(),
            method: "POST".into(),
            auth: "bearer".into(),
            auth_name: "Authorization".into(),
            key: "".into(),
            query_param: "query".into(),
            limit_param: "maxResults".into(),
            results_path: "/organic".into(),
            title_field: "title".into(),
            url_field: "url".into(),
            desc_field: "description".into(),
            parse: "std".into(),
            quota: 0,
        },
    ]
}

/// 读配置里的 `providers` 数组,与内置模板合并成最终可用集。
///
/// - 配置里 id 与内置相同 → 用内置模板,再以配置中出现的字段覆盖;
/// - 配置里 id 不在内置 → 新增供应商(parse 强制 "std");
/// - 配置里没有 providers 数组 / 数组为空 → 回退到全部内置模板(向后兼容);
/// - 条目 `enabled:false`(停用)或 `deleted:true`(删除内置的墓碑)→
///   不进可用集;墓碑同时抑制下方「缺失内置回填」,否则删了又长回来。
///
/// 旧的平铺 key(searxng_url / *_api_key)若存在且某家内置 key/endpoint 为空,
/// 则以平铺值为该家兜底(平滑迁移,不破坏已填配置)。
pub fn resolve_providers(cfg: &mut Config) -> Vec<Provider> {
    let templates = builtin_templates();
    let raw = cfg.raw_providers();

    if raw.is_empty() {
        let mut out: Vec<Provider> = Vec::new();
        for t in &templates {
            let mut p = t.clone();
            apply_legacy_key(&mut p, cfg);
            out.push(p);
        }
        return out;
    }

    let mut out: Vec<Provider> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for entry in raw {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if id.is_empty() {
            continue;
        }
        seen.push(id.clone());
        // 停用/已删:先进 seen(抑制模板回填)再跳过,不进可用集
        if entry_flag(&entry, "deleted") == Some(true)
            || entry_flag(&entry, "enabled") == Some(false)
        {
            continue;
        }
        let templ = templates.iter().find(|t| t.id.as_str() == id.as_str());
        let mut p = if let Some(t) = templ {
            t.clone()
        } else {
            Provider {
                id: id.clone(),
                name: id.clone(),
                parse: "std".into(),
                ..Provider::default()
            }
        };
        apply_overrides(&mut p, &entry);
        apply_legacy_key(&mut p, cfg);
        // 存量配置迁移:配置里若固化着旧缺陷默认值(tavily auth:"none" /
        // linkup limit_param:"depth",2026-09-04 全量真连探针发现),回退到
        // 修正后的内置模板值;用户显式改过的其他取值不受影响。
        if let Some(t) = templ {
            if p.id == "tavily" && p.auth == "none" {
                p.auth = t.auth.clone();
                p.auth_name = t.auth_name.clone();
            }
            if p.id == "linkup" && p.limit_param == "depth" {
                p.limit_param = t.limit_param.clone();
            }
        }
        out.push(p);
    }
    // 内置但未出现在 providers 数组里的,补到末尾(避免误删内置;
    // 墓碑/停用条目已占位 seen,不会被回填)
    for t in &templates {
        if !seen.iter().any(|s| s.as_str() == t.id.as_str()) {
            let mut p = t.clone();
            apply_legacy_key(&mut p, cfg);
            out.push(p);
        }
    }
    out
}

/// 管理面单查(真搜测试用):含停用家,但已删墓碑不给测(UI 上删除态只提供恢复)。
pub fn resolve_any(cfg: &mut Config, wanted: &str) -> Option<Provider> {
    let templates = builtin_templates();
    for entry in cfg.raw_providers() {
        let id = entry.get("id").and_then(Value::as_str).unwrap_or_default();
        if id != wanted {
            continue;
        }
        if entry_flag(&entry, "deleted") == Some(true) {
            return None;
        }
        let templ = templates.iter().find(|t| t.id.as_str() == id);
        let mut p = if let Some(t) = templ {
            t.clone()
        } else {
            Provider {
                id: id.to_string(),
                name: id.to_string(),
                parse: "std".into(),
                ..Provider::default()
            }
        };
        apply_overrides(&mut p, &entry);
        apply_legacy_key(&mut p, cfg);
        return Some(p);
    }
    templates.iter().find(|t| t.id.as_str() == wanted).cloned()
}

fn entry_flag(entry: &Value, key: &str) -> Option<bool> {
    entry.get(key).and_then(Value::as_bool)
}

/// 用配置条目覆盖模板字段(仅覆盖字符串/数字/布尔能表达的字段)。
fn apply_overrides(p: &mut Provider, entry: &Value) {
    fn s(entry: &Value, key: &str, cur: &mut String) {
        if let Some(v) = entry.get(key).and_then(Value::as_str) {
            if !v.is_empty() {
                *cur = v.to_string();
            }
        }
    }
    s(entry, "name", &mut p.name);
    s(entry, "endpoint", &mut p.endpoint);
    s(entry, "method", &mut p.method);
    s(entry, "auth", &mut p.auth);
    s(entry, "auth_name", &mut p.auth_name);
    s(entry, "key", &mut p.key);
    s(entry, "query_param", &mut p.query_param);
    s(entry, "limit_param", &mut p.limit_param);
    s(entry, "results_path", &mut p.results_path);
    s(entry, "title_field", &mut p.title_field);
    s(entry, "url_field", &mut p.url_field);
    s(entry, "desc_field", &mut p.desc_field);
    s(entry, "parse", &mut p.parse);
    if let Some(v) = entry.get("builtin").and_then(Value::as_bool) {
        p.builtin = v;
    }
    if let Some(n) = entry.get("quota").and_then(|v| v.as_i64()) {
        p.quota = n;
    }
}

/// 旧的平铺 key 兜底:内置家有 key/endpoint 为空时,读对应旧字段。
fn apply_legacy_key(p: &mut Provider, cfg: &mut Config) {
    match p.id.as_str() {
        "searxng" => {
            // SearXNG 的「endpoint」即其地址;旧字段是 searxng_url
            if p.endpoint.is_empty() {
                p.endpoint = cfg.get_str("searxng_url");
            }
        }
        "ddgs" | "marginalia" => {} // 免 key
        other => {
            if p.key.is_empty() {
                let legacy = format!("{other}_api_key");
                p.key = cfg.get_str(&legacy);
            }
        }
    }
}

/// 一家可用的 key 列表(逗号分隔,去空白去空)。
pub fn provider_keys(p: &Provider) -> Vec<String> {
    split_keys(&p.key)
}

/// 该家是否可用(内置免 key 的按协议恒可用;其余需 key 或 endpoint)。
pub fn is_available(p: &Provider) -> bool {
    match p.parse.as_str() {
        "ddg" | "marginalia" => true,
        "searxng" => !p.endpoint.is_empty(),
        _ => !provider_keys(p).is_empty(),
    }
}

// ---------------------------------------------------------------------------
// 通用 JSON 适配器(全新供应商走这里;内置家一般走各自特殊解析)
// ---------------------------------------------------------------------------

pub async fn run_generic(
    client: &reqwest::Client,
    p: &Provider,
    query: &str,
    limit: usize,
) -> Result<Vec<Item>, String> {
    let keys = provider_keys(p);
    let limit = limit.clamp(1, 50);
    let resp = if keys.is_empty() {
        let p = p.clone();
        let q = query.to_string();
        let r = build_generic(client, &p, &q, limit, "")
            .send()
            .await
            .map_err(|e| format!("Could not reach {}: {e}", p.name))?;
        check_status(r, p.name.as_str()).await.map_err(|e| match e {
            HttpErr::Status(_, m) => m,
            HttpErr::Other(m) => m,
        })
    } else {
        with_key_rotation(&keys, |key| {
            let client = client.clone();
            let p = p.clone();
            let query = query.to_string();
            async move {
                let r = build_generic(&client, &p, &query, limit, &key)
                    .send()
                    .await
                    .map_err(|e| HttpErr::Other(format!("Could not reach {}: {e}", p.name)))?;
                check_status(r, p.name.as_str()).await
            }
        })
        .await
    }
    .map_err(|e| format!("{}: {e}", p.name))?;
    let data: Value = resp
        .json()
        .await
        .map_err(|e| format!("{} response is not JSON: {e}", p.name))?;
    let results = data
        .pointer(&p.results_path)
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(results
        .iter()
        .take(limit)
        .filter(|it| !j(it, p.url_field.as_str()).is_empty())
        .map(|it| Item {
            title: {
                let t = j(it, p.title_field.as_str());
                if t.is_empty() {
                    j(it, p.url_field.as_str())
                } else {
                    t
                }
            },
            url: j(it, p.url_field.as_str()),
            description: j(it, p.desc_field.as_str()).chars().take(500).collect(),
        })
        .collect())
}

/// 把一家 Provider 的 endpoint + query/limit + key 拼成 reqwest 请求。
/// - GET :endpoint?query_param={query}[&limit_param={limit}],key 按 auth 放置
/// - POST:JSON body {"<query_param>": query, "<limit_param>"?: limit},key 按 auth 放置
fn build_generic(
    client: &reqwest::Client,
    p: &Provider,
    query: &str,
    limit: usize,
    key: &str,
) -> reqwest::RequestBuilder {
    let is_get = p.method.to_lowercase() == "get";
    let mut qpairs: Vec<(&str, String)> = Vec::new();
    let mut body: Value = json!({});

    if is_get {
        qpairs.push((p.query_param.as_str(), query.to_string()));
        if !p.limit_param.is_empty() {
            qpairs.push((p.limit_param.as_str(), limit.to_string()));
        }
        if p.auth.as_str() == "query" && !key.is_empty() {
            qpairs.push((p.auth_name.as_str(), key.to_string()));
        }
    } else if let Some(o) = body.as_object_mut() {
        o.insert(p.query_param.clone(), Value::String(query.to_string()));
        if !p.limit_param.is_empty() {
            o.insert(p.limit_param.clone(), json!(limit));
        }
        if p.auth.as_str() == "query" && !key.is_empty() {
            o.insert(p.auth_name.clone(), Value::String(key.to_string()));
        }
    }

    let mut builder = if is_get {
        client
            .get(p.endpoint.as_str())
            .query(&qpairs)
            .timeout(std::time::Duration::from_secs(20))
    } else {
        client
            .post(p.endpoint.as_str())
            .json(&body)
            .timeout(std::time::Duration::from_secs(20))
    };

    if !key.is_empty() {
        match p.auth.as_str() {
            "header" => builder = builder.header(p.auth_name.as_str(), key),
            "bearer" => builder = builder.header("Authorization", format!("Bearer {key}")),
            _ => {} // query/none:已在上方注入,header 无需再加
        }
    }
    builder
}

fn j(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 非 2xx → HttpErr::Status(2xx 返回原 resp;供轮换识别 401/403/429)。
async fn check_status(resp: reqwest::Response, name: &str) -> Result<reqwest::Response, HttpErr> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    Err(HttpErr::Status(
        status.as_u16(),
        format!("{name} returned HTTP {}", status.as_u16()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_templates_cover_all_twelve() {
        let ts = builtin_templates();
        assert_eq!(ts.len(), 12, "应恰好 12 内置");
        let ids: Vec<String> = ts.iter().map(|t| t.id.clone()).collect();
        for want in [
            "searxng",
            "ddgs",
            "jina",
            "marginalia",
            "serper",
            "tavily",
            "exa",
            "brave",
            "langsearch",
            "linkup",
            "you",
            "websearchapi",
        ] {
            assert!(
                ids.iter().any(|s| s.as_str() == want),
                "缺内置 {want}: {ids:?}"
            );
        }
    }

    #[test]
    fn provider_keys_splits_comma() {
        let p = Provider {
            key: " a,b , c ".into(),
            ..Provider::default()
        };
        assert_eq!(
            provider_keys(&p),
            ["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn availability_rules() {
        // 免 key 内置恒可用
        let mut ddg = Provider::default();
        ddg.parse = "ddg".into();
        assert!(is_available(&ddg));
        // searxng 须有 endpoint
        let mut sx = Provider::default();
        sx.parse = "searxng".into();
        assert!(!is_available(&sx));
        sx.endpoint = "http://127.0.0.1:8080".into();
        assert!(is_available(&sx));
        // 需 key 的:无 key 不可用
        let serper = Provider {
            parse: "std".into(),
            ..Provider::default()
        };
        assert!(!is_available(&serper));
    }

    #[test]
    fn resolve_providers_merges_custom_and_builtin() {
        // 配置含一个自定义供应商(custom1)+ 覆盖内置 tavily 的 key;
        // 未列出的内置(如 serper)仍保留在末尾。
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg_path = dir.path().join("mcp.json");
        std::fs::write(
            &cfg_path,
            serde_json::json!({
                "providers": [
                    {
                        "id": "tavily",
                        "key": "tv-key",
                        "quota": 1000,
                    },
                    {
                        "id": "custom1",
                        "name": "MySearch",
                        "endpoint": "https://api.example.com/search",
                        "method": "POST",
                        "auth": "header",
                        "auth_name": "X-API-KEY",
                        "key": "my-key",
                        "query_param": "q",
                        "limit_param": "num",
                        "results_path": "/organic",
                        "title_field": "title",
                        "url_field": "link",
                        "desc_field": "snippet",
                    },
                ]
            })
            .to_string(),
        )
        .unwrap();
        let mut cfg = Config::new(Some(cfg_path));
        let providers = resolve_providers(&mut cfg);

        // tavily:内置模板 + 配置覆盖 key/quota
        let tav = providers.iter().find(|p| p.id == "tavily").expect("tavily");
        assert_eq!(tav.key, "tv-key");
        assert_eq!(tav.quota, 1000);
        assert!(tav.builtin);
        assert_eq!(tav.endpoint, "https://api.tavily.com/search");

        // custom1:全新供应商,强制 std 解析
        let c = providers
            .iter()
            .find(|p| p.id == "custom1")
            .expect("custom1");
        assert_eq!(c.name, "MySearch");
        assert_eq!(c.parse, "std");
        assert!(!c.builtin);
        assert_eq!(c.results_path, "/organic");

        // 未列出的内置(serper)仍在末尾
        assert!(providers.iter().any(|p| p.id == "serper"));
        // 总数 = 12 内置 + 1 自定义
        assert_eq!(providers.len(), 13);
    }

    #[test]
    fn empty_providers_falls_back_to_all_builtin() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg_path = dir.path().join("mcp.json");
        std::fs::write(&cfg_path, r#"{}"#).unwrap();
        let mut cfg = Config::new(Some(cfg_path));
        let providers = resolve_providers(&mut cfg);
        assert_eq!(providers.len(), 12);
    }

    #[test]
    fn disabled_entry_skipped_and_not_refilled() {
        // enabled:false → 不进可用集,且模板回填不得把它加回来
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg_path = dir.path().join("mcp.json");
        std::fs::write(
            &cfg_path,
            serde_json::json!({ "providers": [ { "id": "serper", "enabled": false } ] })
                .to_string(),
        )
        .unwrap();
        let mut cfg = Config::new(Some(cfg_path));
        let providers = resolve_providers(&mut cfg);
        assert!(
            !providers.iter().any(|p| p.id == "serper"),
            "停用家不得出现"
        );
        assert_eq!(providers.len(), 11, "其余 11 家内置仍在");
        // 管理面单查:停用家仍可测(真搜测试用)
        assert!(resolve_any(&mut cfg, "serper").is_some());
    }

    #[test]
    fn deleted_tombstone_suppresses_template_refill() {
        // deleted:true 墓碑 → 删除内置生效,且不被「缺失内置回填」复活
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg_path = dir.path().join("mcp.json");
        std::fs::write(
            &cfg_path,
            serde_json::json!({ "providers": [ { "id": "jina", "deleted": true } ] }).to_string(),
        )
        .unwrap();
        let mut cfg = Config::new(Some(cfg_path));
        let providers = resolve_providers(&mut cfg);
        assert!(!providers.iter().any(|p| p.id == "jina"), "墓碑家不得复活");
        assert_eq!(providers.len(), 11);
        // 管理面单查:墓碑不给测
        assert!(resolve_any(&mut cfg, "jina").is_none());
        // 其余内置照常
        assert!(providers.iter().any(|p| p.id == "serper"));
    }

    #[test]
    fn resolve_any_finds_custom_without_polluting_active_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg_path = dir.path().join("mcp.json");
        std::fs::write(
            &cfg_path,
            serde_json::json!({ "providers": [
                { "id": "custom1", "name": "MySearch", "endpoint": "https://x.example/search" },
            ] })
            .to_string(),
        )
        .unwrap();
        let mut cfg = Config::new(Some(cfg_path));
        let p = resolve_any(&mut cfg, "custom1").expect("custom1");
        assert_eq!(p.name, "MySearch");
        assert_eq!(p.parse, "std");
        // 主流程可用集不受影响:custom1 有效 + 12 内置
        let active = resolve_providers(&mut cfg);
        assert_eq!(active.len(), 13);
    }
}
