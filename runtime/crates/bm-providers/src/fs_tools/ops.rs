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
/// 单次读取文件大小上限:16MB(防超大文件全量入内存 DoS)
const MAX_READ_BYTES: u64 = 16 * 1_048_576;
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
    if let Ok(meta) = std::fs::metadata(&path)
        && meta.len() > MAX_READ_BYTES
    {
        return tool_err(format!(
            "文件过大({}MB > 上限 {}MB),请使用带行号/分片读取工具",
            meta.len() / (1024 * 1024),
            MAX_READ_BYTES / (1024 * 1024)
        ));
    }
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
/// 单条替换意图(edits 数组元素或单处字段的归一形态)。
struct EditIntent {
    old: String,
    new: String,
    replace_all: bool,
}

/// 在 content 中定位 old 的全部命中区间;0 命中且疑似 CRLF 差异时按
/// \r\n 形态重试(模型侧常按 \n 给原文)。返回 (生效old, 生效new, 区间集)。
fn locate(content: &str, intent: &EditIntent) -> (String, String, Vec<(usize, usize)>) {
    let mut old_eff = intent.old.clone();
    let mut new_eff = intent.new.clone();
    let mut ranges: Vec<(usize, usize)> = content
        .match_indices(&intent.old)
        .map(|(i, s)| (i, i + s.len()))
        .collect();
    if ranges.is_empty() && content.contains("\r\n") && intent.old.contains('\n') {
        old_eff = intent.old.replace('\n', "\r\n");
        new_eff = intent.new.replace('\n', "\r\n");
        ranges = content
            .match_indices(&old_eff)
            .map(|(i, s)| (i, i + s.len()))
            .collect();
    }
    (old_eff, new_eff, ranges)
}

/// 一条编辑在原文快照上的定位结果:(生效old, 生效new, 命中区间集)。
type LocatedEdit = (String, String, Vec<(usize, usize)>);

/// fs.edit(ADR-0022 批量升级):单处 old_string/new_string 或 edits 数组
/// 二选一。批量语义对齐 pi:全部编辑基于**文件当前原文**快照定位,区间
/// 不得重叠,一次读-校-写原子提交——避免多轮往返与中间态漂移。
pub fn edit(roots: &Roots, args: &Value) -> Value {
    let path = match roots.resolve(args["path"].as_str().unwrap_or_default()) {
        Ok(p) => p,
        Err(e) => return tool_err(e),
    };
    // 归一两条入口:edits 数组(批量)/ 单处字段(向后兼容)
    let intents: Vec<EditIntent> = if let Some(list) = args["edits"].as_array() {
        if list.is_empty() {
            return tool_err("edits 不能为空数组(单处改动请直接传 old_string/new_string)");
        }
        let mut v = Vec::with_capacity(list.len());
        for (i, e) in list.iter().enumerate() {
            let old = e["old_string"].as_str().unwrap_or_default().to_string();
            let new = e["new_string"].as_str().unwrap_or_default().to_string();
            if old.is_empty() {
                return tool_err(format!("edits[{i}].old_string 必填(要被替换的精确原文)"));
            }
            v.push(EditIntent {
                old,
                new,
                replace_all: e["replace_all"].as_bool().unwrap_or(false),
            });
        }
        v
    } else {
        let old = args["old_string"].as_str().unwrap_or_default().to_string();
        let new = args["new_string"].as_str().unwrap_or_default().to_string();
        if old.is_empty() {
            return tool_err("old_string 必填(要被替换的精确原文;或传 edits 数组批量替换)");
        }
        vec![EditIntent {
            old,
            new,
            replace_all: args["replace_all"].as_bool().unwrap_or(false),
        }]
    };

    let raw = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return tool_err(format!("读取失败:{}({e})", display_path(&path))),
    };
    let content = match String::from_utf8(raw) {
        Ok(s) => s,
        Err(_) => {
            return tool_err("文件不是 UTF-8 编码,拒绝编辑(请人工处理或走 system.exec)");
        }
    };

    // 定位:全部意图都针对同一原文快照解析区间
    let mut plan: Vec<LocatedEdit> = Vec::new();
    for (i, intent) in intents.iter().enumerate() {
        let (old_eff, new_eff, ranges) = locate(&content, intent);
        if ranges.is_empty() {
            return tool_err(format!(
                "edits[{i}] 的 old_string 在 {} 中未找到(注意须与文件原文逐字一致,含缩进)",
                display_path(&path)
            ));
        }
        if ranges.len() > 1 && !intent.replace_all {
            return tool_err(format!(
                "edits[{i}] 的 old_string 命中 {} 处,要求唯一;请补充更多上下文收窄,或对该条传 replace_all=true",
                ranges.len()
            ));
        }
        plan.push((old_eff, new_eff, ranges));
    }

    // 区间汇总 + 不相交校验(跨编辑重叠 = 语义冲突,拒执行)
    let mut all: Vec<(usize, usize, &str)> = Vec::new();
    for (_, new_eff, ranges) in &plan {
        for (s, e) in ranges {
            all.push((*s, *e, new_eff.as_str()));
        }
    }
    all.sort_by_key(|(s, e, _)| (*s, *e));
    for w in all.windows(2) {
        if w[1].0 < w[0].1 {
            return tool_err("多处编辑的匹配区间重叠,请拆分调用或调整 old_string 使其互不重叠");
        }
    }

    // 按区间重建(区间已升序且不相交)
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    for (s, e, new_eff) in &all {
        out.push_str(&content[cursor..*s]);
        out.push_str(new_eff);
        cursor = *e;
    }
    out.push_str(&content[cursor..]);

    let replacements = all.len();
    match std::fs::write(&path, out.as_bytes()) {
        Ok(()) => json!({
            "ok": true,
            "path": display_path(&path),
            "replacements": replacements,
            "edits": plan.len(),
        }),
        Err(e) => tool_err(format!("写回失败:{}({e})", display_path(&path))),
    }
}

#[cfg(test)]
mod tests {

    // ---- ADR-0022:fs_edit 批量 edits 数组 ------------------------------

    fn write_tmp(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, content).expect("write tmp");
        p
    }

    #[test]
    fn edit_batch_applies_multiple_disjoint_edits_against_original_snapshot() {
        let dir = tempfile::tempdir().expect("tmp");
        let roots = roots_for(dir.path());
        let p = write_tmp(dir.path(), "b.txt", "alpha\nbeta\ngamma\n");
        let out = edit(
            &roots,
            &serde_json::json!({
                "path": p.display().to_string(),
                "edits": [
                    {"old_string": "gamma", "new_string": "GAMMA"},
                    {"old_string": "alpha", "new_string": "ALPHA"}
                ]
            }),
        );
        assert_eq!(out["ok"], true, "{out}");
        let content = std::fs::read_to_string(&p).expect("read");
        assert_eq!(content, "ALPHA\nbeta\nGAMMA\n");
        assert_eq!(out["edits"], serde_json::json!(2));
        assert_eq!(out["replacements"], serde_json::json!(2));
    }

    #[test]
    fn edit_batch_rejects_overlapping_ranges() {
        let dir = tempfile::tempdir().expect("tmp");
        let roots = roots_for(dir.path());
        let p = write_tmp(dir.path(), "c.txt", "hello world\n");
        let out = edit(
            &roots,
            &serde_json::json!({
                "path": p.display().to_string(),
                "edits": [
                    {"old_string": "hello world", "new_string": "X"},
                    {"old_string": "world", "new_string": "Y"}
                ]
            }),
        );
        assert_eq!(out["ok"], false, "{out}");
        assert!(out["error"].as_str().unwrap().contains("重叠"));
    }

    #[test]
    fn edit_batch_second_edit_sees_original_not_intermediate() {
        // Pi 语义:第二条 old_string 匹配的是原文件,不是第一条的结果
        let dir = tempfile::tempdir().expect("tmp");
        let roots = roots_for(dir.path());
        let p = write_tmp(dir.path(), "d.txt", "foo bar\n");
        let out = edit(
            &roots,
            &serde_json::json!({
                "path": p.display().to_string(),
                "edits": [
                    {"old_string": "foo bar", "new_string": "foo baz"},
                    {"old_string": "foo", "new_string": "qux"}
                ]
            }),
        );
        // "foo" 在原文只命中 1 处,且与第一条区间重叠 → 应拒重叠而非错改
        assert_eq!(out["ok"], false, "{out}");
    }

    #[test]
    fn edit_single_entry_still_works_backward_compatible() {
        let dir = tempfile::tempdir().expect("tmp");
        let roots = roots_for(dir.path());
        let p = write_tmp(dir.path(), "e.txt", "keep me\n");
        let out = edit(
            &roots,
            &serde_json::json!({
                "path": p.display().to_string(),
                "old_string": "keep",
                "new_string": "CHANGE"
            }),
        );
        assert_eq!(out["ok"], true, "{out}");
        assert!(std::fs::read_to_string(&p).unwrap().contains("CHANGE me"));
    }

    #[test]
    fn edit_batch_replace_all_within_single_edit_item() {
        let dir = tempfile::tempdir().expect("tmp");
        let roots = roots_for(dir.path());
        let p = write_tmp(dir.path(), "f.txt", "a b a b a\n");
        let out = edit(
            &roots,
            &serde_json::json!({
                "path": p.display().to_string(),
                "edits": [
                    {"old_string": "a", "new_string": "Z", "replace_all": true}
                ]
            }),
        );
        assert_eq!(out["ok"], true, "{out}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "Z b Z b Z\n");
    }
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
