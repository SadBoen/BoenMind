//! crate 边界守卫（门禁 1 的一环）：依赖只许向下。
//!
//! 分层（借鉴 bobleer check-crate-boundaries，路径前缀定层）：
//!   layer 5  kernel-contracts   契约/端口（任何 crate 可依赖）
//!   layer 4  kernel-session / kernel-llm / kernel-tools / kernel-storage
//!   layer 3  kernel-loop / kernel-supervisor
//!   layer 2  kernel-assembly    组合根
//!   layer 1  headless           二进制
//!
//! 规则：crate 只能依赖层号 >= 自己的 workspace crate；向上依赖即违规。
//! 用 Rust 实现（不依赖外部 Python/bash），`cargo test --workspace` 即门禁。

use std::path::Path;

/// 已知分层表；未知 crate 不参与检查。
fn layer_of(crate_name: &str) -> Option<u32> {
    Some(match crate_name {
        "kernel-contracts" => 5,
        "kernel-session" | "kernel-llm" | "kernel-tools" | "kernel-storage" => 4,
        "kernel-loop" | "kernel-supervisor" => 3,
        "kernel-assembly" => 2,
        "headless" => 1,
        _ => return None,
    })
}

/// 从 Cargo.toml 里收集对本 workspace crate 的依赖。
/// 朴素解析：匹配 `name = {` 或 `name = "` 行，过滤 workspace crate 名。
fn workspace_deps(manifest: &str) -> Vec<String> {
    let mut deps = Vec::new();
    for line in manifest.lines() {
        let line = line.trim();
        let Some(eq) = line.find('=') else {
            continue;
        };
        let name = line[..eq].trim();
        if name.starts_with("kernel-") || name == "headless" {
            deps.push(name.to_string());
        }
    }
    deps
}

#[test]
fn dependencies_are_downward_only() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut checked = 0;
    for entry in std::fs::read_dir(&root).expect("kernel workspace dir") {
        let entry = entry.unwrap();
        let crate_dir = entry.path();
        let crate_name = crate_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap()
            .to_string();
        if layer_of(&crate_name).is_none() {
            continue;
        }
        let manifest = crate_dir.join("Cargo.toml");
        if !manifest.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest).unwrap();
        let my_layer = layer_of(&crate_name).unwrap();
        for dep in workspace_deps(&text) {
            if let Some(dep_layer) = layer_of(&dep) {
                assert!(
                    dep_layer >= my_layer,
                    "VIOLATION: {crate_name} (layer {my_layer}) depends upward on {dep} (layer {dep_layer})"
                );
            }
        }
        checked += 1;
    }
    assert!(checked >= 8, "expected >=8 manifests checked, got {checked}");
    println!("ok: {checked} manifests, downward-only workspace deps");
}
