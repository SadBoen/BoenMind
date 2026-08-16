# BoenMind 设置面板架构（2026-08-16 定稿）

> 状态：**设计定稿，实施进行中**。阶段状态见文末表格。

## 一、需求（用户开题，2026-08-16）

1. SKILL / MCP / 插件都允许有设置页面（如设置 KEY）。
2. 软件 APP 之间隔离；APP 引用的 LLM 由管家派出的专家提供 → 需要专家预设界面。
3. 不同 APP 的 LLM 对应 SKILL/MCP/插件不同：有独有的、有公共的（如聊天与编程的记忆系统不同）。
4. 每个软件 APP 本身有单独设置界面。
5. 桌面形态全删除（留一个点了没用的切换开关），等软件形态稳定后再议。

外加：设置内容分级——"普通用户 / 资深者"两档，资深模式提供更多设置内容。

## 二、拍板记录（2026-08-16）

| # | 决策点 | 结论 |
|---|---|---|
| 1 | 桌面形态删除范围 | **全删除**（desktop 组件 8 文件 + 相关 store 状态 + 外观页壁纸区），保留形态切换开关（点桌面选项仅 toast 提示，无实际效果） |
| 2 | 专家预设与 subagent 角色 | **同池**：预设即 `~/.boenmind/agents/*.md`（AgentDefinition frontmatter 超集），子代理派工与 APP 专家读同一批人，不搞两套目录 |
| 3 | 作用域引擎过滤 | **做**：manifest scopes + 引擎按 session.app 过滤工具面（bm_engine.rs 场景工具组装登记点），分步实施但要看到最后效果 |
| 4 | per-app 配置存储 | **单源**：config.toml 一个文件加 `[apps.<appId>]` 段；所有 APP 共用同一套 LLM 交互底层（bm-engine），APP 专属配置只是单源文件内的段，不做独立配置文件 |
| 5 | SKILL 设置 schema 载体 | skill 目录下 `settings.json`（调研结论：官方 SKILL 规范无设置项概念，生态通用做法即外部配置文件；SKILL.md 保持官方规范兼容） |
| 6 | 预置专家团队 | 本期只做容器 + default + **编程三专家**（architect/coder/reviewer）；办公/炒股/养生预置留规划 |
| 7 | 分级机制首期范围 | 外观 + 扩展中心 + 管家页 |

## 三、SKILL 规范调研结论（拍板点 5 依据）

- 官方规范（Anthropic `agent_skills_spec` v1.0，anthropics/skills 仓库）：SKILL.md frontmatter 必填 `name`/`description`，可选 `license`/`compatibility`/`metadata`/`allowed-tools`。**无设置项概念**。
- 生态现状：各平台（Claude Code 等）扩展的是调用行为字段（model/hooks/argument-hint），均非"用户配置表单"；用户可配置项通用做法 = **skill 目录/用户目录下的外部配置文件**（如 drawio-skill 的 styles/<name>.json）。
- 结论：SKILL.md 保持官方规范兼容（可移植），设置项走同目录 `settings.json`（与插件 `settingsSchema` 同构的 JSON Schema 描述）。

## 四、总体架构

设置体系为三级结构：

```
设置中心（Settings App）
├── 应用组（每 APP 一页，普通用户主战场）
│     聊天设置 / 编程设置 / [WIKI 占位]
├── 系统组（全局）
│     专家预设 · 模型提供商 · 扩展中心(SKILL/MCP/插件) · 管家 · 外观 · 关于
└── 贯穿机制：普通/资深分级 + 搜索（后续）
```

- 每 APP 设置页双入口：设置中心"应用"分组 / APP 内工具栏设置按钮。
- 扩展中心 = SKILL/MCP/插件 三个子 tab 收敛（现散在 3 个独立 tab），统一处理启停 + 设置 + 作用域。
- 外观降级为全局设置的一个分组（桌面退役后变薄）。

## 五、每 APP 设置页（模板）

所有带 LLM 的 APP（chat/coding/未来 wiki…）共用模板：

| 分组 | 内容 |
|---|---|
| 模型与专家 | 该 APP 绑定的专家预设（管家派工依据）、默认模型、思考档位 |
| 扩展 | 该 APP 独有 SKILL/MCP/插件（增删）+ 查看公共扩展（只读） |
| 记忆 | 记忆桶开关与选择（聊天=全局聊天记忆；编程=按项目分桶，coding-memory 上升为通用机制） |
| 布局 | 该 APP 的 dockview 布局（重置/编辑，DEFAULT_LAYOUTS 已按 appId 分布局） |
| 工作区 | 工作目录（现全局 workspace 设置下放 APP 级） |

数据落点：后端 config.toml `[apps.<appId>]` 段；前端 APPS 注册表 `AppEntry` 加 `settingsComponent` 字段登记每 APP 设置页。

## 六、专家预设

**概念**：专家预设 = 管家指派给 APP 的"工作人格"完整配置，是 subagent 角色（AgentDefinition）的超集：

```
专家预设 = 角色（提示词 + 说明） + 模型(provider::model)
         + 扩展子集（允许的 SKILL/MCP/插件） + 记忆桶（可选）
```

- 界面：专家预设页 = 卡片列表（启用/停用）+ 编辑对话框。存 `~/.boenmind/agents/`，与 subagent 工具同池。
- 预置：`default`（已有）+ 编程三专家：

| 专家 id | 名称 | 工具面 | 职责 |
|---|---|---|---|
| architect | 架构师 | 只读（read/grep/find/ls）+ write | 需求拆解、方案设计、结构决策、技术评审 |
| coder | 码农 | 全工具（含 edit/write/bash） | 按方案实现、修 bug |
| reviewer | 审查者 | 只读 + bash（跑测试/静态检查） | 代码审查、测试验证、质量报告 |

- 管家对接：管家设置页补"管家会话的专家/模型选择"（写 StewardConfig），与专家预设联动。

## 七、扩展统一设置模式

三类扩展统一为"扩展 = 元数据 + 启停 + 设置 schema + 作用域"：

| 扩展 | 设置 schema 载体 | 说明 |
|---|---|---|
| 插件 | 已有 `settingsSchema`（manifest） | 已满足（PluginSettingsDialog schema 表单） |
| SKILL | skill 目录下 `settings.json`（新约定） | SKILL.md 保持官方规范兼容；有 schema 才显示"设置"按钮 |
| MCP | server 配置已有 `env/headers/tool_timeout_ms` | env 即 KEY 天然载体；McpSettings 编辑表单补全（键值编辑、掩码显示） |

KEY 等敏感值沿用 providers 做法：存 config.toml、表单掩码显示。"设置页"对三类扩展同一套交互语言（列表项 → 齿轮 → 表单）。

## 八、作用域模型

```
公共扩展（scope: *）   → 所有 APP 会话工具面（现状即全局）
APP 独有扩展（scope: chat/coding）→ 只进该 APP 会话工具面
```

- 落地：manifest/配置加 `scopes` 字段；bm_engine.rs 场景工具组装登记点（L346 附近，现 chat/coding 分支为空）按 session.app 过滤插件工具/MCP 工具/场景 skill。
- 记忆按作用域：记忆桶 = (app, 可选项目)。聊天 APP 用全局 memory/facts.md；编程 APP 用 coding-memory 项目桶。
- 前端：扩展中心每行显示作用域徽标（公共/聊天/编程）可改归属；插件页 category 分类（all/system/app）演进为作用域过滤。

## 九、外观设置（桌面退役后）

| 分组 | 标准可见 | 资深可见 |
|---|---|---|
| 主题 | 亮/暗/跟随系统 | 强调色 |
| 文字 | 字体档位 小/标准/大 | 字体族、界面密度、reduce motion |
| 语言 | 4 语 | — |
| 布局 | 一键重置布局（带确认） | 恢复全部外观默认 |

桌面退役：删 components/desktop/ 8 文件；store 删 wallpaper/openApps/focusedApp/minimized 状态块；viewMode 保留（默认 classic）；外观页形态区保留切换 UI，点"桌面"仅 toast 提示。

## 十、普通/资深分级

- 全局一个开关（设置页右上角），`tier: basic|expert` 元数据驱动显示，存 `boenmind.settingsTier` 默认 basic。
- **切换只改可见性，绝不动任何已保存设置值**；关键操作（重置类）永远可见 + 确认框。
- 首期标注：外观全部、扩展中心（资深可见作用域编辑/高级字段）、管家页整体（资深向）。应用设置页全部 basic。

## 十一、数据模型与存储

- 后端 config.toml（单源权威）：现有字段保持；新增 `[apps.<appId>]` 段（expert / extensions / memory / working_dir）；插件作用域并入插件启停结构。
- 前端：纯前端外观项统一 `boenmind.appearance.*` 前缀（lib/appearance.ts 载体）；`boenmind.settingsTier` 分级开关；dockview 快照 `boenmind.dock.v8.<appId>` 不动。
- 专家预设：`~/.boenmind/agents/*.md`（AgentDefinition + 扩展字段 extensions/memory）。
- Tauri：维持现状（窗口外观仍静态，本期不做运行时窗口设置）。

## 十二、阶段划分与状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| 1 | 桌面形态退役（删代码、留开关、简化外观页） | ✅ 2026-08-16：desktop 8 文件删 7 留 StatusBar→shared；store 删 wallpaper/窗口状态；外观页留切换开关（点桌面仅 toast）；i18n/壁纸 CSS 清理；tsc+build 通过 |
| 2 | 扩展统一设置模式（SKILL settings.json + MCP env 表单；插件保持） | ⬜ |
| 3 | 作用域（manifest scopes + 引擎按 session.app 过滤 + 前端徽标/编辑） | ⬜ |
| 4 | 专家预设页 + 编程三专家预置 + per-app 设置页（[apps] 段 + AppEntry.settingsComponent + 双入口） | ⬜ |
| 5 | 普通/资深分级机制 + 高级项收尾 | ⬜ |
| 6 | 可选：设置搜索、外观配置导入导出、主题插件化 | ⬜ |
