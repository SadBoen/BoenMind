# docs —— 文档地图（2026-08-16 整理轮）

> 归档原则：已完成/已解决/被覆盖的 → `docs/archive/`（保留决策轨迹与验收证据）；活文档留在根目录。本索引列出全部文档与各自角色。

## 活文档（根目录）

| 文档 | 角色 | 何时读 |
|---|---|---|
| **everything-is-plugin-architecture.md** | 架构设计（v0.24）：三铁律/内核/借鉴清单/插件化设计/渐进路线/歧路注记 | 架构决策、设计参照 |
| **HANDOFF_KERNEL_PHASE1.md** | 阶段 1 活交接（每轮必读）：当前状态/轮次脉络/下一步/坑/待拍板 | 每轮开工前 |
| **boenmind-strategic-review.md** | 战略层回看：命名哲学/三护城河/五年路径/100 小弟时间哲学 | 战略决策 |
| **REVIEW_LANDSCAPE_2026-08-15.md** | 全网对标调研报告（底座 Top10 + 吸收清单 21+16 条） | 吸收项执行时（待拍板 3 引用） |
| **REVIEW_TOOLS_CROSS_2026-08-16.md** | 三工具交叉审查（code-architecture/codebase-reviewer/ln-24 各独立全库审查 + 交叉校验；P0-P3 修复清单待拍板） | 本轮回头看结论、修复排期 |
| **research/** | 架构方向调研素材（2026-08-15 四份：agent-foundations/memory-systems/plugin-landscape/desktop-shell-landscape） | 阶段 4/5 设计时 |

## 归档（docs/archive/，见 archive/README.md 逐条状态）

- 已完成：kernel-implementation-plan（阶段 0）、SERVICE_FACES（13 面注册）、ACCEPTANCE_M1（M1 验收）、HANDOFF_DESKTOP_SHELL（桌面壳）、HANDOFF_KERNEL_PHASE1_ARCHIVE（阶段 1 历史）
- 已解决：REVIEW_ARCHITECTURE（内核未接线→已解决）、REVIEW_CODE（P0/P1 已修）、REVIEW_LONG_RUN（长程测试 P1-P4 已修）
- 已拍板：REVIEW_BEFORE_CODING_APP（7 拍板点；含被推翻的 M2 形态建议）

## 其他文档

- 根目录 README.md：产品使用手册（启动/配置/发布/部署）
- frontend/README.md：前端开发说明
- review-tools-2026-08-16/：三工具交叉审查的三份独立报告（工具A code-architecture 29 条 / 工具B codebase-reviewer 26 条 / 工具C ln-24 14 条；结论见 REVIEW_TOOLS_CROSS_2026-08-16.md）

## 决策轨迹速查

| 想查什么 | 看哪里 |
|---|---|
| 为什么不换 dsh 内核 | 架构 §15.4 |
| 为什么恢复经典三栏（桌面壳歧路） | 架构 §四·B 注 + archive/HANDOFF_DESKTOP_SHELL.md |
| 13 服务面怎么铺开的 | archive/SERVICE_FACES_2026-08-15.md |
| M1 验收证据 | archive/ACCEPTANCE_M1_2026-08-15.md |
| 编程应用 7 拍板点 | archive/REVIEW_BEFORE_CODING_APP.md + 活交接待拍板 1 |
| 调研吸收清单 | REVIEW_LANDSCAPE_2026-08-15.md §六 |
