# 五家 Agent 超时与限制横向对比调研报告(limits & timeout)

- 调研日期:2026-09-07
- 调研动机:用户 VPS 上让 BoenMind 执行 `git clone`,命令 60s 硬顶必撞超时。本报告横向摸清业界 agent 的超时/限制数值、实现形态(硬编码 / 模型逐次传参 / 配置文件),以及是否存在「按任务大小动态给时间」机制,结论支撑 BoenMind 的 limits 配置面(ADR-0024)与后台转轨(ADR-0025)设计。
- 调研对象:
  1. **BoenMind**(自家 v0.0.11,问题方)
  2. **pi**(`badlogic/pi-mono`,TypeScript 原版)
  3. **pi_agent_rust**(Dicklesworthstone,Rust 重写版)
  4. **Hermes**(`NousResearch/hermes-agent`,Python;用户 VPS 在役)
  5. **DSH**(`@deepseek-ai/dsh` v0.1.1-rc.2,TypeScript;本机 npm 全局在役)
  6. **ZCode**(用户在用的桌面 AI CLI,Electron;本机逆向)
- 本文自包含,可直接交给任何 AI 做交叉审核,不依赖原对话上下文。同族前篇:`docs/agent-tools-payload-comparison-report.md`(报文对比)、`docs/runtime-limits-inventory-20260907.md`(BoenMind 限制全量盘点,本篇 BoenMind 节为其精选引用)。
- 方法:只看代码、配置与官方文档原文,不看宣传;每条数值带证据标签与出处,数值如与源码不符以源码为准。

## 0. 证据分级

| 标签 | 含义 |
|---|---|
| 【本机源码实锤】 | DSH / ZCode / BoenMind 本机安装包可读源码或反编译产物,文件:行号可复查 |
| 【官方仓库源码实锤】 | GitHub raw 文件逐字核对 |
| 【官方文档实锤】 | 官方文档页核对 |
| 【本机配置快照】 | 用户自己的配置 / 插件备份只读检查 |
| 【待核实】 | 有线索未复核,如实标注(集中列于 §10) |

## 1. TL;DR(三条结论)

1. **「按任务大小动态算时间」五家都没有。** 业界等价物是三件套:**模型逐次传参申请时长 + 配置上限钳制 + 长任务转后台**。最接近"动态"的是 Hermes——前台超 600s 不报错、自动转后台并承诺完成通知;DSH / ZCode 提供 `run_in_background` 完全不限时;pi(TS) 干脆不设默认超时,把决定权全交给模型。没有任何一家按命令或任务的体量(如 clone 仓库大小)计算超时。
2. **除 TS 版 pi 外全部有配置文件;「全硬编码 + 模型传参被钳死」是 BoenMind 独一份。** Hermes 几乎全量 .env + config.yaml(配置面最全),pi_agent_rust 用 settings.json 分层覆盖,DSH 有 settings schema(挂 UI 设置项),ZCode 六层配置。BoenMind 全仓只有 `BOEN_TURN_TIMEOUT_SECS` 一个环境变量可调,其余全写死;且模型自传的 timeout_ms 被 min() 链钳回 manifest 的 60s,传参形同虚设——这个组合五家仅此一家。
3. **命令超时业界收敛值 = 默认 120s / 前台上限 600s / 后台不限时。** pi_agent_rust、DSH、ZCode 三家独立收敛到同一组数(默认 120s、上限 600s);Hermes 默认 180s、前台上限同为 600s;pi(TS) 无默认、上限约 24.86 天(等于没有)。BoenMind 的 60s manifest 值低于全体业界默认档,是 VPS clone 超时的直接死因。

## 2. BoenMind(自家 v0.0.11)【本机源码实锤】

### 2.1 数值表

| 限制 | 数值 | 出处(file:line) |
|---|---|---|
| exec 命令超时(**铁顶**) | manifest `timeout_ms=60000`;模型自传先钳 1s~300s;执行体取 min(模型传值, deadline);deadline = manifest 值 clamp(100ms, 600s)——四层钳制后**恒 ≤60s**,模型传多大都没用 | `runtime/crates/bm-providers/src/system_exec.rs:42/80-84`;`bm-core/src/runtime/turn/capability.rs:731` |
| exec 输出截断 | 16,000 字符 | `system_exec.rs:20` |
| 回合侧工具等待 | 非审批 60s / 审批类 300s,超时报「审批等待超时」 | `bm-core/src/runtime/turn/spawn.rs:534` |
| 模型调用超时 | 默认 120s,**`BOEN_TURN_TIMEOUT_SECS` 是全仓唯一 env 旋钮** | `bm-core/src/runtime.rs:39-50` |
| 模型重试 | 供应商链内最多 3 次 | `spawn.rs:56-62` |
| 防空转熔断 | 同工具同参连续 5 次**硬熔断**(终止本轮工具循环),滑动窗口 20 条签名 | `spawn.rs:127/384-411` |
| MCP 工具超时 | 默认 30s(`DEFAULT_TOOL_TIMEOUT_MS`,mcp.json `tool_timeout_ms` 可配);远程 MCP HTTP 60s;MCP 子进程 60s 窗口内重生 ≥3 次熔断 | `bm-providers/src/mcp.rs:20/708/533-548` |
| HTTP Provider 熔断 | 连续 3 次失败 → 冷却 30s | `bm-core/src/runtime/provider_health.rs:16-18` |
| 应答硬顶 | 流式 900s / 非流式 180s;前端 60s 收不到字节主动断开 | `bm-surface-http/src/openai_compat.rs:280/359`;webapp `runtime.tsx:249` |
| 历史回喂 | 最近 20 轮且总量 ≤24,000 字符 | `bm-core/src/runtime/turn/history.rs:4-6` |
| fs 工具 | read/write 上限各 16MB;搜索默认 80 条钳 500;输出 16K 字符;>1MB 文件搜索跳过;read 单次 10,000 行 | `bm-providers/src/fs_tools/ops.rs:14-26` |
| 管理面 | 打包下载 256MB / 5000 条;预览 512KB;删除批 100;目录浏览 1000 条 | `webadmin.rs` |

全量清单(含管理面杂项、基建隐含)见 `docs/runtime-limits-inventory-20260907.md`。

### 2.2 形态与机制判定

- 配置形态:**全部硬编码常量,无配置文件**;可调旋钮仅 `BOEN_TURN_TIMEOUT_SECS` 一个 env + mcp.json 的 `tool_timeout_ms` / `restart_limit`(插件侧)+ skill manifest `timeout_ms`。
- 模型传参:允许模型自传 timeout_ms,但被 min() 链钳死在 manifest 60s,**形同虚设**。
- 后台任务:**无**任何后台转轨机制,回合侧 60s 等不到即报错。
- 按任务大小动态给时间:**无**。

## 3. pi(badlogic/pi-mono,TypeScript 原版)【官方仓库源码实锤】

### 3.1 数值表

| 限制 | 数值 | 出处 |
|---|---|---|
| bash 超时默认 | **无默认超时**。schema 原文「Timeout in seconds (optional, no default timeout)」;`resolveTimeoutMs` 对 undefined 返回 undefined,不设定时器 | `packages/coding-agent/src/core/tools/bash.ts` |
| 模型自调 | 逐次传 timeout(秒),自由定 | 同上 |
| 上限 | `MAX_TIMEOUT_MS = 2^31-1 ms ≈ 24.86 天`;超限**抛错拒绝**(非静默钳制) | 同上 |
| 超时行为 | `killProcessTree` 杀整个进程树;超时/中止后仍把**已捕获输出以错误消息附回模型**(不白等) | 同上 |
| 输出截断 | bash 保留末尾 tail / read 保留开头 head;默认 **2000 行或 50KB** 先到者;截断后**完整输出落临时文件并把路径给模型**;grep 单行截 500 字符 | `src/core/tools/truncate.ts:11-13` |

### 3.2 形态与机制判定

- 配置形态:**全部硬编码常量,无配置文件,也无 env 覆盖**——五家中唯一。
- 模型传参:六家中最自由(上限近似不存在,超限是显式抛错而非静默钳制)。
- 后台任务:本轮素材未核实到后台机制(与原版极简定位一致,勿当「没有」引用)。
- 按任务大小动态给时间:**无**。

## 4. pi_agent_rust(Dicklesworthstone,Rust 重写版)【官方 README 实锤】

### 4.1 数值表

| 限制 | 数值 |
|---|---|
| bash 超时默认 | **120s**,模型逐次可调;**timeout: 0 = 完全禁用超时** |
| 超时行为 | 杀进程树:TERM → 5s 宽限 → KILL |
| 输出截断 | MAX_LINES 2000 / MAX_BYTES 1MB(头 1000 行 + 尾 1000 行,中段省略标记);grep 单行 500 字符 |
| 配置 | settings.json 分层:`~/.pi/agent/settings.json`(全局)+ 项目 `.pi/settings.json`;优先级 **CLI > env > 项目 > 全局 > 默认** |
| 重试 | max_retries 3 / base_delay 1000ms / max_delay 30000ms |
| 工具迭代 | max_tool_iterations 默认 **50** |
| 上下文压缩 | reserve_tokens 8192 / keep_recent_tokens 20000 |
| fs 限制 | read 2000 行 / 1MB;find 1000 条 / ls 500 条 / grep 100 条 |
| 后台任务 | 有;工件预算 **16MiB/条、256MiB 总量、4096 条**(`PI_JOBS_ARTIFACT_RETENTION=rotate`) |
| 子代理 | tasks 最多 8、默认并发 4、**子代理不能再开子代理**;web_search 带熔断 |

### 4.2 形态与机制判定

- 配置形态:分层配置文件(settings.json)+ env,默认值硬编码兜底。
- timeout: 0 是「显式关闸」,不是动态计算——按任务大小动态给时间:**无**。

## 5. Hermes(NousResearch/hermes-agent,Python)【官方文档+官方仓库源码实锤】

### 5.1 数值表

| 限制 | 数值 | 出处 |
|---|---|---|
| terminal 默认 | **180s**(env `TERMINAL_TIMEOUT`,代码 `_parse_env_var("TERMINAL_TIMEOUT","180")`;**注意官方 `.env.example` 样例配的是 60s**,照抄样例反而变小) | `tools/terminal_tool.py` |
| 前台上限 | **600s**(env `TERMINAL_MAX_FOREGROUND_TIMEOUT` 可覆盖);模型逐次传 timeout(秒,≥1) | 同上 |
| **超限转后台** | 超 600s 的前台请求**不报错,自动转后台** + `notify_on_complete` + 提示模型勿重跑(`_PROMOTED_NOTE`) | 同上 |
| 输出上限 | **100,000 字符**(registry `max_result_size_chars`),完整文本落文件把路径给模型 | registry |
| 沙箱生命周期 | `TERMINAL_LIFETIME_SECONDS` 默认 300s | 同上 |
| LLM | `HERMES_API_TIMEOUT` 1800s;流式 stale 180s(本地 provider 900s);`HERMES_STREAM_RETRIES` 3;连续 stale 5 次熔断放弃(`HERMES_STREAM_STALE_GIVEUP`);每会话最大工具迭代 `HERMES_MAX_ITERATIONS` 500;网关 agent 不活动超时 1800s | env 参考 + 源码 |
| 浏览器/定时任务 | `BROWSER_INACTIVITY_TIMEOUT`(env);cron 任务超时 600s、并行 4;重启风暴熔断 120s 窗口 5 次 | 同上 |

### 5.2 形态与机制判定

- 配置形态:**五家中配置面最全**——几乎全部走 .env + config.yaml(上下文压缩阈值与 fallback 链**只有 config.yaml 没有 env**);官方 env 参考文档数十个 TIMEOUT / MAX / RETRY 变量带默认值。
- 用户 VPS 实际 .env:TERMINAL_TIMEOUT / TERMINAL_LIFETIME_SECONDS / BROWSER_SESSION_TIMEOUT / BROWSER_INACTIVITY_TIMEOUT 四键存在,但值未读取【待核实】(快照仓 `D:\96_CoderWorld\hermes-setting-readonly` 只含键名)。
- 用户自研 Hermes 插件几乎全硬编码【本机配置快照】:web-multisearch 并行总兜底 25s、各源 12~30s;agnes 生图 360s / 生视频轮询 15s×40 次;pdf-omni 单文件最长等 600s、页数 200~2000、文件 200~300MB。
- 按任务大小动态给时间:**无**——超限转后台是**转轨策略**,不是把时间按任务大小放大。

## 6. DSH(@deepseek-ai/dsh v0.1.1-rc.2,TypeScript)【本机源码实锤】

### 6.1 数值表

| 限制 | 数值 | 出处 |
|---|---|---|
| bash/pwsh 超时 | 默认 timeoutMs **120s**、maxTimeoutMs **600s**;钳制 = min(传值 ?? 默认, 上限) | `dsh-timeout/lib/index.js:43-46` |
| 超时行为 | 杀进程宽限 3s;输出 64KB / 流内存,截断后全文 spill 落盘(上限 64MiB) | 同上 |
| **后台任务** | `run_in_background: true` = **完全无超时**,任务进 ctx.jobs;job_output 工具收产出:wait 每次默认 30s、钳 600s,**可反复调用** | `dsh-tool-jobs/lib/index.js:267` |
| LLM 重试 | 5 次(500ms → 10s 抖动) | — |
| MCP | 单工具 60s;重连 10 次封顶 | — |
| 轮数 | 无总轮数上限;连续重复调用 [3,5,8] 次仅注入提醒(advisory,**不硬杀**);goal 轮数上限 256 | — |
| 并行 | 工具并发 10 | — |
| 上下文压缩 | 窗口×0.8 触发、保留×0.16、摘要 8192 tokens;tool-result 修剪 8192/4096/1024 字符 | — |
| fs 限制 | read 2000 行 / 单行 2000 字符 / 单次 50KiB / ≥10MiB 流式;glob 100 条 / grep 250 条 / 30s | — |
| web | fetch 30s / 输出 200K 字符 | — |

### 6.2 形态与机制判定

- 配置形态:硬编码默认 + composition/settings schema 配置覆盖(每子包 `installSettingsSection` 挂 UI 设置项);本机用户 `.dsh` 配置零覆盖 = 全默认在役。
- job_output 反复 wait 是「分段等」,单次有界、总时长不受限——按任务大小动态给时间:**无**。

## 7. ZCode(桌面 AI CLI,Electron)【本机 bundle 实锤】

> 来源:本机安装目录 `resources/glm/zcode.cjs`(12.6MB minified bundle)逆向;配置键名为逆向还原,数值以匹配到的代码片段为准(见 §10)。

### 7.1 数值表

| 限制 | 数值 |
|---|---|
| Bash 超时 | defaultTimeoutMs **120s** / maxTimeoutMs **600s**;模型逐次传 timeout 钳制(allowCallOverride:true,`Math.min(传值 ?? 默认, 上限)`);env `BASH_DEFAULT_TIMEOUT_MS` / `BASH_MAX_TIMEOUT_MS` 可覆盖 |
| 超时行为 | SIGKILL 宽限 5s |
| 后台任务 | 后台 Bash 默认 **300s**;子代理后台 Bash 硬顶默认 **1h**(配置键 `subagents.backgroundBashMaxMs`,到点自动 cancel) |
| 输出 | 内联默认 30,000 字符(env `BASH_MAX_OUTPUT_LENGTH`)**硬顶 150,000**;后台任务持久化顶格 5GB |
| MCP | 单调用默认 30s(配置键 `mcp.servers.<name>.timeoutMs` + 探针 5s);插件注册的 MCP server 600s |
| 网络 | 全局 network.timeout 默认 180s(env `ZCODE_HTTP_TIMEOUT` / `ZCODE_TIMEOUT`) |
| 模型流 | 空闲超时 600s(`modelStream.idleTimeoutMs`),恢复重试每次 +30s、最多 10 次;模型 API 重试 11 次(2s 起、指数、封顶 60s,env `ZCODE_MODEL_RETRY_*` 可调) |
| 重复调用 | **软熔断**:同一工具同输入连续 3 次仅注入提醒「不要原样重复调用」(`modelAnomalyGuard.repeatedToolCallWarningThreshold`),不硬杀 |
| 子代理 | 深度 = 1 禁止嵌套(结构性);工具并发 10(`toolConcurrency.maxConcurrency`) |
| 上下文 | 默认窗口 200k tokens;auto-compact 阈值 = 有效窗口 − 输出预留 − 13k buffer(公式硬编码 + 多配置键覆盖);连续 3 次压缩失败熔断 |
| 配置 | 分层 System / User / Project / Session / Env / Cli(`~/.zcode/cli/config.json` + 项目 `.zcode/config.json` + zcode.json)——六家形态最成熟 |

### 7.2 机制判定

- 按任务大小动态给时间:**无**。

## 8. 横向大对比表

| 维度 | BoenMind | pi(TS) | pi_agent_rust | Hermes | DSH | ZCode |
|---|---|---|---|---|---|---|
| 命令超时默认 | 60s(manifest 硬顶) | 无默认 | 120s | 180s | 120s | 120s |
| 前台命令上限 | 60s(min 链钳死) | ≈24.86 天(2^31-1 ms) | 自由可调,0=禁用超时 | 600s(env 可覆盖) | 600s | 600s |
| 模型逐次自调 | 允许传但被钳死,形同虚设 | 自由(上限近似无) | 自由(0=禁用) | 自由(≥1s) | 自由(钳上限) | 自由(钳上限,env 可改上下限) |
| 超限/上限行为 | 静默钳回 60s | 超上限抛错拒绝 | 0=显式禁用 | 超 600s **自动转后台**+通知 | 钳到 600s | 钳到 600s |
| 后台任务 | 无 | 素材未核实 | 有(工件 16MiB/条、256MiB 总、4096 条) | 有(超限自动转 + cron 600s/并行 4) | 有(run_in_background 无超时 + job_output 轮询) | 有(后台默认 300s;子代理 1h 硬顶) |
| 输出截断 | 16K 字符 | 2000 行/50KB,全文落临时文件给路径 | 2000 行/1MB,头尾保留中段省略 | 100K 字符,全文落文件给路径 | 64KB,spill 落盘 64MiB | 30K(硬顶 150K);后台持久化 5GB |
| 模型调用超时 | 120s(唯一 env 可调) | 素材未覆盖 | 素材未覆盖 | 1800s;流 stale 180s(本地 900s) | 素材未覆盖 | 流空闲 600s(+30s/次恢复) |
| 模型重试 | 3 次 | 素材未覆盖 | 3 次(1s→30s) | 流 3 次 + 连续 stale 5 次熔断 | 5 次(500ms→10s 抖动) | 11 次(2s→60s)+ 流恢复 10 次 |
| 重复调用处置 | **硬熔断**:同工具同参 5 次终止本轮 | 素材未覆盖 | 素材未覆盖 | 素材未覆盖 | **软提醒** [3,5,8] 次 | **软提醒** 3 次 |
| 迭代/轮数上限 | 工具循环 5 轮 + 任务 max_turns 1000 | 素材未覆盖 | max_tool_iterations 50 | HERMES_MAX_ITERATIONS 500 | 无总上限(goal 256) | 素材未覆盖 |
| 上下文压缩 | 无(历史回喂 20 轮/24K 字符截断) | 素材未覆盖 | reserve 8192 / keep 20000 | config.yaml 配阈值 | 窗口×0.8 触发,摘要 8192 | 200k 窗口 auto-compact,公式 −13k buffer |
| 配置形态 | **全硬编码 + 1 个 env,无配置文件** | **全硬编码,无任何配置** | settings.json 五级覆盖 | .env + config.yaml(变量最多) | settings schema(UI 挂载) | 六层分层(最成熟) |
| 按任务大小动态给时间 | 无 | 无 | 无 | 无(转后台 ≠ 放大) | 无 | 无 |

注:「素材未覆盖」= 本轮调研素材未含该项,不得当「没有该机制」引用;补核后更新本表。

## 9. BoenMind 差距清单(修复方向 = ADR-0024 / ADR-0025,已批准动工)

| # | 差距 | 现状(证据) | 业界对照 | 修复方向 |
|---|---|---|---|---|
| 1 | **exec 60s 铁顶** | manifest 60000 + min 链四层钳制(`system_exec.rs:42/80-84`;`capability.rs:731`) | 业界收敛:默认 120s、上限 600s(pi_agent_rust / DSH / ZCode 一致;Hermes 180s/600s) | ADR-0024 limits 配置面:manifest 默认抬到业界档,数值出配置 |
| 2 | **模型传参被钳死** | min(模型传值, deadline),deadline 又钳回 manifest → 传参形同虚设 | 五家通行的「模型逐次申请 + 上限钳制」(pi_agent_rust 还支持 0=禁用;pi 上限抛错不静默) | ADR-0024:上限钳制取代 manifest 钳死,让模型自调真实生效 |
| 3 | **无后台任务** | 回合侧 60s 等不到即报错(`spawn.rs:534`);无任何转轨 | Hermes 超 600s 自动转后台+通知;DSH run_in_background 无超时 + job_output 轮询;ZCode 后台 300s / 子代理 1h;pi_agent_rust 工件预算制 | ADR-0025 后台转轨:长命令转后台 jobs + 轮询收产出 + 工件落盘(预算制) |
| 4 | **无配置面** | 全硬编码,仅 `BOEN_TURN_TIMEOUT_SECS` 一个 env(`runtime.rs:39-50`) | Hermes .env+config.yaml;pi_agent_rust settings.json 五级;DSH settings schema;ZCode 六层 | ADR-0024:超时/截断/熔断/迭代等数值入配置文件(数据目录),env 仅作覆盖 |
| 5 | 输出截断偏紧(附带) | exec/fs 输出 16K 字符(`system_exec.rs:20`;`fs_tools/ops.rs`) | 业界 30K~100K,且超限**全文落盘把路径给模型**(pi/DSH/Hermes 一致做法) | 随 ADR-0024 一并参数化;超限落盘续读 |

## 10. 未核实清单

1. **用户 VPS Hermes .env 四键实际值**(TERMINAL_TIMEOUT / TERMINAL_LIFETIME_SECONDS / BROWSER_SESSION_TIMEOUT / BROWSER_INACTIVITY_TIMEOUT):快照仓只含键名,值未登录读取【待核实】。
2. **ZCode 数值为 minified bundle 逆向**:12.6MB `zcode.cjs` 常量与配置键命名可能失真,数值以 bundle 中匹配到的代码片段为准;后续官方文档可交叉验证。
3. **pi_agent_rust 全部数值来自官方 README**,未逐行核对 Rust 源码(README 与实现可能滞后)。
4. **pi(TS) 后台任务能力**:本轮素材未覆盖,§3.2/§8 标「素材未核实/未覆盖」,非断言其没有。
5. **Hermes VPS 实际 terminal 超时**:代码默认 180s 已实锤,但若 VPS .env 照抄官方样例则为 60s——实际取值取决于第 1 条的核实结果。

## 11. 复核指引(供跨 AI 验证)

- **BoenMind**:全部 file:line 可直接打开对照;全量限制盘点与 rg 复现命令见 `docs/runtime-limits-inventory-20260907.md` §七。
- **pi(TS)**:github.com/badlogic/pi-mono → `packages/coding-agent/src/core/tools/bash.ts`(resolveTimeoutMs / MAX_TIMEOUT_MS / killProcessTree)、`truncate.ts`(截断常量),GitHub raw 逐字核对。
- **pi_agent_rust**:官方仓库 README(github.com/Dicklesworthstone/pi_agent_rust)逐节核对。
- **Hermes**:github.com/NousResearch/hermes-agent → `tools/terminal_tool.py`(`_parse_env_var("TERMINAL_TIMEOUT","180")`、`TERMINAL_MAX_FOREGROUND_TIMEOUT`、`_PROMOTED_NOTE`)、registry `max_result_size_chars`、官方 env 参考文档页。
- **DSH**:本机 npm 全局包 `C:\Users\Boen\AppData\Roaming\fnm\aliases\default\node_modules\@deepseek-ai\dsh\`,重点 `dsh-timeout/lib/index.js:43-46`(钳制公式)、`dsh-tool-jobs/lib/index.js:267`(job_output wait)。
- **ZCode**:本机安装目录 `resources/glm/zcode.cjs`,按关键词检索:`BASH_DEFAULT_TIMEOUT_MS`、`BASH_MAX_TIMEOUT_MS`、`subagents.backgroundBashMaxMs`、`modelStream.idleTimeoutMs`、`modelAnomalyGuard`、`BASH_MAX_OUTPUT_LENGTH`。
- 数值如与源码不符,以源码为准并回报修订本报告。

> 本报告为调研产物,不修改任何产品代码;修复项以 ADR-0024(limits 配置面)/ ADR-0025(后台转轨)为准入(均已批准动工)。
