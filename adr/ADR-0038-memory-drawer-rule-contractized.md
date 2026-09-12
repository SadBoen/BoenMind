---
status: accepted
date: 2026-09-11
summary: manifest 增 authorization 声明(主体系留+读放宽),Broker 步 4.5 变解释器删硬编码规则;memory 仍不生产可达(2026-09-11)
supersedes: []
superseded_by: []
---

# ADR-0038: memory 抽屉授权规则合同化

- 状态: Accepted（2026-09-11，issue #36 / 审计台账 F-11 裁决）
- 日期: 2026-09-11
- 关联: ADR-0006（权限以合同显式化，元原则）、ADR-0020（内置能力冻结；条款 3 四判据）、ADR-0031（批次 2 成员身份面，与本条同族）、issue #36

## 背景

`Broker::decide` 步 4.5 的 `memory_drawer_verdict`（`broker/mod.rs`）以硬编码 Rust 逻辑定义记忆抽屉权限规则：agent 主体对自己的抽屉常量放行（`agent:<id>`↔`memory:agent:<id>`；`coord:`/`worker:` 前缀↔`memory:task:<id>`），`memory.search` 对 `memory:user` 放宽，越界升级审批。代码注释自述与 ADR-0006「权力以合同显式化」存在张力（F-11，2026-08-30）。ADR-0031 曾把本项排在批次 2（成员身份面冻结 principal 段格式之后再动裁决步）。

现状另有决定性事实（#59 残余-4，commit `ff30e1c`）：`memory.*` 三能力**生产组合根零调用**——`memory_capabilities` 已迁 `bm-testkit/src/memory_fixtures.rs`，全仓仅测试引用。即：步 4.5 在**生产不可达**。故本项不是「线上权限规则治理」，而是「给测试夹具立合同、为将来 memory 进生产铺路」。

## 决策

1. **规则上移到 manifest，Broker 变成解释器而非规则本体**。新增可选合同字段 `authorization`（Minor，只增不破），形状承载「主体系留（principal 前缀 → 抽屉标签模板）+ 读放宽（capability 对某抽屉放行）」。`Broker::decide` 步 4.5 改为**读取 manifest 声明**执行，删除硬编码的 capability 名与 principal 前缀分支。
2. **主体系留与读放宽以声明表达，缺省行为不变**：未声明 `authorization` 的 manifest → 步 4.5 直接跳过（与当前非 memory 能力同路径）；memory/manifest 声明后行为与现状逐条等价（含 `coord:`/`worker:` → task 抽屉、search 对 user 抽屉放宽、越界升级审批）。
3. **能力名集合不再硬编码**。此前 `memory.write`/`memory.search` 字面量判断改为「凡声明了 `authorization` 的能力即走抽屉裁决」，`memory.delete` 维持不适用（args 无 scope，条目所有权面留档演进）。
4. **生产可达性不变**：memory 仍未进生产组合根，须过 ADR-0020 条款 3 四判据方可迁回（`memory_fixtures.rs` 头注既定）；本条只使规则在合同内有声明，不改变部署面。
5. **与批次 2 的边界**：principal 段格式（成员段）若在批次 2 变更，改的是声明里前缀模板的取值（数据），不再是改 Rust 分支（代码）——这正是合同化的收益。

## 后果

- ADR-0006 张力消除：抽屉规则成为可校验的合同声明，`Broker` 不含角色/命名空间政策常量。
- 守护测试迁移：`broker/tests/m7.rs` 的抽屉用例（agent 自抽屉放行、user 抽屉写升级、跨 agent 升级、task 成员按 task 划界、search 放宽、显式 Grant 谓词优先、surface:user 不变）改为对**声明化规则**断言，行为逐条不变；`bm-testkit/tests/memory_drawer.rs` 端到端语义不变。
- manifest 合同新增可选字段；消费方忽略未知字段（既有语义），旧 manifest 不受影响。
- 若 memory 未来进生产，规则已在合同内，无需再动 Broker 裁决步。
- `decisions.md` 增条：per-capability 授权规则以 manifest 声明为真源，Broker 只做解释。
