# BoenMind 架构工件目录

> 本目录是《BoenMind 核心架构基线》(../BoenMind-CORE-ARCHITECTURE.md)的配套工件:
> 架构模型即代码、多模型辩论记录、外部系统对照验证报告。
> 层级:第 0 层(架构基线)的支撑物;机器可读合同在第 1 层(../boenmind-contracts/),与本目录互不依赖。

## 文件清单

```text
boenmind.c4                    Structurizr C4 DSL 工作区(架构模型唯一权威载体)
debates/R1..R5-*-transcript.md §17 五条核心裁决的 Zen consensus 多模型辩论全程转录
debates/zen-consensus-final.md 跨裁决终局合成(三大张力 + ADR 计划)
deepwiki-validation.md         Erlang/OTP、Kubernetes、VS Code 三系统对照验证报告
```

## boenmind.c4 视图 ↔ 基线章节映射

| 视图 | 类型 | 基线章节 |
|---|---|---|
| SystemContext | 系统上下文 | §1 产品本质 |
| Container | 容器 | §15 推荐核心拓扑;标签=L0-L5(§2.1) |
| L2Components | 组件 | §6 Registry、§7 Broker、§8 Bus、§9 状态机/审批/预算 |
| BrokerCall | 动态 | §7 统一调用管线 |
| ButlerPaths | 动态 | §10.2 两条路径 |
| TaskFlow | 动态 | §22 端到端工作流 |
| ProviderHotSwap | 动态 | §13.1 |
| ProviderCrash | 动态 | §13.2 |
| GenerationUpgrade | 动态 | §13.4/§13.7 |
| Deployment-Phase1 | 部署(阶段一单进程) | §1.1/§21 |
| Deployment-Phase2 | 部署(阶段二多进程) | §4.3/§21 |

## 验证与渲染

```text
验证:structurizr-dsl 4.1.0 解析通过(66 元素 / 111 关系 / 11 视图)。
     本仓库验证方式:Maven 引入 com.structurizr:structurizr-dsl,
     StructurizrDslParser.parse(new File("boenmind.c4")) 无异常即通过。
渲染:任意 Structurizr 兼容渲染器(structurizr.com、on-premises、Ilograph 导出等)
     导入 boenmind.c4 即可出图;DSL 本身不依赖任何渲染器。
纪律:拓扑变更先改本模型,再改正文(基线 §15/§24、ADR-0008);
     视图与正文不一致以模型为准。
```

## 辩论与验证结论速览

- 五条裁决:R1 三权分立**有条件维持**(10:4);R2 Butler/Coordinator、R3 L0+generation、R4 Task 状态归 L2、R5 万物皆插件均**修订**(无一条被推翻)。新增裁决两条:权限以合同显式化(元原则)、L0 自举豁免与升级信任链。详见 `../adr/`。
- 外部验证:L0-L5 分层被三系统印证;热替换 C7/C8 确认、C1-C6 部分确认、无偏差;单写者租约与验证期禁副作用为 BoenMind 独有加强;修订建议 S1-S10 全部 proposed,待里程碑回看裁决。
