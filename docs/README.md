# docs —— 文档地图（2026-08-17 校准）

> 归档原则：已完成/已解决/被覆盖的 → `docs/archive/`（保留决策轨迹与验收证据）；活文档留在根目录。本索引列出全部文档与各自角色。
>
> **文档新鲜度（对照代码）**：主文档 / 登记表 / HANDOFF 当前状态段 = **2026-08-17**。架构文件交叉审查 = `REVIEW_ARCH_CROSS_2026-08-17.md`。
>
> **权威顺序**：`design/DSH_PROJECT_V2_2026-08-17.md`（新方向宪法）> `everything-is-plugin-architecture.md`（历史语义）> `EXTENSION_POINTS_REGISTRY.md` > `HANDOFF_KERNEL_PHASE1.md` > `docs/design/*`。

## 活文档（根目录）

| 文档 | 角色 | 何时读 |
|---|---|---|
| **design/DSH_PROJECT_V2_2026-08-17.md** | **BoenMind 新方向宪法（v2.1 定稿）**：Rust 微内核自研后端 + 前端借 dsh 生态 + web-server 协议兼容层 + 插件/APP 全 Rust | **所有新开发，开工前必读** |
| **HANDOFF_DSH_V21_2026-08-17.md** | **dsh 内核 v2.1 迭代指针**：现状/下一轮 M1 动手清单/铁律/坑/材料索引 | **下一轮开工前必读** |
| **everything-is-plugin-architecture.md** | 架构宪法（v0.26，pi 内核时代）：三铁律/内核/插件化设计——语义层仍有效，dsh 方向由其 v2 计划接管 | 历史语义/三铁律参照 |
| **EXTENSION_POINTS_REGISTRY.md** | 扩展点唯一计数器（协议 14 面 + mcp / 12 挂点 / 产品扩展点） | 每轮新增/接线扩展点时 |
| **HANDOFF_KERNEL_PHASE1.md** | 迭代指针：当前状态表 / 下一步 / 待拍板（后文日记以上表为准） | 每轮开工前 |
| **design/WIKI_REWRITE_ARCHITECTURE_2026-08-17.md** | WIKI APP 重写架构（用户 7 条 + 三席合成） | 重写 WIKI / 摄取 / 图谱 / wiki.db 时 |
| **design/SETTINGS_ARCHITECTURE_2026-08-16.md** | 设置中心拍板 + 阶段 1–5 已落地 | 改设置/作用域/`[apps]` 时 |
| **PLAN_MCP_PLUGIN_2026-08-16.md** | MCP 官方插件计划（0–3+4b 完成，4c OAuth 后置） | 改 MCP / 连 server 时 |
| **boenmind-strategic-review.md** | 战略层回看：命名哲学/三护城河/五年路径 | 战略决策 |
| **REVIEW_LANDSCAPE_2026-08-15.md** | 全网对标调研（吸收清单） | 吸收项执行时 |
| **REVIEW_TOOLS_CROSS_2026-08-16.md** | 代码三工具交叉（接线/权限门） | 上轮代码基线 |
| **REVIEW_TOOLS_CROSS_2026-08-17.md** | 代码三工具交叉（配置三持有已修） | 配置/权限/workspace 边界 |
| **REVIEW_ARCH_CROSS_2026-08-17.md** | **架构文件**三工具交叉（本文档整理轮） | 要不要大手术、文档漂移 |
| **REVIEW_FRONTEND_CROSS_2026-08-16.md** | 前端专项交叉 | 前端问题排期 |
| **research/** | 架构方向调研素材 | 阶段 4/5 设计时 |

## 归档（docs/archive/，见 archive/README.md 逐条状态）

- 已完成：kernel-implementation-plan（阶段 0）、SERVICE_FACES（**当时 13 面**，其后 +provider/+mcp，活权威=登记表）、ACCEPTANCE_M1、HANDOFF_DESKTOP_SHELL（桌面壳歧路）、HANDOFF_KERNEL_PHASE1_ARCHIVE、HANDOFF_LLM_PROVIDER_PLUGIN
- 已解决：REVIEW_ARCHITECTURE（内核未接线→其后已铺面）、REVIEW_CODE、REVIEW_LONG_RUN
- 已拍板：REVIEW_BEFORE_CODING_APP（7 拍板点）
- 被 v2 覆盖：DSH_PROJECT_V1_2026-08-17_FAMILYBUCKET（dsh 全家桶真身 → v2 微内核自研）

## 其他文档

- 根目录 README.md：产品使用手册
- frontend/README.md：前端开发说明
- review-tools-2026-08-16/、review-tools-2026-08-17/：代码轮独立报告
- review-frontend-2026-08-16/：前端轮独立报告
- review-arch-2026-08-17/：架构文件轮独立报告（A/B/C）

## 决策轨迹速查

| 想查什么 | 看哪里 |
|---|---|
| 为什么不换 dsh 内核 | 架构 §15.4 |
| 为什么恢复经典三栏、桌面为何退役 | 架构 §四·B + archive/HANDOFF_DESKTOP_SHELL.md + 设置文阶段 1 |
| 服务面怎么数 | **登记表**（不要数 SERVICE_FACES 图纸） |
| M1 验收证据 | archive/ACCEPTANCE_M1_2026-08-15.md |
| 编程应用 7 拍板点 | archive/REVIEW_BEFORE_CODING_APP.md + HANDOFF 待拍板 1 |
| 调研吸收清单 | REVIEW_LANDSCAPE_2026-08-15.md §六 |
| 要不要动架构大手术 | REVIEW_ARCH_CROSS_2026-08-17.md |
