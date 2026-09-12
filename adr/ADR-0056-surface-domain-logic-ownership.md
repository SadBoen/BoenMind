---
status: accepted
date: 2026-09-13
summary: surface 领域逻辑归属裁定——config_store/rebuild_routes 等保留 surface 内(耦合用户面向的磁盘配置形状);system prompt 结构化组成上移(已实施)
supersedes: []
superseded_by: []
---
# ADR-0056: surface 领域逻辑归属与 system prompt 结构化组成

- 关联: ADR-0044(surface 服务层边界核实)、ADR-0042(模型路由端口上移)、ADR-0046(依赖反转)、ADR-0005 §6(改合同双重门槛)、decisions.md 条目 11(磁盘形状是用户面向的稳定形状)
- 背景: ADR-0044 核实 surface **依赖面**已正确反转(无具体 provider 类型,编译期证明),但 surface 仍「依赖面薄、逻辑面厚」,自持若干领域策略(config_store 合并语义、rebuild_routes 路由编排、providers CRUD、mcp 生命周期)。这些逻辑本可上移内核端口,但需评估是否属过度设计。

## 决策

1. **`config_store` / `rebuild_routes` / providers CRUD / MCP 生命周期保留 surface 内**。
   理由与 decisions.md 条目 11 同源:这些编排耦合**用户面向的磁盘配置形状**(providers.json/model.json/mcp.json——ADR-0049/0051 已裁磁盘形状不合一、是面向用户的稳定形状而非冻结合同)。内核端口(`ModelRouter`/`SecretStore`/`McpAdmin`)已提供原语;把「读某磁盘形状 → 播种密钥 → 建表」编排塞进内核,等于把用户配置形状的知识反向渗入内核接口,违反「合同面向能力/bm-contract 放数据」的分层。阶段一单界面下「多界面复用」无真实消费者,上移属 ADR-0005 §6 双门槛不通过的过度设计。

2. **system prompt 结构化组成上移内核(本 ADR 实施项)**。
   诊断面(上下文透视)原由前端**正则反解析** prompt 文本标记(`[附加技能 · name]`/`[工作目录]`)重建 persona/技能/工作区——脆弱耦合。改为:
   - `bm-core::roles` 新增 `RolePreamble`(+`SkillPreamble`),`compose_preamble` 产出结构化组成,`render` 渲染为 prompt 文本,`compose_role_prompt` 退化为其薄封装(逐字等价)。
   - 模型调用快照 `ContextRecord.system_parts`(`{persona, skills[], workspace}`)随 `context-log.jsonl` 落盘。
   - 前端 `recipe.ts` 直读 `system_parts`;仅在旧快照缺该字段时回退文本标记解析(逐步淘汰)。

## 后果

- system_parts 入快照:前端上下文透视不再反解析 prompt;后端改 prompt 组装形状前端不受影响。端到端测试断言技能来自 `system_parts`。
- config_store/rebuild_routes 等**零变更**:归属裁定为 surface,surface 覆盖测试与依赖边界不变。
- 内核新增 `RolePreamble` 公开类型与 `compose_preamble` 入口;`compose_role_prompt` 行为逐字保持(既有调用方无感)。
- 本 ADR 关闭「surface 逻辑归属」待决项;不再重复提议上移(无新证据前)。
