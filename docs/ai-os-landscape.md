# AI OS / Agent OS 赛道调研报告

> 状态：调研中（2026-08-14）。第一轮全网搜索完成，Life Agent OS 与 @kernel.chat/agent-os 源码研读待补。
> 目的：验证 BoenMind 2.0「万物皆插件 + Agent OS」想法的赛道位置，吸收同类项目设计。

## 一、概念谱系：一切从 Karpathy 的"LLM OS"开始

2023-09/11，Andrej Karpathy 提出 **LLM OS** 概念（推特 + 《Intro to Large Language Models》演讲）——LLM 是"新操作系统的内核进程"。经典映射：

| OS 概念 | LLM OS 映射 |
|---|---|
| CPU | LLM 推理引擎 |
| RAM | 上下文窗口（128K tokens） |
| 文件系统 | 向量库 / 长期记忆（embedding 检索） |
| 进程 | 各 agent |
| IPC | 工具调用（tool calls） |
| 内核 | 编排层（orchestration） |
| 外设 | 视频/音频/浏览器/计算器/代码解释器 |

**关键辨析**（Mnemoverse 的六种含义分类法）：Karpathy 的版本是"**隐喻**，不是机制"（metaphor, not mechanism）。此后分裂为两条路：
- **隐喻派**：把 OS 类比当心智模型（绝大多数产品宣传）
- **机制派**：真把 OS 的机制（调度/换页/内核/驱动/权限）工程化——**BoenMind 2.0 属于这一派**，本报告重点调研机制派。

## 二、学术/开源机制派项目

### 2.1 AIOS（agiresearch/AIOS）—— 学术线最系统

- **论文**：*AIOS: LLM Agent Operating System*（arXiv:2403.16971，COLM 2025）+ *LLM as OS, Agents as Apps*（arXiv:2312.03815）。
- **架构**：LLM kernel 抽象在 OS kernel 之上，六大 Manager：
  - **Memory Manager**：运行时记忆管理（分配/读写/删除/更新/压缩）；RAM 用尽时 **K-LRU 换页**到磁盘（K-LRU eviction）；**trie 压缩**保持 prompt 在上下文窗口内；
  - **Scheduler**：agent/系统调用请求排队处理；
  - **Storage Manager**：持久数据（文件/知识库/被换出的记忆），本地文件 + 向量库（ChromaDB）；
  - **Tool Manager**：API 工具管理（参数校验/冲突消解）；
  - **Access Manager**：agent 间读写权限。
- 有实验性 **Rust 重写（aios-rs）**：context/memory/storage/tool/scheduler/LLM 的 trait 定义 + 占位实现——与我们的内核服务抽象同构。
- **借鉴点**：K-LRU 换页 + trie 压缩的组合（记忆分层管理）；Access Manager 的 agent 间权限。

### 2.2 MemGPT → Letta —— 虚拟内存分页先驱

- **论文**：*MemGPT: Towards LLMs as Operating Systems*（UC Berkeley，arXiv:2310.08560）。**把 LLM 当 OS 处理虚拟上下文**：prompt = RAM，外部存储 = disk，agent 通过函数调用**分页进出**。
- 记忆分层：主上下文（system + 工作上下文 + FIFO 消息队列）↔ 外部上下文（recall storage 消息库 + archival storage 文档库）；自编辑记忆 + 语义检索。
- 商业化：改名 **Letta**（Apache 2.0），Letta Inc. 融资 $10M（2024-09）。
- **借鉴点**："分页"语义与我们的**压缩 replace 事务**互补——MemGPT 分页是主动的（函数调用），我们是事件日志投影的（可审计）。MemGPT 没有可回放的事件日志。

### 2.3 Life Agent OS（broomva/life）—— Rust，最接近我们（源码研读完成）

- 13 模块 / 76 crates / 2625+ 测试 / MIT / Rust 2024，v0.3.0（2026-04）。模块-生物类比：aiOS（内核契约=DNA）/ Arcan（认知+执行=中枢神经，事件溯源+可重放会话）/ Lago（持久化=长期记忆，journal+blob+知识图谱）/ Autonomic（稳态）/ Praxis（工具执行）/ Haima（财务）/ Nous（评估）/ Anima（身份）/ Vigil（可观测）/ Spaces（网络，**已 exclude**）。
- **内核契约形式**（aios-protocol，纯契约 crate 零运行时依赖）：27 个类型模块 + **14 个 Port trait**（EventStorePort/ToolHarnessPort/PolicyGatePort/ApprovalPort/SessionPort/KnowledgePort/…，全部 `Arc<dyn>` 可换）+ 事件枚举 `EventKind` ~55 变体 + `Custom{event_type,data}` 扩展口。
- **关键设计（直接验证/补强我们的架构）**：
  1. **扩展事件走 Custom 命名空间**（Spec D4）：跨层稳定事件才进一等枚举变体，层内扩展（`ergon.stream`、`vigil.llm_call`）一律 Custom 带命名空间前缀——**与我们的"核心域强类型 + 插件域注册式"设计互相印证**；
  2. **分支化事件日志**：`(session_id, branch_id, seq)` 三维寻址，fork 独立序列、merge 后只读——日志是版本树不是列表；
  3. **投影（Projection）折叠重放**是状态恢复的 canonical 姿势，有"重放两次字节一致"的确定性测试；
  4. **能力模式串**：`Capability("fs:write:/session/**")` glob 模式，策略 gate 按模式匹配；
  5. **hook 的 adapter-trait 归属**（依赖反转：hook crate 拥有 trait、substrate 实现）；
  6. 三层扩展粒度：TurnMiddleware 洋葱 / 生命周期 hook / 流 sink（每事件落库）；
  7. **复合安全门**：policy + capability + budget + sandbox 四层 filter，失败降级 Recover→AskHuman；
  8. 稳态/评估/财务全消费同一事件流（可观测从第一天挂在日志上）。
- **局限（避免照抄）**：无动态插件加载（全部编译期组合）；31.5 万行 monorepo 编译 5-15 分钟；文档/版本状态混乱（VERSION 文件滞后）；Spaces 模块被 exclude 但仍在模块表。

### 2.4 @kernel.chat/agent-os（isaacsight/kernel）—— "AI agents 的 POSIX"（源码研读完成）

- 单包 ~2600 行，零运行时依赖，Apache 2.0。定位："跑在 Modal-class 沙箱之上、MCP/A2A 之下"的**合约层**（"The OS doesn't reinvent the sandbox; it's the contract between agents"）。
- **POSIX 映射**：fork/exec→spawn/chexec、权限→acap（能力令牌）、隔离→ns、cgroups→ulimit-tok、审计→哈希链、信号/管道→handoff。
- **八个原语**：spawn（能力清单显式声明，无隐式继承）/ acap（HMAC 签名令牌：subject/scope/ttl/max_invocations，**降级四硬约束**：subject.kind 不可变、scope 子集、invocations≤源剩余、granted_by=原 holder 形成委托链）/ ns（仅类型）/ ulimit-tok（**reject-before-execute 预检** + warn_at 软警）/ chexec（**taint 污点追踪**：外部输入打标、敏感工具按策略拒绝、untaint 白名单）/ audit（哈希链 prev_hash+self_hash，**agent-os 内未实现**，实际在 kbot-finance）/ handoff（降级交接 + 10 个类型化错误码）/ snapshot（仅类型）。
- 另有 vault（凭据注入，agent 不触明文）+ outcomes（rubric 自评循环）。
- **成熟度诚实性**：README 自列 shipped/partial 状态表；但"宣称与交付脱节"明显（ns/snapshot/audit 类型齐全实现缺席、`./audit` export 悬空、invocations 计数无递增路径、沙箱零集成）。
- **核心借鉴**：acap 降级四硬约束（"接收方永远不可能比发送方多"由规则保证→直接可移植为我们的会话裁剪单调递减）；taint 作为与能力正交的第二强制维度（把提示注入/EchoLeak 从模型行为问题变成结构性拒绝）；reject-before-execute 两段式配额；canonicalize+哈希的审计链。

### 2.5 其他

| 项目 | 一句话 | 参考价值 |
|---|---|---|
| automata/llmos | Karpathy 概念的开源探索（CPU=scheduler+Python exec、MEM=embeddings、FS=向量库+MemGPT） | 概念验证 |
| kase1111-hash/Agent-OS | LLM-native OS，**自然语言"宪法"作为内核**治理（Whisper 编排者/Smith 守护者等角色 agent），CC0 | 宪法治理概念（对比我们的结构化权限） |
| Qualixar OS | arXiv:2604.06392，AI agent 编排的通用 OS，带插件/skill 注册表（qualixar/qos） | 插件注册表形态 |
| Ouroboros（Q00/ouroboros） | "Stop prompting. Start specifying."——采访→种子→执行→评估→进化的持久循环，9 个专家人格 + 本体相似度收敛检测 | 进化循环（远期） |
| use-agent-os/agent-os | OpenRouter 网关型"AgentOS"（名字撞车） | 无（排除） |

## 三、巨头/商业线（2026 现状）

### 3.1 微软：Windows 正式定位为 "Agent OS"（Build 2026-06-02）

- **MXC（Microsoft Execution Containers）**：OS 级 agent 沙箱，**四级隔离**（进程→会话→VM/WSL→Windows 365），Windows 内核强制；OpenAI、Nvidia、Manus、Nous Research 已集成。
- **Windows Agent Framework**（MIT 开源）+ **AgentGuard** 治理层 + **Agent Store**。
- Copilot 超级应用（Chat+Cowork+Code+Autopilots 合一）。
- 内部实验：Project Aion（Copilot 替换开始菜单/任务栏的 agentic OS，Win3 精简内核 + Edge shell）→ 可能演化为 Project Solara。
- 高管表态："Windows is evolving into an agentic OS"（后被用户反弹而收回，但方向明确）。

### 3.2 OpenAI：Workspace Agents + Codex Computer Use + OpenClaw

- **Workspace Agents**（2026-04-23，Custom GPTs 的继任者，基于 Codex 基础设施）。
- **Codex Computer Use**：完整桌面交互（开应用/自动化工作流）。
- **OpenClaw**（openclaw/openclaw，MIT）：个人 AI 助手 + **Gateway 控制面**（sessions/tools/events/channel connections），Tools/Skills/Plugins 三类扩展 + ClawHub 分享平台，由 OpenClaw Foundation（非营利）管理。**OpenAI 是赞助商（非收购）**——被业内称为"agent OS 领域的 Linux"（加入 Windows + Foundation）。

### 3.3 Anthropic：Claude OS 全栈

- 模型（Opus 4.6/4.7，1M context，Claude Code Agent Teams 多 agent 编排）+ 桌面（**Cowork**，2026-04-09 GA：Claude 在桌面端到端工作）+ MCP（10,000+ servers）+ 传闻中的 "Conway" 常驻后台 agent。
- 估值 ~$965B（Series H）超 OpenAI。

### 3.4 行业叙事

- Exponential View：把今天的 LLM 集群比作 1960 年代大型机，"主框架到 Mac"的转移——个人 AI 设备时代。
- OpenClaw = "Linux"，Anthropic = "微软/苹果"，OpenAI/ChatGPT = "苹果"（+Jony Ive 硬件计划）——**agent OS 的"操作系统战争"叙事已成主流**。

## 四、与 BoenMind 2.0 的对照（初步）

| 维度 | AIOS | MemGPT/Letta | Life Agent OS | kernel.chat | Windows MXC | **BoenMind 2.0** |
|---|---|---|---|---|---|---|
| 语言 | Python(+Rust 实验) | Python | **Rust** | TS/Node | C++/内核 | **Rust** |
| 内核形态 | LLM kernel + 6 Manager | 虚拟上下文 | 契约 crate（aiOS） | POSIX 原语 | 真 OS 内核 | 加载器/注册表/事件总线/日志 |
| 事件溯源 | ✗ | ✗ | ✓（Lago journal） | 审计（内容寻址） | ✗ | ✓（会话事件日志=唯一事实源） |
| 插件/扩展 | SDK | ✗ | 模块+桥 crate | 原语 | Agent Store | **一切皆插件（能力/应用/基础设施/平台驱动）** |
| 记忆 | K-LRU 换页+trie 压缩 | 分页+自编辑 | 知识图谱 | ns 隔离 | — | **记忆=日志投影服务（可替换插件）** |
| 权限 | Access Manager | ✗ | — | **acap 能力令牌+降级** | AgentGuard | 能力声明+裁剪会话+把关链 |
| UI 即插件 | ✗ | ✗ | ✗ | ✗ | 部分（Copilot 超级应用） | **✓（应用插件=独立界面+Agent 核心）** |
| 平台驱动 | ✗ | ✗ | ✗ | 沙箱集成层 | ✓（真内核） | **✓（Platform trait=HAL）** |
| 前端=DE | ✗ | ✗ | ✗ | ✗ | ✓（shell 层） | **✓（多 DE 并存）** |

## 五、初步结论

1. **"Agent OS"是 2026 年已被巨头背书的主流叙事**（微软 Build 2026 官方定调、OpenAI/Anthropic 全栈竞争）——BoenMind 2.0 的 Agent OS 方向不是空想，是赛道前沿。
2. **机制派项目稀少且各缺一块**：AIOS 有调度换页无事件溯源；MemGPT 有分页无日志；Life Agent OS 有事件溯源+契约但**无动态插件加载、无 UI 应用层、无平台驱动层**；kernel.chat 有权限/配额/审计原语但无 agent loop、无事件溯源、沙箱零集成；Windows MXC 有真沙箱但封闭。
3. **BoenMind 2.0 的组合（事件日志唯一事实源 + 动态插件加载 + 应用插件 UI + 平台驱动层）在开源赛道没有完整同款**——独特点是"**应用插件（独立 UI + Agent 核心）**"、"**记忆=日志投影**"和"**动态插件化**"（Life Agent OS 最接近但只有编译期组合）。
4. **已被验证的设计**（两家源码级确认）：扩展事件走 Custom/注册式命名空间（Life Agent OS Spec D4 与我们的两层分治互相印证）；事件日志即真相 + 投影重放；能力声明制（模式串/令牌）。
5. **可吸收清单**（进万物皆插件架构 v0.6）：
   - Life Agent OS：分支化事件日志（branch_id 三维寻址）、Port trait 契约形式化、能力模式串 glob、hook adapter-trait 依赖反转、三层扩展粒度、复合安全门、契约 crate 零依赖纯粹性
   - kernel.chat：acap 降级四硬约束（会话裁剪单调递减）、taint 污点追踪（提示注入的结构性拒绝）、reject-before-execute 配额、审计哈希链、canonicalize+签名
   - AIOS：K-LRU 换页 + trie 压缩（记忆分层）
   - MemGPT：主动分页语义（与压缩 replace 事务互补）
   - 避免照抄：Life 的巨型 monorepo 编译期组合、kernel.chat 的"宣称与交付脱节"（能力矩阵按 shipped/partial 诚实标注）、AIOS 的 Python 性能

---
*（2026-08-14 完成：全网搜索 + 4 个开源项目源码级研读（AIOS/MemGPT/Life Agent OS/kernel.chat）+ 巨头动态。研读副本：D:/96_CoderWorld/life-agent-os、D:/96_CoderWorld/kernel-agent-os）*
