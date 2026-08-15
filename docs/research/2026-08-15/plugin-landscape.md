# BoenMind 插件生态横向调研（2026-08-15）

> 调研子代理产出。只写本文件，不动仓库其他文件，未运行构建。
> 方法：先读自有插件源码（`backend/plugins/`），再对每个插件类别找全网同类前列项目对比，重点源码级核实 deepseek-harness（dsh）的前端插件机制。
> 来源 URL 随段给出；关键论断做二次验证并标注 **已核实 / 未核实**。
> dsh 源码基线：本地浅克隆 `D:/96_CoderWorld/deepseek-harness`（最后提交 47f9438，2026-08-13，v0.1.0-rc.5）。

---

## ① 我们插件现状一览（读代码所得）

### 插件机制

- 插件语言 TS（swc 转译），QuickJS 沙箱运行，hostcall 桥（`pi.http` / `pi.tool` / `pi.on` 等）由 Rust 宿主提供。
- 扩展面来自 `@mariozechner/pi-coding-agent` 的 `ExtensionAPI`（pi.dev 生态），manifest 为 `extension.json`（schema `pi.ext.manifest.v1`：extension_id / capabilities / settings / testSources / quota）。
- 权限三档（含 YOLO），`extension-permissions.json` 权威。
- 沙箱事实（源码注释与 README 中已由团队验证）：`node:fs` 写是 VFS 内存层不落盘、真实持久化必须 `pi.tool("write")`；读有 host 回退；`pi.http` 仅 GET/POST、TLS 强制；exec 被政策拒绝；npm 包不可导入（pdf-omni 因此做成 TS 薄壳 + Rust 端点）。

### 各插件功能

| 插件 | 现在做什么 | 怎么实现 |
|---|---|---|
| `hello.ts` | 最小自定义工具示例 | `pi.registerTool` + TypeBox schema，execute 返回 content/details |
| `bookmark.ts` | `/bookmark [label]` 给最后一条 assistant 消息打标签、`/unbookmark` 撤销；标签出现在 /tree 导航 | `pi.registerCommand` + `ctx.sessionManager.getEntries()` 反查 + `pi.setLabel` |
| `web-search/` | `web_search` 多源聚合搜索（jina/tavily/exa/serper + 自定义 JSON API 源）+ `web_fetch` 网页正文提取 + `search_usage` 用量查询 | 纯插件内 TS：免费额度账本（quota.json，按项目）、失败惩罚窗口（5min）、429 冷却 1h 复活、剩余额度比例+今日调用加权选源、URL 规范化去跟踪参数去重、标题 shingle（中英）转载合并、结果缓存 JSONL、Firecrawl→Jina 提取级联、SSRF 防护、Tavily /usage API 校准 |
| `ctx-compactor/` | `ctx_execute` 沙箱 JS 执行（Think in Code）、大工具输出进模型前修剪为自描述占位符并落库、`ctx_search` 词频检索找回原文、压缩触发观测 | 挂 `tool_result` post-exec 事件（SDK 路径可用）改写结果；白名单豁免（web_search/web_fetch/subagent 等 5 个）；秘密扫描（sk-*/AKIA/ghp_/bearer 等 6 类正则）；索引 JSONL 按项目分桶、8MB 轮转丢最旧一半；占位符含检索 key + 原文前 300 字符摘要 |
| `pdf-omni/` | `parse_pdf`：PDF→高保真 Markdown，MinerU 主 + LlamaParse 增强/交叉验证，级联三级分桶省 credits，多 key 串行预算账本 | TS 薄壳（~120 行）只做 schema/参数校验/透传，经 loopback `POST /api/plugins/pdf-omni/parse` 交给 bm-server Rust 核心（mineru.rs/llamaparse.rs/pdf_ops.rs/verify.rs/refine.rs/budget.rs）；API key 由端点读插件设置文件（单源） |
| `refine-suggest/` | `submit_refinement_suggestions`：代理完成任务后提交针对 skill description / 系统提示词的改进建议 | 插件是"记录桩"（不落状态、不生效）；bm-server 在 toolCallStart 事件流截获参数写入 refinement_suggestions 表（status=pending），用户设置页审批后由宿主改 SKILL.md（备份可回滚）或追加 custom_system_prompt |

**共同点**：全部经 `pi.registerTool` / `pi.registerCommand` / `pi.on` 扩展；配置走 extension.json settings（schema 驱动设置页）；无任何插件贡献 UI 页面（设置页是宿主按 manifest 生成，插件只声明 schema）。

---

## ② 每个类别：同类前列项目对比

### 2.1 web-search（多源搜索 / 免费源管理 / 失败惩罚 / 自动切换）

**同类前列项目表**（星数为 2026-08-15 GitHub API 查询，已核实）

| 项目 | 定位 | 星数 | 语言 | 与我们差异 |
|---|---|---|---|---|
| Tavily | AI 搜索 SaaS（2026-02 被 Nebius 收购，未核实官方公告） | 无主仓库 | SaaS | 我们把它当免费源之一接入 |
| Firecrawl（/search） | 搜索+抓取+清洗一站式 SaaS + 开源 | 167,431（firecrawl/firecrawl） | TS | 我们只用于 web_fetch 提取 |
| Exa | 神经/语义搜索 SaaS（Instant <200ms） | exa-mcp-server 4,867 | SaaS | 我们当免费源接入 |
| Brave Search API | 独立索引搜索 SaaS | 无主仓库 | SaaS | 未接入（未核实免费额度口径） |
| SearXNG | 自建元搜索引擎（80+ 引擎聚合） | 35,480 | Python | 我们自定义源即为此设计（URL 模板 + JSON 路径解析） |
| Serper | Google SERP SaaS | 无主仓库 | SaaS | deep 档付费源 |
| open-deep-research（langchain-ai） | 深度研究编排（supervisor+研究者多 agent） | 12,613 | Python | 我们无编排引擎（控制权留给模型） |
| search-mcp-rotator 等多源 MCP | 多源搜索 MCP server 群 | 千级 | TS/Python | 同为"多源+fallback"，机制各有侧重 |

**逐项目分析 / 可吸收点**：

- **open-deep-research**（https://github.com/langchain-ai/open_deep_research）：Tavily 只做 **URL 发现**，正文自己 fetch（browser-like UA + markdownify）；`think_tool` 四段反思（现状/缺口/证据质量/是否继续）；`raw_notes → compressed_research` 状态压缩 + 50k 字符上限。可吸收：深度研究 = 搜索→抓取→反思→压缩的编排模板（我们 web_search 工具描述已指导"拆多个查询"，编排层未做——刻意不做，见 README）。
- **多源 MCP 群**（search-mcp-rotator https://github.com/sahilchouksey/search-mcp-rotator ；multi-search-mcp https://github.com/guptabhishek/multi-search-mcp ；@greatnxy/web-search-mcp；fast-web-search-mcp）：**"多源聚合 + 免费额度分摊 + fallback"是 MCP 生态已充分验证的模式**。rotator 有 8 源 key 轮换 + 熔断器 + 冷却（策略 round-robin/priority/random）；multi-search-mcp 用优先级 fallback 吃各家免费月额度；greatnxy 用 key 池 + 429 暂停。我们与之相比：**更完整**（额度账本 + x-ratelimit-remaining 响应头探测 + Tavily 官方 usage API 校准 + 内容级转载去重 + 缓存），**可吸收**：选源策略做成可配置（priority/random）、circuit breaker 命名对齐（我们有等价物）。
- **Firecrawl /search**（https://www.firecrawl.dev/blog/best-web-search-apis）：差异化在"搜索即抓取"（/search 返回正文而非 snippet）。可吸收：可选 deep 模式自动抓取 top-N 正文（我们目前两段式，模型显式调 web_fetch——保留两段式，但可评估 deep 模式）。
- **Exa**：语义搜索 + Find Similar；我们已接（free 档）。无新吸收点。

**来源**：https://github.com/langchain-ai/open_deep_research 、https://lobehub.com/zh/mcp/sahilchouksey-search-mcp-rotator 、https://github.com/guptabhishek/multi-search-mcp 、https://www.firecrawl.dev/blog/best-web-search-apis 、https://dev.to/bolshchikov/open-deep-research-internals-a-step-by-step-architecture-guide-2ibk

### 2.2 pdf-omni（MinerU+LlamaParse 级联三级分桶 / 端点 loopback / 预算账本）

**2026 谁最好**（星数已核实；基准数据来自第三方，未核实厂商偏倚）：

| 项目 | 星数 | 语言/许可 | 2026 结论 |
|---|---|---|---|
| MinerU（opendatalab） | 77,644 | Python/AGPL | 公式/表格最强（独立实测），但删脚注、造假 H1；有 pipeline/VLM 双后端 |
| marker 2（datalab-to） | 38,746 | Python/GPL | olmOCR-bench（Datalab 自测 1403 PDF）：76.0% 综合第一 > MinerU 72.7% > docling 50.3%；数字可能有厂商偏倚 |
| docling（IBM） | 64,773 | Python/MIT | 表格强、无 GPU 可跑、MIT；但丢公式和图片（独立实测）；peer-review 论文（36 份葡语文档）配置后 94.1% 第一——"分层分块+元数据比转换器本身更重要" |
| Stirling-PDF | 89,511 | Java | PDF 工具箱（合并/拆分/OCR），非 LLM 解析，不直接竞争 |
| Zerox（getomni-ai） | 12,265 | TS+Python/MIT | 视觉 LLM 逐页 OCR（gpt-4o 系/Gemini/Claude3），零训练；可做第三引擎备选 |

**级联/分桶/预算有没有别人先做**：公开生态**未见与我们同构的实现**（MinerU pipeline→VLM 是后端自动降级；docling 是分层模型；Zerox 是分页并发+重试；"双引擎交叉验证 + 表格/图/公式三级分桶省 credits + 多 key 串行预算账本"组合未检索到先做者——**未核实**，仅基于 WebSearch 覆盖，未做论文/专利级检索，结论降级为"公开生态未见同构实现"）。我们的 pdf-omni 是差异化长板，但缺少量化基准。

**可吸收**：① Datalab 的 olmOCR-bench 方法学（https://github.com/datalab-to/marker 提及）——给我们 pdf-omni 建 A/B 基准（8 页论文→级联 30 credits vs 全文 50 的实测已存在，可扩展为套件）；② docling MIT 许可可作无 GPU 本地兜底引擎；③ Zerox 纯视觉路线作为第三引擎评估项。**不可吸收**：三家都没有"预算账本"概念（额度管理是 BoenMind 特有刚需）。

**来源**：https://github.com/opendatalab/MinerU 、https://github.com/datalab-to/marker 、https://github.com/docling-project/docling 、https://github.com/Stirling-Tools/Stirling-PDF 、https://github.com/getomni-ai/zerox 、dev.to 独立实测（2026-06 前后，链接见搜索记录）

### 2.3 ctx-compactor（压缩/修剪/检索，水线触发）

| 项目 | 定位 | 星数 | 机制要点 | 可吸收点 |
|---|---|---|---|---|
| OpenHands condenser | 事件流压缩 | 84,060（OpenHands） | Condensation tombstone 事件 + View 投影；LLMSummarizingCondenser：max_size（默认~120 events）触发，keep_first=4 头 + 尾保留，中间摘要；软触发失败下次重试、硬触发全量重置；2x 成本下降 | 摘要事件携带被遮蔽区间引用链（我们占位符只留 key 不留来源区间）；condenser 接口化（NoOp/Pipeline/Attention 多实现） |
| dsh compaction | 压缩引擎（四层） | 95,658 | pressure 80% 水线（agent/pre-step 每步测 token）+ overflow 硬触发；摘要+保留尾 16%；SurfaceOp replace + sourceEventSeqs 引用链（压缩可审计可重放）；tool-result-pruner 先裁剪再摘要 | 压缩=可重放事务 + 引用链；两阶段降级；按模型 policies |
| mem0 | 记忆 SDK | 63,275 | 抽取式：每次写入判 ADD/UPDATE/DELETE/NOOP，26k→7k token；无原文、无遗忘原则 | 操作类型判定（记忆插件可吸收）；不可吸收：丢原文（我们保留原文可检索） |
| Zep/Graphiti | 时态知识图谱 | 4,836 | 双时态 episode/entity/relation 子图；FTS/向量检索 | 检索接口的时态维度（我们无） |
| Letta（MemGPT） | 状态化 agent 运行时 | 24,245 | core/recall/archival 三层；代理自编辑记忆（memory_insert/replace/rethink） | 分层记忆 + 工具可见的自编辑面 |
| LangChain ConversationSummaryMemory | 摘要内存 | langchain 主仓 | 保留最近 N 条 + 滚动摘要（最早的家用方案） | 我们水线压缩同模式（无新点） |
| Claude Code | 压缩 | 141,474（anthropics/claude-code） | 92% auto-compact 阈值、8 节压缩算法、/compact /clear /rewind；压缩保留 system prompt + root CLAUDE.md（**已核实官方文档**） | 压缩保留白名单 |
| Cline | 压缩 | 66,204 | auto-compact ≥80% 触发 + Focus Chain；**教训：触发检测依赖 model family 判断，provider 别名不一致导致跳过压缩（issue #8315）** | 触发逻辑必须 provider 无关（我们宿主侧 Rust 不踩此坑） |

**我们 vs 它们的差异**：我们的修剪是"工具结果级、原文可找回"（占位符 + 索引 + 秘密扫描 + 白名单豁免），dsh pruner 直接丢中间、Claude Code 压缩丢细节、mem0 丢原文——**"修剪必须配检索"这条我们是最完整的实现之一**。检索是词频扫描，弱于 dsh 的 SQLite FTS5 和 Zep 的向量/图检索。

**来源**：https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/context/condenser/README.md 、https://openhands.dev/blog/openhands-context-condensensation-for-more-efficient-ai-agents 、https://code.claude.com/docs/en/context-window 、https://github.com/cline/cline/issues/8315 、https://github.com/cline/cline/discussions/3248 、https://github.com/ruvnet/RuVector/blob/feat/mincut-decompiler/docs/research/claude-code-rvsource/07-context-and-session-management.md 、https://github.com/mem0ai/mem0 、https://github.com/getzep/zep 、https://github.com/letta-ai/letta

### 2.4 refine-suggest（建议审批）

- **Self-Refine（arXiv 2303.17651）/ CRITIC（arXiv 2305.11738）/ Reflexion**：生成-批评-修订循环。关键结论（Snorkel 悖论，https://snorkel.ai/blog/the-self-critique-paradox-why-ai-verification-fails-where-its-needed-most/ ）：**无外部信号的自批评在简单任务上有害（98%→57% 案例）**；外部验证（工具/用户）才可靠。→ 直接支持我们"代理只提建议、用户审批生效"的设计（宿主审批 = 外部信号）。
- **Prime Agent /refine**（https://github.com/PrimeIntellect-ai/prime-agent ，15,927 星）：trajectory 复盘 + 证据支持的小更新 + snapshot 回滚 + 永不改不可变基础提示词——我们 refine-suggest 的心智来源，差异（有意）：上游自动生效，我们加审批门（上游 Factorio 演示曾把作弊经验 refine 进知识库——README 已记）。
- **PR-Agent/Qodo**（https://github.com/The-PR-Agent/pr-agent ，12,545 星，Apache 2.0）：/describe /review /improve /ask；multi-agent + context engine + Rules 系统；review 严重度分级。可吸收：/improve 一键应用、严重度分级、rules 绑定。
- **CodeRabbit**（SaaS，2M+ repos）：40+ 确定性 linter 先行 + LLM 审查；.coderabbit.yaml 自然语言配置；增量审查（只审新 commit）；~90s 延迟、~15% 误报（第三方对比口径，未核实）。可吸收：确定性层先行、增量审查。
- **Claude Code code-review 官方插件**（~43 万安装）：置信度打分评审——机制细节未核实。

**可吸收汇总**：① 建议携带证据引用/置信度/影响范围（我们目前只有 reason 一段文本）；② 确定性检查层先行（未来若做代码 review 类功能）；③ 批量审批 UI（现状逐条审批，无批量）。

### 2.5 bookmark（书签）

我们：会话内标签（setLabel）+ /tree 导航 + /bookmark //unbookmark 命令，仅"最后一条 assistant 消息"粒度。同类 agent 工具（https://github.com/wickes1/chromium-bookmarks-mcp 、https://github.com/infinitepi-io/bookmark-manager-mcp 、Agentic Bookmarks、keeply-mcp、oibars knowledge-graph）：浏览器书签读写、JSON 持久化+分类+搜索、VS Code .bookmarks/ git 支持（28 工具）、书签→知识图谱（kg_search/kg_add_entity）。可吸收：分类 + 搜索 + 跨会话持久化（我们标签持久于会话存储，但无搜索/分类）；knowledge-graph 化思路可直接给未来"收藏/笔记"应用插件。pi.dev 生态未检索到 bookmark 扩展（未核实）。

### 2.6 通用插件机制（我们插件商店路线的参照）

| 生态 | 现状（2026-08） | UI 贡献面 | 对我们商店路线的参照 |
|---|---|---|---|
| pi.dev（Earendil Inc，pi-coding-agent） | npm/git 安装、50+ 示例（站上口径，**未核实 200+**）；TUI 框架 | **TUI UI**：状态行、overlay、自定义编辑器界面、快捷键（https://pi.dev/ ） | BoenMind ExtensionAPI 即其扩展面；商店可对接（此前已拍板） |
| Claude Code plugins + marketplace | 官方 marketplace + 聚合器 9k~31,904 个（口径差异大，未核实官方数）；组件：skills 175k / commands 59k / agents 47k / MCP 7.6k / hooks 7k / LSP 521（ClaudePluginHub 口径） | 无页面级 UI（skills/commands/hooks/MCP/agents/LSP/output styles） | `.claude-plugin/plugin.json` 成为事实标准（ZCode/OpenHands 均兼容探测） |
| ZCode 插件市场 | 官方市场 zcode-plugins-official（本机缓存核实 7 个插件：browser-use/document-skills/android-emulator 等）；marketplace.json（directory/github/git/url/git-subdir 源） | **无**（plugin.json = name/version/commands/skills/hooks/mcpServers/userConfig，**本机核实**） | 与架构文档"ZCode 无页面贡献点"论断一致 |
| OpenHands microagents | markdown+frontmatter 微代理（skills 约定）；插件格式兼容 .claude-plugin/；AgentSkills 标准推进中 | 无 | 组件级标准跨家统一趋势（https://docs.openhands.dev/overview/plugins ） |
| MCP 2026 | 官方 registry：3,454（2026-01）→ ~9.4k~14k（2026-04）→ ~19k~22k（2026-05~07，口径不一）；SDK 月下载 97M+；80% 云环境有 MCP；2025-12 移交 AAIF（Linux Foundation）；**质量危机：仅 9% 端点全健康、~41% 无认证** | 无 UI | 插件商店路线：先 curated list 后市场；MCP 质量治理是反面教材 |

**来源**：https://pi.dev/ 、https://github.com/anthropics/claude-code 、https://claudepluginhub.com 、https://docs.openhands.dev/overview/plugins 、https://docs.all-hands.dev/usage/prompting/microagents-overview 、https://learnagent.org/library/updates/mcp-ecosystem-2026-h1/ 、https://dugganusa.com/post/praise-jeevesus-we-mapped-every-mcp-server-and-we-re-auditing-them-next 、ZCode 本机缓存 `C:/Users/Boen/.zcode/cli/plugins/cache/zcode-plugins-official/` 与 zcode-guide diagnosing-plugins SKILL.md

---

## ③ dsh 前端插件机制专项（重点，源码核实）

**对照文档**：`docs/deepseek-harness-evaluation.md` 的 §4.10 应用壳（"apps/web: Vite 6 + React 18 SPA"）——**已核实**（apps/web/package.json：vite ^6.0.0、react ^18.2.0），但该文未展开前端插件化细节，本专项补齐。

### a. console/webui 是不是独立插件包？贡献点如何注册？

**是——整个 Web UI 都是插件组装**。"console"即 Web UI（`npx @deepseek-ai/dsh web` → http://127.0.0.1:3080 ，README 已核实）。分层：

1. **`apps/web` 是 ~20 行引导**（main.ts → `AppWebEntry`），无业务 UI；"loader holding、module-table seeding、AppRoot gate、plugin assembly 全在 @deepseek-ai/dsh-client-web"。
2. **`dsh-web-app` bundle 的 `cordis.patch.yml`（425 行）列出全部浏览器插件 roster**（已核实，packages/bundle/web-app/cordis.patch.yml）：`ui-theme / ui-layout / ui-sidebar / ui-settings(-general/-models/-plugins/-plugin-inventory) / ui-conversation / ui-tool / ui-workflow-run / ui-deliverables / ui-workspace / ui-input-trigger / ui-commands / ui-skill / ui-subagent / ui-jobs / ui-goal / ui-message-feedback / ui-model-selection / ui-permission / ui-agent-preset / ui-plan / ui-user-questions / ui-trajectory` 等 28 个 ui-* 插件（另有 modules/connection/api-remotes/client-runtime/locale 等客户端底座）——**每个 UI 功能是一个独立 npm 插件包**。
3. 浏览器侧：node 半部把 roster 扫成 `window.__DSH_BOOT__` 入口图，`dsh-client-modules` 建模块表，vendored Cordis Loader 逐条挂载；第三方插件客户端产物经 `/plugins/<id>/client.js` 提供。

**注册机制两层**（均源码核实）：

- **Slot 系统**（`packages/client/ui-slots` 纯核心 + `packages/client/runtime/src/client/slots.ts` 的 Cordis Service 层）：
  - `SlotMap` 用 TS 声明合并扩展（`declare module '@deepseek-ai/dsh-client-ui-slots' { interface SlotMap {...} }`），注册必须命中已声明槽位（load-time 校验：未声明槽、重复声明、跨 scope store 冲突直接 throw）。
  - 单次 `ctx.slots.register({ name, children?, store?, inject?, key?, id?, order?, label?, locale? }, Component)` 同时：贡献组件 + **声明子槽位（声明=渲染授权）** + 挂 store + 注入业务回调；返回 disposer，卸载经 `ctx.effect` 级联撤销（卸载一个声明者连带撤销其声明的全部子槽和 store）。
  - 槽位类型：`single`（独占替换）/ `list`（追加，order 排序）/ `chain`（selector 自荐选举 + fallback）/ `keyed`（按 key 分发渲染器）；scope：`root` / `session` / `session-maybe`。
  - `ctx.slots.inject(key, cb)`：等待槽位被声明后安装贡献（声明存在即同步执行）。
- **ConversationNodeDefinition**（`packages/client/runtime/src/client/contract/conversation.ts`）：事件→节点状态机。`match(event)→{id,role}`（纯身份提取，不做 fold）→ 引擎按 `(kind,id)` 定位 Context → `start`/`update` 产出 State → `publication`（immediate/animation-frame/none 控发布节流）→ `buildLocationData`（向 engine 拥有的 Turn/Step 发布业务值，供兄弟节点经 `useTurnData(key)` 消费）→ `buildViewNode`（产出 target 渲染节点）。三条摄入路径：replace（全量重建）/ prepend（翻旧页，只重放受影响 Context）/ append（每事件 D 次 match + 常数时间查找）。`start` 里可用 `reader.previous(kind)` 查更早的业务 Context。声明合并扩展 `SessionEventMap` / `ChatNodeDataMap` / `ConversationStepDataMap`。
- **具体例子（官方 cookbook，代码核实）**：`docs/cookbook/adding-a-conversation-node.md` 的 review-job 完整示例：定义 `review/start|progress|end` 事件家族（合并进 SessionEventMap）→ Definition 五函数 → `ctx.conversationEvents.register(definition)` + `ctx.slots.inject('conversation.chat.node', () => ctx.slots.register({name:'conversation.chat.node', key:'review-job'}, ReviewNodeView))`。完整代码摘录见本文件附录 A。
- **社区 UI 插件实证（源码核实）**：`ccq1/dsh-side-panel`（@dsh-external/dsh-side-panel v0.2.0，BSD-3）——dual-face 插件：package.json 声明 `dsh.bundle.patch` + `dsh.client.platform:"web"`；host 半部 `inject=['webServer','sessions']` 注册自有 HTTP 路由（`ctx.webServer.register({kind:'exact', path:FILE_BROWSER_ROUTE, handler})`，文件浏览/git diff/终端后端）+ 监听 `session/event`（turn/start|end 做 git 快照）；client 半部 `inject=['sessions','workspaces']` 注入 DOM/React UI（xterm + codemirror + marked）。cordis.patch.yml 3 行 insert。**这证明：第三方插件可贡献完整的前端+后端一体化功能面板**。

### b. ConversationNodeDefinition 之外的 UI 贡献面清单（源码核实的 SlotMap 全量键）

| 面 | 槽位（声明者） |
|---|---|
| 整体壳 | `root`（runtime 内建）→ ui-layout AppFrame 声明 `sidebar` / `conversation` / `details` / `shell.overlay` |
| 侧栏 | `sidebar.workspaces`（会话/工作区浏览）、`sidebar.settings`、`sidebar.footer.action`（list） |
| 会话头 | `conversation.session.header.actions`（list，按 order）、`.utilities`（list） |
| 整页视图 tab | `conversation.view`（list，one-at-a-time；chat 在此，trajectory/waterfall 由 ui-trajectory 注册——**页面级贡献点**） |
| 聊天节点渲染 | `conversation.chat.node`（keyed）、`conversation.chat.commandview`（keyed，按命令名）、`conversation.chat.turnTail`（chain）、`conversation.chat.assistant-actions`（list） |
| 输入区（toolbar 级） | `conversation.composer`（chain 接管）、`conversation.composer.bar`、`conversation.input.plan` / `.input.model`（single 命名座位）、`conversation.input.left` / `.right` / `.dock`、`conversation.composer.dock`（list） |
| 空态 | `conversation.hero.workspace`、`conversation.hero.agentPreset` |
| 详情面板 | `conversation.details.tool`（single，tool 输出详情）、`tool.call.toolview`（per-tool 渲染） |
| 设置页 | `settings.plugin.item`（list，插件配置卡片——每个插件可注册自己的设置卡 UI） |
| 悬浮层 | `shell.overlay`（list，帧级悬浮） |
| 后端 | `ctx.webServer.register`（HTTP 路由 exact/prefix）+ `registerUpgrade`（WebSocket）+ `tapIndex`（index.html 变换）+ fallback 座位；`ctx.storage` 后端 seam |
| 主题/语言 | ui-theme（theme/change 事件 + DOM presenter）、`ctx.locale.register(NS, dict)` |

**结论：dsh 的前端贡献面覆盖 page（view tab）/ panel（details/sidebar/side-panel）/ toolbar（input 区座位）/ chat node / settings card / overlay / 主题 / 语言 / 后端路由——远超"聊天节点注册"**。社区已产出整站替代品：`zhu1090093659/dsh-web-ui`（1,946 星：task board/git graph/右侧面板/移动 UI/token 统计）、TUI（dsh-tianshu-tui、openma-ai/deepseek-harness-tui Rust/ratatui）、`anywhere-labs/deepseek-harness-desktop`（1,886 星桌面壳）、`dsh-market`（插件市场 UI，one-click 装/升/换主题）、`Noob-stupid/dsh-plugin-hub`（插件管理面板）。**"前端就是插件"在 dsh 是完整事实而非萌芽**。

### c. 带前端插件的最小样板（目录结构 + 关键文件）

**官方最小形态（cookbook UI plugin，已核实）**：

```
my-ui/
├── package.json      # name/version; 若需进 web roster：dsh.bundle.patch → cordis.patch.yml，dsh.client.platform:"web"
├── index.js          # export const name/inject; export function apply(ctx)
└── cordis.patch.yml  # - insert: [{id, name, config}]
```

```js
// index.js —— 官方 A UI plugin（extension-cookbook.md 原文摘录）
export const name = 'my-ui'
export const inject = ['agents']
export function apply(ctx: Context) {
  ctx.on('session/event', (_session, event) => {
    if (event.type === 'assistant/chunk' && event.data.chunk.type === 'text-delta') {
      render(event.data.chunk.text)
    }
  })
  onUserInput(text => ctx.agents.get(SessionId('client-session'))?.followup(createUserMessage({
    content: [{ type: 'text', text }], source: { kind: 'user' },
  })))
}
```

**进聊天流的形态**：注册 `ConversationNodeDefinition` + keyed 渲染器（附录 A 全文）；**进页面级的形态**：注册 `conversation.view` list 槽位（附录 B，ui-trajectory 原文）；**前后端一体的形态**：dsh-side-panel 结构（附录 C）。安装：`dsh plugin --profile web add <pkg>`（awesome-dsh-plugin README 已核实）。

### d. dsh 2026-08 星数 / 版本 / 社区插件数量（GitHub API 已核实）

- repo 创建 2026-08-13；**95,658 星 / 8,869 fork**（2026-08-15 API 查询）；v0.1.0-rc.5（本地 package.json + git log 已核实）；最后推送 2026-08-13（无后续提交，MIT）。
- 社区插件：**awesome-dsh-plugin 列表 365 个（README 自报；实际条目 368 行，已核实）**；GitHub topic `dsh-plugin` 共 **2,487 仓库**（search API 已核实）；UI Enhancements 是列表内最大类别之一。对比：docs/deepseek-harness-evaluation.md 记的"2026-08-13 发布，几天内 3.3 万+ star、约 300 社区插件"已被 8-15 数据超越（95.7k / 365+），该文数字需滚动更新。

### e. 结论：对照 BoenMind 架构 §6.4 论断是否成立

**§6.4 定位声明原文**（docs/everything-is-plugin-architecture.md 第 636 行）："应用插件（独立 UI + Agent 核心）是四家都未完全做到的层——dsh 仅有聊天节点注册（ConversationNodeDefinition）的萌芽，ZCode/Hermes/pi 插件均不贡献 UI 页面。"

**判定：论断对 dsh 的部分不成立，需要修正**：

1. "dsh 仅有聊天节点注册的萌芽" —— **错**。dsh 的前端插件化是完整机制：slot 树（20+ 槽位、single/list/chain/keyed 四种语义、声明合并、卸载级联）+ view tab 页面 + settings 卡片 + 后端路由注册 + storage seam + 主题/语言包；整个 Web UI 本身是 ~35 个 ui-* 插件组装；社区 2 天内已产出替代 UI/桌面壳/TUI/插件市场。"前端都是插件"在 dsh 是工程事实。
2. "ZCode/pi 插件均不贡献 UI 页面" —— **成立（核实）**。ZCode plugin.json 仅 commands/skills/hooks/mcpServers/userConfig（本机核实）；pi 扩展只有 TUI 内贡献（状态行/overlay/编辑器界面），无页面级 Web UI。"Hermes" 为私有仓库，未独立核实。
3. **BoenMind 设想的"应用插件 = 壳 + 确定性引擎 + 管家派专家"与 dsh 的真实差距**（这才是 §6.4 论断应改写成的表述）：
   - dsh 已有：前端包（slot/页面/卡片）、后端路由注册、服务 seam（storage/webServer）、事件日志投影、子代理/ACP 委派——**机制零件齐**。
   - dsh 没有（BoenMind 仍是原创）：
     ① **应用级 manifest 权限模型**（capabilities/sensitive/override 声明 + 按 app 裁剪）——dsh 插件是宿主进程内全权 npm 包，无沙箱（沙箱只约束工具产生的进程，不约束插件代码；QuickJS 真沙箱是 BoenMind 的硬优势）；
     ② **"应用"第一公民概念**（安装=获得服务+工具+事件域；app 间能力调用/事件订阅/数据血缘）——dsh 只有 bundle/preset 组装，无 app 边界语义；
     ③ **管家派专家 / 寄生关系**（软件零 Agent 核心，spawn_app_session 裁剪会话、预算上限）——dsh 有 subagent provider 与 preset 但无此产品化概念；
     ④ **插件 UI 隔离加载**（iframe 一次性凭证/Web Component 受控渲染）——dsh 客户端插件直接跑在主 React 运行时（无隔离，崩溃会波及全 UI）。
   - 结论修正建议：**"UI 即插件"不再是我们独有的主张（dsh 已做到且生态已验证需求）；BoenMind 的原创点应聚焦为"受权限治理的应用插件"（manifest 权限 + 沙箱执行 + 裁剪会话 + 隔离 UI + 事件血缘互通）**。§6.4 的"四家都未完全做到"应改为"ZCode/pi/Hermes 无 UI 贡献面（成立），dsh 有完整 UI 插件机制但无应用级权限与隔离（本调研修正）"。

---

## ④ 汇总表（可吸收点 | 来自 | 我们现状 | 建议 | 优先级）

| # | 可吸收点 | 来自 | 我们现状 | 建议 | 优先级 |
|---|---|---|---|---|---|
| 1 | 选源策略可配置（priority/random）+ 熔断/冷却命名对齐 | search-mcp-rotator / multi-search-mcp | 固定加权选源（比例−今日调用−惩罚） | settings 增加选源策略选项；命名对齐便于生态交流 | 低 |
| 2 | 搜索即抓取一站式（/search 返回正文） | Firecrawl | web_search + web_fetch 两段式 | 保留两段式（模型可控）；可选 deep 模式自动抓取 top-N | 中 |
| 3 | 深度研究编排（think_tool 反思 + 状态压缩） | open-deep-research | 无编排层（有意） | 继续不做；工具描述已有拆分指导 | 低（不做） |
| 4 | 公开基准方法学（olmOCR-bench 1403 PDF 套件） | datalab/marker 2 | 仅 8 页论文级联实测 | pdf-omni 建 A/B 基准套件（级联/verify/refine 收益量化） | 高 |
| 5 | 第三引擎备选（纯视觉 LLM OCR、MIT） | Zerox | MinerU + LlamaParse | 评估 Zerox 作为无 GPU/兜底引擎 | 低 |
| 6 | 压缩可审计（replace + sourceEventSeqs 引用链） | dsh SurfaceOp | 占位符只留 key 不留来源区间 | 自研会话日志层时吸收（已列 dsh 评估拍板点 4） | 中 |
| 7 | 压缩事件 tombstone + View 投影 + 多 condenser 实现 | OpenHands condenser | 修剪固定策略 | 压缩方案设计时接口化 | 中 |
| 8 | 压缩保留白名单（system prompt + root CLAUDE.md 幸存） | Claude Code | 水线注入（未明确白名单语义） | 对照现有压缩方案补白名单语义 | 中 |
| 9 | 触发检测 provider 无关 | Cline 教训（issue #8315） | Rust 宿主不依赖模型别名 | 自研压缩时保持 | 低 |
| 10 | SQLite FTS5 检索 | dsh session-query-sqlite / Zep | 词频扫描（ctx_search） | 索引升级 FTS（当前规模可缓） | 中 |
| 11 | 记忆操作类型判定（ADD/UPDATE/DELETE/NOOP） | mem0 | 无 | 记忆插件吸收 | 中 |
| 12 | 建议带证据/置信度/影响范围字段 | PR-Agent 严重度分级 / CRITIC 外部信号论 | reason 单段文本 | refine-suggest 加 evidence/confidence 字段 | 低 |
| 13 | 确定性检查层先行 + 增量审查 | CodeRabbit | 无 review 功能 | 未来 review 功能的设计约束 | 低（远期） |
| 14 | 书签分类/搜索/知识图谱化 | bookmark MCP 群 | 标签 + /tree | 应用插件（收藏/Wiki）参考 | 低 |
| 15 | slot 树式前端贡献面（声明合并/single-list-chain-keyed/卸载级联/作用域） | dsh ui-slots（源码核实） | 应用插件前端三候选（iframe/WC/联邦）待拍板 | 吸收 dsh 槽位语义设计；隔离层仍用我们 iframe/WC 方案（dsh 无隔离） | 高 |
| 16 | 插件注册后端路由 seam | dsh ctx.webServer.register | bm-server /api/plugins/<id>/ loopback 已有 | 已有等价物；接口形态对照即可 | 低 |
| 17 | 组件级插件格式兼容（.claude-plugin/plugin.json + AgentSkills 事实标准） | Claude Code / ZCode / OpenHands 兼容探测（已核实） | pi ext manifest v1（自有） | 商店路线：读入 Claude 格式（skills/commands/hooks），执行层保留我们的 QuickJS 沙箱 | 高 |
| 18 | 先 curated list 后市场（社区目录→插件内市场） | awesome-dsh-plugin → dsh-market | 无商店 | 早期 curated list + git 安装（ZCode marketplace.json 机制可复用） | 高 |
| 19 | 主题/语言包插件化 | dsh ui-theme / ctx.locale | 无 | 应用插件主题支持 | 低 |
| 20 | 可逆副作用 + 卸载级联（ctx.effect 包裹一切注册） | Cordis（dsh vendor 5690 行） | pi 机制有部分卸载语义 | 自研核心时吸收（已列 dsh 评估工作量表） | 高 |
| 21 | MCP 质量治理教训（9% 健康/41% 无认证） | MCP registry 审计 2026 | 无商店 | 商店上架加健康检查/权限评审（差异化卖点） | 中 |

---

## ⑤ 未核实 / 存疑清单

1. **pi.dev "200+ 插件"**：pi.dev 站上只看到"50+ examples"；200+ 来自 BoenMind 内部文档（deepseek-harness-evaluation.md、HANDOFF_EVERYTHING_IS_PLUGIN.md），未独立核实。商店对接前需重新摸底。
2. **dsh 社区插件数口径**：awesome-dsh-plugin 自报 365（实际条目 368，已核实）；GitHub topic `dsh-plugin` 2,487 仓库（含主仓与大量弱相关，已核实数字但口径含噪）。生态成立 2 天，数据点极短。
3. **Claude Code 插件数**：聚合器口径 9k~31,904 差异巨大；官方 marketplace 口径未获得；"43 万安装的 code-review"等安装量来自聚合器。
4. **MCP registry 数**：各源 1,787~21,962 口径不一，无官方实时看板；质量数据（9% 健康、41% 无认证）来自第三方审计（Rapid Claw / AgentMarketCap）。
5. **olmOCR-bench 数字有厂商偏倚**（Datalab 自测）；peer-review 论文（Applied Sciences 2026）结论方向不同（docling 配置后第一）——"谁最好"应写为"分场景"。
6. **"级联/分桶/预算无先做者"**：仅基于 WebSearch 覆盖，未做论文/专利级检索；结论降级为"公开生态未见同构实现"。
7. **Tavily 被 Nebius 收购（2026-02）**：来自一篇对比博文，未核实官方公告。
8. **CodeRabbit 误报率/延迟（15%/90s）**：第三方对比口径，未核实。
9. **Claude Code code-review 插件机制**：仅核实安装量存在，机制未核实。
10. **ZCode 远程市场仓库**：只核实了本机缓存 7 个插件与 manifest schema（plugin.json = commands/skills/hooks/mcpServers/userConfig），市场仓库内容未看。
11. **§6.4 "Hermes 插件不贡献 UI 页面"**：Hermes 为私有仓库，无法独立核实（沿用内部论断）。
12. **dsh eval 文档数据过期**："3.3 万+ star、约 300 社区插件"是 2026-08-14 的时点数据，8-15 已为 95.7k 星 / 365+ 插件，文档数字需滚动更新。

---

## 附录 A：dsh 官方"聊天节点"最小完整示例（docs/cookbook/adding-a-conversation-node.md 摘录，已核实）

```ts
import { createElement } from 'react'
import type { Branded } from '@deepseek-ai/dsh-brand'
import type {
  ClientContext, ConversationLocation, ConversationNodeContext, ConversationNodeDefinition,
} from '@deepseek-ai/dsh-client-runtime/client'
import type { ChatNodeViewProps } from '@deepseek-ai/dsh-client-ui-conversation/client'

type ReviewId = Branded<'ReviewId'>
interface ReviewStartData { readonly reviewId: ReviewId; readonly turn: number; readonly step: number; readonly title: string }
interface ReviewProgressData { readonly reviewId: ReviewId; readonly turn: number; readonly step: number; readonly completed: number }
interface ReviewEndData { readonly reviewId: ReviewId; readonly turn: number; readonly step: number; readonly summary: string }

declare module '@deepseek-ai/dsh-session/types' {
  interface SessionEventMap {
    'review/start': ReviewStartData
    'review/progress': ReviewProgressData
    'review/end': ReviewEndData
  }
}
interface ReviewChatData { readonly title: string; readonly completed: number; readonly status: 'running' | 'completed'; readonly summary?: string }
declare module '@deepseek-ai/dsh-client-ui-conversation/client' { interface ChatNodeDataMap { 'review-job': ReviewChatData } }
declare module '@deepseek-ai/dsh-client-runtime/client' { interface ConversationStepDataMap { 'review-job': ReviewChatData } }

interface ReviewState extends ReviewChatData { readonly turn: number; readonly step: number }
function locationOf(context: ConversationNodeContext): ConversationLocation {
  return context.start?.location ?? context.matches[0]?.location ?? { kind: 'unresolved' }
}

const reviewDefinition: ConversationNodeDefinition<ReviewState> = {
  kind: 'review-job',
  target: 'chat',
  match: (event) => {
    if (event.type === 'review/start') return { id: String(event.data.reviewId), role: 'start' }
    if (event.type === 'review/progress' || event.type === 'review/end') return { id: String(event.data.reviewId), role: 'update' }
    return null
  },
  start: (_context, match) => {
    if (match.event.type !== 'review/start') throw new Error('review-job requires review/start')
    return { turn: match.event.data.turn, step: match.event.data.step, title: match.event.data.title, completed: 0, status: 'running' }
  },
  update: (context, match) => {
    if (match.event.type === 'review/progress') return { ...context.state, completed: match.event.data.completed }
    if (match.event.type === 'review/end') return { ...context.state, completed: 100, status: 'completed', summary: match.event.data.summary }
    return context.state
  },
  publication: match => match.event.type === 'review/progress' ? 'animation-frame' : 'immediate',
  buildLocationData: (context, scope) => { /* 省略：向 engine 的 step 发布 useTurnData 可读的业务值 */ return null },
  buildViewNode: (context) => {
    if (context.state === undefined) return null
    return { key: context.key, kind: 'review-job', id: context.id, target: 'chat',
             anchorSeq: context.start?.event.seq ?? context.matches[0]?.event.seq ?? 0,
             location: locationOf(context), visibility: 'visible', data: { title: context.state.title, completed: context.state.completed, status: context.state.status } }
  },
}

function ReviewNodeView({ node }: ChatNodeViewProps<'review-job'>) {
  const text = node.data.summary ?? `${node.data.title}: ${node.data.completed}%`
  return createElement('p', null, text)
}

export const inject = ['conversationEvents', 'slots']
export function apply(ctx: ClientContext): void {
  ctx.conversationEvents.register(reviewDefinition)
  ctx.slots.inject('conversation.chat.node', () => ctx.slots.register({
    name: 'conversation.chat.node', key: 'review-job',
  }, ReviewNodeView))
}
```

## 附录 B：dsh "页面 tab" 注册（packages/client/ui-trajectory/src/client/index.ts 摘录，已核实）

```ts
export const inject = ['slots', 'conversationEvents', 'conversationViews', 'sessions', 'locale']
export function apply(ctx: Context): void {
  ctx.effect(() => ctx.locale.register(NS, { zh, en }), 'ui-trajectory: dictionaries')
  // ... 注册 5 个 ConversationNodeDefinition（trajectory 视图内部行）...
  registerTrajectoryConversationView(ctx)
  ctx.slots.inject('conversation.view', () => ctx.slots.register({
    name: 'conversation.view',
    id: 'trajectory',
    order: 10,
    locale: NS,
    label: () => t('view.trajectory'),
    inject: (sessionId: SessionId) => ({ /* session 级注入面 */ }),
  }, TrajectoryView))
}
```

## 附录 C：社区"前后端一体"插件结构（ccq1/dsh-side-panel，已核实）

```
package.json        # dsh.bundle.patch → cordis.patch.yml; dsh.client.platform:"web"; exports: "."(host) / "./client"
cordis.patch.yml    # - insert: [{id: side-panel, name: '@dsh-external/dsh-side-panel', config: {...}}]
src/index.ts        # host 半部：inject=['webServer','sessions']; ctx.webServer.register({kind:'exact',path,handler});
                    #           ctx.on('session/event', turn/start|end → git 快照)
src/client/index.ts # client 半部：inject=['sessions','workspaces']; 注入 DOM/React（xterm/codemirror/marked）
```
