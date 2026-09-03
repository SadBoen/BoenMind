//! 文件读/写/精确替换。read 直通(带行号+分页+截断),write/edit 审批类
//! (宿主侧 destructiveHint → required)。edit 走精确字符串替换,不走 sed
//! (转义地狱),CRLF 文件自动兼容;非 UTF-8 拒改。

use serde_json::{json, Value};

use crate::config::Config;
use crate::guard::Roots;

const READ_DEFAULT_LINES: usize = 2_000;
const READ_MAX_LINES: usize = 10_000;

fn tool_err(msg: impl Into<String>) -> Value {
    json!({"ok": false, "error": msg.into()})
}

pub fn read(cfg: &Config, roots: &Roots, args: &Value) -> Value {
    let path = match roots.resolve(args["path"].as_str().unwrap_or_default()) {
        Ok(p) => p,
        Err(e) => return tool_err(e),
    };
    let raw = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            return tool_err(format!(
                "读取失败:{}({e})",
                crate::guard::display_path(&path)
            ))
        }
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
        if body.chars().count() + rendered.chars().count() > cfg.max_output_chars {
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
        "path": crate::guard::display_path(&path),
        "total_lines": total_lines,
        "offset": offset,
        "lines_emitted": emitted,
        "truncated": cap_hit || consumed < total_lines,
        "content": body,
    })
}

const LINE_CAP_CHARS_FALLBACK: usize = 500;

pub fn write(_cfg: &Config, roots: &Roots, args: &Value) -> Value {
    let path = match roots.resolve(args["path"].as_str().unwrap_or_default()) {
        Ok(p) => p,
        Err(e) => return tool_err(e),
    };
    let content = args["content"].as_str().unwrap_or_default();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return tool_err(format!("建父目录失败:{e}"));
        }
    }
    match std::fs::write(&path, content.as_bytes()) {
        Ok(()) => json!({
            "ok": true,
            "path": crate::guard::display_path(&path),
            "bytes": content.len(),
        }),
        Err(e) => tool_err(format!(
            "写入失败:{}({e})",
            crate::guard::display_path(&path)
        )),
    }
}

/// 统计 needle 在 haystack 中的出现次数
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

pub fn edit(_cfg: &Config, roots: &Roots, args: &Value) -> Value {
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
        Err(e) => {
            return tool_err(format!(
                "读取失败:{}({e})",
                crate::guard::display_path(&path)
            ))
        }
    };
    let mut content = match String::from_utf8(raw) {
        Ok(s) => s,
        Err(_) => {
            return tool_err("文件不是 UTF-8 编码,拒绝编辑(请人工处理或走 exec)");
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
            crate::guard::display_path(&path)
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
            "path": crate::guard::display_path(&path),
            "replacements": hits,
        }),
        Err(e) => tool_err(format!(
            "写回失败:{}({e})",
            crate::guard::display_path(&path)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::Roots;
    use std::path::Path;

    fn roots_for(dir: &Path) -> Roots {
        Roots::new(&[dir.display().to_string()])
    }

    fn name_of(p: &Path) -> String {
        p.display().to_string()
    }

    #[test]
    fn read_returns_line_numbers_and_respects_offset_limit() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("f.txt"), "l1\nl2\nl3\nl4\n").expect("write");
        let r = roots_for(dir.path());
        let out = read(&Config::load(None), &r, &json!({"path": "f.txt"}));
        assert_eq!(out["ok"], true);
        assert_eq!(out["total_lines"], 4);
        assert_eq!(out["lines_emitted"], 4);
        assert_eq!(out["truncated"], false);
        let body = out["content"].as_str().expect("body");
        assert!(body.contains("     1\tl1"));
        assert!(body.contains("     4\tl4"));

        let page = read(
            &Config::load(None),
            &r,
            &json!({"path": "f.txt", "offset": 3, "limit": 1}),
        );
        assert_eq!(page["lines_emitted"], 1);
        assert_eq!(page["truncated"], true, "第 3 行之后还有第 4 行");
        assert!(page["content"].as_str().expect("b").contains("l3"));
    }

    #[test]
    fn read_char_cap_sets_truncated() {
        let dir = tempfile::tempdir().expect("tmp");
        let line = "y".repeat(300);
        let big: String = (0..200).map(|_| format!("{line}\n")).collect();
        std::fs::write(dir.path().join("big.txt"), &big).expect("write");
        let mut cfg = Config::load(None);
        cfg.max_output_chars = 2_000;
        let r = roots_for(dir.path());
        let out = read(&cfg, &r, &json!({"path": "big.txt"}));
        assert_eq!(out["truncated"], true);
        assert!(out["content"].as_str().expect("b").chars().count() <= 2_000);
    }

    #[test]
    fn write_creates_parents_and_reports_bytes() {
        let dir = tempfile::tempdir().expect("tmp");
        let r = roots_for(dir.path());
        let out = write(
            &Config::load(None),
            &r,
            &json!({"path": "x/y/new.txt", "content": "hello"}),
        );
        assert_eq!(out["ok"], true);
        assert_eq!(out["bytes"], 5);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("x/y/new.txt")).expect("read"),
            "hello"
        );
    }

    #[test]
    fn edit_unique_hit_replaces_and_missing_or_multi_errors() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(
            dir.path().join("c.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .expect("write");
        let r = roots_for(dir.path());
        let cfg = Config::load(None);

        let miss = edit(
            &cfg,
            &r,
            &json!({"path": "c.rs", "old_string": "不存在的原文", "new_string": "x"}),
        );
        assert_eq!(miss["ok"], false);

        let multi_src = "a\na\na\n";
        std::fs::write(dir.path().join("m.txt"), multi_src).expect("write");
        let multi = edit(
            &cfg,
            &r,
            &json!({"path": "m.txt", "old_string": "a", "new_string": "b"}),
        );
        assert_eq!(multi["ok"], false, "多处命中必须要求唯一:{multi}");
        let all = edit(
            &cfg,
            &r,
            &json!({"path": "m.txt", "old_string": "a", "new_string": "b", "replace_all": true}),
        );
        assert_eq!(all["replacements"], 3);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("m.txt")).expect("r"),
            "b\nb\nb\n"
        );

        let ok = edit(
            &cfg,
            &r,
            &json!({"path": "c.rs", "old_string": "println!(\"hi\");", "new_string": "println!(\"yo\");"}),
        );
        assert_eq!(ok["replacements"], 1);
        assert!(std::fs::read_to_string(dir.path().join("c.rs"))
            .expect("r")
            .contains("println!(\"yo\");"));
    }

    #[test]
    fn edit_crlf_file_with_lf_old_string_still_matches() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("w.txt"), "alpha\r\nbeta\r\n").expect("write");
        let r = roots_for(dir.path());
        let out = edit(
            &Config::load(None),
            &r,
            &json!({"path": "w.txt", "old_string": "alpha\nbeta", "new_string": "gamma\ndelta"}),
        );
        assert_eq!(out["ok"], true, "CRLF 自动兼容:{out}");
        assert_eq!(out["replacements"], 1);
        let after = std::fs::read_to_string(dir.path().join("w.txt")).expect("r");
        assert_eq!(after, "gamma\r\ndelta\r\n");
    }

    #[test]
    fn guard_escape_and_empty_roots_flow_into_tool_errors() {
        let dir = tempfile::tempdir().expect("tmp");
        let r = roots_for(dir.path());
        let out = read(
            &Config::load(None),
            &r,
            &json!({"path": dir.path().join("../outside.txt").display().to_string()}),
        );
        assert_eq!(out["ok"], false);
        let empty = Roots::new(&[]);
        let out2 = write(
            &Config::load(None),
            &empty,
            &json!({"path": "a", "content": ""}),
        );
        assert_eq!(out2["ok"], false);
    }

    #[test]
    fn edit_rejects_non_utf8() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("bin.txt"), [0xff, 0xfe, b'a']).expect("write");
        let r = roots_for(dir.path());
        let out = edit(
            &Config::load(None),
            &r,
            &json!({"path": "bin.txt", "old_string": "a", "new_string": "b"}),
        );
        assert_eq!(out["ok"], false);
        assert!(out["error"].as_str().expect("e").contains("UTF-8"));
    }

    #[test]
    fn path_names_round_trip_through_display() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("f.txt");
        std::fs::write(&p, "x").expect("w");
        assert!(!name_of(&p).contains(r"\\?\"));
    }
}
