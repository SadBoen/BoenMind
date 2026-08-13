//! 架构依赖测试（LoopX L9 吸收）：铁律 3"吸收不进核心"从人工审计升级为 CI 机器强制。
//!
//! bm-protocol 是契约层，只允许 serde 族的序列化依赖，禁止任何 workspace
//! 内部依赖。这里直接解析本 crate 的 Cargo.toml 文本（不为测试引入外部
//! 解析库——被保护的 crate 自身不能再有依赖），并扫描源码中的 crate 引用
//! （隐藏依赖不算分离，L9）。
//!
//! 变更纪律：新增外部依赖必须先在本文件白名单登记并写明理由，
//! 同步架构文档 §3.6 L9；白名单外新增任何依赖 → 测试失败（CI 强制）。

use std::path::{Path, PathBuf};

/// bm-protocol 允许的外部依赖白名单（仅序列化基础设施）。
const ALLOWED_EXTERNAL: &[&str] = &["serde", "serde_json"];

#[test]
fn zero_workspace_deps_and_only_serde_family() {
    let toml = read_manifest();
    let (deps, dev_deps) = parse_dep_sections(&toml);
    let all: Vec<String> = deps.into_iter().chain(dev_deps).collect();

    let internal: Vec<&String> = all.iter().filter(|d| d.starts_with("bm-")).collect();
    assert!(
        internal.is_empty(),
        "bm-protocol 不得依赖任何 workspace 内部 crate（铁律 3 契约层），发现: {internal:?}"
    );

    for dep in &all {
        assert!(
            ALLOWED_EXTERNAL.contains(&dep.as_str()),
            "bm-protocol 出现白名单外依赖 `{dep}`——契约层只允许 {ALLOWED_EXTERNAL:?}；\
             若确有必要请更新本文件白名单并同步架构文档 §3.6 L9"
        );
    }
}

#[test]
fn sources_do_not_reference_internal_crates() {
    for path in rust_sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src")) {
        let text = std::fs::read_to_string(&path).unwrap();
        for (idx, line) in text.lines().enumerate() {
            // 只扫代码部分（// 后为注释；本项目不用块注释引用 crate）
            let code = line.split("//").next().unwrap_or("");
            for needle in ["::bm_", "extern crate bm_", "use bm_"] {
                assert!(
                    !code.contains(needle),
                    "{}:{} 隐藏引用 workspace crate：{line}",
                    path.display(),
                    idx + 1
                );
            }
        }
    }
}

// ============================================================
// 工具：Cargo.toml 依赖区解析（文本级）
// ============================================================

fn read_manifest() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("读取 Cargo.toml 失败")
}

/// 收集 src 目录（递归）下全部 .rs 文件。
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rs(dir, &mut out);
    out.sort();
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("读取 src 目录失败") {
        let path = entry.expect("读取目录项失败").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 解析 Cargo.toml 的依赖区，返回 (dependencies, dev+build dependencies)。
///
/// 支持本项目实际使用的写法：
/// - 行内：`name = "..."` / `name = { version = "...", ... }` / `name = { workspace = true }`（含跨行续写）
/// - 多行表：`[dependencies.name]` / `[dev-dependencies.name]`
/// - target 限定：`[target.'cfg(...)'.dependencies]` 及其 `.dependencies.name` 形态
///
/// 遇到解析不了的形态直接 panic（宁可测试失败也不静默漏检）。
fn parse_dep_sections(toml: &str) -> (Vec<String>, Vec<String>) {
    #[derive(Clone, Copy, PartialEq)]
    enum Section {
        Deps,
        Dev,
        Skip,
        /// 多行表依赖：`[dependencies.name]` 的 name（一次性入账，不进入行解析）
        TableDep,
    }

    let mut deps: Vec<String> = Vec::new();
    let mut dev: Vec<String> = Vec::new();
    let mut cur: Section = Section::Skip;
    let mut brace_depth: i32 = 0;

    for (idx, raw) in toml.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            brace_depth = 0;
            let header = line.trim_start_matches('[').trim_end_matches(']').trim();
            // 多行表依赖（含 target 限定），一次性入账后退出行解析
            let table = ["dev-dependencies.", "build-dependencies.", "dependencies."]
                .iter()
                .find_map(|prefix| {
                    header
                        .strip_prefix(*prefix)
                        .or_else(|| header.rsplit_once(&format!(".{prefix}")).map(|(_, n)| n))
                        .map(|name| (unquote(name.trim()), *prefix != "dependencies."))
                });
            if let Some((name, is_dev)) = table {
                (if is_dev { &mut dev } else { &mut deps }).push(name);
                cur = Section::TableDep;
                continue;
            }
            cur = match header {
                "dependencies" => Section::Deps,
                "dev-dependencies" | "build-dependencies" => Section::Dev,
                // target 限定整区：依赖方向约束同样适用
                h if h.starts_with("target.") && h.ends_with(".dependencies") => Section::Deps,
                h if h.starts_with("target.")
                    && (h.ends_with(".dev-dependencies") || h.ends_with(".build-dependencies")) =>
                {
                    Section::Dev
                }
                _ => Section::Skip,
            };
            continue;
        }

        let target = match cur {
            Section::Skip | Section::TableDep => continue,
            Section::Deps => &mut deps,
            Section::Dev => &mut dev,
        };

        if brace_depth > 0 {
            // 行内表跨行续写：只更新括号深度，不当作新依赖行
            brace_depth += braces(line);
            if brace_depth > 0 {
                continue;
            }
            // 收口行（含 '}'）只闭合不解析，下一行起恢复正常
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            panic!("Cargo.toml 第 {idx} 行解析失败（依赖区出现非依赖行？）: {line}");
        };
        target.push(name.trim().to_string());
        brace_depth = braces(rest);
    }
    (deps, dev)
}

fn braces(s: &str) -> i32 {
    let open = s.chars().filter(|c| matches!(*c, '{')).count() as i32;
    let close = s.chars().filter(|c| matches!(*c, '}')).count() as i32;
    open - close
}

fn unquote(s: &str) -> String {
    s.trim_matches('"').trim_matches('\'').to_string()
}
