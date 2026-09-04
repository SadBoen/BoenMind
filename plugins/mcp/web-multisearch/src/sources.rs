//! 搜索源执行 + 聚合器 — 基于 cascade 的供应商注册表驱动。
//!
//! 每个执行源 = cascade::Provider 的一个可用实例:
//! - `parse == "std"` → 通用 JSON 适配器(cascade::run_generic),覆盖 serper/
//!   tavily/exa/brave/langsearch/linkup/you/websearchapi 与全部自定义供应商。
//! - `parse` 为内置特例(searxng/ddg/jina/marginalia)→ 各自特殊解析(HTML/
//!   markdown / SearXNG JSON),保留原实现。
//!
//! 聚合逻辑沿用原版:每源取 limit+2 条,RRF 融合 + CJK 同题镜像合并后截回
//! limit;全局 25s 兜底,超时源直接丢弃;单源失败绝不拖垮聚合。meta 带
//! 各源耗时遥测。
//!
//! ddgs 说明:走系统 curl 子进程抓 DDG HTML(免 key),reqwest 直连仅降级;
//! 失败不影响聚合。与 Python 版 ddgs 库语义一致(指纹伪装层面本版为尽力而为)。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use futures::stream::{FuturesUnordered, StreamExt};
use serde_json::{json, Value};

use crate::cascade::{self, Item, Provider};
use crate::fusion::{annotate, merge_mirrors, rrf_fuse, RawItem, MIRROR_THRESHOLD, RRF_K};

/// 整体等待上限:各源内部已有 12-30s 超时,这里兜底,超时源直接丢弃。
const GLOBAL_TIMEOUT: Duration = Duration::from_secs(25);

/// 单源执行:按 parse 分派(特殊内置解析 vs 通用 JSON)。返回 Vec<Item>。
pub async fn run_source(
    client: &reqwest::Client,
    p: &Provider,
    query: &str,
    limit: usize,
) -> Result<Vec<Item>, String> {
    match p.parse.as_str() {
        "searxng" => searxng(client, p, query, limit).await,
        "ddg" => ddgs(client, query, limit).await,
        "jina" => jina_search(client, p, query, limit).await,
        "marginalia" => marginalia(client, query, limit).await,
        _ => cascade::run_generic(client, p, query, limit).await,
    }
}

// ---------------------------------------------------------------------------
// 聚合器(原有语义保留;输入改为已解析的 provider 列表)
// ---------------------------------------------------------------------------

/// 聚合:source_names 是已解析的可用 provider id 列表,providers 是完整集。
pub async fn aggregate(
    client: &reqwest::Client,
    providers: &[Provider],
    name: &str,
    query: &str,
    limit: usize,
) -> Value {
    let available: Vec<&Provider> = providers
        .iter()
        .filter(|p| cascade::is_available(p))
        .collect();
    if available.is_empty() {
        return json!({
            "success": false,
            "error": format!(
                "{name}: no source available (configured sources); check API keys and endpoints"
            )
        });
    }

    // 每源取 limit+2 条:RRF 融合去重后截回 limit
    let per_source_limit = limit + 2;
    let deadline = tokio::time::Instant::now() + GLOBAL_TIMEOUT;

    let mut futs = FuturesUnordered::new();
    for p in &available {
        let client = client.clone();
        let p = (*p).clone();
        let query = query.to_string();
        let limit = per_source_limit;
        futs.push(async move {
            let started = Instant::now();
            let out = run_source(&client, &p, &query, limit).await;
            let ms = started.elapsed().as_millis() as u64;
            (p.id.clone(), ms, out)
        });
    }

    let mut per_source: Vec<Vec<RawItem>> = Vec::new();
    let mut timings: BTreeMap<String, u64> = BTreeMap::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    let sleep = tokio::time::sleep_until(deadline);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => {
                // 全局兜底到点:未完成的源标记超时丢弃
                for p in &available {
                    if !timings.contains_key(&p.id) {
                        failed.push((p.id.clone(), "global timeout".into()));
                    }
                }
                break;
            }
            item = futs.next() => {
                let Some((id, ms, out)) = item else { break; };
                timings.insert(id.clone(), ms);
                match out {
                    Ok(items) if !items.is_empty() => {
                        per_source.push(items.into_iter().map(|it| RawItem {
                            title: it.title, url: it.url,
                            description: it.description, source: id.clone(),
                        }).collect());
                    }
                    Ok(_) => failed.push((id.clone(), "no results".into())),
                    Err(e) => failed.push((id.clone(), e)),
                }
            }
        }
    }

    if per_source.is_empty() {
        let flat: Vec<String> = failed
            .iter()
            .map(|(n, e)| format!("('{n}', '{e}')"))
            .collect();
        return json!({
            "success": false,
            "error": format!("{name}: all sources failed: [{}]", flat.join(", "))
        });
    }

    // RRF 融合 → 同题镜像合并 → 截断输出
    let fused = rrf_fuse(&per_source, RRF_K);
    let fused = merge_mirrors(fused, MIRROR_THRESHOLD);
    let unique = fused.len();
    let web = annotate(fused, limit);
    let sources_ok: Vec<String> = timings
        .keys()
        .filter(|n| !failed.iter().any(|(f, _)| f.as_str() == n.as_str()))
        .cloned()
        .collect();
    json!({
        "success": true,
        "data": { "web": web },
        "meta": {
            "mode": name,
            "sources_ok": sources_ok,
            "sources_failed": failed.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
            "timings_ms": timings,
            "unique": unique,
        },
    })
}

// ---------------------------------------------------------------------------
// 内置特例解析器(保留原实现;入参改为 &Provider 以适配新模型)
// ---------------------------------------------------------------------------

async fn searxng(
    client: &reqwest::Client,
    p: &Provider,
    query: &str,
    limit: usize,
) -> Result<Vec<Item>, String> {
    let base = p.endpoint.trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("searxng: endpoint 未配置".into());
    }
    let resp = client
        .get(format!("{base}/search"))
        .query(&[("q", query), ("format", "json")])
        .timeout(Duration::from_secs(12))
        .send()
        .await
        .map_err(|e| format!("searxng: {e}"))?;
    let resp = resp
        .error_for_status()
        .map_err(|e| format!("searxng: {e}"))?;
    let data: Value = resp.json().await.map_err(|e| format!("searxng: {e}"))?;
    let results = data.get("results").and_then(Value::as_array);
    Ok(to_items(
        results.map(|a| a.as_slice()).unwrap_or_default(),
        limit,
        ("title", "url", "content"),
    ))
}

/// DuckDuckGo HTML 端点抓取(免 key;href 常为 /l/?uddg= 跳转,需解包)。
///
/// 走**系统 curl 子进程**:reqwest(rustls/Schannel 指纹)会被 DDG 发人机验证页,
/// 系统 curl 指纹可通过——Python 版 ddgs 库同理靠 primp 伪装过检。curl 于
/// Win10+/Linux/macOS 均为系统自带;无 curl 时降级回 reqwest 直连。
async fn ddgs(_client: &reqwest::Client, query: &str, limit: usize) -> Result<Vec<Item>, String> {
    let html = match fetch_via_curl(query).await {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[web-multisearch] ddgs curl 路径失败({e}),降级 reqwest 直连");
            let resp = ddgs_fallback_client()
                .post("https://html.duckduckgo.com/html/")
                .form(&[("q", query), ("kl", "wt-wt")])
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .map_err(|e| format!("ddgs: {e}"))?;
            let resp = resp.error_for_status().map_err(|e| format!("ddgs: {e}"))?;
            resp.text().await.map_err(|e| format!("ddgs: {e}"))?
        }
    };
    #[cfg(feature = "ddg_debug_dump")]
    {
        use std::io::Write;
        if let Ok(mut f) = std::fs::File::create("ddg_last.html") {
            let _ = f.write_all(html.as_bytes());
        }
    }
    parse_ddg_html(&html, limit)
}

/// 经系统 curl 抓取 DDG HTML 页(--data-urlencode 由 curl 负责编码;
/// 单元素 argv 传递,无 shell,无注入面)。
async fn fetch_via_curl(query: &str) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    let mut child = tokio::process::Command::new("curl")
        .args([
            "-s",
            "-m",
            "15",
            "-A",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36",
            "--data-urlencode",
            &format!("q={query}"),
            "--data-urlencode",
            "kl=wt-wt",
            "https://html.duckduckgo.com/html/",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("curl 不可用: {e}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "curl stdout 不可读".to_string())?;
    let mut html = Vec::new();
    let read_fut = stdout.read_to_end(&mut html);
    let status_fut = child.wait();
    let (status, _) = tokio::join!(status_fut, read_fut);
    let status = status.map_err(|e| format!("curl 执行失败: {e}"))?;
    if !status.success() {
        return Err(format!("curl 退出码 {:?}", status.code()));
    }
    Ok(String::from_utf8_lossy(&html).into_owned())
}

/// 降级直连客户端(native-tls;见 ddgs 注释)。
fn ddgs_fallback_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .use_native_tls()
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36",
            )
            .build()
            .expect("ddgs 降级客户端构造")
    })
}

fn parse_ddg_html(html: &str, limit: usize) -> Result<Vec<Item>, String> {
    use regex::Regex;
    static A_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static SNIP_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let a_re = A_RE.get_or_init(|| {
        Regex::new(r#"<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
            .expect("静态正则")
    });
    let snip_re = SNIP_RE.get_or_init(|| {
        Regex::new(r#"<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"#).expect("静态正则")
    });
    let snippets: Vec<String> = snip_re
        .captures_iter(html)
        .map(|c| clean_html_fragment(&c[1]))
        .collect();
    let mut out = Vec::new();
    for (i, cap) in a_re.captures_iter(html).enumerate() {
        if out.len() >= limit {
            break;
        }
        let mut url = cap[1].trim().to_string();
        if let Some(idx) = url.find("uddg=") {
            let enc = &url[idx + 5..];
            let end = enc.find('&').unwrap_or(enc.len());
            url = percent_decode(&enc[..end]);
        } else if url.starts_with("//") {
            url = format!("https:{url}");
        }
        if url.is_empty() {
            continue;
        }
        out.push(Item {
            title: clean_html_fragment(&cap[2]),
            url,
            description: snippets.get(i).cloned().unwrap_or_default(),
        });
    }
    if out.is_empty() {
        // 诊断信息只含长度与标记位,不落查询词与页面正文
        return Err(format!(
            "ddgs: no results (body_len={}, has_result_class={})",
            html.len(),
            html.contains("result__a")
        ));
    }
    Ok(out)
}

fn clean_html_fragment(s: &str) -> String {
    use regex::Regex;
    static TAG_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let tag_re = TAG_RE.get_or_init(|| Regex::new(r"<[^>]*>").expect("静态正则"));
    let no_tags = tag_re.replace_all(s, "");
    no_tags
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}

fn percent_decode(s: &str) -> String {
    fn hex_val(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Jina Search(s.jina.ai,markdown 输出解析;逗号多 Key 轮换)。
async fn jina_search(
    client: &reqwest::Client,
    p: &Provider,
    query: &str,
    limit: usize,
) -> Result<Vec<Item>, String> {
    use crate::keys::{split_keys, with_key_rotation, HttpErr};
    let keys = split_keys(&p.key);
    if keys.is_empty() {
        return Err("JINA_API_KEY is not set".into());
    }
    let resp = with_key_rotation(&keys, |key| {
        let client = client.clone();
        let query = query.to_string();
        async move {
            let r = client
                .get(p.endpoint.as_str())
                .query(&[("q", query)])
                .header("Authorization", format!("Bearer {key}"))
                .header("Accept", "text/markdown")
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(|e| HttpErr::Other(format!("Could not reach Jina Search: {e}")))?;
            check_status(r, "Jina Search").await
        }
    })
    .await?;
    let text = resp.text().await.map_err(|e| format!("Jina Search: {e}"))?;
    Ok(parse_jina_markdown(&text, limit))
}

/// Jina markdown 输出的 best-effort 解析(Python _parse_search_markdown 移植)。
fn parse_jina_markdown(text: &str, limit: usize) -> Vec<Item> {
    use regex::Regex;
    static LINK_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let link_re = LINK_RE.get_or_init(|| Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").expect("静态正则"));
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let is_result_start = |ln: &str| link_re.is_match(ln) || ln.starts_with("###");

    let mut results: Vec<Item> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() && results.len() < limit {
        let line = lines[i];
        if line.is_empty() {
            i += 1;
            continue;
        }
        let (title, url) = if let Some(m) = link_re.captures(line) {
            let t = m[1].trim();
            (
                if t.is_empty() {
                    line.to_string()
                } else {
                    t.to_string()
                },
                m[2].trim().to_string(),
            )
        } else if line.starts_with("###") {
            let title = line.trim_start_matches('#').trim().to_string();
            let mut jdx = i + 1;
            while jdx < lines.len() && lines[jdx].is_empty() {
                jdx += 1;
            }
            let url = if jdx < lines.len() && !lines[jdx].is_empty() && !is_result_start(lines[jdx])
            {
                lines[jdx].to_string()
            } else {
                String::new()
            };
            i = jdx; // 与 Python 一致:无条件前进到下一个非空行
            (title, url)
        } else {
            i += 1;
            continue;
        };

        // 收集后续描述,直到下一个结果标记
        let mut desc_parts: Vec<&str> = Vec::new();
        let mut jdx = i + 1;
        while jdx < lines.len() && !lines[jdx].is_empty() && !is_result_start(lines[jdx]) {
            desc_parts.push(lines[jdx]);
            jdx += 1;
        }
        i = jdx;

        if url.is_empty() {
            continue;
        }
        let description: String = desc_parts.join(" ").chars().take(500).collect();
        results.push(Item {
            title,
            url,
            description,
        });
    }
    results
}

/// Marginalia(公共免 key;limit 用 count 参数)。
async fn marginalia(
    client: &reqwest::Client,
    query: &str,
    limit: usize,
) -> Result<Vec<Item>, String> {
    let resp = client
        .get("https://api2.marginalia-search.com/search")
        .query(&[
            ("query", query.to_string()),
            ("count", limit.clamp(1, 50).to_string()),
        ])
        .header("api-key", "public")
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(12))
        .send()
        .await
        .map_err(|e| format!("Could not reach Marginalia: {e}"))?;
    let status = resp.status();
    let data: Value = resp
        .json()
        .await
        .map_err(|e| format!("Marginalia response is not JSON: {e}"))?;
    if status.is_client_error() || status.is_server_error() {
        return Err(format!("Marginalia returned HTTP {}", status.as_u16()));
    }
    let raw = data
        .get("results")
        .and_then(Value::as_array)
        .ok_or("Unexpected Marginalia response shape")?;
    let mut out = Vec::new();
    for it in raw {
        let url = j(it, "url").trim().to_string();
        if url.is_empty() {
            continue;
        }
        let title = {
            let t = j(it, "title");
            if t.is_empty() {
                url.clone()
            } else {
                t
            }
        };
        out.push(Item {
            title,
            url,
            description: j(it, "description"),
        });
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

fn to_items(arr: &[Value], limit: usize, fields: (&str, &str, &str)) -> Vec<Item> {
    arr.iter()
        .take(limit)
        .filter(|it| !j(it, fields.1).is_empty())
        .map(|it| Item {
            title: j(it, fields.0),
            url: j(it, fields.1),
            description: j(it, fields.2),
        })
        .collect()
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

use crate::keys::HttpErr;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jina_markdown_both_shapes() {
        let md = "以下是搜索结果:\n\n### First Result\nhttps://e.com/one\n这是描述文字。\n\n### Second\nhttps://e.com/two\n更多描述\n\n不相关段落\n[Linked](https://e.com/three)\n链接描述行\n";
        let out = parse_jina_markdown(md, 10);
        assert_eq!(out.len(), 3, "应解析 3 条,实际 {:?}", out);
        assert_eq!(out[0].title, "First Result");
        assert_eq!(out[0].url, "https://e.com/one");
        assert_eq!(out[0].description, "这是描述文字。");
        assert_eq!(out[2].title, "Linked");
        assert_eq!(out[2].description, "链接描述行");
    }

    #[test]
    fn jina_markdown_limit_and_preamble_skip() {
        let md =
            "# 概述\n无结果标记的行\n\n### Only\nhttps://e.com/1\n\n### Two\nhttps://e.com/2\n";
        let out = parse_jina_markdown(md, 1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].url, "https://e.com/1");
    }

    #[test]
    fn jina_header_claims_following_bare_url() {
        let md = "### NoUrl\n### Real\nhttps://e.com/real\n描述\n";
        assert_eq!(parse_jina_markdown(md, 10).len(), 0);
        let md2 = "### Only\nhttps://e.com/only\n这是描述\n\n尾部\n";
        let out = parse_jina_markdown(md2, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].url, "https://e.com/only");
        assert_eq!(out[0].description, "这是描述");
    }

    #[test]
    fn ddg_html_parse_and_uddg_unwrap() {
        let html = r##"<div><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa&amp;rut=xyz">Example <b>Site</b></a>
<a class="result__snippet" href="#">这是 <b>摘要</b> 内容 &amp; 更多</a>
<a rel="nofollow" class="result__a" href="https://direct.example.org/b">Direct</a></div>"##;
        let out = parse_ddg_html(html, 5).expect("应解析出结果");
        assert_eq!(out[0].url, "https://example.com/a");
        assert_eq!(out[0].title, "Example Site");
        assert_eq!(out[0].description, "这是 摘要 内容 & 更多");
        assert_eq!(out[1].url, "https://direct.example.org/b");
    }

    #[test]
    fn ddg_no_results_is_error() {
        assert!(parse_ddg_html("<html>anomaly page</html>", 5).is_err());
    }

    #[test]
    fn percent_decode_utf8() {
        assert_eq!(percent_decode("a%20b%2Bc"), "a b+c");
        assert_eq!(percent_decode("%E4%B8%AD%E6%96%87"), "中文");
        assert_eq!(percent_decode("plain"), "plain");
    }
}
