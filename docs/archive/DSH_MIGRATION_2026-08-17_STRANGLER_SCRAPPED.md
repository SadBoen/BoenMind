# BoenMind × DSH 迁移计划（2026-08-17）

> 状态：**待拍板**（§八 拍板点 1–8）
> 前置：2026-08-13/15/17 三轮"换不换 dsh 内核"讨论（架构文档 §15.4 记录了结论轨迹）。三轮以"重开判据未满足"暂缓；**2026-08-17 用户拍板：以 dsh 为核心运行时**。本计划即其执行书——不再重议"是否迁"，只议"顺序与门禁"。
> 调研基线：dsh 官方仓库 `deepseek-ai/deepseek-harness`（2026-08-13 开源，npm 0.1.0-rc.6，MIT，TS/Node）+ 本机浅克隆 `D:/96_CoderWorld/deepseek-harness`（2026-08-15 基线，47f9438）+ 08-17 全网复核（官方 npm 44 包在选、社区插件 9 仓库已验证、docs 站 `deepseek-harness.github.io/deepseek-harness`）。
> 关联文档：`everything-is-plugin-architecture.md`（架构宪法）、`frontend/public/docs/expert-team.md`（专家团队唯一描述载体）、`backend/vendor/UPSTREAM_TRACKING.md`（上游台账，本计划续增 dsh 区）。

---

## 〇、一句话结论

BoenMind 的业务能力与差异化——**沙箱权限、APP 边界、管家派专家、插件 UI 隔离、专家团队、皮肤系统、工具调用可视化、记忆/压缩策略**——全部以 **dsh 插件形态**保留；Rust 内核（loop/session/tools/storage/mcp）由 **dsh 原生插件缝**替换；`bm-server` 以 strangler 方式与 dsh 并行共存，逐阶段通过门禁后退役。**迁移是"换实现、保语义"，我们的原创点一个不丢。**

---

## 一、必须面对的事实（决定本计划写法）

1. **dsh 极年轻**：2026-08-13 开源，全部版本为 rc（最新 0.1.0-rc.6），官方明示"会有破坏性变更"，`SESSION_FORMAT_VERSION=0`。→ 一切**锁版本**（本次选型即锁定 0.1.0-rc.6 快照），迁移内置回退。
2. **dsh 是 TS/Node**（Cordis 元框架），不是 Rust。→ 这是**技术栈迁移**，不是运行时对换；Rust 资产处置见 §八·7。
3. **dsh 全插件化**：loop、session、tools、compaction、subagent、storage、UI 全是可替换插件缝。→ 我们当年造轮子的领域，绝大多数 dsh 已有官方缝（映射见 §二）。
4. **dsh 没有四样东西**（2026-08-15 源码级核实，REVIEW_LANDSCAPE §五）：① 应用级权限/沙箱（dsh 插件=宿主进程内全权 npm 包）；② "APP"第一公民 + app 间事件血缘；③ 管家派专家/寄生关系；④ 插件 UI 隔离加载。→ **这四样是 BoenMind 的原创点，迁移后以插件保留**（§六），这正是本次迁移的工程核心。
5. **dsh 前端 = React 壳 + ui-slots**（SlotMap 声明合并 + register）+ 三栏 AppFrame + `--dsw-*` 主题令牌；客户端插件**可以带 React**（官方 UI 本身就是 React，壳提供 React 实例）。→ 我们现有 React 组件可改造为 ui-slot 插件，**不用换前端语言**。
6. **浏览器自动化是 dsh 生态缺口**（只有 web_fetch/web_search，无 Playwright 类官方/社区插件）。→ 我们的浏览器能力（UPSTREAM_TRACKING T4 排期）需**自研插件**。
7. dsh 144k 星但仅 4 天 → 星数参考价值低，只信源码与锁版本。

---

## 二、现状盘点 → dsh 映射总表

### 2.1 后台

| BoenMind 资产 | 现状（实现） | dsh 对应物 | 处置 |
|---|---|---|---|
| `bm-server`（axum REST/SSE/静态） | 网关+业务 API+鉴权 | dsh web-server + 我们的 API 兼容插件 | Phase 1 并行代理 → Phase 6 退役（旧会话只读） |
| `bm-loop` / `bm-kernel`（回合循环/事件） | 自研内核 | `dsh-agent-loop` + `dsh-session` + `agent/*` 事件 | Phase 2 替换，bm 侧冻结 |
| `bm-storage-turso`（sessions/messages/tool_calls + 事件日志） | SQLite | `dsh-session-persistence-sqlite`（SessionEvent 一行一条，node:sqlite） | Phase 1 数据桥 + 迁移工具（§八·1） |
| `bm-mcp`（rmcp 裁剪，dual-era 协商） | Rust MCP client/server | `dsh-mcp-client`（stdio + streamable-http，工具名 `mcp__server__tool`） | Phase 3 替换 |
| `bm-compactor` + `ctx-compactor` 插件 | 修剪/秘密扫描/找回/索引 | `dsh-compaction` 缝（触发 `pressure`/`context-overflow`）+ `compaction-basic` + 我们的策略插件 | Phase 3 迁移（策略逻辑搬插件） |
| `bm-memory` / 记忆桶 / 项目隔离 | 桶+分层 | `dsh-storage-domain` 表单（`ctx.storage` 可注册自定义后端） | Phase 3 自研记忆插件 |
| 插件运行时（TS+**QuickJS 沙箱**） | 真沙箱硬优势 | Node 宿主进程插件（**无沙箱**） | §八·4 两档信任方案 |
| `web-search` 插件（多源+账本+失败惩罚） | 纯 TS 插件 | `dsh-tool-web`（web_fetch/web_search）+ 我们的多源策略插件 | Phase 3 迁移 |
| `pdf-omni`（TS 薄壳+Rust 核心） | 双层架构 | Node 实现或保留 Rust 子进程端点 | §八·7 |
| `role` / `coding-memory` / `refine-suggest` | 自研插件 | 各对应 dsh 插件（agent-presets / storage-domain / 审批事件） | Phase 3–4 |
| 13 服务面 / 12 挂点（EXTENSION_POINTS_REGISTRY） | 自研扩展点 | dsh 事件/服务缝（`tools/*`、`agent/*`、`turn/*`、`step/*`…） | 映射表另列（迁移期逐面映射） |
| 权限三档 + `ask_user`（PermissionBridge） | 宿主级 | dsh 有 approvals 事件（会话内审批） | Phase 3 自研权限插件 |

### 2.2 前端（单元化映射，详见 §五）

| 我们的单元（现状组件） | dsh 槽位 | 插件来源 |
|---|---|---|
| 聊天单元（ChatPane/SessionList/ChatInput/MessageItem/TokenRing/ToolCallBlock/ThinkBlock/ScrollIndicators） | `conversation` 槽 | 官方 `dsh-client-ui-conversation` 参照 + 我们改造 |
| 目录树单元（FilePanel/FilePreview/ProjectSwitcher） | `sidebar` 槽 | 社区 `dsh-explorer`（git-first 文件树）参照 |
| 分支图单元（GitGraph） | `sidebar`/details 槽 | 社区 `dsh-git-graph` |
| 终端单元（xterm） | details/sidebar 槽 | 官方 `dsh-terminal` + 社区 `giiiiiithub/terminal` |
| 任务清单（TodoPanel） | `details`/workspace 槽 | 官方 `dsh-client-ui-plan`/`ui-workspace` |
| 设置中心（含作用域/`[apps]` 段） | settings 槽 | `dsh-client-ui-settings*` + `dsh-scope` 缝 |
| 状态栏（StatusBarActions） | layout 槽 | `dsh-client-ui-layout` |
| 皮肤/特效 | 主题层（`--dsw-*` 令牌） | 皮肤插件（§六·3） |
| 权限弹窗（PermissionDialog/AskDialog） | 自研 | 权限插件（§六·1） |
| 布局（dockview 8.1 workbench 模型） | AppFrame 三栏 | §八·3 拍板 |

### 2.3 功能亮点（详见 §六）

专家团队 / 管家 Steward / 皮肤特效 / 工具调用可视化 / 记忆分层 / 压缩策略 / 可审计心智 / 便携包热升级——全部有 dsh 对应缝或自研插件路径。

---

## 三、后台迁移步骤（一步一步，带门禁）

> 原则：strangler——dsh 侧身并行，逐阶段接管，**任何阶段门禁不过即回退**，`bm-server` 全程在线，数据双份不丢。立场：这是顺序问题，不是是否迁移问题。

### Phase 0 准备与概念验证（1 轮，可立即开工）

- 插件快照下载（§四，**本轮已执行**：官方 npm 44 包 + 社区 9 仓库 → `third-party/dsh-plugins/`）。
- dsh 沙箱跑通：`dsh headless` 完成一次完整回合（用户消息 → 工具调用 → 回复）；`dsh web`（127.0.0.1:3080）GUI 启动。
- 概念验证（PoC）：我们前端经 `bm-server` 的 `/api/dsh` 代理接一个真实 dsh 会话，验证 API 契约/鉴权（BOENMIND_TOKEN 复用）/流式/工具可视化。
- **门禁 0**：hello-world 全链路 + 鉴权复用成功。产出：dsh 与本机现有测试集的对照基线。

### Phase 1 并行共存（strangler 起点）

- `bm-server` 增 `/api/dsh` 反向代理 + `session.app` 路由开关（新会话默认走 dsh，可一键回退 bm）。
- 数据：**dsh 独立 sqlite**（不自研适配器直写 turso——两套 schema 同步是坑，见 §八·1）；另写一次性迁移工具（messages/tool_calls → SessionEvent 序列，事件日志语义与 dsh 同构，迁移可行）。
- 前端 SessionProvider 切换（app 级）。
- **门禁 1**：dsh 会话与 bm 会话功能对等——现有 5 集成套件（host/load/execute/events/session）各出 dsh 版对照，全绿。

### Phase 2 核心面迁移

- 工具执行/回合循环/事件/压缩/subagent 全部切 dsh 原生；`bm-loop`/`bm-kernel`/`bm-mcp` **冻结**（只修 bug 不增功能）。
- 权限门/CSRF/workspace 白名单等宿主安全层在 dsh 侧重建（权限插件先于工具面切换上线）。
- **门禁 2**：长程测试（Space Sentry 单会话构建游戏）dsh 版全绿；压缩 A/B（省 token / 答案质量）**不劣化**于现状。

### Phase 3 能力插件化（差异化上移）

- 权限/ask_user、记忆桶、coding-memory、web-search 策略、refine-suggest、pdf-omni → dsh 插件形态。
- 作用域（plugin_scopes/skill_scopes + `[apps]`）→ `dsh-scope` 缝 + 我们的 app 边界插件。
- **门禁 3**：设置中心/作用域/权限/记忆逐项验收（对照 REVIEW_PERMISSIONS_TOOLS_2026-08-17 报告项）。

### Phase 4 专家团队 + 管家

- `team` 插件（团队=agents 清单+队长配置，对齐 expert-team.md）+ `dsh-subagent`（spawn-in-process，含 continuable 后台子代理）+ `dsh-workflow`/`dsh-goal` 编排 + **outputSchema 结构化返回（dsh 原生，替代我们 vendor P9 补丁）** + 团队 DAG 可视化（社区 `dsh-task-dag` 参照）。
- `steward` 管家插件（治理区间政策 + set_wake 等价值）；自我进化=版本化替换（dsh 插件原子替换天然支持）。
- **门禁 4**：专家团队全链路（派工 → 并行 → 结构化返回 → 汇总）对照 expert-team.md 阶段路线。

### Phase 5 前端迁移（细节见 §三·A）

- b→a 两步：先 React 壳接 dsh API 保业务不中断 → 再逐单元改造成 ui-slot 插件替换。
- **门禁 5**：A–E 24 项回归 + 皮肤/特效/i18n 三语验收。

### Phase 6 退役与管线切换

- `bm-server` 只读旧会话 + 静态托管；turso 归档（只读）。
- 构建/发布重构：便携包内置 Node runtime、Docker 镜像重做、CI 换（Node 测试 + 现有 VMware runner 复用）、热升级改造（dsh 插件热装 = 天然载体）。
- **门禁 6**：v0.2.0 全量回归 + 便携包真实启动（沿用"先本地实测再发版"铁律）。

### 回退条件

任一阶段门禁不过 → 该阶段回退，dsh 侧停止接管，`bm-server` 继续服务。数据双写/双存保证无丢失。**回退不是失败，是 strangler 的正常分支。**

---

## 三·A 前端

### 现状

React 19 + Vite 8 + pnpm + tailwind 4 + dockview-react 8.1 + @base-ui/react + i18next（zh/en/ja/ko）+ 皮肤系统（data-skin + 参数化）+ 特效。组件已高度单元化（ChatPane/SessionList/FilePanel…），改造是**机械映射**而非重写。

### dsh 前端机制（08-17 复核）

- 壳 = `dsh-web-app`（React），服务 127.0.0.1:3080；插件 UI 经 **ui-slots**（SlotMap 声明合并 → `register()` 贡献组件/子槽/store/语言包）注入到 AppFrame 的 `sidebar` / `conversation` / `details` / `conversation.empty` 槽。
- 客户端插件 = 构建好的 JS bundle，`dsh.client` 声明 + `window.__DSH_BOOT__` 启动图 + `__ModuleLoader__`（浏览器侧内核机制）注册 factory；**React 由壳提供实例**（可带 React，官方 UI 即 React——修正 2026-08-15 旧论断）。
- 主题：`dsh-client-ui-theme` 统一 `--dsw-*` + `--dsw-alias-*` 语义令牌 → 我们的皮肤参数化直接映射到令牌层。

### 三条路线（§八·2 拍板）

- **a. 直接替换**：壳换 `dsh-web-app`，我们全部组件改 ui-slot 插件。红利最大，但一步到位风险高。
- **b. 长期混合**：保留我们的壳，dsh headless 后端 + 消费其 API。迁移成本最低，但"前端即插件"红利吃不到（我们的 UI 隔离/皮肤/单元化仍是自研体系）。
- **c. 渐进（推荐）**：Phase 5 内 b→a 两步走——先壳接 API 保业务，再逐单元插件化替换，每替换一个槽位即验收。

### 单元化处理（§五 详述）

每个单元 = 一个 ui-slot 插件，独立可替换（呼应"功能原子化、管家决定替换"）；默认布局 + 可重置。

---

## 四、插件选型（本轮已下载，见 `third-party/dsh-plugins/`）

> 选型原则：**官方缝优先**（compaction/subagent/mcp/tools/session 都有官方插件，不重复造）；**社区只收"我们缺且官方无"的 UI 类**；浏览器自动化缺口自研（T4 续）。版本一律锁 rc 快照。

### 表 A：官方 npm（44 包，`npm pack @latest` → `official/`）

| 类别 | 包（@deepseek-ai/） | 对应我们 |
|---|---|---|
| 运行时 | dsh、dsh-base、dsh-web-app、dsh-headless | 引擎/壳/无头 |
| 回合/会话 | dsh-agent、dsh-agent-loop、dsh-session | bm-loop/bm-kernel |
| 存储 | dsh-session-persistence-sqlite / -jsonl、dsh-storage、dsh-storage-sqlite / -json | turso/事件日志 |
| 工具 | dsh-tools、dsh-tool-fs、dsh-tool-fs-search、dsh-tool-str-replace-editor、dsh-tool-terminal、dsh-tool-bash、dsh-tool-web、dsh-web-search-deepseek、dsh-web-fetch-http | 工具面/搜索 |
| MCP | dsh-mcp-client | bm-mcp |
| 压缩/记忆 | dsh-compaction、dsh-compaction-basic、dsh-compaction-tool-result-pruner、dsh-spill、dsh-spill-local | ctx-compactor |
| 子代理/编排 | dsh-subagent、dsh-subagent-spawn-in-process、dsh-tool-subagent-control、dsh-workflow、dsh-tool-workflow、dsh-goal | 专家团队 |
| 专家/作用域 | dsh-agent-presets、dsh-scope | agents/*.md、plugin_scopes |
| UI | dsh-client-ui-slots、dsh-client-ui-layout、dsh-client-ui-theme、dsh-client-ui-tool、dsh-client-ui-trajectory、dsh-client-web-react | 槽位/布局/皮肤/工具可视化 |
| 遥测/审计 | dsh-session-telemetry | 工具调用显示/审计 |
| LLM | dsh-llm、dsh-llm-deepseek | Provider 适配缝 |

### 表 B：社区 GitHub（9 仓库已验证，`git clone --depth 1` → `community/`）

| 仓库 | 作用 | 对应我们 |
|---|---|---|
| No-PRM/dsh-explorer | Git-first 文件树（git 装饰） | FilePanel |
| WhitePlusMS/dsh-git-graph | 分支图 | GitGraph |
| SenryLee/dsh-frosted-window | 毛玻璃皮肤 | 玻璃皮肤（Aqua 参照） |
| LeemanCheung/dsh-task-dag | 子代理任务 DAG 可视化 | 专家团队编排可视化 |
| giiiiiithub/terminal | node-pty + xterm 终端 | 终端单元 |
| tsonglew/dsh-workspace-search | VS Code 式工作区关键字搜索 | 工作区检索 |
| Js2Hou/dsh-mcp-manager | MCP 管理面板 | 设置中心 MCP 段 |
| zhijun-dai/Catppuccin-dsh-theme | 主题（令牌改造参照） | 皮肤系统 |
| dsh-market/dsh-market | 插件市场（curated list） | 插件市场（复用 ZCode marketplace.json 机制） |

另附 `awesome-dsh-plugin/awesome-dsh-plugin` curated 清单作持续跟踪入口。

---

## 五、UI 单元化处理

- **定义**：单元 = 可在 dsh ui-slot 中独立注册/替换的插件单位。映射见 §2.2 表。
- **我们的特有单元（dsh 没有的，自研）**：权限弹窗（PermissionDialog/AskDialog）、工具调用可视化（ToolCallBlock + TokenRing，dsh 只有 trajectory 基础）、任务清单（TodoPanel → ui-plan 对照）、皮肤选择器。
- **布局**：dsh AppFrame 三栏（sidebar/conversation/details，可拖拽/悬浮 pill）vs 我们的 dockview workbench——§八·3 拍板（默认建议跟随生态三栏，dock 需求量大再自研布局插件）。
- **隔离**：dsh 客户端插件跑在壳 React 运行时（崩溃波及全局）→ 我们的关键插件（皮肤/特效/第三方）做**防御式错误边界**；重度隔离（iframe/子进程）保留为我们的工程纪律。
- 默认布局 + 重置按钮延续现有设计（dockview 默认布局已迁移为 dsh 默认布局）。

---

## 六、功能亮点处理

1. **专家团队**（expert-team.md 为唯一描述载体）：`team` 插件 = 团队配置集合（队长+agents 清单+模型/技能分配，与 `agents/*.md` 互导）+ `dsh-subagent` 子代理（**spawn-in-process 隔离天然满足数据隔离**，continuable 后台子代理支持长任务）+ `dsh-workflow`/`dsh-goal` 队长编排 + `outputSchema` **结构化返回（原生替代 P9 补丁，删 UPSTREAM_PATCHES 对应项）** + `dsh-task-dag` 团队 DAG 可视化 + 团队单元入 conversation/details 槽。预置团队（办公/炒股/养生）即插即用。
2. **管家 Steward**：`steward` 插件 = 治理区间政策（300s~86400s）+ wake 工具 + 管家会话注入（BM_STEWARD_SESSION 语义保留）；**自我进化 = 版本化替换**——dsh 插件原子替换天然承载（不用再自建 74B 热升级签名链）。
3. **皮肤/特效**：`--dsw-*` 令牌层参数化（三参数滑杆语义保留）；玻璃/礼花/波纹 → 皮肤插件（毛玻璃参照 SenryLee/dsh-frosted-window，特效插件自研，Canvas/WebGL 不变）。
4. **工具调用显示/审计**：`dsh-session-telemetry` + `dsh-client-ui-tool` + 我们的摘要键（DSH 摘要键已在 tool-summary.ts 用过）+ 事件日志审计 UI 保留（SessionEvent append-only 与 dsh **同构**——可审计心智是 dsh 原生语义）。
5. **记忆分层/压缩**：记忆 = `dsh-storage-domain` 表单 + 我们的记忆插件（桶/项目隔离）；压缩 = `dsh-compaction` 策略插件（把 ctx-compactor 的修剪/秘密扫描/找回逻辑原样搬迁，触发水线对齐 bm-core 50%）。
6. **可审计心智/软件形态革命**：append-only 事件日志（dsh 原生）+ 我们的审计 UI + 事件血缘（GlobalSeq 迁移为 dsh `agent/*` 事件序列）。
7. **便携包/热升级/三平台**：Node 22 runtime 内置便携包 + dsh 插件热装（热升级天然载体）；三平台 = dsh 跨平台 + Tauri 壳复用。

---

## 七、风险与对策

| 风险 | 对策 |
|---|---|
| pre-1.0 破坏性变更（rc 版本/格式 0） | 锁 0.1.0-rc.6 快照 + lockfile；UPSTREAM_TRACKING 增 dsh 区，补丁最小化+issue 优先 |
| 数据迁移（turso → dsh sqlite） | Phase 1 双存；一次性迁移工具；turso 只读归档；事件日志语义同构，迁移可行 |
| 性能（Node vs Rust：回合间隔/并行吞吐/大上下文） | Phase 0 基准先行（对照现有长程数据）；不过标 Phase 2 重议 |
| **沙箱丧失**（dsh 插件无沙箱） | §八·4：两档信任（官方插件宿主进程 / 第三方 worker 隔离）+ 权限插件兜底 |
| 浏览器自动化缺口 | 自研浏览器插件（T4 续排期） |
| CI/分发重构（Rust→Node 构建链） | Phase 6 专项；VMware runner 复用 |
| 生态泡沫（144k 星/4 天） | 只信源码+锁版本；社区插件逐个源码核验（已做 9 个） |
| 前端隔离退化（插件崩溃波及全 UI） | 防御式错误边界 + 关键插件分级隔离 |

---

## 八、拍板点（本轮待用户定）

1. **数据桥**：dsh 独立 sqlite + 一次性迁移工具（**推荐**，两 schema 不同步）vs 自研持久化适配器把 SessionEvent 直写现有 turso。
2. **前端路线**：c 渐进 b→a（**推荐**）vs a 直接替换 vs b 长期混合。
3. **布局**：跟随 dsh AppFrame 三栏（**推荐**，吃生态红利）vs 自研 dock 布局插件（保留 workbench）。
4. **插件信任**：两档——官方/自研插件宿主进程 + 第三方插件隔离 worker（**推荐**，映射现有普通/资深分级）vs 全宿主 vs 保留 QuickJS 硬沙箱给不可信插件。
5. **浏览器自动化**：本轮排期自研浏览器插件（**推荐**）vs 后置。
6. **Node 分发**：便携包内置 Node 22 runtime（**推荐**）vs 依赖系统 Node。
7. **Rust 资产**：`bm-server` 全退役（只读旧会话）vs 保留重服务子进程（pdf-omni 等 Rust 端点）。
8. **门禁验收**：沿用 VMware runner + 本地 pre-push 钩子（**推荐**）vs 新设。

---

## 附：与既往决策的连续性

- 三轮"不换"讨论设计的 strangler 迁移路径（迁移假设轮产物）**原样沿用**——本计划是它的执行细化。
- "学 dsh 不抄 dsh"：现在直接用 dsh 本体，自研插件仍按我们架构（不做 dsh 内部二开）。
- 生态转接器原则：zcode/hermes 等生态仍走转接器插件，不动 dsh 核。
- 万物皆插件/插件自治边界：dsh 是"一切皆插件"的完整实现，我们的三铁律与它同构——迁移是换实现保语义。
