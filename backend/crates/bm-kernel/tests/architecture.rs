//! 架构依赖测试（LoopX L9 吸收）：bm-kernel 不得依赖 bm-server / bm-core。
//!
//! 内核层只能向下依赖（bm-protocol + 外部基础设施）；对应用层的依赖是
//! 破坏性外向边，机器强制禁止。检查两层：
//! 1. Cargo.toml 文本解析（dependencies / dev / build / target 限定全扫）；
//! 2. 源码扫描 `use` / `extern crate` / 全限定路径（隐藏依赖不算分离，L9）。
//!
//! 变更纪律：若确需新增对上层 crate 的依赖，先回架构讨论（这是依赖方向
//! 反转，不是本文件改一行的事）。

use std::path::{Path, PathBuf};

/// 内核禁止依赖的上层 crate（依赖方向：protocol ← kernel ← storage ← server）。
const FORBIDDEN: &[&str] = &["bm-server", "bm-core"];

#[test]
fn does_not_depend_on_upper_crates() {
    let toml = read_manifest();
    let (deps, dev_deps) = parse_dep_sections(&toml);
    for dep in deps.iter().chain(dev_deps.iter()) {
        assert!(
            !FORBIDDEN.contains(&dep.as_str()),
            "bm-kernel 依赖 `{dep}` 破坏依赖方向（内核层 ← 应用层）——铁律 3，禁止"
        );
    }
}

#[test]
fn sources_do_not_reference_upper_crates() {
    // 只扫 src/：tests/ 目录含本守卫文件自身（会提及被禁 crate 名）。
    for path in rust_sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src")) {
        let text = std::fs::read_to_string(&path).unwrap();
        for (idx, line) in text.lines().enumerate() {
            // 只扫代码部分（// 后为注释）
            let code = line.split("//").next().unwrap_or("");
            for dep in FORBIDDEN {
                let crate_name = dep.replace('-', "_");
                for form in [
                    format!("::{crate_name}"),
                    format!("use {crate_name}"),
                    format!("extern crate {crate_name}"),
                    format!("{crate_name}::"),
                ] {
                    assert!(
                        !code.contains(&form),
                        "{}:{} 隐藏引用上层 crate：{line}",
                        path.display(),
                        idx + 1
                    );
                }
            }
        }
    }
}

// ============================================================
// 工具：Cargo.toml 依赖区解析（文本级，与 bm-protocol 的守卫同构）
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
