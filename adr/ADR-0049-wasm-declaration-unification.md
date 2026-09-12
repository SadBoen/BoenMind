---
status: accepted
date: 2026-09-12
summary: wasm 声明格式合一——归一化 WasmDecl + 单一 synthesize,skills.json 与 plugins.json 两种磁盘形状共用一条 manifest 合成路径
supersedes: []
superseded_by: []
---

# ADR-0049: wasm 声明格式合一(单一 manifest 合成路径)

- 关联: ADR-0041(通用 wasm 宿主与去特化)、ADR-0046(反转补完,本为其 P4)、ADR-0016(Skill 脚本执行面)
- 背景: 同一个 wasm 宿主承载**两种磁盘声明形状**:`skills.json`(`{skills:[{scripts:[…]}]}` + `SkillDefinition`)与 `plugins.json`(顶层数组 + `WASM_PLUGIN_SCHEMA`)。两者的**装载**已共用 `register_wasm`,但 **manifest 合成**仍是两个独立 `json!` 块(`manifests_for` 与 `load_plugins_file`),默认值/字段集并行维护——典型"同一语义两处写"。

## 决策

**引入归一化结构 `WasmDecl` + 单一合成方法 `synthesize`**(ADR-0049):
- 两种磁盘形状各作**薄适配器**,把字段映射为 `WasmDecl`(capability/provider/version/输入出参 schema/effect/idempotent/timeout/approval/scopes/description);
- manifest 合成**只剩一条代码路径**(`WasmDecl::synthesize`,`execution_mode` 恒 async、`cancellable` 恒 true);
- 语义差异**显式化**:`description` 仅在有值时出现(插件行为),技能路径不产出该字段——此前散在两个 `json!` 里的隐式差异,现由结构体字段 + 单点 `if let` 表达。

## 后果

- **manifest 合成单源**:`grep execution_mode/cancellable` 在 `skill_wasm.rs` 只剩 `synthesize` 一处;两个装载路径不再各自维护字段表。
- **零行为变更**:501 测试全绿;`clippy -D warnings` 零警告;`skill_wasm` 10 项测试(含 schema 门正反例)全过。
- **真进程实证**:同一进程同时装载 `skills.json`(技能 `sk_demo`)与 `plugins.json`(插件 `demo.p4`),两格式均装载成功、能力面均在场(`skill.sk_demo.echo` / `demo.p4`,身份均 `tool`)——经同一 `synthesize`。
- **未做**:未合并两种**磁盘格式**(`skills.json` 的 `{skills:[…]}` vs `plugins.json` 顶层数组)。理由:`skill.v0_1` 是**冻结的 Minor-add-only 合同**,改其承载形状属破坏性变更,收益不抵风险;归一化止于**内部语义层**(`WasmDecl`),磁盘格式作为面向用户的稳定契约各自保留。
