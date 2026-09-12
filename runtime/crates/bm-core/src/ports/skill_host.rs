//! wasm 插件宿主端口(ADR-0046):管理面驱动宿主装载/摘除能力的契约。
//!
//! 背景:`bm-surface-http` 的插件/技能管理面(热重载、卸载)原先直接持具体
//! `bm_providers::skill_wasm::SkillScriptManager`,使 surface 常规依赖
//! `bm-providers`——与已正确反转的 `ModelRouter` 形成"半成品反转"。本端口把
//! 管理面**实际用到**的宿主操作上移到 core,surface 只依赖端口。
//!
//! 与 `CapabilityProvider` 的分工:后者是**执行面**(单能力 `invoke`),本端口是
//! **管理面**(装载声明、注册/摘除一组能力)。执行仍走 `AsyncCapabilityExecutor`。

use bm_contract::capability::CapabilityManifest;
use bm_contract::plugin::{PluginKind, PluginMeta};
use bm_contract::skill::SkillDefinition;
use std::path::Path;
use std::sync::Arc;

use crate::registry::CapabilityProvider;

/// wasm 能力宿主的管理面契约。实现方 = `bm_providers::skill_wasm::SkillScriptManager`。
pub trait SkillHost: Send + Sync {
 /// 从**声明文件**装载通用 wasm 插件(ADR-0041);返回合成 manifests。
    fn load_plugins_file(&self, decl_path: &Path) -> Vec<CapabilityManifest>;

 /// 注册一个技能的全部脚本(ADR-0016);返回合成 manifests。
    fn register_skill(
        &self,
        skill_id: &str,
        def: &SkillDefinition,
        skill_root: &Path,
    ) -> Result<Vec<CapabilityManifest>, String>;

 /// 按技能 id 摘除其全部能力,返回被摘除的 capability 名(ADR-0033)。
    fn unregister_skill(&self, skill_id: &str) -> Vec<String>;

 /// 摘除全部**通用插件**来源的能力(ADR-0042:按装载来源,不按名字前缀)。
    fn unregister_all_generic(&self) -> Vec<String>;
}

/// manifests → 待注册能力对(占位 Provider;真正执行走异步分道)。
/// 从 `SkillScriptManager::capability_entries` 上移为 core 自由函数(ADR-0046):
/// 实现只用到 contract 类型与 core 的 `provider_fn_with_meta`,故归 core 最合适
/// ——surface 构造注册对时不再需要 `bm-providers`。每个 wasm 插件以
/// [`PluginKind::Tool`] 声明身份,id 取 manifest 的 provider 字段。
pub fn placeholder_entries(
    manifests: Vec<CapabilityManifest>,
) -> Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)> {
    manifests
        .into_iter()
        .map(|m| {
            let meta = PluginMeta::new(m.provider.clone(), m.version.clone(), PluginKind::Tool);
            let handle =
                crate::broker::provider_fn_with_meta(meta, |_| Err("wasm 能力仅限异步路径".into()));
            (m, handle)
        })
        .collect()
}

// ---- ADR-0053:wasm 家族声明装载单入口 ---------------------------------------
// 背景:wasm 家族有两个磁盘声明源(技能 `skills.json` + 通用插件 `plugins.json`),
// 「读哪个文件、何种形状、哪些条目算声明、技能根目录在哪」这套知识
// 启动装配(bin)与管理面(surface)两侧各自实现**,同一形状解析多份。
// 现收口:读取与归一化在此单源;装载方只调 [`load_wasm_declarations`]。

/// 技能声明文件路径(`<data>/config/skills.json`)。
pub fn skills_config_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("config").join("skills.json")
}

/// 通用插件声明文件路径(`<data>/config/plugins.json`)。
pub fn plugins_config_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("config").join("plugins.json")
}

/// 技能脚本根目录(`<data>/skills/<skill_id>`)。
pub fn skill_root(data_dir: &Path, skill_id: &str) -> std::path::PathBuf {
    data_dir.join("skills").join(skill_id)
}

/// 读 `<data>/config/skills.json`,返回**声明了 scripts 的技能**(id, 定义)。
/// 唯一读取点:启动装载与管理面单技能热重载共用同一形状解析,消除「同一文件
/// 两处各解一遍」。缺文件/损坏/结构化失败一律返回空表(纯知识包技能被跳过),
/// 与收口前两侧的宽容口径一致——技能面不因单个文件损坏拖垮内核启动。
pub fn read_skill_definitions(data_dir: &Path) -> Vec<(String, SkillDefinition)> {
    let Some(v) = crate::json_store::read_json_lenient(&skills_config_path(data_dir)) else {
        return Vec::new();
    };
    let Some(list) = v["skills"].as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|sk| {
            let id = sk["skill_id"].as_str()?;
 // 纯知识包(无 scripts)不是能力来源,跳过。
            sk.get("scripts")?;
            let def = serde_json::from_value::<SkillDefinition>(sk.clone()).ok()?;
            Some((id.to_string(), def))
        })
        .collect()
}

/// 装载 wasm 家族**全部磁盘声明**为待注册能力对(ADR-0053 单入口):
/// 技能(`skills.json` 中带 scripts 者)在先,通用插件(`plugins.json`)其后。
/// 启动装配与整表重载共用此唯一入口;单项粒度(单技能/单插件)的重载仍走
/// `SkillHost::register_skill` / `unregister_*`——粒度差异是语义,不合并。
/// 单条声明非法由宿主内部跳过并告警,不阻断整体装载。
pub fn load_wasm_declarations(
    host: &dyn SkillHost,
    data_dir: &Path,
) -> Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)> {
    let mut out: Vec<(CapabilityManifest, Arc<dyn CapabilityProvider>)> = Vec::new();
 // 技能:逐条编译注册(已声明 scripts 者)。
    for (id, def) in read_skill_definitions(data_dir) {
        match host.register_skill(&id, &def, &skill_root(data_dir, &id)) {
            Ok(manifests) => out.extend(placeholder_entries(manifests)),
            Err(e) => eprintln!("[Skill] 技能 {id} 装载失败(已跳过): {e}"),
        }
    }
 // 通用插件:声明文件整表装载。
    out.extend(placeholder_entries(
        host.load_plugins_file(&plugins_config_path(data_dir)),
    ));
    out
}

#[cfg(test)]
mod loader_tests {
    use super::*;
    use serde_json::json;

 /// wasm 家族声明读取单入口(ADR-0053):只挑声明了 scripts 的技能,
 /// 纯知识包跳过;缺文件/损坏返回空表(不阻断启动)。
 #[test]
    fn read_skill_definitions_selects_scripted_only() {
        let dir = tempfile::tempdir().expect("临时目录");
        let cfg = dir.path().join("config");
        std::fs::create_dir_all(&cfg).expect("建 config");

 // 缺文件 → 空
        assert!(read_skill_definitions(dir.path()).is_empty());

        std::fs::write(
            skills_config_path(dir.path()),
            json!({"skills": [
                {"skill_id": "kb_only", "name": "纯知识", "instruction": "x"},
                {"skill_id": "with_scripts", "name": "带脚本", "instruction": "y",
                 "scripts": [{"name": "s", "path": "s.wasm", "effect": "read-only",
                              "input_schema": {"type": "object"},
                              "output_schema": {"type": "object"}}]}
            ]})
            .to_string(),
        )
        .expect("写 skills.json");

        let defs = read_skill_definitions(dir.path());
        assert_eq!(defs.len(), 1, "只应取带 scripts 的技能");
        assert_eq!(defs[0].0, "with_scripts");
        assert!(defs[0].1.scripts.is_some());

 // 损坏 → 空表(宽容,不阻断内核启动)
        std::fs::write(skills_config_path(dir.path()), b"{not json").expect("写坏文件");
        assert!(read_skill_definitions(dir.path()).is_empty());
    }

 #[test]
    fn config_and_root_paths_are_single_sourced() {
        let d = std::path::Path::new("/data");
        assert!(skills_config_path(d).ends_with("config/skills.json"));
        assert!(plugins_config_path(d).ends_with("config/plugins.json"));
        assert!(skill_root(d, "sk").ends_with("skills/sk"));
    }
}
