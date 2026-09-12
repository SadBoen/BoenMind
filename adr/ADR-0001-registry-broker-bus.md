---
status: accepted
date: 2026-08-28
summary: 三权分立维持;Broker 授权数据面快路径合法化,binding_epoch 固化
supersedes: []
superseded_by: []
---

# ADR-0001 Registry/Broker/Bus 三权分立 
- 状态: accepted-with-conditions - 日期: - 决策类型: 架构裁决(对基线 §17 裁决 R1 的复核结论) 
## 背景(原裁决文本) 
> Registry 负责「谁提供什么」(统一注册中心,按对象类型分表);Broker 负责「能不能调用以及调用谁」(所有跨域调用统一入口,没有任何特权通道);Bus 负责「发生了什么、进度如何、异步消息如何传播」。三者职责严格分离,一切调用方共用同一 Broker 入口。 
## 裁决(决策要点) 
1. 确认 Registry/Broker/Bus 三权分立为 Runtime Core 的常驻结构:Registry 持有「谁提供什么」(统一注册中心按对象类型分表,持久逻辑目录+可丢失运行时缓存两层),Broker 持有「能不能调用以及调用谁」,Bus 持有「发生了什么、进度如何、异步消息如何传播」(持久事实源+内存分发层);三者为同一进程内的协议角色分立,不要求分进程部署。 2. 所有跨域调用(Agent/前端按钮/语音/自动化规则/Timer/Butler/其他 App)必须经 Broker 统一入口;裁定「没有任何特权通道」的准确含义为:不存在未经 Broker 授权建立、或脱离审计关联的调用与数据通道;由 Broker 授权后建立、且生命周期与收据摘要回写 Broker/Bus 的受约束数据面快路径不构成特权通道,与任何已授权调用无关的独立数据传输仍一律禁止。 3. Broker 在授权决策点从 Registry 取 binding 并固化不可变 binding_epoch 与 provider_instance_id,写入调用凭证与审计记录并由 Provider 侧校验,不匹配即拒绝或重试;热替换(§13.1 draining→handshake→原子切 binding)只影响后续调用,不得改变在途调用的授权-执行-审计一致性;须对 §6.4 Provider Binding 结构与 §7 调用关联字段做相应字段增补。 4. Broker 七步管线是逻辑职责拆分而非运行时串行管线:实现必须把策略预编译为按「调用方×目标 Capability」的 O(1) capability 查表(同 seL4/Mojo 先例),禁止逐条策略求值进入热路径;M4 里程碑以 p99 压测与队头阻塞注入测试证伪,超标即回炉实现方案。 5. 高频瞬态流(模型 token 增量、UI 打字机帧、音视频、大文件传输)走 Broker 授权的数据面快路径:不逐帧通过七步管线、不落盘(§8 行736)、不占用 L2 持久单写者;持久事实只在 Broker 状态提交点产生。 6. Bus 事件严格表达已发生的事实,禁止命令语义;Bus 层实施事件 schema 机器校验并在持久化前拒绝违规事件,持久事件生产者集合限于 Broker 状态提交点;event_seq 全局单调,所有投影、消费位点与 resume cursor 仅由持久日志重建;Orchestrator 任务板只是投影,不是第二事实源。 7. 异步协作的「请执行下一步」是一次新的 Broker Capability Call,由 Orchestrator 或订阅事实的一方发起;禁止在 Bus 之外另建 durable command queue 等绕过统一入口的隐形机制,事件流不得被当作 RPC 通道使用。 8. 外部副作用的可靠性不依赖调用原子性:副作用类 Provider 的 Capability 合同强制实现收据、按 operation_id/idempotency_key 查询与幂等重放防护;Runtime Core 以事务性 outbox 绑定本地状态提交、审计写入与事实事件的发布顺序;outcome_unknown 一律先核验外部系统收据再决定重试(§13.3);统一入口承诺统一策略与统一审计,不承诺统一副作用原子性。 
## 条件与验收 
- Broker 策略必须预编译为「调用方×目标 Capability」的 O(1) capability 查表,七步管线只允许作为逻辑拆分存在;此项列为 M4 Broker 实现的验收项,证伪判据:统一入口 p99 开销、队头阻塞注入、Broker 故障半径测试,超标即回炉实现方案并重审本裁决的运行前提(被证伪的是实现而非三权分立本身)。 - 基线须新增 binding_epoch + provider_instance_id 机制(修订 §6.4 Provider Binding 结构与 §7 调用关联字段):Broker 在授权决策点固化 epoch,注入调用凭证与审计记录,Provider 侧校验不匹配即拒绝/重试;测试:在授权返回与 Provider 提交之间强制切换 binding,验证执行对象、审计对象与策略摘要三方一致;Runtime generation 变更不得改变已签发在途调用的授权与审计归属。(注:FOR-R2 称 §6.4 行604 已含 generation 字段经核为误引,基线现文本仅含 Runtime Core 代际,故此项为必做增补而非既有性质。) - Bus 层实施事件 schema 机器校验:含命令语义的事件在持久化前拒绝并告警;持久事件生产者白名单限于 Broker 状态提交点;在「生产者伪造/乱序发布」故障注入下,投影必须可重建或异常可检出,否则事件溯源前提不成立。 - 数据面分流合规四测试(采纳 AGAINST-R2 测试组,作为「授权后快路径」的准入门槛):① 无 Broker 签发授权(lease:binding_epoch+策略版本+operation_id+deadline)的通道建立必须被拒绝;② epoch/generation 变更不得影响已授权通道的审计归属;③ 通道吞吐不得被 L2 持久单写者 p99 牵制;④ 崩溃注入下外部提交与收据摘要可最终对账,且通道字节数/阶段/收据摘要/错误全程回写 Broker/Bus。 - 副作用类 Provider 合同强制:实现收据、按 operation_id/idempotency_key 查询与幂等重放防护;Runtime Core 以事务性 outbox 绑定「本地状态提交→审计写入→事实事件追加」的顺序;可证伪指标:「外部提交成功而审计/事实缺失」比例与恢复后重复副作用比例须设上限并纳入回归测试;通用 Broker 禁止以盲目重试猜测外部结果。 - 降级路径落地(§14.2 行1444):Event Bus 暂停时核心状态仍可提交、订阅从事件位点补发;瞬态事件不落盘(行736)与持久写路径(L2 单写者)物理隔离,数据面吞吐不受持久化尾延迟牵制。 - 架构守护 CI 常驻:Bus 被当 RPC 通道、审批/Task 命令混入事件流、混层缓存超出「可丢失运行时缓存」范畴(行612-616)三种侵蚀一经出现即测试失败;Broker 自身故障的兜底仅由 L0 Supervisor 重启/generation 切换承担(行1123),不得以增设特权降级通道的方式实现。 
## 开放问题（未决；供里程碑回看时审视） 
- **「没有任何特权通道」的字面效力**：机制已收敛（授权后受约束直连＋审计回写不构成旁路），但原文绝对化表述是否改写为「lease 授权的专用通道」未定。 - **投影漂移是否需要第二道自愈机制**：是否引入 level-triggered 对账／重列（EMPIRICAL 主张需要，FOR 反对）；留待 M2 以「生产者伪造／乱序」故障注入证伪。 - **统一调用 envelope 的字段边界**：deadline/取消/重试/幂等/resume_cursor 是否按调用类别（短调用／流式／长任务）可选化或裁剪，未收敛。 - **Broker 单点故障与降级路径**：是否在基线明示「统一入口≠统一可靠性」、是否为 Broker 增设进程内降级路径而不违反内核最小机制；目前仅达成「L0 Supervisor 兜底重启／generation 切换」最低共识。 
## 后果 
- 本 ADR 并入后,基线 §17 对应裁决以本文件为准;条件的验收责任落在对应里程碑(条件中已注明 M2/M3/M4/M7 等)。 - 条件全部闭合前,该裁决对外宣称口径为「有条件成立」;任一验收被证伪即触发本 ADR 复审(而非默认回退)。 