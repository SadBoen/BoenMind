# BoenMind 交接文档（2026-08-21 前端重写批）

给下一轮对话/协作的接手者。**本轮任务：废弃当前前端，从零重写前端**。交接内容 = 做什么 + 唯一参考（`docs/frontend-redesign/DESIGN.md`）+ 怎么用已装的 design skills。

---

## 一、仓库一句话（本轮只需知道的）

BoenMind = Rust 微内核 agent 平台。后端（Rust web-server，默认 `127.0.0.1:3080`）**同时服务静态前端（`frontend/dist`）与全部 API**。前端当前是 React 19 + antd v6 + dockview 的 SPA（在 `frontend/src/`），**本轮整体废弃重写**。前端重写**不依赖后端改动**——后端契约（`POST /api/<method>` RPC + WebSocket mux 帧）已冻结，照 `DESIGN.md` 对接即可。

分层：`kernel/`（submodule 只读）→ `bm/ports`（契约）→ `plugins/` → `bm/assembly`（组合根）→ `bm/web-server|headless`（L0）→ `frontend/`（本轮重写对象）。

---

## 二、要做的事（本轮主线）

### 核心：从头重写前端，做到与当前前端功能等价（并预留扩展）

- **废弃** `frontend/src/` 当前全部实现（React 19 + antd + dockview 那套），随你选技术栈、布局方案，**只遵守 `DESIGN.md` §1.1 硬约束**（后端契约相关）。
- **窗口覆盖**（全要，见 DESIGN.md §4）：登录、会话列表、聊天（消息流/流式/工具卡/模型选择/目标卡片）、文件管理器（树/预览/编辑/上传/新建文件夹/右键）、设置（五分区 + 三档主题系统）、全局状态栏 + 审批弹窗。**编程/CodingApp 本轮不做**（§4.6）。
- **后端契约不得改**：RPC 信封、WS 帧、goal CAS、workdir 事实源、auth 双态——全部按 DESIGN.md §2/§3。
- **主题**：仅黑白/卡通/玻璃**三档**（明暗维度已删，勿引入新档），背景维度（默认/渐变/图片）与三档正交。见 DESIGN.md §4.4.1。
- **推理**：技术栈、布局自由；桌面壳 Tauri 能力浏览器下必须优雅降级。

### 参考（必读，唯一事实源）

1. **`docs/frontend-redesign/DESIGN.md`**（381 行）——前端重写设计文档：总体约束/数据契约/页面交互规格/状态管理/已知坑/验收清单。**实现前端的第一手依据**。
2. **后端契约源码**（字段/语义如文档未尽，直接查）：`bm/web-server/src/` 的 `rpc.rs`（信封）、`ws.rs`（帧）、`api/`（各 RPC handler）、`approval.rs`。
3. 三档主题设计稿（背景灵感）：`docs/themes/{heibai,karton,boli}/DESIGN.md`（黑白/卡通/玻璃三档）。

### 验收门禁（交付前必须过）

见 DESIGN.md §7 验证清单。额外：`cargo build --workspace && cargo test --workspace -- --test-threads=1` 后端不改也应全绿（接口必须按 §2/§3 对齐）；`frontend` 侧 `tsc --noEmit` + `vite build` 通过。**看界面必须识图**（视觉核对，不只靠 DOM 文字猜）。

---

## 三、怎么用这波已装的 design skills（ZCode 已装 18 个）

装到了 `~/.zcode/skills/`（ZCode 用户级 skill 目录，frontmatter 已校验）。做前端时按需激活（自然语言触发即可，也可用 `/` 命令 `Skill` 调）：

### 设计向（本轮主力）
| skill | 装了什么 | 什么时候用 |
|---|---|---|
| **ui-ux-pro-max** | 整套 7 个：`ui-ux-pro-max`（UI/UX 智能体，79 样式/192 调色板/119 UX 指南/74 字体配/105 图标）、`design`（品牌/设计令牌/UI 风格/Logo/横幅/图标）、`design-system`（**三层 token 架构**：primitive→semantic→component + CSS 变量 + 间距/字体刻度）、`ui-styling`（shadcn/ui + Tailwind）、`slides`、`brand`、`banner-design` | 设计系统搭建、token 体系、组件规范、调色板/字距/UX 指南查询。**做 BoenMind 三档主题 + 组件库时首选** |
| **interface-design** | 单 skill | **看板/设置页/SaaS 工具/数据界面**——BoenMind 设置页、文件管理、审批弹窗这类产品界面定向 |
| **impeccable** | 单 skill | 设计评审/打磨/audit：交付前把 UI 过一遍，找 bland/anti-pattern |
| **frontend-design**（anthropics 官方） | 单 skill（理念散文） | 做视觉方向/避免模板感：给每个界面定独特审美，不做反面教材 |

### 工程向（过程方法论，Superpowers 核心子集）
| skill | 用途 |
|---|---|
| **test-driven-development** | 按 TDD 写前端测试 |
| **systematic-debugging** | 系统性排查（不瞎试） |
| **executing-plans** | 计划执行（`writing-plans` 未装——有 DESIGN.md 即计划，不需要另装） |
| **requesting-code-review / receiving-code-review** | 交互相审 |
| **verification-before-completion** | 完成前验证（对应"验收门禁"） |
| **dispatching-parallel-agents** | 拆并行子任务（如重写时并行审模块） |
| **brainstorming** | 设计方向头脑风暴 |

> 注：superpowers 装的是**核心 8 个**（没装 `writing-skills`/`using-superpowers` 等元技能，也没装 `writing-plans`——计划由 DESIGN.md 充当）。

### 触发方式
- 你说"用 ui-ux-pro-max 设计系统"、"按 impeccable 评审"等，我（下轮对话的 ZCode）会匹配 skill 并加载其说明执行。装好的技能不占用当前上下文，触发才注入。

---

## 四、本轮边界 / 已知坑（对照 DESIGN.md §6）

- **RPC 信封 method 必须逐字**匹配 path；审批应答 key = 外层 rpcId（不是 approvalId）。
- **projection seq 去重**：快照不带 seq，增量 `seq>水位` 才收。
- **session.list updatedAt 是假值**（恒 1970）——别用它排序。
- **session.prompt 异步非流式**：HTTP 立返 `{accepted}`，消息全靠 WS `session/event` 增量。
- **文件面**：workdir 未配置时文件窗口提示去设置；读文件失败转"仅下载"；上传 409/413 有文案。
- **Markdown**：文件预览必须 sanitize，聊天消息不做（现状）。
- **Tauri 能力**：浏览器下优雅降级（隐藏/空操作），不能崩。
- **本地持久化键**（沿用不换名，用户在数据在）：`bm_session_token/bm_recentSession/bm_autoRestore/bm_preset/bm_background/bm_accent/bm_fontsize/bm_glass_opacity/bm_approvalTrust/bm_seen_version`。

---

## 五、本轮状态（写交接时）

- ✅ DESIGN.md 已定稿（技术栈不限定/只描述窗口/三档/无 CodingApp）
- ✅ 5 个 skill 包共 18 个 skill 已装到 `~/.zcode/skills/` 并校验 frontmatter
- ⬜ 前端重写尚未开始（等下一轮对话执行）
- ⬜ DESIGN.md 本身未 commit（在工作区未跟踪），建议提交后再开始开发