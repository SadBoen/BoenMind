---
status: accepted
date: 2026-09-12
summary: 接线插件身份——CapabilityDiscovery 承载身份成为真实消费者、MCP/内核能力补声明、PluginKind 收敛为 Tool/Connector、移除空转 shutdown 钩子
supersedes: []
superseded_by: []
---

# ADR-0045: 插件身份接线与过度建模收敛(PluginKind 6→2、移除空转 shutdown)

- 关联: ADR-0041(插件身份契约)、ADR-0042(核实轮方法论)、ADR-0036(声明为唯一真源)
- 背景: 重新评估发现 ADR-0041 引入的插件身份层是**空转的仪式**——`plugin_meta_of` 只有测试调用;`PluginKind` 6 变体中 `Store/Surface/Judge/Sandbox` **全仓零构造**(4/6 死变体);`shutdown` 无任何生产实现(唯二出现是 trait 默认与测试替身);`CapabilityDiscovery` 不含身份;无 HTTP 暴露;MCP 占位 provider 不声明身份。这是"契约比代码激进"的实例,且是本次改造自身引入的。

## 决策

1. **让发现面成为身份的真实消费者**。`CapabilityDiscovery`(注册表机器可读发现面,`registry.rs`)增 `plugin_kind`/`plugin_id`/`plugin_version`,`discover()` 经 `plugin_meta_of` 取值。经 `capability.list` 自动流出(该处理器直接序列化整个 `CapabilityDiscovery`),**无需改 wire**。
2. **管理面透出身份**。`boenmind-server` 的 `builtin_caps` 快照(`/admin/capabilities` 的 `builtin` 项)增 `plugin_kind`/`plugin_id`/`plugin_version`。
3. **前端按真实身份渲染**。"插件中心"表格行增**身份徽标**(取 `plugin_kind`,悬停显示 id/version),与既有的来源分类(builtin/external/wasm）**正交**——不再靠前端按来源猜测身份。
4. **补全身份声明**:MCP 占位 provider 改用 `provider_fn_with_meta`(id = `mcp.<server>`,`Tool`);内核 `task.share.*` 也声明 `kernel.share`/`Tool`。此前仅内置 fs/exec/context/model 声明。
5. **收敛过度建模(裁掉空转变体)**:`PluginKind` 由 6 变体收敛为 **`Tool`/`Connector`**——只保留确有插件实现的族。`Store`(实为 `EventStore` 端口)、`Surface`(壳)、`Judge`/`Sandbox`(硬编码 crate)从无插件实现,挂"插件家族"名不副实。需要时按 ADR 增发。
6. **移除空转 `shutdown` 钩子**。资源归**执行器**而非 provider 占位符所有:wasm 模块由 `SkillScriptManager::unregister_provider` 摘除、MCP 子进程由 `McpHub::disconnect_server` 清理。留一个永远 `Ok(())` 的钩子是空转,故从 trait 移除;`CapabilityRegistry::unregister` 恢复为纯摘除。

## 后果

- **身份层不再空转**:`capability.list` / `/admin/capabilities` 承载身份;前端徽标渲染;MCP 与内核能力均声明。守护测试改为 `plugin_identity_is_consumed_by_discovery`(断言发现面承载身份 + 未声明者为空 + 注销后消失)。
- **真浏览器手测**:`/admin/capabilities` 返回 `model.invoke→connector/kernel.model.invoke`、`system.exec`/`fs.*→tool/kernel.exec|kernel.fs`、`context.compress→tool`;插件页表格行显示 `connector`/`tool` 身份徽标。
- **零行为变更**:501 测试全绿;`clippy -D warnings` 零警告;前端 build 通过、lint 无新增。
- 6 变体 enum → 2 变体,消除 4 个永不被构造的"未来占位";`shutdown` 移除,消除一个永不生效的钩子——**两个"为不存在的需求建模"的实例就此收敛**。
