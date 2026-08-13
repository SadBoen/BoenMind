//! MinerU Precision Extract API 客户端（官方云服务，需 token）。
//!
//! - 本地文件: POST /api/v4/file-urls/batch 申请签名 URL → PUT 流式上传 → 轮询
//! - 公网 URL:  POST /api/v4/extract/task 直接提交 → 轮询
//! - 限额: ≤200MB / ≤200 页/文件；超页数按 190 页切块逐块解析后合并（lopdf）
//! - 结果: 轮询 done 后下载 zip（full.md + content_list.json + images/）解压到临时目录
//!
//! 协议与 Hermes pdf-omni（Python 版）一致；实现全部 Rust（reqwest 流式上传/
//! 下载、zip crate 解压、lopdf 切分）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

use super::pdf_ops::{self, PdfOpsError};
use super::{MdStats, ParseOutput};

const MINERU_BASE: &str = "https://mineru.net";
const MINERU_UPLOAD_BATCH: &str = "https://mineru.net/api/v4/file-urls/batch";
const MINERU_EXTRACT_TASK: &str = "https://mineru.net/api/v4/extract/task";
const MINERU_BATCH_RESULT: &str = "https://mineru.net/api/v4/extract-results/batch";

/// 单文件限额（服务商要求）
const MAX_BYTES: u64 = 200 * 1024 * 1024;
const MAX_PAGES: usize = 200;
/// 切分块页数（留余量避免边界超限）
const CHUNK_PAGES: usize = 190;
/// 轮询间隔与单文件最长等待
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_WAIT: Duration = Duration::from_secs(600);

#[derive(Debug, Error)]
pub enum MineruError {
    #[error("文件 {size_mb}MB 超过 MinerU 单文件 200MB 上限，请先切分")]
    TooLarge { size_mb: u64 },
    #[error("MinerU API: {0}")]
    Api(String),
    #[error("MinerU 解析超过 600s 未完成")]
    Timeout,
    #[error("MinerU 结果缺少 full.md")]
    MissingMarkdown,
    #[error("HTTP: {0}")]
    Http(#[from] reqwest::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("PDF 操作: {0}")]
    Pdf(#[from] PdfOpsError),
}

#[derive(Debug, Clone)]
pub struct MineruConfig {
    pub token: String,
    pub model_version: String,
    pub is_ocr: bool,
    pub language: Option<String>,
}

pub type MineruResult<T> = Result<T, MineruError>;

/// 解析本地文件；>200 页自动切块逐块解析后合并。
pub async fn parse_file(path: &Path, config: MineruConfig) -> MineruResult<ParseOutput> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_BYTES {
        return Err(MineruError::TooLarge {
            size_mb: size / 1024 / 1024,
        });
    }
    let pages = pdf_ops::page_count(path).ok();
    if pages.is_some_and(|p| p > MAX_PAGES) {
        let total = pages.unwrap();
        let tmp = temp_dir("mineru-chunk");
        let parts = pdf_ops::split_by_pages(path, CHUNK_PAGES, &tmp)?;
        let mut results = Vec::new();
        for part in &parts {
            results.push(parse_one_file(part, &config).await?);
        }
        let md = merge_markdown(&results);
        let _ = std::fs::remove_dir_all(&tmp);
        return Ok(ParseOutput {
            markdown: md.clone(),
            stats: md_stats(&md),
            pages: Some(total),
            chunked: true,
            chunks: results.len(),
            ..Default::default()
        });
    }
    parse_one_file(path, &config).await
}

/// 单个文件：签名上传 → 轮询 → zip 解压。
async fn parse_one_file(path: &Path, config: &MineruConfig) -> MineruResult<ParseOutput> {
    let client = reqwest::Client::new();
    let fname = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    // 1. 申请签名上传 URL
    let mut payload = serde_json::json!({
        "files": [{ "name": fname }],
        "model_version": config.model_version,
    });
    if config.is_ocr {
        payload["files"][0]["is_ocr"] = Value::Bool(true);
    }
    if let Some(lang) = &config.language {
        payload["language"] = Value::String(lang.clone());
    }
    let resp = client
        .post(MINERU_UPLOAD_BATCH)
        .bearer_auth(&config.token)
        .json(&payload)
        .send()
        .await?;
    let resp = check_status(resp).await?;
    let body: Value = resp.json().await?;
    if body["code"] != 0 {
        return Err(MineruError::Api(format!(
            "申请上传失败: {}",
            body["msg"].as_str().unwrap_or("未知错误")
        )));
    }
    let batch_id = body["data"]["batch_id"]
        .as_str()
        .ok_or_else(|| MineruError::Api("响应缺少 batch_id".into()))?
        .to_string();
    let file_url = body["data"]["file_urls"][0]
        .as_str()
        .ok_or_else(|| MineruError::Api("响应缺少 file_urls".into()))?
        .to_string();

    // 2. 流式 PUT 上传（文件对象直传，不整体读入内存）
    let file = tokio::fs::File::open(path).await?;
    let up = client.put(&file_url).body(file).send().await?;
    if !up.status().is_success() {
        let code = up.status();
        let text = up.text().await.unwrap_or_default();
        return Err(MineruError::Api(format!(
            "上传失败 HTTP {}: {}",
            code,
            &text[..text.len().min(300)]
        )));
    }

    // 3. 轮询批量结果
    let state_url = format!("{MINERU_BATCH_RESULT}/{batch_id}");
    let zip_url = poll_batch(&client, &config.token, &state_url).await?;

    // 4. 下载 zip 并解压
    extract_result_zip(&client, &zip_url).await
}

/// 公网 URL 直接提交任务并轮询。
pub async fn parse_url(url: &str, config: MineruConfig) -> MineruResult<ParseOutput> {
    let client = reqwest::Client::new();
    let mut payload = serde_json::json!({ "url": url, "model_version": config.model_version });
    if config.is_ocr {
        payload["is_ocr"] = Value::Bool(true);
    }
    if let Some(lang) = &config.language {
        payload["language"] = Value::String(lang.clone());
    }
    let resp = client
        .post(MINERU_EXTRACT_TASK)
        .bearer_auth(&config.token)
        .json(&payload)
        .send()
        .await?;
    let resp = check_status(resp).await?;
    let body: Value = resp.json().await?;
    if body["code"] != 0 {
        return Err(MineruError::Api(format!(
            "提交失败: {}",
            body["msg"].as_str().unwrap_or("未知错误")
        )));
    }
    let task_id = body["data"]["task_id"]
        .as_str()
        .ok_or_else(|| MineruError::Api("响应缺少 task_id".into()))?
        .to_string();

    let state_url = format!("{MINERU_BASE}/api/v4/extract/task/{task_id}");
    let zip_url = poll_task(&client, &config.token, &state_url).await?;
    extract_result_zip(&client, &zip_url).await
}

/// 校验响应状态码；非 2xx 读取 body 摘要并报错（消费响应）。
async fn check_status(resp: reqwest::Response) -> MineruResult<reqwest::Response> {
    if !resp.status().is_success() {
        let code = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(MineruError::Api(format!(
            "HTTP {}: {}",
            code,
            &text[..text.len().min(300)]
        )));
    }
    Ok(resp)
}

async fn poll_batch(
    client: &reqwest::Client,
    token: &str,
    state_url: &str,
) -> MineruResult<String> {
    let deadline = tokio::time::Instant::now() + MAX_WAIT;
    while tokio::time::Instant::now() < deadline {
        let resp = client.get(state_url).bearer_auth(token).send().await?;
        let resp = check_status(resp).await?;
        let body: Value = resp.json().await?;
        if body["code"] != 0 {
            return Err(MineruError::Api(format!(
                "查询失败: {}",
                body["msg"].as_str().unwrap_or("未知错误")
            )));
        }
        let results = &body["data"]["extract_result"];
        if let Some(r) = results.as_array().and_then(|a| a.first()) {
            match r["state"].as_str().unwrap_or("pending") {
                "done" => {
                    return r["full_zip_url"]
                        .as_str()
                        .map(str::to_string)
                        .ok_or_else(|| MineruError::Api("done 但缺少 full_zip_url".into()));
                }
                "failed" => {
                    return Err(MineruError::Api(format!(
                        "解析失败: {}",
                        r["err_msg"].as_str().unwrap_or("未知错误")
                    )));
                }
                state => {
                    let prog = &r["extract_progress"];
                    tracing::info!(
                        event = "pdf_omni.mineru_progress",
                        state,
                        extracted = prog["extracted_pages"].as_u64().unwrap_or(0),
                        total = prog["total_pages"].as_u64().unwrap_or(0),
                    );
                }
            }
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    Err(MineruError::Timeout)
}

async fn poll_task(client: &reqwest::Client, token: &str, state_url: &str) -> MineruResult<String> {
    let deadline = tokio::time::Instant::now() + MAX_WAIT;
    while tokio::time::Instant::now() < deadline {
        let resp = client.get(state_url).bearer_auth(token).send().await?;
        let resp = check_status(resp).await?;
        let body: Value = resp.json().await?;
        if body["code"] != 0 {
            return Err(MineruError::Api(format!(
                "查询失败: {}",
                body["msg"].as_str().unwrap_or("未知错误")
            )));
        }
        let er = &body["data"]["extract_result"];
        match er["state"].as_str().unwrap_or("pending") {
            "done" => {
                return er["full_zip_url"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| MineruError::Api("done 但缺少 full_zip_url".into()));
            }
            "failed" => {
                return Err(MineruError::Api(format!(
                    "解析失败: {}",
                    er["err_msg"].as_str().unwrap_or("未知错误")
                )));
            }
            _ => {}
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    Err(MineruError::Timeout)
}

/// 下载结果 zip → 解压到临时目录 → 读取 full.md / content_list.json（保留 images/）。
async fn extract_result_zip(
    client: &reqwest::Client,
    zip_url: &str,
) -> MineruResult<ParseOutput> {
    let resp = client.get(zip_url).send().await?;
    let resp = check_status(resp).await?;
    let bytes = resp.bytes().await?;

    let tmp = temp_dir("mineru-result");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec()))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        // zip slip 防护：拒绝越界路径
        let target = Path::new(&tmp).join(&name);
        let Ok(normalized) = target.canonicalize().or_else(|_| {
            // 目录可能尚不存在，做词法归一化
            Ok::<_, std::io::Error>(lexical_normalize(&target))
        }) else {
            continue;
        };
        if !normalized.starts_with(&tmp) {
            tracing::warn!(event = "pdf_omni.zip_slip_blocked", name);
            continue;
        }
        if entry.is_dir() {
            let _ = std::fs::create_dir_all(&normalized);
            continue;
        }
        if let Some(parent) = normalized.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut out = std::io::BufWriter::new(std::fs::File::create(&normalized)?);
        std::io::copy(&mut entry, &mut out)?;
    }

    let md_path = tmp.join("full.md");
    if !md_path.exists() {
        return Err(MineruError::MissingMarkdown);
    }
    let md = std::fs::read_to_string(&md_path)?;
    // content_list.json（级联增强用）；可能带前缀（如 1712_content_list.json）
    let content_list = read_content_list(&tmp);
    Ok(ParseOutput {
        markdown: md.clone(),
        stats: md_stats(&md),
        pages: None,
        chunked: false,
        chunks: 1,
        content_list,
        images_dir: Some(tmp),
        ..Default::default()
    })
}

/// 词法路径归一化（不触发 IO），用于 zip 条目路径检查。
fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn read_content_list(tmp: &Path) -> Option<Vec<Value>> {
    let entries = std::fs::read_dir(tmp).ok()?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.ends_with("_content_list.json") {
            if let Ok(text) = std::fs::read_to_string(e.path()) {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if let Some(arr) = v.as_array() {
                        return Some(arr.clone());
                    }
                }
            }
        }
    }
    None
}

fn merge_markdown(parts: &[ParseOutput]) -> String {
    parts
        .iter()
        .filter_map(|p| {
            let md = p.markdown.trim();
            if md.is_empty() { None } else { Some(md.to_string()) }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn md_stats(md: &str) -> MdStats {
    super::verify::compute_stats(md)
}

/// 唯一临时目录（uuid 后缀；std 方案，避免新增 tempfile 生产依赖）。
pub(crate) fn temp_dir(prefix: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("bm-{prefix}-{}", uuid::Uuid::new_v4()));
    dir
}
