# BoenMind 架构决策记录(ADR)

> 层级:第 0 层(架构基线)的决策日志。基线正文只保留稳定结论;增量裁决、修订与条件全部在这里。两者冲突时以更新的 ADR 为准。
> 生成:2026-08-28,§17 五条核心裁决的 Zen consensus 多模型辩论(glm-5-turbo / gpt-5.6-luna / gemini-3.7-flash 三模型家族,角色跨裁决轮换,两轮质证+逐裁决合成)+ DeepWiki 三系统对照验证。
> 辩论全程转录与验证报告已按 ADR-0027(2026-09-08)移出仓溯 git 史;结论速览见 `../architecture/README.md`。
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
| [ADR-0010](ADR-0010-third-party-model-gateway.md) | 第三方模型网关信任边界 | accepted-with-conditions | 接受 NewAPI 型网关为真实模型通道;内容视同对网关可见,不改 input_trust 语义 |
| [ADR-0011](ADR-0011-first-apps-as-mcp-servers.md) | 首批真实 App 以 MCP Server 形态接入 | accepted | App 以进程外 MCP stdio server 接入,不新增内核「App 对象」合同 |
| — | *(ADR-0012 配置管理 API:随 M10 dsh 线未提交工作归档,编号永久跳空,存 `archive/m10-dsh-frontend` 分支)* | archived | — |
| [ADR-0013](ADR-0013-dsh-frontend-abandoned.md) | 弃用 dsh 复刻前端 | accepted | dsh 复刻 Web 前端全量删除(归档分支),换新参考界面重做 |
| [ADR-0014](ADR-0014-webui-w-series-assistant-ui.md) | W 序列 WEBUI 技术路线 | accepted | assistant-ui 组件库自建壳+OpenAI 兼容插座;W 序列独立编号 |
| [ADR-0015](ADR-0015-doc-consolidation.md) | 文档体系整理:熔入式修订与三层附页 | accepted | 基线增补熔入正文不挂引注块;HISTORY/PLAYBOOK/BACKLOG 三附页;milestones/README 导航 |
| [ADR-0016](ADR-0016-skill-v0.2-scripts-broker-pipeline.md) | Skill v0.2 脚本执行架构与 Broker 管线覆盖 | accepted | wasmtime 运行时沙箱+WASM 为主零 Python+全量纳入 Broker 七步管线 |
| [ADR-0017](ADR-0017-context-mode-rust-mcp.md) | context-mode Rust MCP 官方插件 | accepted | 外部 MCP、官方随包但默认不启用、Linux 单 exe、显式批准与 Broker 风险边界 |
| [ADR-0018](ADR-0018-workspace-registry-and-session-binding.md) | 工作区注册表与会话级工作目录绑定 | accepted | config/workspaces.json 项目注册;AgentSpec/SendInput 只增 workspace 字段;回合级 system prompt 注入;探针无 shell |
| [ADR-0019](ADR-0019-system-exec-approval-tool.md) | system.exec 内置命令执行工具(审批类) | accepted | 常规 shell 能力+每条命令审批卡;内置异步能力(.async);真沙箱仍归 ADR-0016 |
| [ADR-0020](ADR-0020-internal-capability-freeze.md) | 内置能力封闭清单与例外裁决 | accepted | 内置能力严格封闭(仅保留系统内置命令执行 system.exec 作为过渡态例外) |
| [ADR-0021](ADR-0021-fs-tools-internalization.md) | 文件工具集内置化(fs.*)与 code-tools 插件退役 | accepted | 查/读/改收编内核(修订单 ADR-0020 清单范围,用户裁决);沙箱=工作区注册表;审批分级不变 |
| [ADR-0022](ADR-0022-native-tool-protocol-and-description-governance.md) | 工具调用原生协议还原与描述治理 | accepted | role:tool+tool_call_id/tool_calls 回喂/删防复读禁令/manifest.description 自描述/fs_edit edits 数组(2026-09-06) |
| [ADR-0023](ADR-0023-bundled-plugin-default-install-and-lifecycle.md) | 官方随包插件默认安装与生命周期管控 | accepted | 启动播种默认安装(修订 0005/0006/0017 对官方件的显式批准要求)/墓碑防复活/批准即上线卸载即下线/purge 物理删除/弃用标记(2026-09-06) |
| [ADR-0024](ADR-0024-limits-config-surface.md) | 运行时限制集中配置面(limits.json) | accepted | ~40 项硬编码限制收敛单文件+安全钳制+env>文件>代码默认+LimitsCell 热生效;设置页全量可编辑;exec 默认 120s/上限 600s 对齐业界(2026-09-07) |
| [ADR-0025](ADR-0025-background-command-promotion.md) | 长命令后台转轨 | accepted | exec 增 run_in_background+超限自动转轨(Hermes 式);system.job_output 轮询收取(DSH 式);回合 prompt 注入作业摘要;不做主动推注入(2026-09-07) |
| [ADR-0026](ADR-0026-doc-discipline-normative-narrative-separation.md) | 文档纪律:规范与叙事分离 | accepted | 规范文档零编年史/HISTORY 单行制/SETTLED.md 唯一查重清单/评审必读入口收窄(2026-09-08) |
| [ADR-0027](ADR-0027-docs-minimalism-delete-after-delivery.md) | 文档极简纪律:仓库只存规范、方向与欠账 | accepted | 过程文档(规格/回看/转录/时间线)交付即删;交付全史=git;入库白名单四类(2026-09-08) |
| [ADR-0028](ADR-0028-conversation-limits-default-zero.md) | 对话链路限制默认全零(0=不限制) | accepted | 十项限制默认归零(0=不限)+0 语义显式实现+熔断只拦同命令同参 10 次(2026-09-08) |
| [ADR-0029](ADR-0029-feed-fidelity-no-kernel-coaching.md) | 回喂忠实性原则:内核只回事实,话术出内核 | accepted | 回喂只承载事实+错误原文保真(detail)+去内核指导性教练话术(2026-09-08) |
| [ADR-0030](ADR-0030-server-side-approval-adjudication.md) | 审批裁决后台化:权限模式为服务端会话状态 | accepted | 模式按对话记于服务端(默认ask)+yolo由服务端裁决放行并审计标注auto+前端降级选择器+硬拒绝/熔断不动(2026-09-09) |

> 编号说明:ADR-0012 于 2026-08-31 随 M10 dsh 前端线归档(见 ADR-0013 编号说明),编号不回收;主干引用配置管理口径时以 config_store 现状与 W2/W4 规格为准。
> 文体注:0001-0009 状态用小写 accepted(-with-conditions),0010/0011 起用 Accepted(日期)/已裁决(用户,日期)——语义等价,统一以本表为准。
