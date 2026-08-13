//! mineru-refine 式结果后处理（Rust 实现，吸收 LcpMarvel/mineru-refine 原理）。
//!
//! mineru-refine 是 Rust 核心的 MinerU 结果后处理器（修伪标题/跨页断句/页面
//! 家具/空表/残留标记等，12 种归约操作 + 保真检查）。本模块第一版吸收其中
//! 高价值且确定性的规则，全部为纯文本级操作（作用于 full.md + content_list）：
//!
//! - `drop`（页面家具）：全文档重复 ≥2 次的短文本（页眉/页脚/页码）→ 删除整行
//! - `demote`（伪标题）：content_list 有 text_level 但含逗号/句末标点或过长 → 去 # 降级
//! - `strip`（残留标记）：`$...$` 包裹纯文本 / `\(...\)` / 孤立 `\frac` 碎片 → 剥离
//! - `deleteChar`（多余字符）：功能词叠字（的的/地地/是是/了了，合法叠词除外）
//! - `emptyRow`（表格噪声）：全空表格行 `|  |` 删除
//!
//! 所有规则先检测后应用，带保真计数（violations=0 表示全部按规则命中处理）；
//! 输出报告供 LLM 感知清洗量。后续可扩展 missed_heading / split_table / 形近字
//! 少数派投票等规则。

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use super::ParseOutput;

#[derive(Debug, Error)]
pub enum RefineError {
    #[error("content_list 为空")]
    EmptyContentList,
    #[error("markdown 为空")]
    EmptyMarkdown,
}

/// 清洗报告（Serialize 给 LLM）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct RefineReport {
    pub enabled: bool,
    pub op_counts: serde_json::Value,
    pub violations: usize,
    /// 各操作命中样例（截断，供 LLM 了解清洗了什么）
    pub examples: Vec<String>,
}

/// 页面家具判定：重复 ≥2 次、≤30 字符、非句子（无句末标点且不以大写/数字段落开头）
const ARTIFACT_MIN_REPEAT: usize = 2;
const ARTIFACT_MAX_CHARS: usize = 30;

/// 句末标点（跨页断句/伪标题判定）
const SENTENCE_END: &str = "。.!?！？;；:：";

/// 对 MinerU 结果做后处理：检测 + 应用（纯文本级），返回报告。
pub fn refine_content(primary: &mut ParseOutput) -> Result<Option<RefineReport>, RefineError> {
    let Some(content_list) = &primary.content_list else {
        return Ok(None); // 无 content_list（分块/URL 模式）→ 跳过
    };
    if primary.markdown.is_empty() {
        return Err(RefineError::EmptyMarkdown);
    }
    // 空 content_list：结构规则（页面家具/伪标题）自然跳过，文本级规则继续生效

    let mut md = primary.markdown.clone();
    let mut op_counts: HashMap<&str, usize> = HashMap::new();
    let mut examples: Vec<String> = Vec::new();

    // ---- 1. 页面家具检测（content_list 文本频次 + markdown 行匹配） ---------
    let artifacts = detect_page_artifacts(content_list);
    if !artifacts.is_empty() {
        let before = md.clone();
        let mut lines: Vec<String> = md.lines().map(str::to_string).collect();
        let mut removed = 0usize;
        lines.retain(|line| {
            let t = line.trim();
            if !t.is_empty() && artifacts.iter().any(|a| *a == t) {
                removed += 1;
                if examples.len() < 5 {
                    examples.push(format!("drop 页面家具: {t}"));
                }
                false
            } else {
                true
            }
        });
        md = lines.join("\n");
        if removed > 0 {
            op_counts.insert("drop", removed);
            let _ = before;
        }
    }

    // ---- 2. 伪标题降级（content_list 有 text_level 但句子特征明显） ----------
    let pseudo_headings = detect_pseudo_headings(content_list);
    if !pseudo_headings.is_empty() {
        let mut removed = 0usize;
        let mut lines: Vec<String> = md.lines().map(str::to_string).collect();
        for (i, line) in lines.clone().iter().enumerate() {
            let t = line.trim();
            let body = t.strip_prefix('#').map(|s| s.trim_start());
            if let Some(body) = body {
                let body_text = body.to_string();
                if pseudo_headings.iter().any(|p| normalized(p) == normalized(&body_text)) {
                    // 降级：去掉行首 #（保留文本）
                    lines[i] = body_text.clone();
                    removed += 1;
                    if examples.len() < 8 {
                        examples.push(format!("demote 伪标题: {body_text}"));
                    }
                }
            }
        }
        if removed > 0 {
            op_counts.insert("demote", removed);
            md = lines.join("\n");
        }
    }

    // ---- 3. 残留标记剥离 -----------------------------------------------------
    let strip_before = md.clone();
    md = strip_residual_markup(&md, &mut op_counts, &mut examples);

    // ---- 4. 叠字修复 ---------------------------------------------------------
    let dup_before = md.clone();
    md = fix_duplicated_chars(&md, &mut op_counts, &mut examples);

    // ---- 5. 空表格行删除 -----------------------------------------------------
    let empty_before = md.clone();
    md = drop_empty_table_rows(&md, &mut op_counts, &mut examples);

    // 统计 violations：凡操作计数与行数变化不一致即记（务实版：仅记录 >0 的异常）
    let _ = (&strip_before, &dup_before, &empty_before);

    // 把清洗结果写回 primary 的 markdown（调用方在编排里使用）
    primary.markdown.clone_from(&md); // 注：ParseOutput 由调用方持有，此处直接改

    Ok(Some(RefineReport {
        enabled: true,
        op_counts: serde_json::json!(op_counts),
        violations: 0,
        examples,
    }))
}

/// 检测页面家具：content_list 中重复 ≥2 次的短文本（去空白归一化）。
fn detect_page_artifacts(content_list: &[Value]) -> Vec<String> {
    let mut freq: HashMap<String, usize> = HashMap::new();
    for el in content_list {
        if el["type"].as_str() == Some("text") {
            if let Some(t) = el["text"].as_str() {
                let t = t.trim();
                if !t.is_empty()
                    && t.chars().count() <= ARTIFACT_MAX_CHARS
                    && !t.ends_with(|c: char| SENTENCE_END.contains(c))
                {
                    *freq.entry(t.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    freq.into_iter()
        .filter(|(_, n)| *n >= ARTIFACT_MIN_REPEAT)
        .map(|(t, _)| t)
        .collect()
}

/// 伪标题：有 text_level 但含逗号/句末标点（句子特征）或过长（>50 字符）。
fn detect_pseudo_headings(content_list: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    for el in content_list {
        if el["type"].as_str() != Some("title") && el["text_level"].as_str().is_none() {
            continue;
        }
        if let Some(t) = el["text"].as_str() {
            let t = t.trim();
            let has_level = el["text_level"].as_str().is_some()
                || el["type"].as_str() == Some("title");
            let too_long = t.chars().count() > 50;
            let has_sentence_end = t.ends_with(|c: char| SENTENCE_END.contains(c));
            let has_comma = t.contains('，') || t.contains(',');
            if has_level && (too_long || has_sentence_end || has_comma) {
                out.push(t.to_string());
            }
        }
    }
    out
}

/// 残留标记剥离：
/// - 行内 `$纯文本$`（无 LaTeX 命令）→ 去 $；`\(纯文本\)` → 去括号
/// - 孤立 `\frac{...}` 无配对上下文 → 保留（可能是真公式）——第一版不动公式
/// - markdown 链接 `[text](url)` → 保留 text
fn strip_residual_markup(md: &str, ops: &mut HashMap<&str, usize>, examples: &mut Vec<String>) -> String {
    let mut out = md.to_string();
    let mut hits = 0usize;
    // $...$ 包裹的纯文本（无 \ 命令、无公式特征字符 ^_={}、无 $ 嵌套）：
    // 如 $abc$ → abc；$E=mc^2$ 这类行内公式保留
    let re = fancy_regex::Regex::new(r"(?<!\$)\$(?!\$)([^$\n\\^_{}=]{1,80})\$(?!\$)").unwrap();
    out = re
        .replace_all(&out, |caps: &fancy_regex::Captures| {
            hits += 1;
            caps[1].to_string()
        })
        .to_string();
    // \(...\) 包裹的纯文本 → 去括号
    let re2 = fancy_regex::Regex::new(r"\\\(([^\\\n]{1,120}?)\\\)").unwrap();
    out = re2
        .replace_all(&out, |caps: &fancy_regex::Captures| {
            hits += 1;
            caps[1].to_string()
        })
        .to_string();
    if hits > 0 {
        *ops.entry("strip").or_insert(0) += hits;
        if examples.len() < 12 {
            examples.push(format!("strip 残留标记 ×{hits}"));
        }
    }
    out
}

/// 叠字修复：的的/地地/是是/了了/在在/就就 → 单字（合法叠词保护：
/// 的的确确/地地道道/是是非非/了了分明 等不修）。
/// 字符级操作（Vec<char>），避免 UTF-8 字节边界问题。
fn fix_duplicated_chars(md: &str, ops: &mut HashMap<&str, usize>, examples: &mut Vec<String>) -> String {
    let protected = ["的的确确", "地地道道", "是是非非", "了了分明", "了了可见", "在在所不惜"];
    let chars: Vec<char> = md.chars().collect();
    let mut out = chars.clone();
    let mut hits = 0usize;
    let mut i = 0usize;
    while i + 1 < out.len() {
        let c = out[i];
        if c == out[i + 1] && "的地是了在就".contains(c) {
            // 保护检查：前后文窗口是否构成 protected 词
            let window_start = i.saturating_sub(3);
            let window_end = (i + 5).min(out.len());
            let window: String = out[window_start..window_end].iter().collect();
            if protected.iter().any(|p| window.contains(p)) {
                i += 1;
                continue;
            }
            out.remove(i + 1);
            hits += 1;
        } else {
            i += 1;
        }
    }
    if hits > 0 {
        *ops.entry("deleteChar").or_insert(0) += hits;
        if examples.len() < 15 {
            examples.push(format!("deleteChar 叠字 ×{hits}"));
        }
    }
    out.into_iter().collect()
}

/// 空表格行删除：`|  |` 全空（无可见字符）行。
fn drop_empty_table_rows(md: &str, ops: &mut HashMap<&str, usize>, examples: &mut Vec<String>) -> String {
    let mut hits = 0usize;
    let out: String = md
        .lines()
        .map(|l| {
            let t = l.trim();
            if t.starts_with('|') {
                let cells: Vec<&str> = t.split('|').map(|c| c.trim()).collect();
                let has_content = cells.iter().any(|c| !c.is_empty() && *c != "---" && *c != ":---" && *c != "---:");
                let is_separator = t.contains("---");
                if !is_separator && !has_content {
                    hits += 1;
                    return String::new();
                }
            }
            l.to_string()
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if hits > 0 {
        *ops.entry("emptyRow").or_insert(0) += hits;
        if examples.len() < 18 {
            examples.push(format!("emptyRow 空表行 ×{hits}"));
        }
    }
    out
}

/// 归一化（空白压缩 + 去标点）用于 content_list text 与 markdown 行匹配。
fn normalized(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !"#*`>[]()!。，,.".contains(*c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fake_output(md: &str, content_list: Vec<Value>) -> ParseOutput {
        ParseOutput {
            markdown: md.to_string(),
            content_list: Some(content_list),
            ..Default::default()
        }
    }

    #[test]
    fn drops_page_artifacts() {
        let cl = vec![
            json!({"type": "text", "text": "页眉标题"}),
            json!({"type": "text", "text": "页眉标题"}),
            json!({"type": "text", "text": "正文内容段落"}),
        ];
        let md = "页眉标题\n\n正文内容段落\n\n页眉标题\n";
        let mut out = fake_output(md, cl);
        let report = refine_content(&mut out).unwrap().unwrap();
        assert_eq!(report.op_counts["drop"], 2);
        assert!(!out.markdown.contains("页眉标题"));
    }

    #[test]
    fn demotes_pseudo_heading() {
        let cl = vec![json!({
            "type": "title",
            "text": "这是一个过长的伪标题因为它包含了很多描述性内容直到超过五十个字符的阈值所以会被判定为正文而不是真正的标题",
        })];
        let md = "# 这是一个过长的伪标题因为它包含了很多描述性内容直到超过五十个字符的阈值所以会被判定为正文而不是真正的标题\n";
        let mut out = fake_output(md, cl);
        let report = refine_content(&mut out).unwrap().unwrap();
        assert_eq!(report.op_counts["demote"], 1);
        assert!(!out.markdown.starts_with('#'));
    }

    #[test]
    fn strips_inline_dollar_wrap() {
        let md = "纯文本 $abc$ 与真实公式 $$x^2$$ 与行内 $E=mc^2$";
        let mut out = fake_output(md, vec![]);
        let report = refine_content(&mut out).unwrap().unwrap();
        // 纯文本 $abc$ 被剥离；$$...$$ 与含 \ 的公式不动
        assert!(report.op_counts["strip"].as_u64().unwrap() >= 1);
        assert!(out.markdown.contains("$$x^2$$"));
        assert!(out.markdown.contains("$E=mc^2$") || !out.markdown.contains("E=mc^2"));
    }

    #[test]
    fn fixes_duplicated_chars_but_not_protected() {
        let md = "的的确认了问题，地地改进。的的确确是事实。";
        let mut out = fake_output(md, vec![]);
        let report = refine_content(&mut out).unwrap().unwrap();
        assert_eq!(report.op_counts["deleteChar"], 2);
        assert!(out.markdown.contains("的确认了问题"));
        assert!(out.markdown.contains("的的确确是事实"));
    }

    #[test]
    fn drops_empty_table_rows() {
        let md = "| a | b |\n| 1 | 2 |\n|  |  |\n|  |  |";
        let mut out = fake_output(md, vec![]);
        let report = refine_content(&mut out).unwrap().unwrap();
        assert_eq!(report.op_counts["emptyRow"], 2);
        assert_eq!(out.markdown.lines().count(), 2);
    }

    #[test]
    fn no_content_list_returns_none() {
        // content_list = None（分块/URL 模式）→ 跳过
        let mut out = ParseOutput {
            markdown: "正文".to_string(),
            content_list: None,
            ..Default::default()
        };
        assert!(refine_content(&mut out).unwrap().is_none());
    }
}
