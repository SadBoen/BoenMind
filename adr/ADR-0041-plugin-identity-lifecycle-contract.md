---
status: accepted
date: 2026-09-12
summary: 补插件身份契约(PluginKind/PluginMeta)+ CapabilityProvider 增 plugin_meta/shutdown 默认方法 + 注销调生命周期 + wasm 宿主去 skill. 前缀守卫(ADR-0040 治理首批架构改动)
supersedes: []
superseded_by: []
---

# ADR-0041: 插件身份与生命周期契约落地(去特化第一步)

- 关联: ADR-0005(万物皆插件)、ADR-0016/0033(skill wasm 执行面)、ADR-0036(执行分道以合同声明为真源)、ADR-0040(文档治理)
- 背景: 系统的扩展面长期只有**行为** trait(`CapabilityProvider::invoke`)与 manifest 数据,**没有插件身份与生命周期**——内核无法回答"这是什么类型的扩展、由谁提供、何时起停",`unregister` 直接摘除句柄、不通知 Provider;wasm 宿主(`skill_wasm.rs`)虽已具备"编译 → 合成 manifest → 走 Broker 平权管线"的通用执行形态,却被 `skill.` 命名与 `SkillScriptDefinition` 绑住。ADR-0005「万物皆插件」的支点在代码里缺一件契约。

## 决策

1. **契约层补插件身份(ID)**。`bm-contract` 新增 `plugin` 模块:`PluginKind`(tool/connector/store/surface/judge/sandbox)与 `PluginMeta{id, version, kind}` 纯数据。遵循既有分层——`bm-contract` 放数据、`bm-core` 放行为。与 `CapabilityManifest` 分工:manifest 描述**单个能力**的调用契约,PluginMeta 描述**提供者**的身份与族属(一个插件可提供多个能力)。

2. **行为层补插件契约(默认实现,非破坏)**。`CapabilityProvider` 增两个带默认实现的方法:`plugin_meta() -> Option<PluginMeta>`(缺省 `None` = 按 `PluginKind::Tool` 对待)与 `shutdown() -> Result<(), String>`(缺省空实现 = 纯函数型 provider 无需实现)。既有 4 处实现零改动即满足契约。

3. **生命周期被真正调用**。`CapabilityRegistry::unregister` 在摘除前先调 `shutdown()`,失败仅告警不阻断(绑定已失效,进程回收兜底);新增 `plugin_meta_of(capability)` 读取能力所属插件身份。

4. **wasm 宿主去特化(第一步)**。删除 `SkillScriptManager as AsyncCapabilityExecutor` 里冗余的 `capability.starts_with("skill.")` 守卫——宿主本就按**精确 capability 查编译表**,前缀判断是冗余的字符串派发。删除后宿主对命名空间不可知:凡注册进其编译表的 wasm 能力皆可执行。生产路由仍由 `SplitExecutor` 按 capability 分道,本步不改路由。

5. **装载面抽出通用 API,真实 provider 声明身份**。
   - `SkillScriptManager::register_wasm(capability, wasm_path, root, timeout_ms)`:通用装载(任意 capability 名,校验 wasm 落在 `root` 内),`register_skill` 降为它的上层(命名/清单由 `SkillDefinition` 驱动)。
   - `SkillScriptManager::load_plugins_file(path)`:从**声明文件**装载通用 wasm 插件(每项声明 capability/provider/wasm/effect/timeout 等),是宿主在 `skills.json` 之外的第二个真实调用方。
   - `bm_core::broker::provider_fn_with_meta`:闭包型 provider 也能声明 `PluginMeta`。
   - 生产 provider 全部声明身份:`model.invoke`(Connector)、`fs.*`(Tool)、`system.exec`/`job_output`(Tool)、`context.compress`(Tool)、每个 wasm 插件(Tool,id = manifest.provider)。契约由此**被真实消费**,而非仅测试使用。

6. **分道按归属而非名字前缀**。`SplitExecutor` 对 wasm 分支改用 `SkillScriptManager::has_capability(capability)`(查宿主编译表),取代 `capability.starts_with("skill.")`。组合根 `boenmind-server` 启动时额外装载 `<data>/config/plugins.json`(与 `skills.json` 平级),与技能共用同一宿主实例。

## 后果

- 「万物皆插件」从口号进了一步:扩展有**类型与身份**,Provider 有**释放钩子**。这是把 `skill_wasm` 泛化为通用 wasm 插件宿主的前置契约面。
- **零破坏**:所有既有 provider/manifest/路由行为不变(496 测试全绿;新增 5 项覆盖身份读取、生命周期调用、wasm 身份声明、通用装载、声明文件装载)。
- **未做(留待后续 ADR)**:WIT/Component 级通用宿主接口(现为 WASI 命令式:stdin 进 JSON / stdout 出 JSON)、**通用插件的管理面**(扫描/批准/热重载——现只支持启动期从 `plugins.json` 装载)、插件依赖与版本协商。本 ADR 只落**契约、最小生命周期、通用装载面与第二个调用方**。
- 守护测试:`bm-core::registry::provider_lifecycle_and_plugin_meta_are_wired`(身份可读 + 注销必触发 shutdown + 未声明身份走默认)、`bm-providers::skill_wasm::{host_is_namespace_agnostic, capability_entries_declare_plugin_identity, generic_register_wasm_accepts_any_capability_name, load_plugins_file_registers_generic_wasm_capability}`。
