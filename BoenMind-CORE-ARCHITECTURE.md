# BoenMind 核心架构基线

> 定位：目标系统的第一性原理与稳定结论——原则、边界、不变量、核心裁决、里程碑制度。
> 源起：2026-08-26 与用户重新梳理后的核心愿景；不照搬 DSH、Pi、Pi Agent Rust 或旧 BoenMind 方案。
> 分期：阶段一交付能在 Windows、Linux、macOS 上长期运行的 AI 工作软件；阶段二才评估是否演进为 AI Runtime / AI OS（§21）。阶段二的完整系统级设计是长期方向，不是阶段一的前提或验收条件。
>
> **如何读**：§1-§16 原则与合同面；§17 核心裁决；§18 里程碑定义；§19 回看制度；§22 大白话版；§23 ADR 机制；§24 模型即代码。
> **维护规则**：架构增量一律发 ADR（§23）；ADR 对本文的修订**直接熔入正文并标注 ADR 编号**，不挂追加式引注块（ADR-0015）；正文与 `architecture/boenmind.c4` 不一致时以模型为准。
> **编号锚点**：§1-§24 编号被 adr/、milestones/ 大量引用（§13.5/§17/§18/§19 最密）；重排本文结构前必须先 grep 引用面（`grep -rn "基线 §" adr/ milestones/`）。

## 1. 产品本质

BoenMind 不是“一个聊天窗口加一堆工具”，而是一个面向个人软件生态的 **AI Runtime / AI 操作系统**：

```text
通信 App ─┐
邮件 App ─┤
音乐 App ─┼── 共用同一个 AI Runtime
股票 App ─┤
Wiki App ──┘
```

系统统一提供：

- Agent 生命周期；
- 记忆与技能；
- 工具与能力注册；
- 身份、权限和审批；
- App 之间的受控通信；
- 多 Agent 任务协作；
- 插件装载、替换、升级和故障恢复；
- 与聊天、搜索、语音、按钮等前端形态解耦的接入面。

各软件不再各自维护一套完整的 AI、记忆和技能，而是作为隔离的 App/服务域接入统一 Runtime。

### 1.1 当前真正要做的产品

阶段一不是先造一个操作系统的“半成品”，而是先做一个能长期替用户工作的跨平台软件。它必须能安装、启动、执行任务、记录过程、发现错误，并在界面关闭、网络断开或进程重启后恢复工作状态。

阶段一的核心验收问题不是“架构概念是否完整”，而是：

```text
Agent 接到了什么任务？
它实际做了哪些动作？
每个动作是否真的成功？
任务现在处于什么状态？
中途断掉后能不能继续？
如果做错了，能不能从日志找到原因？
```

阶段一先保留稳定的 Wire API、事件 Schema、Operation 状态、Capability 调用边界和持久化格式；L0 Supervisor、Runtime Generation、系统级热升级等内容放到阶段二重新评估。

阶段一的 Wire API 是未来 L1 Kernel Contract 的子集与前身：凡进入 Wire API 的字段自冻结之日起只增不破；阶段二收敛 L1 合同时以阶段一的实证为基础收缩边界，而不是推倒重来。

阶段一同时冻结一批非目标：单用户、单设备，无账号体系；无多设备同步；无用户可编程自动化规则引擎；无移动端 Surface；无插件市场（仅本地安装）；无跨能力自动事务。非目标不是永远不做，而是防止范围蔓延；解除某条非目标前，必须先检查它是否破坏既有合同与扩展点。

部署形态与 Surface 矩阵（ADR-0009 修订，2026-08-29）：原「本地优先」修正为**私有部署优先（本机或自管 VPS）**——Runtime 允许部署于用户自管的 Linux VPS，单用户经 TLS + 个人令牌/passkey 的网页 Surface 访问；仍无账号体系、无多用户、无多设备同步（访问端无状态，状态单一权威仍在 Runtime）、无移动端。Surface 矩阵：CLI（脚本 JSON/JSONL + 交互式 TUI）与 Web UI 为阶段一交付 Surface；Tauri 仅作 Windows 桌面壳并复用 Web 前端同一代码库（ADR-0013/0014 后 = `runtime/webapp`）；不做 Linux/macOS 桌面壳。安全前置与里程碑落点见 ADR-0009 与威胁模型 T-13/T-14。

## 2. 核心对象边界

必须严格区分以下概念：

| 对象 | 定义 |
|---|---|
| **Runtime** | 由 L1 根合同、L2 Runtime Core 和受其管理的运行实例组成；负责身份、权限、通信、调度、持久化、监督和恢复 |
| **App** | 一个业务或系统服务域，拥有自己的数据、能力、Agent 和生命周期 |
| **Capability** | 可被调用的确定性能力，例如 `audio.volume.set`、`mail.search` |
| **Agent** | 负责理解、规划、判断和协作的运行时执行实例 |
| **Task** | 一个需要完成的工作上下文，可包含多个 Agent |
| **Team** | 为一个 Task 组织起来的 Agent 集合 |
| **Coordinator Agent** | Task 级队长，负责拆解任务、召集成员、收集结果和监管执行 |
| **Plugin** | 可安装、替换、升级的能力实现或 Runtime 扩展 |
| **Surface** | 人或外部软件接入系统的交互形态，如聊天、搜索、语音、通知或按钮 |
| **Session** | Surface 与 Agent、Task 或 Capability 之间的可恢复交互上下文 |
| **Operation** | 一次可追踪的 Capability、Command 或 Agent 执行，拥有稳定的 `operation_id` |
| **Approval** | 对高风险或外部副作用操作的持久化审批请求及其决定 |
| **Artifact** | Task 或 Agent 产生的报告、文件、结构化结果等可引用产物 |
| **Memory** | 按域隔离的持久化经验与事实存储；地址即权限边界，实现可替换 |
| **Skill** | 可版本化的程序性知识包：步骤或提示模板加允许调用的 Capability 清单；是数据不是执行体 |
| **Secret Ref** | 凭据的稳定引用（如 `secret:mail/account-1`）；凭据本体只存于可替换的 Secret Store，永不进入上下文、日志和事件 |
| **Projection** | 从规范状态或事件日志派生的查询视图，例如 Task Board 或 CLI watch 视图 |

基本关系：

```text
Plugin 组成或扩展 App
App 提供 Capability 和领域 Agent
Agent 消费 Capability 并完成推理/协作
Task 组织多个 Agent
Surface 连接 Agent、Task 或 Capability
```

系统不能围绕单个聊天 Agent 组织，否则会重新形成“超级 Agent + 工具箱”的单体架构。

### 2.1 五层运行时边界（阶段二演进模型）

为了明确“外围可替换”和“内核如何升级”，系统分为五层：

这一节描述的是阶段二的完整演进模型，不是阶段一必须一次性实现的系统分层。阶段一可以先把 Runtime Core、Provider 和 Surface 做成一个跨平台软件中的逻辑模块或少量进程；只有真实运行证明需要更强隔离和系统级升级时，才逐步拆出 L0-L5 的完整边界。

```text
L0  Bootstrap / Supervisor
    启动、监控、升级、代际选择、排空、回滚

L1  Kernel Contract
    IPC、身份、Capability、事件、Agent/Task 生命周期、状态迁移合同

L2  Runtime Core
    Registry、Broker、Bus、Agent/Task、Memory、Skill、Persistence、Recovery

L3  System Services / Providers
    Audio、通知、文件、浏览器、模型连接器等系统服务和驱动适配器

L4  User Apps
    Mail、Music、Stock、Communication、Wiki、Butler 等业务/系统 App

L5  Surfaces
    Chat、CLI、Search、Voice、Notification、按钮和第三方界面
```

其中：

- **L0 是最小且最稳定的启动底座**，不承载业务逻辑，负责选择和管理 Runtime 代际；
- **L1 是所有可替换组件共同遵守的根合同**，改变它才属于系统级大升级；
- **L2 是可按代际升级的 Runtime Core**，升级时由 L0 管理新旧版本切换；
- L3/L4/L5 是主要的局部升级对象，不应因为它们升级而替换整个 Runtime；
- L3/L4 可以是独立进程，也可以在明确的可信边界内作为 Runtime 内模块实现，但只有独立进程才具备真正的独立崩溃隔离和热替换能力。

因此，“万物皆插件”不是要求 L0/L1 任意热插拔，而是要求外围实现依赖稳定合同，核心服务支持可回滚的代际升级。

CLI 是 L5 的标准 Surface，不是 GUI 不可用时的临时备份。GUI、CLI、语音和第三方界面都通过 L1 合同接入；CLI 另外通过受限的 L0 Control Protocol 访问 Supervisor 和 Upgrade Manager 的运行控制能力。L2 业务权限仍必须经过 Broker，CLI 不得因为是本地进程而绕过身份、权限、审批或审计。

### 2.2 状态归属与升级对象

系统状态不能只存在某个进程的内存里。按归属划分：

```text
L0 状态：当前 active generation、安装版本、升级事务、回滚指针
L1 状态：协议/合同版本、迁移版本、兼容矩阵
L2 状态：Registry、Agent、Task、权限、事件、记忆索引、Approval、Artifact 引用和模型调用账本
App 状态：各 App 私有数据、凭据引用、领域投影
Provider 状态：连接器游标、外部系统映射、可恢复的副作用记录
```

Task 的规范状态、生命周期和事件归 L2 持有；Orchestrator 自己保存的 Task Board、编队策略和卡片布局属于 Projection 或编排层私有状态，可由事件重建，不能成为 Task 存在与否的唯一依据。归属细分为三层（ADR-0004）：(a) Task/Session/Operation/Approval/Artifact 的规范状态与生命周期——L2 唯一持有；(b) 编排决策（命令意图、成员变更、预算扣减）——以不可变决策/意图事件持久写入 L2 事件日志，属持久合同；(c) UI 视图（Task Board、卡片布局、排序偏好）与 Orchestrator 策略私有参数（prompt 模板选择等）——Projection/私有状态，可随时丢弃重建，永不作为 Task 存在性依据。

L0 只保存启动和升级所需的最小控制状态；业务状态归 L2、App 或 Provider 所有。
Runtime Core 重启或换代时，必须从持久状态恢复，而不是依赖旧进程仍在内存中。

一次升级的最小对象是一个 **Runtime Generation**。它不是某个插件版本，而是能够共同运行并拥有一套 L2 状态访问权的完整 Runtime Core 代际：

```text
runtime:boenmind
generation:42
version:1.4.0
state: prepared | migrating | validating | committing | active | draining | rolled_back | failed
```

更完整的升级事务状态见第 13.4 节；这里的 `generation` 表示完整 L2 Runtime Core 代际，不表示单个插件实例。

每个时刻只能有一个 generation 对同一份可写业务状态拥有写入权。旧代际排空期间默认只读；只有已经进入不可重复、且写入归属仍由旧代际独占的收尾事务，才允许完成。其余进行中请求必须取消、转交或标记为 `outcome_unknown`，不能让新旧代际同时写同一状态。

### 2.3 扩展点：万物皆插件的准确含义

"万物皆插件"的准确含义是：**内核只由合同与最小机制组成，其余一切都能在不修改内核的前提下增加、替换和移除。**系统不靠枚举功能保持完整，而靠开放合同保持可扩展。具体功能和业务可以写进本文档作为示例与验收场景，但任何功能都不允许成为内核特权。

合同（稳定层，改动才构成系统级升级）：

```text
Wire API / Surface Protocol
事件 Schema 与全局顺序
Capability Manifest 与调用语义
Operation 收据与状态机
Approval 状态机与 Budget 对象
身份、权限与统一错误信封
Secret Ref 引用格式
```

扩展点（开放集合，新事物从这里插入，全部以普通 Provider/App/Plugin 身份注册）：

```text
Provider   任何确定性能力实现：系统服务、连接器、模型连接器、
           记忆引擎、定时器、Secret Store、评价器
App        任何业务域或系统服务域
Surface    任何交互适配器
Plugin     任何需要打包、分发、升级的实现单元
Skill      任何注入给 Agent 的程序性知识
```

判定一个新功能能否优雅接入，看它能否回答六个问题：

```text
它以什么身份注册？          Registry 既有分表，或按同一模式新增分表
它遵守哪个既有合同？        调用、事件、收据、审批之一
它的权限如何声明和授予？    manifest 声明 + Broker 裁决，无特权通道
它的崩溃如何被隔离和恢复？  进程边界 + Supervisor 规则
它的状态归谁、如何恢复？    L2 / App / Provider 状态归属（见 2.2）
它能否不重启内核而被替换？  Provider / Plugin 生命周期
```

六个问题都能回答，就做成插件接入；必须改合同才能接入的，按 13.5 的升级级别处理，且必须先证明该抽象无法用既有扩展点表达。反过来说：内核每增加一个内建特权功能，都必须给出"为什么不做成 Provider/App"的明确理由。本文档所有章节都遵守这一条——先定义合同与扩展点，再谈具体实现放在哪里。

## 3. Agent 的系统地位

Agent 不是整个软件的中心，而是一种可以被 Runtime 随时：

```text
创建、启动、暂停、恢复、取消、通信、监督、重启、回收
```

的执行实例。

示例：

```text
agent:butler
agent:mail-search
agent:mail-compose
agent:stock-analysis
agent:music-control
agent:task-123-coordinator
```

Agent 可以同时存在十几个甚至更多，但每个 Agent 都必须绑定明确的：

- 所属 App；
- 当前 Task；
- 身份；
- 能力授权；
- 预算；
- 截止时间；
- 父子/归因关系；
- 可访问记忆范围。

Agent 不是权限来源。它能做什么，取决于 Runtime 授予它的 Capability。

Agent 生命周期状态机（与 Operation 状态机对等；迁移只能由 L2 写入并发布事件，必须携带原因码）：

```text
created → starting → running ⇄ waiting(input | approval | tool | model)
                              ⇄ paused
                    → stopping → stopped
任意状态 → failed | cancelled
恢复：重启后 interrupted → resuming → running | stopped
```

Agent 之间不建立私有通道；一切协作经 Task 消息面或 Broker。

## 4. App 隔离模型

App 是主要的领域和安全边界。每个 App 至少隔离四类内容。

### 4.1 数据隔离

```text
memory:app:mail
memory:app:stock
memory:app:music
memory:task:<task-id>
memory:agent:<agent-id>
```

默认情况下：

- Mail Agent 只能访问邮件域及被 Task 授予的数据；
- Stock Agent 只能访问股票域及被 Task 授予的数据；
- Butler Agent 不能直接读取其他 App 的原始数据库；
- 团队共享 Task 上下文、结果和产物，不默认共享 App 私有数据。

Memory 是一等合同对象，不是某个 App 的私有实现。作用域即权限边界：

```text
memory:app:<app>      App 域私有经验
memory:task:<id>      任务上下文，结束后按保留策略归档或删除
memory:agent:<id>     单 Agent 工作记忆
memory:user           用户级偏好与事实，写入需显式授权
```

```text
读写检索删除都走 Broker（memory.write / memory.search / memory.delete），
受数据域隔离约束；
阶段一检索用 FTS，向量引擎是可替换实现，接口预留不承诺；
默认不自动写长期记忆，Task 结束时的提炼由所属 App 显式声明；
用户纠正优先级最高，覆盖而非追加；来源被删除时记忆级联失效。
```

Skill 是注入给 Agent 的知识包（步骤或提示模板 + allowed_capabilities），永远只是数据：加载 Skill 不改变权限，执行仍走 Agent 与 Broker。三者区分：Capability 是可执行的确定性动作，Skill 是怎么做的知识，Plugin 是打包与生命周期单元。Skill v0.2 第一步已增发合同 Minor 字段 `version`（版本号）与 `references`（按需加载引用分支文件清单）（ADR-0016）；同一 ADR 定义的 scripts 可执行脚本执行面，待 wasmtime 管线接入实施时另行增发合同，实施前 Skill 仍只是数据。

### 4.2 能力隔离

能力使用明确命名空间：

```text
mail.search
mail.read
mail.create_draft
mail.send
stock.quote.get
stock.analyze
stock.place_order
music.search
music.play
audio.volume.get
audio.volume.set
audio.mute
```

App 可以公开能力，但“公开”不等于绕过 Runtime。所有调用仍然经过 Broker 的身份、权限、参数和审计检查。

### 4.3 运行时隔离

需要独立故障边界的 App/Provider 运行在独立进程，并由 L0 Supervisor 管理：

```text
L0 Supervisor
├── L2 Runtime Core generation
├── L4 Butler App Runtime
├── L4 Mail App Runtime
├── L4 Stock App Runtime
├── L4 Music App Runtime
└── L3 Audio Provider Runtime
```

这些进程通过 L1 合同和 Broker/Bus 通信，而不是共享内部对象。一个 App/Provider 崩溃时，其他 App 和 Runtime Core 不应被拖垮；若能力属于可信高频内建模块，也可以暂时进程内实现，但失去独立崩溃边界。

### 4.4 身份隔离

请求必须携带可审计的调用者身份，不能只传一个裸方法名：

```json
{
  "principal": "agent:butler",
  "app": "butler",
  "task_id": "task-123",
  "capability": "audio.volume.set"
}
```

可能的身份包括：

```text
user
system
app:mail
app:stock
agent:mail-search
agent:task-123-coordinator
team:task-123
plugin:mail-connector
```

### 4.5 数据信任分级与提示注入防线

邮件正文、网页、文件和第三方 API 返回值都是攻击者可控内容，会进入 Agent 上下文；恶意邮件可以直接试图诱导 `mail.send`。Broker 权限是必要条件，不是充分条件。所有数据携带信任级别，全链路传递：

```text
trusted          用户直接输入、系统自身产生
agent-derived    Agent 推理产出
untrusted        外部内容：邮件正文、网页、文件、第三方返回值
```

规则：

```text
不可信内容永远作为带来源标注的数据进入上下文，
  不与系统指令同构拼接；
由 untrusted 内容驱动的回合，reversible-command 及以上调用
  一律升级为审批（调用上下文携带 input_trust 字段）；
审批卡片由 Broker 生成结构化摘要，untrusted 原文只能作为
  带标注引用展示，不能冒充"任务理由"；
Agent 不得依据 untrusted 内容请求扩权或自我授权新能力；
跨域传递必须先过脱敏并保留来源标注。
```

阶段一落地成本很低：内容标注 + Broker 门控 + 注入回归用例，不需要任何检测模型。

### 4.6 秘密边界

```text
凭据本体只存在于 Secret Store：OS keychain 优先（Windows
  Credential Manager / macOS Keychain / libsecret），
  加密文件兜底；Secret Store 本身是可替换的 Provider；
上下文、日志、事件、投影中出现的是 Secret Ref，不是秘密；
Provider 在 handshake 或调用时由 Runtime 注入凭据，
  并同受脱敏管线约束；
录入、轮换、删除凭据属 high-risk-command，需要审批。
```

配置、状态、秘密三者分家：用户可读设置进配置文件；运行事实进 L2 状态与事件日志；凭据只进 Secret Store，永不入前两者。

## 5. Capability：确定性能力的统一抽象

并非所有操作都需要经过 Agent。

例如“把音量调到 60%”应当是：

```text
Butler Agent / 前端按钮 / 语音解析器 / 自动化规则
  → Capability Broker
  → audio.volume.set
  → Audio Provider
```

而不是：

```text
Butler Agent
  → Music Agent
  → Music Agent 理解请求
  → Music Agent 调音量
```

后者增加延迟、消耗 Token，并制造不必要的 Agent 耦合。

### 5.1 系统服务与业务 App

有些能力虽然被某个软件暴露，但架构上属于公共系统服务。例如音量控制不应只属于 Music App：

```text
Audio System Service
├── audio.volume.get
├── audio.volume.set
├── audio.mute
└── audio.output.list

Music App
├── music.search
├── music.play
└── music.pause
```

因此系统允许两类 App：

- **业务 App**：邮件、音乐、股票、通信等；
- **系统服务 App**：音频、通知、剪贴板、文件选择、日历等。

### 5.2 Capability 描述

每个能力必须在注册表中声明：

```json
{
  "capability": "audio.volume.set",
  "provider": "system.audio",
  "version": "1.0.0",
  "input_schema": {},
  "output_schema": {},
  "effect": "low-risk-command",
  "idempotent": true,
  "cancellable": true,
  "timeout_ms": 1000,
  "approval": "not-required",
  "scopes": ["audio.control"],
  "verification": {
    "query": "audio.volume.get",
    "expect": {"volume": 60},
    "within_ms": 2000
  },
  "undo": null,
  "retry": {"max_attempts": 3, "backoff_ms": 500, "retry_on": ["timeout", "unavailable"]},
  "deprecated_by": null
}
```

前十个是必填字段，其余是可选扩展字段，manifest 本身是开放结构：新增可选字段走 Minor 升级，消费方必须忽略不认识的字段。

```text
scopes         权限范围标签，授权和审计以 scope 为粒度
verification   事后核验钩子：Command 类能力声明"如何确认生效"，
               Observation 层据此自动核验，使"Agent 声称完成"与
               "系统实际观察到"的对照成为机制而非口号；
               无法声明 verification 的 external-side-effect 能力
               必须返回外部收据（邮件 message-id、订单号等），
               写入执行收据的 result_reference
undo           可补偿性声明：reversible-command 鼓励声明逆操作；
               Task 失败时 Coordinator 应提议补偿而非静默继续；
               阶段一不要求自动事务，只要求可补偿性显式化
retry          自动重试策略，由 Broker 统一执行；仅 read-only 与
               low-risk-command 允许自动重试，reversible 及以上
               必须依赖幂等键或恢复流程
deprecated_by  能力废弃链，旧版本保留期与调用告警由此驱动
```

### 5.3 风险等级

不能只用“有危害/无危害”二分：

```text
read-only
low-risk-command
reversible-command
external-side-effect
high-risk-command
```

示例：

```text
audio.volume.set       low-risk-command
music.play              low-risk-command
mail.create_draft       reversible-command
mail.send               external-side-effect
stock.place_order       high-risk-command
```

即使不需要用户弹窗，也必须经过 Broker 检查和审计。风险等级与 4.5 的信任分级正交组合：untrusted 来源把实际风险上提一级处理。

### 5.4 模型连接器也是 Provider

模型调用是系统能力的一部分，不是内核特权。模型接入遵守与其他 Provider 完全相同的模式：

```text
模型调用遵循稳定的模型合同：
  输入：messages、tools、参数、上下文预算
  输出：增量或完整结果、用量、结束原因
实现可替换：云端 API、本地模型、未来的模型网关，
  调用方只依赖合同，不依赖具体厂商
```

每次模型调用产生一条调用账本记录，并入 Execution Log：

```text
model_id、参数、prompt 引用与哈希（原文入受保护引用位置）
token 用量与成本、耗时、是否流式中断
request_id / operation_id / agent_id / task_id
```

账本同时是预算记账（9.7）和录制回放测试的载体。配套规则：

```text
凭据由 Secret Store 注入，Agent 全程不可见（4.6）；
外发内容先过脱敏与信任分级检查（4.5）；
降级链：主模型超时/超限/不可用 → 备选模型 → Agent 回合失败，
  Task 进入 blocked，不跨域自动重试；
上下文压缩由 Agent 执行器负责，每次压缩写入 Execution Log
  （压缩前后 token 数与被摘要范围），保证轨迹可解释；
独立 Judge 走同一合同与同一记账，仅策略不同（tools: []、temperature: 0）。
```

阶段一允许模型连接器作为 L2 内建模块实现，但合同与记账从第一天就按 Provider 边界设计；阶段二外置为独立进程时，调用方无感。

## 6. Runtime Registry：统一注册中心

系统可以只有一个统一注册中心，但内部按对象类型分表：

```text
Runtime Registry
├── App Registry
├── Plugin Registry
├── Capability Registry
├── Agent Registry
├── Task/Team Registry
├── Surface Registry
└── Provider Binding Registry
```

分表是开放集合：新对象类型（模型、技能、审批、产物等）按同一模式增设分表，不修改内核结构。

### 6.1 App Registry

记录：

- App 身份与版本；
- 数据域；
- 生命周期状态；
- 提供的 Capability；
- 允许的外部调用；
- 支持的事件；
- 健康状态；
- 运行进程和连接信息。

### 6.2 Capability Registry

回答：

```text
谁提供 audio.volume.set？
输入输出合同是什么？
风险等级是什么？
是否幂等、可取消、可重试？
哪些身份可以调用？
当前 Provider 是否健康？
```

### 6.3 Agent Registry

记录：

- Agent 身份；
- 所属 App；
- 所属 Task/Team；
- 模型和运行状态；
- 能力授权；
- 预算和截止时间；
- 父 Agent 与归因链；
- 当前运行句柄。

### 6.4 Provider Binding Registry

将稳定的 Capability 名称绑定到当前实现：

```text
audio.volume.set → system.audio@1.2.0
mail.search      → mail.connector@3.1.0
stock.quote.get  → broker.connector@2.0.0
```

调用方只依赖 `audio.volume.set`，不依赖具体插件名称、进程地址或实现语言。

Registry 必须分成两层：

```text
持久化逻辑目录
  capability → provider/version/protocol/status

运行时缓存
  capability → 当前连接、进程句柄、健康检查结果
```

运行时缓存可以在重启后丢失并重建；持久化逻辑目录不能依赖内存指针或临时进程地址。
Provider 的 `active / draining / unavailable` 状态、版本和合同版本都必须可恢复。

Registry 还应为 CLI 和其他 Surface 提供机器可读的发现结果，包括输入输出 Schema、风险等级、所需 scope、是否支持 dry-run、是否可恢复以及当前健康状态。帮助文本、参数补全和运行时校验应尽量从同一份合同生成，避免 CLI 自己维护另一套能力定义。

## 7. Capability Broker：所有跨域调用的统一入口

Registry 只负责“谁提供什么”；Bus 只负责消息传输；真正做调用裁决的是 Broker。

```text
Caller
  → Capability Broker
  → Registry 查找 Capability
  → 身份/权限/Task scope 检查
  → 参数校验
  → Provider Binding
  → 执行或转发
  → 超时/取消/重试处理
  → 返回结果
  → 记录审计并发布事件
```

所有调用方共用这一入口：

```text
Agent
前端按钮
语音交互适配器
自动化规则
Timer
Butler
其他 App
```

Timer 和自动化规则只是普通调用方：定时能力本身是可替换的系统服务 Provider，内核不持有调度特权。

调用方不需要知道 Provider 是：

- Runtime 内的 Rust 实现；
- 独立 Rust 进程；
- TypeScript 进程；
- Python 服务；
- WASM 模块；
- 远程服务。

统一调用形式：

```text
broker.call("audio.volume.set", params, context)
```

每次可追踪调用都应携带或产生以下关联字段：

```text
request_id       当前请求
operation_id     当前执行操作
correlation_id   跨调用、事件和重试的关联标识
task_id          所属 Task，可为空
deadline         绝对截止时间
binding_epoch    授权决策点固化的不可变绑定代际
cancellation     取消句柄或取消状态
idempotency_key  副作用操作的幂等键，可为空
input_trust      调用输入信任级别：trusted | agent-derived | untrusted
provider_instance_id 授权时固化的 Provider 实例标识
resume_cursor    断线重连或事件重放位置，可为空
```

授权决策点固化（ADR-0001）：Broker 在授权决策点从 Registry 取 binding 并固化不可变 `binding_epoch` 与 `provider_instance_id`，写入调用凭证与审计记录并由 Provider 侧校验，不匹配即拒绝或重试；热替换（§13.1 draining→handshake→原子切 binding）只影响后续调用，不得改变在途调用的授权-执行-审计一致性。

`operation_id` 对应一次可恢复的执行状态，而不是某个进程内的临时 Future。Surface 断开连接不等于 Operation 或 Task 被取消；取消必须是显式语义。

## 8. Event Bus：事件和异步消息

总线不应该取代所有 RPC，而应承载已经发生的事实、进度和异步协作。

### 8.1 领域事件

```text
audio.volume.changed
mail.received
stock.price.changed
music.track.started
```

事件表达“发生了什么”，不是“请做什么”。

```text
Query：现在音量是多少？
Command：把音量调到 60%。
Event：音量刚刚变成了 60%。
```

### 8.2 运行时事件

```text
agent.started
agent.completed
agent.failed
agent.cancelled
task.progress
task.completed
plugin.started
plugin.crashed
provider.changed
approval.requested
```

### 8.3 总线原则

- 发布者不需要知道订阅者；
- 订阅者不需要知道发布者内部实现；
- 事件应带来源、版本、关联 Task/Agent 和时间；
- 事件处理应支持幂等和重复投递；
- 总线断线不能导致核心状态不可恢复；
- 事件事实与查询投影分离。

总线应采用“持久事实源 + 内存分发层”的两级结构：

```text
Durable Event Log
  → Bus Router
  → 各订阅者的消费位点/投影
```

内存 Bus 只负责低延迟分发；需要恢复、重放或跨 Runtime generation 交接的事件必须先进入持久事件日志。模型 token 增量、UI 打字机帧和工具进度等瞬态事件默认不落盘；它们丢失后由持久状态或重新订阅恢复，不得被误当成事实源。
每个消费者持有自己的确认位点，重连后从最后确认位置继续消费；允许重复投递，但消费者必须幂等。

持久事件日志由 L2 单写者追加，`event_seq` 是全局单调递增序。这是阶段一即成立的性质，不依赖阶段二：所有投影重建、消费位点和 resume cursor 都以 `event_seq` 为准，时间戳只作参考信息，不参与排序。deadline 同时记录单调时钟起点与绝对 UTC 时间，系统休眠唤醒后按剩余量重算，避免休眠导致误判超时；时间戳一律存 UTC，展示层转本地时区。

### 8.4 日志不是附属功能

对于长期运行的 Agent，日志是判断程序是否做对的主要事实来源，不只是出错后的调试材料。阶段一至少要保存四类记录：

```text
Event Log
  已经发生的持久事实，可重放和恢复

Execution Log
  Agent 回合、工具调用、参数摘要、结果、错误和耗时

Observation Log
  对“Agent 声称完成”和“系统实际观察到”的对照

Evaluation Record
  确定性检查、独立 Judge 评价和人工复盘结果
```

所有记录至少要能关联：

```text
session_id
agent_id
task_id
operation_id
request_id
correlation_id
parent_operation_id
event_seq
timestamp
state
```

日志内容必须脱敏，不能把 API Key、完整凭据或不必要的隐私原文写入普通日志。大型结果和原始输入可以存入受保护的引用位置，日志中只保留摘要、哈希和引用。

日志与产物有保留策略：Event Log 与 Execution Log 默认保留期由配置决定；Artifact 有大小上限和回收规则。用户删除权是个人数据软件的硬需求：删除按范围（单 Task / 单 App 域 / 全局）执行，等于状态删除 + 事件日志墓碑标记 + 投影重建 + 记忆级联失效；墓碑保证消费者回放时不会"删了又长回来"。备份必须加密，脱敏一致性同日志，备份中的删除靠墓碑重放生效。

投影与快照（ADR-0004）：投影自持久事件日志与周期性快照重建；事件日志实行 Kafka log compaction 式压实，**快照/压实为强制义务，不依赖保留期配置**。此项同时废止「仅升级前创建快照」的单一时点要求，§13.7 按此执行。恢复 RTO 指标目标设定：针对单机个人场景，90 天历史冷启动与单调修复 RTO ≤ 30s（WAL autocheckpoint 1000 页定标 + 内存快照镜像兜底）。阶段二多进程形态下，若出现编排空窗期孤儿 Agent，默认执行安全收容与暂停策略，拒绝未授权外部副作用。

## 9. 四种交互语义

系统不应使用一个万能消息格式解决所有问题。

### 9.1 Query

只读、确定性、通常同步：

```text
audio.volume.get
mail.unread.count
stock.quote.get
music.current_track
```

### 9.2 Command

确定性状态改变：

```text
audio.volume.set
music.play
mail.create_draft
calendar.create_event
```

### 9.3 Event

广播已发生事实：

```text
mail.received
stock.price.changed
agent.completed
plugin.crashed
```

### 9.4 Task/Agent Message

需要理解、规划、判断或多步协作：

```text
“分析过去一周邮件中可能影响股票持仓的风险”
“找出最近购买但还没有报销的发票”
```

### 9.5 Operation 与执行收据

Query、Command 和 Task/Agent Message 发起的可追踪执行，统一产生 `operation_id` 和执行收据。执行收据至少记录：

```text
operation_id
request_id
principal / delegation_chain
capability 或 task_type
state
created_at / completed_at
action_summary
result_reference 或 error
```

状态至少区分：

```text
not_started
running
succeeded
failed
cancelled
timeout
outcome_unknown
interrupted
```

其中 `outcome_unknown` 只能通过外部系统核验、用户裁定或明确的恢复流程结束，不能被普通重试逻辑当作 `failed` 处理。

幂等键（ADR-0004）：外部副作用命令必须携带稳定的 Task-step-attempt 幂等键；恢复时对 `outcome_unknown` 的 Operation 先查询/认领/补偿，禁止依据投影推导直接重发命令。

### 9.6 Approval 状态机

Approval 不是一次性弹窗，是持久合同对象：

```text
requested → waiting_user → approved | denied
                ├── expired（超时，等价 denied）
                └── withdrawn（调用方取消）
```

```text
默认拒绝：超时即 denied，任何配置都不允许"超时默认同意"；
授权范围由用户在批准时选择：
  once            仅本次 Operation
  task:<id>       本 Task 内同类调用
  count:<n>       本 App 内 n 次
  ttl:<duration>  时限内同类调用
  forever         永久授权：二次确认 + 随时可撤销 + 每次使用都审计
审批请求持久化，Runtime 重启后恢复；
GUI 关闭时经 Notification Surface 提醒，CLI 可 list / approve；
等待审批的 Operation 处于 waiting_approval，denied → cancelled。
```

### 9.7 Budget 对象

预算是挂在 Agent 和 Task 上的合同对象，维度开放，以键值对扩展、老版本忽略未知键：

```text
budget:
  max_tokens / max_cost / max_wallclock_ms
  max_tool_calls / max_turns
  max_sub_agents / max_depth / max_concurrent_tools
  <extension>: <value>
```

强制点有三层，缺一不可：

```text
回合开始：       剩余预算预估，不足则不发起回合
工具调用前：     已用量加本次预估对比上限，超限拒绝并发布 budget.exceeded
模型/工具返回后：实际记账，更新 Agent 与 Task 两级账本
```

```text
软限（默认 80%）→ budget.warning 事件
硬限 → Agent 暂停，Task 进入 blocked(budget_exhausted)，请求用户裁定
追加预算是受控变更：只能由用户批准；Coordinator 不能扩大预算
```

预算二分（ADR-0002）：Coordinator 可在 Task 预算包络内向成员子分配预算（逐笔记账、仅在包络内分配），不可扩容；包络扩容仅限用户批准；成员重试受 manifest retry 策略与成员级预算双重约束，Broker 为唯一执行点。

### 9.8 统一错误信封

Wire API 的所有错误使用统一信封；核心错误码封闭，扩展码用命名空间追加：

```text
error:
  code:            validation_failed | permission_denied | approval_required
                 | approval_denied | unavailable | timeout | cancelled
                 | budget_exceeded | idempotency_conflict | outcome_unknown
                 | internal | <namespace>.<extension>
  message:         脱敏的人类可读信息
  retryable:       bool
  retry_after_ms:  可选
  detail_ref:      结构化详情的受保护引用
```

CLI 退出码映射：validation_failed 与 idempotency_conflict → 2；权限与审批拒绝 → 3；approval_required → 4；执行失败与预算耗尽 → 5；outcome_unknown → 6；unavailable 与 timeout → 7。重试语义由 Capability manifest 的 retry 字段统一声明，Broker 是唯一执行者。

## 10. Butler App

管家保留为一个真实的 App，但它不是拥有所有权限的超级 Agent。

### 10.1 Butler 的权限

Butler 默认拥有较高的协调权限：

```text
task.create
task.cancel
agent.spawn
agent.pause
agent.resume
agent.stop
agent.watch
team.create
capability.discover
event.subscribe
task.collect
capability.call
```

但默认不拥有：

```text
mail.read
mail.send
stock.place_order
music.control
任何其他 App 的原始数据库访问权
```

因此：

> Butler 拥有系统协调权，不自动拥有其他领域的操作权。

capability.call 的边界（ADR-0002 修订）：作为协调权行使时，仅能引用 manifest 已批准的能力清单、资源谓词与风险等级，不得作为泛化逃生舱；来源含 untrusted 且风险等级 reversible 及以上时强制 approval_required；低风险确定性能力可按 task:<id> 作用域批量预授权。

### 10.2 Butler 调用其他 App 的两条路径

#### 路径 A：直接调用公开 Capability

适合低延迟、确定性、结构化操作：

```text
Butler Agent
  → Broker
  → mail.search
  → Mail Provider
  → 结构化结果
```

这里的“直接”是调用 App 暴露的受控 API，不是绕过 Runtime 访问数据库。

#### 路径 B：请求领域 Agent

适合复杂理解、领域判断和多步推理：

```text
Butler Agent
  → Broker / Agent Service
  → Mail App Agent
  → Mail Agent 使用 mail.search/mail.read
  → 生成领域结果
  → Butler Agent
```

两条路径共存：

```text
确定性操作 → Capability
复杂推理   → Domain Agent
```

### 10.3 Butler App 与 Orchestrator 的边界

Butler 是一个真实的 App、身份和可接入的 Surface；Orchestrator 是通过公开 wire API 工作的编排客户端。二者可以由同一产品功能提供，但不能在架构上混为一个拥有内核特权的进程：

```text
Butler App
  → SYSTEM / Butler 会话
  → 公开 wire API
  → Runtime Core
```

L2 持有 Task 的规范状态、成员关系、预算、截止时间和生命周期事件。Orchestrator 可以持有 Task Board、编队策略和任务卡片投影，但不能以自己的数据库代替 L2 的规范状态。Orchestrator 崩溃时，Task 和 Agent 会话继续由 Runtime 监督；Orchestrator 恢复后从持久状态和事件日志重建投影。

恢复语义（ADR-0004；条件 6 于 M5 结算）：Orchestrator 恢复 = 先结算未决意图，再从最近一致的持久状态与决策事件重新推理下一步；明确声明不重放 LLM 内部推理过程；Runtime 仅承担会话监督。编排重启的触发者恰为二者：①用户显式 resume（任意 Surface 的 task.resume）；②Watchdog 自动触发——监护判定停滞成立后持久发布事实事件 `watchdog.reorchestration.triggered`，由编排器消费后重新推理；Watchdog 与 Runtime 监督层均不推断编排下一步。停滞窗口：无进展信号（无新事件/无心跳更新/无 Operation 状态变化）持续超停滞阈值（默认 15 分钟，Task 可配置）判定 stalled 并触发自动重启；自最近进展累计超硬顶（默认 24 小时）不再自动重启，Task 转 blocked 等待用户裁定；waiting_approval 态豁免自动重启。数值与机制详见 `milestones/M5-implementation-spec.md` §5.2。

## 11. Agent Team 与 Coordinator Agent

“队长是管家的分身”可以作为产品理解，但内核中不应复制 Butler 的完整身份。

更准确的定义是：

> Butler 为某个 Task 创建一个具有受限协调权限的 Coordinator Agent。

```text
Butler Agent
  │
  └── Task: 分析邮件中的股票风险
          │
          └── Coordinator Agent
                 ├── Mail Agent
                 ├── Stock Agent
                 └── Report Agent
```

### 11.1 Coordinator 的能力

```text
创建成员
发送任务
调用公开 Capability
收集结果
设置截止时间
重试或替换成员
控制 Task 预算
提交报告
```

### 11.2 Coordinator 的限制

```text
不自动继承 Butler 的全部权限
不能读取所有 App 私有数据
不能把成员权限无限转授
不能绕过 Broker
不能扩大 Task 预算
```

子树裁剪（ADR-0002）：Coordinator 的协调动词按其所属 Task 子树裁剪——task.cancel/agent.pause/agent.stop/task.collect/team.create 仅可作用于本 Task 子树内的成员与子任务，作用域绑定 §3 的「当前 Task」与「父子归因」字段；子树外目标一律默认拒绝。

协调权二分（ADR-0002）：协调权细分为 safe_coordination（只读查询、状态查询、结果收集，可默认继承）与 mutation_coordination（生命周期控制与团队组建：cancel/pause/stop/agent.spawn/team.create，须在 Task 授权中显式列出，不可默认继承）。据此，Coordinator 是含受限判断行为（创建成员、重试或替换成员）的**非确定性控制面实体**，其安全由交集、预算、默认拒绝审批与子树裁剪等补偿控制围堵而非消除（不采用「确定性有界状态机」类表述）。

### 11.3 权限计算

Coordinator：

```text
Coordinator 权限
= Butler 可授予的协调权限
∩ 当前 Task 授权
∩ 用户授权
```

Domain Worker：

```text
Worker 权限
= 所属 App 能力
∩ 当前 Task 授权
∩ 成员角色授权
```

团队共享：

```text
Task Board
Task Messages
Artifacts
Reports
Progress Events
```

团队不默认共享所有 App 原始记忆和数据库。

Grant 物化（ADR-0002）：三方交集的计算结果必须物化为经 Broker 记账的 Approval/Grant 绑定记录（作用域 task:<id>、默认拒绝、可撤销、重启可恢复）；Grant 字段含 audience、action、资源谓词、delegation_depth=0（不可再转授）、过期时间、撤回版本与父授权哈希；成员角色授权的签发者定义为 Coordinator，且不得超过其自身 Grant 上界。

## 12. Plugin 与 App 的运行模型

“万物皆插件”不等于“所有东西都是网络微服务”。需要同时区分组合、替换、隔离和分布式：

```text
可组合     = 能否装入系统并协同工作
可替换     = 能否换实现而不改调用方
可隔离     = 崩溃/卡死是否局部化
可分布式   = 能否独立进程或远程运行
```

实现热替换和故障恢复需要：

```text
稳定能力合同
生命周期管理
状态外置
进程隔离
版本兼容
超时与取消
故障恢复
```

### 12.1 Runtime 内进程实现

适合：

```text
IPC Broker
Identity
Capability Policy
Registry
Scheduler
Task State
Event Store
Protocol Codec
```

优点：低延迟、低内存、Rust 类型和资源控制较强。

缺点：进程内组件不能独立崩溃和热替换；其中任意组件改变，通常需要重启或切换整个 Runtime generation。

**Supervisor 不属于这组进程内组件。** 它位于 L0，必须能够在 Runtime Core 失败时仍然存活，负责启动、监控、排空、代际切换和回滚。

### 12.2 独立进程 Provider/App

适合：

```text
第三方连接器
Python 量化模块
浏览器自动化
不稳定 SDK
复杂解析器
外部服务接入
```

优点：崩溃隔离、独立升级、支持多语言。

缺点：需要处理 IPC、断线、超时、版本和状态恢复。

两种实现对调用方暴露同一个 Capability 合同：

```text
broker.call("mail.search", params, context)
```

### 12.3 插件与 MCP 的信任模型

插件 manifest 必须声明权限，安装与升级是权限决策点：

```text
manifest 声明：capabilities、data_domains、process（进程内或独立）、
              所需 secret 引用
生命周期：registered → installed → enabled ⇄ disabled → uninstalled
                                        ↘ quarantined（崩溃过多自动隔离）
安装与升级：展示权限 diff，用户批准后生效；
           升级导致权限扩大必须重新批准，缩小权限静默生效
签名：阶段一至少做来源标识（官方源或第三方），签名校验字段预留
```

MCP Server 是外部 Provider 的一种，遵循同一条信任链：

```text
MCP tools → Capability Registry 注册（命名空间 mcp:<server>.<tool>）
风险未知的一律按不低于 reversible-command 对待，首次调用需审批；
崩溃与挂起走 13.2 的 Provider 崩溃路径
```

## 13. 热插拔、升级与故障恢复

### 13.1 Provider 热替换

```text
Provider A
  → Registry 标记 draining
  → 不接受新请求
  → 等待短请求完成或按 deadline 取消
  → Provider B handshake
  → 校验能力合同和健康状态
  → Registry 原子切换 binding
  → 发布 provider.changed
  → 停止 Provider A
```

调用方始终只看到稳定的 Capability 名称。

### 13.2 Provider 崩溃

```text
Provider 崩溃
  → Supervisor 检测
  → Registry 标记 unavailable
  → Bus 发布 provider.crashed
  → 新请求得到明确 unavailable
  → Supervisor 重启
  → 重新 handshake/register
  → Registry 恢复 binding
```

不能让调用方无限等待。

### 13.3 副作用操作

邮件发送、下单、文件写入等操作不能因为进程重启就盲目重放，必须支持 9.5 的完整 Operation 状态机（含 timeout 与 interrupted）；副作用恢复只关心其中三个状态：

```text
succeeded          已确认完成，不重放
failed             可进入重试决策
outcome_unknown    先核验外部系统，不自动重放
```

如果结果未知，恢复流程应先查询外部系统：

```text
确认未执行 → 可以重试
确认已执行 → 继续任务
无法确认   → 请求用户裁定
```

### 13.4 Runtime Core 的升级

Runtime Core 不能像普通 Provider 一样只替换一个函数，也不应覆盖正在运行的旧二进制。升级单位是新的 Runtime generation，由 L0 Supervisor 和 Upgrade Manager 管理：

```text
Runtime v1 / generation 41 = active
        │
        ├── 安装 v2 到独立版本目录
        ├── 校验签名、依赖、L1 合同和迁移路径
        ├── 创建状态快照/确认事件日志检查点
        ├── 启动 v2 / generation 42 = validating
        ├── v2 在隔离状态副本中只读恢复 Registry、Agent、Task 和权限状态
        ├── 验证期禁止真实外部副作用，只允许模拟/沙箱/只读探针
        ├── 健康检查、协议检查、恢复检查通过
        ├── 原子切换 active generation
        ├── v1 = draining，排空或取消旧请求
        └── 停止 v1，保留回滚材料
```

升级失败时：

```text
v2 验证失败或启动崩溃
  → active generation 仍保持 v1
  → 清理/隔离 v2 的临时状态
  → 记录 upgrade.failed
  → v1 继续服务
```

切换完成后若发现迁移或运行异常：

```text
停止 v2 写入
  → 根据 generation 事务和状态快照回退 active 指针
  → 若迁移可逆则执行反向迁移；不可逆迁移则恢复迁移前快照或使用兼容读取层
  → 恢复 v1
  → 记录 rollback.completed
```

回退边界（ADR-0003）：active 指针回退仅对可执行工件、只读配置与本地指针原子有效；仅当状态格式未发生不兼容变更（或存在声明兼容读取层）且切换后未产生状态分叉时方可自动回退；不可逆迁移已执行或状态已分叉时，只能经迁移前快照恢复或人工维护窗口回退，禁止以指针回退制造旧代码读新事实的静默损坏。

双代际期间必须有写入栅栏：同一 Agent、Task、Registry 或事件日志不能被 v1/v2 同时写入。L0 通过 generation lease/单写者租约或等价机制确认当前写入者；仅有“两个进程都启动成功”不能视为切换完成。术语定位（ADR-0003）：「单写者租约」统一为 lease/fencing 机制（排他写权+过期栅栏），不采用「consensus 原语」表述；其保证范围限于本地写权排他与旧代际禁写，不提供远端副作用确认或撤销；租约异常时拒绝授予新租约并冻结写入等待人工介入。

模型正在生成的瞬态流不要求无缝续接；应将未完成回合标记为 `interrupted`，由新代际按恢复规则继续或重新执行。

升级恢复的目标是**语义连续**，不是保证每个瞬间的内存状态连续：

- 已提交的 Agent、Task、权限、事件和记忆状态必须可恢复；
- 正在生成的 token、打开的网络连接和临时内存对象可以丢失；
- 不能安全判断结果的外部副作用必须进入 `outcome_unknown`，不能自动重放；
- 新代际恢复后应向 Surface 发布 `generation.changed` 和必要的 `agent.interrupted`/`task.recovered` 事件。

语义连续的边界与频率约束（ADR-0003）：升级目标是会话级连续；in-flight 回合标记 agent.interrupted 并自持久状态恢复；升级频率纳入策略约束（升级窗口合并、最低升级间隔）；连续性承诺不依赖内存连续，且 draining 范围与 interrupted 宽松度不得随实现放宽。

升级事务至少应记录以下阶段，并且阶段状态本身归 L0 持久化：

```text
prepared     已下载/安装，尚未接管
migrating    正在对快照或新代际专属状态执行迁移
validating   新代际已启动，只读恢复并接受健康检查
committing   已取得单写者租约，准备提交 active 指针
active       新代际已成为唯一写入者
draining     旧代际只读排空
rolled_back  已恢复旧代际
failed       升级失败，等待清理或人工处理
```

迁移必须优先写入新代际专属的临时/版本化状态，验证通过后再提交 active 指针；不能在旧代际仍 active 时直接原地改写其唯一状态库。迁移脚本必须声明输入版本、输出版本、是否可逆、失败清理方式和校验方法。

切换后观察窗 probation（ADR-0003）：新代际切换后进入观察窗，健康判据至少包括——规定时限内完成一次成功的 Agent 会话恢复、Provider binding 确认、租约正常续约、无未处理 error event；判据满足视为升级成功（对标 Android markBootSuccessful）；状态未分叉的异常触发自动回退，状态已分叉的异常冻结写入并转人工处置；判据未在基线定义前不得启用自动回滚。

外部副作用前置（ADR-0003）：触及真实 Provider 的升级，validating 通过不构成自动切换的充分条件；须叠加幂等键、Provider 查询/对账合同或升级前生产探针中的至少一项；不可证明副作用的升级标记为人工维护窗口并保留旧版本，不得宣称自动回滚——§13.3 的核验流程由此前置为升级门槛。

### 13.5 升级级别

```text
Patch
  L1 合同、状态格式和权限语义不变；可重启切换，通常可自动回滚

Minor
  只新增向后兼容的字段/能力/事件；旧客户端可继续工作

Major
  IPC、权限、事件语义或状态格式不兼容；必须经过迁移、维护窗口或明确的兼容桥
```

真正类似“Windows 10 → Windows 11”的，是 L1 根合同、身份/权限根语义或不可兼容的持久状态格式发生变化，而不是普通 Runtime Core 修复。

分层适用（ADR-0003）：完整 generation 升级流程（双代际+隔离 validating）适用于不可兼容 L1/L2 语义升级（L1 Major）与 Runtime Service 级升级；L1 Minor 与 L2 插件默认走 §13.1/§13.6 局部 draining/原子 binding 替换，且局部路径强制同等单写者 fencing 与排空保证。

### 13.6 局部升级与系统级升级对照

| 升级对象 | 影响范围 | 推荐机制 |
|---|---|---|
| Surface | 单个交互界面 | 替换前端/适配器，不动 Agent 状态 |
| App | 单个业务域 | 独立进程替换，迁移 App 私有状态 |
| Provider/驱动 | 单项 Capability | draining + 新 Provider handshake + 原子切换 binding |
| Runtime Service | L2 的一组核心服务 | 新 generation 验证、切换、排空、回滚 |
| L1 根合同 | 全系统基础语义 | 维护模式/兼容桥/状态迁移，属于重大系统升级 |

### 13.7 Upgrade Manager 的职责

Upgrade Manager 属于 L0 控制面，不承载业务 Agent。它负责：

- 管理 Runtime generation 和 active 指针；
- 保存版本、合同、迁移和回滚元数据；
- 维护快照/事件日志压实的持续义务（§8.4，ADR-0004），升级前确认检查点就绪；
- 启动验证代际并执行健康检查；
- 施加“同一状态单写者”约束；
- 处理排空、取消和长连接交接；
- 升级失败时恢复旧代际；
- 只在验证成功后提交 active 切换。

Upgrade Manager 和 Supervisor 可以是同一个 L0 小程序，也可以是两个模块；但它们不能依赖待升级的 L2 Runtime 才能完成回滚。

## 14. Surface 与核心解耦

Runtime 不输出“聊天界面协议”，而输出底层能力：

```text
创建/唤醒 Agent
发送 Agent 输入
订阅 Agent 状态
获取结构化结果
订阅工具进度
提交审批决定
创建/查询/取消 Task
调用公开 Capability
```

不同 Surface 只是不同适配器：

```text
聊天窗口      Conversation Surface
命令行        CLI Surface
搜索框        Query Surface
语音互动      Voice Surface
通知中心      Notification Surface
邮件侧栏      Mail Surface
股票工作台    Trading Surface
```

因此：

```text
Surface 不拥有 Agent
Agent 不拥有 Surface
Agent 不直接拥有 App 数据库
App 不直接拥有 Runtime 内核
```

后台向前台提供的不是“聊天专用接口”，而是统一的：

```text
RPC / Query / Command
Event / Progress
Task / Agent stream
Approval
```

### 14.1 CLI Surface

CLI 是基础发行物中必须存在的标准 Surface，用于在没有 GUI、GUI 尚未安装或 GUI 本身故障时检查和操作系统。它与其他 Surface 共享 L1 Surface Protocol，不直接读数据库、不直接连接 Provider，也不绕过 Capability Broker。

CLI 至少分为两组命令：

```text
用户操作面：
boenmind task create
boenmind task list
boenmind task watch <task-id>
boenmind task cancel <task-id>
boenmind task logs <task-id>
boenmind agent list
boenmind call <capability>
boenmind approval list
boenmind approval approve <approval-id>

运行控制面（阶段二交付；阶段一仅提供 status / doctor / logs）：
boenmind runtime status
boenmind runtime doctor
boenmind runtime logs
boenmind runtime generation      # 阶段二
boenmind runtime drain           # 阶段二
boenmind runtime upgrade         # 阶段二
boenmind runtime rollback        # 阶段二
```

运行控制面通过受限的 L0 Control Protocol 访问 Supervisor / Upgrade Manager；即使 L2 Runtime Core 不可用，也必须能够执行状态查看、排空、回滚和恢复所需的最小操作。L0 不能因此获得邮件发送、下单等业务能力。

CLI 的异步操作必须区分提交、观察、取消和重新连接：

```text
boenmind task run "分析本周邮件中的股票风险"
  → task_id + operation_id

boenmind task watch <task-id> --since <resume-cursor>
  → 从事件游标继续观察
```

CLI 退出或网络断开默认只断开 Surface，不取消 Task。`watch` 必须支持事件游标、重复投递去重和背压；人类输出与机器输出至少分为普通文本、JSON 和 JSONL 三种模式。命令帮助、参数校验和补全应从 Registry 的 Capability 合同生成。

建议固定基础退出码：

```text
0  成功完成或成功提交
1  未分类内部错误（不应出现，出现即为 bug）
2  参数或合同错误
3  权限拒绝
4  需要审批
5  Task/Operation 执行失败
6  outcome_unknown，不能安全判断副作用结果
7  Runtime 或 Provider 不可用
```

### 14.2 Surface 接管与降级

Surface 不拥有 Task。一次交互可以从 Chat 创建 Task，随后由 CLI 或 Tauri 接管：

```text
Chat  → 创建 Task 123
CLI   → attach Task 123
Tauri → 继续观察 Task 123
```

Surface 只持有 Session、订阅和 resume cursor；Task、Agent、Operation 和 Approval 状态由 Runtime 持久化。

基本降级矩阵如下：

```text
Runtime 进程不可用     → 无服务；重启后从持久状态与事件日志恢复
                      （阶段二独立 L0 存在时升级为：L0 状态、日志、
                       generation、排空、回滚仍可用）
Provider 不可用       → 其他健康 Capability 继续工作
Event Bus 暂停        → 核心状态可恢复提交，订阅从事件位点补发
Model Provider 不可用 → Query、低风险确定性 Capability 仍可用
持久化只读            → 允许查询，禁止新的副作用和不可恢复写入
GUI 不可用             → CLI 继续提供用户操作和运行控制
```

接管协议（ADR-0004）：取得接管权时由 L2 递增 task_epoch；所有编排命令携带 epoch 并经 CAS 校验与租约门禁；过期 epoch 命令返回可审计的 stale-command 结果。

## 15. 推荐核心拓扑

拓扑唯一权威是 `architecture/boenmind.c4`（详见 §24，ADR-0008）。本节不再复制图形——2026-08-28 之前的 ASCII 文字拓扑已声明降级为非权威并移除；文字与模型不一致时，以模型为准。已裁决的形态增量（正文随 ADR 熔入）：部署形态 = 本机单进程或自管 VPS 托管（ADR-0009）；Web UI Surface = assistant-ui 自建壳 `runtime/webapp`（ADR-0013 弃 dsh 复刻、ADR-0014 定 assistant-ui 路线）；真实 App 与 MCP 工具以进程外 stdio server 接入（ADR-0011）。C4 模型与实现的已知漂移台账见 `milestones/BACKLOG.md`。

本节保留两条恒定规则：

1. **CLI 双协议边界**：CLI 对 L0 的运行控制请求走单独的 Control Protocol；对 L2 的用户操作请求仍走 Surface Protocol 和 Capability Broker。两条路径可以由同一个 `boenmind` 可执行文件提供，但合同、权限和审计边界必须保持分离。
2. **跨域调用统一入口**：Caller → Capability Broker → Registry → Policy → Provider，不存在第二条绕行通道。

L0-L5 分层与各部件职责见 §2.1；容器级拓扑、部署环境与动态视图见 `architecture/boenmind.c4`（§24）。

## 16. 核心规则总表

```text
能确定执行的事情       → 直接调用 Capability
需要自然语言理解的事情 → 交给 Agent
需要多 Agent 协作      → 创建 Task + Coordinator Agent
需要广播状态           → 发布 Event
需要查询当前状态       → Query
需要执行确定性改变     → Command
需要替换实现           → Registry 原子切换 Provider
需要隔离崩溃           → 独立进程
需要高性能             → Rust 进程内或 Rust Provider
需要多语言生态         → 独立进程协议
需要无 GUI 操作        → CLI Surface + L0 Control Protocol
需要跨界面接管         → 持久 Task/Operation + Session cursor
需要新增功能           → 优先做成 Provider/App/Skill 插件，不改内核
新功能能否优雅接入     → 回答 2.3 的六个扩展点问题
```

## 17. 当前架构裁决

推荐采用：

```text
Rust Runtime Core
Runtime Registry
Capability Broker
Event Bus
多 App 隔离
Agent Registry
Task/Team/Coordinator
L0 Bootstrap/Supervisor/Upgrade Manager
Runtime generation、状态迁移与回滚
与前端无关的 Surface Protocol
```

其中：

- **Registry** 负责“谁提供什么”；
- **Broker** 负责“能不能调用以及调用谁”；
- **Bus** 负责“发生了什么、进度如何、异步消息如何传播”；
- **Agent** 负责理解、规划、判断和协作；
- **Capability** 负责确定性动作和数据访问；
- **Butler App** 负责高权限协调，不默认拥有所有领域操作权；
- **Coordinator Agent** 是某个 Task 的受限队长，不是 Butler 的完整复制品；
- **App** 是领域和安全边界；
- **Surface** 是可替换的交互适配器；
- **插件进程** 是实现热替换、崩溃隔离和多语言支持的主要手段。
- **L0 Supervisor/Upgrade Manager** 是独立于待升级 Runtime 的最小控制面；
- **Runtime Core** 通过 generation 进行升级、验证、切换、排空和回滚；
- **L1 根合同**决定什么是局部升级，什么必须进入系统级重大升级。
- **CLI** 是 L5 的标准 Surface，也是无 GUI 时的人类控制入口；其 L0 控制命令与 L2 业务调用分属不同协议和权限边界；
- **Task 的规范状态**归 L2，Orchestrator 的任务板是可重建的投影，不是第二个事实源；
- **Session、Operation、Approval、Artifact 和 Projection** 将接管、执行追踪、审批恢复和多 Surface 查询从隐含行为提升为可持久化合同对象；
- **内核由合同与最小机制组成**：模型连接器、记忆引擎、定时器、评价器、Secret Store 都是扩展点上的普通 Provider，不是内核特权组件；
- **新增功能默认走插件路径**：只有既有扩展点无法表达时才允许改合同，并按 13.5 的升级级别处理。

最终原则：

> **总线负责“发生了什么”和异步协作；注册表负责“谁提供什么”；Broker 负责“能不能调用以及调用谁”；Agent 只在需要理解和规划时介入；低风险、确定性的公开能力直接走 Capability Broker。**

### 17.1 裁决复核（2026-08-28）与增补熔入

五条核心裁决（R1-R5）经 Zen consensus 三模型辩论复核：glm-5-turbo、gpt-5.6-luna、gemini-3.7-flash 三个模型家族分任架构师（钢人论证）、挑战者（安全与可靠性反驳）、实证研究者（以真实系统证据裁决），角色跨裁决轮换，两轮辩论（独立立场→交叉质证）+逐裁决合成。结论：R1 三权分立有条件维持，R2-R5 修订，无一条被推翻；辩论新增两条裁决——**ADR-0006 权限以合同显式化（元原则）**与 **ADR-0007 L0 自举豁免与升级信任链**。逐裁决结论、共识比分与条件见 `adr/README.md` 索引与各 ADR 文件；全程转录见 `architecture/debates/`。

三大结构性张力如实记录，供后续里程碑回看时优先审视：①权力分立与协调效率互斥；②极简内核与治理完备互斥；③唯一真源与投影时效互斥。

各 ADR 对基线的修订（涉及 §2.2／§6.4／§7／§8.4／§9.5／§9.7／§10.1／§10.3／§11.2／§11.3／§13.4／§13.5／§13.7／§14.2）已熔入对应正文并标注 ADR 编号；ADR-0015 起这是对基线增补的唯一维护方式（不再使用追加式引注块）。外部系统对照验证结论与 S1-S10 修订建议见 §24 及 ADR-0008；S1-S10 的裁决状态台账见 `milestones/BACKLOG.md`。

## 18. 里程碑：阶段一 M0-M8 与阶段二批次

必须按可运行检查点推进，而不是按“模块写完”推进。每个里程碑完成后，都要回头评估此前的设计和实现，再决定是否进入下一个里程碑。阶段二批次（M9 起）与 W 序列（WebUI）同样按可运行检查点推进；本节只保留各里程碑的范围定义与通过条件，交付状态（日期/tag/测试数/结论/遗留）统一记录于 `milestones/HISTORY.md`，未结事项统一记录于 `milestones/BACKLOG.md`。

### M0：范围、合同和测试基线

```text
M0.1 冻结阶段一范围、阶段二非目标与扩展点清单（2.3 六问）
M0.2 定义 Wire API、统一错误信封、事件 Schema 和 Operation 状态机
M0.3 定义 Windows / Linux / macOS 测试矩阵
M0.4 建立脱敏日志、事件回放和测试样例（含提示注入用例集）
M0.5 威胁模型与数据信任分级
M0.6 性能与资源基线定标
```

通过条件：合同可机器校验，日志可以读取和回放，且至少有一条端到端轨迹作为后续回归基线；扩展点六问与非目标清单生效，成为此后每个新功能的准入检查。

交付记录（2026-08-28，tag `m0.2-contracts-frozen`）：M0.1-M0.6 全部交付——§1.1 非目标清单与 §2.3 扩展点六问即此后每个新功能的准入检查；合同库冻结 v1.0（`boenmind-contracts`，彼时 9 个 JSON 带 `x-frozen` 注解，随后续里程碑只增）；M0.3-M0.6 工件在 `boenmind-contracts/m0/`——三平台测试矩阵、提示注入用例集（PI-01..12）、威胁模型与数据信任分级（T-01..12）、性能与资源基线定标（P-01..08，数值由 M1 以 mock 模型回填）。

### M1：最小 Runtime 与单 Agent 闭环

```text
M1.1 Runtime 启动和停止
M1.2 Session 创建、恢复和关闭
M1.3 单 Agent 回合和模型调用
M1.4 基础错误、取消和超时
M1.5 第一版 Execution Log
M1.6 最小 Secret Store，模型凭据不进上下文和日志
M1.7 模型连接器合同、调用账本与降级链
M1.8 预算记账与强制点最小版
```

通过条件：Agent 能完成简单任务；界面断开不会损坏 Session；输入、输出、工具调用和错误都能关联到 Session、Agent 和 Operation。

### M2：持久化、事件日志与崩溃恢复

```text
M2.1 SQLite 规范状态
M2.2 Append-only Event Log
M2.3 Snapshot 和恢复
M2.4 Event Replay
M2.5 Operation 状态机
M2.6 outcome_unknown 处理
M2.7 全局 event_seq 单调序与 resume cursor 语义
```

通过条件：强制终止后可以恢复 Session、Task 和 Operation；已完成操作不会因重启自动重复；重复投递不会破坏投影；未知副作用不会被当成普通失败。另含四项混沌测试（ADR-0004，作为 R4 的可证伪验收前置）：杀 Orchestrator 后 CLI attach、损坏本地任务板库、同 event_seq 前缀重建确定性校验、旧 epoch 命令拒绝。

### M3：统一 Wire API、CLI 和跨平台启动

```text
M3.1 Surface Protocol
M3.2 CLI 的 session / task / agent / approval 命令（含 task logs）
M3.3 watch 和 resume cursor
M3.4 Tauri Desktop 最小界面
M3.5 Windows / Linux / macOS 打包和启动
M3.6 跨平台路径、进程和权限适配
```

通过条件：GUI 和 CLI 使用同一套 Runtime API；CLI 退出不会默认取消 Task；三个平台都能安装、启动、执行、退出和恢复；状态和日志语义一致。

### M4：Capability、Broker、权限和审批

```text
M4.1 Capability Registry
M4.2 Capability Broker
M4.3 输入输出 Schema 校验
M4.4 身份与权限交集
M4.5 Approval 持久化和恢复
M4.6 审计事件和调用归因链
M4.7 审批状态机与授权范围（once / task / count / ttl / forever）
M4.8 input_trust 信任分级门控
```

通过条件：所有能力调用经过 Broker；GUI、CLI 和 Agent 不能绕过权限；高风险能力需要审批；审批中断后可以恢复；审计记录能追溯调用者、Task 和 Operation。

### M5：Butler、Task 和长期监护

```text
M5.1 Butler 作为内置 App
M5.2 Task 创建和生命周期
M5.3 Coordinator Agent
M5.4 Task Board Projection
M5.5 watch / pause / resume / stop
M5.6 Watchdog 和 Observation Log
M5.7 记忆作用域与 memory.* Capability（实现可替换）
```

通过条件：Butler 只有协调权限；Task 状态不依赖 Butler 内存；长期任务能发现无进展、重复动作和持续错误；Agent 声称完成的结果能够被实际观察和核验。

### M6：Team、Delegate 和多 Agent 协作

```text
M6.1 Team 定义
M6.2 成员 prompt 和工具授权
M6.3 Agent spawn
M6.4 delegate 归因链
M6.5 预算、深度和并发上限
M6.6 结果收集和报告
```

通过条件：成员权限只减不增；委派受深度、预算和并发约束；成员故障不会破坏整个 Task；团队结果有来源、状态和关联 Operation。

### M7：Provider、MCP 和 App 隔离

```text
M7.1 内置 Capability Provider
M7.2 MCP Server 接入
M7.3 Provider handshake 和能力发现
M7.4 Provider 崩溃、重启和 unavailable
M7.5 Provider 进度、超时和取消
M7.6 App 数据域隔离
M7.7 插件与 MCP 信任：manifest 权限声明、安装批准、未知风险首次调用审批
```

通过条件：调用方只依赖 Capability；MCP Provider 可以发现、调用和报告进度；Provider 崩溃不会拖垮 Runtime；失败调用不会无限等待；App 不能通过内部数据库绕过 Broker。

### M8：首批真实 App 与发行质量

```text
M8.1 Wiki App
M8.2 股票或其他确定性领域 App
M8.3 多 Surface 协作
M8.4 长任务压力测试
M8.5 数据迁移、备份和恢复
M8.6 三平台发布包
M8.7 独立 Judge 和评估报告
M8.8 数据保留期、用户删除与墓碑回放验证
```

通过条件：至少两个真实 App 使用同一套 Runtime、Broker、Task 和日志机制；长任务可以回放和评估；关键副作用有执行收据；三平台完成端到端回归；历史会话不因发布和迁移损坏。

### M9：阶段二第一批——记忆抽屉授权、模型真流式、worker 自主环 v0

范围：memory:user 显式授权执行面（Broker 裁决步升级审批 + Grant scope 谓词）；模型连接器真流式（SSE 打字机）；autorun worker 自主环初级版。通过条件：实网流式联通；真实浏览器端到端手测通过；授权与抽屉隔离有可证伪测试。规格与回看：`milestones/M9-implementation-spec.md`、`milestones/M9-review.md`。

### 全面回看 M1-M9

整体回看门：四道门禁全绿（260 测试）；新发现 F-01..F-11 入审计台账；条件：C4 模型回写（F-06）列为阶段二下一批开工前置。记录：`milestones/FULL-REVIEW-2026-08-30.md`。

### W 序列（WebUI，ADR-0014）：W1-W9

以 assistant-ui 组件库自建 Web 壳（`runtime/webapp`，Vite+React+TS），后端 OpenAI 兼容插座 `/v1/chat/completions`（SSE 流式）；约束：每个组件组必须在 assistant-ui 找到原型（W1 规格 §5）。W1 = 壳与流式对话；W2 = 设置中心/provider 库/工作区/可拖布局 + webadmin 管理面（壳子私用，暂不入冻结合同，行为规格 = webadmin_tests）；W3 = 两级主题系统（四主题 + 每主题设置项）；W4 = 对话工具闭环（tools 合同启用 + 直通工具注入）与角色 system prompt；W5 = 会话记忆回喂与上下文透视面板；W6 = 对话级模型选择与常用清单；W7 = 关于页与在线升级通道；W8 = 常规设置与工作区绑定；W9 = 轨迹视图与跨会话检索。惯例：W 序列验收记录并入各规格的验收门小节，不另立 review 文件（ADR-0015）。

## 19. 每个里程碑都必须回看、测试和评估

每个里程碑（M 序列与 W 序列批次）结束后，必须执行以下回看门。没有完成回看，就不算里程碑完成。

```text
A. 功能测试
   验证本里程碑新增行为。

B. 回归测试
   验证此前里程碑的行为没有被破坏。

C. 故障测试
   测试断网、进程终止、超时、重复投递、Provider 崩溃、
   Surface 断开和状态恢复。

D. 日志回放
   从 Event Log 和 Execution Log 重建实际执行过程。

E. 确定性评估
   检查 Schema、状态机、权限、Operation 和外部结果。

F. LLM 评估
   必要时使用独立 Judge 判断轨迹是否符合任务目标，tools: []。

G. 架构复盘
   检查是否出现新的事实源、绕过 Broker、重复逻辑或不合理耦合。

H. 验收裁决
   通过、带条件通过、退回修改或阻塞。

I. 性能冒烟
   对照 M0.6 基线检查启动、内存和调用开销无明显劣化。
```

每次回看至少回答：

```text
1. 新增能力是否真的解决了目标问题？
2. 旧能力是否仍然可用？
3. 崩溃、断线和重复执行时会发生什么？
4. 日志能否解释程序做过的每一步？
5. 结果是否被实际观察和核验，而不是只听 Agent 自述？
6. 当前合同和状态模型是否仍然稳定？
7. 是否应该继续推进，还是退回修改？
```

每次回看产生一条 Evaluation Record：

```text
milestone_id
build_or_commit_id
test_run_id
log_range
deterministic_checks
failure_tests
replay_result
llm_evaluation
known_failures
architecture_changes
acceptance_decision
reviewed_at
```

`acceptance_decision` 只能是：

```text
passed
passed_with_conditions
returned_for_revision
blocked
```

## 20. 阶段一的长期工作回环

长期运行的 Agent 不能只根据最后一条自然语言回复判断成功。阶段一必须逐步建立以下回环：

```text
Agent 执行
  → 写入事件和执行日志
  → 观察真实工具结果和状态变化
  → 做确定性检查
  → 必要时由独立 Judge 评价
  → 判断成功、失败、无进展或结果未知
  → 继续、暂停、重试、升级或请求用户裁定
```

至少检测：

- 工具调用后是否真的产生预期状态变化；
- Agent 是否连续重复同一工具、参数或错误；
- 是否长时间没有有效进展；
- 是否已经完成却继续执行；
- 是否声称写入成功但没有可核验结果；
- 是否发生外部副作用但结果未知；
- 是否超过预算、截止时间或并发上限；
- Provider 不可用时是否仍持续等待。

监护层至少区分：

```text
progressing
completed
failed
stalled
repeating
waiting_approval
outcome_unknown
interrupted
```

重试前必须先判断操作是否幂等、失败发生在调用前还是调用后，以及外部副作用是否已经发生。不能因为 Agent 说“已经完成”就直接把任务标成成功；外部系统查询、文件状态、Provider 收据和确定性断言优先于模型自述。

需要模型评价时，使用独立的窄请求，而不是重新启动完整的 coding Agent：

```json
{
  "messages": [
    {"role": "system", "content": "最小且固定的评价规则"},
    {"role": "user", "content": "任务目标、脱敏执行轨迹和必要工具结果"}
  ],
  "tools": [],
  "temperature": 0
}
```

Judge 只能辅助判断轨迹是否合理，不能单独证明邮件已发送、订单已成交或文件已写入。

## 21. 阶段二：AI OS 演进方向

阶段二暂不承诺，只保留从阶段一演进时需要重新评估的方向。原文前面定义的 L0-L5 完整边界、Runtime Generation、系统级升级和回滚，都属于这一阶段的设计空间。

```text
阶段一：
  一个跨平台软件
  一个 Runtime Core
  持久化 Task / Agent / Operation
  内置 Provider + MCP Provider
  GUI + CLI 等 Surface
  日志、观察、评估和恢复

阶段二：
  独立 Bootstrap / Supervisor
  多 Runtime Generation
  系统级 App 和 Provider 隔离
  统一的 AI OS 身份、权限和事件总线
  系统级热升级、迁移和回滚
```

只有阶段一证明跨平台运行、长期 Agent、日志闭环、恢复和多 App 协作确实成立后，才重新判断是否进入阶段二。阶段二不是阶段一的自动升级，也不能反过来成为阶段一迟迟不能交付的理由。

阶段二启动时，阶段一以插件形式长出的能力（模型连接器、记忆引擎、评价器、Secret Store、定时器等）应按 2.3 的扩展点原样迁移为独立进程 Provider，而不是推倒重设计。阶段一到阶段二的迁移成本，正是对阶段一是否守住合同边界的最终检验。

## 22. 软件核心架构的大白话说明

可以把 BoenMind 想成一个真正会办事的“工作室”，而不是一个聊天窗口。

```text
用户         = 提出事情的人
GUI / CLI    = 工作室的前台和电话
Runtime      = 工作室的总管和档案室
Task         = 一件要办完的事情
Agent        = 负责思考和办事的工作人员
Butler       = 接单、分工、催办和汇报的管家
Capability   = 工作室可以执行的具体动作
Broker       = 门卫和审批台，决定谁能做什么
Provider     = 真正执行动作的部门或外部服务
Event Log    = 按时间记下发生过什么
Execution Log= 记下工作人员具体做了什么
Observation  = 检查事情是否真的发生
Evaluation   = 判断这次工作是否做对
```

一次普通工作大致这样走：

```text
用户：查一下最近的邮件，找出可能影响持仓的风险
  ↓
GUI 或 CLI 把请求交给 Runtime
  ↓
Runtime 创建一个持久化 Task
  ↓
Butler 判断是否需要 Mail Agent 和 Stock Agent
  ↓
Agent 使用 mail.search、mail.read 等 Capability
  ↓
Broker 检查权限、参数、审批和调用范围
  ↓
Provider 真正访问邮件系统
  ↓
Runtime 记录每一步，并检查返回结果
  ↓
Stock Agent 分析，Report Agent 整理结果
  ↓
Observation 检查结果是否有真实依据
  ↓
Butler 向用户汇报，并保留可回放的工作记录
```

其中最关键的一点是：**工作人员说“我做完了”不算做完，档案和检查结果证明做完了才算做完。**

这也是日志系统的意义。以后出了问题，可以沿着一条记录回看：用户说了什么、Butler 怎么分工、哪个 Agent 调了哪个工具、工具返回什么、状态有没有变化、为什么最后判定成功或失败。

所以阶段一的软件核心不是下面这个简单结构：

```text
聊天窗口 → 一个超级 Agent → 一堆工具
```

而是：

```text
多个界面
  → 一个可恢复的 Runtime
  → 持久化 Task 和 Agent
  → Broker 统一管权限和工具调用
  → Provider 执行具体能力
  → 日志和观察层验证结果
```

最终原则：

> **先做一个能在 Windows、Linux、macOS 上长期工作的软件；让它记得住、看得见、查得清、断了能恢复。等这套软件真正证明价值后，再决定是否向 AI OS 进军。**

## 23. 架构决策记录（ADR）

所有架构裁决的增量（新裁决、修订、条件与验收）以 ADR 记录于 `adr/`；本文档正文只保留稳定结论，两者冲突时以更新的 ADR 为准。

| ADR | 标题 | 状态 |
|---|---|---|
| ADR-0001 | Registry/Broker/Bus 三权分立 | accepted-with-conditions |
| ADR-0002 | Butler 仅持协调权，Coordinator 为受限队长 | accepted-with-conditions |
| ADR-0003 | L0 独立控制面与 Runtime generation 升级回滚 | accepted-with-conditions |
| ADR-0004 | Task 规范状态归 L2，任务板仅为投影 | accepted-with-conditions |
| ADR-0005 | 万物皆插件：内核只含合同与最小机制 | accepted-with-conditions |
| ADR-0006 | 权限以合同显式化（元原则） | accepted |
| ADR-0007 | L0 自举豁免与升级信任链 | accepted-with-conditions |
| ADR-0008 | 架构即代码与外部实证验证 | accepted |
| ADR-0009 | 部署形态与 Surface 策略：VPS 托管／Web＋TUI Surface／Windows 桌面壳 | accepted-with-conditions |
| ADR-0010 | 第三方模型网关信任边界 | accepted-with-conditions |
| ADR-0011 | 首批真实 App 以 MCP Server 形态接入 | accepted |
| ADR-0012 | 配置管理 API（随 M10 dsh 线未提交工作归档） | archived（编号永久跳空，存 `archive/m10-dsh-frontend` 分支，见 ADR-0013 编号说明） |
| ADR-0013 | 弃用 dsh 复刻前端 | accepted |
| ADR-0014 | W 序列 WEBUI：assistant-ui 自建壳 | accepted |
| ADR-0015 | 文档体系整理：熔入式修订与三层附页 | accepted |
| ADR-0016 | Skill v0.2 脚本执行架构与 Broker 管线覆盖 | accepted |
| ADR-0017 | context-mode Rust MCP 官方插件 | accepted |
| ADR-0018 | 工作区注册表与会话级工作目录绑定 | accepted |
| ADR-0019 | system.exec 内置命令执行工具(审批类) | accepted |
| ADR-0020 | 内置能力封闭清单与例外裁决 | accepted |

## 24. 架构模型即代码与外部实证验证

- **模型即代码**：全文架构图以 Structurizr C4 DSL 维护于 `architecture/boenmind.c4`（structurizr-dsl 4.1.0 解析验证通过；冻结时点 66 元素/111 关系/11 视图，ADR-0009 后演进出 85 元素/128 关系/12 视图，演进口径以 `architecture/README.md` 为准：SystemContext／Container／L2Components＋六个动态视图＋三个部署环境）。任何 Structurizr 兼容渲染器导入即可出图；修改架构先改模型（ADR-0008；VPS 部署环境自 ADR-0009 起）。
- **外部实证验证**：以 DeepWiki 对照 Erlang/OTP、Kubernetes、VS Code 三个真实 runtime 系统验证 L0-L5 分层与插件热替换设计，报告见 `architecture/deepwiki-validation.md`——C1-C8 逐条裁决：热替换与崩溃隔离（C7/C8）确认，分层与合同化（C1-C6）部分确认，无偏差；单写者租约与验证期禁副作用为本设计独有加强。修订建议 S1-S10 逐条裁决状态（S5/S9 已闭合，S3/S4/S8 部分采纳，余 proposed）以 `milestones/BACKLOG.md` 台账为准，只在里程碑回看时裁决，不自动采纳。
- **辩论记录**：`architecture/debates/` 存有五条核心裁决的完整辩论转录（三方两轮+逐裁决合成）与跨裁决终局合成，是 §17.1 与全部 ADR 的证据底稿。
