//! LlamaParse v2 REST API 客户端（multipart 一条龙 → 轮询 job）+ 级联增强。
//!
//! - 档位（每次请求必选）：fast(1 credit, 无 Markdown) / cost_effective(3) /
//!   agentic(10) / agentic_plus(45)。免费计划 10K credits/月，用完 402 硬停。
//!   48h 内完全相同的请求重解析 0 credits；换档位/改参数会 bust 缓存重新计费。
//! - 多 key 串行预算：见 [`super::budget`]（先用完第一把再切下一把）。
//! - 级联增强（cascade，仅 engine=mineru 本地文件）：MinerU 先识别，问题内容
//!   按类型/尺寸三级分桶交给 LlamaParse（原理与实测数据见 Hermes pdf-omni
//!   references/merge-strategy.md，2026-08-09）：
//!   桶1 表格+小图(bbox<25%页) → A4 原尺寸拼接（100% 细节，mermaid 可触发）
//!   桶2 大图/图表页(≥25%)   → 整页单独提交 + specialized_chart_parsing=agentic
//!   桶3 纯公式页(无表无图)   → 整页 2×2 网格拼（公式依赖上下文，97% 保留）

use std::path::Path;
use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};
use thiserror::Error;

use super::budget::{BudgetLedger, DEFAULT_BUDGET_PER_KEY};
use super::pdf_ops::{self, PdfOpsError};
use super::{ParseOutput, Tier};

const LLAMAPARSE_BASE: &str = "https://api.cloud.llamaindex.ai";
const LLAMAPARSE_UPLOAD: &str = "https://api.cloud.llamaindex.ai/api/v2/parse/upload";

/// 单文件大小上限（参照 Doc2X 兜底值；服务商未公布官方限制）
const MAX_BYTES: u64 = 300 * 1024 * 1024;

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_WAIT: Duration = Duration::from_secs(600);

#[derive(Debug, Error)]
pub enum LlamaParseError {
    #[error("文件 {size_mb}MB 超过 LlamaParse 单文件 300MB 上限，请先切分")]
    TooLarge { size_mb: u64 },
    #[error("LlamaParse 免费额度已用尽(402)")]
    QuotaExceeded,
    #[error("LlamaParse 预算不足: 本次任务估算 {est} credits，所有 key 剩余预算都放不下(可调高 budget_per_key 或拆分文件)")]
    BudgetInsufficient { est: u64 },
    #[error("LlamaParse 全部 API key 不可用(预算用尽或 402)")]
    AllKeysExhausted,
    #[error("LlamaParse API: {0}")]
    Api(String),
    #[error("LlamaParse 任务 {0}: {1}")]
    JobFailed(String, String),
    #[error("LlamaParse 解析超过 600s 未完成")]
    Timeout,
    #[error("HTTP: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("PDF 操作: {0}")]
    Pdf(#[from] PdfOpsError),
}

/// LlamaParse 配置：tokens 为逗号分隔的多 key（串行预算，先用完第一把）。
#[derive(Debug, Clone)]
pub struct LlamaParseConfig {
    pub tokens: String,
    pub tier: Tier,
}

impl LlamaParseConfig {
    fn token_list(&self) -> Vec<String> {
        self.tokens
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    }
}

/// 单文件解析（multipart 流式上传）。含多 key 串行预算与 402 兜底切换。
pub async fn parse_file(path: &Path, config: &LlamaParseConfig) -> Result<ParseOutput, LlamaParseError> {
    parse_file_with_expand(path, config, "markdown_full").await
}

async fn parse_file_with_expand(
    path: &Path,
    config: &LlamaParseConfig,
    expand: &str,
) -> Result<ParseOutput, LlamaParseError> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_BYTES {
        return Err(LlamaParseError::TooLarge {
            size_mb: size / 1024 / 1024,
        });
    }
    let rate = config.tier.rate();
    let est_pages = estimate_pages(path)?;
    let task_credits = est_pages.max(1) as u64 * rate;

    let tokens = config.token_list();
    if tokens.is_empty() {
        return Err(LlamaParseError::Api("未配置 LLAMAPARSE_API_KEY".into()));
    }
    let app_dir = bm_core::config::app_dir();
    let mut ledger = BudgetLedger::load(&app_dir, DEFAULT_BUDGET_PER_KEY);

    let mut last_err: Option<LlamaParseError> = None;
    let mut tried: Vec<String> = Vec::new();
    while tried.len() < tokens.len() {
        let Some((_, token)) = ledger.pick_key(&tokens, task_credits) else {
            if tried.is_empty() {
                return Err(LlamaParseError::BudgetInsufficient { est: task_credits });
            }
            break;
        };
        if tried.contains(&token) {
            break;
        }
        tried.push(token.clone());
        match upload_and_poll(&token, path, config, expand, None).await {
            Ok(mut out) => {
                let used = est_pages.max(out.pages.unwrap_or(1)) as u64 * rate;
                let total = ledger.record_usage(&token, used);
                out.used_key = Some(token.clone());
                tracing::info!(
                    event = "pdf_omni.llamaparse_usage",
                    credits_used = total,
                    budget = ledger.budget_per_key(),
                    key_index = tried.len(),
                );
                return Ok(out);
            }
            Err(LlamaParseError::QuotaExceeded) => {
                // 意外 402（用量与云端不同步，如其他端并发消耗）→ 标记用尽切下一把
                ledger.mark_exhausted(&token);
                last_err = Some(LlamaParseError::QuotaExceeded);
                continue;
            }
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    Err(last_err.unwrap_or(LlamaParseError::AllKeysExhausted))
}

/// 上传（multipart 流式）→ 轮询 job。`chart_parsing` 为 Some 时带上
/// specialized_chart_parsing（字符串档位，传布尔会 400）。
async fn upload_and_poll(
    token: &str,
    path: &Path,
    config: &LlamaParseConfig,
    expand: &str,
    chart_parsing: Option<&str>,
) -> Result<ParseOutput, LlamaParseError> {
    let client = reqwest::Client::new();
    let mut configuration = serde_json::json!({
        "tier": config.tier.name(),
        "version": "latest",
    });
    if let Some(cp) = chart_parsing {
        configuration["processing_options"] =
            json!({ "specialized_chart_parsing": cp });
    }
    let fname = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    // 流式 multipart（Part::file 内部走 tokio 流，不整体读入内存）
    let file_part = Part::file(path).await?.file_name(fname).mime_str("application/pdf")?;
    let form = Form::new()
        .part("file", file_part)
        .text("configuration", configuration.to_string());

    let resp = client
        .post(LLAMAPARSE_UPLOAD)
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::PAYMENT_REQUIRED {
        return Err(LlamaParseError::QuotaExceeded);
    }
    if !resp.status().is_success() {
        let code = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(LlamaParseError::Api(format!(
            "提交失败 HTTP {}: {}",
            code,
            &text[..text.len().min(300)]
        )));
    }
    let body: Value = resp.json().await?;
    let job_id = body["id"]
        .as_str()
        .ok_or_else(|| LlamaParseError::Api(format!("响应缺少 job id: {body}")))?
        .to_string();

    // 轮询 job 状态；expand 内容在响应顶层(data)，不在 job 里
    let status_url = format!("{LLAMAPARSE_BASE}/api/v2/parse/{job_id}?expand={expand}");
    let deadline = tokio::time::Instant::now() + MAX_WAIT;
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        if tokio::time::Instant::now() >= deadline {
            return Err(LlamaParseError::Timeout);
        }
        let r = client.get(&status_url).bearer_auth(token).send().await?;
        if r.status() == reqwest::StatusCode::PAYMENT_REQUIRED {
            return Err(LlamaParseError::QuotaExceeded);
        }
        if !r.status().is_success() {
            let code = r.status();
            let text = r.text().await.unwrap_or_default();
            return Err(LlamaParseError::Api(format!(
                "轮询失败 HTTP {}: {}",
                code,
                &text[..text.len().min(300)]
            )));
        }
        let data: Value = r.json().await?;
        let job = data.get("job").unwrap_or(&data);
        match job["status"].as_str().unwrap_or("PENDING").to_uppercase().as_str() {
            "COMPLETED" => {
                let md = if expand == "markdown_full" {
                    data["markdown_full"].as_str().unwrap_or("").to_string()
                } else {
                    join_pages_markdown(&data)
                };
                return Ok(ParseOutput {
                    markdown: md.clone(),
                    stats: super::verify::compute_stats(&md),
                    pages: Some(est_from_response(&data)),
                    tier: Some(config.tier),
                    job_id: Some(job_id),
                    chunked: false,
                    chunks: 1,
                    ..Default::default()
                });
            }
            "FAILED" | "CANCELLED" => {
                return Err(LlamaParseError::JobFailed(
                    job["status"].as_str().unwrap_or("FAILED").to_string(),
                    job["error_message"].as_str().unwrap_or("未知错误").to_string(),
                ));
            }
            _ => {
                tracing::info!(event = "pdf_omni.llamaparse_poll", status = %job["status"]);
            }
        }
    }
}

/// expand=markdown 时结果在顶层 data['markdown']['pages'][]。
fn join_pages_markdown(data: &Value) -> String {
    let pages = &data["markdown"];
    let pages_list = if pages.is_array() {
        pages.as_array().cloned().unwrap_or_default()
    } else {
        pages["pages"].as_array().cloned().unwrap_or_default()
    };
    pages_list
        .iter()
        .filter_map(|p| p["markdown"].as_str())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn est_from_response(data: &Value) -> usize {
    data["markdown"]["pages"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0)
}

/// 估算单文件计费页数：PDF 按实际页数（lopdf），失败/图片按 1 页。
fn estimate_pages(path: &Path) -> Result<usize, LlamaParseError> {
    if path
        .extension()
        .is_some_and(|e| e.to_string_lossy().to_lowercase() == "pdf")
    {
        Ok(pdf_ops::page_count(path).unwrap_or(1))
    } else {
        Ok(1)
    }
}

// ---------------------------------------------------------------------------
// 级联增强（三级分桶）
// ---------------------------------------------------------------------------

/// 级联增强：MinerU 先解析，问题内容按类型/尺寸分桶交给 LlamaParse 重解析。
///
/// 分桶策略（2026-08-09 实测确定，与 Hermes 版一致）：
/// - 表格元素 + 小图（bbox 面积 < 25% 页）→ 渲染图按原尺寸拼进 A4 画布
/// - 大图/图表页（≥25% 页）→ 整页单独提交（开 specialized_chart_parsing 触发 mermaid）
/// - 纯公式页（无表无图）→ 整页 2×2 网格拼（公式依赖上下文，97% 保留）
///
/// credits 账本 = ⌈拼接图组数⌉×费率 + 大图页数×费率 + ⌈公式页/4⌉×费率。
pub async fn cascade_enhance(
    primary: &ParseOutput,
    source_pdf: &Path,
    config: &LlamaParseConfig,
) -> Result<Value, LlamaParseError> {
    let Some(content_list) = &primary.content_list else {
        return Ok(json!({"enhanced": [], "note": "MinerU 未返回 content_list(分块/URL 模式不支持级联)"}));
    };
    let Some(images_dir) = &primary.images_dir else {
        return Ok(json!({"enhanced": [], "note": "MinerU 结果缺少 images 目录"}));
    };
    if config.token_list().is_empty() {
        return Err(LlamaParseError::Api("未配置 LLAMAPARSE_API_KEY".into()));
    }

    let rate = config.tier.rate();
    // 页面尺寸推断（从元素 bbox 最大值）
    let mut max_x = 612.0_f64;
    let mut max_y = 792.0_f64;
    for el in content_list {
        if let Some(bbox) = el["bbox"].as_array() {
            if bbox.len() >= 4 {
                max_x = max_x.max(bbox[2].as_f64().unwrap_or(0.0));
                max_y = max_y.max(bbox[3].as_f64().unwrap_or(0.0));
            }
        }
    }
    let page_area = max_x * max_y;
    let small_ratio = 0.25_f64;

    // 分桶
    let mut pack_items: Vec<(String, usize, String, Vec<f64>)> = Vec::new(); // (img, page, kind, bbox)
    let mut solo_pages: std::collections::BTreeSet<usize> = Default::default();
    let mut pages_with_media: std::collections::BTreeSet<usize> = Default::default();
    let mut pages_with_equations: std::collections::BTreeSet<usize> = Default::default();

    for el in content_list {
        let kind = el["type"].as_str().unwrap_or("");
        let pidx = el["page_idx"].as_u64().unwrap_or(0) as usize;
        let bbox: Vec<f64> = el["bbox"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_f64()).collect())
            .unwrap_or_else(|| vec![0.0, 0.0, 100.0, 100.0]);
        let w = (bbox.get(2).copied().unwrap_or(100.0) - bbox.get(0).copied().unwrap_or(0.0)).max(10.0);
        let h = (bbox.get(3).copied().unwrap_or(100.0) - bbox.get(1).copied().unwrap_or(0.0)).max(10.0);
        let img_path = el["img_path"].as_str().unwrap_or("");
        if (kind == "table" || kind == "image") && !img_path.is_empty() {
            let img = images_dir.join(img_path);
            if img.exists() {
                if kind == "table" || (w * h) / page_area < small_ratio {
                    pack_items.push((img.display().to_string(), pidx, kind.to_string(), bbox));
                } else {
                    solo_pages.insert(pidx);
                }
                pages_with_media.insert(pidx);
            }
        } else if kind == "equation" {
            pages_with_equations.insert(pidx);
        }
    }

    let total_pages = content_list
        .iter()
        .filter_map(|el| el["page_idx"].as_u64())
        .max()
        .map(|m| m as usize + 1)
        .unwrap_or(0);
    if pack_items.is_empty() && solo_pages.is_empty() && pages_with_equations.is_empty() {
        return Ok(json!({"enhanced": [], "note": "未发现表格/公式/图表元素, 无需级联增强"}));
    }

    let mut enhanced: Vec<Value> = Vec::new();
    let mut credits_used: u64 = 0;

    // ---- 桶1: 表格图 + 小图 → A4 原尺寸拼接 ---------------------------------
    if !pack_items.is_empty() {
        let tmp = super::mineru::temp_dir("cascade-pack");
        let img_paths: Vec<std::path::PathBuf> =
            pack_items.iter().map(|(p, _, _, _)| std::path::PathBuf::from(p)).collect();
        let packed = pdf_ops::pack_images_a4(&img_paths, &tmp)?;
        let n = packed.len().max(1);
        for (pi, packed_img) in packed.iter().enumerate() {
            let r = parse_image_with_chart(packed_img, config).await?;
            let start = pi * pack_items.len() / n;
            let end = (pi + 1) * pack_items.len() / n;
            let members = &pack_items[start..end];
            let md_p = &r.markdown;
            enhanced.push(json!({
                "mode": "packed",
                "packed_page": pi + 1,
                "pages": members.iter().map(|m| m.1 + 1).collect::<Vec<_>>(),
                "items": members.iter().map(|m| json!({"kind": m.2, "page": m.1 + 1})).collect::<Vec<_>>(),
                "has_mermaid": md_p.contains("mermaid"),
                "llamaparse_md": md_p.chars().take(4000).collect::<String>(),
                "llamaparse_chars": md_p.len(),
            }));
            credits_used += rate;
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---- 桶2: 大图/图表页 → 逐页单独提交（保细节触发 mermaid） ---------------
    if !solo_pages.is_empty() {
        for idx in &solo_pages {
            let tmp = super::mineru::temp_dir("cascade-solo");
            let sub = pdf_ops::extract_pages(source_pdf, &[*idx], &tmp)?;
            let r2 = parse_image_with_chart(&sub, config).await?;
            let _ = std::fs::remove_dir_all(&tmp);
            let md2 = &r2.markdown;
            enhanced.push(json!({
                "mode": "solo",
                "page": idx + 1,
                "has_image": true,
                "has_mermaid": md2.contains("mermaid"),
                "llamaparse_md": md2.chars().take(4000).collect::<String>(),
                "llamaparse_chars": md2.len(),
            }));
            credits_used += rate;
        }
    }

    // ---- 桶3: 纯公式页 → 整页 2×2 网格拼（公式依赖上下文） -------------------
    let grid_pages: Vec<usize> = pages_with_equations
        .difference(&pages_with_media)
        .copied()
        .collect();
    if !grid_pages.is_empty() {
        let tmp = super::mineru::temp_dir("cascade-grid");
        let (grid_pdf, groups) = pdf_ops::grid_merge_2x2(source_pdf, &grid_pages, &tmp)?;
        let r = parse_file_with_expand(&grid_pdf, config, "markdown").await?;
        let _ = std::fs::remove_dir_all(&tmp);
        let per_page: Vec<String> = r
            .markdown
            .split("\n\n")
            .map(|s| s.to_string())
            .collect();
        for (gi, group) in groups.iter().enumerate() {
            let llama_md = per_page.get(gi).cloned().unwrap_or_default();
            for idx in group {
                enhanced.push(json!({
                    "mode": "grid",
                    "page": idx + 1,
                    "has_equation": true,
                    "llamaparse_md": llama_md.chars().take(4000).collect::<String>(),
                    "llamaparse_chars": llama_md.len(),
                }));
            }
        }
        credits_used += groups.len() as u64 * rate;
    }

    // 排序（按页序）
    enhanced.sort_by(|a, b| {
        let pa = a["page"].as_u64().unwrap_or(0).min(a["packed_page"].as_u64().unwrap_or(0));
        let pb = b["page"].as_u64().unwrap_or(0).min(b["packed_page"].as_u64().unwrap_or(0));
        pa.cmp(&pb)
    });

    let problem_pages: std::collections::BTreeSet<usize> = enhanced
        .iter()
        .filter_map(|e| e["page"].as_u64().map(|p| p as usize))
        .chain(pack_items.iter().map(|m| m.1))
        .collect();

    Ok(json!({
        "enhanced": enhanced,
        "packed_items": pack_items.len(),
        "solo_pages": solo_pages.iter().map(|p| p + 1).collect::<Vec<_>>(),
        "grid_pages": grid_pages.iter().map(|p| p + 1).collect::<Vec<_>>(),
        "total_pdf_pages": total_pages,
        "tier": config.tier.name(),
        "rate_per_page": rate,
        "credits_used": credits_used,
        "credits_if_full_parse": problem_pages.len() as u64 * rate,
        "savings": format!(
            "级联成本 {credits_used} credits(表格/小图拼 A4 + 大图 {} 页单独 + 公式页拼), 问题页 {} 页全文跑要 {} credits",
            solo_pages.len(),
            problem_pages.len(),
            problem_pages.len() as u64 * rate,
        ),
        "note": "三级分桶: 表格/小图→A4原尺寸拼接(100%细节); 大图→单独(触发mermaid); 纯公式页→2×2网格拼。MinerU API 无元素级置信度字段, 判定为启发式。",
    }))
}

/// 图片/单页提交：开 specialized_chart_parsing（字符串档位，传布尔会 400）。
async fn parse_image_with_chart(
    path: &Path,
    config: &LlamaParseConfig,
) -> Result<ParseOutput, LlamaParseError> {
    parse_with_chart_parsing(path, config, "agentic").await
}

async fn parse_with_chart_parsing(
    path: &Path,
    config: &LlamaParseConfig,
    chart_parsing: &str,
) -> Result<ParseOutput, LlamaParseError> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_BYTES {
        return Err(LlamaParseError::TooLarge {
            size_mb: size / 1024 / 1024,
        });
    }
    let rate = config.tier.rate();
    let est_pages = estimate_pages(path)?;
    let task_credits = est_pages.max(1) as u64 * rate;

    let tokens = config.token_list();
    let app_dir = bm_core::config::app_dir();
    let mut ledger = BudgetLedger::load(&app_dir, DEFAULT_BUDGET_PER_KEY);

    let mut last_err: Option<LlamaParseError> = None;
    let mut tried: Vec<String> = Vec::new();
    while tried.len() < tokens.len() {
        let Some((_, token)) = ledger.pick_key(&tokens, task_credits) else {
            if tried.is_empty() {
                return Err(LlamaParseError::BudgetInsufficient { est: task_credits });
            }
            break;
        };
        if tried.contains(&token) {
            break;
        }
        tried.push(token.clone());
        let result = upload_and_poll(&token, path, config, "markdown_full", Some(chart_parsing)).await;
        match result {
            Ok(mut out) => {
                let used = est_pages.max(out.pages.unwrap_or(1)) as u64 * rate;
                ledger.record_usage(&token, used);
                out.used_key = Some(token.clone());
                return Ok(out);
            }
            Err(LlamaParseError::QuotaExceeded) => {
                ledger.mark_exhausted(&token);
                last_err = Some(LlamaParseError::QuotaExceeded);
                continue;
            }
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    Err(last_err.unwrap_or(LlamaParseError::AllKeysExhausted))
}
