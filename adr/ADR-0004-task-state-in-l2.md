# ADR-0004 Task 规范状态归 L2,任务板仅为投影

- 状态: accepted-with-conditions
- 日期: 2026-08-28
- 决策类型: 架构裁决(对基线 §17 裁决 R4 的复核结论)
- 来源: Zen consensus 多模型辩论——三个模型家族分任三方并跨裁决轮换角色(FOR=glm-5-turbo, AGAINST=gpt-5.6-luna, EMPIRICAL=gemini-3.7-flash);两轮(独立立场→交叉质证)+逐裁决合成
- 辩论记录: `architecture/debates/R4-TaskL2-transcript.md`
- 辩论数据: 共识 9 项 / 未决分歧 4 项 / 条件 8 项 / 决策要点 8 项;合成结论 **amend**

## 背景(原裁决文本)

> Task 的规范状态、生命周期、成员关系、预算与截止时间归 L2 唯一持有;Orchestrator 的 Task Board 是 Projection,可由事件日志重建,不能成为 Task 存在与否的唯一依据;Orchestrator 崩溃时 Task 继续由 Runtime 监督。

## 裁决(决策要点)

1. Task、Session、Operation、Approval、Artifact 的规范状态、生命周期、成员关系、预算与截止时间唯一由 L2 持久层持有；任何进程内存中的任务板与任何 Surface 都不是 Task 存在性依据，删除以 L2 墓碑为准。
2. 编排决策（命令意图、成员变更、预算扣减）以不可变决策事件持久写入 L2 单写者事件日志，属持久合同；『可重建投影』仅适用于 UI 视图（任务板、卡片布局、排序偏好）与 Orchestrator 策略私有参数，二者永不作为 Task 存在性依据。
3. 投影重建只绑定全局单调 event_seq 的持久单写者日志；事件日志实行周期性压实与快照（Kafka log compaction 式），长生命周期 Task 的重建不得依赖日志保留期配置。
4. L2 对 Task 写入实施 epoch fencing：task_epoch 递增、CAS 条件写入与命令租约，携带过期 epoch 的迟到命令一律判定 Stale 拒绝并留下可审计事件，防止跨 Surface 接管与热替换期间的旧编排器脑裂。
5. 外部副作用命令必须携带稳定的 Task-step-attempt 幂等键；恢复流程先结算未决 Operation（§9.5 outcome_unknown 只允许查询、认领或显式补偿，禁止普通重试），确认结果后才允许生成新决策。
6. Orchestrator 崩溃期间 Runtime 仅维持 Agent 会话、租约与心跳监督，不推断编排下一步；恢复后由 Orchestrator 从最近一致的持久状态与决策事件重新推理，系统不承诺 LLM 决策过程的重放复原。
7. 高频遥测、心跳与进度事件走独立写路径，不写入 Task 规范记录，避免 SQLite 写放大与冷启动回放尾延迟。
8. M2 里程碑纳入四项混沌验收：杀 Orchestrator 后 CLI attach 同一 Task 可继续观察与操作；损坏本地任务板库无行为差异；同一 event_seq 前缀两次重建结果确定一致；旧 epoch 命令在 epoch 推进后必被拒绝——未通过即视为 R4 未实现。

## 对基线的修订(自并入起生效;正文未逐条改写处,以本节文本为准)

- 改写第 141 行及裁决原文的并列表述为三层归属：(a) Task/Session/Operation/Approval/Artifact 的规范状态与生命周期——L2 唯一持有；(b) 编排决策（命令意图、成员变更、预算扣减）——以不可变决策/意图事件持久写入 L2 事件日志，属持久合同；(c) UI 视图（Task Board、卡片布局、排序偏好）与 Orchestrator 策略私有参数（prompt 模板选择等）——Projection/私有状态，可随时丢弃重建，永不作为 Task 存在性依据。
- §14.2 增补接管协议：取得接管权时由 L2 递增 task_epoch；所有编排命令携带 epoch 并经 CAS 校验与租约门禁；过期 epoch 命令返回可审计的 stale-command 结果。
- 改写第 776 行为『投影自持久事件日志与周期性快照重建；事件日志实行 Kafka log compaction 式压实，快照/压实为强制义务，不依赖保留期配置』，并相应废止第 1316 行『仅升级前创建快照』的单一时点要求。
- §9.5 增补：外部副作用命令必须携带稳定的 Task-step-attempt 幂等键；恢复时对 outcome_unknown 的 Operation 先查询/认领/补偿，禁止依据投影推导直接重发命令。
- 新增恢复语义条款：Orchestrator 恢复 = 先结算未决意图，再从最近一致的持久状态与决策事件重新推理下一步；明确声明不重放 LLM 内部推理过程；Runtime 仅承担会话监督，并定义编排重启的触发者与停滞窗口上限。
- M2（第 1590 行）验收清单增补四项混沌测试（杀 Orchestrator 后 CLI attach、损坏本地任务板库、同 event_seq 前缀重建确定性校验、旧 epoch 命令拒绝）作为 R4 的可证伪验收前置。

## 条件与验收

- 投影/视图重建只允许绑定 L2 持久单写者事件日志（全局单调 event_seq）；任何 best-effort 事件流（如 k8s Events 式短期事件）不得作为重建依据。
- 事件日志必须具备周期性压实与快照机制，长生命周期 Task 的重建不得依赖第 776 行的保留期配置；该机制是 M2 验收的前置条件。
- L2 对 Task 实施写入门禁：task_epoch 递增 + CAS 条件写入 + 命令租约 fencing；携带过期 epoch 的命令一律判定 Stale 拒绝并留下可审计事件。
- 外部副作用命令必须携带稳定的 Task-step-attempt 幂等键；恢复流程先结算未决 Operation（§9.5 outcome_unknown 的查询/认领/补偿），确认结果后才允许生成新决策。
- 恢复语义边界必须显式写入基线：不承诺 LLM 决策过程重放，只承诺规范状态/存在性恢复与幂等续跑，防止实现者误读为『恢复后一切如初』。
- Runtime 在 Orchestrator 失联期间只做会话/租约/心跳监督，不得推断编排下一步；编排重启的触发者与停滞窗口上限须在基线中补充定义。
- 高频遥测、心跳与进度事件走独立写路径，不写入 Task 规范记录，避免 SQLite 写放大与冷启动回放尾延迟。
- M2 纳入四项混沌验收（杀 Orchestrator 后 CLI attach、损坏本地任务板库、同前缀重建确定性、旧 epoch 命令拒绝），任何一项未通过即视为 R4 未实现。

## 共识(经质证未被驳倒的命题)

- 合同对象存在性归属：Task、Session、Operation、Approval、Artifact 的规范状态、生命周期、成员关系、预算与截止时间由 L2 持久层唯一持有，删除以 L2 墓碑为准；Orchestrator/Butler 进程内存、本地任务板库与任何 Surface 均不构成存在性判据——进程崩溃、界面断开或跨接管不得使 Task 消失或『删了又长回来』。三方第二轮均明确接受，挑战方主动收敛立场。
- Task 存在性与 Agent 会话存活是正交维度：Orchestrator 崩溃时由 Runtime 以 supervisor 身份独立维持 Agent 会话、租约与心跳（§9.6）；且 Runtime 只承担监督，不得在缺少已持久化编排意图时自行推断下一步编排动作，否则构成隐形第二协调器。该责任边界经质证后未被任何一方驳倒。
- 投影重建的唯一合法依据是 L2 单写者、全局单调 event_seq 的持久事件日志（第 736-739 行），不得绑定 best-effort 事件流（Kubernetes Events 默认约 1 小时 TTL、官方明令不可用于状态重建的反例）；且事件日志必须配套周期性压实/快照——仅第 1316 行的升级前快照加第 776 行的可配置保留期，不足以支撑超过保留期的长生命周期 Task 重建。此为三方一致接受的 R4 硬前提。
- 编排『决策结果』与『决策过程』必须区分：spawn/cancel/成员替换/预算扣减等决策结果以事件形式持久写入日志，属持久事实；LLM 决策过程（内部推理、采样随机性、未持久化 Scratchpad、In-Flight 生成）不承诺重放复原。FOR-R2『持久化决策结果而非决策过程』与 AGAINST 的 PlanDecision 意图日志、EMPIRICAL 的 Decision Records 在此收敛；裁决原文将『编队策略』整体归入可丢弃 Projection 的写法被三方共同认定为缺陷。
- 恢复语义边界：恢复 = 先结算未决意图与未知副作用（§9.5 outcome_unknown 只允许查询/认领/显式补偿，禁止普通重试），再从最近一致的持久状态重新推理下一步；系统承诺的是规范状态与存在性可恢复加幂等续跑，不承诺编排路径的精确重入或『恢复后一切如初』。三方第二轮就此收敛。
- 跨 Surface 接管与热替换必须有 fencing：L2 对 Task 写入实施 task_epoch/generation 递增 + CAS 条件写入 + 命令租约，携带过期 epoch 的迟到命令必须判定 Stale 拒绝并产生可审计事件；全局单调 event_seq（第 739 行）使该机制在单设备单写者下实现成本低。AGAINST 提出、FOR-R2 作为实现补丁接受、EMPIRICAL-R2 定性为必补约束，机制本身无异议。
- 外部副作用命令必须携带稳定的 Task-step-attempt 幂等键：AGAINST 给出的双 spawn 故障剧本（O1 在成员提交事件落盘前崩溃，O2 依据旧成员集重发 spawn，导致重复 Agent 与预算双重消耗）两轮内未被 FOR 驳倒，成为原文必须补齐的机制。
- 高频遥测与规范状态写路径分离：心跳、进度、token 流等走独立监督/瞬态通道，不写入 Task 规范记录（同构于 Kubernetes Status 子资源独立于 Spec）；单设备单用户数十 TPS 的账本与租约写入远低于 SQLite WAL 能力上限，写放大不构成对 L2 唯一事实源的反对理由。EMPIRICAL 以量化数据驳倒 AGAINST 的写放大论点，AGAINST-R2 已收窄该批评。
- 裁决可证伪且验收须工程强制：M2（第 1590 行）必须纳入混沌验收——杀 Orchestrator 后 CLI attach 同一 Task 仍可观察与操作；损坏/删除 Orchestrator 本地任务板库无行为差异；同一 event_seq 前缀两次重建结果确定一致；epoch=N 命令在 epoch 推进到 N+1 后必被拒绝。可证伪判据三方均接受，不设验收则纪律必然退化为任务板第二事实源（k8s Events 与桌面工具的历史败因）。

## 未决分歧(如实记录,不强行抹平)

- 编排决策记录的保真度与粒度未收敛：FOR 主张只持久化『决策结果事件』即可；AGAINST-R2 要求完整意图字段（task_epoch、plan_version、policy_version、输入事件游标、模型/提示词摘要、随机种子、目标命令、attempt_id、预算扣减、前置版本），但同时承认完整保存提示词与工具上下文会显著扩大事件体积、只存摘要又留下『无法证明策略为何选择某成员或为何扣减预算』的审计盲区；EMPIRICAL 承认此权衡但未给出最低必选字段集。最低保真集直接决定事件体积、存储成本与恢复尾延迟，两轮后仍为开放问题。
- Orchestrator 失联窗口的责任空档未定义：重规划由恢复后的 Orchestrator 执行已成共识，但重启/重规划的触发者、可接受的停滞窗口上限、以及 Runtime 在无持久化编排意图时被允许的受限动作集（续租、安全暂停、补偿）均无条款。AGAINST 警告『要么任务停滞、要么 Runtime 沦为隐形第二协调器』，FOR 与 EMPIRICAL 未给出窗口与触发承诺。
- Temporal 等先例的可迁移性解释分歧未裁决：FOR-R2 认为只需移植『状态由 History 重建、决策结果即事件』这一半；AGAINST-R2 认为 Temporal 的恢复语义依赖确定性 Worker 代码与版本化 replay 约束，LLM 编排缺此二者时类比在关键处不成立。机制补救虽已进共识，该解释分歧影响『决策日志最低字段』的判断（与第一条分歧联动），本身未被裁决。
- 定量恢复预算缺失：三方同意需要压实/快照与恢复时间预算，但压实频率、快照粒度、投影重建延迟目标均无数值共识——FOR 曾主张重建路径需达毫秒级（借 informer 本地缓存预热），EMPIRICAL 指出单设备数月运行加海量细粒度事件后冷启动全量回放尾延迟是真实风险，基线亦无可验证的恢复时间目标条款。

## 后果

- 本 ADR 并入后,基线 §17 对应裁决以本文件为准;条件的验收责任落在对应里程碑(条件中已注明 M2/M3/M4/M7 等)。
- 条件全部闭合前,该裁决对外宣称口径为「有条件成立」;任一验收被证伪即触发本 ADR 复审(而非默认回退)。
