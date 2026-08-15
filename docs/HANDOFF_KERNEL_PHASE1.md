# HANDOFF —— 阶段 1 交接（精简版）

> 每轮必读此文件（约 4KB）。历史全量/查证细节 → docs/HANDOFF_KERNEL_PHASE1_ARCHIVE.md（按标题检索）。
> 本文件只保留：当前状态 / 轮次脉络 / 下一步 / 注意坑 / 待拍板。开工前先 `git pull`。

## 当前状态（2026-08-15）

**阶段 1 完成态**：主线 A（执行级事件日志 + 自研 bm-loop 引擎）+ 主线 B（pi-compat 插件兼容层）全部落地；默认引擎已反转 bm；Steward 三件套（调度器/inject 通道/前端状态页）真实验收通过；legacy 删空（§十三终点）。BoenMind = 可聊天/调工具/派子代理/有管家/有记忆的完整运行时。

**编程应用 M1 已验收通过（8254bd7，本日）**：BoenMind 用自身运行时（bm-loop + MiniMax-M3 + 内置工具，纯 API 驱动）在自身仓库完成真实 bug 修复全链路（prompt_hash 注入面缺口：定位→修复→回归测试→git 提交），独立复核 33 测试全绿 + clippy 零告警。**验收暴露 6 个真实问题**（报告 docs/ACCEPTANCE_M1_2026-08-15.md §四）：① 单回合 64 步预算不够真实任务（回合 1 被砍断需人工续接）② grep/find 无 ignore+无超时（全库 grep 卡 4 分钟）③ read 无行区间参数（步数燃烧主因）④ bash cmd /C 路径引号习惯冲突 ⑤ ctx-compactor 索引写服务进程 cwd ⑥ 模型路径幻觉——①②③ 随 M2 修（工具面是独立壳基座）。**下一步 = M2 独立壳应用起步**（用户拍板：独立壳而非现有前端加卡片）。

**最近四轮回溯**：
- 修复轮（同日，用户定调"回头看查出问题先修"）：三真缺口修两件（declare_event! 宏 / branch/fork 事件）、压缩参数双轨打通（bm-core effective()）、memory/write 生产者接线、**内核第一根接线**（bm-compactor 经 KernelBuilder 装配进生产，bm 引擎从 kernel 取事件日志+压缩服务）——测试全绿 + clippy 零 lint
- 回头看+对标轮（本轮）：架构回头看（内核未接线三轨实锤/文档漂移修正）+ 全网对标三调研（底座前 10/记忆/插件同类，笔记 docs/research/2026-08-15/ 约 100KB 全部标注核实口径）→ 报告 docs/REVIEW_ARCHITECTURE_2026-08-15.md + docs/REVIEW_LANDSCAPE_2026-08-15.md；架构文档 v0.21
- 代码回看轮（5c6451b）：三子代理并行审查全代码 → P0×3 修复（会话串行锁/压缩后 usage 重置/投影守卫）+ P1 一批（死代码/失败风暴/flusher 泄漏等）；报告 docs/REVIEW_CODE_2026-08-15.md；未修项（inbox 未接线/prompt_hash 契约/env 集中化）挂编程应用 M2
- 拍板轮（fa5019b）：pre-push 本地质量门（hooks/pre-push，GitHub 私有仓库无 Actions 免费额度）；pi 目录改名 `~/.boenmind/pi` → `~/.boenmind/agents`（启动自动迁移，真实验收过）；回头看立项材料 docs/REVIEW_BEFORE_CODING_APP.md（7 拍板点待拍）
- Steward 验收轮（059c9e6）：采集器全链路真实验收 + inject 锚点缺陷修复（note_round_done 推进 last_wake_at）

**⚠️ 当前唯一外部依赖**：GitHub 账户 Billing 未处理 → workflow 全瘫（含自托管 job，账户层拒绝调度）。质量门已由 pre-push 钩子本地接管，日常 push 不受影响；macOS 构建链（仅打 tag 触发）受影响，发版时再议。

## 轮次脉络（完整 commit 明细见归档）

| 轮次 | 要点 |
|---|---|
| M1 验收 | 运行时自修真实 bug 全链路通过（8254bd7）+ 6 问题登记（本轮，报告 ACCEPTANCE_M1） |
| 回头看+对标 | 架构回头看两报告 + 三调研笔记 + 架构 v0.21 |
| 代码回看 | P0×3 + P1 修复（5c6451b） |
| 拍板轮 | 质量门方案 A / pi 改名 agents / 编程立项材料（fa5019b） |
| Steward 验收 | 采集器全链路 + 锚点修复（059c9e6） |
| Steward 续接 | 静默窗口/低成本模型/boot 汇报/前端状态页 + 窗口预算修复（b799dc3, 18a15e9） |
| CI 提速 | VMware 自托管 runner 接管质量门 3 Rust job（3005936）；sccache GHA 实证放弃 |
| pi 废除②③ | subagent 换 bm-loop（4997e8b）；legacy 删空 + asupersync 迁 vendor（0592cab） |
| 阶段 1 主干 | A1-A7/B1-B6 全部落地（见归档 commit 索引） |

## 下一步动作（都可直接开工）

1. **桌面壳（M2 独立壳的新形态，用户 2026-08-15 定调，方案已批准待开工）**：前端改造成模拟 OS 桌面——启动画面→空桌面→左下开始菜单（聊天/编程/设置/插件/管家）+ 任务栏 + 居中可拖拽窗口（react-rnd）+ 红黄绿三圆点；风格锚 = Cosmos Cloud 紫蓝渐变玻璃风；聊天/设置/插件/管家组件零改动嵌入、后端零改动。**开工先读 docs/HANDOFF_DESKTOP_SHELL.md（自包含：拍板清单/现状地图/步骤/坑/验证）**；同步修 M1 验收报告 §四 ①②③ 留后续轮
2. **Steward 收尾**：采集器挂任务计划程序（README 有 schtasks 命令，填真实管家会话 ID）——待用户点头（机器级定时唤醒）
3. **代码回看未修项**（挂 M2）：inbox 双队列接线或删除、prompt_hash 已随 M1 验收修复（8254bd7）、BM_STEWARD_* env 集中化、15min 超时任务驻留
4. **回头看尾账（已修完，剩余均有排期不阻塞）**：merge 事件随 session.* merge 工具落地时补；GlobalSeq 存储列阶段 5 前落（类型已留口）；压缩锁恢复已如实标注（单写者+回放幂等无实际影响，阶段 3 补实现）
5. 可选顺手件：code-graph MCP 工具面实测（新会话生效）

## 注意坑（浓缩操作要点，完整背景见归档 §〇·五）

**构建/测试**
- bm-compat 测试必须 `--test host --test load --test execute --test events --test session`（lib cfg(test) 缺上游 dev-deps，裸跑报 proptest 找不到）
- standalone 起服务必须 `--features embed`（否则 `/` 404）；本地测试/编译加 `CARGO_PROFILE_DEV_DEBUG=0`（debug exe 2GB 坑）
- **服务运行中不能编译**（exe 被锁，链接失败"拒绝访问"）；`cargo build | tail` 的退出码是 tail 的（吞失败码）——先停服务再编，别用管道接退出码
- 引擎选择：bm 默认已反转；resolve 逻辑在 bm_engine（env > settings > 默认）

**API/前端**
- Windows curl 中文 JSON 报 invalid unicode（含 em-dash 等 Unicode 标点）——验收用纯 ASCII 或浏览器
- `/api/sessions/{id}/events` 是 SSE 流，curl 会挂住——验证事件用 messages 面或日志
- IAB 浏览器 fill 不触发 React onChange（按钮 disabled 不解除）——须真实键盘 type

**引擎/压缩**
- MiniMax 流式须 `stream_options.include_usage`（默认 usage:null）；缓存字段在 `prompt_tokens_details.cached_tokens`
- pi/bm 对比口径：bm input=全量、pi input=未命中（勿双重计数）
- chars/4 粗估对中文低估 ~2×（水线判定用 max(粗估, 真实 usage)）；413/400 已修（工具结果 5MB 硬顶 + 窗口/2 预算双点）
- 压缩参数来源 = `[compaction]` 配置经 `CompactionConfig::effective(provider, model)` 换算（overrides 优先；enabled=false = 不挂压缩插件裸跑）；bm-compactor 经内核 KernelBuilder 注册为 "compactor" 服务，引擎从 kernel 取
- fork 后子分支 **seq 1 = branch/fork 标记**（首事件非空）——写会话生命周期工具（session.*，M3 起）时注意：fork 不再产生空分支

**插件/工具**
- 桥调用首参 secret 不绑定 JS 形参；tool_result 事件 content 用 ContentBlock 数组（`[{type:"text",text}]`）
- 内置工具 schema 须注册进 ToolRegistry（模型看不到就不会调）；SELF_TOOLS 跳过搜索类工具是设计
- 目录型插件须 extension.json；改插件源须同步 `~/.boenmind/extensions/` 副本；Disposer 必须交回 apply 的 Vec

**Steward**
- 管家提示词须覆盖式声明置尾（模型拒绝扮演管家）；验收用全新会话（身份历史污染）；inject 的 wake_after_seconds 会被回合内 set_wake 覆盖；验收加速 `BM_STEWARD_PACING_MIN_S=10`；两段式起服务（先无 env 建会话 → 带 BM_STEWARD_SESSION 重启）
- 静默窗口监视事件日志 head_seq（非共享 progress）；回合失败自动清 next_wake_at（防失败风暴，已修）

**subagent**
- 子进程协议 pointer 是 camelCase（`/assistantMessageEvent/delta`）；子进程无插件引擎（工具面=内置∩csv）
- `(&BTreeMap).clone()` 克隆引用须 `(*x).clone()`；`lines().next_line()` 返回 Result<Option> 要 transpose；取消传播靠 kill_on_drop

## 待拍板

1. ~~**编程应用 7 拍板点**~~ → **已拍板（2026-08-15 用户拍定）**：① M1 零新增直接验收；② **M2 独立壳应用起步**（注意：与文档原建议"现有前端加卡片"不同，用户选了独立壳）；③ 迁移门槛 **M3**；④ 三平台 T **后置**；⑤-⑦ 按文档建议执行（CI 方案 A 长期用 / pi 死数据随专项清 / 记忆写回契约 M1 后做）——M1 开工中
2. **对标吸收清单拍板**（docs/REVIEW_LANDSCAPE_2026-08-15.md §六）：高优先 9 条（dsh slot 机制/memory 契约字段/事件订阅/晶体模板/淡化三机制/pdf 基准/ponytail 技能/商店路线/.claude-plugin 兼容）执行时机——多数按阶段落地（记忆→阶段 5、slot→阶段 4、商店→§四·C），建议无需单独立项，随阶段吸收即可；另有 ACKEN 项目请用户提供来源后复核
3. `PI_SUBAGENT_*` 环境变量命名残留（自研协议通道仍用 pi 前缀）——改名待拍板
4. GitHub Billing 处理（用户操作；不处理则 CI 永久本地化，macOS 构建链发版时另想办法）
5. 远期（有触发时机，不急）：前端隔离机制（阶段 4）、沙箱层级（阶段 3）、平台驱动 ABI 纪律

## 关联文档

- 架构：docs/everything-is-plugin-architecture.md（v0.21，三铁律/§6.8 编程应用/§14 管家/§15 回头看登记）
- 架构回头看：docs/REVIEW_ARCHITECTURE_2026-08-15.md
- 对标调研：docs/REVIEW_LANDSCAPE_2026-08-15.md（笔记：docs/research/2026-08-15/）
- 代码回看：docs/REVIEW_CODE_2026-08-15.md
- 编程立项：docs/REVIEW_BEFORE_CODING_APP.md
- 历史全量：docs/HANDOFF_KERNEL_PHASE1_ARCHIVE.md
