# ADR-0033: 技能脚本生命周期——skill.* 异步分道归位与热重载

- 状态: Accepted（2026-09-11，issue #54 实施 + ADR-0016 第二步补正）
- 日期: 2026-09-11
- 关联: ADR-0016（wasmtime 脚本执行面与 Broker 七步管线覆盖）、ADR-0001 条件 2（binding_epoch 代际连续）、ADR-0032（注销墓碑化 + 注册期冻结门禁）、ADR-0006（权限以合同显式化）

## 背景

ADR-0016 第二步（wasmtime 脚本执行面）宣称脚本能力「与内置/MCP 能力完全平权地走 Broker 七步管线，执行体实现 `AsyncCapabilityExecutor`，与 MCP 同分道」。同轮坐实两处偏差：

1. **异步分道判定漏了 `skill.*`**。异步标记由装载方按 `manifest.provider` 显式打：启动注册只认 `mcp.` 前缀或 `.async` 后缀（`handle.rs`），而热注册只认 `mcp.` 前缀（`handlers.rs`）。技能脚本的 `provider = "skill.<id>"` 两条都不命中 → `is_async("skill.*")` 恒假 → 调用落到 `capability_entries` 的同步占位 provider（固定返回「skill 能力仅限异步路径」）。生产 wasm 脚本执行面**结构上不可达**，且全仓无集成测试覆盖，缺陷长期静默。
2. **无卸载/热重载路径**。`SkillScriptManager::entries` 进程期内只增不减；改技能脚本必须重启内核，误装载的坏/恶意脚本无法在线摘除。MCP 侧早有对等能力（`disconnect_server` + `capabilities_unregister`），技能面缺失。

## 决策

1. **异步分道判定收敛为唯一真源**。`CapabilityRegistry::provider_is_async(provider)` 定义命名约定：`mcp.` 前缀（外部子进程）、`.async` 后缀（内置异步体，如 `system.exec` 的 `builtin.async`）、`skill.` 前缀（wasm 脚本执行面）；`mark_async_for(capability, provider)` 依此自动标记。启动注册与热注册两处调用点共用，杜绝不对称。
2. **`SkillScriptManager::unregister_skill(skill_id)`**：按 `skill.<id>.` 前缀从编译缓存摘除条目并返回被摘除的 capability 名；未装载的 id 幂等返回空表。
3. **管理面热重载**。`/admin/skills` 的 POST（保存/覆盖）与 DELETE 后即重载：旧脚本能力经 `handle.capabilities_unregister` **墓碑化**（`status=unavailable`，代际不回退，复用 ADR-0032 机制）→ 管理器 `unregister_skill` 摘缓存 → 按最新 `skills.json` 定义 `register_skill` 重编译 → `handle.capabilities_register` 注册，异步分道由 [决策 1] 自动归属。动态响应 `note` 如实回报结果（不再笼统写「下一回合起生效」）。
4. **共享同一管理器实例**。`Arc<SkillScriptManager>` 由启动装载与 `SplitExecutor` 共用，并注入 `AdminConfig.skills`——热重载必须改这个实例的编译缓存，注册面与执行体才同源（新建管理器只注册 manifest 却无执行映射，是错误做法）。装配抽取为 `register_skill_scripts`，启动装载与热重载共用，歧义面收敛一处。
5. **失败留痕不阻断**。单技能编译/注册失败仅告警并如实回报，不拖垮其余技能或保存动作（与「技能只是数据，加载不改变权限」一致——权限仍由 manifest + Broker 统一裁决）。

## 后果

- 守护测试：`bm-testkit/tests/skill_wasm_reload.rs`（真 wasm 端到端执行成功 + 热重载摘旧/重注册即时生效 + 代际 max+1 不回退 + 幂等空摘除）；夹具经 `wat` crate 由 WAT 文本生成，走生产 `Module::from_binary` 路径。
- `skill.*` 从「结构不可达」转为可执行：此前任何脚本技能的调用必然失败，本 ADR 后正常执行——属缺陷修复而非能力放宽，权限/审计语义零变化。
- 未装配脚本执行面（`skills=None`）时热重载跳过，纯知识包技能保存/删除行为不变。
- 遗留：MCP 子进程无 OS 级沙箱（#55）、插件协议无 SDK（#56）维持 OPEN，不在本 ADR 范围。
