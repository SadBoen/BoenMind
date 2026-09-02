//! 排序融合与去重 — 与 Python 版 providers/fusion.py 逐行等价移植。
//!
//! - `normalize_url`:URL 规范化(小写 host、去 www/尾斜杠/hash、丢跟踪参数)
//! - `rrf_fuse`:Reciprocal Rank Fusion,得分 = Σ 1/(60+rank)
//! - `merge_mirrors`:同题镜像合并(CJK 二字 bigram + 拉丁词,Jaccard ≥ 0.9)

use std::collections::{BTreeSet, HashMap};

use url::Url;

/// 常见跟踪参数:规范化 URL 时丢弃(同一页面不再被算成多条)。
const TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "fbclid",
    "gclid",
    "mc_cid",
    "mc_eid",
    "yclid",
    "igshid",
    "ref",
    "ref_src",
    "spm",
    "from",
    "source",
    "wfr",
    "_ga",
];

pub const RRF_K: f64 = 60.0;
pub const MIRROR_THRESHOLD: f64 = 0.9;

/// 参与融合的单条结果(源内顺序即排名)。
#[derive(Debug, Clone)]
pub struct RawItem {
    pub title: String,
    pub url: String,
    pub description: String,
    pub source: String,
}

/// 融合后的条目。
#[derive(Debug, Clone)]
pub struct FusedItem {
    pub url: String,
    pub title: String,
    pub description: String,
    pub sources: BTreeSet<String>,
    pub score: f64,
    pub hits: u32,
}

/// 规范化 URL 用于去重:小写 host、去 www、去尾斜杠、去 hash、丢跟踪参数。
/// 无 scheme/netloc 或解析失败时退回原值(Python 版 urlparse 同款宽容语义)。
pub fn normalize_url(url: &str) -> String {
    let raw = url.trim();
    let parsed = match Url::parse(raw) {
        Ok(u) => u,
        Err(_) => return raw.to_string(),
    };
    if parsed.host_str().is_none() {
        return raw.to_string();
    }
    let host = {
        let h = parsed.host_str().unwrap_or_default().to_lowercase();
        h.strip_prefix("www.").map(String::from).unwrap_or(h)
    };
    let path = {
        let p = parsed.path();
        let trimmed = p.trim_end_matches('/');
        if trimmed.is_empty() {
            "/"
        } else {
            trimmed
        }
    };
    // query:解析 → 丢跟踪参数 → 排序 → 重编码
    let mut keep: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    keep.retain(|(k, _)| !TRACKING_PARAMS.contains(&k.to_lowercase().as_str()));
    keep.sort();
    let query = if keep.is_empty() {
        String::new()
    } else {
        let mut ser = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in &keep {
            ser.append_pair(k, v);
        }
        ser.finish()
    };
    let scheme = parsed.scheme();
    let port = match parsed.port() {
        Some(p) if is_default_port(scheme, p) => String::new(),
        Some(p) => format!(":{p}"),
        None => String::new(),
    };
    if query.is_empty() {
        format!("{scheme}://{host}{port}{path}")
    } else {
        format!("{scheme}://{host}{port}{path}?{query}")
    }
}

fn is_default_port(scheme: &str, port: u16) -> bool {
    matches!((scheme, port), ("http", 80) | ("https", 443))
}

/// Reciprocal Rank Fusion:把各源的有序结果列表融合成一个排序。
/// URL(规范化后)相同条目合并:贡献源并入 sources,保留更完整的
/// title/description;得分 = Σ 1/(k+rank)。排序:得分降序,平手时命中源数降序。
pub fn rrf_fuse(per_source: &[Vec<RawItem>], k: f64) -> Vec<FusedItem> {
    let mut groups: HashMap<String, FusedItem> = HashMap::new();
    for results in per_source {
        let mut rank: u64 = 0;
        for r in results {
            let url = r.url.trim();
            if url.is_empty() {
                continue;
            }
            let key = normalize_url(url);
            if key.is_empty() {
                continue;
            }
            rank += 1;
            let g = groups.entry(key).or_insert_with(|| FusedItem {
                url: url.to_string(),
                title: r.title.clone(),
                description: r.description.clone(),
                sources: BTreeSet::new(),
                score: 0.0,
                hits: 0,
            });
            g.hits += 1;
            g.score += 1.0 / (k + rank as f64);
            g.sources.insert(r.source.clone());
            if r.title.chars().count() > g.title.chars().count() {
                g.title = r.title.clone();
            }
            if r.description.chars().count() > g.description.chars().count() {
                g.description = r.description.clone();
            }
        }
    }
    let mut out: Vec<FusedItem> = groups.into_values().collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.hits.cmp(&a.hits))
    });
    out
}

fn is_cjk(ch: char) -> bool {
    let c = ch as u32;
    (0x4E00..=0x9FFF).contains(&c) || (0x3400..=0x4DBF).contains(&c)
}

/// 标题 token 化:CJK 连续段切成相邻二字 bigram,拉丁/数字切成小写词。
pub fn tokenize_title(title: &str) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    let cleaned: String = title.to_lowercase();
    let chars: Vec<char> = cleaned.chars().collect();
    for i in 0..chars.len() {
        let ch = chars[i];
        let nxt = chars.get(i + 1).copied().unwrap_or('\0');
        if is_cjk(ch) && is_cjk(nxt) {
            tokens.insert(format!("{ch}{nxt}"));
        }
    }
    static WORD_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = WORD_RE.get_or_init(|| regex::Regex::new(r"[a-z0-9]+").expect("静态正则"));
    for m in re.find_iter(&cleaned) {
        tokens.insert(m.as_str().to_string());
    }
    tokens
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    inter as f64 / (a.len() + b.len() - inter) as f64
}

/// 合并同题镜像:转载站点常用近似标题转发同一内容。
/// 仅当两个标题都至少有 2 个 token 时才参与比较;合并时来源集合并集、
/// 补齐空缺的 title/description。
pub fn merge_mirrors(items: Vec<FusedItem>, threshold: f64) -> Vec<FusedItem> {
    let mut kept: Vec<FusedItem> = Vec::new();
    let mut token_sets: Vec<BTreeSet<String>> = Vec::new();
    for it in items {
        let toks = tokenize_title(&it.title);
        let mut mirror_idx: Option<usize> = None;
        if toks.len() >= 2 {
            for (i, ts) in token_sets.iter().enumerate() {
                if ts.len() >= 2 && jaccard(ts, &toks) >= threshold {
                    mirror_idx = Some(i);
                    break;
                }
            }
        }
        match mirror_idx {
            Some(i) => {
                let orig = &mut kept[i];
                orig.sources.extend(it.sources);
                if orig.title.is_empty() {
                    orig.title = it.title;
                }
                if orig.description.is_empty() {
                    orig.description = it.description;
                }
            }
            None => {
                kept.push(it);
                token_sets.push(toks);
            }
        }
    }
    kept
}

/// 输出标准结构:description 前缀标注来源集合,如 `[ddgs|jina|searxng]`。
pub fn annotate(items: Vec<FusedItem>, limit: usize) -> Vec<serde_json::Value> {
    items
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(i, m)| {
            let srcs: Vec<&str> = m.sources.iter().map(String::as_str).collect();
            let prefix = format!("[{}] ", srcs.join("|"));
            let title = m.title.clone();
            let mut desc = m.description.clone();
            if !desc.is_empty() {
                desc = format!("{prefix}{desc}");
                serde_json::json!({
                    "title": title, "url": m.url,
                    "description": desc, "position": i + 1,
                })
            } else if !title.is_empty() {
                serde_json::json!({
                    "title": format!("{prefix}{title}"), "url": m.url,
                    "description": "", "position": i + 1,
                })
            } else {
                serde_json::json!({
                    "title": prefix.trim_end(), "url": m.url,
                    "description": "", "position": i + 1,
                })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(title: &str, url: &str, source: &str) -> RawItem {
        RawItem {
            title: title.into(),
            url: url.into(),
            description: String::new(),
            source: source.into(),
        }
    }

    #[test]
    fn normalize_strips_tracking_and_www() {
        assert_eq!(
            normalize_url("https://WWW.Example.com/a/b/?utm_source=x&id=2#frag"),
            "https://example.com/a/b?id=2"
        );
        assert_eq!(
            normalize_url("https://example.com/"),
            "https://example.com/"
        );
        // 无 scheme → 退回原值
        assert_eq!(normalize_url("example.com/x"), "example.com/x");
        // 参数排序
        assert_eq!(
            normalize_url("https://e.com/?b=1&a=2"),
            "https://e.com/?a=2&b=1"
        );
    }

    #[test]
    fn rrf_multi_source_hit_tops() {
        let a = vec![
            raw("A1", "https://e.com/1", "s1"),
            raw("A2", "https://e.com/2", "s1"),
        ];
        let b = vec![RawItem {
            title: "B1".into(),
            url: "https://e.com/1".into(),
            description: "更长的描述内容".into(),
            source: "s2".into(),
        }];
        let fused = rrf_fuse(&[a, b], 60.0);
        assert_eq!(fused[0].url, "https://e.com/1");
        assert_eq!(fused[0].hits, 2);
        assert_eq!(fused[0].sources.len(), 2);
        assert_eq!(fused[0].description, "更长的描述内容");
        assert_eq!(fused[1].url, "https://e.com/2");
    }

    #[test]
    fn rrf_rank_skips_empty_urls() {
        let a = vec![
            raw("空 url 应跳过且不占 rank", "  ", "s1"),
            raw("真第一", "https://e.com/1", "s1"),
        ];
        let fused = rrf_fuse(&[a], 60.0);
        assert_eq!(fused.len(), 1);
        assert!((fused[0].score - 1.0 / 61.0).abs() < 1e-9);
    }

    #[test]
    fn mirrors_merge_cjk_titles() {
        let items = vec![
            FusedItem {
                url: "https://a.com/x".into(),
                title: "大语言模型评测报告发布".into(),
                description: String::new(),
                sources: BTreeSet::from(["s1".to_string()]),
                score: 1.0,
                hits: 1,
            },
            FusedItem {
                url: "https://b.com/mirror".into(),
                title: "大语言模型评测报告发布!".into(),
                description: "镜像补充".into(),
                sources: BTreeSet::from(["s2".to_string()]),
                score: 0.9,
                hits: 1,
            },
        ];
        let merged = merge_mirrors(items, 0.9);
        // bigram 重合度高 → 合并,来源并集,描述补齐
        assert_eq!(merged.len(), 1, "近似标题应合并");
        assert_eq!(merged[0].sources.len(), 2);
        assert_eq!(merged[0].description, "镜像补充");
    }

    #[test]
    fn single_token_titles_never_merge() {
        let items = vec![
            FusedItem {
                url: "https://a.com/".into(),
                title: "Home".into(),
                description: String::new(),
                sources: BTreeSet::from(["s1".to_string()]),
                score: 1.0,
                hits: 1,
            },
            FusedItem {
                url: "https://b.com/".into(),
                title: "home page".into(),
                description: String::new(),
                sources: BTreeSet::from(["s2".to_string()]),
                score: 0.9,
                hits: 1,
            },
        ];
        // "Home" 仅 1 token;"home page" 两词也仅 2 token 但与前者 jaccard 低
        // ——此处断言不误合泛化标题
        assert_eq!(merge_mirrors(items, 0.9).len(), 2);
    }

    #[test]
    fn annotate_prefixes_and_positions() {
        let items = vec![FusedItem {
            url: "https://e.com/".into(),
            title: "标题".into(),
            description: "描述".into(),
            sources: BTreeSet::from(["b".to_string(), "a".to_string()]),
            score: 1.0,
            hits: 2,
        }];
        let out = annotate(items, 5);
        assert_eq!(out[0]["position"], 1);
        assert_eq!(out[0]["description"], "[a|b] 描述");
    }
}
