# BoenMind 扶正计划：新建独立项目，以 DSH 为唯一真身（2026-08-17）

> 状态：**待拍板**（§八 拍板点 1–7）
> 背景：2026-08-17 用户拍板以 DeepSeek Harness（dsh）为核心运行时。**初稿（DSH_MIGRATION_2026-08-17.md）被否：不是改造现有仓库，是新建独立项目、以 dsh 为真身扶正**，现有 BoenMind 仓库（pi 内核时代）降级为参考资产。本稿按此重写。
> **事实基线声明**：本计划只依据 2026-08-17 的最新核查（npm registry + 本机 dsh 浅克隆 + 官方 docs），**不引用 2026-08-15 及以前的任何旧调研结论**（用户定调：以最新 DSH 为准，避免旧结论误导）。
> 关联：`third-party/dsh-plugins/`（插件快照，已下载）、`frontend/public/docs/expert-team.md`（专家团队描述载体）、`backend/vendor/UPSTREAM_TRACKING.md`（上游台账，增补 dsh 区）。

---

## 〇、一句话结论

新建独立仓库（新分支起步），**`dsh-base`（官方默认全家桶）+ `dsh-web-app`（官方 UI 全家桶）作为开箱即用的唯一真身**，先原样跑起来、跑对，再按我们自己的想法**逐单元升级**（先官方、后社区、最后自研）；专家团队/管家/皮肤/审计/记忆等 BoenMind 亮点全部以 **dsh 插件形态**分层挂载。现有仓库只当"参考资产"读，不改造、不拖带。**迁移一词作废，改称扶正（dsh 为主，我们做增强）。**

---

## 一、先回答用户四个问题

### 1. npm 的程序要不要编译？比 Rust 长还是短？

- **官方 dsh 包：发布即产物，无编译**。已核查：`@deepseek-ai/dsh`、`dsh-web-app` 的 package.json 无 postinstall、无 node-gyp/napi 构建（native 能力走官方预编译 addon）。`npm install` = 下载 + 解压 + 依赖解析。
- **全量安装官方全家桶**（dsh-base 80+ 包 + web-app 全套）≈ 几分钟（取决于网络），之后 `dsh web` 即开。
- **自研插件需要构建**：TS 源码 → 宿主 bundle + client bundle（esbuild/rolldown，**秒级**），官方支持热重载（`dsh-client-hmr`），改插件即改即生效。
- **对比 Rust 现状**（真实数据）：本地 debug 编译几十分钟级（2GB 产物坑见记忆）、CI VMware runner 全量 10 分钟+。→ **dsh 时代"编译时间"基本消失**，迭代速度数量级提升，正好兑现"实现速度∝Token"哲学——省下的编译时间全部变成功能迭代的 Token。

### 2. 桌面版怎么搞？还能用现在的外壳吗？

- **能，现有 Tauri 2 外壳整体复用**。dsh 官方无桌面壳（纯 web 服务，`dsh web` 监听 127.0.0.1:3080）。
- 方案：Tauri 窗口加载 dsh web 产物（**build 时把 web-app 产物拷入 `frontendDist`，壳内同时拉起本机 node dsh 后端进程**）。现有 `frontend/src-tauri` 的配置全是资产：图标、产品名/identifier、签名、updater 端点、打包目标——直接移植到新项目。
- 便携包：内置 Node 22 runtime（拍板点 3），延续"便携目录 + 多文件形态"设计。

### 3. 全家桶优先

- **官方全家桶 = dsh-base 默认组合（80+ 包，即 default profile）+ dsh-web-app（28 个 client-ui 单元）**——已从 npm registry 实取依赖清单（§三 附构成表）。
- 策略：**先整体用官方全家桶，跑通跑对；社区整站全家桶按需（dsh-web-ui / dsh-side-panel / dsh-market）；我们自研的插件逐个替换升级**（用户原话："后面我们再按自己的想法来一个一个升级"）。
- 好处：官方包间配合度最高（同一仓库同版本同契约），开箱即用的 UI 单元覆盖 conversation/sidebar/settings/plan/goal/subagent/workspace/permission/tool/trajectory/skill/theme/layout……我们现有前端单元绝大多数有官方对应（§五 映射表），先免费拿到，再按我们口味改造。

### 4. 关于记忆清空

记忆文件不物理删除（保留决策轨迹：三轮"不换"讨论、strangler 设计仍是资产）；但**本计划已声明不引用旧结论**，且旧记忆均已加"已被取代"标注。后续所有轮次以本计划 + 最新 npm 事实为准。

---

## 二、新项目形态

```
boenmind-dsh/               # 新仓库（或现有仓库新分支 refactor/dsh-core，拍板点 1）
├── dsh-home/               # DSH_HOME：profiles/ + 配置 + 锁版本 lockfile
│   ├── profiles/default/   # dsh-base + dsh-web-app + 我们的业务插件（bundle 有序组合）
│   └── dsh.profile
├── plugins/                # 我们的自研插件（TS，dsh 规范）
│   ├── team/               # 专家团队（对齐 expert-team.md）
│   ├── steward/            # 管家（治理区间 + wake）
│   ├── skins/              # 皮肤/特效（--dsw-* 令牌）
│   ├── memory/             # 记忆分层/项目隔离
│   ├── audit/              # 审计/工具调用显示升级
│   └── browser/            # 浏览器自动化（生态缺口，自研）
├── shell/                  # Tauri 2 桌面壳（移植现有配置）
├── node-runtime/           # 便携包内置 Node（拍板点 3）
└── docs/
```

- **版本**：产品名保持 BoenMind；版本建议 **v0.2.0 起**（v0.1.x = pi 内核时代，v0.2.x = dsh 内核时代），拍板点 2。
- **参考资产**：现有仓库只读查阅（插件逻辑/前端组件/专家提示词/记忆语义都是搬运素材，不是代码依赖）。

---

## 三、后台实现顺序（无 strangler 包袱，直接建真身）

> 新项目没有"并行共存/回退"负担——每一步都是在 dsh 真身上叠加 BoenMind 语义，门禁 = 该步验收标准。

### M0 骨架与全家桶 bootstrap（第 1 轮可完成）

- 新仓库初始化 + `npm init` + 锁版本安装：dsh、dsh-base、dsh-web-app（**按 npm lockfile 锁死 rc 快照**，升级走显式操作）。
- `dsh web` 跑通：默认 profile 启动 → 完成一次完整会话（用户消息 → 工具调用 → 回复）→ 设置页/模型配置/会话列表可用。
- 配置 DeepSeek 模型（`dsh-llm-deepseek` 已在全家桶）。
- **门禁 0**：纯官方全家桶下，Chat 全链路 + 文件工具 + 子代理 + 工作区浏览全通。

### M1 产品外壳（登录/鉴权/桌面）

- Tauri 壳移植：窗口加载 dsh web、图标/签名/updater 配置迁入；`BOENMIND_TOKEN` 鉴权语义接入 `dsh-api-gateway`（全家桶自带）。
- 便携包形态：Node runtime + dsh-home + 壳，多文件目录（沿用既有设计）。
- **门禁 1**：桌面版从零启动 → 登录 → 会话 → 关闭重开数据完好。

### M2 业务插件面（BoenMind 语义挂载）

- 记忆插件（`dsh-storage-domain` 表单 + 桶/项目隔离）、皮肤插件（`--dsw-*` 令牌参数化）、审计/工具显示插件（`dsh-session-telemetry` + 摘要键）、权限策略插件（两档信任 + 审批）。
- **门禁 2**：逐插件验收（对照 REVIEW_PERMISSIONS_TOOLS_2026-08-17 报告项）。

### M3 专家团队 + 管家（亮点上移）

- `team` 插件：团队=队长+专家清单（模型/技能分配，对齐 expert-team.md）+ `dsh-subagent`（spawn/fork，continuable 后台）+ `dsh-workflow`/`dsh-goal` 编排 + `outputSchema` 结构化返回（原生能力）+ 团队 DAG 可视化（社区 task-dag 参照）。
- `steward` 插件：治理区间政策 + wake + 版本化替换进化（dsh 插件原子替换天然承载）。
- **门禁 3**：专家团队全链路（派工→并行→结构化返回→汇总）+ 管家轮次自驱。

### M4 发布与三平台

- CI 重做（Node 测试 + 现有 VMware runner 复用）、Docker 镜像、热升级（dsh 插件热装天然载体）、三平台打包（Tauri 跨平台）。
- **门禁 4**：v0.2.0 全量回归 + 便携包真实启动（沿用"先本地实测再发版"铁律）。

---

## 四、官方全家桶构成（npm registry 实取，2026-08-17）

### dsh-base 默认组合（default profile，80+ 包，节选主干）

| 域 | 包 |
|---|---|
| 核心 | dsh-agent / dsh-agent-loop / dsh-session / dsh-system-prompt / dsh-tools / dsh-web / dsh-goal / dsh-skill |
| 工具 | dsh-tool-fs / dsh-tool-fs-search / dsh-tool-str-replace-editor / dsh-tool-bash / dsh-tool-pwsh / dsh-tool-web / dsh-tool-subagent / dsh-tool-subagent-control / dsh-tool-subagent-report / dsh-tool-goal / dsh-tool-todo / dsh-tool-tasks / dsh-tool-skill / dsh-tool-workflow / dsh-tool-ralph |
| 子代理/编排 | dsh-subagent / dsh-subagent-spawn / dsh-subagent-fork / dsh-subagent-codex / dsh-subagent-claude-code / dsh-workflow-workerthread / dsh-plan-mode |
| 压缩/记忆 | dsh-compact-basic / dsh-compact-tool-result-prune / dsh-spill-local / dsh-spill-policy / dsh-session-checkpoint-policy / dsh-session-query-sqlite |
| 安全/权限 | dsh-permission / dsh-user-approval / dsh-fs-policy / dsh-fs-sandbox / dsh-bash-sandbox / dsh-pwsh-sandbox / dsh-sandbox-local / dsh-sandbox-policy / dsh-timeout-policy / dsh-repeat-tool-guard / dsh-credentials-local |
| 存储/设置 | dsh-session-persistence-jsonl / dsh-settings-local / dsh-tasks-local / dsh-attachment-local / dsh-session-title |
| LLM | dsh-llm / dsh-llm-deepseek / dsh-llm-pi-ai / dsh-llm-retry / dsh-token-meter / dsh-agent-default-model |
| 其他 | dsh-commands / dsh-command-goal / dsh-command-compact / dsh-command-feedback / dsh-api-gateway / dsh-user-interaction / dsh-workspace-context / dsh-session-projection / dsh-session-telemetry-otel / dsh-fs-local / dsh-subprocess-local / dsh-skill-local / dsh-skill-badge / dsh-typert-loader / dsh-typert-registry / dsh-bash-env / dsh-web-search-deepseek / @deepseek-ai/cordis-plugin-hmr / @deepseek-ai/cordis-plugin-timer |

### dsh-web-app UI 全家桶（28 个 client-ui 单元 + 运行时）

| 单元 | 包 | 对应我们（映射参照） |
|---|---|---|
| 会话 | dsh-client-ui-conversation | ChatPane/SessionList/ChatInput/MessageItem |
| 侧栏/工作区 | dsh-client-ui-sidebar / dsh-client-ui-workspace | FilePanel/ProjectSwitcher/工作区 |
| 设置 | dsh-client-ui-settings / -settings-general / -agent-preset | 设置中心/专家预设 |
| 规划/目标 | dsh-client-ui-plan / dsh-client-ui-goal | TodoPanel/任务清单 |
| 子代理 | dsh-client-ui-subagent | 专家团队子代理视图 |
| 工具/轨迹 | dsh-client-ui-tool / dsh-client-ui-trajectory | ToolCallBlock/工具调用可视化 |
| 权限/提问 | dsh-client-ui-permission / dsh-client-ui-question / dsh-client-ui-user-questions | PermissionDialog/AskDialog |
| 模型/技能/命令 | dsh-client-ui-model / -models / -skill / -slash / -command | 模型下拉/技能/命令 |
| 主题/布局 | dsh-client-ui-theme / dsh-client-ui-layout | 皮肤/布局/状态栏 |
| 交付物/作业 | dsh-client-ui-deliverables / dsh-client-ui-jobs | 交付/后台任务 |
| 运行时 | dsh-app-boot / dsh-client-connection / dsh-client-modules / dsh-client-runtime / dsh-client-locale / dsh-client-hmr / dsh-client-ui-directory-picker-native / dsh-api-remotes | 壳/连接/模块加载/语言/热重载/目录选择 |

> **注意（包名演进实录）**：npm 侧包名比 08-13 基线已变（如 `dsh-compact-basic`/`dsh-subagent-spawn` vs 早前 `dsh-compaction`/`dsh-subagent-spawn-in-process`）——**一切以 npm lockfile 为准**，选型快照 `third-party/dsh-plugins/official/` 已含最新 tgz。

---

## 五、前端：单元化 = 先官方后自研，逐个升级

1. **第一步：官方全家桶原样跑**（M0 门禁 0 已覆盖）——conversation/sidebar/settings/plan 等单元全部来自官方包，零自研先见全貌。
2. **第二步：逐单元按我们想法升级**（用户定调）：
   - 聊天单元：官方 conversation 基础上，换我们的输入区（TokenRing/排队不打断/停止发送并存）、消息渲染（ThinkBlock/ToolCallBlock 摘要化）、流式头像。
   - 目录树：官方 workspace → 社区 dsh-explorer（git-first）→ 最终自研（含 ProjectSwitcher 语义）。
   - 皮肤/特效：官方 theme 令牌 → 我们 skins 插件（参数化滑杆 + 玻璃 + 礼花/波纹 WebGL）。
   - 状态栏/审计：官方 trajectory → 我们的摘要键 + TokenRing 圆环。
   - 权限弹窗：官方 permission → 我们的 AskDialog/PermissionBridge 语义。
   - 布局：官方 AppFrame 三栏 → 若要 workbench 级 dock 再自研布局插件（后置）。
3. **每升级一个单元 = 一个独立插件替换，可单独回退**（呼应"功能原子化、管家决定替换"）。

---

## 六、功能亮点处理（全部 dsh 插件形态）

1. **专家团队**：`team` 插件 = 团队配置（队长+专家清单+模型/技能分配，与 agents/*.md 互导）+ dsh-subagent（进程内/独立子代理，continuable 后台长任务）+ workflow/goal 队长编排 + **outputSchema 结构化返回（dsh 原生，替代我们 vendor P9 补丁）** + 团队 DAG 可视化（社区 task-dag 参照）。
2. **管家 Steward**：`steward` 插件 = 治理区间（300s~86400s）+ wake 工具 + 管家会话注入；自我进化 = 版本化替换（dsh 插件原子替换天然承载，不用自建 74B 热升级链）。
3. **皮肤/特效**：`--dsw-*` 令牌参数化（三参数滑杆语义保留）；玻璃皮肤参照社区 frosty-window；礼花/波纹特效自研插件（Canvas/WebGL 代码不变，适配 dsh 主题令牌）。
4. **工具调用显示/审计**：官方 `dsh-session-telemetry` + `dsh-client-ui-tool/trajectory` + 我们的摘要键逻辑（DSH 摘要键现成）→ `audit` 插件。
5. **记忆分层/项目隔离**：`dsh-storage-domain` 表单 + `memory` 插件（桶/项目隔离语义搬入）。
6. **可审计心智**：dsh 原生 append-only SessionEvent（与我们的事件日志同构）→ 审计 UI 直接建在官方轨迹上。
7. **便携包/热升级/三平台**：Node runtime 内置 + Tauri 壳复用 + dsh 插件热装（热升级天然载体）。

---

## 七、风险与对策

| 风险 | 对策 |
|---|---|
| rc 生态破坏性变更 | 锁 npm 快照 + lockfile；升级显式化（UPSTREAM_TRACKING 增 dsh 区）；快照目录已备 |
| 无沙箱（插件=宿主进程全权） | 两档信任（官方/自研宿主 + 第三方隔离 worker，拍板点 5）+ 官方 sandbox 包先用（dsh-fs-sandbox/bash-sandbox 已在全家桶） |
| 浏览器自动化缺口 | 自研 browser 插件（T4 续排期） |
| 数据（历史会话进不了新内核） | 只读归档现有 turso；一次性导入脚本可选（SessionEvent 语义同构） |
| 前端 React 版本/契约漂移 | 插件只用官方提供的 React 实例与 ui-slot API（不锁死依赖版本） |
| 生态泡沫（星数异常） | 只信源码 + lockfile；社区插件逐个源码核验（已核 11 个） |

---

## 八、拍板点（待用户定）

1. **新仓库 vs 现有仓库新分支**：新仓库 `boenmind-dsh`（**推荐**，真身独立、参考资产只读）vs 现有仓库 `refactor/dsh-core` 分支（保留历史、CI/发布要拆干净）。
2. **版本号**：v0.2.0 起（**推荐**，语义清晰）vs v0.1.0 独立起 vs 其他。
3. **Node 分发**：便携包内置 Node 22 runtime（**推荐**）vs 依赖系统 Node。
4. **桌面壳**：Tauri 2 复用现有配置（**推荐**）vs 纯浏览器先行（web 版先发，桌面后置）。
5. **插件信任两档**（**推荐**）vs 全宿主 vs 自研硬沙箱。
6. **浏览器自动化**：M2 后立即排期（**推荐**）vs 后置。
7. **历史数据**：只读归档（**推荐**）vs 一次性导入脚本 vs 不迁。

---

## 附：与既往决策的连续性（仅语义层）

- "学 dsh 不抄 dsh" → 现在直接用 dsh 本体，自研插件仍按我们架构（不做 dsh 内部二开）。
- 生态转接器原则：zcode/hermes 等生态仍走转接器插件，不动 dsh 核。
- 万物皆插件/插件自治边界：dsh 是完整实现，我们的三铁律与它同构——扶正 = 换实现保语义。
- 三护城河（可审计心智/软件形态革命/Steward 治理）：全部在 dsh 语义上保留（§六）。
