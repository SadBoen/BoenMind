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
///
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
