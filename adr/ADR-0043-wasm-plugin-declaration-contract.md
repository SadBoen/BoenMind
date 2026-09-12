---
status: accepted
date: 2026-09-12
summary: 通用 wasm 插件声明合同化(plugins.json 的临时形状冻结为 wasm-plugin.v0_1 并纳入装载期 schema 门);②「WIT/Component 接口」核实为暂不需要,其真实内容是合同化
supersedes: []
superseded_by: []
---

# ADR-0043: 通用 wasm 插件声明合同化(② 的真实内容)

- 关联: ADR-0041(插件身份契约与通用宿主)、ADR-0042(核实轮方法论)、ADR-0016(WASM 脚本执行面)、ADR-0034(插件协议 SDK)
- 背景: 路线图条目 ② 原表述为「wasm 插件接口从 WASI 命令式升级为 WIT/Component」。按 ADR-0042 的方法先核实后动手:**真实缺口不是接口形态,而是协议未合同化**——`config/plugins.json` 的形状(由本轮 ADR-0041 实现临时引入)**既无 schema 也无装载期校验**,与本仓对 `mcp.json`(有 `mcp-server.v0_1.schema.json` + 装载期校验)的既定纪律不符(同 ADR-0032 的"注册期零校验"教训)。

## 决策

1. **新增冻结合同 `boenmind-contracts/plugin/wasm-plugin.v0_1.schema.json`**,冻结通用 wasm 插件声明形状:数组,每项 `{capability, provider?, version?, wasm, effect?, approval?, idempotent?, timeout_ms?, input_schema?, output_schema?, scopes?, description?}`。`capability`/`wasm` 必填;`additionalProperties: false`;能力名/风险级/scope 标签与 `capability/manifest.v0_1` 同口径复用定义。
2. **内嵌入 `bm-contract`**(`registries::WASM_PLUGIN_SCHEMA`),由 `boenmind-contracts/scripts/validate.py` 自动纳入 R1/R1b(R1b schema 自检:该合同过 draft-07 子集 lint)。
3. **装载期过冻结 schema 门**:`SkillScriptManager::load_plugins_file` 在解析后先校验整份声明,违反即**整体拒绝装载并告警**(不半装)。此前仅按"字段是否存在"跳过,形状错误可静默漏过。
4. **不做 WIT/Component 升级(② 其余部分)**:核实结论——现行 WASI 命令式(stdin 进 JSON / stdout 出 JSON)足以支撑"能力 = 入参/出参 JSON"的语义,与 Broker 的 `input_schema`/`output_schema` 校验面天然对齐;升级到 Component 需改插件作者 ABI、引入 WIT 工具链,收益(类型化)在本系统的 JSON 边界上并不显著,**属可选优化而非欠账**。留待出现"需要结构化流式/复杂类型"的真实需求时另立 ADR。

## 后果

- 通用插件声明与 MCP/provider 声明同级别:**机器可校验的冻结合同**,不再是"仅存在于实现的临时形状"。
- 守护测试:`bm-providers::skill_wasm::load_plugins_file_rejects_schema_violations`(未知字段、非法能力名被拒;**合法声明仍通过**,防"全拒")。
- `boenmind-contracts/scripts/validate.py` 全绿(schema 22 → 23 份,文件 26 → 27 个)。
- **零行为变更**:501 测试全绿;既有声明本就符合合同(测试通过即证)。
- 合同只增不破:后续插件声明新增可选字段按 Minor 增发。
- 路线图 ② 状态更新为「**已核实:真实内容是合同化,已完成**;WIT/Component 升级为可选优化,非欠账」。
