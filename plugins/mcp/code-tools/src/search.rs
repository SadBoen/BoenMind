//! 内容搜索:rg 同款引擎(grep-regex 正则 + grep-searcher 逐行),
//! walkdir 顺序遍历,跳过隐藏/黑名单目录与超限大文件,命中即收,总量封顶。

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{sinks, BinaryDetection, SearcherBuilder};
use serde_json::{json, Value};
use walkdir::WalkDir;

use crate::config::Config;
use crate::guard::{display_path, Roots};

pub const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", "dist", "build"];
const LINE_CAP_CHARS: usize = 400;

fn skipped_dir(e: &walkdir::DirEntry) -> bool {
    if e.depth() == 0 {
        return false;
    }
    let name = e.file_name().to_string_lossy();
    e.file_type().is_dir() && (name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()))
}

pub fn search(cfg: &Config, roots: &Roots, args: &Value) -> Value {
    if roots.is_empty() {
        return json!({"ok": false, "error": "未配置 allowed_roots(设置页填根目录,分号分隔;改后「重载 MCP」)"});
    }
    let query = args["query"].as_str().unwrap_or_default().trim();
    if query.is_empty() {
        return json!({"ok": false, "error": "query 参数不能为空"});
    }
    let fixed = args["fixed"].as_bool().unwrap_or(false);
    let case_sensitive = args["case_sensitive"].as_bool().unwrap_or(false);
    let pattern = if fixed {
        regex::escape(query)
    } else {
        query.to_string()
    };
    let matcher = match RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .build(&pattern)
    {
        Ok(m) => m,
        Err(e) => {
            return json!({"ok": false, "error": format!("正则非法(fixed=true 可按字面搜索):{e}")});
        }
    };

    let max_results = args["max_results"]
        .as_u64()
        .map(|m| (m as usize).min(cfg.max_results))
        .unwrap_or(cfg.max_results);

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
            let oversized = entry
                .metadata()
                .map(|md| md.len() > cfg.max_file_bytes)
                .unwrap_or(false);
            if oversized {
                continue;
            }
            files_searched += 1;
            let mut searcher = SearcherBuilder::new()
                .binary_detection(BinaryDetection::quit(0))
                .line_number(true)
                .build();
            let file = display_path(entry.path());
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
            if let Err(_e) = searcher.search_path(&matcher, entry.path(), sink) {
                // 单文件失败(权限/编码)跳过,不中断整场搜索
                continue;
            }
            if total >= max_results as u64 {
                break 'roots;
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
        > cfg.max_output_chars
        && hits.len() > 1
    {
        hits.pop();
        out["matches"] = json!(hits);
        out["truncated"] = json!(true);
    }
    out
}

/// 供测试断言:按 (file, line) 提取命中键
#[cfg(test)]
pub fn hit_keys(v: &Value) -> Vec<(String, u64)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::Roots;

    fn cfg() -> Config {
        Config::load(None)
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
        let r = Roots::new(&[dir.path().display().to_string()]);
        (dir, r)
    }

    #[test]
    fn finds_matches_and_skips_hidden_and_blacklist() {
        let (_d, r) = tree();
        let out = search(&cfg(), &r, &json!({"query": "answer"}));
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
        assert!(
            keys.iter().any(|(f, l)| f.ends_with("a.rs") && *l == 2),
            "a.rs 第 2 行应命中:{keys:?}"
        );
        assert!(
            keys.iter()
                .any(|(f, l)| f.ends_with(r"src\b.rs") && *l == 1),
            "src/b.rs 第 1 行应命中:{keys:?}"
        );
    }

    #[test]
    fn fixed_mode_survives_regex_metachars() {
        let (_d, r) = tree();
        let bad = search(&cfg(), &r, &json!({"query": "42;", "fixed": true}));
        assert_eq!(bad["ok"], true);
        // a.rs "…42;" 与 src/b.rs "…42;" 各一处,共 2
        assert_eq!(hit_keys(&bad).len(), 2);
        let err = search(&cfg(), &r, &json!({"query": "((", "fixed": false}));
        assert_eq!(err["ok"], false, "非法正则应给出可读错误");
    }

    #[test]
    fn max_results_caps_and_marks_truncated() {
        let (_d, r) = tree();
        let out = search(&cfg(), &r, &json!({"query": "answer", "max_results": 1}));
        assert_eq!(out["truncated"], true);
        assert_eq!(hit_keys(&out).len(), 1);
    }

    #[test]
    fn empty_query_is_tool_error() {
        let (_d, r) = tree();
        let out = search(&cfg(), &r, &json!({"query": "  "}));
        assert_eq!(out["ok"], false);
    }

    #[test]
    fn per_line_overlong_gets_ellipsized() {
        let dir = tempfile::tempdir().expect("tmp");
        let long = "x".repeat(2_000);
        std::fs::write(dir.path().join("big.rs"), format!("let s = \"{long}\";\n")).expect("write");
        let r = Roots::new(&[dir.path().display().to_string()]);
        let out = search(&cfg(), &r, &json!({"query": "let s"}));
        let text = out["matches"][0]["text"].as_str().expect("text");
        assert!(text.chars().count() <= LINE_CAP_CHARS + 1);
        assert!(text.ends_with('…'));
    }
}
