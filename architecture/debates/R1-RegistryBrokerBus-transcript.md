# Zen Consensus 辩论记录:R1 Registry/Broker/Bus 三权分立

辩论对象:§17 裁决 R1「Registry/Broker/Bus 三权分立」

> 协议:Zen consensus——三个不同模型家族担任三方(FOR=架构师钢人论证,AGAINST=安全可靠性挑战,EMPIRICAL=真实系统实证裁决),两轮(独立立场→交叉质证),缺席与不可裁决的分歧如实记录。

角色分配:FOR=glm-5-turbo, AGAINST=gpt-5.6-luna, EMPIRICAL=gemini-3.7-flash

## 裁决原文

裁决:Registry 负责「谁提供什么」(统一注册中心,按对象类型分表);Broker 负责「能不能调用以及调用谁」(所有跨域调用统一入口:身份/权限/Task scope/参数校验/绑定/超时取消重试/审计,没有任何特权通道);Bus 负责「发生了什么、进度如何、异步消息如何传播」(持久事实源+内存分发层,事件表达已发生的事实而非请求)。三者职责严格分离,一切调用方(Agent/按钮/语音/自动化规则/Timer/其他 App)共用同一 Broker 入口。

## 第一轮:独立立场

### 【FOR】

```json
{
  "conceded_risks": [
    "高频瞬态流(模型 token 增量、UI 打字机帧)与低风险确定性调用会被三层间接拉高延迟与复杂度,若不做 §8 行736 的瞬态事件不落盘和进程内快路径,单用户场景可能过度设计。",
    "四种交互语义(§9)的边界在实践中易被侵蚀:把 Bus 当 RPC 通道、把审批/Task 消息混入事件流,会使『事实非请求』退化,需要持续的架构守护与测试纪律。",
    "Broker 单入口同时是性能与可用性的集中点:Broker 自身故障即一切跨域调用不可用,必须由 L0 Supervisor 与进程内降级路径兜底,加重了『内核最小机制』的实现负担。"
  ],
  "evidence": [
    "基线 §6(行538-619):统一注册中心按对象类型分表;持久逻辑目录与可丢失运行时缓存两层;调用方只依赖稳定 Capability 名。",
    "基线 §7(行621-681):Broker 唯一调用管线(身份/权限/scope/校验/绑定/超时取消重试/审计);Agent、前端按钮、语音、自动化规则、Timer、Butler、其他 App 七类调用方共用入口(行638-650);correlation 字段与『operation_id 对应可恢复执行状态』(行681)。",
    "基线 §8(行683-776):『事件表达已发生的事实,不是请做什么』(行696);持久事实源+内存分发两级结构(行728-737);event_seq 全局单调、休眠唤醒按剩余量重算 deadline(行739)。",
    "基线 §13.1/§13.2(行1172-1201):热替换=Registry 原子切换 binding;崩溃恢复=Supervisor/Registry/Bus 三方协作序列,『不能让调用方无限等待』。",
    "基线 §13.3(行1203-1218):outcome_unknown 先核验外部系统再决定重试,证明事实与请求分离直接决定副作用安全。",
    "基线 §16(行1497-1514)与 §17(行1516-1557):『需要替换实现→Registry 原子切换 Provider』及裁决原文。",
    "Erlang/OTP:supervisor 树管生死、name registry 管寻址、mailbox 只管传输,三者分离支撑 AXD301 级可用性与热代码升级;mailbox 从不被当事实源,否则重启即丢历史。",
    "Kubernetes:Service/EndpointSlice(谁提供什么)与 kube-proxy(能否到达、怎么转发)分离;controller 一律 level-triggered,靠 watch 事实流而非点对点请求收敛;etcd 是唯一持久事实源。",
    "Chromium Mojo:渲染进程不存在绕过 browser process 的能力通道,权限裁决单点化是其沙箱安全成立的机制条件——『无特权通道』的同构先例。",
    "VS Code:contribution points 注册、统一命令分发(executeCommand)、onDidChange* 事件三类机制分离,使数千扩展在不改内核前提下共存与热替换。",
    "OSGi:service registry 的注册/解绑/追踪使 bundle 可动态装卸,证明注册面与调用面分离是热替换的先决条件。",
    "systemd:dbus 暴露服务注册与状态信号,journal 持久记录已发生事实并与 unit 运行状态分离——单机系统同样坚持注册面/事实面分离。"
  ],
  "key_arguments": [
    "Registry 的「名字→实现」单一间接层(Provider Binding)把插件热替换压缩为一次原子 binding 切换而调用方零感知,若发现/路由逻辑散落在 Broker、Bus 或调用方内部,热替换立即退化为分布式一致性问题并产生旧句柄悬空(§6.4 行604、§13.1 行1174-1186)。",
    "Broker 作为无特权统一入口把安全模型变成可证伪性质——任何绕过路径必然表现为审计日志缺口,且提示注入防线(§4.5)所需的 input_trust 分级(§7 行677)只有在这个单点才能被强制执行而非各调用方自觉遵守(§7 行638-650)。",
    "Bus 的「事件=已发生事实而非请求」语义加「持久事实源+内存分发」两级结构是崩溃可恢复的机制前提:投影、消费位点、resume cursor 全部可由全局单调 event_seq 重建,并阻断恢复流程把请求当事实重放导致的副作用双发(§8 行696/728-739;§13.3 行1203-1218)。",
    "三条窄合同使三个平面可独立替换升级——Registry 换存储、Broker 加策略、Bus 换实现都不触碰调用方,这是「内核由合同与最小机制组成、万物皆插件」唯一能机制化落地的结构而非口头承诺(§17 行1551-1552、§16 行1506)。",
    "单用户单设备恰恰把三权分立的成本压到最低(进程内库+本地持久文件即可,无网络分区与共识问题),而长期自治 Agent 使崩溃、休眠唤醒、generation 切换成为必然事件而非异常,恢复路径必须常态可用——该场景下三权分立的收益成本比达到峰值而非谷底。",
    "三权分立把「当前真相」锚定在 Registry(持久逻辑目录不依赖内存指针,§6.4 行606-617)、「历史事实」锚定在 Bus 持久日志,配合「Orchestrator 任务板只是投影」(§17 行1549),构成对第二事实源漂移的可测试防线——任何混层缓存超出可丢失运行时缓存范畴即可被一致性测试证伪。"
  ],
  "role": "FOR",
  "stance": "R1 三权分立不是架构风格偏好,而是四条硬目标——插件热替换、崩溃可恢复、无旁路可审计安全、内核最小机制——唯一能机制化落地的最小机制结构,且单用户单设备场景把它的成本压到最低、把长期自治收益推到最高。"
}
```

### 【AGAINST】

```json
{
  "conceded_risks": [
    "统一 Broker 确实能把身份、权限、参数校验和审计集中到一个可测试的强制点，显著降低 Agent、GUI、CLI 各自实现安全策略而产生的绕过风险。",
    "Registry 与 Bus 分离能避免把当前 Provider 目录、运行时健康状态和已发生事实混成一个不可恢复的共享状态源。",
    "事件重复投递、Provider 崩溃和副作用未知结果若没有统一关联标识，恢复时很容易出现重复执行或审计断链；基线第 7 节和第 13.3 节对此类问题的约束方向是正确的。"
  ],
  "evidence": [
    "基线 §7 第 621-681 行把查表、授权、参数校验、绑定、超时取消重试、结果、审计和事件发布全部串入 Broker；这形成单一串行化热点，而 §8 第 728-739 行又要求持久日志由 L2 单写者追加，二者叠加后所有高频调用都竞争同一控制面。",
    "基线 §7 第 667-681 行要求每次调用携带 operation_id、deadline、cancellation、idempotency_key 和 resume_cursor；这实际上把流式订阅、取消、恢复和副作用提交的状态机塞进 Broker，超出简单能力路由器的职责边界。",
    "基线 §13.1 第 1172-1186 行允许 Provider 在 draining 后切换 binding，但 §7 第 627-635 行没有明确规定授权决策、Provider 版本和最终执行之间必须使用不可变 binding 版本；若切换发生在校验后，审计可能显示 A，实际执行却落到 B。",
    "基线 §13.3 第 1203-1219 行承认邮件发送、下单、文件写入存在 outcome_unknown，要求先查外部系统或请求用户裁定；这证明 Broker 的统一超时/重试并不能提供原子调用语义，真正的可靠性必须依赖每个 Provider 的收据、查询和恢复协议。",
    "基线 §14.2 第 1437-1447 行要求 Event Bus 暂停时核心状态仍可提交、订阅从位点补发；但 §7 第 633-635 行把“返回结果、记录审计并发布事件”放在一次调用尾部，未定义状态提交、审计写入、事实事件追加三者失败时的原子边界。",
    "Kubernetes 的 API Server 适合对声明式资源做认证、准入和持久化，但 kubelet、container runtime 和节点本地探针仍使用专门的本地控制协议；这说明“所有跨域操作都经一个通用 Broker”并不适合实时流、长连接和故障隔离场景。",
    "Kafka 将持久日志、消费者位点和发布/订阅解耦，并不把每条消息的业务授权、幂等副作用恢复和外部收据都强行放入 broker；其实践表明事实传播层与业务事务协调层需要不同的故障模型。",
    "D-Bus 的 daemon 可集中做名称发现和部分策略，但高吞吐数据流通常走 Unix domain socket、共享内存或专用协议；把控制面和数据面都经过统一代理会产生可观测的吞吐和尾延迟代价。"
  ],
  "key_arguments": [
    "将每个调用都串过 Broker 会把本地低风险热路径、流式传输和长任务控制集中到一个进程与持久化单写者，Broker 或其数据库抖动即可让本来互不相关的音频、UI、Timer 和 Provider 调用一起出现尾延迟或级联阻塞。",
    "三权分立没有解决“副作用、审计、状态事件”的原子性：Provider 已发邮件但 Broker 在收据写入前超时，Bus 可能没有事实、审计可能缺 operation 结果，而恢复器只能进入 outcome_unknown 并暂停 Task，用户最终面对的是已发送却无法确认的订单或邮件。",
    "Registry 的动态 binding 与 Broker 的授权存在 TOCTOU 缺口：权限检查针对健康的 Provider A 完成后，热替换切到合同兼容但信任级别或数据驻留不同的 Provider B，若不把 binding generation 固定进授权票据，调用对象和审计对象就不一致。",
    "把“事件只能表达事实”作为硬边界会迫使异步协作另造 durable command queue、租约、背压和投递确认机制；否则订阅者无法可靠表达“请执行下一步”，而把该请求折返 Broker 又会增加往返、重试和死信复杂度，形成事实总线之外的隐形第四套机制。",
    "统一入口对 Timer、自动化规则和 Agent 使用同一调用模型，无法自然表达批量事务、租约持有、持续订阅、实时音视频和大文件流；若强行抽象成 capability call，取消、背压、部分成功和资源配额都会被塞进通用 envelope，协议复杂度与测试矩阵快速膨胀。"
  ],
  "role": "AGAINST",
  "stance": "R1 把控制面、数据面和事务恢复过度收拢到统一 Broker，牺牲了故障隔离、实时性与副作用可证明性；应保留统一策略入口，但允许受合同约束的专用数据通道和事务协调器。"
}
```

### 【EMPIRICAL】

```json
{
  "conceded_risks": [
    "同进程分权不是故障分权(§12.1,1108-1123 行):Registry 锁竞争或 Broker 拥塞会冻结所有调用方,而 OTP 的无中心消息拓扑恰恰没有这种单一卡点,k8s 也长期把 apiserver 视为扩缩容瓶颈——单设备规模可缓解,但必须回答锁粒度与队头阻塞问题。",
    "统一入口容易演化成「上帝中间件」:k8s admission 与 D-Bus 策略的历史都显示单一裁决点会持续吸聚功能并拖长全部调用的尾延迟,若 Broker 按 §7 的七步管线(625-636 行)逐步求值而不预编译成查表,每一步都在叠加延迟。",
    "「事件=已发生事实」的纪律若无 Bus 层事件 schema 机器校验,只能靠生产者自觉与评审维持;某个 Provider 把请求伪装成事实发布时,持久日志的重放会放大而非记录错误——这是 k8s 用 level-triggered 对账刻意规避、而 BoenMind 选择承受的已知代价。"
  ],
  "evidence": [
    "Kubernetes:kube-apiserver 是标准部署中唯一写 etcd 且唯一对外入口,认证/RBAC/admission/审计内联(验证 Broker 单一入口);kubelet 走专门的 NodeAuthorizer 第二授权通道( BoenMind 更严的反例面);Events API 为短 TTL 的 best-effort 记录,控制器靠 informer re-list/resync 做 level-triggered 对账(对「持久事件日志即事实源」的已知反例)。",
    "Erlang/OTP:register/global/pg 名字注册 + 无访问控制的消息传递 + 监督树重启恢复,无持久事件总线(恢复靠重建而非回放);code server 支持双版本在位热替换但业界罕用,主流转向滚动/蓝绿代际发布——与 §13.4(1223 行)的 generation 方案一致,证明极端宽松的通信模型也能数十年在线,R1 的严格性是安全/审计选择而非可用性必需。",
    "Chromium Mojo 与 seL4:capability 制 IPC,由浏览器进程/内核作为 broker 只授予已获权接口,每次调用的检查是预编译 capability 的 O(1) 查表——证明「统一入口+每调用强制检查」可在微秒级完成,是 R1 可负担性的直接先例。",
    "D-Bus/systemd 桌面栈:单一守护进程同时承担名字注册(RequestName)、逐消息策略检查与信号路由,且保留 peer-to-peer 直连例外——证明三权可共实现于单进程(与 §12.1 同构),但也说明现实系统保留了特权直连,BoenMind 的「零特权通道」超出先例。",
    "OSGi:服务注册中心 + 服务引用原子切换 + bundle 热安装/升级,与 §13.1 draining→handshake→原子切 binding(1172-1186 行)同构;OSGi 类空间/版本耦合事故反证按「整个 Provider 版本」切换而非按类或函数替换的合理性。",
    "Kafka:持久分区日志 + 消费者位点 + 至少一次投递 + 要求消费端幂等,与 §8.3(728-739 行)「持久事件日志→路由→各消费者位点」逐条对应;基线佐证另见 §6 双层注册表(606-619 行)、§7 统一调用形态与关联字段(661-681 行)、§16(1503/1506 行)、§17(1535-1537/1556 行)。"
  ],
  "key_arguments": [
    "职责三分本身是已被验证的最佳实践组合而非激进设计:单一裁决入口由 Kubernetes kube-apiserver 整建制验证(所有调用方——kubectl、控制器、kubelet、operator——都必须经认证、RBAC、admission、审计,无第二调用通道),与 BoenMind §7(621-682 行)逐项对应。",
    "三权的「分」是协议角色而非物理分立:k8s 把 Registry 与 Broker 融在同一 apiserver,D-Bus 在单一守护进程里同时做名字注册、逐消息策略检查和信号路由,BoenMind §12.1(1108-1117 行)把三者放进同一 Runtime Core 进程恰与 D-Bus 同构——这是保守的、有大量装机量背书的形态。",
    "比一切先例更严格处一是「零特权通道」:BoenMind 连 Timer、前端按钮、自动化规则都强制走 Broker(650 行),而 VS Code 扩展宿主的 RPC 无每调用鉴权、Erlang/OTP 对进程间消息完全没有访问控制、k8s 还为 kubelet 保留了专门的 NodeAuthorizer 第二授权路径——这条超强度条款的代价(p99 开销、首拥塞点)必须在 M4 用压测证伪,不能当既成结论。",
    "比先例更严格处二是「持久事件日志即事实源 + 全局单调 event_seq」(§8.3,728-739 行):Kubernetes 明确拒绝把 Events 当事实源(短 TTL 后丢弃,控制器靠 level-triggered 重 LIST/resync 对账),BoenMind 选的是 Kafka/事件溯源一系而非 k8s 一系,回放正确性完全押在所有生产者遵守「事件=已发生事实」纪律上(§8.1,696 行),这是整个裁决里最大的经验风险点。",
    "比先例更宽松处同样明确且代价可控:Runtime Core 升级放弃 OTP 式在位双版本热替换、改用 generation 切换(§13.4,1223 行),崩溃恢复靠 Supervisor 重启而非事件回放(§13.2,1188-1199 行),且没有 Envoy 级熔断/负载均衡/多租户配额——这些放宽与业界从热升级转向蓝绿/代际发布的收敛一致,换来的是单团队可实现的工程可行性。",
    "可证伪判据在实现层而不在架构层:seL4 与 Chromium Mojo 证明每调用强制检查可做到纳秒-微秒级,前提是把策略预编译成每调用方 capability 表做 O(1) 查表;若 Broker 按 §7 的七步管线(625-636 行)逐条求值策略,同进程 Broker 将成为队头阻塞点,届时被证伪的是实现方案而非三权分立本身。"
  ],
  "role": "EMPIRICAL",
  "stance": "R1 的三权分立就职责分解本身属于被 D-Bus、Kubernetes apiserver、Chromium Mojo、OSGi 和 Kafka 等真实系统分别验证过的最佳实践组合而非无先例激进设计,但「绝对零特权通道」和「持久事件日志即事实源」两处强于所有先例,应作为待 M4/M2 证伪的假设而非既成结论。"
}
```

## 第二轮:交叉质证

### 【FOR-R2】

```json
{
  "role": "FOR-R2",
  "stance": "经质证修正：接受「Broker 七步管线必须预编译为 O(1) capability 查表」作为 R1 生效的实现前提，接受「持久事件日志的机器 schema 校验」作为事实纪律的工程必要条件；三权分立的核心结构论不变。",
  "key_arguments": [
    "接受 EMPIRICAL 的预编译查表论点并修正自己立场：EMPIRICAL 正确指出 seL4/Mojo 先例中每调用检查是 O(1) capability 查表而非逐条策略求值。若 Broker 按 §7 行625-636 的七步管线（身份→权限→scope→校验→绑定→超时取消重试→审计）每步独立求值，同进程内的确会形成队头阻塞。修正结论：R1 的正确性依赖「Broker 内部实现把策略预编译为调用方+目标 Capability 的 O(1) 查表，七步管线是逻辑拆分而非运行时串行管线」，这应作为 M4 压测的证伪判据，但反驳 AGAINST 由此推导的「应允许专用数据通道」——内部实现优化不等于协议层开特权旁路，正如 seL4 内核做了 capability 缓存优化但从不允许绕过 capability 检查的 IPC 通道。",
    "驳斥 AGAINST 的 TOCTOU 论点——Binding generation 快照使审计一致性可在单进程内原子保证：AGAINST 声称 Registry 动态 binding 与 Broker 授权存在 TOCTOU 缺口（校验时是 Provider A，执行时已切到 Provider B）。这个批评在分布式系统里成立，但在 §12.1 行1108-1117 明确的「同一 Runtime Core 进程」内不成立：Broker 在授权决策点从 Registry 取到 binding 并锁定其 generation 号（§6.4 行604 的 Provider Binding 结构已含 generation），后续绑定和审计全部携带该 generation，直到本次调用完成。这恰是 OSGi ServiceReference 在 resolve 时快照服务引用、直到 release 前不随注册表变化的同构机制——OSGi 二十年的运行证明单进程内 binding generation 快照足以消除 TOCTOU，不需要分布式事务。EMPIRICAL 的「待证伪假设」框架在此处的应用应修正为：TOCTOU 风险仅当 Broker 与 Registry 跨进程部署时才成立，单进程形态下已被先例充分验证。",
    "驳斥 AGAINST 的「原子性缺口」论点——outcome_unknown 恰是三权分立的机制成果而非缺陷：AGAINST 声称 Broker 统一超时/重试不能提供原子调用语义，以「邮件已发但审计未写」为例。这个批评混淆了「调用语义的原子性」与「外部副作用的原子性」：任何架构——包括 AGAINST 提议的「专用事务协调器」——都无法让「已到达外部 SMTP 服务器的邮件」回滚。§13.3 行1203-1218 的 outcome_unknown 机制恰恰是三权分立的机制优势：Bus 持久日志记录了「Broker 发起了调用、Provider 报告了 outcome_unknown」这一事实，恢复器据此查询外部系统收据。若没有 Bus 的持久事实源和 Broker 的统一 correlation-id（§7 行681），这个恢复路径根本不存在——谁来保证恢复查询的触发？谁来保证不会重复发送？AGAINST 的「事务协调器」方案本质上是把 Broker 的一部分职责搬到一个新组件，但只要这个新组件不经过 Broker 的统一入口，它本身就构成 AGAINST 未解释如何审计的特权通道。",
    "接受 EMPIRICAL 的「持久事件日志即事实源是最大经验风险点」并补充防线机制：EMPIRICAL 正确指出 Kubernetes 拒绝把 Events 当事实源而采用 level-triggered 对账，BoenMind 选的是 Kafka/事件溯源一系。我接受这是 R1 中经验风险最高的条款。但修正点在于：Kubernetes 拒绝事件当事实源的原因是 Events API 为短 TTL best-effort 记录且生产者不可控（任意控制器都写 Events）——而 BoenMind §8 行696 明确约束「事件表达已发生的事实，不是请做什么」且只有 Broker 调用完成后的状态提交点才产生事件（行733-735），生产者集合远小于 Kubernetes。补充防线：§8 行736 的「瞬态事件不落盘」+ §12.2 的「L2 单写者追加」双重约束使持久日志的写入路径可控，再加上 EMPIRICAL 提出的「Bus 层事件 schema 机器校验」（我接受此点并纳入修正立场），三道防线叠加使 BoenMind 的事实纪律执行成本远低于 Kubernetes 控制器生态。对 EMPIRICAL 的「待 M2 证伪」判定表示同意，但反对由此推导的「应改用 level-triggered 对账」——单用户本地场景下事件溯源的可测试性优于对账循环的收敛性证明。",
    "驳斥 AGAINST 的「专用数据通道」主张——「不同调用模型」是 Broker 内部的路由策略差异而非协议层特权旁路：AGAINST 以音频流、大文件传输、长连接为例论证统一入口无法自然表达。这个论证犯了层混淆错误：三权分立约束的是「所有跨域调用的入口与审计点」，不是「所有跨域调用的传输协议」。§7 行621-635 的 Broker 管线处理的是调用的授权、绑定和审计元数据，实际的载荷传输完全可以——且在 §12.1 行1108-1123 的同进程架构下自然就是——通过内存引用传递或共享缓冲区完成，不经过任何序列化/反序列化。这恰与 D-Bus 的设计同构：D-Bus daemon 处理名字注册和策略检查，高吞吐数据流走 Unix domain socket 直连——但直连仍需经过 daemon 的策略裁决建立连接，之后才切换到数据面。AGAINST 引用 D-Bus 的「peer-to-peer 直连例外」来反对「零特权通道」，但忽略了 D-Bus 的直连是在 daemon 授权后建立的受约束通道，而非未经授权的旁路。BoenMind 完全可以采用同构方案：Broker 完成授权后建立直连引用，后续流式传输走快路径，但审计关联和生命周期管理仍在 Broker。",
    "驳斥 AGAINST 的「隐形第四套机制」论点——「请执行下一步」是新的 Broker 调用而非 Bus 请求：AGAINST 声称「事件只能表达事实」会迫使异步协作另造 durable command queue。这是对 BoenMind 编排模型的误读：§9 的四种交互语义中，Agent 协作步骤之间的「请执行下一步」就是一次新的 Capability Call，走 Broker 入口，由 Orchestrator Task 板驱动（§17 行1549）。Bus 只传播「上一步已完成、结果是什么」这个事实，Orchestrator 或订阅该事实的 Agent 决定是否发起新的 Broker 调用。这恰恰是事件驱动架构的标准模式——Kafka Streams 的处理拓扑中，stream processor 消费已发生事件、产生新事件、副作用通过 sink 写出，流程控制靠事件驱动而非靠向事件流发送命令。AGAINST 的「折返 Broker 增加往返」批评忽略了一个事实：在单进程同线程（或同线程池）内，一次 Broker 调用就是一次函数调用加 O(1) 查表，往返成本是纳秒级而非网络级。"
  ],
  "evidence": [
    "基线 §12.1（行1108-1117）：Runtime Core 进程包含 Registry/Broker/Bus 三者，同进程消除了 AGAINST 的分布式 TOCTOU 前提。",
    "基线 §6.4（行604）：Provider Binding 结构含 generation 字段，为 Broker 授权决策点的快照提供不可变锚点。",
    "基线 §7（行625-636）：七步管线的逻辑拆分——关键点是「逻辑拆分」不等于「运行时串行」，预编译为 O(1) 查表是实现层的必然选择（seL4/Mojo 先例）。",
    "基线 §7（行681）：correlation 字段与 operation_id 对应可恢复执行状态——这是 outcome_unknown 恢复路径的机制锚点，AGAINST 的「事务协调器」方案若绕过此字段则无法保证恢复一致性。",
    "基线 §8（行696/728-739）：事件=已发生事实 + 持久事实源 + 内存分发 + event_seq 全局单调——生产者集合仅限于 Broker 调用完成点，远小于 Kubernetes 的任意控制器。",
    "基线 §12.2（行1119-1123）：L2 单写者追加 + 瞬态事件不落盘（行736）——使持久日志写入路径可控、可审计。",
    "OSGi ServiceReference：resolve 时快照服务引用，直到 release 前不随注册表变化——单进程内消除 TOCTOU 的直接先例。",
    "seL4 capability 查表：每调用 O(1) capability 检查，内核做缓存优化但从不允许绕过检查的 IPC——预编译查表+零旁路的同构先例。",
    "Kafka Streams 处理拓扑：消费事实事件→本地决策→产生新事件/新调用，流程控制靠事件驱动而非向事件流发命令——对 AGAINST「隐形第四套机制」的直接反例。",
    "D-Bus 高吞吐路径：daemon 做名字注册和策略检查后建立受约束直连，数据面走 Unix domain socket——证明「统一授权入口+直连数据面」可在不违反零特权通道前提下实现，驳斥 AGAINST 的专用通道主张。",
    "Chromium Mojo IPC：browser process 做权限裁决后允许渲染进程通过共享缓冲区传递大块数据，但所有 channel 建立必须经 browser process 授权——与 BoenMind 的「Broker 授权后直连」方案同构。"
  ],
  "conceded_risks": [
    "高频瞬态流（模型 token 增量、UI 打字机帧）与低风险确定性调用会被三层间接拉高延迟与复杂度，若不做 §8 行736 的瞬态事件不落盘和进程内快路径，单用户场景可能过度设计。（R1 未变）",
    "四种交互语义（§9）的边界在实践中易被侵蚀：把 Bus 当 RPC 通道、把审批/Task 消息混入事件流，会使「事实非请求」退化，需要持续的架构守护与测试纪律。（R1 未变）",
    "Broker 单入口同时是性能与可用性的集中点：Broker 自身故障即一切跨域调用不可用，必须由 L0 Supervisor 与进程内降级路径兜底，加重了「内核最小机制」的实现负担。（R1 未变）",
    "（新增/接受 EMPIRICAL 修正）Broker 七步管线若实现为运行时逐条求值而非预编译 O(1) 查表，将成为队头阻塞点，这必须在 M4 用压测证伪，被证伪的是实现方案而非三权分立本身。",
    "（新增/接受 EMPIRICAL 修正）持久事件日志作为事实源的正确性完全押在所有生产者遵守「事件=已发生事实」纪律上，必须由 Bus 层事件 schema 机器校验（拒绝含命令语义的事件）作为工程强制力，否则回放会放大错误。",
    "（新增/接受 EMPIRICAL 修正）「绝对零特权通道」的严格程度超出所有先例（Erlang/OTP 无访问控制、k8s 为 kubelet 保留第二授权路径、VS Code 扩展 RPC 无每调用鉴权），其 p99 开销代价需要在 M4 压测中量化，若超出单用户可接受阈值则需要重新评估。"
  ]
}
```

### 【AGAINST-R2】

```json
{
  "conceded_risks": [
    "接受 FOR 关于“统一策略入口”是安全基线的论证：若 Agent、GUI、Timer 各自鉴权，提示注入和权限漂移确实会制造不可见旁路；基线 §4.5、§7 第 638-650 行要求 input_trust 与调用方统一处理，这一点不能退回到调用方自律。",
    "接受 EMPIRICAL 关于职责分离不等于物理分进程：§12.1 第 1108-1123 行把 Registry、Broker、Bus 放在 Runtime Core 内是可行的，且 Chromium Mojo 的 capability 查表说明授权检查本身未必造成不可接受的微秒级开销。我的修正是反对“所有数据与控制流都必须经同一通用 Broker”，不是反对三种协议角色或统一策略面。",
    "仍承认单用户单设备降低了分布式共识、网络分区和部署成本；但这只降低实现成本，不能消除单进程锁竞争、持久化尾延迟和外部副作用不可逆造成的可靠性问题。",
    "仍承认 Bus 的事实语义对防止恢复时把请求重放成副作用很重要；问题在于它不能单独承担可靠命令投递、外部收据和业务状态提交的事务协调。"
  ],
  "evidence": [
    "对 FOR“热替换只需 Registry 原子切 binding”的回应：原子切换只解决未来查找，不解决已经通过 Broker 授权、正在等待 Provider 结果的调用。基线 §13.1 第 1172-1186 行若未把 binding generation 固化进授权票据和审计记录，A 经授权后切到 B 仍会产生授权对象与执行对象不一致；因此 Registry 原子性不是端到端一致性的充分条件。可用测试证伪：在授权返回与 Provider 提交之间强制 generation 切换，检查执行 binding、审计 binding 和策略摘要是否始终相同。",
    "对 FOR/EMPIRICAL“kube-apiserver/Chromium Mojo 证明统一入口可负担”的回应：这些先例只证明控制请求的认证/准入可以集中，并不证明实时音频、模型 token 流、大文件传输、长连接取消和外部副作用也应逐消息走同一个持久化管线。EMPIRICAL 自己承认 Kubernetes 的 kubelet 有 NodeAuthorizer 专门通道，D-Bus 也保留 peer-to-peer 例外；这直接反驳 R1 中“没有任何特权通道”的绝对表述，而不是仅仅提示优化空间。",
    "具体故障剧本：Agent 通过 Broker 获得邮件 Provider A 的授权，A 已向 SMTP/外部 API 提交邮件，但 Broker 在写入结果审计和 Bus 事实事件前崩溃或 deadline 到期。Supervisor 恢复后依据 §13.3 第 1203-1219 行只能标记 outcome_unknown；若外部系统查询暂时失败，恢复器要么重试造成重复邮件，要么不重试造成任务永久停滞。即使 operation_id 和 idempotency_key 存在，外部 Provider 不支持按该 key 查询或已提交但未返回收据时，Broker 无法凭自身统一语义制造原子性。可证伪指标是“外部提交成功、审计/事实缺失”的比例以及恢复后重复副作用比例。",
    "对“Bus 持久事实源可由 event_seq 重建一切”的回应：event_seq 只能重建已追加的事件，不能补出追加前已经发生的外部事实，也不能让错误发布的 command-shaped event 变成事实。基线 §14.2 第 1437-1447 行要求 Bus 暂停时核心状态仍可提交，反而说明状态提交与事件追加存在独立失败窗口；若无事务性 outbox 或收据表，重放只能扩大审计盲区。",
    "隐藏成本是协议膨胀而非单次查表延迟：§7 第 667-681 行把 deadline、取消、重试、幂等和 resume_cursor 统一塞入调用 envelope，导致短调用、流式调用、长任务共享同一状态机。为了避免 Broker 队头阻塞，必然再引入流专用调度、租约、背压、配额、断点和死信；这些机制若仍隐藏在 Broker 内，Broker 变成上帝中间件，若移到旁路，又违反“无特权通道”并形成不可见审计盲区。",
    "更优替代方案：保留 Broker 作为不可绕过的“策略闸门”，先完成身份、scope、参数校验、风险审批，并签发绑定 immutable binding_generation、策略版本、operation_id、deadline 的短期 invocation lease；普通控制调用继续经 Broker。对音视频、token 流、大文件和 Provider 专属长任务，使用由该 lease 授权的 capability-specific 数据通道，通道建立与关闭、字节数/阶段、收据摘要和错误仍回写 Broker/Bus。副作用类 Provider 必须实现收据、查询和幂等协议；Runtime Core 用事务性 outbox/inbox 将“本地状态、审计、待发布事实”绑定，外部结果通过收据状态机处理 outcome_unknown，而不是由通用 Broker 重试猜测。该方案可用四组测试验证：旁路无 lease 必须拒绝；generation 变更不得影响已签发 lease；通道吞吐不受持久事件单写者 p99 牵制；崩溃注入下外部提交与收据状态最终可对账。"
  ],
  "key_arguments": [
    "已修正但不退让：我接受“统一 Broker 是安全策略入口、三权首先是合同角色而非必须分进程”，但驳斥“因此所有跨域数据和控制动作都必须经过同一通用调用管线”。Mojo 的 O(1) capability 检查只覆盖授权查表，不能证明持久审计、取消恢复和大流传输应共享同一队列。",
    "R1 在外部副作用上失效：Broker 能统一决定“是否调用”，不能保证邮件、下单、文件写入等不可逆操作与审计、Bus 事件原子提交。具体崩溃窗口会制造 outcome_unknown、重复副作用或永久停滞；§13.3 已承认该边界，因此“统一入口”不能被宣传成统一可靠性。",
    "R1 的“零特权通道”与其自身目标冲突：为了支持 §9 的流式/长任务语义和 §14.2 的 Bus 暂停可用性，工程上必然需要专用数据面、租约或本地 Provider 协议。把它们全部包装进 Broker 会增加队头阻塞和测试矩阵，把它们放在 Broker 外又会产生未审计旁路；硬裁决两边都付成本。",
    "Registry 原子 binding 解决的是发现与未来路由，不是授权到执行的不可变关联。没有 binding_generation 固化在 invocation lease、审计和收据中，热替换可导致 A 被授权、B 实际执行，尤其当 Provider 的数据驻留、信任级别或副作用能力不同。",
    "Bus 的“事实而非请求”必须保留，但需要与 command queue、outbox/inbox、外部收据状态机明确分工；否则可靠异步协作会另造隐形第四套机制，或者错误事实被持久重放，二者都削弱审计可信度。",
    "替代裁决应是“统一策略入口 + 受 lease 约束的专用通道 + 持久 outbox/收据协调”，而非无条件三权绝对隔离。这样保留 FOR 所说的无旁路可审计性质，同时把延迟、流控和副作用恢复交给适合其故障模型的机制。"
  ],
  "role": "AGAINST-R2",
  "stance": "已修正：统一 Broker 应保留为不可绕过的策略与审计闸门，但 R1 的“所有跨域操作无任何特权通道”过强；它在外部副作用、实时数据面和 binding 热替换窗口中会失效，应改为 lease 授权的专用通道与 outbox/收据协调。"
}
```

### 【EMPIRICAL-R2】

```json
{
  "conceded_risks": [
    "TOCTOU 与审计断链风险: 若 Broker 鉴权与 Provider 执行未绑定不可变 Generation Token(§13.1,1172-1186 行),在 Draining 窗口内的并发调用可能出现「按 A 鉴权、由 B 执行」的记录漂移,必须在 M0/M1 协议层将 binding_epoch 纳入调用凭证。",
    "非原子副作用与 Outbox 缺口: 外部副作用(发邮件/扣费)与持久事件日志无法做到单机 2PC;Broker 超时并不能阻止 Provider 已发起的副作用,§13.3 outcome_unknown 是工程妥协而非解决,恢复链路仍需依赖每个 Provider 的幂等收据与主动对账协议。",
    "纯事件总线表达异步控制流的二次折返开销: 严格禁止 Durable Command Queue 迫使异步步骤通过「事件→Orchestrator 投影→再发起 Broker 调用」回旋,在长任务编排中放大了往返延迟与状态机复杂度。"
  ],
  "evidence": [
    "Chromium Mojo / seL4: Mojo 证明控制面(Broker 授予 Pipe/Endpoint)与数据面(直接在共享内存/IPC 通道传输音视频与大流)物理分立,每调用检查为 O(1) 查表(<1µs),实证了「统一入口管控」并不等同于「数据流穿透中间件」。",
    "Kubernetes / Kafka Outbox Pattern: k8s 依靠 status/generation 与 level-triggered 对账避免了对分布式事件原子性的妄想;分布式事务先例证明本地事务+Outbox 表追加事实日志是解决「业务提交与事件发布非原子」的唯一可靠解法。",
    "OSGi Bundle Lifecycle 与 D-Bus Name Owner: OSGi 在服务解绑时引入 ServiceTracker 版本租约与引用计数,D-Bus 引入 Unique Name (:1.xx) 绑定以防名字切换引发 TOCTOU,证明动态绑定必须有 Generation/Epoch 隔离机制。",
    "Erlang/OTP & Linux io_uring: 证明内核/控制面负责建立资源描述符与能力边界,而高频 I/O 靠环形缓冲区或轻量 actor 进程直连,不存在集中式持久化单写者拖垮非持久流的必然性(对应 §8.3 瞬态流不落盘)。",
    "基线对应: §6.4(606-619 行双层注册表)、§7(625-681 行调用管线与 correlation 字段)、§8.3(728-739 行持久与内存两级总线)、§13.1-§13.3(1172-1218 行热换/崩溃/副作用核验)、§17(1535-1556 行)。"
  ],
  "key_arguments": [
    "接受 AGAINST 关于 TOCTOU 的质证并修正控制流实现要求: 动态 Provider 切换(§13.1)与 Broker 鉴权(§7)之间确实存在时间差风险。借鉴 OSGi 与 D-Bus 经验,Broker 查表后必须向调用上下文注入不可变的 `binding_epoch` 与 `provider_instance_id`,Provider 消费时校验 epoch 不匹配直接重试,确保鉴权、审计与执行对象三者强一致,消除鉴权后换实现的漏洞。",
    "驳斥 AGAINST「Broker 必然沦为流式与实时数据面瓶颈」的推论: AGAINST 混淆了控制面(Broker)与数据面(Pipe/Stream)。Chromium Mojo 和 D-Bus 的先例均表明:Broker 仅负责鉴权、路由决策并协商出直接数据通道(或内存 Channel 描述符),瞬态 token 流与音视频流直接走进程间快路径且不落盘(§8.3,736 行),无需每一帧穿透 Broker 完整七步管线与 L2 持久写者。",
    "驳斥 FOR 关于「三权分立天然解决第二事实源漂移」的过度乐观: 单靠概念分立无法防止投影漂移。Kubernetes 依靠 Level-triggered 对账与 ResourceVersion,Kafka 依靠严格 Commit Log 序。BoenMind 要使「任务板是投影而非事实源」(§17,1549 行)成立,必须依赖 Bus 层的严格 Schema 机器校验与全局 `event_seq` 单调序(§8.3,739 行),否则任何生产者伪造事件或乱序都将导致投影状态不可逆损坏。",
    "接受 AGAINST 指出的副作用非原子性本质,厘清 Broker 的边界: Broker 的超时与重试无法提供跨进程或外部系统的分布式事务。实证表明单靠 Broker 无法消除 `outcome_unknown`(§13.3);必须将 Broker 定位为「幂等性协调与调用拦截器」,真正的副作用恢复必须依托 Provider 本地收据(Local Receipt)及 Outbox 机制,不能把业务一致性幻觉寄托于单一 Broker 容器。",
    "坚持 R1 职责三分是单用户长期自治系统的最小必要架构: 结合 FOR 的论据,单设备进程内 Rust + SQLite 使得分立的跨层开销压至纳秒/微秒级,而面对 Agent 长期运行必经的休眠、热更新与崩溃恢复,Registry 锁定状态、Broker 拦截注入、Bus 固化历史序列,比无中心网状拓扑(如纯 OTP 进程拓扑)具备压倒性的可调试、可审计与可恢复优势。"
  ],
  "role": "EMPIRICAL-R2",
  "stance": "(已修正) R1 的三权分立在单机架构下是兼顾插件热替换与可恢复性的已被验证的合理抽象,但必须明确划分「Broker 控制面鉴权」与「专用数据面直连」以防流拥塞,并通过引入不可变 Binding Epoch 消除热替换 TOCTOU 漏洞、通过 Provider 收据机制兜底副作用非原子性。"
}
```

## 合成裁决

```json
{
  "adr_points": [
    "确认 Registry/Broker/Bus 三权分立为 Runtime Core 的常驻结构:Registry 持有「谁提供什么」(统一注册中心按对象类型分表,持久逻辑目录+可丢失运行时缓存两层),Broker 持有「能不能调用以及调用谁」,Bus 持有「发生了什么、进度如何、异步消息如何传播」(持久事实源+内存分发层);三者为同一进程内的协议角色分立,不要求分进程部署。",
    "所有跨域调用(Agent/前端按钮/语音/自动化规则/Timer/Butler/其他 App)必须经 Broker 统一入口;裁定「没有任何特权通道」的准确含义为:不存在未经 Broker 授权建立、或脱离审计关联的调用与数据通道;由 Broker 授权后建立、且生命周期与收据摘要回写 Broker/Bus 的受约束数据面快路径不构成特权通道,与任何已授权调用无关的独立数据传输仍一律禁止。",
    "Broker 在授权决策点从 Registry 取 binding 并固化不可变 binding_epoch 与 provider_instance_id,写入调用凭证与审计记录并由 Provider 侧校验,不匹配即拒绝或重试;热替换(§13.1 draining→handshake→原子切 binding)只影响后续调用,不得改变在途调用的授权-执行-审计一致性;须对 §6.4 Provider Binding 结构与 §7 调用关联字段做相应字段增补。",
    "Broker 七步管线是逻辑职责拆分而非运行时串行管线:实现必须把策略预编译为按「调用方×目标 Capability」的 O(1) capability 查表(同 seL4/Mojo 先例),禁止逐条策略求值进入热路径;M4 里程碑以 p99 压测与队头阻塞注入测试证伪,超标即回炉实现方案。",
    "高频瞬态流(模型 token 增量、UI 打字机帧、音视频、大文件传输)走 Broker 授权的数据面快路径:不逐帧通过七步管线、不落盘(§8 行736)、不占用 L2 持久单写者;持久事实只在 Broker 状态提交点产生。",
    "Bus 事件严格表达已发生的事实,禁止命令语义;Bus 层实施事件 schema 机器校验并在持久化前拒绝违规事件,持久事件生产者集合限于 Broker 状态提交点;event_seq 全局单调,所有投影、消费位点与 resume cursor 仅由持久日志重建;Orchestrator 任务板只是投影,不是第二事实源。",
    "异步协作的「请执行下一步」是一次新的 Broker Capability Call,由 Orchestrator 或订阅事实的一方发起;禁止在 Bus 之外另建 durable command queue 等绕过统一入口的隐形机制,事件流不得被当作 RPC 通道使用。",
    "外部副作用的可靠性不依赖调用原子性:副作用类 Provider 的 Capability 合同强制实现收据、按 operation_id/idempotency_key 查询与幂等重放防护;Runtime Core 以事务性 outbox 绑定本地状态提交、审计写入与事实事件的发布顺序;outcome_unknown 一律先核验外部系统收据再决定重试(§13.3);统一入口承诺统一策略与统一审计,不承诺统一副作用原子性。"
  ],
  "conditions": [
    "Broker 策略必须预编译为「调用方×目标 Capability」的 O(1) capability 查表,七步管线只允许作为逻辑拆分存在;此项列为 M4 Broker 实现的验收项,证伪判据:统一入口 p99 开销、队头阻塞注入、Broker 故障半径测试,超标即回炉实现方案并重审本裁决的运行前提(被证伪的是实现而非三权分立本身)。",
    "基线须新增 binding_epoch + provider_instance_id 机制(修订 §6.4 Provider Binding 结构与 §7 调用关联字段):Broker 在授权决策点固化 epoch,注入调用凭证与审计记录,Provider 侧校验不匹配即拒绝/重试;测试:在授权返回与 Provider 提交之间强制切换 binding,验证执行对象、审计对象与策略摘要三方一致;Runtime generation 变更不得改变已签发在途调用的授权与审计归属。(注:FOR-R2 称 §6.4 行604 已含 generation 字段经核为误引,基线现文本仅含 Runtime Core 代际,故此项为必做增补而非既有性质。)",
    "Bus 层实施事件 schema 机器校验:含命令语义的事件在持久化前拒绝并告警;持久事件生产者白名单限于 Broker 状态提交点;在「生产者伪造/乱序发布」故障注入下,投影必须可重建或异常可检出,否则事件溯源前提不成立。",
    "数据面分流合规四测试(采纳 AGAINST-R2 测试组,作为「授权后快路径」的准入门槛):① 无 Broker 签发授权(lease:binding_epoch+策略版本+operation_id+deadline)的通道建立必须被拒绝;② epoch/generation 变更不得影响已授权通道的审计归属;③ 通道吞吐不得被 L2 持久单写者 p99 牵制;④ 崩溃注入下外部提交与收据摘要可最终对账,且通道字节数/阶段/收据摘要/错误全程回写 Broker/Bus。",
    "副作用类 Provider 合同强制:实现收据、按 operation_id/idempotency_key 查询与幂等重放防护;Runtime Core 以事务性 outbox 绑定「本地状态提交→审计写入→事实事件追加」的顺序;可证伪指标:「外部提交成功而审计/事实缺失」比例与恢复后重复副作用比例须设上限并纳入回归测试;通用 Broker 禁止以盲目重试猜测外部结果。",
    "降级路径落地(§14.2 行1444):Event Bus 暂停时核心状态仍可提交、订阅从事件位点补发;瞬态事件不落盘(行736)与持久写路径(L2 单写者)物理隔离,数据面吞吐不受持久化尾延迟牵制。",
    "架构守护 CI 常驻:Bus 被当 RPC 通道、审批/Task 命令混入事件流、混层缓存超出「可丢失运行时缓存」范畴(行612-616)三种侵蚀一经出现即测试失败;Broker 自身故障的兜底仅由 L0 Supervisor 重启/generation 切换承担(行1123),不得以增设特权降级通道的方式实现。"
  ],
  "consensus_points": [
    "三权分立是同一进程内的协议角色分立而非物理分进程(基线 §12.1 行1104-1123):Registry/Broker/Bus 同处 Runtime Core 与 D-Bus 单守护进程(名字注册+逐消息策略+信号路由共实现)、Kubernetes kube-apiserver(认证/RBAC/admission/审计内联)的已验证形态同构,有大量装机量先例;AGAINST-R2 明确接受「三权首先是合同角色而非必须分进程」。",
    "统一 Broker 入口是不可绕过的策略与审计强制点:七类调用方(Agent/按钮/语音/自动化规则/Timer/Butler/其他 App,§7 行638-650)共用同一入口,身份/权限/Task scope/参数校验/绑定/审计单点强制;任何绕过路径必然表现为审计缺口,input_trust 信任分级(行677/364/M4.8)只有在该单点才能被强制执行。AGAINST-R2 明确让步:「统一策略入口不能退回到调用方自律」。",
    "Registry 的单一「名字→实现」间接层+原子 binding 切换(§6.4 行594-619、§13.1 行1172-1186、§16 行1506)是插件热替换的机制前提:替换压缩为一次原子切换而调用方零感知;发现/路由逻辑散落到 Broker/Bus 或调用方内部会使热替换退化为分布式一致性问题并产生旧句柄悬空。两轮质证未被驳倒。",
    "授权-执行-审计一致性必须由不可变 binding epoch(或 generation)快照保证:Broker 在授权决策点固化 binding_epoch 与 provider_instance_id,注入调用凭证、写入审计并由 Provider 侧校验,不匹配即拒绝/重试;单进程形态下该机制足以消除热替换窗口的 TOCTOU(OSGi ServiceReference resolve 时快照、D-Bus Unique Name 先例),无需分布式事务。三方在 R2 收敛——但注意:经核验,基线 §6.4/§7 现文本并不含该字段(全文 generation 仅指 Runtime Core 代际,行1223-1268),FOR-R2 引注「§6.4 行604 已含 generation」为误引,该机制是须新增的基线扩展而非既有性质。",
    "Broker 七步管线(§7 行625-636)是逻辑职责拆分而非运行时串行管线:策略必须预编译为按「调用方×目标 Capability」的 O(1) capability 查表(seL4/Mojo 先例,每调用检查可低于微秒);若实现为逐条策略求值则形成队头阻塞,届时被证伪的是实现方案而非三权分立本身。FOR-R2 主动接受、EMPIRICAL 提出、AGAINST 的异议针对持久化管线而非查表本身,三方收敛。",
    "控制面与数据面分流:Broker 负责授权、绑定与审计关联;高吞吐瞬态流(模型 token 增量、UI 打字机帧、音视频、大文件)经 Broker 授权建立的受约束通道(内存引用/共享缓冲/直连描述符)传输,不逐帧穿透完整管线、不落盘(§8 行736)、不占用 L2 单写者(行739);通道的建立、生命周期、字节数/收据摘要与错误必须回写 Broker/Bus。三方 R2 收敛:FOR/EMPIRICAL 以 D-Bus(daemon 授权后 Unix socket 直连)与 Chromium Mojo(browser process 授权后共享内存)论证,AGAINST 以 invocation lease 提出机制同构方案。",
    "Bus「事件=已发生的事实而非请求」(§8 行696)语义保留:它是崩溃恢复(投影、消费位点、resume cursor 仅由全局单调 event_seq 重建,行739)与副作用安全(§13.3 行1203-1219 先核验外部系统再决定重试)的共同机制前提;异步协作的「请执行下一步」是一次新的 Broker Capability Call,由 Orchestrator 或订阅事实的一方发起(§17 行1549 任务板只是投影),不向 Bus 投递命令、不在 Broker 之外另建 durable command queue。AGAINST 的「隐形第四套机制」论点被 Kafka Streams 事件驱动拓扑反例驳回,其 R2 亦接受事实语义必须保留。",
    "事实纪律需要工程强制力而非生产者自觉:Bus 层事件 schema 机器校验(拒绝含命令语义的事件持久化)、持久事件生产者集合限于 Broker 状态提交点(行733-735)、叠加瞬态事件不落盘(行736)与 L2 单写者追加(行739)构成三道防线。EMPIRICAL 提出、FOR-R2 接受纳入立场、AGAINST 要求错误事实不得被持久重放,三方收敛。",
    "外部副作用不因统一入口而原子化:任何架构(含 AGAINST 提议的事务协调器)都无法回滚已到达外部系统的副作用;统一入口承诺统一策略与统一审计,不承诺统一副作用可靠性。副作用恢复依赖 Provider 收据/按 operation_id/idempotency_key 查询/幂等协议与 Runtime Core 事务性 outbox(绑定本地状态提交、审计写入、待发布事实),Broker 定位为幂等协调与调用拦截器,§13.3 outcome_unknown 是兜底路径而非充分机制。三方 R2 收敛(EMPIRICAL-R2 明确支持 AGAINST 的本质论点,FOR-R2 接受收据/outbox 为 §13.3 的实现机制)。",
    "场景适配判断成立:单用户单设备+进程内实现+本地持久化把三权分立的成本压到最低(无网络分区与共识问题),而长期自治 Agent 使崩溃、休眠唤醒、generation 切换成为常态事件而非异常,恢复路径必须常态可用;相较无中心网状拓扑(纯 OTP 进程拓扑),中心化裁决点在可调试、可审计与可恢复上占优。三方均持,未被驳倒。"
  ],
  "disputes": [
    "「没有任何特权通道」的字面效力未决:机制已收敛(授权后受约束直连+审计回写不构成旁路),但 FOR/EMPIRICAL 认为 R1 自身职责界定(Broker 管「能不能调用以及调用谁」、行623「Bus 只负责消息传输」、行685「总线不应取代所有 RPC」)已把载荷传输排除在 Broker 管线外,原文无须改动;AGAINST 坚持原文绝对化表述过强、与流式/长任务目标自相冲突,必须改写为「lease 授权的专用通道」。主席以释义方式裁决(见 adr_points),字面是否修订的异议如实保留。",
    "投影漂移是否需要第二道自愈机制未决:EMPIRICAL 指出 Kubernetes 拒绝把 Events 当事实源、控制器靠 level-triggered re-list/resync 对账收敛,持久事件日志即事实源是 R1 最大的经验风险点;FOR 主张单用户本地场景下事件溯源的可测试性优于对账循环的收敛性证明,以 schema 校验+event_seq+受限生产者集合为足够防线,明确反对引入对账循环;EMPIRICAL-R2 将要求收敛为 schema 校验+event_seq 但未再坚持对账、亦未正式撤回。留待 M2 事件层以「生产者伪造/乱序」故障注入证伪。",
    "统一调用 envelope 的字段边界未决:AGAINST 认为 §7 行667-681 将 deadline/取消/重试/幂等/resume_cursor 对所有调用强制,使短调用、流式调用、长任务共享同一状态机,叠加流专用调度、租约、背压、配额、死信后协议复杂度与测试矩阵快速膨胀;FOR 认为数据面分流后剩余差异只是 Broker 内部路由策略,不构成协议层问题。「关联字段是否按调用类别可选化/裁剪」在两轮内未形成一致结论。",
    "文档承诺边界与 Broker 单点故障半径的表述未决:三方均承认 Broker 自身故障即一切跨域调用不可用、同进程分权不是故障分权(§12.1 行1121 进程内组件不能独立崩溃和热替换),但对基线是否须在 Broker 职责定义处明示「统一入口≠统一可靠性」(FOR 认为行681 与 §13.3 已隐含该边界;AGAINST 要求显式写出可靠性由 Provider 收据协议承担),以及是否/如何为 Broker 增设进程内降级路径而不违反内核最小机制,仅达成「L0 Supervisor 兜底重启/generation 切换」的最低共识,降级路径的具体形态未决。"
  ],
  "ruling_id": "R1",
  "verdict": "uphold_with_conditions"
}
```
