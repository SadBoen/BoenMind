# 全网对标调研报告（2026-08-15）：Agent 底座 + 插件同类功能

> 触发：用户"全网搜排名前 10 的 Agent 底座 / 同类插件项目，与我们对比，看是否有值得吸收的经验（边分析边写笔记，出报告前再次验证）"。
> 方法：三个调研子代理并行（Agent 底座 / 记忆系统 / 插件同类功能），边调研边写笔记——笔记全文见
> docs/research/2026-08-15/（agent-foundations.md / memory-systems.md / plugin-landscape.md，共约 100KB，逐条标注核实口径）；
> 主代理出报告前二次抽验关键论断（dsh 本地克隆源码直证 + gh api 星数实测，见各节标注）。
> 数据口径：星数 = GitHub REST API 2026-08-15 实采；机制 = README/源码直读；论文 = arXiv API 复核；厂商 benchmark 数字 = 一律视为自报，未独立复现。

## 〇、一句话结论

2026-08 的生态验证了"一切皆插件"正当其时（dsh 3 天 9.6 万星 + 社区 UI 插件/皮肤、Dify 插件市场、Hermes 桌面插件 SDK、Claude Code 插件四件套都指向"宿主可组合"）。
**三项最值得吸收**：① dsh 的"前端即插件"完整机制（事件→视图节点投影 + Slot 注册表）——这是 BoenMind 当前最明确短板，且它推翻了我们架构文档 §6.4 的旧论断；② 记忆系统的"契约字段 + 淡化机制 + 钩子全集"（agentmemory/Graphiti/agent-crystallize 三件拼齐）；③ 把关链"自动裁决层"（Hermes smart approvals）与带基准的技能沉淀（ponytail）。
**我们的独有资产仍然成立**：QuickJS 真沙箱、append-only 事件日志、Steward 常驻治理在全球 Top10 中无同级对标。

## 一、Top 10 Agent 底座（星数 2026-08-15 API 实测）

| # | 项目 | 星数 | 语言 | 定位 | 插件机制 | 对 BoenMind 一句话 |
|---|---|---|---|---|---|---|
| 1 | LangChain | 144,234 | Py/JS | 编排平台+生态 | 无插件体系 | 生态最大；checkpoint 弱于我们事件日志 |
| 2 | Claude Code / Agent SDK | 141,474 | TS/Py | 产品即平台 | plugins+hooks+skills+MCP 四件套 | 四件套我们已同构 |
| 3 | **deepseek-harness** | 95,831 | TS | "一切皆插件"装配运行时 | Cordis（可逆副作用、重复即抛） | 同构；前端贡献点我们缺（见 §五） |
| 4 | pi | 90,436 | TS/Rust | agent 工具包 | QuickJS + pi.dev 商店 | 已吸收 P1-P6 |
| 5 | OpenHands（Agent Canvas） | 84,060 | TS | 开发者控制中心（可跑任意 ACP agent） | 事件流 + microagent | 治理=人工驾驶舱，Steward 更强 |
| 6 | AutoGen | 60,424 | Python | 多 agent 框架（**维护模式**） | 无沙箱插件 | 被 MS Agent Framework 取代，星数陷阱样本 |
| 7 | CrewAI | 57,081 | Python | 角色化编排 | 无插件 | crew=提示词角色，不吸收 |
| 8 | Goose | 52,809 | Rust | 本机通用 agent | MCP 即扩展（70+ extensions） | 验证 MCP=扩展通道；QuickJS 更硬 |
| 9 | LangGraph | 39,693 | Py/JS | 有状态图编排 | 无 | durable execution 我们以日志天然覆盖 |
| 10 | smolagents | 28,808 | Python | 极简 code-as-action | 无 | 与 dsh PTC 同思路（已吸收） |

周边须知晓（不计入 Top10）：**Hermes 230,644★**（个人助手产品非底座，v0.19/v0.20 新机制见 §二）、n8n 200,638★、Dify 152,445★、browser-use 109,251★、**ponytail 102,744★**（提示词技能）、cline 66,204★、Mastra 27,200★、A2A 协议 25,347★、coze-studio 21,450★、Pydantic AI 19,299★、MS Agent Framework 12,806★。
榜单交叉验证（中英文 2026 榜）一致口径：生产编排=LangGraph、TS 全栈=Mastra、快原型=CrewAI、极简=smolagents；MCP 已成标配；星数≠活跃度。

## 二、用户点名 12 项目核实结果（逐项）

| 项目 | 核实结果 |
|---|---|
| 1. Hermes Agent Team（Michael） | 230,644★，v0.19 "Quicksilver"（**smart approvals 自动裁决**、durable delivery ledger）、v0.20 "Herald"（A2A 插件、签名外发 webhook、**桌面插件 SDK + artifacts 沙箱预览**）。可吸收 3 项入 §六 |
| 2. agentmemory | = rohitg00/agentmemory（27,017★，TS，SQLite 零外部依赖），**与 elizaOS 无关**。与 BoenMind 记忆路线重合度最高的项目：12 自动钩子、4 层巩固、艾宾浩斯衰减+自动遗忘、supersession+provenance |
| 3. TencentDB-Agent-Memory | = **TencentCloud** 组织（21,706★，TS），非 Tencent 主 org。L0→L3 分层蒸馏 + 四资产（Chat Memory/Skill/LLM-Wiki/Code-Graph）+ 上下文卸载 + 检索预算封顶。**无 arXiv 论文**（arXiv API 查无），官方来源仅 GitHub+腾讯云博客 |
| 4. WoLiu-Al-Agent | 用户给的 URL 404（Al→AI 拼写差），真实仓库 CloakGloom/WoLiu-AI-Agent：**1★ 个人项目**（2026-08-09 创建），参考价值≈0，仅作国内个人开发者形态样本 |
| 5. Mempalace | = MemPalace/mempalace（58,373★，Python，**2026-04 创建、4 个月 58K 星，增速异常已标存疑**）。逐字存储不摘要 + 时序 KG（validity windows）+ 压缩前 auto-save——与"记忆=日志投影"同构，可吸收"记忆只存指针不存原文" |
| 6. mnemosyne | 同名 ≥6 项目，锁定 mnemosyne-oss/mnemosyne（2,516★，Hermes 生态）：单 SQLite + sqlite-vec + FTS5、48 字节二进制向量、halflife 168h 衰减、加密 delta sync |
| 7. Dify | 152,445★。插件五类（含 **agent-strategies**）、.difypkg 单文件分发、DSL 版本化、BM25+向量混合检索。SaaS 平台路线不吸收，插件市场形态可参照 |
| 8. NimaChu/Agent-wiki | **已改名 my-wiki**（123★，API 重定向证实）：原始资料→蒸馏 wiki→证据链→知识可视化，"知识不再随会话蒸发"——命中分身交接痛点 |
| 9. Dyad | 21,244★，**已 pivot** 为"本地 AI 应用生成器"（v0/Lovable 替代），原 agent 内核现状未核实；仅其 compaction/记忆评测夹具文化可留意 |
| 10. block/buzz | = block/buzz（27,402★，Rust，2026-03 创建）：Nostr **签名事件日志**协作平台——"agent 是成员不是 bot，按身份授权而非权限旗标"。与我们事件日志+Steward 治理同构度极高，可吸收身份优先授权哲学 |
| 11. ACKEN | **未找到**（GitHub+WebSearch 双查无果）。近名候选 Ackgent/Actenon/akctl/Kagent 均非所指。**请用户提供来源后复核** |
| 12. ponytail | = DietrichGebert/ponytail（102,744★，2026-06 创建）："懒资深工程师"精简代码**技能**（-54% LOC，git diff 计分基准，n=4）——"带基准的提示词技能"范本，可直接做成 BoenMind 技能插件 |

## 三、记忆系统对标（要点）

前列：mem0 63,275★（2026-04 新算法 ADD-only）、MemPalace 58,373★、codebase-memory-mcp 38,956★、Khoj 36,493★、cognee 30,024★、Graphiti 29,931★、Supermemory 28,913★、agentmemory 27,017★、Letta 24,245★（主仓 legacy 化，SDK 转 TS）、TencentDB 21,706★、Zep 4,836★、HippoRAG 3,942★、basic-memory 3,661★、memobase 2,841★、Memary 2,638★（停更）、LangMem 1,606★、agent-crystallize 6★。

论文经 arXiv API 复核：MemGPT 2310.08560、Zep 2501.13956、HippoRAG 2405.14831/2502.14802、sleep-time compute 2504.13171（Letta）。

**对我们的直接结论**：
- 我们"记忆=事件日志投影（可重建可审计）"在全域是稀缺设计——mem0/Letta/Zep 全是状态库（提取物无 provenance），MemPalace/basic-memory 的"原文留转录、记忆只存指针"与我们同构，是交叉印证；
- MemoryPlugin trait 六方法切法被 LangMem（热路径工具+后台管理）与 agentmemory（钩子全集）双双佐证——设计成立，缺实现；
- 16 条吸收点见 §六（最高价值：memory/write 契约字段、记忆插件改订阅事件总线、分身交接晶体字段模板、淡化三机制）。

## 四、插件同类功能对标（要点）

**web-search**：多源+免费额度+fallback 是 MCP 生态已验证模式（search-mcp-rotator 8 源熔断、multi-search-mcp 优先级 fallback）；我们更完整（额度账本+响应头探测+Tavily usage 校准+内容级去重）；可吸收选源策略可配置化（低优先）。

**pdf-omni**：2026 无单一最优（MinerU 77,644★公式表格最强但删脚注、marker2 自测基准第一、docling 64,773★ MIT 无 GPU）；**"级联三级分桶+双引擎交叉验证+多 key 预算账本"组合公开生态未见同构先做者**（结论降级标注：未做论文级检索）——这是我们的差异化长板；最大短板 = 无量化基准，吸收 olmOCR-bench 方法学建 A/B 基准套件（高优先）。

**ctx-compactor**：dsh（SurfaceOp replace+引用链）、OpenHands condenser（tombstone+View 投影）、Claude Code（92% 阈值+保留白名单）的"压缩可审计"比我们更先进（已列入自研日志层吸收计划）；但**"修剪原文可找回+秘密扫描"我们是最完整实现之一**；检索词频弱于 FTS5（中优先）。

**refine-suggest**：Snorkel 悖论（无外部信号的自批评在简单任务有害 98%→57%）**直接佐证我们的"建议+用户审批"设计**；可吸收建议携带证据/置信度字段（低优先）。

**插件生态**：`.claude-plugin/plugin.json` 成跨家事实标准（ZCode/OpenHands 均兼容探测）；MCP registry 2026 已 ~1.8 万~2.2 万但质量危机（9% 健康/41% 无认证）——我们商店路线 = 先 curated list 后市场 + 健康检查上架（差异化卖点）；ZCode 插件无 UI 贡献面（本机核实，架构文档该论断成立）。

## 五、重点：deepseek-harness "前端就是插件"（源码级核实 + 抽验通过）

**事实**（本地克隆 HEAD 47f9438 直证 + 本代理复验：31 个 ui-* 包、web-app roster 424 行含 56 条 UI 插件装配、ui-slots 源码"Declaring is claiming"语义）：

- 整个 Web UI = **约 30 个 ui-* 插件包**经 cordis.patch.yml roster 组装，apps/web 只是 20 行引导；
- **Slot 系统**：SlotMap 声明合并 + 单次 `register()` 同时贡献组件/声明子槽/挂 store/注入回调；四种槽语义（single 独占/list 追加/chain 自荐选举/keyed 按 key）；重复注册即抛、disposer 级联撤销；scope 分 root/session；
- **ConversationNodeDefinition**：事件→视图节点状态机（match/start/update/buildViewNode），全部从 append-only 会话日志投影——UI 是事件日志的浏览器端投影面；
- 贡献面覆盖：页面 tab（conversation.view）/侧栏/工具条座位/聊天节点/设置卡片/悬浮层/**后端 HTTP/WS 路由**（ctx.webServer.register）/主题/语言包；
- 社区实证：awesome-dsh-plugin 365+ 插件（1,125★）、dsh-web-ui 整站替代皮肤 1,946★、桌面壳 1,886★、插件市场 UI 等——第三方前后端一体插件（ccq1/dsh-side-panel）已源码核实。

**对架构文档 §6.4 的修正（重要）**：原论断"dsh 仅有聊天节点注册的萌芽"**不成立**——dsh 前端插件化是完整机制，"前端都是插件"是工程事实（用户的判断正确）。但 dsh 没有四样东西，**BoenMind 的原创点应改写为"受权限治理的应用插件"**：
1. **应用级 manifest 权限**（capabilities/sensitive/override + 按 app 裁剪会话）——dsh 插件是宿主进程内全权 npm 包，无沙箱（QuickJS 真沙箱是我们的硬优势）；
2. **"应用"第一公民 + app 间事件血缘互通**——dsh 只有 bundle/preset 组装，无 app 边界语义；
3. **管家派专家/寄生关系**（软件零 Agent 核心）——dsh 无此产品化概念；
4. **插件 UI 隔离加载**（iframe 凭证/受控渲染）——dsh 客户端插件直接跑在主 React 运行时，崩溃波及全 UI。

## 六、可吸收点汇总（三调研合并，按优先级）

**高优先（低投入高收益 / 直接补短板）**：
1. **前端 slot 树 + 事件投影机制**（dsh，源码已核实）——前端引入"事件→节点定义"注册表 + slot 注册（single/list/chain/keyed）；吸收槽位语义，隔离层仍用我们的 iframe/WC 方案（dsh 无隔离）。应用插件=贡献 conversation node + slot 面板。与架构 §6.4 阶段 4 计划合并
2. **memory/write 契约字段**（agentmemory supersession/provenance + Graphiti bi-temporal）——op(add/invalidate/forget) + source_event_ids + confidence + validity_from/to + supersedes；记忆只存指针不存原文（MemPalace 印证）
3. **记忆插件改订阅事件总线**（agentmemory 12 hooks 全集）——on_turn 保留同步入口，maintain 消费事件队列异步写
4. **分身交接浓缩模板**（agent-crystallize 晶体字段：decision/evidence/open-loop/test/next-action/memory-candidate）——直接模板化进 session.transfer 简报 + memory/write 交接事件
5. **淡化三机制**（mnemosyne halflife + agentmemory 艾宾浩斯/TTL/矛盾检测）——maintain 流水线实现 decay/strengthen/contradiction，参数化 halflife
6. **pdf-omni 量化基准**（olmOCR-bench 方法学）——级联/verify/refine 收益建 A/B 套件
7. **带基准的技能沉淀**（ponytail）——ponytail 人格先做 AGENTS.md 文本版技能；"技能吸收走基准"纳入 Steward 沉淀流程
8. **先 curated list 后市场**（awesome-dsh-plugin→dsh-market 路线）——复用 ZCode marketplace.json 机制
9. **组件级格式兼容 .claude-plugin**（Claude Code 事实标准）——读入其 skills/commands/hooks，执行层保留 QuickJS 沙箱

**中优先（需设计投入）**：
10. 把关链**自动裁决层**（Hermes smart approvals）——默认批准已标记低危命令，高危才弹窗，审计照记（解决"弹窗太重"）
11. 检索预算封顶（TencentDB）——project() 注入加条数/字符/超时上限
12. 传送带升 4 层 + persona 层（TencentDB L0→L3 + agentmemory 4-tier）
13. 身份优先授权（buzz）——专家=独立身份+签名事件，审计哈希链升级路径；短期吸收治理表述
14. 上下文卸载（TencentDB refs/*.md + 任务画布）——工具大结果外置、日志记指针，压缩的补充策略
15. artifacts 沙箱预览 UI 范式（Hermes v0.20）+ 外发签名 webhook 转接器插件 + ACP 转接器（OpenHands/Goose）
16. memory-vector 二期 = 单文件 sqlite-vec + FTS5 + RRF（mnemosyne 50/30/20 打分）
17. 压缩保留白名单语义（Claude Code system prompt + root CLAUDE.md 幸存）
18. worktree 隔离并行专家（Maestro）

**低优先/远期**：选源策略可配置、Zerox 第三引擎评估、FTS5 检索升级、refine-suggest 证据字段、主题/语言包插件化、sleep-time compute 预计算（Letta 论文已核实）、实体链接、LangMem 提示词优化器、书签知识图谱化、图循环 safety（Strands）。

**明确不吸收**：LangGraph 图 DSL/checkpoint（事件日志是上位替代）、CrewAI crew 隐喻、Dify 平台架构/租户模型、Hermes 语音/多 gateway 全家桶（违背内核最小）、AutoGen 本体（官方已弃）、WoLiu 全套（个人作品级）。

## 七、我们已更强的点（交叉印证，勿动摇）

1. **真沙箱**：QuickJS 插件运行时 vs dsh node:vm / CrewAI 无沙箱 / LangChain 无沙箱——Top10 无同级；
2. **append-only 事件日志**：唯一事实源+投影重建，与 dsh 同构、比 LangGraph checkpoint 强一代；
3. **Steward 常驻治理**：全球无同类对标（Maestro=人工驾驶舱、OpenHands=运行台、buzz=协作房间）；
4. **Rust 单二进制 + 寄生宿主 OS**：Top10 中仅 Goose 是 Rust 且非插件式内核；
5. **工具把关链深度**（阶梯权限/审批/配额/审计哈希链）：OpenAI guardrails/CrewAI 无此深度。

## 八、未核实 / 存疑清单（需要时可补充核实）

1. **ACKEN**：双查无果，请用户提供来源；2. **Bifrost/FastMEM/checkpoint-mcp/doobidoo-mcp-memory**：仓库已删或账户 404，无法核实；3. TencentDB 论文不存在（arXiv 查无），其 61% token 节省为博客二手数字；4. MemPalace 4 个月 58K 星增速异常；5. pi.dev"200+ 插件"口径未独立核实（站上只见 50+ examples）；6. 各家 LongMemEval/BEAM benchmark 分数均为厂商自报且互有公开争议；7. olmOCR-bench 有厂商偏倚；8. Tavily 被 Nebius 收购未核实官方公告；9. Manus/JARVIS 未找到权威开源仓库；10. 星数为 8-15 快照，dsh/Hermes/ponytail 日变幅度大。
