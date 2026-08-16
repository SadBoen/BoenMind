# 工作交接：专家角色提示词撰写

> 2026-08-16 用户开题（设置中心设计意见第 3 条），交接给后续执行轮。
> 用户原话："角色提示词，你帮我写好，参考其他牛逼项目，我现在最满意的是 ZCode 的角色能力，同一个模型，在它里面跑，能力很强的，但它不开源。"

## 任务

为 4 个预置专家写出**高质量中文角色提示词**，目标是"同一个模型在 BoenMind 里跑出 ZCode 的效果"：

| 专家 id | 现状 | 问题 |
|---|---|---|
| `architect` | **正文为空**（只有 frontmatter） | 完全没提示词 |
| `coder` | 3 职责 + 3 准则（约 8 行） | 太简略，无输出格式/工作流约束 |
| `reviewer` | 3 职责 + 3 准则（约 9 行） | 太简略 |
| `default` | 3 职责 + 3 准则（约 8 行） | 太简略 |

文件位置：`~/.boenmind/agents/agents/*.md`（frontmatter + Markdown 正文；正文 = system_prompt 字段）。

## 参考源（按优先级）

1. **ZCode--CLI--agent（GitHub，MIT）**——社区自研的 ZCode 同思路实现，角色/系统提示词结构可扒；记忆 [[zcode-open-source-status]] 记录过
2. **zcode-open-bridge**——ZCode 桥接项目，可能含角色面机制
3. **Claude Code 的 CLAUDE.md 风格**——角色注入 + 工作纪律的写法
4. **hermes-agent**（NousResearch）——记忆 [[pi-agent-gap-analysis]] 对比过
5. 现有架构：`backend/crates/bm-core/src/agent.rs` 的 SYSTEM_PROMPT、`bm-server/src/roles.rs`（宿主注入挂点，记忆 [[factory-plugins-role-and-coding-memory]]）

## 要求

- 每份提示词包含：角色定位 → 职责 → 工作流程/纪律 → 输出格式约定 → 边界（不做的事）
- 面向"编程 APP 内被主代理派工"的场景（子代理模式），参考 `subagent_tool.rs` 的派工契约
- 中文撰写；写完实测：绑定专家跑一轮真实任务，对比默认行为
- 若发现 ZCode 系开源实现有系统级"思维链/工作区纪律"写法，注明来源与可吸收点

## 相关上下文

- 专家 = 模板（非实例），见架构文档 SETTINGS_ARCHITECTURE_2026-08-16.md §六"模板与实例"（2026-08-16 用户定调）
- 专家字段接线状态：**管理面完整，运行面未接线**（模型/记忆桶/扩展子集/绑定均只存不读）——本任务只管提示词内容，接线属另一轮
- 提示词自我进化/经验累积：待讨论议题，见记忆 [[expert-role-experience-evolution]]
