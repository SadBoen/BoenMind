# Agent 记忆系统全景调研（2026-08-15）

> 注（2026-08-15 文档清理）：文中引用的 docs/deepseek-harness-evaluation.md、docs/HANDOFF_EVERYTHING_IS_PLUGIN.md、docs/context-compression-plan.md 等源文件已从工作区删除，原始内容在 git 历史可查。
> 调研性质：BoenMind 记忆设计的外部对标调研笔记。只写本文件，未动仓库其他文件。
> 数据核实方式：星数/语言/活跃度 = **GitHub API（gh，已认证）实测于 2026-08-15**；机制细节 = 各仓库 README 直读；论文 = arXiv API 复核；少量机制来自 WebSearch 摘要（已标注"二手来源"）。
> 坐标系：BoenMind 记忆设计 = 事件日志的投影服务（MemoryPlugin trait 六方法：on_turn/maintain/project/tool_schemas/on_pre_compress/retrieve，仅一个活跃插件，`memory.provider` 切换；首版 memory-compactor + memory-file 传送带 facts.md/today.md/longterm.md，二期 memory-vector）。详见 docs/everything-is-plugin-architecture.md §6.1、§6.7 尾。

---

## ① 总表（星数 2026-08-15 实测）

| 项目 | 星数 | 语言 | 存储引擎 | 一句话特点 |
|---|---|---|---|---|
| mem0ai/mem0 | 63,275 | Python | 可换（Qdrant/pgvector 等） | 通用记忆层 API，2026-04 新算法：ADD-only 提取 + 实体链接 + 多信号 RRF 检索 |
| MemPalace/mempalace | 58,373 | Python | ChromaDB（可插拔）+ SQLite KG | 逐字存储不摘要的本地记忆宫殿（wing/room/drawer），96.6% LongMemEval R@5 |
| DeusData/codebase-memory-mcp | 38,956 | C | 自带持久 KG（单二进制） | 代码库级记忆 MCP：158 语言索引、亚毫秒查询、99% 少 token |
| khoj-ai/khoj | 36,493 | Python | 自托管多后端 | 个人"第二大脑"（docs/web 检索 + agent），非严格 agent memory 框架 |
| topoteretes/cognee | 30,024 | Python | 多后端（自托管 KG 优先） | ECL 管线（extract/cognify/load）把任意数据变可查询知识图谱 |
| getzep/graphiti | 29,931 | Python | Neo4j/FalkorDB/Neptune 等 | 时序上下文图谱：事实带有效期，invalidate 取代删除（论文 2501.13956） |
| supermemoryai/supermemory | 28,913 | TypeScript | 托管/自托管多后端 | 号称多项 benchmark 第一（95% R@15、99.4% 上下文缩减、~50ms profile） |
| rohitg00/agentmemory | 27,017 | TypeScript | SQLite + iii-engine（零外部依赖） | 编码 Agent 的 MCP 记忆引擎：12 自动钩子 + 4 层巩固 + 艾宾浩斯衰减 + 自动遗忘 |
| letta-ai/letta | 24,245 | Python | Postgres + 向量库 | MemGPT 三代：三区内存（core/recall/archival）+ agent 自主换页；repo 已标记 legacy，活跃开发移至 TS SDK |
| TencentCloud/TencentDB-Agent-Memory | 21,702 | TypeScript | SQLite + sqlite-vec（可选腾讯云向量库） | 团队级记忆中枢：四资产（Chat Memory/Skill/LLM-Wiki/Code-Graph）+ L0→L3 蒸馏 + 上下文卸载 |
| getzep/zep | 4,836 | Python | 托管 API | Zep 托管服务仓库（Graphiti 引擎的服务化外壳） |
| OSU-NLP-Group/HippoRAG | 3,942 | Python | 自建索引（OpenIE KG + 编码器） | 神经生物学启发的 RAG：离线建图便宜、在线 PPR 多跳（HippoRAG 2 = 2502.14802） |
| basicmachines-co/basic-memory | 3,661 | Python | Markdown 文件 + SQLite（MCP） | Claude Code 生态最知名的文件式记忆 MCP，笔记人可读、自动语义链接 |
| letta-ai/letta-code | 2,997 | TypeScript | 本地/云/自托管 | Letta 新 Agent SDK（状态化 agent 运行时的现行仓库） |
| memodb-io/memobase | 2,841 | Python | 自带服务端 | 用户画像型记忆：profile 随时间压缩演化；2026-01 后停更 |
| kingjulio8238/Memary | 2,638 | Python | Neo4j/Weaviate/FalkorDB | 仿人脑多记忆系统（episodic/semantic/procedural）；2024-10 后停更 |
| mnemosyne-oss/mnemosyne | 2,516 | Python | 单 SQLite（sqlite-vec + FTS5 + 48B 二进制向量） | 零云记忆：BEAM 架构 + 半衰期衰减 + 双向加密同步，Hermes 生态 |
| langchain-ai/langmem | 1,606 | Python | 任意存储（LangGraph store 原生） | 记忆 SDK：热路径记忆工具 + 后台记忆管理器 + 提示词优化器 |
| letta-ai/sleep-time-compute | 137 | Python | 论文配套 | "睡眠时间计算"：离线预想查询、预计算上下文（2504.13171） |
| NimaChu/my-wiki（原名 Agent-wiki） | 123 | JavaScript | 本地 Markdown + raw 证据 | Agent 可用的本地知识 wiki：原始资料→蒸馏 wiki→证据链→可视化知识宇宙 |
| stewie-sh/agent-crystallize | 6 | TypeScript | 本地 Markdown（.agent-crystals/） | "Stop compacting. Start crystallizing."：压缩前/交接前固化工作晶体 |

未核实行（见 ④）：Bifrost、FastMEM、checkpoint-mcp、doobidoo/mcp-memory-service、TencentDB 论文。

---

## ② 逐项目分析

### mem0 — 通用记忆层（63.3K★，Python）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/mem0ai/mem0
定位：把"记忆"做成独立服务的开创者，`add/search/update/delete` REST/SDK，多租户作用域（user/agent/session），框架无关（LangChain/CrewAI/AutoGen/任意 loop）。
2026-04 新算法（README 已核实）：**单 pass ADD-only 提取**——一次 LLM 调用只做新增，不再 update/delete，记忆只累积；agent 确认过的事实一等公民；**实体链接**（实体抽取、嵌入、跨记忆链接以提升检索）；**多信号检索**（语义 + BM25 + 实体匹配并行打分融合）；时间感知检索。自报 LoCoMo 71.4→92.5、LongMemEval 67.8→94.4（托管平台数字，OSS 方向性相似，厂商自报未独立复核）。
与 BoenMind 对比：mem0 的"记忆"是提取物而非投影，没有原始会话日志作为可重建事实源；无 provenance 回源。可吸收：ADD-only 把写契约大幅简化（与"记忆写回契约"痛点直接相关）；实体链接与 RRF 融合可作为 memory-vector 二期检索参考；缺 provenance 恰是我们的日志投影优势所在。

### MemPalace/mempalace — 逐字记忆宫殿（58.4K★，Python）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/MemPalace/mempalace
定位：本地优先，**逐字存储对话原文、只做语义检索、不摘要不改写**。结构：wing（人/项目）→ room（主题）→ drawer（原文内容）；检索可 scoped 到 wing。检索后端可插拔（默认 ChromaDB，接口 `mempalace/backends/base.py`）；另有**时序实体关系图谱**（validity windows：add/query/invalidate/timeline，SQLite 本地）。44 个 MCP 工具；每个专家 agent 有自己的 wing + diary；**auto-save hooks**（Claude Code/Codex/Cursor）定期保存且**在上下文压缩前保存**，`mine`/`sweep` 可回填历史转录（幂等、可续跑）。自报 96.6% LongMemEval R@5 零 API 调用（自报，未独立复核）。
与 BoenMind 对比：其核心思想"**原文留在转录里、记忆只是索引+scoped 检索**"与我们"记忆=事件日志投影、可重建"完全同构——日志就是 verbatim 事实源，记忆插件只需存指针/索引，不必重复存原文。可吸收：压缩前保存钩子（对应 on_pre_compress，已吸收）；per-agent wing 命名空间；invalidate 语义。

### Letta（原 MemGPT）— 状态化 agent 运行时（letta 24.2K★ + letta-code 3.0K★）
**核实方式：gh api + README 直读；论文 2310.08560 经 arXiv API 复核（已核实）** · 来源 https://github.com/letta-ai/letta 、 https://github.com/letta-ai/letta-code
定位：完整 agent 运行时（不单卖记忆）。MemGPT 三区内存：**core**（始终在上下文，如 RAM）、**recall**（可搜索对话历史）、**archival**（长期向量存储），**agent 自己用函数调用换页**（BoenMind 已吸收为 A12）。当前仓库已标注 legacy server，活跃开发移到 letta-code（TS SDK，可跑本地/云/自托管）。
配套论文 **sleep-time compute（2504.13171，arXiv 复核）**：离线"睡眠时间"预想用户可能问什么、预计算上下文，test-time compute 降 5 倍、多查询摊薄 2.5 倍——这是**后台 maintain 流水线（记忆夜间维护/预计算）的理论依据**，可直接引用到 BoenMind maintain 设计。
对比：Letta 高 lock-in（拥有整个 agent loop），BoenMind 记忆只是内核旁的一个插件；吸收其分页语义与 sleep-time 思想，不吸收其运行时形态。

### Zep / Graphiti — 时序知识图谱（graphiti 29.9K★ + zep 4.8K★）
**核实方式：gh api + README 直读；论文 2501.13956 经 arXiv API 复核（已核实）** · 来源 https://github.com/getzep/graphiti
定位：为 agent 构建**时序上下文图谱**：事实随边带有效期（bi-temporal），"什么时候是真是假"可查；新事实**取代**旧事实而非删除（invalidate）；增量更新不需重建图谱；保留 provenance 到源数据；支持预定义/学习式 ontology；hybrid 检索（语义+关键词+图遍历）；有 MCP server。
论文《Zep: A Temporal Knowledge Graph Architecture for Agent Memory》（2501.13956，已核实）。
对比：这是"老记忆淡化"痛点的**最教科书答案**——淡化不是删数据，是**断言过期**（invalidate 事件，日志天然支持：append 一条 invalidate 事件即可重放）。但图构建开销大（二手来源称单对话 ~600K token 足迹、即时检索可能未就绪，未核实），BoenMind 首版不建图，仅吸收**有效期字段进 memory/write 契约**。

### cognee — 自托管记忆平台（30.0K★，Python）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/topoteretes/cognee
定位：任意格式数据入库 → ECL 管线（**E**xtract/**C**ognify/**L**oad）→ 自托管知识图谱；多后端可换；定位"AI memory platform"。强调记忆=跨会话持久 + 关联 + 可回忆。
对比：cognee 的 **Cognify = 后台记忆管线化**，与 BoenMind 的 `maintain`（后台异步维护）同构——维持 current trait 即可，无额外吸收点；其重图谱路线与首版 compactor/file 路线不同，作为二期 vector 之后的备选对照。

### LangMem — LangGraph 生态记忆 SDK（1.6K★，Python）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/langchain-ai/langmem
定位：三类能力：① 热路径记忆工具（create_manage_memory_tool / create_search_memory_tool，agent 对话中主动读写）；② **后台记忆管理器**（自动提取、巩固、更新）；③ **提示词优化器**（从交互中提炼 prompt 改进）。存储任意（LangGraph BaseStore 原生，InMemory→Postgres 可换）。
对比：①+② 与 MemoryPlugin 的 `tool_schemas`（模型侧记忆工具）+ `maintain`（后台）**一一对应，佐证 trait 切法**；③ 提示一个新输出形态：记忆插件不止产出 `project()` 注入文本，还可产出"提示词改进"类建议——可作 maintain 的远期产物。

### Memobase — 用户画像记忆（2.8K★，Python）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/memodb-io/memobase
定位：chatbot 应用的"用户画像"型长期记忆：profile 分层压缩（画像随时间演化、旧细节折叠进高层概括），flush/retrieval API，多 SDK + MCP。2026-01 后停更。
对比：profile 的**分层压缩演化**与 facts.md 传送带（today→longterm 升层淡化）同构，可吸收其"profile 预算封顶 + 旧层折叠"细则；停更说明单画像模式市场收窄，BoenMind 的多形态插件路线更稳。

### Supermemory — 记忆引擎 + 应用（28.9K★，TypeScript）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/supermemoryai/supermemory
定位：宣称 LongMemEval/LoCoMo/ConvoMem 全部第一（95% R@15、99.4% 上下文缩减、~50ms profiles，厂商自报未独立复核）；托管 + 自托管；API + MCP + 仪表盘。
对比：营销数字打得很满（与 mem0 有公开 benchmark 之争，MemoryAgentBench 指出各家冲突解决率都低），**benchmark 数字一律视为厂商自报**。架构上"profile 化 + 稀疏注入"路线与 mem0 类似；对 BoenMind 无独特机制可吸收。

### rohitg00/agentmemory — 编码 Agent 记忆引擎（27.0K★，TypeScript）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/rohitg00/agentmemory
定位：面向 Claude Code/Cursor/Codex/Hermes/OpenClaw 等 50+ 编码 Agent 的 MCP 记忆引擎（54 tools/6 resources/3 prompts/15 skills），SQLite + iii-engine 零外部依赖，本地嵌入免费。
核心机制（README 已核实）：
- **12 自动钩子**：SessionStart / UserPromptSubmit / PreToolUse / PostToolUse / PostToolUseFailure / **PreCompact（压缩前重注入记忆）** / SubagentStart/Stop / Stop / SessionEnd——全部零手工。
- **4 层巩固**：Working（工具原始观察）→ Episodic（会话压缩摘要）→ Semantic（提取事实/模式）→ Procedural（工作流/决策模式），类比睡眠巩固。
- **淡化/遗忘**：艾宾浩斯衰减曲线、频繁访问增强、过期自动驱逐（TTL + 矛盾检测 + 重要度驱逐）、**记忆版本化与取代（supersession）**。
- **citation provenance**：任意记忆可溯源到原始观察；隐私剥离（API key/secret/`<private>`）；团队记忆命名空间（共享+私有）；与 MEMORY.md 双向同步（Claude bridge）；/forget、/recap、/handoff 等 8 个可调用技能。
对比：这是与 BoenMind 路线**重合度最高**的项目——"事件钩子全集"对应我们的**事件总线订阅**（记忆插件应订阅 turn_start/user_prompt/tool_pre/tool_post/compaction_pre 事件而非轮询）；4 层巩固对应传送带升级；supersession + provenance 直接回答 memory/write 契约设计；/handoff + /recap 对应分身交接浓缩。

### TencentCloud/TencentDB-Agent-Memory — 团队记忆中枢（21.7K★，TypeScript）
**核实方式：gh api + README 直读（已核实）；部分数字二手来源（标注）** · 来源 https://github.com/TencentCloud/TencentDB-Agent-Memory
定位：团队级 memory hub，把对话/文档/代码转成**四种可复用记忆资产**：Chat Memory、Skill、LLM-Wiki、Code-Graph，资产跨框架迁移共享（OpenClaw/Hermes/Claude Code/CodeBuddy），"新成员第一天就能加载团队的存档"。
机制（README 已核实）：
- **Chat Memory 分层蒸馏**：L0 Conversation（原文，可核词句/时间戳/来源）→ L1 Atom → L2 Scenario → L3 Persona（长期画像）；检索分层：通常 L2/L3 快速 bootstrap，需要具体事实时 BM25+向量+RRF 回落到 L1/L0，且**结果按条数/字符数/超时封顶**防止记忆挤爆上下文。
- **上下文卸载**：工具全量输出写 `refs/*.md`，上下文只留一行摘要+索引路径；Mermaid 任务画布组织任务图（L0 原文→L1 offload.jsonl→L2 画布节点→L3 任务索引，node_id 全程可溯源）。
- 基准：README 实测 PersonaMem 48%→76%（已核实）；博客称 61% token 节省、超长会话任务通过率 33%→50%（二手来源，未核实原文）。
- **无 arXiv 论文**（arXiv API 查无"TencentDB"结果；官方来源=GitHub+腾讯云开发者博客）。
对比：L0→L3 蒸馏是 facts.md 传送带的**升级样本**（三层 → 四层 + persona 层）；卸载与任务画布与上下文压缩**正交**（压缩处理对话文本，卸载处理工具结果），可进 context-compression-plan 的补充策略；检索预算封顶应写进 project() 注入规范。

### mnemosyne-oss/mnemosyne — 零云单文件记忆（2.5K★，Python）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/mnemosyne-oss/mnemosyne
定位：Hermes-first 的通用记忆层（MCP/SDK/插件接入 Claude Code/Cursor/Codex/OpenWebUI/OpenClaw 等），`pip install` 一个包、一个 SQLite 文件、无外部服务。
机制（README 已核实）：**BEAM** = Working（热上下文自动注入，TTL 驱逐）+ Episodic（长期，sqlite-vec + FTS5 混合检索）+ TripleStore（时序 KG 版本链）；混合打分 **50% 向量 + 30% FTS5 + 20% 重要度**，全在 SQLite 内；二进制向量 MIB 把 384 维 float32 压成 **48 字节（32 倍）**；**recency halflife 衰减（默认 168h）**；`mnemosyne sleep` 手动跑巩固；**双向 delta 同步 + 客户端加密**（Fernet/PyNaCl，远端只看到元数据/事件 ID/时间戳）。自报 LongMemEval 98.9% R@5（Apr 2026，自报）与 BEAM（ICLR 2026）成绩。
对比：同名项目众多（28naem-del、FrankHu-HK、smfworks、edlontech 等小仓库），调研锁定 mnemosyne-oss 为"与 Agent 记忆最相关"的一个（多义已如实记录）。可吸收：**单文件 SQLite + sqlite-vec 是 memory-vector 二期的低运维首选**（与"可审计"契合：一个文件即全部状态）；halflife 参数化是淡化机制的最简实现；但它是状态库而非日志投影（缺可重建性，我们更强）。

### HippoRAG / HippoRAG 2 — 神经生物学启发记忆（3.9K★，Python）
**核实方式：gh api + README 直读；论文 2405.14831 / 2502.14802 经 arXiv API 复核（已核实）** · 来源 https://github.com/OSU-NLP-Group/HippoRAG
定位：仿人脑长期记忆的 RAG：离线用 OpenIE 抽三元组建 KG（便宜、用标准 LLM），在线 Personalized PageRank 多跳检索。HippoRAG 2《From RAG to Memory》（2502.14802）强调**关联性（多跳检索）与意义构建（大上下文整合）**，不牺牲简单任务表现，离线索引资源消耗远小于传统。
对比：**离线贵活便宜、在线检索快**的切分 = BoenMind `maintain`（后台异步维护）与 `retrieve`（热路径）分离的依据；多跳关联检索可作为 memory-vector 二期与知识图谱路线之间的折中参照。

### Memary — 仿人脑多记忆系统（2.6K★，Python）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/kingjulio8238/Memary
定位：emulates human memory：episodic/semantic/procedural 多系统 + 情感评分，Neo4j/Weaviate/FalkorDB 后端，多 agent 场景。**2024-10 后停更**。
对比：价值在于"多 agent 需要每 agent 独立记忆 + 共享层"的早期实践（与 MemPalace wings、agentmemory team memory 同结论）；停更说明单点记忆库难以维系，BoenMind 的插件化+事件日志路线更可持续。无独特机制可吸收。

### basic-memory — 文件式记忆 MCP（3.7K★，Python）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/basicmachines-co/basic-memory
定位：Claude Code 生态最普及的记忆 MCP：笔记 = Markdown 文件（人可读可编辑），SQLite 做索引与语义链接（entities/relations 自动提取），MCP 工具读/写/搜；托管云版 + Teams 共享工作区。
对比：**人类可读 markdown + MCP** 与我们 memory-file（facts.md 传送带）同路线，佐证首版选择；其"文件即真源、索引可重建"也是投影思想（文件版）。

### mcp-memory / codebase-memory-mcp
**核实方式：gh api（已核实星数）；原 mcp-memory 项目下落不明（未核实）** · 来源 https://github.com/DeusData/codebase-memory-mcp
原 doobidoo/mcp-memory-service（2025 年约 2.3K★ 的知识图谱记忆 MCP，二手记忆）账户已 404，同名 fork 均 <10★。领域内当前最大 MCP 记忆类项目是 **DeusData/codebase-memory-mcp**（39.0K★，纯 C，单静态二进制）：把代码库索引成持久知识图谱（158 语言、亚毫秒查询、99% token 缩减、43 个 agent 接入面）。
对比：印证"代码/知识资产化记忆"是 2026 年 MCP 侧最热形态（与 TencentDB Code-Graph、my-wiki 同趋势）；BoenMind 编程应用的记忆形态（§6.10 差异点⑤"编程学到的东西其他应用也用得上"）可参考其"索引持久化 + 跨 agent 复用"。

### NimaChu/my-wiki（原名 Agent-wiki）— Agent 的知识 wiki（123★，JavaScript）
**核实方式：gh api + README 直读；重命名经 API 重定向证实（已核实）** · 来源 https://github.com/NimaChu/my-wiki
定位：**本地优先、可直接交给 AI Agent 使用的知识管理项目**：保存原始资料（raw 证据/快照/图片）→ 提取可读文本 → 蒸馏原子 Wiki → 建立双向关系与**证据链接** → Viki 问答 → 知识维护（去重/漂移控制）→ 可视化"知识宇宙"网页。默认不需要云数据库/向量库/Obsidian/付费 API；`AGENTS.md` 是 Agent 入口；可选轻量 my-wiki-skill 安装到 Codex/Claude Code/OpenCode 供其他工作区调用。
对比：命中两个 BoenMind 痛点——**"知识沉淀"**（分身交接浓缩的一种落地形态：知识不再随会话蒸发而是进 wiki）与**证据链**（与 memory/write 的 provenance 字段同思想）；其"双界面（Agent 项目目录 + 网页）共享同一本地知识库"与"万物皆插件 + 前端渲染即投影"同构。星数小但机制干净，值得吸收 wiki 化 + 证据链。

### agent-crystallize — 工作晶体固化（6★，TypeScript）
**核实方式：gh api + README 直读（已核实）** · 来源 https://github.com/stewie-sh/agent-crystallize
定位："Stop compacting. Start crystallizing."——压缩让模型活下来，**结晶让工作可恢复**。压缩前/交接前/会话结束前把工作状态固化为带 provenance 的 Markdown 晶体：checkpoint（进行中高频）+ session crystal（current focus、decisions、evidence、open loops、changed files、tests、next actions、memory candidates）；可选 **Continuity Tail**（少量脱敏原文，仅用于语气/细节恢复，不作为固化知识）。
对比：**直接命中"分身交接浓缩"痛点**——其晶体字段（decision/evidence/open-loop/test/next-action/memory-candidate）可直接模板化为 BoenMind `session.transfer` 简报与 memory/write 交接事件；"压缩前固化"与 on_pre_compress 钩子重合。星数虽小，概念最贴。

### 其他
- **Khoj**（36.5K★，Python，自托管第二大脑）：常被记忆对比表拉进来（agentmemory 对比表含 Khoj），属个人知识检索+agent 而非 agent memory 框架，仅作背景。
- **Anthropic《Effective context engineering for AI agents》（2025-09 官方文章，WebFetch 已核实）**：记忆应放在上下文外的文件/笔记里按需加载；压缩保留架构决策/未决 bug/重要实现细节，"先清掉工具调用与结果"是安全的第一步；子代理在干净上下文深工作、只返回浓缩摘要。**与 BoenMind"模型可见即已记录"互补**：可见的必须记录，但不必让一切常驻可见。
- **OpenMemory**（mem0ai/openmemory，30★）：已从"个人记忆助手"转型为"跨 Claude Code/Codex/OpenCode 的会话搬运 CLI/TUI"（已核实），与 agent-crystallize 同属"状态外置"趋势。

---

## ③ 可吸收点汇总表

| # | 机制 | 来自 | 我们现状 | 吸收建议 | 优先级 |
|---|---|---|---|---|---|
| 1 | 记忆=日志投影，原文只在日志、记忆存指针 | MemPalace verbatim；Anthropic 文件外置 | 不变量③已有（投影可重放），但无"指针式记忆"规范 | memory/write 契约支持 `ref`（事件 ID/文件路径）；project() 注入摘要+路径而非全文 | 高 |
| 2 | 自动捕获钩子全集（SessionStart/UserPrompt/PreToolUse/PostToolUse/PreCompact/Subagent） | agentmemory 12 hooks | 事件总线有全部事件点，但记忆插件零实现（仅 on_tool_post 注释） | 记忆插件改为**订阅事件总线**（异步写），on_turn 保留为同步写入口；maintain 消费队列 | 高 |
| 3 | memory/write 契约：provenance + version/supersedes + validity | agentmemory supersession/provenance；Graphiti bi-temporal | 仅定义 `memory/write` 事件类型，无人发出/消费（痛点） | 字段最小集：`op(add/invalidate/forget)`、`source_event_ids`、`confidence`、`validity_from/to`、`supersedes`；日志天然支持 | 高 |
| 4 | 淡化三机制：halflife 衰减 + TTL/重要度驱逐 + 矛盾检测 | mnemosyne halflife；agentmemory Ebbinghaus+auto-forget | 无（痛点：老记忆淡化） | maintain 流水线实现 decay/strengthen/contradiction；参数化 halflife；显式 `forget` 工具进 tool_schemas | 高 |
| 5 | 分身交接浓缩模板（decision/evidence/open-loop/test/next-action/memory-candidate） | agent-crystallize；agentmemory /handoff /recap | §6.6 transfer 有简报概念，无字段模板（痛点） | 晶体字段模板化进 transfer 简报 + memory/write 交接事件；与 on_pre_compress 复用同一固化管线 | 高 |
| 6 | 分层巩固：Working→Episodic→Semantic→Procedural / L0→L3+Persona | agentmemory 4-tier；TencentDB L0-L3 | facts.md 传送带 3 档（today/week/longterm） | 传送带升 4 层 + procedural/persona 层；每层升层即淡化（today→week→longterm） | 中高 |
| 7 | 检索预算封顶（条数/字符/超时） | TencentDB；agentmemory token budget | 无 | project() 注入加预算上限，防记忆挤爆上下文（呼应 §14.5 预算裁剪） | 中高 |
| 8 | 上下文卸载（工具结果→refs 文件+一行索引；任务画布） | TencentDB（61% token 节省，二手） | ctx-compactor 全量摘要；§14.5 预算裁剪已落地 | 卸载作为压缩补充策略：工具大结果外置成文件、日志记指针 | 中 |
| 9 | 单文件 SQLite + sqlite-vec + FTS5 + 二进制向量 | mnemosyne（50/30/20 打分） | vector 二期未实现 | memory-vector 默认 sqlite-vec（零运维、可审计），混合检索 RRF | 中 |
| 10 | ADD-only 写 + 单 pass 提取 | mem0 新算法 | 契约未定 | 首版 op 集最小化（add/invalidate），update/delete 后置 | 中 |
| 11 | 实体链接提升检索 | mem0；agentmemory KG+RRF | 无 | memory-vector 二期可选；与 #9 的 RRF 融合一并设计 | 低中 |
| 12 | sleep-time compute：离线预计算 | Letta（2504.13171 已核实） | maintain 已有、调度器已有 | maintain 定时夜跑预计算（巩固/预想查询），接 next_wake_at 节奏 | 低中 |
| 13 | 多 agent 命名空间（私有+共享+身份装配） | agentmemory team memory；MemPalace wings；TencentDB identity-based | 应用会话 scope 已有；Steward governance.memorize 已规划 | 记忆投影按 agent/team scope 隔离；Steward 写入留审计事件（已有规划，保持） | 低 |
| 14 | 记忆→提示词优化（第三个输出形态） | LangMem prompt optimizer | project() 注入 | maintain 远期可产出"提示词改进建议"事件（非首版） | 低 |
| 15 | wiki 化沉淀 + 证据链 + 双向关系 | my-wiki；TencentDB LLM-Wiki | facts.md 平文件 | longterm 链接化（双向引用+证据链），人类可读保持 | 低 |
| 16 | 记忆资产跨框架可迁移（Skill/Wiki/CodeGraph 资产化） | TencentDB | 插件内私产 | 保持"投影可从日志重建"即可迁移——无需复制资产仓库设计 | 低 |

---

## ④ 未核实 / 存疑清单

1. **Bifrost（agent 记忆）——未核实**。GitHub 全库检索无同名 agent 记忆项目：maximhq/bifrost（7.3K★）是 LLM 网关（"Fastest enterprise AI gateway"），与记忆无关；second-state（org 存在）、Telosnex（org 存在）、WasmEdge 组织下均无 Bifrost 仓库；`bifrost in:name` 扫描仅见 0★ 的 sophusblom/bifrost-memory-graph。2025 年末传闻的"Bifrost 记忆层"未能定位。**请用户提供链接后复核**。
2. **FastMEM——未核实**。matrus2 账户仍在，但 fastmem 仓库已删除（404）；WebSearch 与 GitHub 检索均无同名记忆项目，仅剩 FastMCP 混淆项（Oracle/Redis 等基于 FastMCP 的记忆服务）。历史项目（2025 年中"最快本地记忆服务器"）现不可考。
3. **checkpoint-mcp——未核实**。carbonated-ale 账户 404；`checkpoint-mcp in:name` 最高 1★。原项目（2025 年末约 1.5K★，二手记忆）已消失/改名。同概念替代 agent-crystallize 已核实（6★，活跃）。
4. **doobidoo/mcp-memory-service——未核实**。账户 404，同名仓库均为 <10★ fork。当前 MCP 记忆类最大项目为 DeusData/codebase-memory-mcp（39.0K★，已核实）。
5. **TencentDB-Agent-Memory 论文——未核实/存疑**。arXiv API 查无 TencentDB 相关论文；"曾出现在 LLM agent 记忆综述"未能验证。官方来源仅 GitHub + 腾讯云开发者博客；61% token 节省与 33%→50% 通过率为博客二手数字（README 只实测 PersonaMem 48→76%，已核实）。
6. **ai-imitation/mempalace——仿冒/镜像**。0★ 同名镜像（README 相同）；官方唯一仓库 MemPalace/mempalace 自身 README 警告仿冒站点。"Milla Jovovich 发布 MemPalace"（rtrunews）可信度极低，不予采信。
7. **MemPalace 星数异常**：58.4K★ 但仓库创建于 2026-04（约 4 个月），增速异常高。数字为 API 实测（已核实数据本身），但社区真实性建议留意。
8. **NimaChu/Agent-wiki 重命名**：gh api 请求 `NimaChu/Agent-wiki` 经 GitHub 重定向返回 `NimaChu/my-wiki`（123★，2026-07 创建）——**已核实为改名**，按 my-wiki 调研。
9. **agentmemory 与 elizaOS 无关**：elizaOS 组织下无 agentmemory 仓库（已核实）；rohitg00/agentmemory 为独立项目（27.0K★）。用户若指 eliza 的 agent memory 插件，应另查 elizaOS 仓库内 plugin（本次未展开）。
10. **benchmark 数字一律厂商自报**：mem0、Zep、Supermemory、agentmemory、MemPalace、mnemosyne 各自的 LongMemEval/BEAM 分数互有出入且存在公开争议（MemoryAgentBench 指出冲突解决普遍差，二手来源）——**未独立复核**，仅作相对参考。
11. **WebSearch 配额限制**：本次调研后半程 WebSearch 触发限流，故二手结论全部以 GitHub API + README 直读 + arXiv API + 官方文章 WebFetch 为主源；少量 WebSearch 摘要已逐条标注"二手来源"。
12. **mnemosyne 多义**：同名项目 ≥6 个（mnemosyne-oss / mnemosyne-proj / 28naem-del / FrankHu-HK / smfworks / edlontech / @studiomosaiko npm 包），已锁定 mnemosyne-oss/mnemosyne（2.5K★，Hermes 生态，最相关），其余如实记录。

---

## ⑤ 关键论断复核标注

**已核实（gh api 实测，2026-08-15）**：上表全部星数/语言/push 日期；mem0 63,275★、MemPalace 58,373★、agentmemory 27,017★、TencentDB-Agent-Memory 21,702★（org=TencentCloud，非 Tencent）、mnemosyne-oss 2,516★、my-wiki 123★（Agent-wiki 改名）、codebase-memory-mcp 38,956★。
**已核实（README 直读）**：agentmemory 12 hooks / 4-tier 巩固 / Ebbinghaus 衰减 / auto-forget（TTL+矛盾+重要度）/ supersession / provenance / team memory；TencentDB L0-L3 蒸馏 / 四资产 / PersonaMem 48→76% / 检索预算封顶；MemPalace verbatim / 可插拔 backend / 时序 KG（validity windows）/ 压缩前 auto-save hooks；mnemosyne BEAM 架构 / 50-30-20 打分 / 48B 二进制向量 / halflife 168h / 加密 delta sync；mem0 2026-04 新算法（ADD-only、实体链接、多信号检索）；LangMem 三类能力；agent-crystallize 晶体字段；my-wiki 证据链 + 双界面；Letta 主仓 legacy 化、SDK 转 TypeScript。
**已核实（arXiv API）**：MemGPT=2310.08560《MemGPT: Towards LLMs as Operating Systems》；Zep=2501.13956《Zep: A Temporal Knowledge Graph Architecture for Agent Memory》；HippoRAG=2405.14831；HippoRAG 2=2502.14802《From RAG to Memory》；sleep-time compute=2504.13171《Sleep-time Compute: Beyond Inference Scaling at Test-time》（Letta 论文，内容经 WebFetch 复核）。
**已核实（官方文章 WebFetch）**：Anthropic context engineering 要点（文件外置记忆、压缩保留决策/未决 bug、先清工具结果）。
**未核实**：见 ④ 清单（Bifrost、FastMEM、checkpoint-mcp、doobidoo、TencentDB 论文与 61% 数字、各家 benchmark 分数、MemPalace AAAK 压缩格式（仅第三方分析文提及）、"MemPalace 100% LongMemEval"（rtrunews 来源弃用））。
