//! 文件查/读/写/改的纯逻辑(自 code-tools 插件移植,ADR-0021)。
//! search = rg 同款引擎(grep-regex + grep-searcher)内嵌;read 带行号分页;
//! write/edit 审批类,edit 走精确字符串替换(不走 sed 躲转义地狱),
//! CRLF 文件自动兼容;非 UTF-8 拒改。

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, SearcherBuilder, sinks};
use serde_json::{Value, json};
use walkdir::WalkDir;

use super::guard::{Roots, display_path};

/// 限额(内核固定值;随包插件时代的 config 可调项收敛为常量)。
const MAX_RESULTS_DEFAULT: usize = 80;
const MAX_RESULTS_CEILING: usize = 500;
const MAX_OUTPUT_CHARS: usize = 16_000;
const MAX_FILE_BYTES: u64 = 1_048_576;
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", "dist", "build"];
const LINE_CAP_CHARS: usize = 400;
const READ_DEFAULT_LINES: usize = 2_000;
const READ_MAX_LINES: usize = 10_000;
const LINE_CAP_CHARS_FALLBACK: usize = 500;

fn tool_err(msg: impl Into<String>) -> Value {
    json!({"ok": false, "error": msg.into()})
}

fn skipped_dir(e: &walkdir::DirEntry) -> bool {
    if e.depth() == 0 {
        return false;
    }
    let name = e.file_name().to_string_lossy();
    e.file_type().is_dir() && (name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()))
}

fn glob_to_regex(pat: &str) -> String {
    let mut regex = String::from("^");
    for c in pat.chars() {
        match c {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '.' | '(' | ')' | '[' | ']' | '{' | '}' | '+' | '^' | '$' | '|' | '\\' => {
                regex.push('\\');
                regex.push(c);
            }
            _ => regex.push(c),
        }
    }
    regex.push('$');
    regex
}

pub fn search(roots: &Roots, args: &Value) -> Value {
    if roots.is_empty() {
        return tool_err("工作区注册表为空:在设置「常规 → 工作区」登记至少一个目录后再用文件工具");
    }
    let query = args["query"].as_str().unwrap_or_default().trim();
    if query.is_empty() {
        return tool_err("query 参数不能为空");
    }
    let mode = args["mode"].as_str().unwrap_or("content");
    let fixed = args["fixed"].as_bool().unwrap_or(false);
    let case_sensitive = args["case_sensitive"].as_bool().unwrap_or(false);
    let path_pattern = args["path_pattern"]
        .as_str()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let max_results = args["max_results"]
        .as_u64()
        .map(|m| (m as usize).clamp(1, MAX_RESULTS_CEILING))
        .unwrap_or(MAX_RESULTS_DEFAULT);

    // 路径过滤器 (如果有 path_pattern)
    let path_filter = if let Some(pat) = path_pattern {
        let pat_regex = glob_to_regex(pat);
        regex::RegexBuilder::new(&pat_regex)
            .case_insensitive(true)
            .build()
            .ok()
    } else {
        None
    };

    // 模式一: 仅查找文件路径与名字 (类似 find / glob)
    if mode == "files" {
        let mut hits: Vec<Value> = Vec::new();
        let mut files_searched: u64 = 0;
        let mut total: u64 = 0;

        // 文件名匹配正则: 如果包含 * 或 ? 则作为通配符，否则作为子串模糊搜索
        let file_regex = if query.contains('*') || query.contains('?') {
            glob_to_regex(query)
        } else if fixed {
            format!("(?i){}", regex::escape(query))
        } else {
            // 支持类似于 "README|readme" 这种正则模式
            query.to_string()
        };

        let matcher = match regex::RegexBuilder::new(&file_regex)
            .case_insensitive(!case_sensitive)
            .build()
        {
            Ok(r) => r,
            Err(_) => {
                // 语法错误则退让为纯字面子串匹配
                regex::RegexBuilder::new(&regex::escape(query))
                    .case_insensitive(!case_sensitive)
                    .build()
                    .unwrap()
            }
        };

        'roots_files: for root in roots.roots() {
            for entry in WalkDir::new(root)
                .max_depth(24)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| !skipped_dir(e))
            {
                let Ok(entry) = entry else { continue };
                files_searched += 1;
                let rel_path = display_path(entry.path());
                let file_name = entry.file_name().to_string_lossy();

                if let Some(ref pf) = path_filter
                    && !pf.is_match(&rel_path)
                    && !pf.is_match(&file_name)
                {
                    continue;
                }

                if matcher.is_match(&file_name) || matcher.is_match(&rel_path) {
                    total += 1;
                    hits.push(json!({
                        "file": rel_path,
                        "is_dir": entry.file_type().is_dir(),
                    }));
                    if total >= max_results as u64 {
                        break 'roots_files;
                    }
                }
            }
        }

        return json!({
            "ok": true,
            "mode": "files",
            "query": query,
            "total_matches": total,
            "truncated": total >= max_results as u64,
            "files_searched": files_searched,
            "matches": hits,
        });
    }

    // 模式二: 内容搜索 (类似 ripgrep grep-searcher)
    // 智能容错: 如果用户/模型误传了 fixed=true 但包含明显的正则操作符 '|'，且未找到结果，做自愈尝试
    let (pattern, allow_fallback_regex) = if fixed {
        if query.contains('|') && !query.contains(r"\|") {
            // 带有未转义的竖线，保留原始 query 备用回退
            (regex::escape(query), Some(query.to_string()))
        } else {
            (regex::escape(query), None)
        }
    } else {
        (query.to_string(), None)
    };

    let matcher = match RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .build(&pattern)
    {
        Ok(m) => m,
        Err(e) => {
            return tool_err(format!("正则非法(fixed=true 可按字面搜索):{e}"));
        }
    };

    let mut hits: Vec<Value> = Vec::new();
    let mut total: u64 = 0;
    let mut files_searched: u64 = 0;
    'roots: for root in roots.roots() {
        for entry in WalkDir::new(root)
            .max_depth(24)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !skipped_dir(e))
        {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_file() {
                continue;
            }
            let file = display_path(entry.path());
            let file_name = entry.file_name().to_string_lossy();

            if let Some(ref pf) = path_filter
                && !pf.is_match(&file)
                && !pf.is_match(&file_name)
            {
                continue;
            }

            let oversized = entry
                .metadata()
                .map(|md| md.len() > MAX_FILE_BYTES)
                .unwrap_or(false);
            if oversized {
                continue;
            }
            files_searched += 1;
            let mut searcher = SearcherBuilder::new()
                .binary_detection(BinaryDetection::quit(0))
                .line_number(true)
                .build();
            let sink = sinks::UTF8(|line_no, line: &str| {
                if total >= max_results as u64 {
                    return Ok(false);
                }
                total += 1;
                let mut text = line.trim_end_matches(['\r', '\n']).to_string();
                if text.chars().count() > LINE_CAP_CHARS {
                    text = text.chars().take(LINE_CAP_CHARS).collect::<String>() + "…";
                }
                hits.push(json!({"file": file, "line": line_no, "text": text}));
                Ok(true)
            });
            if searcher.search_path(&matcher, entry.path(), sink).is_err() {
                // 单文件失败(权限/编码)跳过,不中断整场搜索
                continue;
            }
            if total >= max_results as u64 {
                break 'roots;
            }
        }
    }

    // 若 fixed=true + 包含 '|' 导致 0 命中，则以 regex fallback 自动拯救一次
    if total == 0
        && let Some(alt_pat) = allow_fallback_regex
        && let Ok(alt_matcher) = RegexMatcherBuilder::new()
            .case_insensitive(!case_sensitive)
            .build(&alt_pat)
    {
        'roots_fallback: for root in roots.roots() {
            for entry in WalkDir::new(root)
                .max_depth(24)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| !skipped_dir(e))
            {
                let Ok(entry) = entry else { continue };
                if !entry.file_type().is_file() {
                    continue;
                }
                let file = display_path(entry.path());
                let mut searcher = SearcherBuilder::new()
                    .binary_detection(BinaryDetection::quit(0))
                    .line_number(true)
                    .build();
                let sink = sinks::UTF8(|line_no, line: &str| {
                    if total >= max_results as u64 {
                        return Ok(false);
                    }
                    total += 1;
                    let mut text = line.trim_end_matches(['\r', '\n']).to_string();
                    if text.chars().count() > LINE_CAP_CHARS {
                        text = text.chars().take(LINE_CAP_CHARS).collect::<String>() + "…";
                    }
                    hits.push(json!({"file": file, "line": line_no, "text": text}));
                    Ok(true)
                });
                let _ = searcher.search_path(&alt_matcher, entry.path(), sink);
                if total >= max_results as u64 {
                    break 'roots_fallback;
                }
            }
        }
    }

    let truncated = total >= max_results as u64;
    // 输出字符总量封顶:从尾部丢弃命中直至合规
    let mut out = json!({
        "ok": true, "query": query, "total_matches": total,
        "truncated": truncated, "files_searched": files_searched,
        "matches": hits,
    });
    while serde_json::to_string(&out)
        .map(|s| s.chars().count())
        .unwrap_or(0)
        > MAX_OUTPUT_CHARS
        && hits.len() > 1
    {
        hits.pop();
        out["matches"] = json!(hits);
        out["truncated"] = json!(true);
    }
    out
}

pub fn read(roots: &Roots, args: &Value) -> Value {
    let path = match roots.resolve(args["path"].as_str().unwrap_or_default()) {
        Ok(p) => p,
        Err(e) => return tool_err(e),
    };
    let raw = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return tool_err(format!("读取失败:{}({e})", display_path(&path))),
    };
    let content = String::from_utf8_lossy(&raw);
    let total_lines = content.lines().count() as u64;
    let offset = args["offset"].as_u64().unwrap_or(1).max(1);
    let limit = args["limit"]
        .as_u64()
        .map(|l| (l as usize).clamp(1, READ_MAX_LINES))
        .unwrap_or(READ_DEFAULT_LINES);

    let mut body = String::new();
    let mut emitted: u64 = 0;
    let mut cap_hit = false;
    for (idx, line) in content.lines().enumerate().skip((offset - 1) as usize) {
        if emitted >= limit as u64 {
            break;
        }
        let mut l = line.to_string();
        if l.chars().count() > LINE_CAP_CHARS_FALLBACK {
            l = l.chars().take(LINE_CAP_CHARS_FALLBACK).collect::<String>() + "…";
        }
        let rendered = format!("{:>6}\t{}", idx as u64 + 1, l);
        if body.chars().count() + rendered.chars().count() > MAX_OUTPUT_CHARS {
            cap_hit = true;
            break;
        }
        body.push_str(&rendered);
        body.push('\n');
        emitted += 1;
    }
    let consumed = offset - 1 + emitted;
    json!({
        "ok": true,
        "path": display_path(&path),
        "total_lines": total_lines,
        "offset": offset,
        "lines_emitted": emitted,
        "truncated": cap_hit || consumed < total_lines,
        "content": body,
    })
}

pub fn write(roots: &Roots, args: &Value) -> Value {
    let path = match roots.resolve(args["path"].as_str().unwrap_or_default()) {
        Ok(p) => p,
        Err(e) => return tool_err(e),
    };
    let content = args["content"].as_str().unwrap_or_default();
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return tool_err(format!("建父目录失败:{e}"));
    }
    match std::fs::write(&path, content.as_bytes()) {
        Ok(()) => json!({
            "ok": true,
            "path": display_path(&path),
            "bytes": content.len(),
        }),
        Err(e) => tool_err(format!("写入失败:{}({e})", display_path(&path))),
    }
}

/// 统计 needle 在 haystack 中的出现次数
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

pub fn edit(roots: &Roots, args: &Value) -> Value {
    let path = match roots.resolve(args["path"].as_str().unwrap_or_default()) {
        Ok(p) => p,
        Err(e) => return tool_err(e),
    };
    let old = args["old_string"].as_str().unwrap_or("");
    let new = args["new_string"].as_str().unwrap_or("");
    let replace_all = args["replace_all"].as_bool().unwrap_or(false);
    if old.is_empty() {
        return tool_err("old_string 必填(要被替换的精确原文)");
    }
    let raw = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return tool_err(format!("读取失败:{}({e})", display_path(&path))),
    };
    let mut content = match String::from_utf8(raw) {
        Ok(s) => s,
        Err(_) => {
            return tool_err("文件不是 UTF-8 编码,拒绝编辑(请人工处理或走 system.exec)");
        }
    };
    let mut old_eff = old.to_string();
    let mut new_eff = new.to_string();
    let mut hits = count_occurrences(&content, old);
    if hits == 0 && content.contains("\r\n") && old.contains('\n') {
        // CRLF 兼容:模型侧原文按 \n 给出时,换 \r\n 形态重试
        old_eff = old.replace('\n', "\r\n");
        new_eff = new.replace('\n', "\r\n");
        hits = count_occurrences(&content, &old_eff);
    }
    if hits == 0 {
        return tool_err(format!(
            "old_string 在 {} 中未找到(注意须与文件原文逐字一致,含缩进)",
            display_path(&path)
        ));
    }
    if hits > 1 && !replace_all {
        return tool_err(format!(
            "old_string 命中 {hits} 处,要求唯一;请补充更多上下文收窄,或传 replace_all=true 全部替换"
        ));
    }
    content = content.replace(&old_eff, &new_eff);
    match std::fs::write(&path, content.as_bytes()) {
        Ok(()) => json!({
            "ok": true,
            "path": display_path(&path),
            "replacements": hits,
        }),
        Err(e) => tool_err(format!("写回失败:{}({e})", display_path(&path))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn roots_for(dir: &Path) -> Roots {
        Roots::new(&[dir.display().to_string()])
    }

    fn tree() -> (tempfile::TempDir, Roots) {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("a.rs"), "fn main() {}\nlet answer = 42;\n").expect("write");
        std::fs::create_dir_all(dir.path().join("src")).expect("dir");
        std::fs::write(dir.path().join("src/b.rs"), "const ANSWER: i32 = 42;\n").expect("write");
        std::fs::create_dir_all(dir.path().join(".git")).expect("dir");
        std::fs::write(dir.path().join(".git/hidden.rs"), "let answer = 42;\n").expect("write");
        std::fs::create_dir_all(dir.path().join("node_modules/x")).expect("dir");
        std::fs::write(dir.path().join("node_modules/x/c.js"), "let answer = 42;\n")
            .expect("write");
        let r = roots_for(dir.path());
        (dir, r)
    }

    fn hit_keys(v: &Value) -> Vec<(String, u64)> {
        v["matches"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|m| {
                        (
                            m["file"].as_str().unwrap_or_default().to_string(),
                            m["line"].as_u64().unwrap_or(0),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn finds_matches_and_skips_hidden_and_blacklist() {
        let (_d, r) = tree();
        let out = search(&r, &json!({"query": "answer"}));
        assert_eq!(out["ok"], true);
        let keys = hit_keys(&out);
        // 期望恰两处:a.rs:2(let answer)+ src/b.rs:1(ANSWER,忽略大小写);
        // .git/hidden.rs 与 node_modules/c.js 必须被黑名单剔除
        assert_eq!(keys.len(), 2, "跳过 .git 与 node_modules:{keys:?}");
        assert!(
            keys.iter()
                .all(|(f, _)| !f.contains(".git") && !f.contains("node_modules")),
            "黑名单目录不得出现在命中里:{keys:?}"
        );
    }

    #[test]
    fn fixed_mode_survives_regex_metachars() {
        let (_d, r) = tree();
        let bad = search(&r, &json!({"query": "42;", "fixed": true}));
        assert_eq!(bad["ok"], true);
        assert_eq!(hit_keys(&bad).len(), 2);
        let err = search(&r, &json!({"query": "((", "fixed": false}));
        assert_eq!(err["ok"], false, "非法正则应给出可读错误");
    }

    #[test]
    fn max_results_caps_and_marks_truncated() {
        let (_d, r) = tree();
        let out = search(&r, &json!({"query": "answer", "max_results": 1}));
        assert_eq!(out["truncated"], true);
        assert_eq!(hit_keys(&out).len(), 1);
    }

    #[test]
    fn empty_query_is_tool_error() {
        let (_d, r) = tree();
        let out = search(&r, &json!({"query": "  "}));
        assert_eq!(out["ok"], false);
    }

    #[test]
    fn read_returns_line_numbers_and_respects_offset_limit() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("f.txt"), "l1\nl2\nl3\nl4\n").expect("write");
        let r = roots_for(dir.path());
        let out = read(&r, &json!({"path": "f.txt"}));
        assert_eq!(out["ok"], true);
        assert_eq!(out["total_lines"], json!(4));
        let content = out["content"].as_str().expect("content");
        assert!(content.contains("1\tl1"), "首行带行号:{content}");
        let paged = read(&r, &json!({"path": "f.txt", "offset": 3, "limit": 1}));
        let paged_body = paged["content"].as_str().expect("content");
        assert!(paged_body.trim_end().ends_with("3\tl3"));
        assert_eq!(paged["truncated"], json!(true), "还有第 4 行未出");
    }

    #[test]
    fn write_creates_parents_and_reports_bytes() {
        let dir = tempfile::tempdir().expect("tmp");
        let r = roots_for(dir.path());
        let out = write(&r, &json!({"path": "deep/new/f.txt", "content": "hello"}));
        assert_eq!(out["ok"], true);
        assert_eq!(out["bytes"], json!(5));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("deep/new/f.txt")).expect("read"),
            "hello"
        );
    }

    #[test]
    fn edit_unique_hit_replaces_and_crlf_compatible() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("crlf.txt"), "alpha\r\nbeta\r\n").expect("write");
        let r = roots_for(dir.path());
        let out = edit(
            &r,
            &json!({"path": "crlf.txt", "old_string": "alpha\nbeta", "new_string": "ALPHA\nBETA"}),
        );
        assert_eq!(out["ok"], true, "CRLF 兼容替换:{out}");
        assert_eq!(out["replacements"], json!(1));
        let body = std::fs::read_to_string(dir.path().join("crlf.txt")).expect("read");
        assert!(body.contains("ALPHA\r\nBETA"));
    }

    #[test]
    fn edit_multi_hit_rejected_without_replace_all() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("m.txt"), "x\nx\n").expect("write");
        let r = roots_for(dir.path());
        let out = edit(
            &r,
            &json!({"path": "m.txt", "old_string": "x", "new_string": "y"}),
        );
        assert_eq!(out["ok"], false);
        let out2 = edit(
            &r,
            &json!({"path": "m.txt", "old_string": "x", "new_string": "y", "replace_all": true}),
        );
        assert_eq!(out2["replacements"], json!(2));
    }

    #[test]
    fn escape_outside_roots_rejected_in_ops() {
        let dir = tempfile::tempdir().expect("tmp");
        let r = roots_for(dir.path());
        let out = read(&r, &json!({"path": "../../etc/passwd"}));
        assert_eq!(out["ok"], false, "越界读取必须被拒:{out}");
    }

    #[test]
    fn search_files_mode_finds_by_name_and_wildcard() {
        let (d, r) = tree();
        std::fs::write(d.path().join("README.md"), "# Hello BoenMind\n").expect("write");
        // 1. 查找包含 README 的文件（类似 find / glob）
        let out = search(&r, &json!({"query": "README", "mode": "files"}));
        assert_eq!(out["ok"], true);
        assert_eq!(out["mode"], "files");
        assert_eq!(out["total_matches"], 1);
        let matches = out["matches"].as_array().expect("matches array");
        assert!(matches[0]["file"].as_str().unwrap().ends_with("README.md"));

        // 2. 通配符模式查找
        let out_glob = search(&r, &json!({"query": "*.rs", "mode": "files"}));
        assert_eq!(out_glob["ok"], true);
        let matches_glob = out_glob["matches"].as_array().expect("matches array");
        assert!(
            matches_glob
                .iter()
                .any(|m| m["file"].as_str().unwrap().ends_with("a.rs"))
        );
    }

    #[test]
    fn search_content_with_path_pattern_and_fallback_regex() {
        let (_d, r) = tree();
        // 1. 带 path_pattern 限制只查 a.rs(排除 src/b.rs)
        let out = search(
            &r,
            &json!({
                "query": "answer",
                "path_pattern": "*a.rs",
                "mode": "content"
            }),
        );
        assert_eq!(out["ok"], true);
        let matches = out["matches"].as_array().expect("matches array");
        assert_eq!(matches.len(), 1);
        assert!(matches[0]["file"].as_str().unwrap().ends_with("a.rs"));

        // 2. 误传 fixed=true 但包含 '|' 且原字面搜索无结果时的自动回退拯救
        let out_fixed_fallback = search(
            &r,
            &json!({
                "query": "answer|something_nonexistent",
                "fixed": true,
                "mode": "content"
            }),
        );
        assert_eq!(out_fixed_fallback["ok"], true);
        assert!(
            !out_fixed_fallback["matches"].as_array().unwrap().is_empty(),
            "fallback 成功救回包含 | 的模式"
        );
    }
}
