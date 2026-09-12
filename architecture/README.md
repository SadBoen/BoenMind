# BoenMind 架构工件目录

> 本目录是《BoenMind 核心架构基线》(../BoenMind-CORE-ARCHITECTURE.md)的配套工件:架构模型即代码。
> 层级:第 0 层(架构基线)的支撑物;机器可读合同在第 1 层(../boenmind-contracts/),与本目录互不依赖。
> 注:辩论转录(debates/)与外部验证报告(deepwiki-validation.md)已按 ADR-0027(2026-09-08)移出仓溯 git 史;其结论速览保留在本 README 末节与基线 §17/§24。

## 文件清单

```text
boenmind.c4                    Structurizr C4 DSL 工作区(架构模型唯一权威载体)
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
| Deployment-VPS | 部署(阶段一变体:VPS 托管,浏览器无状态访问) | §1.1(ADR-0009 修订)/§21 |
| Deployment-Phase2 | 部署(阶段二多进程) | §4.3/§21 |

## 验证与渲染

```text
验证:structurizr-dsl 4.1.0 解析通过(85 元素 / 128 关系 / 12 视图,2026-08-29 ADR-0009 起)。
     本仓库验证方式:Maven 引入 com.structurizr:structurizr-dsl,
     StructurizrDslParser.parse(new File("boenmind.c4")) 无异常即通过。
渲染:任意 Structurizr 兼容渲染器(structurizr.com、on-premises、Ilograph 导出等)
     导入 boenmind.c4 即可出图;DSL 本身不依赖任何渲染器。
纪律:拓扑变更先改本模型,再改正文(基线 §15/§24、ADR-0008);
     视图与正文不一致以模型为准。
```

## 结论索引

五条核心裁决的复核结论与现行状态见 `../adr/README.md` 与基线 §17.1；外部对照验证结论见基线 §24，S1-S10 修订建议的裁决状态以 GitHub Issues 台账为准（本 README 不维护）。
