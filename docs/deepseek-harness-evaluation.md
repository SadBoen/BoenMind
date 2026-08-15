# DeepSeek Harness（dsh）全面研读与对标评估

> **2026-08-15 数字滚动注**：本文为 2026-08-14 时点数据（3.3 万★/约 300 社区插件）；8-15 实测已至 95.8K★/awesome 清单 365+ 插件/topic 2,487 仓，且"前端就是插件"经源码级复核为完整机制（本文 §4.10 未展开）——更新见 docs/REVIEW_LANDSCAPE_2026-08-15.md §五 与 docs/research/2026-08-15/plugin-landscape.md §③。

> 状态：调研完成（2026-08-14）。对标报告，不含实现。结论与拍板点见文末。
> 上游：https://github.com/deepseek-ai/deepseek-harness （MIT，v0.1.0-rc.5 开发者预览，2026-08-13 发布，几天内 3.3 万+ star、约 300 社区插件）
> 研读基线：浅克隆 `--depth 1`，77MB，7412 文件。本地副本：`D:/96_CoderWorld/deepseek-harness`

## 一、项目概况

DeepSeek 官方开源的 **agent harness（智能体装配运行时）**，README 标题即 *"Everything is a Plugin"*（一切皆插件）。定位是与 OpenAI Codex / Anthropic Claude Code 竞争的编码/办公 agent 产品，但产品哲学截然不同：**"Codex 是成品，dsh 是组装 agent 的运行时"**——它交付的不是一个 agent，而是一个可以任意组装/替换/扩展出 agent 的框架。

- **语言**：TypeScript / Node.js（pnpm monorepo，Node ^22.19，`type: module`）。根 `package.json` 一个 workspace 覆盖 `vendor/*`、`packages/*/*`、`native/*`、`apps/*`、`website`。另有 Python SDK（`deepseek-harness-sdk`，PyPI，自带运行时）和 `native/`（Linux Landlock runner）。
- **底层框架**：**Cordis**（北大 + DeepSeek 论文《A Programming Paradigm for Spatiotemporal Composability》），以 vendor 方式引入（`vendor/` 共 34 个 TS 文件、5690 行，含 cordis/cosmokit/loader/schemastery/hmr/logger 全家桶）。
- **48 个 packages**，全部 `@deepseek-ai/dsh-*` npm 包：core（agent/agent-loop/session/tools/scope/system-prompt）、llm、compaction、context、sandbox、skill、subagent、mcp、plan、workflow、goal、schedule、jobs、todo、spill、session-query、storage、credentials、fs、shell、terminal、bundle（base/web-app/headless）、preset、extensions、acp、lsp、web、client、host、hooks、api、boot 等。

## 二、架构总览：一切皆插件怎么落地

### 2.1 无特权核心

核心主张：**不存在需要打补丁的特权内核**。模型适配器、工具注册表、会话日志，乃至 agent loop 本身，都是可替换的插件；扩展 dsh 的方式是"把插件挂载到其他插件旁边"，所有注册都是**可逆副作用**（`ctx.effect()` / `ctx.on()` 安装，插件卸载时自动撤销）。

### 2.2 Cordis 五个核心概念

| 概念 | 含义 |
|---|---|
| 插件 | 实现 Service 的对象（函数或 Service 子类），生命周期由 Cordis 挂载 |
| 上下文 | 服务的容器。服务占据稳定 `ctx.<key>`（`ctx.tools`、`ctx.llm`、`ctx.sessions`），按 key 查找而非 import 具体实现 |
| inject 依赖声明 | 插件声明所需服务后等待其就绪再启动——加载顺序由依赖表达，不手动编排 |
| 类型化事件 | TS 声明合并注册事件名；四种分发模式：`emit`（观察）、`waterfall`（环绕中间件）、`parallel`（并行扇出）、`serial`（按序执行） |
| 可逆副作用 | 所有注册（提示词片段/工具 schema/适配器/提供方/监听器）在 reload/teardown 时撤销 |

**waterfall 语义**（最重要的扩展原语）：`ctx.waterfall` 是环绕中间件。监听器收 `(...args, next)`，调 `next()` 委托下游、下游返回值可被包装；不调 `next()` 直接返回 = 短路（策略监听器拥有决策权时短路是设计意图）。只有必须在普通注册前运行才用 `prepend: true`。

### 2.3 Profile / Bundle / Patch 分层（可审计的组合机制）

运行中的 dsh 是一棵插件树，由启动时按序叠加的层组成：

```
空条目列表
  ├─ profile 列出的每个组合包（按顺序）
  ├─ profile 的 cordis.patch.yml
  ├─ home 级的 cordis.patch.yml
  └─ 运行时 --patch overlay
```

- **profile**：harness home 中的具名组装（列组合包 + 装树外插件 + 保存用户 patch）。`web`、`headless` 是随发行版交付的模板。
- **组合包（bundle）**：Cordis 配置项 + 挂载代码的分发格式，`package.json` 的 `dsh.bundle` 字段指向 patch 文件。`dsh-base` 是每个 profile 的第一层（模型适配器/工具/持久化/沙箱与审批策略/设置/凭据/遥测）；`dsh-web-app` 加浏览器应用；`dsh-headless` 加一次性运行器（无服务器）。
- **patch**：按 id 定位条目、替换整个 config 或插入新条目。**每层都可被其上层 patch，全部可审计可回滚**。
- `dsh --profile web --dump-config` 打印实际配置树——打印出的任何条目都能被用户 patch 替换。

### 2.4 事件 = 扩展点（三个事件域）

| 域 | 性质 | 用途 |
|---|---|---|
| 会话事件 | append-only 日志里的持久事实，`session/event` 广播 | 必须跨重启存活的事实 |
| Agent 事件（`agent/*`） | 携带活跃 Agent 的实时事件：inbox、步骤、状态、请求、验证、续跑 | 观察/拦截进行中的工作 |
| 能力事件（`fs/*`、`tools/*`、`telemetry/*`） | 无需导入循环即可给 seam 附加策略/适配器 | 横切关注点 |

## 三、核心机制拆解（源码级）

### 3.1 Agent loop（`core/agent-loop`，`agent.ts` 496 行 + `index.ts` 713 行）

`ReactLoopAgent`：一个步骤 = 一次模型请求 + 它调用的工具；一个轮次 = 零或多个步骤（领取首条输入时打开，不再欠工作时关闭）。

**轮次流程**（架构文档原文图）：

```
turn/start
  claim next-step input plus one queued message
  assemble prompt sections + tool schemas
  -> agent/pre-step                   reject | enter(messages)
     step/start
     append entered messages as user/message
     derive model history from the log
     agent/request -> llm/stream -> assistant/chunk* -> assistant/message
     tool/call* -> tools/pre-execute -> tools/execute -> tools/post-execute -> tool/result*
     step/end
  -> agent/turn-stopping
turn/end
```

**关键设计**：

- **每次请求从日志投影**：`session.deriveMessages()` 从 append-only 日志重建模型历史——"模型可见即已记录"是运行时不变量。
- **inbox 双队列**：`next-turn`（唤醒驱动）/ `next-step`（步骤内续跑）。三种发送：`followup`（下一轮次）、`steer`（下一步）、`inject`（注入上下文**不唤醒**——文件变更通知、AGENTS.md、skill 内容、cron 通知都走这里，落到下一次获准请求中）。
- **五个瀑布式扩展点**：
  1. `agent/pre-step`：改写/拒绝模型所见。首次领取被拒或被改写为空，仍关闭一个不含步骤的持久轮次（日志记录这次尝试）。
  2. `agent/request`：改写请求配置（provider/model/采样参数）——插件可以路由、替换模型。
  3. `agent/request-error`：结构化失败（`LlmError` 保留事实，其他错误 flatten 为 `{code:'UNKNOWN'}`），插件决策 retry。
  4. `tools/pre-execute` / `tools/post-execute`：工具把关链（见 3.4）。
  5. `agent/turn-stopping`（serial）：停止轮次的钩子。
- **max-tokens sticky**：一旦某步触顶，后续正常完成的步骤不得降级轮次结局。
- **取消**：`AgentCancelCause` = user / parent / hook / disposed；abort 后唤醒消息转 `next-turn`（取消的活动不能续接）；`maintenance` 阶段（压缩等后台任务借用 loop 上下文，`runMaintenance`）与运行期互斥。
- **request/header 日志**：每次请求的规范化配置（provider/model/system/tools 快照）落日志（reason: initial/resume/change）——恢复/重放时重构请求头，`adapterDefaults` 剥离后交给插件提案。

### 3.2 会话日志（`core/session`，append-only 事件流）

**唯一事实源**：消息历史从日志派生，原始 `assistant/chunk` 逐块保留（token 级回放保真）。fork、恢复、transcript、遥测、持久化全部派生自该事件流。日志是 lossless JSON、seq 连续，持久化可原样存储。

**事件类型清单**（`SessionEventMap`，**插件可通过 TS 声明合并扩展**）：

| 事件 | 内容 |
|---|---|
| `turn/start` / `turn/end` | 轮次开闭，`TurnEndReason`：completed/aborted/blocked/error/max-tokens/interrupted（合并可扩展 sum type） |
| `step/start` / `step/end` | 步骤开闭 |
| `user/message` | 人类消息 / inject 合成上下文 / goal 续跑，`source` 区分三者 |
| `assistant/chunk` | 原始流式块（回放保真） |
| `assistant/message` | 组装完成的助手消息 + usage（同一条记录，不分离） |
| `tool/call` | 模型请求的工具调用（arguments 原样 JSON 字符串，**执行前记录**） |
| `tool/result` | 模型可见结果 + 内部失败标识 + 工具私有 `meta`（JSON 校验） |
| `todo/write` | 待办整表快照（last-write-wins） |
| `request/header` / `request/context` | 请求配置快照 / 路由元数据（含 contextWindow） |
| `session/end-seed` | 构造种子边界（fork/resume 的持久投影） |

**健壮性设计**：

- `ignorable` 守卫：新事件类型可标记"可忽略"，未认识的**必需**事件会让旧运行时**拒绝重建**而不是静默跳过——防旧版本读坏新日志（漏标 = 过度拒绝，无害；漏记 = 静默恢复残缺会话，有害）。
- 格式版本 `SESSION_FORMAT_VERSION = 0`，版本升级链机制（migrate-on-continue）；**写者决定是否 bump**（"能解析 ≠ 语义正确"）。
- **压缩 = 日志第一公民**：`SurfaceOp` = `'append'` | `{op:'replace', start, end}`——压缩时用摘要节点**替换** surface 区间，`sourceEventSeqs` 携带被遮蔽节点的引用链，**压缩可审计**。任何表面替换的生产者（压缩）都走此机制。
- 新模型可见输入必须新增事件类型（运行时不变量强制）——扩展接口即扩展协议。

### 3.3 Scope 与 preset 隔离（`core/scope` + `preset/agent-presets`）

- `createScope(ctx, key)`：铸造一个带标签的 Cordis context（`ctx.plugin(scope)` fiber），每个 agent 拥有自己的 `agent.ctx`。**注册视图向下继承**（child scope 见祖先 layers），**事件准入向上传播**（祖先 scope 的监听器收到所有后代 agent 的事件——"一个全局组合观察每个子 agent"），链式带环检测。
- **preset**：一次组装 = 一组工具/提示词/服务，发布的服务在 **`isolate` realm**（realm-private symbol）之后——两个会话各自挂载副本，互不可见。`agentPreset` 持久化在 session header（恢复时决定工具集，可续跑不换组合）。
- 子代理：`delegationDepth` 持久化在 header（重启后递归预算不丢）；fork 带 `seedLength` 边界（区分父历史与子工作）。

### 3.4 工具流水线（把关链，`tool-execution-pipeline`）

```
model 发 tool-call → 落 tool/call 日志（执行前！）→ UI pending card
→ tools/pre-execute waterfall（钩子/权限/沙箱）
→ 单调守卫（deny or abstain，身份不可改写）
→ ctx.approval 一次性询问（拒绝/取消/不可用 = deny，工具体被跳过）
→ tools/execute waterfall（timeout/retry/metrics 环绕分发）
→ 工具体执行 → fs/write-intent、fs/edit-intent 守卫（仅 tool-fs 变更）
→ tools/post-execute waterfall（accept/block/replace/add context）
→ finalizeContent（同步仅内容不变式）→ tools/result 冻结通知
→ additionalContexts FIFO → 注入 user/message（在记录的 tool/result 之后）
```

- 三个 waterfall 都能**改写一次调用**；结果在注册表外层做无损快照（快照失败先规范化再进 finalize）。
- 工具挂 owned 会话事件（`todo/write`、`fs/observed`、`hook/invoked`…）由工具自己写。
- 钩子跨工具族复用，工具不耦合策略服务——策略全在事件层。

### 3.5 系统提示词组装（`core/system-prompt`）

- `PromptSection`：命名片段注册，`order` 升序拼接（约定：-100 身份 / 0 人格 / 100-199 工具指南），文本可为静态或按组装上下文动态解析；`complete: true` 声明唯一完整段（多于一个生效 = 组装失败）。
- `PromptContext`：动态上下文（当前文件/目录/时间…）物化为**持久的 user-role 快照**，仅在快照变化或被压缩移除时重新记录。
- 工具 schema 组装：`ToolProviderResult.schemas` + `knownNames`（限制前名称全集——区分"配置名拼写错误"与"已知工具在本作用域被有意隐藏"）。

### 3.6 自我扩展（`extensions/`）—— 一切皆插件的极致

`extensions/ — the agent modifies its own runtime`：**模型侧工具可以检查已加载插件和服务 API、定义并运行模型编写的动态包、再撤回它们**（`tool-cordis` 注册到 ctx.tools，`cordis-host-runner` 用 `node:vm` 沙箱跑 host 半部，另有受限仓库 Plugin 运行时）。即 agent 自己可以扩展自己的运行时，且动态包生命周期受控。

### 3.7 四个内置预设

| 预设 | 内容 |
|---|---|
| standard | 完整编码 agent（文件编辑/shell/搜索/skills/规划/subagents/workflows） |
| PTC（Code Mode） | 用 TS SDK，模型写程序把多步工具调用组合成一次执行（`run_code` 保留传输 + 序列化子调用，子调用带父 token、拒绝呈绑定驳回） |
| minimal | 仅 bash + str_replace_editor 两工具（用于 V4-Flash Code Agent 基准） |
| creator | 搭自定义预设，带运行时检查与插件实验 |

## 四、外围子系统

### 4.1 压缩（`compaction/`，四层：抽象引擎 → compaction-basic → tool-result-pruner → command-compact）

- **双触发**：① pressure 触发——挂 `agent/pre-step`，每步前 `ctx.tokenMeter.measure(session)`，超 `contextWindow × 0.8`（默认水线）即压；② context-overflow 硬触发——挂 `agent/request-error`，错误码 `CONTEXT_WINDOW_EXCEEDED` 时强制压缩并返回 `{kind:'retry'}`，`maxOverflowRetries` 默认 1。
- **策略 = 摘要 + 保留尾**：头部区间摘要，**尾部保留 verbatim 16%**（`retainRatio=0.16` 或绝对 `retainTokens`）；选中区间**不允许切断 tool-call/result 配对**；支持 `modelPolicies` 按模型精确覆盖（与 BoenMind"按模型 50% 水线"同模式，但 dsh 是 80% 默认 + 按 contextWindow 动态缩放 + overflow 硬触发）。
- **摘要方式**：one-shot `ctx.llm.stream()`，**复用会话自身 system prompt + tools + 被压区间消息做前缀**（吃 provider 的 KV prefix cache），末尾追加八节固定模板（Primary Request / Key Technical Concepts / Files and Code / Errors and Fixes / Pending Jobs / Current Work / Next Step / Critical Context）；要求摘要帧后必须更小，否则报错。
- **与日志的关系——压缩是"可重放事务"**：append `compaction/start` → `compaction/summary`（含 shadowedRange/shadowedSeqs/shadowedTokenCount）→ `user/message`（surfaceOp `{op:'replace',start,end}` 落摘要 checkpoint）→ `compaction/end`；unmatched `compaction/start` 即压缩锁。**压缩全程在日志里可审计可重放**。
- **无模型预裁剪**：pruner 先对超长 tool/result 做 head/middle/tail 字符裁剪（Unicode 安全），再重新测量——"先 pruner 后摘要"两阶段降级。

### 4.2 沙箱（`sandbox/`）

- **argv 包装式，不是进程级 daemon**：`SandboxProvider.confine(argv, policy) → ConfinedArgv`（"runner + profile 参数 + `--` + 原 argv"），策略**按调用携带**（per-call），三档：`read-only | workspace-write | danger-full-access`；fail-closed（无后端抛 `SANDBOX_UNAVAILABLE`，绝不裸跑）。
- **平台链**：Linux bwrap（优先）→ Landlock（native addon，`--ro-bind / /` + workspace 可写）；macOS Seatbelt（SBPL profile）；**Windows ACL restricted-token runner**（koffi FFI 调 CreateProcessAsUserW，WRITE_RESTRICTED token + workspace 专属 capability SID 常驻 + 每会话随机私有 temp 目录 SID，dispose 时撤销）。每候选先 spawn `true` 功能探测，失败 fail-closed。
- **升级审批（最值得抄）**：工具 schema 带 `sandbox_permissions`（只允许指向**严格更宽**的目标档）+ `justification`（必填一句话理由）；执行时校验 `WIDER_MODES` 阶梯，走 `ctx.approval.request(...)`（waterfall，outcome `allowed-once|rejected|cancelled|unavailable`），**批准只作用于这一次调用**；拒绝给模型 `[sandbox: file access denied under X mode]` + 升级提示。

### 4.3 Skill（`skill/`）

- 格式：目录下 `SKILL.md`（或裸 .md）+ YAML frontmatter（必填 `name` kebab-case 校验、`description`；可选 `when-to-use`、`disable-model-invocation`、`user-invocable`）。
- 发现：多根扫描（项目 `.dsh/`/`.agents/`、用户 home、bundle 内置）+ **rank 裁决同名冲突**（内置 600，runtime 250）+ fs watch 热更新；注册表 `ctx.skills` 按 scope 分层。
- **注入方式——关键差异：不注入系统提示词**。两条模型可见路径：① catalog 增量注入——每 step 发布 `<available_skills>` 用户消息（名+描述，digest 去重）；② 按需加载——`skill` 工具返回 `<skill_content><skill_resources>…</skill_resources><skill_instructions>…</skill_instructions></skill_content>`；另有 `/name` 手势显式调用（渲染后 body 作为 injected instructions）。

### 4.4 子代理（`subagent/`）

- **`ctx.subagents` 是多 provider 命名注册表**（非单服务）：`spawn`（孩子零父上下文）/ `fork`（以父 session 最后一个 `turn/end` 为界的完整前缀作种子，继承上下文）/ `continuable`（持久化可续聊，子代理用自己 inbox 逐轮调度）/ 委派外部：`claude-code`（官方 SDK）、`codex`（真实 codex app-server --stdio）、`acp`（Agent Client Protocol 子进程）、`dsh-sdk`。
- 孩子 = **同一个 agent loop 工厂创建的独立 Agent**（独立 session、独立 system prompt、共享 cordis context）；`maxDepth` 默认 3、`outputSchema` 结构化输出、`toolFilter`/`persona` 能力位；主循环的 `agent/*` 事件机制同样作用于子代理。

### 4.5 会话级组合（`preset/agent-presets`）

- **agent preset ≠ boot profile**：preset 是"某个 agent 会话看到哪些插件"的组合单位（`agent.cordis.yml`）；profile 是"宿主装哪些插件"（profile/bundle/patch 层）。两个正交轴。
- **standing mount + scope 父链**：preset 只挂载一次（常驻 cordis 子树，standing scope key），每个 agent 把自己的 scope key **父级挂到 preset 上**从而"看见"其注册——**同一份插件实例被多会话共享，服务数据按会话隔离**（isolate realm，realm-private symbol；泄漏到 root realm 的服务名会被 `leakedServices()` 检测并拒绝挂载）。挂载发生在 `agent/created` 前，组合失败整个创建回滚。

### 4.6 组合包与宿主（`bundle/` + `boot/app-boot`）

- `dsh-base` = 纯声明包：`cordis.patch.yml` 451 行补丁，一条 `insert` 把全部基础插件（llm/session/agent-loop/compaction/sandbox/skill/subagent/全套工具/persistence-jsonl/session-query-sqlite/telemetry/user-approval/system-prompt/web…）插进空 profile 根；`package.json` 的 `dsh.bundle.patch` 字段指向 patch 文件；profile 用 `dsh.profile.bundles` 列有序 bundle 列表。
- 工具名/标识解析：安装目录优先、profile 目录次之；`$DSH_HOME/profiles/node_modules` 用 BFS 依赖闭包建 symlink 平面回退目录。

### 4.7 MCP client（`mcp/mcp-client`）

- 官方 `@modelcontextprotocol/sdk` v1.12；stdio 子进程 + streamable-http/SSE 双 transport；自动重连（退避）+ per-call 超时 60s。
- 工具映射：`tools/list` 全量拉取 → 以 `mcp__<serverName>__<rawName>` 确定性公开名注册（≤64 字符 + 正则契约，规范化改名时追加 12 位 SHA-256 防碰撞）；执行用原始名；**两阶段 sync**（先 fetch 全量再 swap，失败保留旧代）+ 列表变化 re-sync。

### 4.8 jobs / goal / schedule / plan / workflow

- **jobs**：后台任务注册表（run_in_background 生命周期，owner 清理，settlement 先到先得）。
- **goal**：同会话目标域，**事件溯源 + CAS 突变**，goal 轮次驱动把续跑注入 inbox（等待 quiescence 后注入下一轮）。
- **schedule**：持久化定时器（at/every≥300s/after），事件溯源记录 + 每 agent 投影最小堆，到期注入消息；重放从日志 fold 出计划。
- **plan**：plan mode 协作状态（per-agent），`plan/mode` 事件 fold，激活时附加 guidance 段，`exit_plan_mode` 交用户审查。
- **workflow**：编排脚本引擎——plain JS body（顶层 await + `return <json>`），worker 线程 `node:vm` 沙箱执行，`agent()` 调用桥回宿主；观察者只收生命周期事件无运行控制权。
- **共同纪律：一切状态事件溯源化**（goal/schedule/plan 都从 session log fold，可重放恢复）。

### 4.9 记忆——**没有独立记忆子系统**

全仓无 memory 包、无 remember 工具。承担"记忆"角色的是三个机制：
1. **compaction checkpoint**（压缩摘要即跨轮持久上下文，八节模板里明确要求保留 Pending Jobs/Key Technical Concepts——隐式记忆载体）；
2. **session 事件日志**（append-only 全量 + JSONL 持久化 + **SQLite FTS5 会话检索**）；
3. **spill**（超大工具输出落盘、模型只拿 locator）。
dsh 的长期记忆哲学是 **"文件系统即记忆"**（tool-catalog 里 ralph 工具描述原话：shared workspace 就是长期记忆，只有有界结构化报告跨轮传递）。

### 4.10 应用壳（`apps/`）

- `apps/cli`：**CLI 即宿主进程**（profile boot、`dsh plugin`、`dsh web` 起 host），模型交互走浏览器 UI，不是 TUI。
- `apps/web`：**Vite 6 + React 18** SPA，dist 由宿主 webserver 托管。
- **没有 Electron、没有 Tauri**（全仓 grep 无结果）——无桌面壳，桌面体验 = 浏览器访问本地 host；`native/` 只是 Landlock addon 等原生模块。

## 五、工程实践（值得抄的）

- **测试文化**：每包带 `invariant.spec.ts`（运行时不变量测试）+ property-based（`properties.spec.ts`）+ 契约回归（`contract-regressions.spec.ts`）+ 快照测试（`test:snapshot`）+ e2e + web stress/perf。质量门 `check:ci` 全套 gates。
- **文档文化**：每个子系统一篇 `subsystems/*.md`（双语 i18n 配对，`verify-translation-pairing` 校验）；生成目录（config-catalog/tool-catalog/event-producer-consumer）；架构文档开宗明义"改动 packages/ 前先读本文"。
- **扩展实操手册**：`cookbook/extension-cookbook.md` 把"加一个工具/LLM 适配器/Chat 节点"写成一步步指南；`docs/` 里"新行为归属位置"一张表定完（新增能力 = 注册 ctx.llm / ctx.tools / ctx.shell…）。

## 六、对照 BoenMind

### 6.1 架构对照

| 维度 | dsh | BoenMind 现状 |
|---|---|---|
| 宿主语言 | TS/Node（插件同语言） | Rust 宿主 + TS/QuickJS 插件层（swc 转译） |
| 核心框架 | Cordis（vendor 5690 行） | vendored pi_agent_rust（**41.4 万行**/147 个 .rs） |
| agent loop | 插件，可替换（~1200 行） | 上游独占（vendor agent.rs 12.9k 行） |
| 会话日志 | append-only 事件流（回放/fork/压缩审计） | turso 表（sessions/messages/tool_calls）+ pi JSONL 旁路（no_session） |
| 压缩 | replace 表面操作 + sourceEventSeqs 引用链 | 按模型 50% 水线注入（bm-core 配置层 + vendor 引擎 3.8k 行，P4 补丁透传） |
| 工具 | pre/guards/approval/execute/post 把关链 | pi 内置工具 + PermissionBridge 弹窗询问（P5 补丁） |
| 插件 | Cordis 插件 = npm 包（挂载即生效） | QuickJS 沙箱 + 自研管理面（npm/git 安装复用上游包管理器） |
| skill | 注册片段（order 排序、complete 段） | 自研注入（XML 块 + read 工具读 SKILL.md） |
| 权限 | ctx.approval 一次性询问 + 单调守卫 + 沙箱升级阶梯（justification 必填、只宽不窄） | 权限三档（含 YOLO）+ 弹窗（fail-closed，extension-permissions.json 权威） |
| 沙箱 | argv 包装 + bwrap/Landlock/Seatbelt/Windows ACL 受限令牌，per-call 策略，fail-closed 探测 | 无 OS 级隔离（exec 政策拒绝） |
| skill | SKILL.md + frontmatter，**catalog 增量注入 + 按需加载（不注入 system prompt）** | 自研注入（XML 块 + read 工具读 SKILL.md） |
| 子代理 | 命名 provider 注册表（spawn/fork/continuable/委派 Claude Code/Codex/ACP） | P9 结构化返回（vendor 子进程 spawn 本进程） |
| 记忆 | **无独立子系统**（压缩 checkpoint + FTS5 检索 + 文件系统即记忆） | 记忆功能已开启 + ctx-compactor 修剪 |
| 存储 | 会话日志即存储（JSONL + FTS5 检索） | turso（已自持，不依赖 vendor） |
| 桌面端 | **无桌面壳**（CLI 宿主 + Vite/React SPA，浏览器访问本地 host） | Tauri 壳 + 热升级 + 插件预装 |

### 6.2 BoenMind 对 vendor 的依赖面（实测数据）

- 编译期仅 **7 个入口**：`pi::sdk`（SessionOptions/AgentSessionHandle/AgentEvent）、`pi::model`、`pi::compaction::ResolvedCompactionSettings`、`pi::extension_dispatcher::ExtensionUiHandler`、`pi::extensions`、`pi::package_manager`、`pi::error`。`bm-core/agent.rs`（291 行）是唯一会话创建漏斗。
- 间接耦合：环境变量（PI_CODING_AGENT_DIR/PI_PERF_TELEMETRY/PI_HTTP_ALLOW_LOOPBACK/PI_EXTENSION_ALLOW_DANGEROUS/PI_SUBAGENT_PROVIDER_ID）、文件协议（~/.boenmind/pi/models.json、skills/ 目录、extension-permissions.json）。
- **BoenMind 已自持**：会话存储（turso）、skill 注入（文本协议）、压缩策略（配置层）、HTTP/SSE/任务/权限桥、插件管理面、热升级、桌面壳。
- **vendor 独占**：agent 循环、LLM 流式 + 14 家 provider 适配（auth.rs 12.5k）、QuickJS 插件运行时（extensions_js.rs 33.9k + extension_dispatcher.rs 14.4k）、内置工具集（tools.rs 17.1k）、压缩执行引擎（3.8k）、subagent 执行、包管理器（8.4k）。
- **补丁台账**：P1-P10（pi）+ A1/A2（asupersync），其中 P4/P5/P9/P10 是**功能负载**（压缩透传/权限桥/结构化返回/bundle 探测），A1/A2 是 Windows 直连 bug（10057，上游锁定 asupersync 0.3.9 必中）。自研核心则全部消失。

### 6.3 关键差异结论

- dsh 把 BoenMind 想要的**每一件事都做成了第一公民**：loop 可替换、日志可回放/可 fork、压缩可审计、preset 隔离、子代理 provider 化、模型可自扩展运行时。这正是"一切皆插件"的完整形态。
- BoenMind 的不可替代资产：**Rust 单二进制 + QuickJS 真沙箱**（比 node:vm 强）、turso 存储、已实测的压缩方案、权限询问桥、热升级管线、桌面壳——这些与 dsh 思路不冲突，是自研核心的既有底座。
- BoenMind 的负债：补丁台账慢性增长、上游 bug 自担（10057）、双代码基生态混乱（TS 版 earendil-works/pi vs Rust 版 Dicklesworthstone）、**架构天花板——pi 是"成品引擎"不是"可组合运行时"，"一切皆插件"在 vendor 模式下永远只能借壳**。

## 七、"自研核心"决策分析

### 7.1 为什么这个念头是成立的

1. **用户的"一切皆插件"想法与 dsh 同构**，且 dsh 已验证其工程可行性（发布几天 300+ 插件生态）。
2. vendor 模式已触顶：loop 在上游手里，P4/P5/P9/P10 全是"加功能只能打补丁"的证据；10057 证明上游底层 bug 要 BoenMind 自己修（补丁打在 asupersync 上）。
3. 自研的分界线已经很清楚：BoenMind 已自持存储/技能/压缩策略/权限桥/HTTP——**差的只是 loop + 事件日志 + 服务注册 + LLM client + 工具注册 + 插件桥**，且这些在 dsh 里有成熟参照（核心包合计约 1.5 万行 TS）。

### 7.2 三条路线

| 路线 | 内容 | 优点 | 代价/风险 |
|---|---|---|---|
| A. 维持 vendor | 继续打补丁，接受天花板 | 零投入、上游迭代白嫖 | 补丁台账增长、上游 bug 自担、一切皆插件永远借壳 |
| B. 全自研核心 | Rust 版"一切皆插件"：服务注册 + 类型化事件 + loop + append-only 日志 + 工具把关链 + LLM client；QuickJS 桥决策见下 | 想法成真、补丁清零、上游免疫、可自主演进（回放/fork/preset 隔离/多 agent 原生） | 数月工程 + 回归 + 生态兼容（pi.dev 插件）风险 |
| C. 渐进替换（strangler） | 先自研会话日志层（append-only 事件流落 turso，双写过渡）→ 自研 loop（trait 抽象 pi 调用面）→ 工具注册 → 最后插件桥 | 每步独立发布、风险可控、随时可停 | 过渡期双轨维护 |

**推荐倾向：C（渐进），但目标形态是 B。** 第一步"会话日志层"现在就能做、与 vendor 无冲突（turso 已有）、收益立现（回放/fork/压缩审计），是零风险的探路石。

### 7.3 工作量估算（参照 dsh 源码规模）

| 模块 | dsh 参照 | Rust 估算 | 说明 |
|---|---|---|---|
| 服务注册 + 类型化事件 + 可逆副作用（Rust 版 Cordis） | 5690 行 TS | 2-3k 行 | Rust 无声明合并，事件类型用 enum + trait；工作量主要在生态习惯 |
| agent loop | ~1200 行 TS | 2k 行 | turn/step/inbox/abort 语义已吃透 |
| append-only 会话日志 | ~2500 行 TS | 2-3k 行 | 事件枚举 + seq + surface/replace + 版本迁移 |
| 工具注册/把关链 | ~800 行（tools 核心）+ 流水线 | 1.5k 行 | pre/execute/post 三事件 + 审批 |
| LLM client（OpenAI 兼容 + streaming） | llm 包 ~900 行 | 3-5k 行 | 复用 bm-core providers 已有配置/模型注册表；14 家方言适配按需 |
| **QuickJS 插件桥（最大不确定项）** | extensions_js.rs 33.9k + dispatcher 14.4k | 方案 a: 0；方案 b: 5-10k | **方案 a**：vendor 这两文件作"库"保留（QuickJS 运行时 + ExtensionBody 协议），自研 loop 调用之——插件生态（pi.dev 200+ 插件）兼容；**方案 b**：rquickjs 自研最小桥（工具注册 + hostcall），彻底独立但生态不兼容 |
| 内置工具集 | tools.rs 17.1k | 可移植自研插件 | BoenMind 已有 web_search/web_fetch/ctx-compactor 等插件资产 |
| 合计（核心，不含插件桥重写） | | **10-15k 行** | |

### 7.4 风险与缓解

- **QuickJS 插件生态兼容**（最大风险）：方案 a（vendor 作库）直接消解。
- **回归**：现有 55+ 测试 + 浏览器实测 + A-E 回归清单可复用；压缩 A/B 对比方法论可复用。
- **pi.dev 商店意向**（此前拍板"对接 pi.dev"）：方案 a 下不受影响。
- **时间窗**：v0.1.x 发布节奏中，建议错开发布冲刺；会话日志层不占发布关键路径。

## 八、拍板点（待用户决策）

1. **自研 vs 维持 vendor**：是否立项？触发条件（如补丁数 > N、或下一个上游破坏性变更）还是立即启动？
2. **QuickJS 桥去留**：方案 a（vendor 作库，保生态）vs 方案 b（rquickjs 自研最小桥，彻底独立）vs 暂不决策（渐进路线先不做桥）？
3. **会话日志层**：是否先行升级为 append-only 事件流（turso 表结构调整，双写过渡）——这是"自研核心"的零风险第一步，也可独立受益（回放/fork/压缩审计/搜索投影缓存）？
4. **压缩升级**：replace 语义 + sourceEventSeqs 引用链（压缩可审计）是否吸收进现有压缩方案？
5. **scope/preset 隔离**：多 agent 原生支持（专家团队阶段化推进）是否按 dsh 模式设计（agent 级 ctx + isolate realm）？
6. **extensions 自修改运行时**（模型定义/加载/撤回插件）：做不做？安全边界（node:vm 类比 QuickJS）怎么定？
7. **OS 级沙箱**：Landlock/Seatbelt/ACL 方案在 BoenMind（Windows 优先）的等价物（Job Object + 受限令牌 + 升级审批阶梯 justification）是否立项？
8. **PTC Code Mode**：模型写 TS 程序组合多步调用——与现有 subagent 结构化的关系？
9. **skill 形态**：是否从"全量注入"迁移到 dsh 的"catalog 增量 + 按需加载"（省 token、避免干扰）？
10. **记忆空白区**：dsh 也无记忆子系统——BoenMind 已开启的记忆功能 + ctx-compactor 其实领先；是否把压缩 checkpoint 升级为"可审计 replace 事务"（同拍板点 4）？

## 九、一句话结论

dsh 用 TS/Cordis 把"一切皆插件"做成了**无特权核心 + append-only 事件日志 + 分层可审计 patch** 的完整工程形态，恰好验证了用户对 BoenMind 的原始构想；BoenMind 的 Rust/QuickJS 底座比 dsh 更硬（真沙箱、单二进制、已自持存储/压缩/权限/技能），差的只是被 vendor 独占的 loop 与插件桥——**自研核心不是从零开始，而是把 BoenMind 已经自持的 11.4k 行领域逻辑从"寄居"变成"主权"，建议按渐进路线（先会话日志层）启动，QuickJS 桥先按方案 a 保留生态。**
