# HANDOFF：核心三插件化 + 真实链路修复 + 皮肤接入交接（2026-08-18）

> 状态：**三项全部完成并已推送**。本交接承接 `docs/HANDOFF_KERNEL_FIX_2026-08-18.md` 的
> "下一轮 = 插件/M5 主线开题"。本轮完成：核心三插件化（5734429）、真实 Minimax SSE 修复
> （f79342e）、dsh-web-ui 皮肤接入（18ae5d3）。挂账：**前端对话视图不渲染消息**（GUI 实测
> 发现，影响可用性，建议下轮优先）。

---

## 1. 一句话交接

**插件主线落地**：LLM provider / loop / tools 三核心组件模块化为 Rust 热插拔插件（含分类标签
Core/Feature），最小装配即可跑完整回合（集成测试钉死）；真实 Minimax-M3 仿真暴露并修复
"SSE 无 `[DONE]` 被误判 torn"的潜伏 bug（真实 provider 回合首次跑通）；dsh-web-ui 全家桶
+ frosted 玻璃皮肤并入前端快照（boot 38→63）。当前基线：cargo test 107 全过、clippy 零警告。

---

## 2. 本轮落地清单（commit 见 git log）

### 2.1 核心三插件化（5734429）——用户定调"最小化运行 = provider + loop + tools 三插件即可跑"
- **contracts 新增 `plugin.rs`**：`PluginCategory{Core,Feature}` 分类标签（用户定调：核心组件
  归 Core、插件管理员按类隐藏、用户日常不可见）+ `PluginManifestEntry{id,category,name,
  description,version}` + 三插件常量 llm/loop/tools。
- **loop 插件化**：`AgentPort` trait（kernel-loop，与 LoopError 同 crate——避开契约层重复
  错误类型；对应 dsh 官方 `dsh-agent-loop`，官方自述 "concrete agent loop plugin"、内部实现
  即 ReactLoopAgent=我们的原型，**官方 loop 也是进程内 Cordis 插件，非独立进程**）+ impl
  委托 ReactLoopAgent + `plugin::manifest()` 声明。
- **llm / tools 插件化**：`plugin::manifest()` 声明（对应 dsh-llm/dsh-tools）；`ToolRegistry`
  补 `unregister`（运行期装卸）。
- **assembly 组合根正式换装 API**（替代裸改 pub 字段，code-review 点名问题消除）：
  - `swap_llm`：`llm: Arc<RwLock<Arc<dyn LlmPort>>>`（**必须两层 Arc**，SharedLlm 共享锁）+
    SharedLlm **每回合现读** → 换装后下一请求生效**对所有会话**，运行中回合持旧 Arc 安全；
  - `swap_loop`：AgentFactory = `Arc<dyn Fn>`（**Box<dyn Fn> 不能 Clone 是坑**）——新会话
    用新实现、运行中会话不受影响；
  - `register_tool` / `unregister_tool` 透传；
  - `plugin_manifest()` 清单。
- create/restore 返回 `Arc<dyn AgentPort>`；headless 装配走三插件路径（mock llm +
  ReactLoopAgent 默认工厂 + 内置工具组）= 最小基座即完整回合闭环。
- web-server：main.rs 裸改 `runtime.llm` → `swap_llm`；`SessionHandle.agent: Arc<dyn AgentPort>`；
  新 RPC `plugin.core.list`（e2e 实测返回三核心 category=core）。
- 集成测试 `kernel-assembly/tests/minimal_three_plugins.rs` ×6：最小装配完整回合 / swap_llm
  下一回合生效 / swap_loop 只影响新会话 / 工具装卸热插拔 / 清单三件全 Core / 默认工厂。
- **坑实录**：parking_lot guard 跨 await 非 Send（先 clone 出 Arc 再 await）；async-trait 需在
  assembly [dependencies]（dev-deps 不够）；重复 #[async_trait] 属性报 E0195。

### 2.2 真实 Minimax SSE 修复（f79342e）——仿真测试暴露的潜伏 bug
- **症状**：web-server 收到完整 SSE 流但回合恒 STREAM_CLOSED torn、无产出（真实 provider
  回合从未跑通过，属潜伏假配链路）。
- **根因**：openai.rs 逐字对齐官方 translate.ts——`[DONE]` 是唯一收尾哨兵，EOF 无 `[DONE]`
  一律 STREAM_CLOSED。curl 直连实测 **MiniMax 流式响应以 finish_reason 帧收尾、不发 `[DONE]`**
  （finish_reason:length 块后直接 EOF）。
- **修**：收尾逻辑统一（官方 `[DONE]` 与 EOF-with-finish 兼容服务同路径——先发 block-end/
  usage 再 finish）；流尽且无任何完成证据（无 `[DONE]` 也无 finish_reason）才 STREAM_CLOSED
  （空流/静默 EOF 仍显错，原契约保留）。
- 回归测试 `eof_with_finish_reason_closes_normally`。

### 2.3 dsh-web-ui 皮肤接入（18ae5d3）——用户拍板"装 dsh-web-ui，才好发现问题"
- profile（dsh-home/profiles/web）：bundle 注册 `@linxin666/dsh-web-ui-all` + **12 皮肤子包**
  （blue-fantasy/whale-song/harbor/qq98/ths/xp/dragon-heir/minecraft/trading/miku/whale-mom/
  matrix）；pnpm-workspace.yaml `allowBuilds`（cloudflared/cpu-features/ssh2）；ssh2 optional
  crypto 构建失败=安全降级（纯 JS 回退）非阻断。
- Node dsh 后端重抓快照：**boot 38→63 条**（web-ui 聚合 + 11 子包 + 12 皮肤 + frosted-window）；
  同步 `kernel/web-server/frontend/`（新 boot rev c7d29214a72e）。
- GUI 实测（真实 Minimax web-server）：设置→皮肤中心打开、12 皮肤在列；**首测"试穿失败，
  详见控制台"——vision 截图确认 + 根因 = 12 皮肤子包未装**（skin-center 只引用它们、
  聚合包不自动带）；装齐后复测 Blue Fantasy 激活无错误、皮肤装饰小部件（XP 窗口/行情条/
  初音未来/交易终端）生效。
- 视觉验证：本会话无 minimax-vision MCP 工具，按同参直连 MiniMax-M3 识图 API 分析截图
  （`.tmp/vision-check.mjs`）；截图存 `.tmp/gui-skin/`。

---

## 3. 验证矩阵

| 项 | 结果 |
|---|---|
| cargo test --workspace | **107 全过**（+7：最小装配 6 条 + SSE EOF-with-finish 1 条） |
| clippy --workspace --all-targets -D warnings | 零警告 |
| verify-gate1.sh | ALL PASS（5734429 后跑过） |
| Minimax-M3 真实仿真（`.tmp/minimax-sim-verify.mjs`） | **ALL PASS**：swap_llm 装配面 + plugin.core.list + llm.models + 真实回合 completed + 中文回答（带 think 推理块） |
| GUI 浏览器实测（真实 Minimax，3080） | 页面加载/工作区/模型按钮 MiniMax-M3/发消息全链路通；**后端 81 事件 completed，但前端对话/轨迹视图不渲染消息（见 §4 挂账 1）** |
| 皮肤 GUI 实测（63-entry 快照） | 皮肤中心 12 款/试穿/激活 vision 确认；装饰小部件生效 |
| conformance / gate25 / m3-r3 / hot-replace | **未重跑**（上轮基线；本轮 openai.rs wire 面有改动，**下轮开工前补跑**——跑法见 §5） |

---

## 4. 遗留/挂账（下轮开题候选）

| # | 项 | 说明 | 优先级 |
|---|---|---|---|
| 1 | **前端对话视图不渲染消息** | GUI 实测：后端事件完整（81 条 completed），前端对话/轨迹 tab 均空白（轨迹只有 timeline 骨架 + "No timing data"）。Rust 后端 ↔ dsh 前端快照的**渲染链路 gap**（conformance 只验证 RPC 层，从未在真实浏览器跑过对话渲染）。修法待查：前端订阅/事件投影消费链路，或 assistant/message wire 形状与前端期望对齐 | **高（下轮优先）** |
| 2 | 前端插件管理按 category 分组折叠 | 核心三插件分类标签（Core/Feature）数据层已就位（plugin.core.list），前端分组/隐藏需 patch dsh 前端快照，有升级同步成本 | 中 |
| 3 | 皮肤管理器 | 12 皮肤靠皮肤中心内置试穿/应用切换够用；要"一键切换+持久互斥+热禁用"再上 dshmarket | 低 |
| 4 | subagent 改管家式会话方向 | 用户开题过：子会话委派替代 M5 team 插件进程；supervisor 退回只管热升级蓝绿。**2 拍板点待用户** | 中 |
| 5 | M5 supervisor 完整化 | 蓝绿替换 + 崩溃计数 + IPC 协议版本化+鉴权（架构 §五·7 升格项） | 后置 |
| 6 | P2-D session-query 索引 | M4 P2 队列剩余项，纯性能/可扩展 | 后置 |
| 7 | LlmError.retryable 死字段 | 全库零消费方；随未来重试接线时删 | 低 |

---

## 5. 环境与纪律（沿用 + 新增）

- **每轮先杀 web-server**：`taskkill //F //IM web-server.exe`（收尾再杀一次）；Node dsh 后端
  同杀 `node.exe`（残留进程占 3090/3080）。
- **验证三件套**：`cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings`
  + `bash kernel/scripts/verify-gate1.sh`。
- **真实链路仿真**：`.tmp/minimax-sim-verify.mjs`（release web-server --config
  ~/.boenmind/config.toml，端口 3090；history 事件信封 = `{event:{type,seq,time,data}}`，
  data 内才是 reason/content；RPC 信封 = `{type:"client-request",rpcId,method,payload}` 走
  POST /api/<method>；Windows 清理子进程 db 句柄需延迟重试）。
- **前端快照重抓**（新增，皮肤/插件变更后）：① **DSH_HOME 必须显式指定**（否则回退默认
  ~/.dsh 丢皮肤插件）→ `DSH_HOME="D:/96_CoderWorld/BoenMind/dsh-home" node
  dsh-home/profiles/node_modules/@deepseek-ai/dsh/lib/bin.js web --port 3090` →
  ② **先 `curl /` 覆盖 .tmp/web-snapshot/index.html 再跑 .tmp/grab-snapshot.mjs**（脚本从
  快照目录读 boot，不先更新会抓旧清单）→ ③ 同步 kernel/web-server/frontend/（含 README）。
- **视觉验证**：`.tmp/vision-check.mjs`（MiniMax-M3 识图直连，key 在 ~/.zcode/v2/config.json
  minimax-vision 配置）。**vision 是 GUI 测试的证据补强**——DOM 快照会误导（皮肤"当前激活"
  文本错位），截图+vision 交叉确认。截图存 `.tmp/gui-skin/`。
- **conformance/gate25/m3-r3/hot-replace 补跑**：conformance 走 `.tmp/dsh-trace-recorder.mjs`
  （固定端口 3081）；gate25/m3-r3 走 `.tmp/gate25-verify.mjs`（端口 3079，全新 db +
  BM_TEST_HOOKS=1）；hot-replace 走 `kernel/scripts/hot-replace-verify.mjs`（端口 3082）。
  跑前起 release web-server（debug exe 2GB 超 PE 限制，见记忆 build-debug-exe-2gb-pitfall）。
- **文件地图（本轮）**：
  - 三插件契约：`kernel/kernel-contracts/src/plugin.rs`
  - AgentPort/loop 插件：`kernel/kernel-loop/src/lib.rs`（AgentPort+委托 impl）、`plugin.rs`
  - llm/tools 插件声明：`kernel/kernel-llm/src/plugin.rs`、`kernel/kernel-tools/src/plugin.rs`
  - 组合根换装 API：`kernel/kernel-assembly/src/lib.rs`（swap_llm/swap_loop/SharedLlm/
    plugin_manifest）
  - SSE 收尾：`kernel/kernel-llm/src/openai.rs`（`[DONE]` 与 EOF-with-finish 统一收尾）
  - 前端快照：`kernel/web-server/frontend/`（63 bundle，README 含重抓流程）
  - 皮肤 profile：`dsh-home/profiles/web/{package.json,pnpm-workspace.yaml}`
