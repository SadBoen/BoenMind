//! pdf-omni 插件核心（Rust 实现）——MinerU(主) + LlamaParse(交叉验证/级联) PDF 解析。
//!
//! 原理吸收自 Hermes pdf-omni 插件（Python 版，2026-08 实测结论），全部重写为
//! Rust（reqwest/lopdf/zip/image），服务商 API（MinerU Precision / LlamaParse v2）
//! 保持不变。差异与增强：
//! - 多 key 串行预算：先用完 key1 再切 key2（避免交替轮换被风控识别），
//!   任务前精确估算防止单任务越线撞 402（用户 2026-08 决策）
//! - verify 交叉验证：双引擎 Jaccard 相似度 + 统计差异报告
//! - cascade 级联增强：表格/小图→A4 原尺寸拼接（100% 细节）、大图页→单独提交
//!   （specialized_chart_parsing 触发 mermaid）、纯公式页→2×2 网格拼页（97% 保留）
//! - mineru-refine 式后处理：伪标题/跨页断句/页面家具/空表/残留标记修复
//! - Doc2X 引擎已封禁（2026-08），不移植
//!
//! 调用方：`routes/pdf_omni.rs` 的 `POST /api/plugins/pdf-omni/parse` 端点
//! （TS 薄壳插件经 loopback 调用）。API keys 由调用方传入（从插件设置文件读取）。

pub mod budget;
pub mod llamaparse;
pub mod mineru;
pub mod pdf_ops;
pub mod refine;
pub mod verify;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use llamaparse::LlamaParseConfig;
pub use mineru::MineruConfig;

/// 引擎枚举（Doc2X 已封禁，仅 MinerU/LlamaParse）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Mineru,
    Llamaparse,
    Auto,
}

impl Engine {
    pub fn name(&self) -> &'static str {
        match self {
            Engine::Mineru => "mineru",
            Engine::Llamaparse => "llamaparse",
            Engine::Auto => "auto",
        }
    }
}

/// LlamaParse 档位（费率 credits/页：fast=1, cost_effective=3, agentic=10, agentic_plus=45）。
/// fast 不输出 Markdown，勿用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Fast,
    CostEffective,
    Agentic,
    AgenticPlus,
}

impl Tier {
    pub fn rate(&self) -> u64 {
        match self {
            Tier::Fast => 1,
            Tier::CostEffective => 3,
            Tier::Agentic => 10,
            Tier::AgenticPlus => 45,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Fast => "fast",
            Tier::CostEffective => "cost_effective",
            Tier::Agentic => "agentic",
            Tier::AgenticPlus => "agentic_plus",
        }
    }
}

impl Default for Tier {
    fn default() -> Self {
        // 实测质量最优且更快；换档位重跑会 bust 48h 缓存重新计费
        Tier::Agentic
    }
}

/// parse_pdf 工具参数（与 TS 薄壳插件 schema 对齐）。
#[derive(Debug, Clone, Deserialize)]
pub struct ParsePdfRequest {
    /// 本地 PDF 路径（workspace 相对/绝对），或 http(s) 公网 URL（仅 MinerU）
    pub file: String,
    /// 解析引擎，默认 mineru。auto = MinerU 优先（失败不降级）
    #[serde(default)]
    pub engine: Option<String>,
    /// LlamaParse 档位，默认 agentic
    #[serde(default)]
    pub tier: Option<String>,
    /// 级联增强（仅 engine=mineru 本地文件）：MinerU 先解析，表格/低置信度页
    /// 自动切出交给 LlamaParse 重解析
    #[serde(default)]
    pub cascade: bool,
    /// 是否用另一引擎跑第二遍做交叉验证（消耗双倍额度）
    #[serde(default)]
    pub verify: bool,
    /// MinerU 模型版本，默认 vlm
    #[serde(default)]
    pub model_version: Option<String>,
    /// 强制 OCR（扫描件/图片型 PDF）
    #[serde(default)]
    pub is_ocr: bool,
    /// 文档语言，默认 ch（中英）
    #[serde(default)]
    pub language: Option<String>,
    /// Markdown 输出目录（workspace 相对路径，默认工作区根）
    #[serde(default)]
    pub out_dir: Option<String>,
    /// 是否在返回前应用 mineru-refine 式后处理（默认 true）
    #[serde(default = "default_true")]
    pub refine: bool,
}

fn default_true() -> bool {
    true
}

/// 引擎配置（API keys 来自插件设置文件，由端点读取后传入）。
#[derive(Debug, Clone, Default)]
pub struct EngineKeys {
    pub mineru: String,
    /// 多 key 用逗号分隔（串行预算：先用完第一把再切下一把）
    pub llamaparse: String,
}

/// 解析统计（Hermes 版同构）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct MdStats {
    pub chars: usize,
    pub paragraphs: usize,
    pub table_rows: usize,
    pub formula_blocks: usize,
    pub inline_formulas: usize,
    pub code_blocks: usize,
}

/// 引擎解析结果（MinerU / LlamaParse 统一形状）。
#[derive(Debug, Clone, Default)]
pub struct ParseOutput {
    pub markdown: String,
    pub stats: MdStats,
    pub pages: Option<usize>,
    pub chunked: bool,
    pub chunks: usize,
    /// MinerU content_list（级联增强用）
    pub content_list: Option<Vec<serde_json::Value>>,
    /// MinerU 结果解压目录（含 images/；编排结束后由调用方清理）
    pub images_dir: Option<PathBuf>,
    /// LlamaParse 任务信息
    pub tier: Option<Tier>,
    pub job_id: Option<String>,
    /// 本任务实际使用的 API key（预算账本统计用）
    pub used_key: Option<String>,
}

/// 最终工具响应（序列化给 LLM；markdown 超 200K 截断，与 Hermes 版一致）。
#[derive(Debug, Clone, Serialize)]
pub struct ParsePdfResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engines_used: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits_per_page: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown_chars: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<MdStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunks: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refine_report: Option<refine::RefineReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_report: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cascade_report: Option<serde_json::Value>,
    pub elapsed_seconds: f64,
}

/// 统一入口：按参数选择引擎执行，可叠加 verify / cascade / refine。
///
/// 返回序列化友好的结果；异常不抛出，全部转为 `success: false` JSON
/// （工具约定：错误也返回 JSON）。
pub async fn parse_pdf_any(
    req: &ParsePdfRequest,
    keys: &EngineKeys,
    workspace_root: &Path,
) -> ParsePdfResult {
    let started = std::time::Instant::now();
    let engine = match req.engine.as_deref().unwrap_or("mineru").to_lowercase().as_str() {
        "mineru" | "" => Engine::Mineru,
        "llamaparse" => Engine::Llamaparse,
        "auto" => Engine::Auto,
        other => {
            return err_result(&format!("不支持的引擎: {other}"), started);
        }
    };
    let tier = match req.tier.as_deref().unwrap_or("agentic").to_lowercase().as_str() {
        "fast" => Tier::Fast,
        "cost_effective" => Tier::CostEffective,
        "agentic" => Tier::Agentic,
        "agentic_plus" => Tier::AgenticPlus,
        other => {
            return err_result(
                &format!("不支持的 LlamaParse 档位: {other} (可选 fast/cost_effective/agentic/agentic_plus)"),
                started,
            );
        }
    };

    let is_url = req.file.starts_with("http://") || req.file.starts_with("https://");
    let engine = if engine == Engine::Auto { Engine::Mineru } else { engine };

    // 引擎依赖检查
    let need_mineru = engine == Engine::Mineru
        || (req.verify && req.engine.as_deref().unwrap_or("").to_lowercase() != "llamaparse");
    let need_llama = engine == Engine::Llamaparse
        || (req.verify && req.engine.as_deref().unwrap_or("").to_lowercase() == "llamaparse")
        || (req.cascade && engine == Engine::Mineru);
    if need_mineru && keys.mineru.trim().is_empty() {
        return err_result("未配置 MINERU_API_KEY（插件设置页）", started);
    }
    if need_llama && keys.llamaparse.trim().is_empty() {
        return err_result("未配置 LLAMAPARSE_API_KEY（插件设置页）", started);
    }

    // 引擎执行器（闭包，verify 复用；错误统一为 String——编排层最终转 JSON）
    let primary_engine = engine;
    let mut primary: ParseOutput;
    let run_engine = |eng: Engine| async move {
        let result: Result<ParseOutput, String> = match eng {
            Engine::Mineru => {
                if is_url {
                    mineru::parse_url(
                        &req.file,
                        MineruConfig { token: keys.mineru.clone(), model_version: req.model_version.clone().unwrap_or_else(|| "vlm".into()), is_ocr: req.is_ocr, language: req.language.clone() },
                    )
                    .await
                    .map_err(|e| e.to_string())
                } else {
                    let path = resolve_local_file(&req.file, workspace_root);
                    mineru::parse_file(
                        &path,
                        MineruConfig { token: keys.mineru.clone(), model_version: req.model_version.clone().unwrap_or_else(|| "vlm".into()), is_ocr: req.is_ocr, language: req.language.clone() },
                    )
                    .await
                    .map_err(|e| e.to_string())
                }
            }
            Engine::Llamaparse => {
                let path = resolve_local_file(&req.file, workspace_root);
                llamaparse::parse_file(
                    &path,
                    &LlamaParseConfig { tokens: keys.llamaparse.clone(), tier },
                )
                .await
                .map_err(|e| e.to_string())
            }
            Engine::Auto => unreachable!(),
        };
        result
    };

    match run_engine(primary_engine).await {
        Ok(out) => primary = out,
        Err(e) => return err_result(&format!("{} 解析失败: {e}", primary_engine.name()), started),
    }
    let mut engines_used = vec![primary_engine.name().to_string()];

    // ---- verify: 按优先级选另一个引擎跑第二遍做交叉验证 -------------------
    let mut verify_report = None;
    if req.verify {
        let secondary = if primary_engine == Engine::Mineru {
            if keys.llamaparse.trim().is_empty() { None } else { Some(Engine::Llamaparse) }
        } else {
            if keys.mineru.trim().is_empty() { None } else { Some(Engine::Mineru) }
        };
        if let Some(eng) = secondary {
            match run_engine(eng).await {
                Ok(secondary_out) => {
                    verify_report = Some(verify::cross_check_json(
                        &primary.markdown,
                        &secondary_out.markdown,
                        primary_engine.name(),
                        eng.name(),
                    ));
                    engines_used.push(eng.name().to_string());
                }
                Err(e) => {
                    verify_report = Some(verify::verify_failure_json(eng.name(), &e.to_string()));
                }
            }
        } else {
            verify_report = Some(verify::verify_failure_json("none", "无可用第二引擎(需配置另一引擎的 API key)"));
        }
    }

    // ---- cascade: MinerU 本地文件 + 已配置 LlamaParse ---------------------
    let mut cascade_report = None;
    if req.cascade && primary_engine == Engine::Mineru && !is_url && !keys.llamaparse.trim().is_empty()
    {
        let path = resolve_local_file(&req.file, workspace_root);
        match llamaparse::cascade_enhance(&primary, &path, &LlamaParseConfig { tokens: keys.llamaparse.clone(), tier }).await {
            Ok(report) => cascade_report = Some(report),
            Err(e) => cascade_report = Some(serde_json::json!({
                "enhanced": [], "note": format!("级联增强失败: {e}"),
            })),
        }
    }

    // ---- mineru-refine 式后处理（默认开启）--------------------------------
    let mut refine_report = None;
    if req.refine && primary.content_list.is_some() {
        match refine::refine_content(&mut primary) {
            Ok(report) => refine_report = report,
            Err(e) => {
                tracing::warn!(event = "pdf_omni.refine_failed", error = %e);
            }
        }
    }

    // ---- 落盘 --------------------------------------------------------------
    let md = primary.markdown.clone();
    let mut saved_path = None;
    if !md.is_empty() && !is_url {
        if let Some(out_dir) = resolve_out_dir(req, workspace_root) {
            if let Some(stem) = Path::new(&req.file).file_stem().map(|s| s.to_string_lossy().to_string()) {
                let target = out_dir.join(format!("{stem}.md"));
                if std::fs::create_dir_all(&out_dir).and_then(|_| std::fs::write(&target, &md)).is_ok() {
                    saved_path = Some(relative_or_abs(&target, workspace_root));
                }
            }
        }
    }

    let truncated = md.len() > 200_000;
    let out = ParsePdfResult {
        success: true,
        error: None,
        engine: Some(primary_engine.name().to_string()),
        engines_used: Some(engines_used),
        tier: primary.tier.map(|t| t.name().to_string()),
        credits_per_page: primary.tier.map(|t| t.rate()),
        markdown: Some(if truncated { truncate_utf8(&md, 200_000) } else { md.clone() }),
        markdown_truncated: Some(truncated),
        markdown_path: saved_path,
        markdown_chars: Some(md.len()),
        stats: Some(primary.stats.clone()),
        chunked: Some(primary.chunked),
        chunks: Some(primary.chunks),
        pages: primary.pages,
        refine_report,
        verify_report,
        cascade_report,
        elapsed_seconds: started.elapsed().as_secs_f64(),
    };
    // 清理 MinerU 临时解压目录
    if let Some(dir) = primary.images_dir.take() {
        pdf_ops::cleanup_dir(&dir);
    }
    out
}

fn err_result(msg: &str, started: std::time::Instant) -> ParsePdfResult {
    ParsePdfResult {
        success: false,
        error: Some(msg.to_string()),
        engine: None,
        engines_used: None,
        tier: None,
        credits_per_page: None,
        markdown: None,
        markdown_truncated: None,
        markdown_path: None,
        markdown_chars: None,
        stats: None,
        chunked: None,
        chunks: None,
        pages: None,
        refine_report: None,
        verify_report: None,
        cascade_report: None,
        elapsed_seconds: started.elapsed().as_secs_f64(),
    }
}

/// 本地文件解析：workspace 内校验（复用 bm-core::workspace::safe_join）。
fn resolve_local_file(file: &str, workspace_root: &Path) -> PathBuf {
    match bm_core::workspace::safe_join(workspace_root, file) {
        Ok(p) => p,
        Err(_) => PathBuf::from(file), // 越界/异常路径交给引擎报错（文件不存在等）
    }
}

fn resolve_out_dir(req: &ParsePdfRequest, workspace_root: &Path) -> Option<PathBuf> {
    match req.out_dir.as_deref() {
        Some(d) if !d.trim().is_empty() => bm_core::workspace::safe_join(workspace_root, d).ok(),
        _ => Some(workspace_root.to_path_buf()),
    }
}

fn relative_or_abs(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

/// 按字节截断但保持 UTF-8 字符边界（200K 截断防 panic）。
fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut idx = max_bytes;
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    s[..idx].to_string()
}
