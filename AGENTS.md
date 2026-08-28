# BoenMind — AI 工作规程(新会话必读)

## 这是什么

BoenMind:个人生态的 AI Runtime / AI OS,当前为**阶段一(跨平台单软件)**。
设计已定稿,经三模型辩论复核(五裁决修订后成立 + 两条新裁决,见 `adr/`)与三真实系统
对照验证(Erlang/OTP、Kubernetes、VS Code,见 `architecture/deepwiki-validation.md`)。
合同库已冻结 v1.0(字段只增不破)。

**当前进度:M0 完成(2026-08-28)。下一步 = M1:最小 Runtime 与单 Agent 闭环——
先写《M1 实现规格》(技术栈、仓库结构、CI),再写代码。规格属于第 2 层,开工时写,不预写。**

## 文件地图(规格分层,基线 §0)

```text
BoenMind-CORE-ARCHITECTURE.md   第 0 层  架构基线:原则/边界/不变量;§17 七条核心裁决;§18 里程碑;§19 回看制度
adr/                            第 0 层  架构决策记录(ADR-0001..0008;基线正文与 ADR 冲突时,以更新的 ADR 为准)
architecture/                   第 0 层  C4 模型 boenmind.c4(拓扑唯一权威)+ 辩论转录(debates/)+ 验证报告
boenmind-contracts/             第 1 层  机器可读合同(v1.0 冻结)+ validate.py 校验器 + m0/ 测试基准
milestones/                     第 2 层  里程碑实现规格与回看记录(M1 起建)
runtime/                        第 3 层  源代码(M1 起,Rust workspace;crate 划分在 M1 规格中定稿)
```

## 新会话工作流

1. 读本文件 → 2. 按手头任务读对应层文件 → 3. 动手前对照下方进度确认当前里程碑 →
4. 产出后自检(合同有变更必跑 `python3 boenmind-contracts/scripts/validate.py`,须全绿)。

## 硬纪律(违反 = 返工)

1. **合同冻结**:boenmind-contracts/ 字段只增不破;删字段/改名/改语义 = Major,走基线 §13.5。
2. **先改模型再改文字**:架构变更先改 `architecture/boenmind.c4`;文字图与模型不一致以模型为准。
3. **决策写 ADR**:新决策在 adr/ 发新文件(编号递增),不修改既有 ADR 的语义。
4. **权限以合同显式化**(ADR-0006):未列入注册合同的权力视为不存在。
5. **里程碑 = 可运行检查点**(§18):P0 测试套件全绿才算完成;完成后按 §19 回看再进下一个。
6. **真实进度只认 git**:主干应始终可校验(validate.py 全绿);提交说明写清动机。

## 进度

- [x] M0 范围/合同/测试基线(2026-08-28,tag `m0.2-contracts-frozen`)
- [ ] **M1 最小 Runtime 与单 Agent 闭环 ← 当前**
- [ ] M2 持久化/事件日志/崩溃恢复(开工前结算 ADR-0003/0004 相关验收条件)
- [ ] M4 Capability/Broker/权限审批(开工前结算 ADR-0001/0002 相关验收条件)
- [ ] M3/M5-M8 见基线 §18;33 项验收条件的分布见各 ADR「条件与验收」
- 注意:`architecture/deepwiki-validation.md` 的 S1-S10 修订建议为 **proposed**,
  仅在各里程碑回看时逐条裁决,勿自动采纳。
