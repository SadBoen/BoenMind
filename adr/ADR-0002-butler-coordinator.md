# ADR-0002 Butler 仅持协调权,Coordinator 为受限队长

- 状态: accepted-with-conditions
- 日期: 2026-08-28
- 决策类型: 架构裁决(对基线 §17 裁决 R2 的复核结论)
- 来源: Zen consensus 多模型辩论——三个模型家族分任三方并跨裁决轮换角色(AGAINST=gpt-5.6-luna, EMPIRICAL=gemini-3.7-flash, FOR=glm-5-turbo);两轮(独立立场→交叉质证)+逐裁决合成
- 辩论记录: `architecture/debates/R2-ButlerCoordinator-transcript.md`
- 辩论数据: 共识 8 项 / 未决分歧 5 项 / 条件 6 项 / 决策要点 8 项;合成结论 **amend**

## 背景(原裁决文本)

> Butler App 是真实 App 但只拥有系统协调权,不默认拥有任何领域操作权;Coordinator Agent 不复制 Butler 完整身份,其权限 = Butler 可授予的协调权 ∩ 当前 Task 授权 ∩ 用户授权;不能把成员权限无限转授、不能绕过 Broker、不能扩大 Task 预算。

## 裁决(决策要点)

1. 裁决维持:Butler 是真实 App 但仅持系统协调权,不默认拥有任何 App 的领域操作权;每个 Task 的 Coordinator Agent 不复制 Butler 身份,其权限 = Butler 可授予的协调权 ∩ 当前 Task 授权 ∩ 用户授权,默认拒绝、Task 结束即失效。
2. 协调动词按 Task 子树裁剪并二分分级:只读查询/结果收集类可默认继承;task.cancel/agent.pause/agent.stop/agent.spawn/team.create 等变更类须在 Task 授权中显式列出,且仅可作用于本 Task 子树内的成员与子任务。
3. 三方交集必须物化为 Broker 记账、按引用绑定的 Approval/Grant 载体:携带 task 作用域、audience、资源谓词、不可再转授标志、过期与撤回版本;成员角色授权由 Coordinator 在其自身上界内签发并经 Broker 审计。
4. capability.call 在协调权语境下仅是『已批准能力清单 + 风险等级 + 资源谓词』的受约束入口,不得作为泛化逃生舱;高风险动作强制 approval_required,低风险确定性能力按 task:<id> 作用域批量预授权。
5. 预算执行『包络内子分配允许、扩容禁止』二分:Agent/Task 两级账本,成员重试受 manifest 重试策略与成员级预算双重约束,Broker 为唯一执行点,包络扩容仅限用户批准。
6. 直接 Capability 与 Domain Agent 双路径共用同一鉴权管道、幂等键、脱敏与收据合同,收据记录来源标注与处理级别,保证审计可重建授权链与证据链;统一合同落地前同一能力不开放双路径。
7. 补偿作用于声明式副作用(外部收据、result_reference、undo 声明),不以读取对端原始数据为前提;untrusted 来源驱动的 reversible 及以上操作一律升级审批,禁止 Agent 依据 untrusted 内容请求扩权。
8. 承认并管理残余风险:将 LLM 置于受限控制面属『成熟机制的无先例组合』,安全主张以注入回归通过阈值与幂等抑制验收为准入条件;数据盲中继开销、审批中断与跨 Task 上下文断裂为已接受代价,由 Memory Service 接口与规划期预扫描+批量预授权缓解。

## 对基线的修订(自并入起生效;正文未逐条改写处,以本节文本为准)

- §11.2 增补:『Coordinator 的协调动词按其所属 Task 子树裁剪:task.cancel/agent.pause/agent.stop/task.collect/team.create 仅可作用于本 Task 子树内的成员与子任务,作用域绑定 §3 的「当前 Task」与「父子归因」字段;子树外目标一律默认拒绝。』
- §11.2 增补协调权二分:『协调权细分为 safe_coordination(只读查询、状态查询、结果收集,可默认继承)与 mutation_coordination(生命周期控制与团队组建:cancel/pause/stop/agent.spawn/team.create,须在 Task 授权中显式列出,不可默认继承)。』
- §11.3 增补公式落地条款:『三方交集的计算结果必须物化为经 Broker 记账的 Approval/Grant 绑定记录(作用域 task:<id>、默认拒绝、可撤销、重启可恢复);Grant 字段含 audience、action、资源谓词、delegation_depth=0(不可再转授)、过期时间、撤回版本与父授权哈希;成员角色授权的签发者定义为 Coordinator,且不得超过其自身 Grant 上界。』
- §10.1 修订 capability.call 条目:『capability.call 作为协调权行使时,仅能引用 manifest 已批准的能力清单、资源谓词与风险等级,不得作为泛化逃生舱;来源含 untrusted 且风险等级 reversible 及以上时强制 approval_required;低风险确定性能力可按 task:<id> 作用域批量预授权。』
- §9.7/§11.1 增补预算二分明文:『Coordinator 可在 Task 预算包络内向成员子分配预算(逐笔记账、仅在包络内分配),不可扩容;包络扩容仅限用户批准(§9.7 第900行不变);成员重试受 manifest retry 策略与成员级预算双重约束,Broker 为唯一执行点。』
- §17/R2 措辞修订:『删除「Coordinator 为确定性有界状态机」类表述,改为:Coordinator 是含受限判断行为(创建成员、重试或替换成员)的非确定性控制面实体,其安全由交集、预算、默认拒绝审批与子树裁剪等补偿控制围堵而非消除。』

## 条件与验收

- 委托载体规格必须在阶段一实现前于 §4/§5 闭环:Grant 至少携带 audience、action、资源谓词、过期时间、撤回版本、不可再转授标志与父授权哈希,由领域 Provider 在调用点验证并强制幂等键;闭环前不得对外宣称 R2 为『可执行的安全机制』。
- 协调动词子树过滤与 safe/mutation 分级必须写入 §11.2/§11.3 并在 Broker 调用点(§7 第629行)强制执行;未落地前,子树外的 task.cancel/agent.stop/team.create 按未授权能力默认拒绝。
- 注入回归用例必须定义量化通过阈值(如 untrusted 驱动的 reversible 及以上操作 100% 升级审批、越权扩权请求 100% 默认拒绝)并作为 CI 门槛;无阈值不得主张 R2 安全性已验证。
- 双路径必须共享同一 Grant、幂等、脱敏与收据合同,收据记录来源标注与处理级别;统一合同落地前,禁止对同一能力同时开放直接 Capability 与 Domain Agent 两条路径。
- 跨 Task 上下文传递接口(Memory Service)必须在阶段一规划;持续性任务(如『帮我管理一周邮件』)不得以 Coordinator 全量重建为默认方案。
- 采纳 AGAINST 提出且未被驳回的可证伪条件作为验收标准:对同一授权的两次等价请求,系统必须在审计日志中证明第二次被幂等抑制;无法证明即视为 R2 实现不完整。

## 共识(经质证未被驳倒的命题)

- 控制面/数据面正交分离成立:Butler 与 Coordinator Agent 仅持系统协调权(§10.1 第930-941行)、默认不持有任何领域操作权(第944-952行),领域数据访问一律经 Broker 路由至领域 Agent/Provider;AGAINST 第二轮明确让步『接受控制面与领域面分离及最小权限边界』,两轮后三方一致。
- 三方交集公式(Butler 可授协调权 ∩ 当前 Task 授权 ∩ 用户授权,§11.3 第1053-1058行)作为权限上界语义成立,且经 §9.6 Approval 持久合同对象(作用域 once/task:<id>/count:<n>/ttl/forever、超时即 denied、可撤销、重启后恢复)落地为按引用绑定的委托载体:凭证驻留 L2 状态机而非随调用方漂移,单设备本地场景下不引入可窃取的 bearer token。
- 四条硬边界经质证未被驳倒:不得绕过 Broker(§7 第621-648行统一入口、§3 第243行禁 Agent 间私有通道)、不得无限转授成员权限(§11.2 第1044行)、不得扩大 Task 预算(§9.7 第900行)、Agent 不是权限来源(§3 第231行);所有调用落统一审计(§7 第635行),调用者身份/时间戳/结果跨双路径统一归因。
- 『数据盲协调面不阻塞补偿』命题驳倒了 AGAINST 的『黑盒链路不可靠』论点:补偿作用于声明式副作用(manifest undo 声明第471-473行、verification 钩子第465-470行、外部收据写入 result_reference),与 Saga / kubectl rollout undo 同构;『读原始证据的核对权』被 R2 正确拒绝,『执行补偿的操作权』经 Broker 在 Task 授权边界内行使。
- 重试风暴按成员有界:manifest 重试策略(第456行)由 Broker 统一执行,且仅 read-only 与 low-risk-command 允许自动重试、reversible 及以上必须依赖幂等键或恢复流程(第474-476行),叠加 Agent/Task 两级预算账本与三层强制点(§9.7 第889-895行);两级共享限额 noisy-neighbor 问题的先例解法是层级细化(systemd per-service MemoryMax、K8s Quota+LimitRange),而非放宽边界。
- 信任分级来源治理阻断注入扩权路径:§4.5 三级标注(trusted/agent-derived/untrusted)下,untrusted 内容驱动的回合中 reversible-command 及以上调用一律升级审批(第363-364行),『Agent 不得依据 untrusted 内容请求扩权或自我授权新能力』(第367行),跨域传递先脱敏并保留来源标注(第368行)。
- 定性共识:R2 属『成熟机制的无先例组合』——控制面/数据面正交、最小特权派生、层次配额、统一 Broker 强制均有 Erlang/OTP、Kubernetes、Chromium、cgroups/systemd 先例,但把非确定性 LLM 放入受限控制面无工程先例,交集/预算/默认拒绝审批是必要补偿控制而非多余负担;该定性未被任何一方驳倒。
- 已接受代价三方共同确认:数据盲中继的 Token 开销与语义折损(K8s 控制器可读全量声明式状态,BoenMind Coordinator 只能依赖结构化汇总)、动态提权的首次审批人机往返(用户离线时停在 waiting_approval)、跨 Task 上下文断裂(须由 Butler 层/Memory Service 承接);缓解手段为规划期预扫描 + task:<id> 作用域批量预授权(§9.6 第868行)与 Memory Service 接口,而非放松权限边界。

## 未决分歧(如实记录,不强行抹平)

- 委托凭证规格的归属层级:AGAINST 要求 Broker 签发、Provider 端验证的不可伪造 Task Capability Grant(字段含 task_id、audience、action、资源谓词、参数约束、风险等级、预算预留、过期、撤回版本、父授权哈希);FOR 主张这是 §4/§5 的实现规格(HOW),R2 只需向下推导约束;EMPIRICAL 核实 §9.6 Approval 已构成按引用绑定的载体、但承认缺『不可再转授』深度标志且 §11.3 成员角色授权签发者未定义(第1060-1067行)。两轮后分歧收窄为『裁决文本内嵌凭证规格 vs 下沉实现规格』,对『何时算闭环』标准不同,未合流。
- 预算竞争的缓解深度:AGAINST 的共享包络竞争场景(成员重试耗尽 Task 预算→合法后续步骤被暂停;发送已达 Provider 但回执丢失时恢复期无法判断是否重复发送)被 EMPIRICAL 承认属实,但 AGAINST 要求按步骤『预留+补偿』机制,EMPIRICAL/FOR 的修正止步于『包络内子分配、扩容禁止』明文化(§11.1 第1035行与 §9.7 第900行的二分钩子);预留(reservation)机制是否进入 R2 未决。
- 双路径结果语义是否规格化:FOR 主张直接 Capability 仅限无状态简单查询、跨域编排必须走 Domain Agent(此为对 §10.2『确定性操作→Capability、复杂推理→Domain Agent』的延伸约束),并以统一 Broker 管道与审计收据驳『审计分裂』;AGAINST 指出同一 mail.search 直连路径返回原始结构化结果、Agent 路径可能只返回摘要,数据版本、脱敏边界与幂等语义不同,要求统一的数据最小化/结果摘要/授权链合同;EMPIRICAL 仅确认审计统一有合同层保证(§7 第629/635行、§4.5 第368行),未裁决语义差异是否需要规格化。
- 注入安全主张的可证伪门槛:经核实基线第371行仅要求『注入回归用例存在』,未定义通过阈值;EMPIRICAL 据此判定 R2 安全性主张现阶段不可证伪,两轮内没有任何一方给出量化阈值方案(如升级率/默认拒绝率指标),悬置。
- 阶段一工程收益定级:FOR 的『Butler/Coordinator 降维为确定性有界状态机、直接降低交付风险』被 EMPIRICAL 驳倒——§11.1(第1029-1036行)明列『创建成员、重试或替换成员』等判断行为,Coordinator 非确定性状态机;其证据卫生亦被纠正(WAL/Snapshot 不在 §11,重建论点真实出处是 §10.3 第1005行与 §17 第1549行『Task 规范状态归 L2、任务板为可重建投影』,精确先例是 K8s informer 重建缓存而非 Erlang let-it-crash)。FOR 的『规划层吸收授权配置复杂度(iOS 式审批弹窗)』论点未被直接质证,真实负载下的用户审批频次成本缺乏实证,收益主张部分悬置。

## 后果

- 本 ADR 并入后,基线 §17 对应裁决以本文件为准;条件的验收责任落在对应里程碑(条件中已注明 M2/M3/M4/M7 等)。
- 条件全部闭合前,该裁决对外宣称口径为「有条件成立」;任一验收被证伪即触发本 ADR 复审(而非默认回退)。
