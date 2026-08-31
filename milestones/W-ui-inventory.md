# W 序列 UI 库存盘点:assistant-ui 官方可直接利用资产(2026-09-01 摸爬)

- 盘点对象:assistant-ui 官方仓库(main,本地克隆)+ showcase 网站
- 结论:**W2/W3 需要的组件面几乎全部有现成成品,工作量 ≈ 选装 + 接线**,
  不是从零写。三层库存,全部 MIT 同源可自由改。

## 1. 三层库存

| 层 | 数量 | 形态 | 获取 |
|---|---|---|---|
| **组件注册表**(shadcn 式一键安装,带样式) | **100+ 组件** | `npx shadcn add r.assistant-ui.com/<名>` 即装即用 | registry(仓库 apps/registry) |
| **完整示例工程**(每个都是可跑的 Next.js 应用) | **42 个** | 源码级参考/整包搬 | 仓库 examples/(已克隆 .tools/eval) |
| 生产案例墙(showcase) | 9 个上线产品 | 视觉参考 | www.assistant-ui.com/showcase |

## 2. 注册表组件 × W2/W3 需求映射(直接命中)

### W2 命中(设置中心 + 工作区 + 布局)
| 我们的组件组 | 注册表现成件 |
|---|---|
| 模型选择 | **Model Picker**、Reasoning effort(力度选择,蓝本输入框同款) |
| 会话列表 | **Thread List**、Thread List Sidebar、Thread Search |
| 待办面板 | **Todo list** |
| 目录树 | **File tree** |
| 产物面板 | **Artifact card** |
| MCP 管理 | **MCP Config Dialog** |
| 设置中心 | **Settings**(注册表现成设置面) |
| 三栏布局 | Chat panel + Server panel(面板组合,自加拖宽) |

### W3 命中(主题 + 独家面板)
| 我们的组件组 | 注册表现成件 |
|---|---|
| 审批卡片 | **Approval card**、Permission grant |
| 任务/计划展示 | Agent plan、Agent status |
| 记忆抽屉 | **Memory** |
| 主题 | 无(令牌层自有,W3 规格 §2 方案不变) |

### 高价值储备(后续 W4+,均现成)
Terminal block、Code diff、Reviewable diff(工具执行展示)、Agent plan/
Subagent list/Handoff/Background runs/Checkpoints/Schedule(Agent 编排)、
Web preview、Canvas、Computer use、Code runner、Voice conversation、
Chart/Trace waterfall/Diagram/Flow graph、Message queue、Attachments、
Feedback dialog、Quote reply、Edit a sent message、Search in conversation、
Prompt library、Command palette、Shared conversation、Onboarding、
Mobile composer、Markdown/Shiki 高亮/Mermaid 图。

## 3. 示例工程重点(42 个,与需求相关者)

- **with-opencode**:⚠️ 高价值——用户蓝本(opencode web)的官方组件包:
  权限卡片/提问卡片/工具组/bash+patch 工具 UI/推理幽灵显示,W3 审批面
  可整包参考
- with-external-store:我们的运行时接法(W1 已用)
- with-custom-thread-list / with-virtualized-thread:W2 会话列表(注意
  前者有已知输入框禁用坑,见记忆)
- with-artifacts:产物面板交互参考
- with-resumable-stream:断线续传(流式稳定性储备)
- with-mcp:MCP 接入参考

## 4. 结论与影响

- W2/W3 组件实现方式从「手写」改为「**注册表选装 + BoenMind 令牌覆盖**」,
  预计 W2 前端工作量显著下降;手写仅剩:布局拖宽、三栏编排、接线层
- 主题系统(W3)不受影响:注册表组件全部走 CSS 令牌,天然随主题换肤
- 风险登记:注册表组件带 shadcn/tailwind 风格假设,需与 W1 手写令牌
  共存策略(选装时同步生成 tailwind 配置,或改写 className 到自有令牌)——
  W2 开工首日定案
