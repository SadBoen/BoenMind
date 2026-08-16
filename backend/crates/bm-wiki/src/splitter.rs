//! Page 正文切分（PRIN-ING-4，对齐 xu-wiki ingest/splitter.py）。
//!
//! 决策树（自上而下，先命中先赢）：
//! 1. 标题（`#`~`######`）是候选切点；
//! 2. 过小的节按物理邻近向上并入 ~max_lines；
//! 3. 无清晰边界 → 按行数硬切；尾段独立成页（floor 除法，不上并）。

/// 把正文切成 ~max_lines 的页列表（空正文 → 空）。
pub fn split_pages(text: &str, max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return Vec::new();
    }
    if lines.len() <= max_lines {
        return vec![lines.join("\n")];
    }

    let header_idx: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, ln)| is_header(ln))
        .map(|(i, _)| i)
        .collect();

    if header_idx.is_empty() {
        return hard_split(&lines, max_lines);
    }

    let mut boundaries: Vec<usize> = vec![0];
    boundaries.extend(header_idx);
    boundaries.push(lines.len());
    boundaries.sort_unstable();
    boundaries.dedup();

    let sections: Vec<(usize, usize)> = boundaries
        .windows(2)
        .filter(|w| w[1] > w[0])
        .map(|w| (w[0], w[1]))
        .collect();

    let mut pages: Vec<String> = Vec::new();
    let mut cur_start = sections[0].0;
    let mut cur_end = cur_start;
    for (a, b) in &sections {
        let seg_len = b - cur_start;
        if seg_len >= max_lines && cur_end > cur_start {
            // 累积段已达上限：冲刷，从本节重新开始
            pages.push(lines[cur_start..cur_end].join("\n"));
            cur_start = *a;
        }
        cur_end = *b;
        if cur_end - cur_start >= max_lines {
            pages.push(lines[cur_start..cur_end].join("\n"));
            cur_start = cur_end;
        }
    }
    if cur_end > cur_start {
        pages.push(lines[cur_start..cur_end].join("\n"));
    }

    // 仍超 2×max_lines 的巨段（内部无标题）→ 硬切
    let mut final_pages: Vec<String> = Vec::new();
    for pg in &pages {
        let pls: Vec<&str> = pg.split('\n').collect();
        if pls.len() > max_lines * 2 {
            final_pages.extend(hard_split(&pls, max_lines));
        } else {
            final_pages.push(pg.clone());
        }
    }
    final_pages
        .into_iter()
        .filter(|p| p.lines().any(|l| !l.trim().is_empty()))
        .collect()
}

fn is_header(line: &str) -> bool {
    let t = line.trim_start();
    let hashes = t.bytes().take_while(|b| *b == b'#').count();
    (1..=6).contains(&hashes) && t.as_bytes().get(hashes) == Some(&b' ')
}

fn hard_split(lines: &[&str], max_lines: usize) -> Vec<String> {
    lines
        .chunks(max_lines)
        .map(|chunk| chunk.join("\n"))
        .filter(|c| c.lines().any(|l| !l.trim().is_empty()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn short_text_single_page() {
        assert_eq!(split_pages("a\nb\nc", 300), vec!["a\nb\nc"]);
    }

    #[test]
    fn empty_text_no_pages() {
        assert!(split_pages("  \n\n  ", 300).is_empty());
        assert!(split_pages("", 300).is_empty());
    }

    #[test]
    fn hard_split_by_line_count() {
        let pages = split_pages(&lines(700), 300);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].split('\n').count(), 300);
        assert_eq!(pages[2].split('\n').count(), 100);
    }

    #[test]
    fn exact_boundary_no_tail() {
        let pages = split_pages(&lines(600), 300);
        assert_eq!(pages.len(), 2);
    }

    #[test]
    fn header_cut_points() {
        let text = format!(
            "# 第一章\n{}\n## 小节\n{}\n# 第二章\n{}",
            lines(100),
            lines(50),
            lines(100)
        );
        let pages = split_pages(&text, 100);
        assert_eq!(pages.len(), 3);
        assert!(pages[0].starts_with("# 第一章"));
        assert!(pages[1].starts_with("## 小节"));
        assert!(pages[2].starts_with("# 第二章"));
    }

    #[test]
    fn giant_section_gets_hard_split() {
        let text = format!("{}{}", lines(10), lines(650).replace("line ", "x "));
        // 无标题 → 全部硬切（10+650 行）
        let pages = split_pages(&text, 300);
        assert_eq!(pages.len(), 3);
    }

    #[test]
    fn trailing_remainder_is_own_page() {
        let pages = split_pages(&lines(900), 300);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[2].split('\n').count(), 300);
    }
}
