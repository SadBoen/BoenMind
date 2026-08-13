//! 双引擎结果交叉验证：Markdown 统计 + 内容行 Jaccard 相似度 + 顺序感知 ratio。
//!
//! 原理吸收自 Hermes pdf-omni（difflib.SequenceMatcher / Jaccard 判定），
//! 指标与判定阈值保持一致：Jaccard < 0.7 或关键统计不一致 → 建议人工核对。

use std::collections::HashSet;

use serde_json::json;

use super::MdStats;

/// Markdown 基础统计：段落/表格/公式/代码块/字符数。
///
/// 公式格式兼容：`$$...$$` / `\[...\]` 块级；`$...$` / `\(...\)` 行内。
pub fn compute_stats(md: &str) -> MdStats {
    let table_rows = md
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with('|') && t[1..].contains('|')
        })
        .count();
    let formula_blocks = count_regex(md, r"\$\$[\s\S]*?\$\$") + count_regex(md, r"\\\[[\s\S]*?\\\]");
    let inline_formulas = count_regex(md, r"(?<!\$)\$(?!\$)[^$\n]{1,200}\$(?!\$)")
        + count_regex(md, r"\\\(.{1,300}?\\\)");
    let code_blocks = count_regex(md, r"```") / 2;
    let paragraphs = md.split("\n\n").filter(|p| !p.trim().is_empty()).count();
    MdStats {
        chars: md.len(),
        paragraphs,
        table_rows,
        formula_blocks,
        inline_formulas,
        code_blocks,
    }
}

fn count_regex(text: &str, pattern: &str) -> usize {
    fancy_regex::Regex::new(pattern)
        .map(|re| re.find_iter(text).count())
        .unwrap_or(0)
}

/// 提取用于比对的核心内容行（去表格/代码/空行/链接语法），归一化空白。
fn content_lines(md: &str) -> HashSet<String> {
    let mut lines = HashSet::new();
    let mut in_code = false;
    for raw in md.lines() {
        let line = raw.trim();
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || line.is_empty() {
            continue;
        }
        if line.starts_with('|') && line[1..].contains('|') {
            continue; // 表格行
        }
        let cleaned: String = line
            .chars()
            .filter(|c| !"#*`>[]()!".contains(*c))
            .collect::<String>()
            .split_whitespace()
            .collect();
        if cleaned.len() >= 8 {
            lines.insert(cleaned); // 太短的行噪声大，忽略
        }
    }
    lines
}

/// 双引擎交叉验证报告（JSON 形状与 Hermes 版一致）。
pub fn cross_check_json(md_a: &str, md_b: &str, name_a: &str, name_b: &str) -> serde_json::Value {
    let stats_a = compute_stats(md_a);
    let stats_b = compute_stats(md_b);
    let lines_a = content_lines(md_a);
    let lines_b = content_lines(md_b);

    let union = lines_a.union(&lines_b).count();
    let jaccard = if union == 0 {
        0.0
    } else {
        lines_a.intersection(&lines_b).count() as f64 / union as f64
    };
    let ratio = if md_a.is_empty() || md_b.is_empty() {
        0.0
    } else {
        sequence_ratio(md_a, md_b)
    };

    let mut diffs = Vec::new();
    for (key, label) in [
        ("paragraphs", "段落数"),
        ("table_rows", "表格行数"),
        ("formula_blocks", "公式块数"),
        ("inline_formulas", "行内公式数"),
        ("chars", "字符数"),
    ] {
        let va = stat_of(&stats_a, key);
        let vb = stat_of(&stats_b, key);
        if key == "chars" {
            if va > 0 && vb > 0 && va.max(vb) as f64 / va.min(vb) as f64 > 2.0 {
                diffs.push(format!("{label}差异过大({va} vs {vb})"));
            }
        } else if va != vb {
            diffs.push(format!("{label}不一致({va} vs {vb})"));
        }
    }
    let verdict = if diffs.is_empty() && jaccard >= 0.7 {
        "一致"
    } else {
        "存在差异, 建议人工核对"
    };

    json!({
        "verdict": verdict,
        "jaccard_similarity": round4(jaccard),
        "char_ratio": round4(ratio),
        "stats": {
            name_a: stats_json(&stats_a),
            name_b: stats_json(&stats_b),
        },
        "differences": diffs,
    })
}

/// 第二引擎执行失败时的降级报告。
pub fn verify_failure_json(engine: &str, err: &str) -> serde_json::Value {
    json!({
        "verdict": format!("交叉验证失败({engine}): {err}"),
        "differences": [],
        "stats": {},
        "jaccard_similarity": 0.0,
        "char_ratio": 0.0,
        "secondary_engine": engine,
    })
}

fn stat_of(s: &MdStats, key: &str) -> usize {
    match key {
        "paragraphs" => s.paragraphs,
        "table_rows" => s.table_rows,
        "formula_blocks" => s.formula_blocks,
        "inline_formulas" => s.inline_formulas,
        "chars" => s.chars,
        _ => 0,
    }
}

fn stats_json(s: &MdStats) -> serde_json::Value {
    json!({
        "chars": s.chars,
        "paragraphs": s.paragraphs,
        "table_rows": s.table_rows,
        "formula_blocks": s.formula_blocks,
        "inline_formulas": s.inline_formulas,
        "code_blocks": s.code_blocks,
    })
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// 顺序感知的相似度（difflib.SequenceMatcher.ratio 的简化版：
/// 最长公共子序列 → 2*LCS / (len_a + len_b)，对 markdown 长度级别的输入足够）。
fn sequence_ratio(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let lcs = lcs_len(a.as_bytes(), b.as_bytes());
    2.0 * lcs as f64 / (a.len() + b.len()) as f64
}

/// 内存受限的 LCS（O(min(a,b)) 空间）——对大 markdown 也安全。
fn lcs_len(a: &[u8], b: &[u8]) -> usize {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let m = short.len();
    let mut prev = vec![0usize; m + 1];
    let mut cur = vec![0usize; m + 1];
    for &cb in long {
        for (j, &ca) in short.iter().enumerate() {
            cur[j + 1] = if ca == cb {
                prev[j] + 1
            } else {
                cur[j].max(prev[j + 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
        cur.fill(0);
    }
    prev[m]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_counts() {
        let md = "# 标题\n\n正文段落。\n\n| a | b |\n| 1 | 2 |\n\n$$x^2$$\n\n```rust\nfn x() {}\n```";
        let s = compute_stats(md);
        assert_eq!(s.table_rows, 2);
        assert_eq!(s.formula_blocks, 1);
        assert_eq!(s.code_blocks, 1);
        assert!(s.paragraphs >= 4);
    }

    #[test]
    fn jaccard_identical_is_one() {
        let md = "同一份内容\n\n第二段文字测试\n\n| t |\n| 1 |";
        let r = cross_check_json(md, md, "a", "b");
        assert_eq!(r["verdict"], "一致");
        assert!(r["jaccard_similarity"].as_f64().unwrap() > 0.99);
    }

    #[test]
    fn jaccard_divergent_flags() {
        let a = "完全不同的文档内容甲\n\n这是第一段\n\n| t |\n| 1 |";
        let b = "完全无关的另一份内容乙\n\n那一段\n\n$$f(x)$$";
        let r = cross_check_json(a, b, "a", "b");
        assert!(r["verdict"].as_str().unwrap().contains("人工核对"));
    }

    #[test]
    fn lcs_basic() {
        assert_eq!(lcs_len(b"abcde", b"ace"), 3);
        assert_eq!(lcs_len(b"abc", b"xyz"), 0);
    }
}
