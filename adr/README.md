# BoenMind 架构决策记录(ADR)

> 层级:第 0 层(架构基线)的决策日志。基线正文只保留稳定结论;增量裁决、修订与条件全部在这里。两者冲突时以更新的 ADR 为准。
> 生成:2026-08-28,§17 五条核心裁决的 Zen consensus 多模型辩论(glm-5-turbo / gpt-5.6-luna / gemini-3.7-flash 三模型家族,角色跨裁决轮换,两轮质证+逐裁决合成)+ DeepWiki 三系统对照验证。
> 辩论全程转录:`architecture/debates/`;外部验证报告:`architecture/deepwiki-validation.md`。
>
> **协议说明(角色轮换的精确语义)**:三方 persona(架构师/挑战者/实证者)与三个模型家族绑定后跨裁决轮换,保证每个模型都体验过不同立场;位置标签(FOR/AGAINST/EMPIRICAL)在各记录中固定。标签与「persona@模型」的实际对应:R1、R4 = FOR=架构师@glm-5-turbo,AGAINST=挑战者@gpt-5.6-luna,EMPIRICAL=实证者@gemini-3.7-flash;R2、R5 = FOR=挑战者@gpt-5.6-luna,AGAINST=实证者@gemini-3.7-flash,EMPIRICAL=架构师@glm-5-turbo;R3 = FOR=实证者@gemini-3.7-flash,AGAINST=架构师@glm-5-turbo,EMPIRICAL=挑战者@gpt-5.6-luna。每场辩论中三种 persona 与三个模型家族均完整在场。

| ADR | 标题 | 状态 | 一句话决策 |
|---|---|---|---|
| [ADR-0001](ADR-0001-registry-broker-bus.md) | Registry/Broker/Bus 三权分立 | accepted-with-conditions | 三权分立维持;Broker 授权数据面快路径合法化,binding_epoch 固化 |
| [ADR-0002](ADR-0002-butler-coordinator.md) | Butler 仅持协调权,Coordinator 为受限队长 | accepted-with-conditions | 协调动词按 Task 子树裁剪+safe/mutation 二分+Grant 物化 |
| [ADR-0003](ADR-0003-l0-generation-upgrade.md) | L0 独立控制面与 Runtime generation 升级回滚 | accepted-with-conditions | 回退限定工件/指针层;probation 观察窗;完整代际流程仅限 L1 Major |
| [ADR-0004](ADR-0004-task-state-in-l2.md) | Task 规范状态归 L2,任务板仅为投影 | accepted-with-conditions | 三层归属;task_epoch fencing;事件日志压实为强制义务 |
| [ADR-0005](ADR-0005-everything-is-plugin.md) | 万物皆插件:内核只含合同与最小机制 | accepted-with-conditions | 最小机制封闭清单+安全不变量四判据+改合同双重门槛 |
| [ADR-0006](ADR-0006-permission-by-contract.md) | 权限以合同显式化(元原则) | accepted | 未列入合同的权力视为不存在 |
| [ADR-0007](ADR-0007-l0-bootstrap-exemption.md) | L0 自举豁免与升级信任链 | accepted-with-conditions | L0 是唯一显式自举豁免体,豁免清单冻结 |
| [ADR-0008](ADR-0008-architecture-as-code-validation.md) | 架构即代码与外部实证验证 | accepted | C4 DSL 为唯一权威;C1-C8 验证成立;S1-S10 proposed |
| [ADR-0009](ADR-0009-deployment-and-surface.md) | 部署形态与 Surface 策略 | accepted-with-conditions | VPS 托管+Web/交互式 TUI Surface+Windows 壳;受限解除「无远程访问」 |
| [ADR-0010](ADR-0010-third-party-model-gateway.md) | 第三方中转网关作为模型 Provider | accepted | 第三方网关=OpenAI 兼容 Provider,信任边界与脱敏口径固化 |
| [ADR-0011](ADR-0011-first-apps-as-mcp-servers.md) | 首批真实 App 以 MCP Server 形态接入 | accepted | App=进程外 MCP stdio server;零内核改动,复用 M7 隔离与审批 |
| [ADR-0012](ADR-0012-config-management-api.md) | 配置管理 API——用户可改配置一律走成套 CRUD | accepted | config.list/get/set/delete 四方法;文件>env>默认,重启生效;secret 打码 |
