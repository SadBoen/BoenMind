# 长程测试报告（2026-08-15 夜）

> 用 BoenMind 编程功能做长程项目（Web 游戏"Space Sentry"），浏览器仿真驱动 + API/事件日志双面监控。
> 结论先行：**核心设计成立**（todo 自动调节闭环、单会话长任务、事件日志审计面、步数预算、压缩触发），
> 实测挖出 **P1 压缩摘要污染 bug**（连续两次中断长任务）与 **skill 会话内不即时生效**（用户重点③不满足）。

## 一、测试设定

| 项 | 值 |
|---|---|
| 任务 | 单会话从零构建完整 Web 游戏（贪吃蛇式太空射击 Space Sentry）：多文件无构建、4 种敌人、道具、连击计分、最高分 localStorage、WebAudio 音效、暂停/重开、触控、粒子/星空/ HUD、README、分里程碑 git 提交 |
| 会话 | app="coding"，MiniMax-M3，默认 thinking；UI 内新建项目 BoenGame（D:\96_CoderWorld\BoenGame，git init）+ UI 新建会话 + UI 聊天框发送（浏览器仿真） |
| 环境 | working_dir 临时指向 BoenGame（已恢复）；vite dev 5173 + bm-server 17321（新 exe 含 usage API） |
| 会话纪律 | 只发自然用户消息（任务/新需求/续接），**不发"继续"式纯续接指令**；回合被砍即记录 |
| 监控 | 事件日志（tool/todo/压缩事件全链）+ /todos + /usage + git + 前端面板实时投影 |

## 二、结果总览

- **7 个回合 / 201 条 assistant 消息 / 工具调用 132+ / 累计输入 9,794,461 tokens（输出 82,724）**
- 回合 1（84 步）：15 项 todo 全完成，5 次提交（脚手架→引擎+实体→音频+关卡→主流程+README→fix），turn reason=completed
- 回合 2-4：skill 探针 + 重建对照（见 §四）
- 回合 5-6：boss 特性被压缩污染中断两次（见 §三 P1）
- 回合 7（78 步）：完成 boss 特性（提交 5e5d2c6），todo 23 项全 ✓
- 产物验证：`node --check` 全过；浏览器跑通 开始→游玩（敌机开火扣命）→GAME OVER→重开；boss 集成代码复核（levels.js 每 5 波 boss 波 + main.js bossHp HUD + entities.js Boss/BossMinion 类）

## 三、设计符合性核对

| 设计点 | 结果 | 证据 |
|---|---|---|
| todo 工具自动调节（用户重点①） | ✅ 完全成立 | 回合 1 建 15 项清单，回合 5 增 8 项（总共 todo 调用 40+9+3，todo/write 快照事件 54+）；面板实时投影：✓/○ 状态、优先级、步数计数器"本回合已执行 N 步"（按回合重置）；REST /todos 与工具同事件链 |
| 长任务单会话完成（用户重点②） | ✅ 回合级成立 / ⚠️ 受压缩 bug 威胁 | 回合 1 单回合完成全量任务（84 步 < 128 预算，未触发预算提示）；多轮续接同会话推进无新开会话；**但**回合 5/6 被压缩摘要污染中断（见 P1），回合 7 靠用户显式"不要总结"才扛过 |
| skill 会话内即时生效（用户重点③） | ❌ 不满足 | 见 §四 |
| 事件日志审计面 | ✅ | tool/call 与 tool/result 一一对应（132/132）；request/header 每回合一枚；usage 聚合正确（状态栏 3.6K→4.9M 实时）；EventQuery 类型过滤（todo/write 专用查询）工作 |
| 步数预算 | ✅ | 128 上限；回合 1 自然完成未触顶；剩 6 步预算提示注入点存在（本测未触发）；todo 面板步数计数实时 |
| 压缩水线 | ⚠️ 触发正确、摘要质量 P1 bug | 5 次压缩事务（start/summary/end 三事件 + Replace 遮蔽），跨回合连续对话无丢失；但摘要内容含模型 <think> 思维前缀 → 见 P1 |
| 工具面（M1 修复项） | ✅ | Windows 提示段有效（全程无路径折腾）；read/grep 未出现超时或 gitignore 越界（grep 15+ 次正常）；agent 用 bash(41)+write(13)+grep(6)+edit(5)+read(4) 完成回合 1 |
| 项目切换 | ✅ | 前端项目集合（新建 BoenGame 项目→文件树跟随）；API root 参数化（git-info?root= 正常） |
| 分支图 | ⚠️ 小问题 | 一度显示"工作目录不是 git 仓库"（API 实际 repo:true）；空态无刷新按钮无法自愈，重载页面恢复；git-info 数据（DAG 拓扑/分支 tip）API 面正确 |
| 状态栏 | ✅ | token 用量（/api/sessions/{id}/usage 聚合）、模型名、工作目录、后端版本全显示 |
| 模型路径幻觉 | ✅ 未复现 | working_dir 锚定 + 本轮未出现 M1 式路径漂移（长上下文下仍稳定） |

## 四、用户重点③实测（skill 会话内即时生效）——不满足

1. 会话进行中安装并启用 game-audit skill（API 安装 + enable）
2. 同会话要求"使用 game-audit skill 审计" → agent 的系统提示（构建时注入 `<available_skills>`）**不含该 skill**；模型靠磁盘探索（dir/ls ~/.boenmind/agents/skills → 读 SKILL.md）**偶然**可用——非设计内生效，若路径猜不中即静默失效
3. 对照实验：同会话切 thinking 档位（触发 agent 重建）→ 再问 → `game-audit` 出现在清单（dtctl/game-audit/web-scraping），模型承认先前答错
4. 机制结论：skill/插件工具面在 **agent 构建时固化**（system prompt 拼 enabled_skills；compat 工具快照服务启动固化），agent 按会话缓存、仅 provider/model/thinking 变化时重建 → **"当前对话就能用"不成立，必须重建或新会话**。MCP 同理（插件 registerMcpServer，插件变更不达运行中 agent）
5. 修复方向（供拍板）：① skill 启用/安装后主动失效当前会话 agent（重建零损失，事件日志唯一状态源已保证）② available_skills 改运行时动态注入（on_request 改写挂点已存在）③ 或前端明示"新 skill 下次生效"

## 五、发现的问题

### P1（高）压缩摘要污染 → 中断长任务（用户重点②的直接威胁）
- **现象**：回合 5、6 的最终输出变成"总结对话历史"文本，boss 实现中断（回合 5 在 entities.js 写完后、回合 6 在 levels.js 未动时）。回合 7 用户显式"不要总结"才完成
- **根因**（compact.rs summarize + bm-compactor）：摘要 LLM 响应（MiniMax 思考模型）以 `<think>用户要求总结对话历史…</think>` 开头，`summarize()` 原样取 MessageEnd.content，未剥离 think 段；CompactionSummary 事件（Replace 后以 assistant 消息入投影）内容 = "用户要求总结…" → 主循环下一步把摘要当任务继续"总结模式"
- **影响**：长会话压缩必触发 → 长任务多轮推进被反复打断；回合 5 压缩 #4 后立刻翻车、回合 6 无新压缩也翻车（旧污染摘要留在投影）
- **修复建议**：① summarize() 剥离 `<think>…</think>`（或取最后一段完整内容）② 摘要请求显式关推理（reasoning_effort=off / temperature 参数化）③ 摘要入投影时标记角色/前缀（如 system 角色 + "【历史压缩摘要】"），防模型误当任务指令

### P2（中）skill/插件会话内不即时生效（见 §四，用户重点③）

### P3（低）分支图空态无刷新按钮
- 空态（repo:false 或请求失败）无刷新入口，必须切项目/重载才自愈；建议空态也渲染刷新按钮

### P4（低）重载后 store 重置（已知）
- IAB 重载页面 → activeNav 回聊天视图（zustand store 重载丢失，与既有记忆一致）

### P5（观察）回合 6 有两条 read 调用失败（api_error，模型自述）
- 事件日志中未见对应 tool/result 异常（可能为超时/路径问题），未复现，挂起观察

## 六、成本真相（长程真相）

- 完整游戏 ≈ 1 回合 84 步 / 109 工具调用 / 输入 357 万 tokens；加 boss 特性 ≈ 7 回合 / 978 万 tokens（含压缩重算、两次污染浪费 ~2 回合）
- 压缩让单步输入受控（5 次触发后继续推进，无超窗失败），但每次压缩重投影 + 污染浪费 ~15-20% 预算
- 观察：回合 1 输入 357 万 ≈ 84 步 × ~42K/步（峰值未达 64K 水线）；回合 5-7 步均输入显著上升（历史 + 工具结果累积），压缩每 ~15 步触发一次

## 七、结论

**设计主线全部验证成立**：todo 事件投影闭环（用户痛点①解决）、单会话长任务（用户重点②回合级成立）、事件日志审计面、步数预算、压缩触发与事务、项目切换、状态栏。
**两个真实缺口**：P1 压缩摘要污染（MiniMax think 前缀未剥离）会实质中断长任务——这是"长任务不用新开多轮对话"的头号敌人，建议尽快修（改动小、有现成测试位）；P2 skill/插件会话内不即时生效（用户重点③），修复方案已有方向待拍板。

## 八、修复状态（2026-08-15 修复轮，51459ae / 169eb29 / b608c1d，已推送）

- **P1 压缩摘要污染**：`compact.rs` 新增 `strip_think_blocks`（剥离 `<think>…</think>`，未闭合丢弃余下），+3 单测；bm-loop 19+2+15 测试全绿。遗留：旧污染摘要仍在事件日志投影（新压缩不再产生）。
- **P2 skill/插件会话内即时生效**：skills 增删启停 → `invalidate_loop_agents`（下一消息重建，零损失）；插件启用/安装 → `CompatEngine::reload` 增量加载 + 工具快照刷新（tools/tool_names 改 Mutex 防竞态）；禁用/卸载无运行时卸载路径（保留至重启）。实测：同会话启用 game-audit 后立即可见。
- **P3 分支图空态刷新**：空态渲染刷新按钮（此前只能切项目/重载自愈）。
- **P4 重载现场恢复**：activeNav/activeSessionId localStorage 持久化；跨应用错配回退 appSessionIds[activeNav]（实测修复——首次实测暴露 coding 视图挂 chat 会话导致 todo/文件订阅错位）。
- **文件面板自动刷新**：订阅活跃会话事件，tool/result 防抖 600ms + turn/end 立即；实测 agent 写文件后树自动出现。
- **任务清单自动滚动**：清单更新后 scrollIntoView(block:nearest) 到首个未完成任务；实测 43 条清单滚至活动项。
- 验证环境：bm-loop 测试全绿、前端 tsc -b 通过、浏览器实测 5 项（P2/P4/文件刷新/清单滚动/事件流恢复）。
