//! 架构依赖测试（LoopX L9 吸收）：bm-memory 是插件层，不得依赖
//! bm-server / bm-core（应用层）。
//!
//! 依赖方向：protocol ← kernel ← loop ← 插件 ← 组装层（bm-server）。
//! 插件只依赖核心契约（bm-loop 的 Compactor / LlmRequest），不碰应用层。
//! 检查两层：
//! 1. Cargo.toml 文本解析（dependencies / dev / build / target 限定全扫）；
//! 2. 源码扫描 `use` / `extern crate` / 全限定路径（隐藏依赖不算分离，L9）。

use std::path::Path;

/// 插件禁止依赖的上层 crate。
const FORBIDDEN: &[&str] = &["bm-server", "bm-core"];

fn rust_sources(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(rust_sources(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn does_not_depend_on_upper_crates() {
    let toml = std::fs::read_to_string(env!("CARGO_MANIFEST_DIR").to_string() + "/Cargo.toml")
        .unwrap();
    for dep in FORBIDDEN {
        // 依赖行形态："bm-server = ..." 或 bm_server = ...
        let in_toml = toml.lines().any(|l| {
            let code = l.split('#').next().unwrap_or("");
            code.trim_start().starts_with(&format!("{dep} "))
                || code.trim_start().starts_with(&format!("{} ", dep.replace('-', "_")))
        });
        assert!(!in_toml, "bm-memory 依赖 `{dep}` 破坏依赖方向——铁律 3，禁止");
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
