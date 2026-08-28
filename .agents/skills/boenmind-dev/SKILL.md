---
name: boenmind-dev
description: BoenMind 仓库的开发规程技能,把 AGENTS.md 硬纪律转成按任务类型的操作清单。凡在本仓库内动 boenmind-contracts 合同、发新 ADR、改架构或 C4 模型、实现里程碑代码、写测试、做里程碑收尾回看——即使没有明说,也应加载本技能,以遵守合同冻结、先改模型再改文字、决策写 ADR、里程碑=可运行检查点等硬纪律。
---

# BoenMind 开发规程

硬纪律的权威文本在根目录 `AGENTS.md`(每次会话自动加载,先读它)。本技能不重复纪律本身,
只回答:做某类任务时,走哪些步、跑哪些检查、产出放到哪。冲突时以 AGENTS.md 与第 0 层文件为准。

## 按任务读文件(规格分层)

| 任务 | 必读 |
|---|---|
| 改合同 / 写实现 | `boenmind-contracts/README.md` + 涉及的 schema/注册表/迁移表 |
| 架构变更 | `architecture/boenmind.c4`(先改)+ 相关 ADR |
| 实现里程碑 | 基线 `BoenMind-CORE-ARCHITECTURE.md` §18 对应小节 + `milestones/` 既有规格 |
| 任何新决策 | `adr/README.md`(新文件、编号递增,不改既有 ADR 语义) |

## 清单 A:改 boenmind-contracts(合同已冻结 v1.0)

1. 只增不破:新增可选字段 = Minor;**删字段 / 改名 / 改语义 = Major——停,先与用户确认**,走基线 §13.5。
2. 事件类型、错误码只新增条目,不改既有条目;envelope 的错误码枚举必须与 `registry/error-codes` 同步(CI 比对)。
3. 动完必跑:`python boenmind-contracts/scripts/validate.py`,R1–R4 必须全绿,这是提交前置。
4. 黄金轨迹与不变量引用的 schema 若受影响,同步更新轨迹文件并保持可回放。

## 清单 B:架构变更

1. 先改 `architecture/boenmind.c4`,再改基线文字;两者不一致时以模型为准。
2. 决策本身发新 ADR(`adr/ADR-000N-<slug>.md`);把结论并入基线正文时保留 ADR 增补标记。
3. `architecture/deepwiki-validation.md` 的 S1–S10 是 proposed:只在里程碑回看时逐条裁决,不得自动采纳。

## 清单 C:实现里程碑(`runtime/` 代码)

1. 开工先写《M 实现规格》进 `milestones/`(技术栈、crate 划分、CI);规格属第 2 层,开工时写,不预写。
2. 实现中:出入参必须过对应 JSON Schema;事件类型/错误码必须在注册表内;状态迁移只能沿 `core-transitions` 的边,表外迁移即 bug。
3. 不变量测试以 INV id 命名(INV-1..INV-12),与 `invariants/M1-invariants.md` 一一对应(CI 规则 R5)。
4. 通过条件 = P0 测试套件全绿 + 黄金轨迹可回放;做完走清单 D,不要跳过回看直接开下一个里程碑。

## 清单 D:里程碑收尾(基线 §19 回看门)

1. 全量测试 + `validate.py` 全绿 + 黄金轨迹回放通过。
2. 按基线 §19 过回看门;逐条裁决本里程碑相关的 ADR 验收条件与 S1–S10 中相关项。
3. 进度只认 git:更新 AGENTS.md 进度区 → 打 tag → 提交说明写清动机。

## 提交纪律

- 主干任何提交点 checkout 出来都应可校验(validate.py 全绿)。
- 提交说明写动机,不写 "update files" 一类空话。
