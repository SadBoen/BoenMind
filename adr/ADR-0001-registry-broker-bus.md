# ADR-0001 Registry/Broker/Bus 三权分立

- 状态: accepted-with-conditions
- 日期: 2026-08-28
- 决策类型: 架构裁决(对基线 §17 裁决 R1 的复核结论)
- 来源: Zen consensus 多模型辩论——三个模型家族分任三方并跨裁决轮换角色(FOR=glm-5-turbo, AGAINST=gpt-5.6-luna, EMPIRICAL=gemini-3.7-flash);两轮(独立立场→交叉质证)+逐裁决合成
- 辩论记录: `architecture/debates/R1-RegistryBrokerBus-transcript.md`
- 辩论数据: 共识 10 项 / 未决分歧 4 项 / 条件 7 项 / 决策要点 8 项;合成结论 **uphold_with_conditions**

## 背景(原裁决文本)

> Registry 负责「谁提供什么」(统一注册中心,按对象类型分表);Broker 负责「能不能调用以及调用谁」(所有跨域调用统一入口,没有任何特权通道);Bus 负责「发生了什么、进度如何、异步消息如何传播」。三者职责严格分离,一切调用方共用同一 Broker 入口。

## 裁决(决策要点)

1. 确认 Registry/Broker/Bus 三权分立为 Runtime Core 的常驻结构:Registry 持有「谁提供什么」(统一注册中心按对象类型分表,持久逻辑目录+可丢失运行时缓存两层),Broker 持有「能不能调用以及调用谁」,Bus 持有「发生了什么、进度如何、异步消息如何传播」(持久事实源+内存分发层);三者为同一进程内的协议角色分立,不要求分进程部署。
2. 所有跨域调用(Agent/前端按钮/语音/自动化规则/Timer/Butler/其他 App)必须经 Broker 统一入口;裁定「没有任何特权通道」的准确含义为:不存在未经 Broker 授权建立、或脱离审计关联的调用与数据通道;由 Broker 授权后建立、且生命周期与收据摘要回写 Broker/Bus 的受约束数据面快路径不构成特权通道,与任何已授权调用无关的独立数据传输仍一律禁止。
3. Broker 在授权决策点从 Registry 取 binding 并固化不可变 binding_epoch 与 provider_instance_id,写入调用凭证与审计记录并由 Provider 侧校验,不匹配即拒绝或重试;热替换(§13.1 draining→handshake→原子切 binding)只影响后续调用,不得改变在途调用的授权-执行-审计一致性;须对 §6.4 Provider Binding 结构与 §7 调用关联字段做相应字段增补。
4. Broker 七步管线是逻辑职责拆分而非运行时串行管线:实现必须把策略预编译为按「调用方×目标 Capability」的 O(1) capability 查表(同 seL4/Mojo 先例),禁止逐条策略求值进入热路径;M4 里程碑以 p99 压测与队头阻塞注入测试证伪,超标即回炉实现方案。
5. 高频瞬态流(模型 token 增量、UI 打字机帧、音视频、大文件传输)走 Broker 授权的数据面快路径:不逐帧通过七步管线、不落盘(§8 行736)、不占用 L2 持久单写者;持久事实只在 Broker 状态提交点产生。
6. Bus 事件严格表达已发生的事实,禁止命令语义;Bus 层实施事件 schema 机器校验并在持久化前拒绝违规事件,持久事件生产者集合限于 Broker 状态提交点;event_seq 全局单调,所有投影、消费位点与 resume cursor 仅由持久日志重建;Orchestrator 任务板只是投影,不是第二事实源。
7. 异步协作的「请执行下一步」是一次新的 Broker Capability Call,由 Orchestrator 或订阅事实的一方发起;禁止在 Bus 之外另建 durable command queue 等绕过统一入口的隐形机制,事件流不得被当作 RPC 通道使用。
8. 外部副作用的可靠性不依赖调用原子性:副作用类 Provider 的 Capability 合同强制实现收据、按 operation_id/idempotency_key 查询与幂等重放防护;Runtime Core 以事务性 outbox 绑定本地状态提交、审计写入与事实事件的发布顺序;outcome_unknown 一律先核验外部系统收据再决定重试(§13.3);统一入口承诺统一策略与统一审计,不承诺统一副作用原子性。

## 对基线的修订(自并入起生效;正文未逐条改写处,以本节文本为准)

- (无)

## 条件与验收

- Broker 策略必须预编译为「调用方×目标 Capability」的 O(1) capability 查表,七步管线只允许作为逻辑拆分存在;此项列为 M4 Broker 实现的验收项,证伪判据:统一入口 p99 开销、队头阻塞注入、Broker 故障半径测试,超标即回炉实现方案并重审本裁决的运行前提(被证伪的是实现而非三权分立本身)。
- 基线须新增 binding_epoch + provider_instance_id 机制(修订 §6.4 Provider Binding 结构与 §7 调用关联字段):Broker 在授权决策点固化 epoch,注入调用凭证与审计记录,Provider 侧校验不匹配即拒绝/重试;测试:在授权返回与 Provider 提交之间强制切换 binding,验证执行对象、审计对象与策略摘要三方一致;Runtime generation 变更不得改变已签发在途调用的授权与审计归属。(注:FOR-R2 称 §6.4 行604 已含 generation 字段经核为误引,基线现文本仅含 Runtime Core 代际,故此项为必做增补而非既有性质。)
- Bus 层实施事件 schema 机器校验:含命令语义的事件在持久化前拒绝并告警;持久事件生产者白名单限于 Broker 状态提交点;在「生产者伪造/乱序发布」故障注入下,投影必须可重建或异常可检出,否则事件溯源前提不成立。
- 数据面分流合规四测试(采纳 AGAINST-R2 测试组,作为「授权后快路径」的准入门槛):① 无 Broker 签发授权(lease:binding_epoch+策略版本+operation_id+deadline)的通道建立必须被拒绝;② epoch/generation 变更不得影响已授权通道的审计归属;③ 通道吞吐不得被 L2 持久单写者 p99 牵制;④ 崩溃注入下外部提交与收据摘要可最终对账,且通道字节数/阶段/收据摘要/错误全程回写 Broker/Bus。
- 副作用类 Provider 合同强制:实现收据、按 operation_id/idempotency_key 查询与幂等重放防护;Runtime Core 以事务性 outbox 绑定「本地状态提交→审计写入→事实事件追加」的顺序;可证伪指标:「外部提交成功而审计/事实缺失」比例与恢复后重复副作用比例须设上限并纳入回归测试;通用 Broker 禁止以盲目重试猜测外部结果。
- 降级路径落地(§14.2 行1444):Event Bus 暂停时核心状态仍可提交、订阅从事件位点补发;瞬态事件不落盘(行736)与持久写路径(L2 单写者)物理隔离,数据面吞吐不受持久化尾延迟牵制。
- 架构守护 CI 常驻:Bus 被当 RPC 通道、审批/Task 命令混入事件流、混层缓存超出「可丢失运行时缓存」范畴(行612-616)三种侵蚀一经出现即测试失败;Broker 自身故障的兜底仅由 L0 Supervisor 重启/generation 切换承担(行1123),不得以增设特权降级通道的方式实现。

## 共识(经质证未被驳倒的命题)

- 三权分立是同一进程内的协议角色分立而非物理分进程(基线 §12.1 行1104-1123):Registry/Broker/Bus 同处 Runtime Core 与 D-Bus 单守护进程(名字注册+逐消息策略+信号路由共实现)、Kubernetes kube-apiserver(认证/RBAC/admission/审计内联)的已验证形态同构,有大量装机量先例;AGAINST-R2 明确接受「三权首先是合同角色而非必须分进程」。
- 统一 Broker 入口是不可绕过的策略与审计强制点:七类调用方(Agent/按钮/语音/自动化规则/Timer/Butler/其他 App,§7 行638-650)共用同一入口,身份/权限/Task scope/参数校验/绑定/审计单点强制;任何绕过路径必然表现为审计缺口,input_trust 信任分级(行677/364/M4.8)只有在该单点才能被强制执行。AGAINST-R2 明确让步:「统一策略入口不能退回到调用方自律」。
- Registry 的单一「名字→实现」间接层+原子 binding 切换(§6.4 行594-619、§13.1 行1172-1186、§16 行1506)是插件热替换的机制前提:替换压缩为一次原子切换而调用方零感知;发现/路由逻辑散落到 Broker/Bus 或调用方内部会使热替换退化为分布式一致性问题并产生旧句柄悬空。两轮质证未被驳倒。
- 授权-执行-审计一致性必须由不可变 binding epoch(或 generation)快照保证:Broker 在授权决策点固化 binding_epoch 与 provider_instance_id,注入调用凭证、写入审计并由 Provider 侧校验,不匹配即拒绝/重试;单进程形态下该机制足以消除热替换窗口的 TOCTOU(OSGi ServiceReference resolve 时快照、D-Bus Unique Name 先例),无需分布式事务。三方在 R2 收敛——但注意:经核验,基线 §6.4/§7 现文本并不含该字段(全文 generation 仅指 Runtime Core 代际,行1223-1268),FOR-R2 引注「§6.4 行604 已含 generation」为误引,该机制是须新增的基线扩展而非既有性质。
- Broker 七步管线(§7 行625-636)是逻辑职责拆分而非运行时串行管线:策略必须预编译为按「调用方×目标 Capability」的 O(1) capability 查表(seL4/Mojo 先例,每调用检查可低于微秒);若实现为逐条策略求值则形成队头阻塞,届时被证伪的是实现方案而非三权分立本身。FOR-R2 主动接受、EMPIRICAL 提出、AGAINST 的异议针对持久化管线而非查表本身,三方收敛。
- 控制面与数据面分流:Broker 负责授权、绑定与审计关联;高吞吐瞬态流(模型 token 增量、UI 打字机帧、音视频、大文件)经 Broker 授权建立的受约束通道(内存引用/共享缓冲/直连描述符)传输,不逐帧穿透完整管线、不落盘(§8 行736)、不占用 L2 单写者(行739);通道的建立、生命周期、字节数/收据摘要与错误必须回写 Broker/Bus。三方 R2 收敛:FOR/EMPIRICAL 以 D-Bus(daemon 授权后 Unix socket 直连)与 Chromium Mojo(browser process 授权后共享内存)论证,AGAINST 以 invocation lease 提出机制同构方案。
- Bus「事件=已发生的事实而非请求」(§8 行696)语义保留:它是崩溃恢复(投影、消费位点、resume cursor 仅由全局单调 event_seq 重建,行739)与副作用安全(§13.3 行1203-1219 先核验外部系统再决定重试)的共同机制前提;异步协作的「请执行下一步」是一次新的 Broker Capability Call,由 Orchestrator 或订阅事实的一方发起(§17 行1549 任务板只是投影),不向 Bus 投递命令、不在 Broker 之外另建 durable command queue。AGAINST 的「隐形第四套机制」论点被 Kafka Streams 事件驱动拓扑反例驳回,其 R2 亦接受事实语义必须保留。
- 事实纪律需要工程强制力而非生产者自觉:Bus 层事件 schema 机器校验(拒绝含命令语义的事件持久化)、持久事件生产者集合限于 Broker 状态提交点(行733-735)、叠加瞬态事件不落盘(行736)与 L2 单写者追加(行739)构成三道防线。EMPIRICAL 提出、FOR-R2 接受纳入立场、AGAINST 要求错误事实不得被持久重放,三方收敛。
- 外部副作用不因统一入口而原子化:任何架构(含 AGAINST 提议的事务协调器)都无法回滚已到达外部系统的副作用;统一入口承诺统一策略与统一审计,不承诺统一副作用可靠性。副作用恢复依赖 Provider 收据/按 operation_id/idempotency_key 查询/幂等协议与 Runtime Core 事务性 outbox(绑定本地状态提交、审计写入、待发布事实),Broker 定位为幂等协调与调用拦截器,§13.3 outcome_unknown 是兜底路径而非充分机制。三方 R2 收敛(EMPIRICAL-R2 明确支持 AGAINST 的本质论点,FOR-R2 接受收据/outbox 为 §13.3 的实现机制)。
- 场景适配判断成立:单用户单设备+进程内实现+本地持久化把三权分立的成本压到最低(无网络分区与共识问题),而长期自治 Agent 使崩溃、休眠唤醒、generation 切换成为常态事件而非异常,恢复路径必须常态可用;相较无中心网状拓扑(纯 OTP 进程拓扑),中心化裁决点在可调试、可审计与可恢复上占优。三方均持,未被驳倒。

## 未决分歧(如实记录,不强行抹平)

- 「没有任何特权通道」的字面效力未决:机制已收敛(授权后受约束直连+审计回写不构成旁路),但 FOR/EMPIRICAL 认为 R1 自身职责界定(Broker 管「能不能调用以及调用谁」、行623「Bus 只负责消息传输」、行685「总线不应取代所有 RPC」)已把载荷传输排除在 Broker 管线外,原文无须改动;AGAINST 坚持原文绝对化表述过强、与流式/长任务目标自相冲突,必须改写为「lease 授权的专用通道」。主席以释义方式裁决(见 adr_points),字面是否修订的异议如实保留。
- 投影漂移是否需要第二道自愈机制未决:EMPIRICAL 指出 Kubernetes 拒绝把 Events 当事实源、控制器靠 level-triggered re-list/resync 对账收敛,持久事件日志即事实源是 R1 最大的经验风险点;FOR 主张单用户本地场景下事件溯源的可测试性优于对账循环的收敛性证明,以 schema 校验+event_seq+受限生产者集合为足够防线,明确反对引入对账循环;EMPIRICAL-R2 将要求收敛为 schema 校验+event_seq 但未再坚持对账、亦未正式撤回。留待 M2 事件层以「生产者伪造/乱序」故障注入证伪。
- 统一调用 envelope 的字段边界未决:AGAINST 认为 §7 行667-681 将 deadline/取消/重试/幂等/resume_cursor 对所有调用强制,使短调用、流式调用、长任务共享同一状态机,叠加流专用调度、租约、背压、配额、死信后协议复杂度与测试矩阵快速膨胀;FOR 认为数据面分流后剩余差异只是 Broker 内部路由策略,不构成协议层问题。「关联字段是否按调用类别可选化/裁剪」在两轮内未形成一致结论。
- 文档承诺边界与 Broker 单点故障半径的表述未决:三方均承认 Broker 自身故障即一切跨域调用不可用、同进程分权不是故障分权(§12.1 行1121 进程内组件不能独立崩溃和热替换),但对基线是否须在 Broker 职责定义处明示「统一入口≠统一可靠性」(FOR 认为行681 与 §13.3 已隐含该边界;AGAINST 要求显式写出可靠性由 Provider 收据协议承担),以及是否/如何为 Broker 增设进程内降级路径而不违反内核最小机制,仅达成「L0 Supervisor 兜底重启/generation 切换」的最低共识,降级路径的具体形态未决。

## 后果

- 本 ADR 并入后,基线 §17 对应裁决以本文件为准;条件的验收责任落在对应里程碑(条件中已注明 M2/M3/M4/M7 等)。
- 条件全部闭合前,该裁决对外宣称口径为「有条件成立」;任一验收被证伪即触发本 ADR 复审(而非默认回退)。
