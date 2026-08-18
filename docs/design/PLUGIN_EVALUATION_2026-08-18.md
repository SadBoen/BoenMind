# 插件评估与开题计划（2026-08-18）

> 承接 `docs/HANDOFF_KERNEL_FIX_2026-08-18.md` 的"下一轮 = 插件/M5 主线开题"。
> 本轮 = 全网调研（dsh 官方全家桶 / Claude Code 生态 / MCP 生态）+ 本地能力对齐 + 候选清单与拼接方案。
> 原则：**优先保证最小化运行**（Rust 微内核不膨胀、进程外优先、文件式优先、已有不重复）。

---

## 1. 一句话结论

三大生态已收敛到同一形态：**内核极简 + 文件式插件（manifest/SKILL/规则文件）+ MCP 进程外工具总线**——与 BoenMind 的"Rust 微内核 + TS/QuickJS 插件 + MCP"路线完全同构（Claude Code 官方甚至开始用 QuickJS/WASM 沙箱跑插件脚本）。因此**官方全家桶整体是抄架构模式，不抄 JS 实现**；真正值得逐个吸收的是"能力类别"（记忆/浏览器/终端/编排），每类选 1-2 个代表作对齐自家插件面。

## 2. 三大生态全景

### 2.1 dsh 官方全家桶（github.com/deepseek-ai/deepseek-harness，15.5 万星，MIT，developer preview）
- 形态：无插件市场概念，而是"接缝（seam）+ 工具（tool）+ 组合（preset）"三类 npm 包，约 40+ 个 `@deepseek-ai/*` 包。
- 核心组合层：`dsh`（CLI/profile 管理）、`dsh-tools`（工具注册表+守卫管线）、`dsh-session`（事件溯源会话日志，**一切模型可见输入必落日志**）、`dsh-system-prompt`、`dsh-agent-presets`（**拼接核心**，按会话组合 preset）、`dsh-persona`（角色插件官方对应物）。
- 终端/执行（官方最成熟类别）：`dsh-shell`/`dsh-bash-local`/`dsh-bash-sandbox` + `dsh-tool-bash`/`dsh-tool-terminal` + `dsh-terminal-bash`（PTY）——seam 抽象 + local/sandbox 双实现的分层可借鉴。
- 搜索/Web：`dsh-web`（接缝）+ `dsh-tool-web`（web_search/web_fetch 工具）+ provider 卡（deepseek/exa/perplexity 官方三 provider + http fetch）——**搜索插件不是独立插件，是接缝+可插拔 provider 模式**。
- 记忆：**官方空白区**（无独立记忆插件，只提供 3 个默认关闭的 MCP 记忆服务器示例）——正是 BoenMind 差异化机会。
- 其它：session 持久化 jsonl、附件、settings、credentials（`~/.env`）、skill、code-runtime（worker-thread）、todo、Web UI、`dsh-tool-cordis`（**模型可自己挂载/卸载/写插件**，dsh 独有激进能力）。
- 拼接机制：Cordis 插件树（服务+类型化事件+可逆副作用）→ profile→bundle→patch 三层组合（`cordis.patch.yml`）→ 事件瀑布注入（`agent/pre-step` 改写消息=记忆注入点 / `agent/request` / `llm/stream` / `tools/pre-execute|post-execute`）。
- 参考价值最高 3 点：① 接缝三件套（接口+实现+模型工具，一次抽象多处实现）② 事件瀑布注入点（记忆/人设用 pre-step 改写，不塞 system prompt 字符串）③ preset/patch 组合层（任意组合 role+memory+web 的载体）。

### 2.2 Claude Code 生态（anthropics/claude-code，141.8 万星）
- 官方插件体系：目录 + `.claude-plugin/plugin.json` manifest，可打包 skills/agents/hooks/MCP servers/LSP servers，命名空间隔离，marketplace 分发。官方市场 `claude-plugins-official` 约 37 官方插件 + 15 第三方收录。
- 官方代表插件：LSP 全家桶（rust-analyzer/typescript/pyright/clangd/gopls 等 12 个）、code-review/code-simplifier/feature-dev/pr-review-toolkit、claude-security/security-guidance、claude-md-management（CLAUDE.md 治理）、skill-creator/plugin-dev/mcp-server-dev、**ralph-loop**（run-until-complete 自动循环）、mcp-tunnels（远程 MCP 隧道）。
- 内建能力面（非插件）：Bash、Read/Write、WebFetch、Grep/Glob、Edit、Task（子代理）、自动记忆（`.claude/projects`+CLAUDE.md）。
- 社区最热（Top 选）：**superpowers**（27.3 万星，agentic skills 框架：先访谈定规格→计划→子代理 TDD 驱动→双阶段审查）、**browser-use**（10.9 万星）、**claude-mem**（9.1 万星，跨会话持久记忆：自动记录→压缩→注入，支持所有 agent）、**chrome-devtools-mcp**（4.9 万星，CDP 浏览器调试/操作）、**wshobson/agents**（3.9 万星，同一套插件跨 Claude Code/Codex/Cursor/OpenCode 安装）、**playwright-mcp**（3.6 万星，微软官方浏览器 MCP）、**github-mcp-server**（3.2 万星）、**context7**（6.1 万星，按需最新第三方库文档，Docs-as-Context）、**dev-browser**（QuickJS/WASM 沙箱跑 Playwright，与 BoenMind 运行时同构）。

### 2.3 MCP 生态（modelcontextprotocol/servers，8.9 万星）
- 官方参考实现只剩 7 个维护中：filesystem / fetch / git / memory（知识图谱，SQLite）/ sequentialthinking / time / everything。其余已归档（brave-search/github/gitlab/postgres/puppeteer/redis/sqlite 等，被官方或厂商替代）——**官方全家桶很薄，真正生态在社区**。
- 社区热门（awesome-mcp-servers 9.2 万星，~3500+ 服务器）：firecrawl（搜索+爬取+结构化抽取+JS 渲染）、exa（神经语义搜索）、tavily、mem0（6.3 万星通用记忆层）、basic-memory（Markdown 文件即知识图谱，本地优先）、postgres-mcp、playwright-mcp、chrome-devtools-mcp、tui-mcp（终端 TUI 交互）、capsulerun/bash（WASM 沙箱 bash）、codemcp（极简 coding agent server）。
- 行业收敛结论：**所有主流 agent 最终统一到 "MCP 为工具总线 + 文件式配置/技能为扩展面"**，插件可移植（wshobson/agents 即证明）。

## 3. BoenMind 现有能力面（已有 vs 差距）

| 能力类别 | 已有（BoenMind） | 差距 |
|---|---|---|
| 插件运行时 | TS/QuickJS（swc 转译+hostcall 桥）、Skill 体系（SKILL.md+安装/管理/作用域）、MCP 客户端（rmcp 裁剪+双 era 协商+三来源配置+崩溃重连+反向 server）、hooks 系统（生命周期钩子+工具调用落库） | — |
| 记忆 | coding-memory 插件（按项目分桶）+ ctx-compactor 压缩插件（50% 水线注入，省 78%） | **缺通用跨会话记忆**（claude-mem 模式的自动记录→压缩→注入） |
| 搜索/Web | web_search + web_fetch（5 源真实 key、用量管理、失败惩罚、自动切换） | 免 key 兜底（DDG/SearXNG）可选；firecrawl 结构化抽取可作为 MCP 可选 |
| 角色/专家 | role 插件 + agents/*.md 专家预设（3 位预置）+ subagent 入口 + 专家提示词 | 无"两阶段最小化 preset"（首个请求只见 2 工具，成功后再放开）——与最小化运行直接同构，值得吸收为默认体验 |
| 工具面 | host 工具（listDirectory/createDirectory/openPath 等，特权表治理） | 内置 bash 已删（政策拒绝 exec）；PTY 终端缺失 |
| 浏览器 | 无（ZCode 侧有 browser-use skill 但属外部） | CDP 进程外 或 QuickJS 插件内 Playwright |
| 代码/Git | 编程应用 M2 壳（文件树/编辑器/todo/事件投影）+ git 状态 | github-mcp-server / context7 / LSP 全家桶未接 |
| 编排 | subagent 团队 + todo + 回合契约 + 事件瀑布 | 无 superpowers 式方法论 skill 包；M5 supervisor/team 进程未建 |

## 4. 候选插件清单（按能力类别，作用+来源+成本+优先级）

> 优先级 P0=最小化且零/低运行时成本；P1=进程外 MCP 可选装；P2=随 M5 或后续主线。
> "吸收方式"：A=抄架构模式自研（进官方插件或内核层）；B=接现成 MCP；C=文件式规则/skill 直接采用。

### 4.1 记忆类
| 候选 | 来源 | 作用 | 吸收方式 | 优先级 |
|---|---|---|---|---|
| claude-mem 模式（自动记录→AI 压缩→注入） | Claude Code 社区 9.1 万星 | 跨会话持久记忆，与 ctx-compactor 互补 | A（自研插件，复用已有压缩） | P1 |
| basic-memory（Markdown 文件即知识图谱） | MCP 社区 3.7 千星 | 本地优先、人机同读写、可进 git——契合可审计心智哲学 | A/B | P2 |
| mem0 | MCP 社区 6.3 万星 | 通用记忆层（语义/情节/程序记忆） | B（可选 MCP） | P3 |
| dsh-memento / dsh-hermes-memory | dsh 社区 | SQLite 有界分层记忆 / hermes MEMORY.md 移植 | 参考 | P3 |

### 4.2 浏览器类
| 候选 | 来源 | 作用 | 吸收方式 | 优先级 |
|---|---|---|---|---|
| chrome-devtools-mcp | MCP 4.9 万星 | CDP 驱动，可接真实登录态 Chrome，DOM/网络/截图 | B（进程外，内核只当客户端） | P1 |
| dev-browser 模式 | Claude Code 6.6 千星 | QuickJS WASM 沙箱跑 Playwright 脚本——与 BoenMind TS/QuickJS 运行时同构 | A（插件内，无 Rust 侧浏览器） | P2 |
| playwright-mcp | MCP 3.6 万星 | 无障碍快照交互，结构化低幻觉 | B | P2 |

### 4.3 搜索/Web 增强
| 候选 | 来源 | 作用 | 吸收方式 | 优先级 |
|---|---|---|---|---|
| 免 key 多引擎（dsh-free-search / SearXNG） | dsh 社区 | 免费无 key 7 引擎+自动故障转移，给现有 web_search 兜底 | A（provider 卡扩展） | P2 |
| firecrawl-mcp | MCP 7.3 千星 | 搜索+爬取+结构化抽取+JS 渲染+PDF | B（可选 MCP） | P3 |

### 4.4 终端/执行类
| 候选 | 来源 | 作用 | 吸收方式 | 优先级 |
|---|---|---|---|---|
| dsh 终端分层（seam+local/sandbox+PTY） | dsh 官方（最成熟类别） | bash/pwsh 抽象接缝+双实现+持久 PTY | A（架构模式） | 待拍板 |
| mcp-server-terminal / pty-mcp | MCP 社区 | 结构化终端状态树 / 交互式 PTY | B | 待拍板 |

> ⚠ 注意：内置 bash 已删是用户定调的政策（exec 拒绝）。终端类是否以"插件+审批门控"形式部分放开 = 拍板点。

### 4.5 代码/Git/上下文工程
| 候选 | 来源 | 作用 | 吸收方式 | 优先级 |
|---|---|---|---|---|
| github-mcp-server | GitHub 官方 3.2 万星 | repo/issue/PR/代码搜索全 API | B | P2（随编程主线） |
| context7 | MCP 6.1 万星 | 按需最新第三方库文档（Docs-as-Context），替代全量 RAG | B/内核只做缓存 | P2 |
| LSP 全家桶模式 | Claude Code 官方 12 个 LSP 插件 | Rust 内核只做进程外 LSP client（tower-lsp），语言支持全插件化 | A（架构模式） | P3 |
| git 工具 | MCP 官方 git server | status/diff/log/branch/worktree，libgit2 内建零额外运行时 | A（内建） | P2 |

### 4.6 编排/方法论类（与最小化运行直接相关）
| 候选 | 来源 | 作用 | 吸收方式 | 优先级 |
|---|---|---|---|---|
| 两阶段最小化 preset（dsh-anchored-standard 3.5 千星） | dsh 社区 | 首个请求只见 bash+read 2 工具，成功后再放开全套 | A（默认体验） | **P0** |
| superpowers 方法论 | Claude Code 27.3 万星 | 访谈→计划→子代理 TDD→双阶段审查的 agentic skills 框架 | C（SKILL.md 包形式） | P1 |
| 分层指令文件统一体（AGENTS.md/CLAUDE.md/rules 统一） | Codex/Cursor/Claude Code 共性 | 一套层级上下文文件，天然适配 Rust 内核 | C/A | **P0** |
| ralph-loop（run-until-complete） | Claude Code 官方 | 自愈循环：一直跑到完成 | A（参考） | P2 |

### 4.7 安全/治理（对照参考）
- Claude Code hooks 生命周期钩子（BoenMind 已有 hooks 体系，对照补缺即可）；claude-security/claude-md-management（CLAUDE.md 治理）。
- dsh 守卫式执行管线（dsh-tools，BoenMind 特权表+审批面已有同构）。

## 5. 拼接组合方案

按"最小化运行"给的默认预设与按场景加装：

```
核心 preset（默认，最小化）
  ├─ 角色/专家（已有 role + agents/*.md）＋ 两阶段最小化 preset（新，P0）
  ├─ 记忆：coding-memory（已有）＋ ctx-compactor（已有）
  ├─ 搜索：web_search/web_fetch（已有，5 源）
  ├─ 工具面：host 工具（已有）+ 审批面（已有）
  └─ 上下文工程：分层指令文件统一体（新，P0）

按场景加装（preset 组合，事件瀑布注入互不冲突）
  ├─ 编程场景：+ todo（已有）/ git / context7 / github-mcp / LSP / subagent 团队
  ├─ 研究场景：+ 浏览器（CDP）/ firecrawl / 免 key 搜索兜底
  └─ 深度记忆场景：+ claude-mem 模式通用记忆（P1）
```

拼接机制对齐 dsh 事件瀑布：记忆/人设用 pre-step 改写消息注入、工具用注册表+守卫、卸载副作用可回滚——BoenMind 已有事件瀑布（turn/start、Step 事件），接缝化改造即可承载任意组合。

## 6. 最小化运行原则（筛选规则）

1. **内核不膨胀**：凡能做插件/MCP 的，不进 Rust 内核（万物皆插件铁律）。
2. **进程外优先**：浏览器/CDP、LSP、大工具走 MCP 进程外，内核只当客户端。
3. **文件式优先**：纯 markdown/目录即插件（SKILL.md 模式）零运行时成本，先于代码插件。
4. **免 key 优先**：搜索/记忆优先免 key 方案（DDG/SearXNG、纯文本 BM25），付费增强作可插 MCP。
5. **官方全家桶抄模式不抄实现**：吸收接缝/事件瀑布/preset 组合/plugin.json 分发机制，不依赖任何 JS 包。
6. **已有不重复**：搜索 5 源、记忆分桶、压缩、角色、hooks 已覆盖，不再引入外部同类。

## 7. 建议落地批次

- **批 1（P0，纯文件式+零运行时成本，随时可做）**：分层指令文件统一体（AGENTS.md 层级）；两阶段最小化 preset 为默认体验；superpowers 式方法论 SKILL.md 包（若拍板）。
- **批 2（P1，进程外 MCP 可选装）**：浏览器 CDP（chrome-devtools-mcp 或自接 CDP）；claude-mem 模式通用记忆插件（复用已有压缩）；免 key 搜索兜底 provider 卡。
- **批 3（P2，随编程主线）**：git 内建（libgit2）、context7 客户端、github-mcp-server、LSP client 模式。
- **批 4（P2，随 M5）**：team 插件进程 + supervisor + IPC（已在 M5 主线规划）；终端类（若拍板放开）。

## 8. 拍板点

1. **浏览器自动化走哪条**：CDP 进程外（chrome-devtools-mcp，最轻、内核只当客户端）／ QuickJS 插件内 Playwright（dev-browser 模式，用自家运行时）／ 先不做？
2. **终端执行是否放开**：内置 bash 已删是既有政策（exec 拒绝）。是否以"插件 + 审批门控"形式提供受限终端（dsh 官方 sandbox 分层为参考）？
3. **通用跨会话记忆做不做**：已有 coding-memory（项目分桶）+ ctx-compactor（压缩）。claude-mem 模式的"自动记录→压缩→注入"是否立项为 P1 官方插件？
4. **首批范围**：只做批 1（文件式 P0 零成本），还是批 1 + 批 2 选一两个一起做？
5. **官方全家桶吸收深度**：只抄架构模式（接缝/事件瀑布/preset），还是也实现"模型自管理插件"（dsh-tool-cordis 式，长期项）？

---
*调研执行：2026-08-18 双代理全网调研（dsh 生态 / Claude Code+MCP 生态），星数为当日 GitHub 实测。*
