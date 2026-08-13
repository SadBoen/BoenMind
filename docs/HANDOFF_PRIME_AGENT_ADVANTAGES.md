# HANDOFF: 吸收 Prime Agent 三个优点（refine 建议 / 断线续跑 / subagent 结构化返回）

> 2026-08-13 · 会话交接 · ✅ **已完成并 commit/push**（6 commits：`2e9bdb8` → `2012300`）
> 决策过程：对比 Prime Agent（PrimeIntellect 2026-08-06 开源，建在 pi 之上）后用户拍板吸收三个优点，
> 同时坚持**宿主审批模式**（避免上游 Factorio 演示"代理自改手册优化作弊"的翻车路径）。

> **2026-08-13 追加（e29c7eb）**：遗留待办 #1「静默升级」已完成并实测——
> 用户澄清需求 = **热升级（点一下升级，不重装、不退出程序）**，非 Tauri 安装包更新。
> 详见 `HANDOFF_HOT_UPDATE.md`（实施纪要）。

## 一、任务目标（用户需求）

1. 上游改动统一台账（单文件、可复现，升级时可重放）
2. 吸收 Prime Agent 三个优点：
   - ① refine 式自我改进 → **代理提建议、用户审批后才生效**（宿主审批模式）
   - ② daemon 长挂机 → 任务状态与会话解耦、**断线续跑**、心跳进度
   - ③ 子代理结构化返回 → 队长像读函数返回值一样取用结果
3. 静默升级软件（**未做**，用户明确本轮优先前两项，见"遗留待办"）

## 〇、关键测试结论（2026-08-13 浏览器实测全链路）

- **① 建议链路**：代理调用 `submit_refinement_suggestions` → bm-server 在工具事件流截获入库（pending）
  → 设置页「改进建议」批准 → SKILL.md frontmatter description 更新（`SKILL.md.bak-<ts>` 备份、
  已启用时同步 pi 目录）→ 还原 → 全部恢复 ✅
- **② 断线续跑**：发送多工具长任务 → 心跳条显示"正在执行工具 bash：…" → 刷新页面（SSE 断开）
  → 任务继续跑完（`/tmp/task_test_summary.md` 真实写入）→ 重进会话状态条显示"任务已完成" ✅
- **③ 结构化返回**：队长派工 default 角色 → 工具结果正文开头出现 `<subagent-structured-result>` 块
  → 队长直接引用 `status=completed / exitCode=0` 汇报，无需读正文摘要 ✅
- **P10 必现 Bug**（实测抓到）：多个 entrypoint=index.ts 的目录型插件共存于扩展根时，上游
  `discover_sibling_index_entries` bundle 探测互相认领 → 会话创建失败 "Ambiguous JS extension
  ownership"。已修（见补丁 P10）。
- **集成坑**（实测抓到）：ctx-compactor 修剪会把 subagent 输出占位符化，砍掉块尾的 status/exitCode
  → 修复：subagent 加入 SELF_TOOLS 不修剪白名单 + P9 块移到正文开头。
- **环境注意**：改 ctx-compactor 等插件源后必须同步 `~/.boenmind/extensions/` 副本；新插件/新配置
  需要**新会话**才加载（agent 句柄缓存）。

## 二、已完成

### 1. 上游补丁台账（`backend/legacy/UPSTREAM_PATCHES.md`）
- 审计结论：对上游全部改动 = **6 文件 8 处**（对比上游基线 `44ddf80` 权威 diff）
- 含基线信息、逐条复现命令、升级流程（上游合入即删对应补丁）、关联 issue
- 本次新增两处补丁并补齐 P1/P2/P7/P8 的统一 `BoenMind 补丁` 源码标记

### 2. 补丁 P9：subagent 结构化返回（`vendor/pi_agent_rust/src/subagents.rs`）
- 工具结果 content **开头**追加 `<subagent-structured-result>` 紧凑 JSON 块（output/stderr 截断
  2000 字符/字段、总块 16KB）；上游 `details` 在 providers/openai.rs 序列化时被丢弃，模型此前
  只能读 markdown 渲染
- 上游 issue [#163](https://github.com/Dicklesworthstone/pi_agent_rust/issues/163)（默认关闭配置项形式建议，预期自持）

### 3. 补丁 P10：extensions.rs bundle 探测误伤（`vendor/pi_agent_rust/src/extensions.rs`）
- `discover_sibling_index_entries` 加"cluster_root 名为 extensions 时跳过"保护（与
  `discover_sibling_extension_entries` 一致）
- 上游 issue [#164](https://github.com/Dicklesworthstone/pi_agent_rust/issues/164)

### 4. 优点①：refine 建议-审批（宿主审批模式）
- 新插件 `backend/plugins/refine-suggest/`（记录桩工具 `submit_refinement_suggestions`，
  预装 + 默认启用）
- bm-server 在工具调用结束事件流截获参数 → `refinement_suggestions` 表（pending）
- 审批 API：list / approve / reject / rollback；生效逻辑在 `bm-core/src/refine.rs`
  （改 SKILL.md 描述带备份 / 追加 `config.custom_system_prompt`，均带校验与回滚）
- 前端设置页「改进建议」tab（状态过滤 + 原文→建议 diff + 批准/拒绝/还原）+ i18n 四语

### 5. 优点②：断线续跑 + 任务状态（daemon 化第一步）
- 去掉"客户端断开即 abort"（断连只停 SSE 推送，任务继续）
- `tasks` 表：每 prompt 回合一条（status/progress/started_at/finished_at/error）
- 心跳：事件回调更新内存进度（工具名+关键短参/回复尾部）→ 5s interval 刷库 + `taskProgress` SSE
- 终态：正常→completed；`Error::Aborted`（停止/15min 超时）→cancelled；其余→failed
- API `GET /api/sessions/{id}/tasks`；前端 `TaskStatusBar`（活跃心跳/断线恢复 running/终态徽章）

### 6. 优点③：配套（提示词 + 契约，已随 P9 一起）
- 队长系统提示词：派工声明 JSON 契约 + 结果以结构化块为准
- `agents/default.md` 输出契约（未指定格式时按 summary/findings/done/open JSON 交付）
- ctx-compactor `SELF_TOOLS` 加 subagent（不修剪）；`docs/expert-team.md` 3.5 小节

## 三、文件清单

| 文件 | 说明 |
|---|---|
| backend/legacy/UPSTREAM_PATCHES.md | **新**：上游补丁统一台账（唯一权威记录） |
| backend/legacy/pi_agent_rust/src/subagents.rs | 补丁 P9：结构化块 |
| backend/legacy/pi_agent_rust/src/extensions.rs | 补丁 P10：bundle 探测保护 |
| backend/legacy/pi_agent_rust/src/{auth,openai,tools}.rs | P1/P2/P7/P8 补统一标记（纯注释） |
| backend/plugins/refine-suggest/ | **新**：建议采集插件（extension.json/index.ts/README） |
| backend/plugins/ctx-compactor/index.ts | SELF_TOOLS 加 subagent |
| backend/crates/bm-core/src/refine.rs | **新**：审批生效/回滚逻辑 |
| backend/crates/bm-core/src/{db,agent,config,plugins,lib}.rs | tasks/refinement 表、SYSTEM_PROMPT、custom_system_prompt、预装清单 |
| backend/crates/bm-server/src/routes/refine.rs | **新**：建议 API 四端点 |
| backend/crates/bm-server/src/{chat,lib}.rs + routes/{sessions,mod}.rs | 截获入库、任务生命周期/心跳、路由 |
| frontend/src/components/settings/RefinementSettings.tsx | **新**：改进建议设置页 |
| frontend/src/components/chat/TaskStatusBar.tsx | **新**：任务状态条 |
| frontend/src/{api/client,stores/app-store,lib/navigation}.tsx 等 | 类型/API/事件/注册 |
| frontend/src/i18n/locales/*.ts | 四语文案 |
| docs/expert-team.md | 3.5 结构化返回小节 |

## 四、遗留待办（下次会话）

1. **静默升级**（用户第三项需求，未做）：自动检查/下载/安装新版本。现有资产：
   `tauri-update.key(.pub/.password)` 签名机制、v0.1.1 发布管线（打 tag 即发布）、
   前端 About 页有"版本与更新"占位。需调研：桌面壳（Tauri）与 bm-server 自更新如何分工、
   检查频率/静默范围拍板。
2. **confirm 弹窗阻塞**：改进建议页的 `window.confirm` 会让页面 JS 阻塞（浏览器自动化实测
   点击卡 30s；人类操作正常）。若要现代化可换应用内确认层（排期可选）。
3. **上游跟进**：P9（#163）/P10（#164）若上游合入，按台账升级流程删除对应补丁。
4. **子代理 `--skill`**：阶段 2 才启用（subagent_child.rs 注释指向 docs/expert-team.md）。
5. **测试残留**：`gui-test-screenshots/`（5 张实测截图）与 `/tmp/task_test_summary.md`、
   工作区 `.tmp/` 测试文件可清理。
