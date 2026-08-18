//! crate 边界守卫（门禁 1 的一环）：依赖只许向下。
//!
//! 分层（借鉴 bobleer check-crate-boundaries，路径前缀定层）：
//!   layer 5  kernel-contracts   契约/端口（任何 crate 可依赖）
//!   layer 4  kernel-session / kernel-llm / kernel-tools / kernel-storage
//!   layer 3  kernel-loop / kernel-supervisor
//!   layer 2  kernel-assembly    组合根
//!   layer 1  headless / web-server  消费壳层
//!
//! 规则：crate 只能依赖层号 >= 自己的 workspace crate；向上依赖即违规。
//! 用 toml 解析 manifest（注释/多行/数组写法均正确识别），未知 kernel-* 成员
//! 硬失败（防新增 crate 漏登记）。`cargo test --workspace` 即门禁。

use std::collections::BTreeSet;
use std::path::Path;

/// 已知分层表；workspace 全部成员必须在此登记（未知成员 = 硬失败）。
fn layer_of(crate_name: &str) -> Option<u32> {
    Some(match crate_name {
        "kernel-contracts" => 5,
        "kernel-session" | "kernel-llm" | "kernel-tools" | "kernel-storage" => 4,
        "kernel-loop" | "kernel-supervisor" => 3,
        "kernel-assembly" => 2,
        "headless" | "web-server" => 1,
        _ => return None,
    })
}

/// 从 Cargo.toml 收集对本 workspace crate 的依赖（正式 + dev）。
fn workspace_deps(manifest_text: &str) -> Vec<String> {
    let parsed: toml::Value = toml::from_str(manifest_text).expect("valid Cargo.toml");
    let mut deps = Vec::new();
    for table in ["dependencies", "dev-dependencies"] {
        if let Some(map) = parsed.get(table).and_then(|v| v.as_table()) {
            for key in map.keys() {
                if key.starts_with("kernel-")
                    || key == "headless"
                    || key == "web-server"
                {
                    deps.push(key.clone());
                }
            }
        }
    }
    deps
}

#[test]
fn dependencies_are_downward_only() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut checked: BTreeSet<String> = BTreeSet::new();
    for entry in std::fs::read_dir(&root).expect("kernel workspace dir") {
        let entry = entry.unwrap();
        let crate_name = entry.file_name().to_string_lossy().to_string();
        let manifest = entry.path().join("Cargo.toml");
        if !manifest.exists() {
            continue; // 非 crate 目录（target/ 等）
        }
        let is_kernelish = crate_name.starts_with("kernel-")
            || crate_name == "headless"
            || crate_name == "web-server";
        let Some(my_layer) = layer_of(&crate_name) else {
            assert!(
                !is_kernelish,
                "VIOLATION: crate {crate_name} has Cargo.toml but is not registered in layer_of"
            );
            continue; // 杂项目录（自带 Cargo.toml 的非 workspace 成员）
        };
        seen.insert(crate_name.clone());
        let text = std::fs::read_to_string(&manifest).unwrap();
        for dep in workspace_deps(&text) {
            if let Some(dep_layer) = layer_of(&dep) {
                assert!(
                    dep_layer >= my_layer,
                    "VIOLATION: {crate_name} (layer {my_layer}) depends upward on {dep} (layer {dep_layer})"
                );
            }
        }
        checked.insert(crate_name.clone());
    }
    // 全部已登记成员必须存在且被检查（防未来新增成员被漏掉）。
    for k in [
        "kernel-contracts",
        "kernel-session",
        "kernel-llm",
        "kernel-tools",
        "kernel-storage",
        "kernel-loop",
        "kernel-supervisor",
        "kernel-assembly",
        "headless",
        "web-server",
    ] {
        assert!(seen.contains(k), "expected crate {k} present in workspace");
        assert!(checked.contains(k), "expected crate {k} checked");
    }
    println!("ok: {} crates checked, downward-only workspace deps", checked.len());
}
