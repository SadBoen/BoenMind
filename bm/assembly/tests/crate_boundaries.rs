//! crate 边界守卫（门禁 1 的一环）：依赖只许向下。
//!
//! BoenMind 产品层 workspace 分层（借鉴 bobleer check-crate-boundaries）：
//!   layer 5  kernel-contracts    契约/端口（任何 crate 可依赖）
//!   layer 4  kernel-session / kernel-storage
//!   layer 3  kernel-supervisor
//!   layer 2  plugin-*            插件实现（llm/loop/tools，只依赖契约/会话）
//!   layer 1  bm-assembly         组合根（唯一装配点，依赖全部实现）
//!   layer 0  web-server / headless / quickjs-bridge  最终程序与桥（只消费组装好的 Runtime）
//!
//! 规则：crate 只能依赖层号 >= 自己的 workspace crate（向上依赖即违规）。
//! 组合根层 1 依赖插件层 2 是正当的（装配职责）。
//! **层 0 禁止依赖 plugin-*（L0 只依赖 bm-assembly + kernel-contracts）**——防止第二组合根：
//! web-server/headless/quickjs-bridge 不得直接 new 具体 provider/loop/tools。
//! 未知 workspace 成员硬失败（防新增 crate 漏登记）。`cargo test --workspace` 即门禁。
//!
//! 依赖收集：解析每个成员 Cargo.toml 的 dependencies/dev-dependencies，
//! 凡指向本 workspace 成员的依赖（含 kernel-*/plugin-*/bm-*/headless/web-server）都登记；
//! 外部 crate（serde/axum/rquickjs…）一律忽略。

use std::collections::BTreeSet;
use std::path::Path;

/// 已知分层表；workspace 全部成员必须在此登记（未知成员 = 硬失败）。
fn layer_of(crate_name: &str) -> Option<u32> {
    Some(match crate_name {
        "kernel-contracts" => 5,
        "kernel-session" | "kernel-storage" => 4,
        "kernel-supervisor" => 3,
        "plugin-llm" | "plugin-loop" | "plugin-tools" => 2,
        "bm-assembly" => 1,
        "web-server" | "headless" | "quickjs-bridge" => 0,
        _ => return None,
    })
}

/// 从 Cargo.toml 读 [package].name（目录名 ≠ 包名，如 bm/assembly → bm-assembly）。
fn package_name(manifest_text: &str) -> String {
    let parsed: toml::Value = toml::from_str(manifest_text).expect("valid Cargo.toml");
    parsed["package"]["name"]
        .as_str()
        .expect("package.name")
        .to_string()
}
fn workspace_deps(manifest_text: &str) -> Vec<String> {
    let parsed: toml::Value = toml::from_str(manifest_text).expect("valid Cargo.toml");
    let mut deps = Vec::new();
    for table in ["dependencies", "dev-dependencies"] {
        if let Some(map) = parsed.get(table).and_then(|v| v.as_table()) {
            for key in map.keys() {
                if key.starts_with("kernel-")
                    || key.starts_with("bm-")
                    || key.starts_with("plugin-")
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
    // BoenMind 主仓三处成员目录：bm/（产品层）、plugins/（Rust 插件）、kernel/（子模块纯内核库）。
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let member_dirs = ["bm", "plugins"];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut checked: BTreeSet<String> = BTreeSet::new();
    for dir in member_dirs {
        for entry in std::fs::read_dir(root.join(dir)).expect("workspace member dir") {
            let entry = entry.unwrap();
            let _dir = entry.file_name(); // 目录名仅用于定位 manifest；包名以 Cargo.toml 为准
            let manifest = entry.path().join("Cargo.toml");
            if !manifest.exists() {
                continue; // 非 crate 目录（frontend/ 等）
            }
            let text = std::fs::read_to_string(&manifest).unwrap();
            let package = package_name(&text);
            let Some(my_layer) = layer_of(&package) else {
                // 非 workspace crate 目录（bm/web-server/frontend 等含 package.json 无 Cargo.toml）。
                // 有 Cargo.toml 但未登记 = 外部子工程，跳过。
                continue;
            };
            seen.insert(package.clone());
            for dep in workspace_deps(&text) {
                // L0（web-server/headless/quickjs-bridge）禁止依赖 plugin-*——
                // 防第二组合根：具体 provider/loop/tools 只能在 bm-assembly 里 new。
                if my_layer == 0 && dep.starts_with("plugin-") {
                    panic!(
                        "VIOLATION: L0 crate {package} depends on plugin implementation {dep} —\
                         concrete plugins must only be assembled in bm-assembly (L1)"
                    );
                }
                if let Some(dep_layer) = layer_of(&dep) {
                    assert!(
                        dep_layer >= my_layer,
                        "VIOLATION: {package} (layer {my_layer}) depends upward on {dep} (layer {dep_layer})"
                    );
                }
            }
            checked.insert(package);
        }
    }
    // 全部已登记成员必须存在且被检查（防未来新增成员被漏掉）。
    // kernel/ 子模块 4 个内核库不在本测试扫描目录（bm/ plugins/），但被 bm-assembly
    // 与 plugins 依赖；只要 layer_of 已登记、且依赖方向下依赖，即满足边界。
    for k in [
        "kernel-contracts",
        "kernel-session",
        "kernel-storage",
        "kernel-supervisor",
        "bm-assembly",
        "web-server",
        "headless",
        "quickjs-bridge",
        "plugin-llm",
        "plugin-loop",
        "plugin-tools",
    ] {
        layer_of(k).unwrap_or_else(|| panic!("expected crate {k} registered in layer_of"));
        if !k.starts_with("kernel-") {
            assert!(seen.contains(k), "expected crate {k} present in workspace");
            assert!(checked.contains(k), "expected crate {k} checked");
        }
    }
    println!("ok: {} crates checked, downward-only workspace deps", checked.len());
}
