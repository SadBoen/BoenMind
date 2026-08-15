# Agent 底座/框架 2026-08 全景调研（对标 BoenMind）

> 状态：调研完成（2026-08-15）。只产出本笔记，未动仓库其他文件。
> 数据口径：星数/动态一律为 **GitHub REST API 2026-08-15 实采**（gh api，已认证），标注【已核实-API】；
> 搜索结论标注【搜索源】（WebSearch 多源交叉，未逐一直访原文）；单一来源或记忆标注【未核实】。
> 与已有研究的关系：dsh D1-D10、pi P1-P6、Hermes H1-H12 已吸收部分不重复展开，只记增量。

---

## ① Top 10 总表（Agent 底座/框架口径）

> 排序口径：只计"底座/框架"（可编程、可扩展、作为宿主运行 agent），剔除纯终端产品（cline、Hermes、browser-use）、工作流平台（n8n、Flowise、AutoGPT 归入周边表）。星数均为 2026-08-15 API 快照【已核实-API】。

| # | 项目 | 星数 | 语言 | 定位 | 插件机制 | 前端形态 | 对 BoenMind 一句话 |
|---|---|---|---|---|---|---|---|
| 1 | LangChain | 144,234 | Python/JS | agent 工程平台（编排+生态） | 无插件体系，integrations 包生态 | 无（LangSmith 商业面板） | 生态规模最大；checkpoint 与我们事件日志投影同族但更弱 |
| 2 | Claude Code / Claude Agent SDK | 141,474 | TS/Python | 终端 coding agent 产品即平台 | plugins + hooks + skills + MCP 四件套 | TUI（自绘终端 UI） | 四件套 BoenMind 已同构（能力插件/事件总线/技能/转接器） |
| 3 | deepseek-harness | 95,615 | TS | "一切皆插件" agent 装配运行时 | Cordis 插件（可逆副作用、重复即抛） | **UI 即插件**（30+ ui-* 包，见②-1） | 与 BoenMind 构想同构，前端贡献点我们缺（详见下） |
| 4 | pi | 90,436 | TS/Rust | agent 工具包（loop/TUI/插件运行时） | QuickJS 插件运行时 + pi.dev 商店 | TUI 壳（app 目录） | 已吸收 P1-P6；仅记 2026-08 动态 |
| 5 | OpenHands（Agent Canvas） | 84,060 | TS/Python | 自托管开发者控制中心（可跑任意 ACP agent） | 无强插件体系，事件流 + microagent 协议 | React Web | 治理=人工驾驶舱+automations；我们 Steward 更强 |
| 6 | Microsoft AutoGen | 60,424 | Python | 多 agent 框架（维护模式，见②-6） | 无沙箱插件，工具注册 | 无（AutoGen Studio 已拆） | 被 Agent Framework 取代；警惕"星数大但停摆" |
| 7 | CrewAI | 57,081 | Python | 角色化多 agent 编排 | 无插件/沙箱，tools 函数 | 无（AMP 商业面板） | crew 隐喻=提示词角色，不吸收；我们把关链更强 |
| 8 | Goose | 52,809 | Rust | 本机通用 agent（桌面+CLI+API） | MCP 即扩展（70+ extensions） | Tauri 桌面 + TUI | 验证"MCP=扩展通道"路线；QuickJS 插件比 MCP 更硬 |
| 9 | LangGraph | 39,693 | Python/JS | 有状态图编排（durable execution） | 无，graph 节点 | 无（Platform 商业件） | checkpoint/time-travel 我们以 append-only 日志天然覆盖 |
| 10 | smolagents | 28,808 | Python | 极简 code-as-action 库（~1k 行核心） | 无 | 无 | 与 dsh PTC 同思路（已吸收）；极简派对照样本 |
| 11 | OpenAI Agents SDK | 28,645 | Python | OpenAI-first 多 agent SDK | guardrails 钩子 | 无（内置 tracing） | handoff/guardrails 语义与我们 subagent/把关链对齐 |

### 周边表（不计入 Top10，但需知晓）【已核实-API 星数】

| 项目 | 星数 | 一句话定位 | 备注 |
|---|---|---|---|
| Hermes Agent（NousResearch） | 230,642 | 个人 AI 助手产品（桌面/CLI/gateway） | 2026-08 新进展见②-2；星数最高但非底座 |
| n8n | 200,638 | 工作流自动化平台 | 非 agent 底座 |
| AutoGPT | 186,605 | 自主 agent 平台（历史项目） | 2026 仍活跃但定位边缘化 |
| Dify | 152,445 | 可视化 Agentic 工作流平台 | 用户点名，见②-3 |
| browser-use | 109,251 | 浏览器 agent 垂直库 | 领域垂直，非底座 |
| cline | 66,204 | VS Code 编码 agent 产品 | 纯终端产品 |
| Microsoft Agent Framework | 12,806 | AutoGen+SK 后继（GA 2026-04-03） | 见②-6 |
| Mastra | 27,200 | TS 全栈 agent（workflows+RAG+evals） | 1.0 于 2026-01【搜索源】 |
| Google ADK | 21,114 | 四语言 SDK（Python/Java/Go/TS），A2A 原生 | GCP 系 |
| coze-studio | 21,450 | 可视化 agent 开发平台（扣子开源版） | 低代码路线 |
| A2A | 25,347 | Agent2Agent 开放协议 | 协议非框架；Hermes v0.20 已内置 |
| Strands Agents（harness-sdk） | 6,908 | AWS 系 model-driven agent SDK（事件 hook 定制 loop） | "Strand（Loops）"见②-10 |
| Maestro（RunMaestro） | 3,248 | 多 agent 桌面驾驶舱 | 见②-10 |
| block/buzz | 27,402 | 人+agent 协作工作区（Nostr 签名事件日志） | 用户点名，见②-5 |
| ponytail | 102,740 | "懒资深工程师"提示词技能（-54% LOC） | 用户点名，见②-7 |
| Pydantic AI | 19,299 | 类型安全 agent SDK | 活跃（Pulse 榜）【搜索源】 |
| Agno | 41,714 | 多 agent 框架 | Pulse 榜活跃【搜索源】 |
| Manus | — | 通用 agent 产品 | 核心闭源，无官方开源仓库【未核实】 |
| JARVIS（本地 Devin 类） | — | — | jarvishq/jarvis 404，未找到权威仓库，见④ |

**Top10 交叉验证结论**：英文榜（AgentMail/Respan/Agentspan/aiagenttools/FutureAGI，2026）一致口径——"生产复杂编排=LangGraph、TS 全栈=Mastra、角色化快原型=CrewAI、极简/学习=smolagents、GCP=ADK、单厂=各家 SDK"；中文榜（Presenc AI 2026-05 星数榜、AI Agent Pulse Tracker 2026-08 活跃榜）星数口径与 API 实测一致方向但数值滞后（如 Hermes 榜单 155.8k vs 实测 230.6k）。共识：MCP 已成标配；AutoGen 星数大但维护模式；星数≠活跃度。来源：https://www.agentmail.to/blog/best-ai-agent-frameworks-2026 、https://www.respan.ai/articles/best-ai-agent-frameworks 、https://agentspan.ai/blogs/best-ai-agent-frameworks-2026/ 、https://aiagenttools.dev/blog-best-ai-agent-frameworks-2026 、https://futureagi.com/blog/best-multi-agent-frameworks-2026/（榜单页未直访，数值以 API 为准）。

---

## ② 逐项目分析

### 2-1 deepseek-harness：2026-08 动态 + "前端就是插件"源码级证据

**动态**【已核实-API】：仓库 2026-08-13T11:56Z 创建，至 2026-08-15 已 95,615 star / 8,860 fork（3 天）；MIT；最新发布 v0.1.0-rc.5（2026-08-13 commit "release(dsh): 0.1.0-rc.5"）；npm 已公开（`@deepseek-ai/dsh-session` next=0.1.0-rc.6 已核实，`dsh-core` 名未命中以 registry 为准）；`has_issues=false`（issues 关闭，社区入口不在 GitHub）；贡献者 22 人（API 首页）；插件生态实证：awesome-dsh-plugin 1,116★、dsh-web-ui 1,946★（"Plugin and skin collection for DSH Web UI"）、modlens 1,304★（视觉插件）、iPolloWork 4,011★。评估文档中"约 300 社区插件"未能找到单一权威清单【未核实】，但生态存在性已证。

**"前端就是插件"机制**（本地浅克隆 @ HEAD 47f9438，2026-08-13，源码直证【已核实-源码】）：

1. `packages/client/README.md` 自述："web-GUI browser half——shell boot、browser-host 通信、共享 UI 服务与 **feature plugins**"。约 30 个 `ui-*` 包，每个都是一个功能插件：ui-slots/ui-theme/ui-sidebar/ui-workspace/ui-conversation/ui-tool/ui-skill/ui-subagent/ui-jobs/ui-permission/ui-plan/ui-user-questions/ui-agent-preset/ui-settings-plugins…（会话里出现的每种能力=一个 UI 插件包）。
2. **Slot 系统**（`packages/client/ui-slots/README.md`）：`SlotMap` 声明合并 + 单一 `register({ name, children?, store?, inject?, ...kind }, Component)` API；一次注册同时声明子 slot、store 席位与业务面（"声明=渲染授权=runtime 规格，同一张表"）；chain-kind slot 由条目**自提名**（`ChainSelect` selector + priority），派发点不选 key；重复/未声明注册在 register 时即抛错；条目 disposer 级联撤销子 slot 与 store。
3. **会话节点定义**（`packages/client/runtime/src/client/contract/conversation.ts`）：`ConversationNodeDefinition<State>` = `{ kind, target?, match(event), start(ctx,match,reader), update(ctx,match), publication?, buildLocationData?, buildViewNode? }`——注释原文 "One independently registered business **Event-to-Node state machine**"。业务数据面（`ConversationTurnDataMap`/`StepDataMap`/`ViewSnapshotMap`）是**声明合并类型**，各业务包 `declare module` 扩展（如 ui-conversation 的 `ChatNodeDataMap['tool-call']`）。
4. **注册表**（`packages/client/runtime/src/client/conversation/event-registry.ts`）：`ConversationEventRegistry.register(definition)` 返回幂等 disposer；同 kind 重复注册**直接抛错**；`registerFallback` 全局唯一、经 `ctx.effect()` 可逆副作用安装（卸载即撤销）。运行时把注册表暴露为 ctx 上的 lazy 服务（`runtime/src/client/index.ts:172`：`conversationEvents: import('./conversation/event-registry.ts')`），sessions 服务用 `rootCtx.get('conversationEvents')` 装配。
5. 实例：`ui-conversation/src/client/conversation-nodes/` 下 compaction/fallback/tool/inbox/assistant/message/command 各一个节点定义；tool.ts 的 `match` 只收 `tool/call` 事件（执行前落日志的那个事件），进而投影递归工具调用树（maxDepth 256）。

**结论**：dsh 前端是**会话事件日志的另一个投影面**——UI 组件不单独取状态，全部经 ConversationNodeDefinition 从 append-only 事件流投影（与 BoenMind"一切状态=投影重建"同构，只是投影发生在浏览器端）；插件对前端的贡献与对后端贡献是同一套 Cordis 注册模型（可逆副作用+重复即抛）。第三方已能用同一机制做 UI 插件/皮肤（dsh-web-ui）。
来源：https://github.com/deepseek-ai/deepseek-harness 、https://github.com/deepseek-ai/deepseek-harness/tree/main/packages/client 、https://github.com/deepseek-ai/deepseek-harness/tree/main/packages/client/ui-slots 、https://github.com/deepseek-ai/deepseek-harness/tree/main/packages/client/runtime/src/client/contract/conversation.ts 、https://github.com/deepseek-ai/deepseek-harness/tree/main/packages/client/runtime/src/client/conversation/event-registry.ts 、https://github.com/zhu1090093659/dsh-web-ui 、https://github.com/awesome-dsh-plugin/awesome-dsh-plugin

### 2-2 Hermes Agent：2026-08 新进展（H1-H12 已覆盖基础）

230,642 star【已核实-API】。三个新版本+当日提交：

- **v0.19.0 "Quicksilver"（2026-07-20）**：TTFB -80%（4.3s→0.9s）、推理流默认开启、桌面 20+ 性能 PR；新机制：Bitwarden/1Password 插件、**smart approvals**（默认自动裁决被标记命令，高危才打扰人）、**subagent 实时观看**、**durable delivery ledger**（响应在 gateway 崩溃后存活投递）。
- **v0.20.0 "Herald"（2026-08-03）**：流式对话语音（barge-in/设备端唤醒词/多平台语音）；**A2A v1.0 插件**（关闭 #514 最老需求）；**签名外发 webhook**（HMAC 生命周期事件推送）；grounded-citations 技能（引用逐条对原文核验）；**桌面平台化**——artifacts 版本卡片+沙箱实时预览、插件 SDK（Kanban 创始插件、`ctx.download`、浮动面板、多窗口）、全局热键速记窗；CLI 命令波（`!command` 不花模型轮次执行 shell、`/init` 生成 AGENTS.md、`/diff`、`/context`、`/focus`、Ctrl+S 暂存）；压缩更智能；工具自恢复。
- **v0.20.1（2026-08-13）+ 当日提交（8-14/15）**：per-profile scope selector（Capabilities 视图）、subagent 迭代上限 50→250。

对比：Hermes 走"超级产品"路线（语音/多 gateway/桌面平台），BoenMind 走"最小内核 Agent OS"路线，不冲突。可吸收：①artifacts 沙箱预览形态→应用插件层 UI 范式；②smart approvals→工具把关链的"自动裁决层"（解决我们"弹窗太重"短板）；③外发 webhook→事件总线加转接器插件即可；④插件 SDK 贡献点设计参考。不可吸收：23 万行级 monorepo 工程体量、Python 主语言。
来源：https://github.com/NousResearch/hermes-agent/releases/tag/v2026.8.3 、https://github.com/NousResearch/hermes-agent/releases/tag/v2026.7.20 、https://github.com/NousResearch/hermes-agent/commits （发行说明内 commit/PR 数为官方自述【未独立核算】）

### 2-3 Dify：插件市场/DSL/工作流/知识库 vs 应用插件层

152,445 star，TS+Python，v1.16.1（2026-07-28）【已核实-API】。插件市场 2025-02-17 随 v1.0.0 上线，首日 120+ 插件【搜索源】；插件五类：tools / model-providers / extensions / **agent-strategies** / bundles，安装三通道（市场一键/GitHub 仓库/本地 .difypkg）【搜索源】；市场 70% 收益归作者【搜索源，未核实官方页面】；官方 `langgenius/dify-plugins` 545★ 活跃（2026-08-14 推送）【已核实-API】。工作流：可视化画布 50+ 节点、Agent 节点 v1.0 起一等公民、Webhook/定时触发；应用可导出 YAML DSL 版本化迁移（DSL 文档滞后，细节【未核实】）。知识库：BM25+向量混合检索、可调分块、可选重排、元数据过滤、15+ 格式解析，插件可贡献 datasource 连接器【搜索源】。插件总数各源口径 50~8677 不一【未核实】。

对比：Dify 是低代码 SaaS 平台（多租户/云托管），非本地 Agent OS；与 BoenMind 可比的是"应用插件层 vs 插件市场"与"工作流"。可吸收：①agent-strategy 插件类型——把"预置组合（preset）"当可市场产品；②.difypkg 单文件分发；③工作流 DSL 文本化（可落事件日志审计，与 dsh workflow 事件溯源同思路）。不可吸收：重型 Web 平台架构、租户/授权模型。
来源：https://github.com/langgenius/dify 、https://github.com/langgenius/dify-plugins 、https://marketplace.dify.ai 、https://docs.dify.ai

### 2-4 Dyad（原 datura-ai）

21,244 star，TS，pushed 2026-08-14【已核实-API】。定位已 pivot：README 自述 "Local, open-source **AI app builder**（v0/Lovable/Replit/Bolt 替代）"，跨平台桌面 app；许可证 Apache-2.0，`src/pro` 目录 fair-source（FSL 1.1）【已核实-README】。原 datura-ai 时代的 agent 内核（dyad-agents、深度搜索、Windows 原生）现状本次未核实【未核实】；目录浅查可见成熟 evals 体系（compaction/chat_history 基准与 fixtures，`src/__tests__/evals/`）。

对比：从"agent 框架"转身"AI 应用生成器"——即"用 agent 产出应用"，与 BoenMind 应用插件层交叉；值得留意的只有其 compaction/记忆评测夹具文化（有 fixtures 的基准化）。不吸收其产品路线。
来源：https://github.com/dyad-sh/dyad

### 2-5 ACKEN — 未找到（如实记录）

GitHub 搜索（`acken`、`ackenproject/acken` 404）与 WebSearch（"ACKEN agent framework ackenproject"）双重无果【已核实：项目不存在于公开 GitHub】。最近似项（均非用户所指的 agent 底座）：Ackgent（fmind，Google ADK + Agent Config YAML 演示）、Actenon（agent 行为审批 proof gate）、akctl（elliottpolk/agentic-kernel 的 CLI）、阿里云 Kagent（ACK K8s agent 框架）。建议向用户确认拼写或来源（可能是内网/未开源项目或误记）。
来源：https://github.com/Actenon 、https://github.com/elliottpolk/akctl 、https://github.com/search?q=acken&type=repositories

### 2-6 block/buzz

`block/buzz` 27,402 star，Rust，Apache-2.0，created 2026-03-06，当日仍推送【已核实-API】。定位：自托管"人与 agent 协作工作区"。本质 = **Nostr relay**：每条消息/反应/工作流步骤/审批/git 事件都是同一事件日志中的**签名事件**（同身份模型、同审计链，人/进程一视同仁）；**agent 是成员不是 bot**——有自己的密钥、频道成员身份、审计链，"按身份授权而非权限旗标"（scoped by identity, not permission flags）；**新增能力=新增 kind 号**，旧客户端无感（协议级渐进）；单事件日志+单搜索索引统一"聊天/补丁/工作流/审批"四类事实。Rust monorepo（Axum relay + 客户端）【已核实-ARCHITECTURE.md】。

对比：与 BoenMind"会话事件日志=唯一事实源 + Steward 常驻会话"同构度极高（事件日志+身份即治理）。可吸收：①身份优先于权限旗标的授权哲学（我们的阶梯权限/审批是"旗标"模型，可演进为"专家=独立身份"）；②签名事件→审计哈希链的升级方向；③agent 即成员（Steward/专家在日志中一视同仁，我们已如此）。不可吸收：Nostr 协议依赖、buzz 不提供 loop/工具执行（靠外部 agent 接入，它只是协作层）。
来源：https://github.com/block/buzz 、https://github.com/block/buzz/blob/main/ARCHITECTURE.md

### 2-7 CloakGloom/WoLiu-AI-Agent

注意：用户给的 `WoLiu-Al-Agent` 404；真实仓库为 **`CloakGloom/WoLiu-AI-Agent`**（"Al"→"AI"拼写差）【已核实-API】。1 star，2026-08-09 创建，MIT，API 主语言 JavaScript（含 Python 后端）【已核实-API】。架构（README 自述）：Python ReAct 循环（Think→Act→Observe，OpenAI 兼容 LLM）+ 工具调度中心（内建 8 + 自定义 15+ 工具、硬件工具、MCP 客户端/本地 MCP Server）+ **15 维演化人格**（warmth/sarcasm/openness 随对话演化、行为过滤）+ ChromaDB 向量记忆（短期滑动窗口+长期 RAG）+ 规则引擎动态构建 system prompt；双端 UI（PC Web 全功能+手机轻量）；ComfyUI/TTS/PPT 生成；状态机驱动 PC↔手机迁移；兄弟仓库 WoLiu-MCP（目录即模块、自动发现、热插拔）。

对比：个人作品级（1★），功能面宽但无沙箱/无事件日志/无治理；对 BoenMind 参考价值≈0。记录意义：国内个人开发者"全栈 AI 助手"典型形态样本（记忆+人格+MCP 三件套是共同想象）。
来源：https://github.com/CloakGloom/WoLiu-AI-Agent 、https://github.com/CloakGloom/WoLiu-MCP

### 2-8 ponytail（精简代码技能）

`DietrichGebert/ponytail` 102,740 star，JavaScript，MIT，created 2026-06-12，pushed 2026-08-07【已核实-API】。机制：把"最懒的资深工程师"人格注入 agent（"你看 50 行，他一声不吭换成 1 行"）——浏览器自带 `input[type=date]` 取代引库+包装+样式表+时区讨论。**带基准的宣称**：-54% LOC（最高 94%）、-20% 成本、-27% 时间；方法论=真实 Claude Code headless 会话改 fastapi full-stack 模板、12 个 feature 任务、n=4、Haiku 4.5、git diff 计分、安全护栏 100% 保留（对照组含 caveman/yagni-oneliner，后者掉护栏）【官方 benchmarks 目录自述，未独立复现】【未核实-基准】。生态：兼容 20 个 agent；ponytail-lite（140★，单 AGENTS.md 无插件版）；tokenwar 等把它并入整合栈。

对比：极低成本高收益的"提示词工程即技能"范本——不是代码框架，而是**带基准验证的技能插件**。可吸收：①该人格片段直接做成 BoenMind 技能插件（AGENTS.md 版先行=渐进式）；②"技能吸收走基准流程"的方法论（我们已有压缩 A/B 基准文化，扩到技能）；③ponytail-lite 证明"先文本后插件"路线可行。注意：效果依赖模型/任务（官方自述代码已极简时收益趋零）。
来源：https://github.com/DietrichGebert/ponytail 、https://github.com/DietrichGebert/ponytail/tree/main/benchmarks 、https://github.com/ilindaniel/ponytail-lite

### 2-9 Top10 底座逐条（紧凑）

- **LangChain / LangGraph**（144,234 / 39,693★，Python/JS，MIT）：平台+图编排；checkpoint 持久化、HITL、time-travel 调试、重试【搜索源】；插件无（integrations 生态）；记忆=LangMem/checkpointer。对比：我们 append-only 日志=checkpoint 上位替代（可回放/可 fork/可审计）；不吸收其图 DSL。
- **Claude Code / Claude Agent SDK**（141,474★；SDK Python 7,894★ / TS 1,696★）：产品即平台；插件+hooks+skills+MCP 四件套、容器沙箱；2026-06 SDK 计费调整后暂停【搜索源】。对比：四件套我们已同构（能力插件/事件总线/技能/MCP 转接器）。
- **pi**（90,436★，earendil-works/pi，原 badlogic/pi-mono）：已吸收 P1-P6 勿重复；2026-08 仅记：描述现为 "AI agent toolkit: unified LLM API, agent loop, TUI, coding agent CLI"，活跃（pushed 8-14）【已核实-API】，无新架构要点。
- **OpenHands / Agent Canvas**（84,060★，TS）：2026-08 定位已变——"自托管开发者控制中心"，可运行 OpenHands/Claude Code/Codex/Gemini 或**任意 ACP 兼容 agent**（本地/远程/云后端），含 automations、worktrees【已核实-README】；事件流架构（EventStream/Action-Observation）为既有认知【未核实】。对比：治理=人工驾驶舱+预置 automations，不是常驻 Steward——我们治理更强；可吸收 ACP 转接器（接外部 agent 的通用协议层，与 MCP 同模式）。
- **AutoGen / Microsoft Agent Framework**（60,424★ 维护模式，pushed 2026-04-15；MAF 12,806★）：MAF 2026-04-03 GA，.NET+Python，图协作模式（sequential/concurrent/handoff/group collaboration）、durability/restartability/observability/governance/HITL【已核实-README + 搜索源】。对比：吸收其协作模式词汇表（对齐我们 Steward 派工语义）；警惕"星数大但停摆"的 AutoGen 教训。
- **CrewAI**（57,081★，Python）：crew 角色隐喻+Flows；无沙箱/无插件【搜索源】。对比：角色=提示词工程，我们专家=独立会话+边界自主决策，更强；不吸收。
- **Goose**（52,809★，Rust）：已从 block/goose 迁至 Linux Foundation AAIF（aaif-goose）；桌面+CLI+API；MCP 即扩展（70+ extensions）、ACP 接订阅【已核实-README】。对比：验证"MCP=扩展通道"；我们 QuickJS 插件运行时比 MCP 更硬。
- **smolagents**（28,808★，Python，Apache-2.0）：code-as-action（模型写 Python 而非 JSON 工具调用），~1k 行核心【已核实-描述+搜索源】。对比：与 dsh PTC 同思路（D 系已吸收）；极简派对照样本——无插件/沙箱/治理/日志。
- **OpenAI Agents SDK**（28,645★，Python，MIT）：handoffs/guardrails/sessions/tracing；OpenAI-first（litellm 适配 100+ 模型）【搜索源】。对比：guardrails=工具把关链弱化版，handoff 语义与我们 subagent 对齐。
来源：https://github.com/langchain-ai/langchain 、https://github.com/langchain-ai/langgraph 、https://github.com/anthropics/claude-code 、https://github.com/earendil-works/pi 、https://github.com/OpenHands/OpenHands 、https://github.com/microsoft/autogen 、https://github.com/microsoft/agent-framework 、https://github.com/crewAIInc/crewAI 、https://github.com/aaif-goose/goose 、https://github.com/huggingface/smolagents 、https://github.com/openai/openai-agents-python

### 2-10 周边值得留意

- **Strands Agents（"Strand（Loops）"）**：strands-agents/harness-sdk 6,908★，AWS 系 model-driven SDK（Python+TS），事件驱动 hook 使 loop 完全可定制（tracing/hooks/guardrails/**steering handlers**）；"Loops"实指 **graph loops**（writer→checker→loop back 带 loop safety）与后台循环，非独立项目（anthropics/claude-loops 已 404【已核实】）；生态有 Temporal durable harness（重试+HITL 审批）、Apify fork【搜索源】。可吸收：steering/guardrail hook 命名与语义（对齐我们 loop 插件接口）、图循环 safety。
  来源：https://github.com/strands-agents/harness-sdk 、https://github.com/apify/strands-harness-sdk 、https://aws.amazon.com/jp/blogs/machine-learning/strands-agents-sdk-a-technical-deep-dive-into-agent-architectures-and-observability/
- **Maestro**（RunMaestro/Maestro 3,248★）：多 agent 桌面驾驶舱（Claude Code/Codex/OpenCode/Factory Droid/Copilot-CLI 直通），**specs 文档→Auto Run 逐任务新会话**（干净上下文、长时无人值守近 24h）、**git worktrees** 并行隔离【已核实-README】。可吸收：worktree 隔离并行专家、任务规格+全新会话派工。
  来源：https://github.com/RunMaestro/Maestro
- **coze-studio**（21,450★）：可视化 agent 平台（低代码路线，与 Dify 同赛道）。**A2A**（25,347★）：开放协议，Hermes/ADK 已原生支持——BoenMind 以转接器插件接入即可（不动内核）。**Mastra**（27,200★，TS）：workflows+RAG+memory+evals 全家桶【搜索源】。
  来源：https://github.com/coze-dev/coze-studio 、https://github.com/a2aproject/A2A 、https://github.com/mastra-ai/mastra

---

## ③ 可吸收点汇总表

> 优先级：高=低投入高收益/直接补短板；中=需设计投入；低=记录备查。N/A=不吸收。

| # | 机制 | 来自 | 我们现状 | 吸收建议 | 优先级 |
|---|---|---|---|---|---|
| 1 | UI 即插件：会话事件→视图节点投影（ConversationNodeDefinition：match/start/update/buildViewNode + SlotMap 声明合并注册） | dsh（源码已核实） | 前端 React 固定结构，无插件贡献点 | 前端引入"事件→节点定义"注册表 + slot 注册；应用插件=贡献 conversation node + slot 面板。可分两步：先 slot 注册表，再事件投影 | 高 |
| 2 | 注册表纪律：重复注册即抛、register 返回幂等 disposer、全局唯一 fallback、可逆副作用安装 | dsh client runtime | 事件总线有 emit/waterfall/parallel/serial，但注册冲突静默 | 服务注册表与插件加载器统一此纪律（卸载即撤销已有，补冲突即抛与 fallback 语义） | 高 |
| 3 | 带基准的提示词技能（-54% LOC、git diff 计分、n=4、安全护栏对照） | ponytail | 技能注入为 XML 块，无基准流程 | 吸收 ponytail 人格为技能插件（先 AGENTS.md 文本版）；把"技能吸收走基准"纳入技能沉淀流程（Steward 侧） | 高 |
| 4 | 自动裁决层（smart approvals：默认批准已标记低危命令，高危才打扰） | Hermes v0.19 | 工具把关链=弹窗审批（默认重） | 把关链加"策略自动裁决"层：标记命令默认放行、敏感命令保留弹窗，审计照记 | 中 |
| 5 | artifacts 版本卡片+沙箱实时预览（生成物安全预览） | Hermes v0.20 | 应用插件独立 UI，无产物预览形态 | 应用插件层加"产物卡片+沙箱预览"UI 范式（QuickJS/受限渲染） | 中 |
| 6 | 身份优先授权（agent=成员：自有密钥、成员身份、审计链），签名事件日志 | buzz | 阶梯权限/审批/审计哈希链（旗标模型） | 演进方向：专家=独立身份+签名事件（审计哈希链升级路径）；短期先吸收"身份优于旗标"的治理表述 | 中 |
| 7 | 外发签名 webhook（HMAC 生命周期事件推送） | Hermes v0.20 | 无外发 | 事件总线加"外发 webhook 转接器插件"（复用会话事件日志过滤） | 中 |
| 8 | ACP 转接器（接任意外部 agent 的标准协议） | OpenHands / Goose | 无 | 与 MCP 同模式：ACP 转接器插件，接外部专家/子代理，不动内核 | 中 |
| 9 | 工作树隔离并行 agent（git worktrees） | Maestro | subagent 并行无文件系统隔离 | 并行专家（Steward 派工）用 worktree/独立工作区隔离冲突 | 中 |
| 10 | agent-strategy 插件类型（预置组合可市场）+ 单文件包分发（.difypkg） | Dify | 准内核 agent-loop 可替换 + preset 概念 | 把"预置组合"做成可分发资产（对齐我们 preset/bundle 概念）；包分发格式参考 | 中 |
| 11 | 工作流 DSL 文本化（YAML，可版本化/迁移） | Dify | 准内核 workflow 插件 | 工作流定义以文本落会话日志（可审计/可重放，与 dsh workflow 事件溯源一致） | 中 |
| 12 | 图循环 safety（writer→checker→loop back 环路保护机制） | Strands | 工作流循环无保护 | 工作流插件加环路保护（最大环数/收敛检测） | 中 |
| 13 | 任务规格文档 + Auto Run 逐任务新会话（干净上下文） | Maestro | Steward 派工 | 派工携带任务规格 + 每任务独立专家会话（我们已有会话即生命周期，只需补规格协议） | 低 |
| 14 | 长时任务 durable 化（harness 跑在持久运行时内：重试/HITL 审批） | Strands+Temporal | 会话事件日志已持久 | 长时任务恢复性设计参考（事件日志天然支持，无需 Temporal） | 低 |
| 15 | steering handlers/guardrail hooks 命名与语义 | Strands | agent-loop 可替换（准内核） | loop 插件接口设计时对齐其 hook 语义词汇 | 低 |
| 16 | 元数据过滤+可调分块+重排（KB 混合检索） | Dify | 记忆 + FTS5（H12 已吸收 CJK） | FTS5 已够；仅吸收"元数据过滤"进记忆检索 | 低 |
| 17 | 新能力=新 kind 号（协议演进无破坏） | buzz | 会话事件版本化 + ignorable 守卫（D 系已吸收） | 已覆盖；记录为交叉印证 | 低 |
| 18 | code-as-action | smolagents | dsh PTC 已吸收（D 系） | 已覆盖 | N/A |

### 明确不吸收（含理由）

- LangGraph 图 DSL/checkpoint 系统——我们事件日志投影是上位替代；图 DSL 限制性过强。
- CrewAI crew 角色隐喻——提示词工程而已；我们专家=独立会话+边界自主决策。
- Dify 平台架构/租户模型——SaaS 形态，违背"寄生宿主 OS"铁律。
- Hermes 语音/多 gateway 全家桶——23 万行级产品路线，违背内核最小铁律。
- AutoGen 本体——官方已转向 Agent Framework（星数陷阱样本）。
- WoLiu 记忆/人格/多端——个人作品级，无工程参照。

### 我们已更强的点（交叉印证，勿动摇）

1. **真沙箱**：QuickJS 插件运行时 vs dsh node:vm / CrewAI 无沙箱 / LangChain 无沙箱——全 Top10 无同级。
2. **append-only 事件日志**：唯一事实源+投影重建，与 dsh 同构但先于其公开；buzz 的签名事件日志同构但非 agent 运行时。
3. **Steward 常驻治理**：全球无同类对标（Maestro=人工驾驶舱、OpenHands=运行台+automations、buzz=协作房间）——保留并吸收 buzz 身份模型。
4. **Rust 单二进制 + 寄生宿主 OS**：Top10 中仅 Goose 是 Rust 且非插件式内核。
5. **工具把关链（阶梯权限/审批/配额/审计哈希链）**：OpenAI guardrails/CrewAI 无此深度；dsh 同族但无配额与哈希链。

---

## ④ 未核实 / 存疑清单

1. **ACKEN**：GitHub 与 WebSearch 双查无果，判定不存在/误记【已核实"未找到"】；候选近名：Ackgent、Actenon、akctl、Kagent。需用户提供来源。
2. **WoLiu-Al-Agent**：404；实际为 WoLiu-AI-Agent【已核实】。
3. **JARVIS（本地 Devin 类）**：jarvishq/jarvis 404，未找到权威仓库【未核实】。
4. **"Strand（Loops）"**：anthropics/claude-loops 不存在【已核实 404】；Strand=Strands Agents（AWS 系 SDK），Loops=其 graph loops 功能，非独立项目【搜索源】。
5. **dsh"约 300 社区插件"**（评估文档原话）：无单一权威清单可核【未核实】；生态存在性已证（awesome-dsh-plugin 1.1k★、dsh-web-ui 1.9k★、modlens 1.3k★）。npm 个别包名（dsh-core）在 registry 未命中【部分未核实】。
6. **Dify 插件总数**：各源 50~8677 口径不一【未核实】；市场分成 70% 未核实官方页面。
7. **Hermes 发行说明内 commit/PR/贡献者统计**：官方自述，未独立核算【未核实】。
8. **ponytail 基准（-54% LOC 等）**：官方 benchmarks 目录自述，未独立复现【未核实-基准】。
9. **Dyad 内部 agent/插件架构现状**：仅 README+目录浅查，datura-ai 时代内核去向未核【未核实】。
10. **OpenHands 事件流架构细节**：既有认知+新 README，未读源码【未核实】。
11. **Manus 开源状况**：判定核心闭源，未逐一查证其官方开源仓库【未核实】。
12. **星数时效**：全部为 2026-08-15 API 快照，dsh/Hermes/ponytail 等日变幅度大，引用时注意。
13. **榜单类数据**（Presenc AI / AI Agent Pulse Tracker / Quark 下载榜）：来自搜索摘要，原始页面未直访【未核实】。

---

## ⑤ 一句话结论

2026-08 的生态验证了"一切皆插件"正当其时（dsh 3 天 95.6k★ + 社区 UI 插件、Dify 插件市场、Hermes 桌面插件 SDK、Goose MCP 扩展都指向"宿主可组合"），而**"前端即插件"是 BoenMind 当前最明确的短板**——dsh 已证明前端可以只是事件日志的投影面（ConversationNodeDefinition + Slot 注册表），此机制可吸收且成本可控；其次是把关链"自动裁决层"（Hermes smart approvals）与带基准的技能沉淀（ponytail）。Steward 治理与 QuickJS 真沙箱仍是我们独有资产，继续持有。
