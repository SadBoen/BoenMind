---
status: accepted
date: 2026-08-28
summary: 协调动词按 Task 子树裁剪+safe/mutation 二分+Grant 物化
supersedes: []
superseded_by: []
---

# ADR-0002 Butler 仅持协调权,Coordinator 为受限队长 
- 状态: accepted-with-conditions - 日期: - 决策类型: 架构裁决(对基线 §17 裁决 R2 的复核结论) 
## 背景(原裁决文本) 
> Butler App 是真实 App 但只拥有系统协调权,不默认拥有任何领域操作权;Coordinator Agent 不复制 Butler 完整身份,其权限 = Butler 可授予的协调权 ∩ 当前 Task 授权 ∩ 用户授权;不能把成员权限无限转授、不能绕过 Broker、不能扩大 Task 预算。 
## 裁决(决策要点) 
1. 裁决维持:Butler 是真实 App 但仅持系统协调权,不默认拥有任何 App 的领域操作权;每个 Task 的 Coordinator Agent 不复制 Butler 身份,其权限 = Butler 可授予的协调权 ∩ 当前 Task 授权 ∩ 用户授权,默认拒绝、Task 结束即失效。 2. 协调动词按 Task 子树裁剪并二分分级:只读查询/结果收集类可默认继承;task.cancel/agent.pause/agent.stop/agent.spawn/team.create 等变更类须在 Task 授权中显式列出,且仅可作用于本 Task 子树内的成员与子任务。 3. 三方交集必须物化为 Broker 记账、按引用绑定的 Approval/Grant 载体:携带 task 作用域、audience、资源谓词、不可再转授标志、过期与撤回版本;成员角色授权由 Coordinator 在其自身上界内签发并经 Broker 审计。 4. capability.call 在协调权语境下仅是『已批准能力清单 + 风险等级 + 资源谓词』的受约束入口,不得作为泛化逃生舱;高风险动作强制 approval_required,低风险确定性能力按 task:<id> 作用域批量预授权。 5. 预算执行『包络内子分配允许、扩容禁止』二分:Agent/Task 两级账本,成员重试受 manifest 重试策略与成员级预算双重约束,Broker 为唯一执行点,包络扩容仅限用户批准。 6. 直接 Capability 与 Domain Agent 双路径共用同一鉴权管道、幂等键、脱敏与收据合同,收据记录来源标注与处理级别,保证审计可重建授权链与证据链;统一合同落地前同一能力不开放双路径。 7. 补偿作用于声明式副作用(外部收据、result_reference、undo 声明),不以读取对端原始数据为前提;untrusted 来源驱动的 reversible 及以上操作一律升级审批,禁止 Agent 依据 untrusted 内容请求扩权。 8. 承认并管理残余风险:将 LLM 置于受限控制面属『成熟机制的无先例组合』,安全主张以注入回归通过阈值与幂等抑制验收为准入条件;数据盲中继开销、审批中断与跨 Task 上下文断裂为已接受代价,由 Memory Service 接口与规划期预扫描+批量预授权缓解。 
## 对基线的修订(自并入起生效;正文未逐条改写处,以本节文本为准) 
- §11.2 增补:『Coordinator 的协调动词按其所属 Task 子树裁剪:task.cancel/agent.pause/agent.stop/task.collect/team.create 仅可作用于本 Task 子树内的成员与子任务,作用域绑定 §3 的「当前 Task」与「父子归因」字段;子树外目标一律默认拒绝。』 - §11.2 增补协调权二分:『协调权细分为 safe_coordination(只读查询、状态查询、结果收集,可默认继承)与 mutation_coordination(生命周期控制与团队组建:cancel/pause/stop/agent.spawn/team.create,须在 Task 授权中显式列出,不可默认继承)。』 - §11.3 增补公式落地条款:『三方交集的计算结果必须物化为经 Broker 记账的 Approval/Grant 绑定记录(作用域 task:<id>、默认拒绝、可撤销、重启可恢复);Grant 字段含 audience、action、资源谓词、delegation_depth=0(不可再转授)、过期时间、撤回版本与父授权哈希;成员角色授权的签发者定义为 Coordinator,且不得超过其自身 Grant 上界。』 - §10.1 修订 capability.call 条目:『capability.call 作为协调权行使时,仅能引用 manifest 已批准的能力清单、资源谓词与风险等级,不得作为泛化逃生舱;来源含 untrusted 且风险等级 reversible 及以上时强制 approval_required;低风险确定性能力可按 task:<id> 作用域批量预授权。』 - §9.7/§11.1 增补预算二分明文:『Coordinator 可在 Task 预算包络内向成员子分配预算(逐笔记账、仅在包络内分配),不可扩容;包络扩容仅限用户批准(§9.7 第900行不变);成员重试受 manifest retry 策略与成员级预算双重约束,Broker 为唯一执行点。』 - §17/R2 措辞修订:『删除「Coordinator 为确定性有界状态机」类表述,改为:Coordinator 是含受限判断行为(创建成员、重试或替换成员)的非确定性控制面实体,其安全由交集、预算、默认拒绝审批与子树裁剪等补偿控制围堵而非消除。』 
## 条件与验收 
- 委托载体规格必须在阶段一实现前于 §4/§5 闭环:Grant 至少携带 audience、action、资源谓词、过期时间、撤回版本、不可再转授标志与父授权哈希,由领域 Provider 在调用点验证并强制幂等键;闭环前不得对外宣称 R2 为『可执行的安全机制』。 - 协调动词子树过滤与 safe/mutation 分级必须写入 §11.2/§11.3 并在 Broker 调用点(§7 第629行)强制执行;未落地前,子树外的 task.cancel/agent.stop/team.create 按未授权能力默认拒绝。 - 注入回归用例必须定义量化通过阈值(如 untrusted 驱动的 reversible 及以上操作 100% 升级审批、越权扩权请求 100% 默认拒绝)并作为 CI 门槛;无阈值不得主张 R2 安全性已验证。 - 双路径必须共享同一 Grant、幂等、脱敏与收据合同,收据记录来源标注与处理级别;统一合同落地前,禁止对同一能力同时开放直接 Capability 与 Domain Agent 两条路径。 - 跨 Task 上下文传递接口(Memory Service)必须在阶段一规划;持续性任务(如『帮我管理一周邮件』)不得以 Coordinator 全量重建为默认方案。 - 采纳 AGAINST 提出且未被驳回的可证伪条件作为验收标准:对同一授权的两次等价请求,系统必须在审计日志中证明第二次被幂等抑制;无法证明即视为 R2 实现不完整。 
## 开放问题（未决；供里程碑回看时审视） 
- **委托凭证规格的归属层级**：凭证规格内嵌于裁决文本，还是下沉为实现规格（HOW）；「何时算闭环」标准不同，未合流。 - **预算竞争的缓解深度**：是否引入按步骤「预留＋补偿」的 reservation 机制，还是止步于「包络内子分配、扩容禁止」。 - **双路径结果语义是否规格化**：同一能力经直连 Capability 与经 Domain Agent 的返回（数据版本／脱敏边界／幂等语义）是否需统一合同。 - **注入安全主张的可证伪门槛**：基线仅要求「注入回归用例存在」，未定义量化阈值（升级率／默认拒绝率），现阶段不可证伪。 - **阶段一工程收益定级**：「规划层吸收授权复杂度」的用户审批频次成本缺实证，收益主张部分悬置。 
## 后果 
- 本 ADR 并入后,基线 §17 对应裁决以本文件为准;条件的验收责任落在对应里程碑(条件中已注明 M2/M3/M4/M7 等)。 - 条件全部闭合前,该裁决对外宣称口径为「有条件成立」;任一验收被证伪即触发本 ADR 复审(而非默认回退)。 