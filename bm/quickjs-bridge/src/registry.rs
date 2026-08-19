//! 目录插件注册表（§5.6）：扫描插件目录 → `LoadedPlugin` 清单。
//!
//! 一个 JS 插件 = 一个文件夹（含 `plugin.json` + 入口 JS，见 [`super::plugin`]）。
//! 注册表只做**发现 + 装载（读盘）**，不建引擎——引擎（`JsBridge`）由组合根
//! 按 manifest 授面逐个装配（`bm-assembly::Runtime::js_bridge`），保持
//! "组合根唯一装配点"纪律。每插件一引擎（独立 AsyncRuntime + 上下文），
//! 插件间天然隔离（全局变量/异常互不干扰）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::plugin::LoadedPlugin;

/// 递归扫描 `dir` 下所有含 `plugin.json` 的目录，返回装载后的插件清单。
///
/// - 目录树任意深度；`plugin.json` 缺失的目录跳过（不是错误）；
/// - **同名 `id` 冲突：靠后扫描的覆盖靠前**（后序覆盖；顺序 = 目录字典序
///   DFS，稳定可预测——外层同层按名称排序，后扫的赢，便于覆盖默认）；
/// - 单个插件 manifest/入口读盘失败 → 返回 `Err`（fail-loud，不静默跳过
///   损坏插件——防止拼错/缺文件静默失效）。
pub fn scan_plugins(dir: &Path) -> Result<Vec<LoadedPlugin>, String> {
    let mut by_id: BTreeMap<String, LoadedPlugin> = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d)
            .map_err(|e| format!("read dir {}: {e}", d.display()))?;
        let mut subdirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort(); // 字典序 → 后 pop 的先扫（同层名称排序，稳定可预测）
        // 本目录若是插件（有 plugin.json），先装载；再压子目录。
        let manifest_path = d.join("plugin.json");
        if manifest_path.is_file() {
            let loaded = LoadedPlugin::load(&d)?;
            by_id.insert(loaded.manifest.id.clone(), loaded);
        }
        for sub in subdirs.into_iter().rev() {
            stack.push(sub);
        }
    }
    Ok(by_id.into_values().collect())
}

/// 装载单个插件目录（便捷入口；错误语义同 [`LoadedPlugin::load`]）。
pub fn load_plugin(dir: &Path) -> Result<LoadedPlugin, String> {
    LoadedPlugin::load(dir)
}

/// 插件注册表：`(插件目录, LoadedPlugin)` 的只读视图（组合根装配用）。
pub struct PluginDir {
    root: PathBuf,
    plugins: Vec<LoadedPlugin>,
}

impl PluginDir {
    /// 扫描并建立注册表视图。`plugins()` 按 id 字典序。
    pub fn scan(root: &Path) -> Result<Self, String> {
        let plugins = scan_plugins(root)?;
        Ok(Self { root: root.to_path_buf(), plugins })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn plugins(&self) -> &[LoadedPlugin] {
        &self.plugins
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qjs-reg-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_plugin(dir: &Path, id: &str, host: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("plugin.json"),
            format!(
                r#"{{"id":"{id}","name":"{id}","entry":"main.js","host":[{host}]}}"#
            ),
        )
        .unwrap();
        fs::write(dir.join("main.js"), format!("host.log('info', '{id}');")).unwrap();
    }

    #[test]
    fn scans_nested_dirs_and_skips_non_plugins() {
        let root = tmp_dir("nested");
        write_plugin(&root.join("p1"), "p1", "\"log\"");
        write_plugin(&root.join("sub").join("deep").join("p2"), "p2", "\"tools.list\"");
        // 无 plugin.json 的目录跳过。
        fs::create_dir_all(root.join("sub").join("not-a-plugin")).unwrap();
        fs::write(root.join("sub").join("not-a-plugin").join("main.js"), "x").unwrap();

        let list = scan_plugins(&root).unwrap();
        let ids: Vec<&str> = list.iter().map(|p| p.manifest.id.as_str()).collect();
        assert_eq!(ids, vec!["p1", "p2"]); // 字典序
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_id_last_scanned_wins() {
        // 同层目录按字典序 DFS："base" < "override" → base 先扫、override 后扫，
        // 后者覆盖前者（后扫覆盖，便于覆盖默认插件）。
        let root = tmp_dir("dup");
        write_plugin(&root.join("base").join("p1"), "p1", "\"log\"");
        write_plugin(&root.join("override").join("p1"), "p1", "\"llm.complete\"");
        let list = scan_plugins(&root).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].manifest.id, "p1");
        assert_eq!(list[0].manifest.host, vec!["llm.complete".to_string()]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_plugin_fails_loud() {
        let root = tmp_dir("corrupt");
        write_plugin(&root.join("good"), "good", "\"log\"");
        fs::create_dir_all(root.join("bad")).unwrap();
        fs::write(root.join("bad").join("plugin.json"), "{not json").unwrap();
        let r = scan_plugins(&root);
        assert!(r.is_err(), "损坏插件必须 fail-loud，不静默跳过");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_dir_view_sorted_and_stable() {
        let root = tmp_dir("view");
        write_plugin(&root.join("b"), "b", "\"log\"");
        write_plugin(&root.join("a"), "a", "\"log\"");
        let view = PluginDir::scan(&root).unwrap();
        let ids: Vec<&str> = view.plugins().iter().map(|p| p.manifest.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert!(!view.is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
