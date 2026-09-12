//! 插件契约(ADR-0041):「万物皆插件」的可寻址身份。
//!
//! 此前系统只有**行为** trait(能力 `invoke`)与 manifest 数据,**没有插件身份**
//! ——内核无法回答「这是什么类型的扩展、由谁提供、何时起停」。本模块补上契约层
//! 缺的纯数据部分(kind/id/version);行为面的生命周期 trait 在
//! `bm_core::registry::CapabilityProvider`(与其余端口同处,遵循
//! 「bm-contract 放数据、bm-core 放行为」的既有分层)。
//!
//! 本模块只做**声明**;现有内置能力/provider 全部非破坏兼容:未声明身份的
//! 提供者按 [`PluginKind::Tool`] 对待(见 `CapabilityProvider::plugin_meta` 默认实现)。

wire_str_enum!(PluginKind {
    Tool => "tool",
    Connector => "connector",
    Store => "store",
    Surface => "surface",
    Judge => "judge",
    Sandbox => "sandbox",
});

/// 插件身份:id 全局唯一(kebab 或点分命名空间),version 语义化,kind 归族。
///
/// 与 [`crate::capability::CapabilityManifest`] 的分工:manifest 描述**单个能力**
/// 的调用契约(入出参、风险、审批);PluginMeta 描述**提供者**的身份与族属——
/// 一个插件可提供多个能力(如一个 wasm 插件导出多个工具)。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginMeta {
    pub id: String,
    pub version: String,
    pub kind: PluginKind,
}

impl PluginMeta {
    pub fn new(id: impl Into<String>, version: impl Into<String>, kind: PluginKind) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
            kind,
        }
    }

    /// 未声明身份时的缺省:工具型、版本未知。
    pub fn tool_unknown(id: impl Into<String>) -> Self {
        Self::new(id, "0.0.0", PluginKind::Tool)
    }
}
