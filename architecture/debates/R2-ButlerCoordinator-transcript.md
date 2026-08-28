# Zen Consensus 辩论记录:R2 Butler 只有协调权,Coordinator 是受限队长

辩论对象:§17 裁决 R2「Butler 只有协调权,Coordinator 是受限队长」

> 协议:Zen consensus——三个不同模型家族担任三方(FOR=架构师钢人论证,AGAINST=安全可靠性挑战,EMPIRICAL=真实系统实证裁决),两轮(独立立场→交叉质证),缺席与不可裁决的分歧如实记录。

角色分配:AGAINST=gpt-5.6-luna, EMPIRICAL=gemini-3.7-flash, FOR=glm-5-turbo

## 裁决原文

裁决:Butler App 是真实 App 但只拥有系统协调权(task.create/cancel、agent.spawn、team.create、task.collect 等),不默认拥有任何领域操作权(mail.read、stock.place_order 等);为每个 Task 创建的 Coordinator Agent 不复制 Butler 完整身份,其权限 = Butler 可授予的协调权 ∩ 当前 Task 授权 ∩ 用户授权;不能读取所有 App 私有数据、不能把成员权限无限转授、不能绕过 Broker、不能扩大 Task 预算。

## 第一轮:独立立场

### 【AGAINST】

```json
{
  "conceded_risks": [
    "若 Coordinator 默认继承 Butler 的领域权限，提示注入或恶意成员可能把一个跨域任务升级成邮件外发、交易下单等高影响操作。",
    "若允许成员权限无限转授，审计会难以确认最终执行者是否获得了用户实际批准的范围。",
    "绕过 Broker 会破坏参数校验、预算强制、取消/超时、幂等和统一收据，故不能以“内部调用”作为例外。"
  ],
  "evidence": [
    "基线 §3（第220-243行）要求每个 Agent 绑定所属 App、Task、能力授权、预算、截止时间、父子归因和记忆范围，并禁止 Agent 间私有通道；这意味着 Coordinator 若只能转发抽象结果，跨域数据的来源、用途和重试责任必须额外建模。",
    "基线 §7（第621-648行）规定 Butler、Agent、前端、Timer 和其他 App 共用 Broker；但第941行又把 capability.call 列入 Butler 默认协调权限，形成“协调权可触发领域调用、却不持有领域权”的双重主体语义，若没有不可伪造的委托凭证，Broker 很难区分用户授权、Butler 请求和 Coordinator 转授。",
    "基线 §9.7（第877-901行）明确预算同时挂在 Agent 和 Task 上且硬限会暂停 Task；在长链路中，Coordinator 不能扩大预算并不等于能避免预算竞争：一个成员重试消耗预算后，后续成员可能因共享预算耗尽而被阻断，协调器还缺少按步骤预留和补偿的机制。",
    "基线 §10.2（第958-992行）允许 Butler 直接调用公开 Capability，也允许请求领域 Agent；这在低风险查询上可行，但同一任务中“直接 Capability”和“领域 Agent”两条路径会产生不同的授权、数据最小化、收据和失败恢复语义，R2 没有规定二者如何统一。",
    "AWS STS 的 AssumeRole 会把身份策略与 session policy 做交集，OAuth 2.0 Token Exchange（RFC 8693）也支持受众、scope 和委托链；这些先例表明“交集”只有在凭证携带 audience、资源条件、过期时间、不可再委托标志并由资源方验证时才可操作，R2 的集合公式本身不是可执行的安全机制。",
    "Kubernetes Controller/RBAC 模式把控制器的协调身份与各工作负载的业务身份分开；现实中的控制器通常还需要按资源、命名空间和动作细分权限，否则协调器的 delete/patch 权会成为大范围故障或拒绝服务半径，说明仅按“协调权”分类过粗。"
  ],
  "key_arguments": [
    "R2 会把跨域工作流变成“协调器只能传递黑盒结果”的脆弱链路：当 Coordinator 无法按资源谓词读取原始证据、核对版本或执行补偿时，错误判断、重复副作用和 outcome_unknown 只能升级给用户，可靠性成本高于授予一次性受限领域委托。",
    "R2 的权限交集缺少可验证的委托载体，导致 Broker 面对 capability.call 时无法可靠绑定“哪一项用户批准、哪一个资源范围、哪一层转授、是否允许再转授”，结果要么过度拒绝造成不可用，要么把 Butler 变成隐藏的混淆代理。",
    "将 pause/resume/stop/cancel/team.create 等高影响控制能力统一视为普通协调权会扩大故障半径：被提示注入或失控的 Coordinator 可以批量取消合法任务、暂停关键 Agent 或制造代理风暴，即使它完全没有 mail.read 或 stock.place_order。",
    "双路径设计会制造可观测性和恢复分裂：同一 mail.search 可能由 Butler 直接调用或由 Mail Agent 代调用，二者的延迟、缓存、数据脱敏、重试幂等和收据归因不同，审计者难以判断最终输出究竟基于哪一份数据。",
    "严格受限的 Coordinator 会迫使每个 App 预先把所有跨域协作所需的中间数据和补偿接口写进 Task 授权，增加授权配置、版本迁移和回放测试复杂度，并把简单的本地自动化变成多次 Broker/IPC/模型往返。"
  ],
  "role": "AGAINST",
  "stance": "R2 的最小权限方向正确，但把“协调权”和“领域权”做成静态集合交集且缺少可验证的委托与资源约束，会在跨域补偿、控制面安全和故障恢复上付出过高代价。"
}
```

### 【EMPIRICAL】

```json
{
  "conceded_risks": [
    "动态提权阻断带来的高频人机交互摩擦：当复杂长链任务执行中动态发现依赖未预先授予的领域权限时，Coordinator 无法自主扩大授权，必须中断流转触发 approval_required 等待用户二次审批，牺牲了全自动规划的连贯性。",
    "数据穿透受限导致的信息中继开销与语义折损：Coordinator 无法直接窥探各 App 私有数据，必须完全依赖 Domain Agent 的局部推理与结构化汇总，在多代理深度串联时会增加上下文打包传递的 Token 开销与潜在信息丢失。"
  ],
  "evidence": [
    "Erlang/OTP 设计规范：supervisor 模块仅拥有子进程启动、终止与重启决策权，严格禁止直接读取或修改 worker 进程内部字典与业务邮箱数据。",
    "Kubernetes 控制器与 RBAC 体系：kube-controller-manager 采用细粒度分权，Pod 仅注入限定命名空间的 ServiceAccount Token，无法向外无限继承或转授 ClusterRole 特权。",
    "Linux cgroups v2 / systemd 切片模型：子 cgroup 继承并受制于父节点资源限额（如 cpu.max、memory.max），天然禁止子进程私自扩张父级分配的预算配额。",
    "Chromium 多进程沙箱与 Mojo IPC：Browser 进程作为中央 Broker 仲裁能力分发，Renderer 与 Worker 子进程无法绕过 IPC 代理直接访问宿主系统或其他渲染上下文的私有存储。",
    "BoenMind 架构基线：§10.1 (第 927-957 行) 明确 Butler 仅有系统协调权而不默认拥有领域操作权；§11.2-§11.3 (第 1040-1055 行) 规定 Coordinator 权限为三方权限交集且禁止无限转授；§9.7 (第 900 行) 强制锁定 Coordinator 无法自扩任务预算。"
  ],
  "key_arguments": [
    "控制面与数据面正交分离是分布式与操作系统防特权提权的经典基石：Butler 仅持系统协调权而剥离领域数据访问权，直接对应 Erlang Supervisor 与 K8s Controller 的纯控制面设计，防止单点妥协引发全域数据沦陷。",
    "基于权限交集的能力衰减杜绝了 Confused Deputy 代理人困境：Coordinator 权限严格计算为 Butler 协调权 ∩ Task 授权 ∩ 用户授权（§11.3），遵循了 K8s ServiceAccount 与 OSGi Bundle 的最小特权派生原则。",
    "层次化不可自扩预算确保了不可信规划实体的资源安全收敛：禁止 Coordinator 调整任务预算配额（§9.7, §11.2）完全复现了 Linux cgroups/systemd 的配额继承单调递减律，杜绝 LLM 死循环导致的计算资源耗尽。",
    "强制 Broker 仲裁消除了绕过审计的隐式侧信道：所有能力与数据交互必须经由统一 Broker 鉴权，复用了 Chromium Mojo 沙箱的代理过滤机制，保障了 Append-only Event Log 的审计完整性。"
  ],
  "role": "EMPIRICAL",
  "stance": "裁决 R2 属于已被 Erlang/OTP、Kubernetes、Linux cgroups 及 Chromium 多进程沙箱充分验证的“控制面与数据面解耦 + 最小特权委托 + 层次配额约束”成熟最佳实践，有效根绝了 AI 协调中枢的单点特权膨胀与越权代理风险。"
}
```

### 【FOR】

```json
{
  "role": "FOR",
  "stance": "Butler 仅持协调权、Coordinator 按任务最小权限委托，是单用户本地优先 AI Runtime 实现崩溃可恢复、插件热替换与阶段一可交付性的唯一正确权限边界设计。",
  "key_arguments": [
    "协调权与领域操作权的强制分离使得 Broker（§2 核心对象边界）成为唯一的能力仲裁点，任何 Agent 对 App 私有数据的访问必须经 Broker 路由——这保证了即使 Coordinator 崩溃或被恶意 Prompt 注入，攻击面被严格限制在当前 Task 授权范围内，无法横向移动到其他 App 的数据，这是崩溃可恢复架构的安全基线。",
    "Coordinator 的权限 = Butler 协调权 ∩ Task 授权 ∩ 用户授权这一三重交集机制，使得每个 Task 的生命周期可被独立序列化到 WAL/Snapshot（§11），恢复时只需重放该 Task 的授权边界即可完整重建 Coordinator 状态，无需恢复 Butler 全局状态——这正是 Erlang/OTP supervisor tree 中 'let it crash' 策略在 AI Agent 层面的直接映射。",
    "Butler 不复制完整身份到 Coordinator，意味着 Coordinator 是一次性的、可丢弃的任务执行上下文，这使插件热替换成为可能：卸载一个 App 只需使依赖它的 Coordinator 失效并重建，不影响 Butler 本体或其他无关 Task——与 OSGi bundle 生命周期和 VS Code Extension Host 隔离模型同构。",
    "禁止 Coordinator 无限转授成员权限并限制 Task 预算，防止了自治 Agent 系统中最危险的权限提升路径（privilege escalation via delegation chain），这一机制在 systemd 的 capability bounding set 和 Kubernetes RBAC 的 'no wildcard escalation' 规则中有完全对应的先例。",
    "阶段一作为单体跨平台软件交付时，这套最小权限模型将 Butler 的实现复杂度从'全能调度器'降维为'纯协调进程'，Coordinator 降维为'有界状态机'，两者均可由确定性状态机驱动而非通用推理引擎，直接降低阶段一的工程风险和交付周期。"
  ],
  "evidence": [
    "§2 核心对象边界（行号未读但用户指明为相关基线）：Broker 作为唯一能力仲裁点，是协调权与领域操作权分离的架构基础。",
    "§10 Butler App（用户指明为相关基线）：Butler 被定义为拥有系统协调权而非领域操作权的 App，R2 裁决是其精确的形式化表述。",
    "§11 Agent Team 与 Coordinator Agent（用户指明为相关基线）：Coordinator 的生命周期、权限边界与 Task 授权的交集模型在此节定义。",
    "§17 当前架构裁决（第 1516-1557 行）：R2 裁决原文所在位置。",
    "Erlang/OTP Supervisor Tree：supervisor 只拥有 restart/stop 子进程的协调权，不复制自身行为到 worker；worker 崩溃后 supervisor 按策略重建，与 Butler→Coordinator 的关系同构。",
    "Kubernetes RBAC：Controller Manager（类比 Butler）只有编排权限，Pod（类比 Coordinator）通过 ServiceAccount 获得最小 API 权限，禁止 cluster-admin 权限自动传递到 Pod。",
    "VS Code Extension Host：主进程（类比 Butler）只做 UI 协调和进程管理，每个扩展在隔离的 Extension Host 中以受限 API 运行，不能直接访问其他扩展的状态。",
    "OSGi Bundle 生命周期：Framework（类比 Butler）只管理 install/start/stop/resolve，Bundle（类比 App）的服务注册与发现通过 Service Registry（类比 Broker），Bundle 之间不直接持有对方引用。",
    "systemd Capability Bounding Set：服务单元只能获得显式声明的 Linux capability，即使以 root 运行也不能突破 bounding set——与 Coordinator 不能突破 Task 预算和授权边界的机制完全对应。",
    "Chromium Site Isolation / Process Model：Browser 进程（类比 Butler）只做导航和资源分配，Renderer 进程（类比 Coordinator）被限制在特定 origin 内，通过 IPC（类比 Broker）请求跨域资源，不能直接读取其他 Renderer 的内存。"
  ],
  "conceded_risks": [
    "三重权限交集（Butler 协调权 ∩ Task 授权 ∩ 用户授权）在阶段一实现时，授权计算逻辑本身可能成为性能瓶颈和正确性陷阱，尤其是 Task 授权的动态变更（如用户中途撤回某项权限）需要 Coordinator 实时响应，状态同步的时序错误可能导致权限越界或合法操作被误拒。",
    "Coordinator 作为一次性上下文无法继承 Butler 的长期记忆和用户偏好模型，对于需要跨 Task 累积上下文的场景（如'帮我管理一周邮件'这类持续性任务），每次 Task 创建都重建 Coordinator 会导致上下文断裂，必须在 Butler 层或独立 Memory Service 中额外设计跨 Task 上下文传递机制。",
    "禁止无限转授在复杂多步工作流中可能导致'权限不足死锁'——例如 Coordinator 需要一个尚未被用户显式授权的 App 能力来完成 Task 中间步骤，但无法自行获取该授权，只能暂停并回退给用户确认，这会降低自治 Agent 的流畅度和用户体验。"
  ]
}
```

## 第二轮:交叉质证

### 【AGAINST-R2】

```json
{
  "conceded_risks": [
    "接受 FOR 关于最小权限、Broker 统一仲裁和不可无限转授的核心判断：若 Coordinator 继承 Butler 的领域权限，提示注入或恶意成员确实可能把低风险任务升级为邮件外发、交易下单等高影响操作；这一风险不能通过“本地单用户”假设消除。",
    "接受 EMPIRICAL 关于控制面与数据面分离的方向性证据，但修正其结论：Erlang Supervisor、Kubernetes Controller 和 Chromium IPC 都不是“仅有粗粒度协调权”即可安全运行，而是依赖资源范围、动作类型、受众、令牌生命周期和不可再委托等细粒度约束；这些先例支持分离原则，不足以证明 R2 当前合同已经可执行。",
    "接受 FOR 关于 Coordinator 可丢弃性有利于崩溃恢复和插件替换，但该收益只成立于 Task 的授权、未完成操作、幂等键和领域结果都已持久化；否则重建会造成重复副作用或无法判断 outcome_unknown，而不是自动获得 Erlang 式的可恢复性。",
    "仍承认严格禁止 Coordinator 扩大预算和绕过 Broker 是必要边界；问题在于 R2 没有定义预算预留、重试归因、取消传播和结果确认机制，因此边界本身不能解决预算竞争和恢复问题。"
  ],
  "evidence": [
    "基线 §10.1 第927-956行把 capability.call 列入 Butler 默认协调权限，同时第944-951行禁止 Butler 默认拥有 mail.read、mail.send、stock.place_order 等领域权限。若 capability.call 的请求凭证不携带具体能力、资源谓词、用户批准引用和不可再委托属性，Broker 无法仅凭“协调权”判断一次调用究竟是合法委托还是混淆代理；这不是集合交集的数学问题，而是缺失可验证凭证的问题。",
    "基线 §10.2 第958-992行明确允许 Butler 直接走公开 Capability，也允许请求 Domain Agent。两条路径对同一领域动作可能产生不同的授权主体、脱敏边界、重试策略和收据归因；例如 mail.search 直接调用返回原始结构化结果，而 Mail Agent 可能只返回摘要。R2 没有规定统一的授权记录、数据最小化和幂等合同，形成可审计但不可比较的双路径。",
    "基线 §11.2-§11.3 第1040-1067行只规定 Coordinator 权限的三方交集以及不能无限转授，却没有规定交集如何编码为不可伪造令牌、如何绑定 audience/resource/action、如何表达过期和撤回、如何禁止二次委托，也没有规定 Domain Worker 代执行时如何保留原始用户授权链。因此“∩”不是可直接验证的安全机制。",
    "基线 §9.7 第877-901行将预算同时绑定 Agent 和 Task，并规定硬限触发暂停。故障场景：用户授权一个“整理并回复邮件”的 Task，Coordinator 被注入后让 Mail Agent 对同一页邮件反复重试；重试先耗尽共享 Task 预算，随后合法的发送步骤被暂停。Coordinator 没有扩大预算并不能防止拒绝服务，且若发送请求已到达 Provider 但回执丢失，恢复时无法仅凭 Task 状态判断是否重复发送。",
    "基线 §3 第220-243行要求 Agent 绑定 App、Task、能力授权、预算、截止时间、父子归因和记忆范围，并禁止 Agent 间私有通道。具体失效剧本：Coordinator 持有 task.cancel、agent.stop 和 task.collect 等控制权；提示注入来自某个 Mail Artifact，诱使其批量 stop 其他合法成员并 collect 尚未完成的报告。即使没有 mail.send 权限，它仍可造成跨任务拒绝服务或证据截断，说明“协调权”本身也需要任务、成员和资源范围的细分。",
    "基线 §17 第1535-1556行同时规定 Broker 决定“能不能调用以及调用谁”、Agent 只在需要理解和规划时介入、低风险确定性能力直接走 Capability Broker。若把 capability.call 作为 Coordinator 的泛化协调能力，却不要求每次调用固定到已批准的能力清单和风险等级，就会让 Agent 绕过“低风险确定性能力直接调用”的可预测路径，把领域判断隐藏在协调请求中，削弱 Broker 的可验证性。",
    "更优替代是保留 R2 的控制面/领域面分离，但把静态三方交集改为 Broker 签发的、不可伪造且不可再委托的 Task Capability Grant：每次 grant 绑定 task_id、coordinator_id、audience/app_id、action、资源谓词、参数约束、风险等级、预算预留、过期时间、撤回版本和父授权哈希；Coordinator 只能提交 grant，领域 Provider 必须验证 grant 和幂等键。高风险动作仍进入 approval_required；低风险动作可批量预授权。task.cancel、agent.stop、team.create 等控制能力也按当前 Task、成员集合和调用者角色细分，并统一直接 Capability 与 Domain Agent 的收据和授权链。"
  ],
  "key_arguments": [
    "驳斥 FOR“权限交集即可保证安全且可恢复”：交集没有凭证语义就无法由 Broker 证明授权来源、受众和资源范围；而没有幂等键、操作阶段和回执持久化，Coordinator 重建仍可能重复发送邮件或把未知结果误判为未执行。可证伪条件是：对同一授权做两次等价请求，系统必须在日志中证明第二次被幂等抑制；R2 当前条文没有此要求。",
    "驳斥 EMPIRICAL“控制器先例已充分验证 R2”：Kubernetes 的 ServiceAccount 不是泛化 controller 权限，cgroups 只约束资源配额，Chromium IPC 也依赖 origin/句柄级校验；这些机制分别补上了 R2 缺失的资源、受众和句柄约束，不能拿来证明 task.cancel、capability.call 这类粗粒度权限安全。",
    "驳斥“只要不能领域读写，协调危害就局部化”：Coordinator 的 pause/resume/stop/cancel/team.create 可制造任务级拒绝服务、代理风暴和审计截断。故障场景中，来自 Mail Artifact 的提示注入批量停止其他成员，系统不发生领域越权，却仍破坏合法任务；因此控制面权限也必须按 Task、成员和数量限额授权。",
    "接受并保留最小权限方向，但修正 R2 的实现：采用不可再委托 Task Capability Grant，把 action、audience、资源谓词、参数约束、审批引用、预算预留、过期/撤回和父授权哈希放入 Broker 签发凭证；Coordinator 只持有当前 Task 的 grant，Provider 负责最终验证。这样保留 Butler 与 Coordinator 的身份隔离，同时让“交集”成为可测试机制。",
    "R2 与基线 §10.2 的双路径共存存在一致性成本：直接 Capability 和 Domain Agent 若不共享同一 grant、幂等、脱敏和收据合同，同一 mail.search 会出现不同数据版本与归因，审计只能看到“谁调用了”而无法重建“基于哪份授权和哪份证据决定”。阶段一为此需额外实现授权链、结果摘要/来源和回执状态机，隐藏成本高于 R2 所声称的简单有界状态机。"
  ],
  "role": "AGAINST-R2",
  "stance": "已修正：我接受控制面与领域面分离及最小权限边界，但反对将粗粒度协调权与不可执行的静态权限交集作为完整裁决；R2 必须补充资源化、不可再委托的授权凭证以及幂等、预算预留和控制面限额，否则在恢复、审计和拒绝服务场景中失效。"
}
```

### 【EMPIRICAL-R2】

```json
{
  "conceded_risks": [
    "协调动词作用域未明文化是当前最大规范缺口：在把「协调动词按 Task 子树裁剪」写进 §11.2 之前，被注入的 Coordinator 理论上可对子树外 Task 施放 task.cancel/agent.stop，该风险不因其不持领域权而消失（AGAINST 的 A3 成立部分）。",
    "数据盲协调面的中继 Token 开销与语义折损维持第一轮让步：K8s 控制器可读全量声明式状态，而 BoenMind Coordinator 只能依赖结构化汇总，跨域深度链路的错误放大只能靠 verification 钩子事后对账缓解，无法事前消除。",
    "「成员角色授权」的签发者与转授深度标志未定义（§11.3 第 1060-1067 行）：规范补齐前，§11.2 第 1044 行「不能把成员权限无限转授」是愿望而非机制。",
    "审批中断的吞吐代价不可消除：即使以 task:<id> 作用域一次批准（第 868 行）覆盖后续同类调用，首次发现未授权依赖仍需人机往返；用户离线时任务停在 waiting_approval（第 874 行），单设备本地系统没有代答机制。",
    "把 LLM 放进控制面无工程先例：所有被引先例的控制面均为确定性代码，R2 的补偿控制（交集/预算/默认拒绝）是对非确定性的围堵而非消除；残余风险集中于提示注入逃逸率，而基线第 371 行仅要求注入回归用例存在，未定义通过阈值——该指标无门槛则 R2 的安全性主张不可证伪。"
  ],
  "evidence": [
    "§10.1 第 930-941 行：Butler 协调权清单含 task.cancel/agent.pause/agent.stop/team.create/capability.call；第 944-952 行为默认拒斥的领域权清单——AGAINST 对第 941 行的引用逐字属实。",
    "§9.6 第 856-875 行：Approval 为持久合同对象，作用域 once/task:<id>/count:<n>/ttl/forever，默认拒绝（第 865 行）、重启可恢复（第 872 行）、forever 需二次确认+可撤销+逐次审计（第 871 行）——构成按引用绑定的委托载体，直接回应 AGAINST 的「缺可验证委托凭证」。",
    "§7 第 626-636 行：Broker 链路含「身份/权限/Task scope 检查」（第 629 行）与「记录审计并发布事件」（第 635 行）；§3 第 243 行禁 Agent 私有通道；§4.5 第 368 行跨域传递先脱敏并保留来源标注——双路径的审计统一与来源建模均有合同层保证。",
    "§9.7 第 879/894/900 行：预算挂 Agent 与 Task 两级账本、追加仅限用户批准；§11.1 第 1035 行「控制 Task 预算」与第 900 行「不能扩大」构成分配/扩容二分；manifest 第 456 行 retry.max_attempts=3 由 Broker 统一执行（第 919 行）——重试风暴按成员有界。",
    "manifest 第 449/464 行 scopes、第 465-470 行 verification 钩子与外部收据强制、第 471-473 行 undo 补偿声明（「Task 失败时 Coordinator 应提议补偿而非静默继续」）——补偿作用于声明式收据而非原始数据。",
    "§4.5 第 352-369 行：trusted/agent-derived/untrusted 三级标注，untrusted 驱动的 reversible 及以上一律升级审批（第 363-364 行），「Agent 不得依据 untrusted 内容请求扩权」（第 367 行），第 371 行注入回归用例。",
    "先例对照：Erlang/OTP supervisor 只控 restart 策略但同节点进程间消息不过滤；K8s kube-controller-manager 仅经 API 对象操作、kubelet 权限 node-scoped、RBAC/TokenReview 撤销经缓存层非瞬时生效（有界陈旧被接受）、rollout undo 为纯 API 对象操作；Chromium 网络服务（Browser 侧）持有 cookie jar 而 renderer 数据盲；systemd per-service MemoryMax 与 K8s ResourceQuota+LimitRange+requests/limits 三级配额均因两级共享限额的 noisy-neighbor 问题而生。",
    "§10.3 第 1005 行与 §17 第 1549 行：Task 规范状态归 L2、任务板为可重建投影——FOR 重建论点的正确出处；§11 第 1026-1067 行并无 WAL/Snapshot，证实 FOR 引证错位。"
  ],
  "key_arguments": [
    "【接受 AGAINST 的「高影响协调权故障半径」并据此修正】逐字核对 §10.1（第 930-941 行）确认 task.cancel/agent.pause/agent.stop/team.create 确与 capability.call 同列普通协调权，而 K8s 的实证教训恰是控制面动词必须按作用域分级（Role vs ClusterRole；kubelet 权限被 node-scoped 正因节点失陷只损失本节点）。修正案：Coordinator 的协调动词必须明文裁剪到其 Task 子树——§3 第 223 行「当前 Task」与第 228 行「父子归因」绑定就是现成作用域钩子，但基线未写明协调动词按子树过滤。此项与是否持有领域权正交，AGAINST 打得准，我方立场在此让步。",
    "【驳斥「权限交集缺少可验证委托载体」】§9.6（第 856-875 行）的 Approval 就是持久化、可重启恢复、带 task:<id>/count/ttl/forever 作用域、默认拒绝（第 865 行）、可撤销全程审计（第 871 行）的委托凭证；Broker 在调用点做「身份/权限/Task scope 检查」（第 629 行）。这正是 AGAINST 自己引用的 AWS STS session policy 的「按引用绑定」变体：凭证不随调用方漂移，而在 L2 状态机中——单设备本地系统里比 STS bearer token 更强（无可窃 token）。但接受其 20%：基线缺「不可再转授」深度标志，且 §11.3（第 1060-1067 行）Worker 公式中「成员角色授权」的签发者未定义；好在 Worker 公式不含 Butler 协调权，委托链在构造上只降一层，方向正确、签名者待补。",
    "【驳斥「黑盒结果导致无法补偿/重复副作用」】实证反例是 Saga 模式与 kubectl rollout undo：补偿从来作用于声明式副作用（收据、订单号、message-id），从不需要读对端原始数据库；基线已强制该机制——manifest 的 undo 声明（第 471-473 行「Task 失败时 Coordinator 应提议补偿」）、verification 钩子与外部收据写入 result_reference（第 465-470 行）。AGAINST 混淆了「读原始证据的核对权」与「对副作用执行补偿的操作权」：前者被 R2 正确拒绝，后者基线已显式化。同理，「来源/重试责任需额外建模」已被 §4.5 第 368 行（脱敏+来源标注）与 manifest retry 字段 Broker 统一执行（第 919 行）覆盖。",
    "【裁决相对先例的严格/宽松谱系】比 Chromium 更严格：Chromium 恰恰反转——协调面（browser 侧网络服务）持有 cookie jar 与凭据、renderer 数据盲；BoenMind 令 Butler/Coordinator 数据盲、领域 Agent 数据富，代价是我第一轮承认的中继开销与语义折损，收益是协调面被注入后无可读蜜罐。比 Erlang/OTP 更严格：OTP supervisor 可向任意 worker 发消息（Erlang 只隔离状态不过滤通信），BoenMind 把一切协作压过 Broker 并禁私有通道（§7 第 621-648 行、§3 第 243 行）。比 K8s 更严格：kube-controller-manager 可读作用域内全量 API 对象，BoenMind Coordinator 默认数据盲。唯一更松处无先例：上述所有控制面均为确定性代码，BoenMind 把 LLM 放进协调角色——故交集/预算/默认拒绝审批是必要补偿控制而非多余负担。",
    "【接受预算竞争观察，但基线已有二分钩子】AGAINST 指出的成员重试耗尽共享预算属实，然而 §11.1（第 1035 行）赋予 Coordinator「控制 Task 预算」而 §9.7（第 900 行）只禁「扩大」——分配/扩容二分意味着按阶段子分配有合同接口；且每个成员自带 Agent 级预算与 manifest retry.max_attempts（第 456 行，由 Broker 唯一执行，第 919 行），重试风暴已被按成员有界。cgroups 实证：两级共享限额的 noisy-neighbor 问题催生了 systemd per-service MemoryMax 与 K8s Quota+LimitRange+requests/limits 三级体系——解法是层级细化而非放宽边界。修正案：把「Coordinator 可在 Task 包络内子分配、不可扩容」写为明文。",
    "【驳斥 FOR 的「唯一正确」与「确定性状态机」断言】Chromium 的反向选择（协调面持全量敏感数据、worker 沙箱化）在同一权衡轴上被十亿级用户验证，R2 的数据盲协调面是可辩护取舍而非唯一解，FOR 的绝对化不成立。且 Coordinator 含创建成员/重试或替换成员等判断行为（§11.1 第 1029-1036 行），并非确定性状态机，FOR 以此压缩阶段一工程风险的论证失效。另纠正 FOR 的证据卫生：WAL/Snapshot 独立序列化不在 §11（第 1026-1067 行仅能力/限制/权限公式），其论点的真实出处是 §10.3 第 1005 行与 §17 第 1549 行「Task 规范状态归 L2、任务板为可重建投影」，精确先例是 K8s informer 从 etcd 重建缓存，而非 Erlang let-it-crash（supervisor 重启到已知 init 态，不做授权重放）。"
  ],
  "role": "EMPIRICAL-R2",
  "stance": "「已修正」经逐行核对基线与先例后判定：R2 的机制骨架（控制面/数据面正交、三方权限交集、层次配额、统一 Broker 强制）属于 Erlang/OTP、Kubernetes、Chromium、cgroups/systemd 反复验证的最佳实践，但其应用对象——把非确定性 LLM 放进受限控制面——无工程先例，故定性为「成熟机制的无先例组合」而非可直接照抄的已验证实践；质证后我修正两点：协调动词必须明文按 Task 子树作用域裁剪（否则 AGAINST 的批量取消/暂停故障半径成立），Task 预算必须显式二分「包络内分配=允许、扩容=禁止」。"
}
```

### 【FOR-R2】

```json
{
  "role": "FOR-R2",
  "stance": "已修正——接受 AGAINST 关于协调权内部需分级、委托载体需可验证的批评，将这两点纳入 R2 的补强修正；但驳斥「黑盒链路不可靠」「双路径分裂」「授权配置爆炸」三条，它们要么误解了 Task 授权的动态作用域，要么把实现规格缺失混同于架构裁决缺陷。",
  "key_arguments": [
    "【接受并修正】AGAINST 指出 pause/resume/stop/cancel/team.create 被笼统归为「协调权」会扩大控制面故障半径——这是正确的。R2 的三方交集公式（Butler 协调权 ∩ Task 授权 ∩ 用户授权）已经隐含了「Coordinator 不能 cancel 不属于自己的 Task」，但裁决未显式区分「安全协调」（task.collect、status.query）和「危险协调」（task.cancel、agent.spawn、team.create）。修正方案：参照 Kubernetes RBAC 对 verbs 的 read/write 分离，将协调权内部细分为 coordinator.safe_coordination（只读查询、结果收集）和 coordinator.mutation_coordination（生命周期控制、团队组建），后者在 Task 授权中需显式列出，不可默认继承。这不需要推翻 R2，而是在 R2 的权限交集框架内做更细的粒度切分——与 systemd 将 CapabilityBoundingSet 和 AmbientCapabilities 分层管理的做法同构。",
    "【接受并修正】AGAINST 指出 R2 的集合公式缺少可验证的委托载体（如不可伪造的 delegation token），导致 Broker 无法区分调用链层级——这指出了一个真实的实现规格缺口，但不是 R2 裁决本身的缺陷。R2 定义的是权限边界的 WHAT（三重交集的拓扑结构），委托载体的 HOW 属于 §4 Capability 系统和 §5 Broker 的实现规格。正如 Kubernetes 的 RBAC 裁决说「Pod 只能获得 ServiceAccount 授权的权限」，但 token 签发、audience 绑定、不可转发标志的机制细节在 API Server 的 webhook 层实现。R2 应被理解为：capability.call 经 Broker 时，Broker 必须验证调用链上每一跳的授权凭证，且凭证携带 audience（目标 App）、scope（允许的操作子集）、delegatable（是否允许再转授）和 expiry——但这属于 R2 向下约束 §4/§5 的推导要求，不是 R2 需要自行包含的格式规范。",
    "【驳斥「黑盒链路不可靠」】AGAINST 声称 Coordinator 无法按资源谓词读取原始证据、核对版本或执行补偿。这误解了 Task 授权的动态性：R2 的「Coordinator 权限 = 协调权 ∩ Task 授权 ∩ 用户授权」并不意味着 Coordinator 永远没有任何领域读权限——Task 授权可以且应该在任务规划阶段就包含「read mail.subject + mail.from from Mail App for this task」这类资源谓词。关键是这些读权限是（a）按 Task 显式声明的、（b）由用户批准的、（c）在 Task 结束后自动失效的。这完全不同于 Butler 默认拥有所有 App 的 mail.read——前者是按需最小授权，后者是静默全局授权。补偿同理：Coordinator 在 Task 授权范围内通过 Broker 调用补偿能力（如 order.cancel），不需要超出 Task 边界的额外权限。这与 AWS Step Functions 的补偿模型一致：Step Function 的执行角色只能操作与当前状态机定义匹配的资源，补偿回调在同一角色边界内完成。",
    "【驳斥「双路径分裂」】AGAINST 指出 Butler 直接调用 Capability（§10.2 第958-992行）与通过 Domain Agent 调用产生不同语义。这两条路径的适用场景根本不同：Butler 直接调用仅用于无状态的简单查询（如 system.status、config.read），不涉及跨域编排；Coordinator 通过 Domain Agent 调用用于需要领域推理、状态管理和数据转换的复合操作。两者都经过 Broker 的同一条鉴权管道——Broker 不关心调用者是人类用户、Butler 还是 Coordinator，它只检查 capability token 的 scope 和 audience。审计分裂的担忧已被 §6 Append-only Event Log 解决：每条 Broker 交易都有唯一的 receipt ID、调用者身份（Agent/App/User）、时间戳和结果，无论走哪条路径都可被统一追溯。这不是「分裂」，而是「分层的统一」——与 Unix 的 syscall 层不区分用户进程和守护进程的调用，只检查 uid/capability 的设计同构。",
    "【驳斥「授权配置爆炸」】AGAINST 声称严格受限会迫使每个 App 预写跨域协作的中间数据接口。这在单用户本地优先场景下不成立：Task 授权不是由用户手写 JSON 策略文件，而是由 Butler 的任务规划阶段（§10 Butler App 的 Planning 逻辑）根据用户自然语言意图自动生成 capability 列表，再经用户确认（approve/modify）。这等同于 iOS 的权限请求弹窗——用户看到的是「此任务需要访问邮件主题以筛选工作邮件」，而不是 capability JSON。配置复杂度被 Butler 的规划层吸收，不外泄给用户。只有当 App 开发者新增 Capability 时才需要声明新接口，这与 OSGi Bundle 注册 Service 时声明接口契约完全同构——是 App 开发者的单次成本，不是每次 Task 的运行时负担。"
  ],
  "evidence": [
    "§10.2（第958-992行）：Butler 可直接调用公开 Capability，但 R2 约束这些调用同样受 Butler 协调权边界限制，且仅限于简单查询类操作，跨域编排必须走 Coordinator→Domain Agent 路径。",
    "§11.2-§11.3（第1040-1055行）：Coordinator 权限为三方交集且禁止无限转授，Task 授权是动态的、按任务声明的，不是 Butler 全局权限的子集。",
    "§6 Event Log：Broker 的每笔交易都有唯一 receipt，统一记录调用者身份、时间戳、操作和结果，消除双路径审计分裂。",
    "§3（第220-243行）：Agent 绑定所属 App、Task、能力授权、预算、截止时间和父子归因，禁止 Agent 间私有通道——这是 Coordinator 无法绕过 Broker 的实现基线。",
    "§9.7（第877-901行）：预算同时挂在 Agent 和 Task 上，硬限暂停 Task——Coordinator 不能扩大预算，但可以在 Task 预算内按步骤做子预算分配，这与 Linux cgroups 的 cpu.weight 分层分配同构。",
    "Kubernetes RBAC verbs 分离：get/list/watch（read）与 create/update/patch/delete（write）分开授权，ServiceAccount 的权限不会因为属于某个 Controller 就自动获得 delete 权限。",
    "AWS Step Functions 补偿模型：状态机的执行角色只能操作与状态机定义匹配的资源，补偿回调（Catch/Retry）在同一角色边界内完成，不需要额外提权。",
    "iOS 权限请求模型：App 不预先拥有任何权限，运行时按需请求，用户以自然语言形式看到「需要访问相机以拍照」，而非配置策略文件——BoenMind 的 Task 授权审批同理。",
    "systemd CapabilityBoundingSet + AmbientCapabilities：BoundingSet 定义绝对上界（类比 R2 的 Butler 协调权），AmbientCapabilities 定义实际授予子进程的子集（类比 Task 授权），二者做交集，子进程不能突破。"
  ],
  "conceded_risks": [
    "三重权限交集的授权计算在 Task 执行过程中遇到用户动态撤回权限时，存在时序竞态：如果撤回事件在 Coordinator 已发起但 Broker 尚未完成的调用之间到达，Broker 必须支持「已完成调用不可撤回但后续调用立即拒绝」的语义——这需要 WAL 级别的原子性保证（§11），实现不当可能导致半执行状态。",
    "Coordinator 无法继承 Butler 的长期记忆和用户偏好模型，对跨 Task 累积上下文的持续性任务（如「帮我管理一周邮件」），每次 Task 重建 Coordinator 确实会导致上下文断裂，Butler 层或独立 Memory Service 必须额外承担跨 Task 上下文传递——这是 R2 选择性隔离的必然代价，接受但需在阶段一就规划 Memory Service 接口。",
    "协调权内部未做安全/危险分级是 R2 当前表述的真实缺陷（已在 key_argument #1 中提出修正方案），若阶段一实现时不做此切分，一个被提示注入的 Coordinator 可以在其 Task 团队内批量取消合法成员 Agent 或制造 Agent 风暴——虽然不会溢出到其他 Task（受 Task 授权边界保护），但仍可在当前 Task 内造成拒绝服务。",
    "复杂多步工作流中 Coordinator 遇到未预先授权的 App 能力时必须中断并回退用户确认，这一动态提权阻断会降低全自动规划的连贯性——但这是最小权限原则的必要代价，可通过 Butler 规划阶段的预扫描和批量预授权请求来缓解（类比 Terraform plan→apply 的预览-批准模式），而非放松权限边界。"
  ]
}
```

## 合成裁决

```json
{
  "adr_points": [
    "裁决维持:Butler 是真实 App 但仅持系统协调权,不默认拥有任何 App 的领域操作权;每个 Task 的 Coordinator Agent 不复制 Butler 身份,其权限 = Butler 可授予的协调权 ∩ 当前 Task 授权 ∩ 用户授权,默认拒绝、Task 结束即失效。",
    "协调动词按 Task 子树裁剪并二分分级:只读查询/结果收集类可默认继承;task.cancel/agent.pause/agent.stop/agent.spawn/team.create 等变更类须在 Task 授权中显式列出,且仅可作用于本 Task 子树内的成员与子任务。",
    "三方交集必须物化为 Broker 记账、按引用绑定的 Approval/Grant 载体:携带 task 作用域、audience、资源谓词、不可再转授标志、过期与撤回版本;成员角色授权由 Coordinator 在其自身上界内签发并经 Broker 审计。",
    "capability.call 在协调权语境下仅是『已批准能力清单 + 风险等级 + 资源谓词』的受约束入口,不得作为泛化逃生舱;高风险动作强制 approval_required,低风险确定性能力按 task:<id> 作用域批量预授权。",
    "预算执行『包络内子分配允许、扩容禁止』二分:Agent/Task 两级账本,成员重试受 manifest 重试策略与成员级预算双重约束,Broker 为唯一执行点,包络扩容仅限用户批准。",
    "直接 Capability 与 Domain Agent 双路径共用同一鉴权管道、幂等键、脱敏与收据合同,收据记录来源标注与处理级别,保证审计可重建授权链与证据链;统一合同落地前同一能力不开放双路径。",
    "补偿作用于声明式副作用(外部收据、result_reference、undo 声明),不以读取对端原始数据为前提;untrusted 来源驱动的 reversible 及以上操作一律升级审批,禁止 Agent 依据 untrusted 内容请求扩权。",
    "承认并管理残余风险:将 LLM 置于受限控制面属『成熟机制的无先例组合』,安全主张以注入回归通过阈值与幂等抑制验收为准入条件;数据盲中继开销、审批中断与跨 Task 上下文断裂为已接受代价,由 Memory Service 接口与规划期预扫描+批量预授权缓解。"
  ],
  "amendments": [
    "§11.2 增补:『Coordinator 的协调动词按其所属 Task 子树裁剪:task.cancel/agent.pause/agent.stop/task.collect/team.create 仅可作用于本 Task 子树内的成员与子任务,作用域绑定 §3 的「当前 Task」与「父子归因」字段;子树外目标一律默认拒绝。』",
    "§11.2 增补协调权二分:『协调权细分为 safe_coordination(只读查询、状态查询、结果收集,可默认继承)与 mutation_coordination(生命周期控制与团队组建:cancel/pause/stop/agent.spawn/team.create,须在 Task 授权中显式列出,不可默认继承)。』",
    "§11.3 增补公式落地条款:『三方交集的计算结果必须物化为经 Broker 记账的 Approval/Grant 绑定记录(作用域 task:<id>、默认拒绝、可撤销、重启可恢复);Grant 字段含 audience、action、资源谓词、delegation_depth=0(不可再转授)、过期时间、撤回版本与父授权哈希;成员角色授权的签发者定义为 Coordinator,且不得超过其自身 Grant 上界。』",
    "§10.1 修订 capability.call 条目:『capability.call 作为协调权行使时,仅能引用 manifest 已批准的能力清单、资源谓词与风险等级,不得作为泛化逃生舱;来源含 untrusted 且风险等级 reversible 及以上时强制 approval_required;低风险确定性能力可按 task:<id> 作用域批量预授权。』",
    "§9.7/§11.1 增补预算二分明文:『Coordinator 可在 Task 预算包络内向成员子分配预算(逐笔记账、仅在包络内分配),不可扩容;包络扩容仅限用户批准(§9.7 第900行不变);成员重试受 manifest retry 策略与成员级预算双重约束,Broker 为唯一执行点。』",
    "§17/R2 措辞修订:『删除「Coordinator 为确定性有界状态机」类表述,改为:Coordinator 是含受限判断行为(创建成员、重试或替换成员)的非确定性控制面实体,其安全由交集、预算、默认拒绝审批与子树裁剪等补偿控制围堵而非消除。』"
  ],
  "conditions": [
    "委托载体规格必须在阶段一实现前于 §4/§5 闭环:Grant 至少携带 audience、action、资源谓词、过期时间、撤回版本、不可再转授标志与父授权哈希,由领域 Provider 在调用点验证并强制幂等键;闭环前不得对外宣称 R2 为『可执行的安全机制』。",
    "协调动词子树过滤与 safe/mutation 分级必须写入 §11.2/§11.3 并在 Broker 调用点(§7 第629行)强制执行;未落地前,子树外的 task.cancel/agent.stop/team.create 按未授权能力默认拒绝。",
    "注入回归用例必须定义量化通过阈值(如 untrusted 驱动的 reversible 及以上操作 100% 升级审批、越权扩权请求 100% 默认拒绝)并作为 CI 门槛;无阈值不得主张 R2 安全性已验证。",
    "双路径必须共享同一 Grant、幂等、脱敏与收据合同,收据记录来源标注与处理级别;统一合同落地前,禁止对同一能力同时开放直接 Capability 与 Domain Agent 两条路径。",
    "跨 Task 上下文传递接口(Memory Service)必须在阶段一规划;持续性任务(如『帮我管理一周邮件』)不得以 Coordinator 全量重建为默认方案。",
    "采纳 AGAINST 提出且未被驳回的可证伪条件作为验收标准:对同一授权的两次等价请求,系统必须在审计日志中证明第二次被幂等抑制;无法证明即视为 R2 实现不完整。"
  ],
  "consensus_points": [
    "控制面/数据面正交分离成立:Butler 与 Coordinator Agent 仅持系统协调权(§10.1 第930-941行)、默认不持有任何领域操作权(第944-952行),领域数据访问一律经 Broker 路由至领域 Agent/Provider;AGAINST 第二轮明确让步『接受控制面与领域面分离及最小权限边界』,两轮后三方一致。",
    "三方交集公式(Butler 可授协调权 ∩ 当前 Task 授权 ∩ 用户授权,§11.3 第1053-1058行)作为权限上界语义成立,且经 §9.6 Approval 持久合同对象(作用域 once/task:<id>/count:<n>/ttl/forever、超时即 denied、可撤销、重启后恢复)落地为按引用绑定的委托载体:凭证驻留 L2 状态机而非随调用方漂移,单设备本地场景下不引入可窃取的 bearer token。",
    "四条硬边界经质证未被驳倒:不得绕过 Broker(§7 第621-648行统一入口、§3 第243行禁 Agent 间私有通道)、不得无限转授成员权限(§11.2 第1044行)、不得扩大 Task 预算(§9.7 第900行)、Agent 不是权限来源(§3 第231行);所有调用落统一审计(§7 第635行),调用者身份/时间戳/结果跨双路径统一归因。",
    "『数据盲协调面不阻塞补偿』命题驳倒了 AGAINST 的『黑盒链路不可靠』论点:补偿作用于声明式副作用(manifest undo 声明第471-473行、verification 钩子第465-470行、外部收据写入 result_reference),与 Saga / kubectl rollout undo 同构;『读原始证据的核对权』被 R2 正确拒绝,『执行补偿的操作权』经 Broker 在 Task 授权边界内行使。",
    "重试风暴按成员有界:manifest 重试策略(第456行)由 Broker 统一执行,且仅 read-only 与 low-risk-command 允许自动重试、reversible 及以上必须依赖幂等键或恢复流程(第474-476行),叠加 Agent/Task 两级预算账本与三层强制点(§9.7 第889-895行);两级共享限额 noisy-neighbor 问题的先例解法是层级细化(systemd per-service MemoryMax、K8s Quota+LimitRange),而非放宽边界。",
    "信任分级来源治理阻断注入扩权路径:§4.5 三级标注(trusted/agent-derived/untrusted)下,untrusted 内容驱动的回合中 reversible-command 及以上调用一律升级审批(第363-364行),『Agent 不得依据 untrusted 内容请求扩权或自我授权新能力』(第367行),跨域传递先脱敏并保留来源标注(第368行)。",
    "定性共识:R2 属『成熟机制的无先例组合』——控制面/数据面正交、最小特权派生、层次配额、统一 Broker 强制均有 Erlang/OTP、Kubernetes、Chromium、cgroups/systemd 先例,但把非确定性 LLM 放入受限控制面无工程先例,交集/预算/默认拒绝审批是必要补偿控制而非多余负担;该定性未被任何一方驳倒。",
    "已接受代价三方共同确认:数据盲中继的 Token 开销与语义折损(K8s 控制器可读全量声明式状态,BoenMind Coordinator 只能依赖结构化汇总)、动态提权的首次审批人机往返(用户离线时停在 waiting_approval)、跨 Task 上下文断裂(须由 Butler 层/Memory Service 承接);缓解手段为规划期预扫描 + task:<id> 作用域批量预授权(§9.6 第868行)与 Memory Service 接口,而非放松权限边界。"
  ],
  "disputes": [
    "委托凭证规格的归属层级:AGAINST 要求 Broker 签发、Provider 端验证的不可伪造 Task Capability Grant(字段含 task_id、audience、action、资源谓词、参数约束、风险等级、预算预留、过期、撤回版本、父授权哈希);FOR 主张这是 §4/§5 的实现规格(HOW),R2 只需向下推导约束;EMPIRICAL 核实 §9.6 Approval 已构成按引用绑定的载体、但承认缺『不可再转授』深度标志且 §11.3 成员角色授权签发者未定义(第1060-1067行)。两轮后分歧收窄为『裁决文本内嵌凭证规格 vs 下沉实现规格』,对『何时算闭环』标准不同,未合流。",
    "预算竞争的缓解深度:AGAINST 的共享包络竞争场景(成员重试耗尽 Task 预算→合法后续步骤被暂停;发送已达 Provider 但回执丢失时恢复期无法判断是否重复发送)被 EMPIRICAL 承认属实,但 AGAINST 要求按步骤『预留+补偿』机制,EMPIRICAL/FOR 的修正止步于『包络内子分配、扩容禁止』明文化(§11.1 第1035行与 §9.7 第900行的二分钩子);预留(reservation)机制是否进入 R2 未决。",
    "双路径结果语义是否规格化:FOR 主张直接 Capability 仅限无状态简单查询、跨域编排必须走 Domain Agent(此为对 §10.2『确定性操作→Capability、复杂推理→Domain Agent』的延伸约束),并以统一 Broker 管道与审计收据驳『审计分裂』;AGAINST 指出同一 mail.search 直连路径返回原始结构化结果、Agent 路径可能只返回摘要,数据版本、脱敏边界与幂等语义不同,要求统一的数据最小化/结果摘要/授权链合同;EMPIRICAL 仅确认审计统一有合同层保证(§7 第629/635行、§4.5 第368行),未裁决语义差异是否需要规格化。",
    "注入安全主张的可证伪门槛:经核实基线第371行仅要求『注入回归用例存在』,未定义通过阈值;EMPIRICAL 据此判定 R2 安全性主张现阶段不可证伪,两轮内没有任何一方给出量化阈值方案(如升级率/默认拒绝率指标),悬置。",
    "阶段一工程收益定级:FOR 的『Butler/Coordinator 降维为确定性有界状态机、直接降低交付风险』被 EMPIRICAL 驳倒——§11.1(第1029-1036行)明列『创建成员、重试或替换成员』等判断行为,Coordinator 非确定性状态机;其证据卫生亦被纠正(WAL/Snapshot 不在 §11,重建论点真实出处是 §10.3 第1005行与 §17 第1549行『Task 规范状态归 L2、任务板为可重建投影』,精确先例是 K8s informer 重建缓存而非 Erlang let-it-crash)。FOR 的『规划层吸收授权配置复杂度(iOS 式审批弹窗)』论点未被直接质证,真实负载下的用户审批频次成本缺乏实证,收益主张部分悬置。"
  ],
  "ruling_id": "R2",
  "verdict": "amend"
}
```
