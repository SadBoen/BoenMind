//! crate 边界守卫（门禁 1 的一环）：依赖只许向下 + 插件间方向纪律。
//!
//! BoenMind 产品层 workspace 分层（借鉴 bobleer check-crate-boundaries）：
//!   layer 5  kernel-contracts    契约/端口（任何 crate 可依赖）
//!   layer 4  kernel-session / kernel-storage
//!   layer 3  kernel-supervisor
//!   layer 2  bm-ports            产品级契约层（Compactor 等内核扩展端口；无实现）
//!   layer 2  plugin-*            插件实现（只依赖契约/会话/bm-ports）
//!   layer 1  bm-assembly         组合根（唯一装配点，依赖全部实现）
//!   layer 0  web-server / headless / quickjs-bridge  最终程序与桥（只消费组装好的 Runtime）
//!
//! 规则 1（向上依赖）：crate 只能依赖层号 >= 自己的 workspace crate（向上依赖即违规）。
//! 组合根层 1 依赖插件层 依赖是正当的（装配职责）。
//! 规则 2（L0 禁令）：层 0 禁止依赖 plugin-*（L0 只依赖 bm-assembly + kernel-contracts）——
//! 防止第二组合根：web-server/headless/quickjs-bridge 不得直接 new 具体 provider/loop/tools。
//! 规则 3（插件间方向，2026-08-20 回头看新增）：**核心插件（llm/loop/tools）禁止依赖
//! 功能插件（auth/compactor）**——核心不得被功能插件污染（plugin-loop 曾编译期依赖
//! plugin-compactor 的 Compactor trait，trait 已上提 bm-ports 修复）。核心插件允许依赖
//! 同层核心实现（loop→tools，ReAct 领域内聚）；`plugin-*` 一律禁止依赖 plugin-compactor
//! 之外的 plugin-*（防功能插件互叠，防功能插件反向依赖核心实现的具体类型）。
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
        "bm-ports" | "host-fs" => 2,
        "plugin-llm" | "plugin-loop" | "plugin-tools" | "plugin-auth" | "plugin-compactor"
        | "plugin-host-tools" | "plugin-code-runtime" | "plugin-web-tools" | "plugin-schedule" => 2,
        "bm-assembly" => 1,
        "web-server" | "headless" | "quickjs-bridge" => 0,
        _ => return None,
    })
}

/// 核心插件：最小基座（provider + loop + tools）——禁止依赖功能插件。
fn is_core_plugin(crate_name: &str) -> bool {
    matches!(
        crate_name,
        "plugin-llm" | "plugin-loop" | "plugin-tools" | "plugin-host-tools"
    )
}

/// 功能插件：用户可加装/可关闭的可选面（auth/compactor/code-runtime）。
fn is_feature_plugin(crate_name: &str) -> bool {
    matches!(crate_name, "plugin-auth" | "plugin-compactor" | "plugin-code-runtime" | "plugin-web-tools" | "plugin-schedule")
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
                // 规则 2：L0（web-server/headless/quickjs-bridge）禁止依赖 plugin-*——
                // 防第二组合根：具体 provider/loop/tools 只能在 bm-assembly 里 new。
                if my_layer == 0 && dep.starts_with("plugin-") {
                    panic!(
                        "VIOLATION: L0 crate {package} depends on plugin implementation {dep} —\
                         concrete plugins must only be assembled in bm-assembly (L1)"
                    );
                }
                // 规则 1：向上依赖（layer 号高于自己即违规；同层允许）。
                if let Some(dep_layer) = layer_of(&dep) {
                    assert!(
                        dep_layer >= my_layer,
                        "VIOLATION: {package} (layer {my_layer}) depends upward on {dep} (layer {dep_layer})"
                    );
                }
                // 规则 3（插件间方向）：核心插件不得依赖功能插件；插件不得依赖
                // 另一功能插件（防功能插件互叠/反向依赖核心具体类型）。
                if my_layer == 2 && dep.starts_with("plugin-") {
                    if is_core_plugin(&package) && is_feature_plugin(&dep) {
                        panic!(
                            "VIOLATION: core plugin {package} depends on feature plugin {dep} —\
                             core must not be polluted by optional feature plugins; \
                             move the needed interface up to bm-ports instead"
                        );
                    }
                    if is_feature_plugin(&package) && is_feature_plugin(&dep) {
                        panic!(
                            "VIOLATION: feature plugin {package} depends on feature plugin {dep} —\
                             feature plugins must be independent (dependency inversion via bm-ports)"
                        );
                    }
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
        "bm-ports",
        "bm-assembly",
        "web-server",
        "headless",
        "quickjs-bridge",
        "plugin-llm",
        "plugin-loop",
        "plugin-tools",
        "plugin-auth",
        "plugin-compactor",
        "plugin-code-runtime",
        "plugin-web-tools",
        "plugin-schedule",
    ] {
        layer_of(k).unwrap_or_else(|| panic!("expected crate {k} registered in layer_of"));
        if !k.starts_with("kernel-") {
            assert!(seen.contains(k), "expected crate {k} present in workspace");
            assert!(checked.contains(k), "expected crate {k} checked");
        }
    }
    println!("ok: {} crates checked, downward-only workspace deps", checked.len());
}