//! Loop 插件声明（核心分类）。
//!
//! 对应 dsh 官方 `@deepseek-ai/dsh-agent-loop`（Cordis 插件）。装配点 =
//! `Runtime::swap_loop` 处的会话代理工厂（见 kernel-assembly），本模块只
//! 声明清单身份。

use kernel_contracts::plugin::{PluginManifestEntry, PLUGIN_LOOP};

/// Loop 插件清单条目（核心组件）。
pub fn manifest() -> PluginManifestEntry {
    PluginManifestEntry::core(
        PLUGIN_LOOP,
        "Loop",
        "回合循环：turn/step 驱动，事件瀑布注入，模型可见即落日志",
    )
}
