# BoenMind 程序内置限制全量盘点(2026-09-07)

> 来源:用户实测两现象——①VPS 上让 AI 分析 GitHub 项目,源码一直下载不下来,最后报 60 秒超时;②遇到「同命令同参 5 次退出」熔断——追问**程序里到底还有多少限制**。
> 方法:全仓源码模式扫描(时长常量 / clamp / MAX_ / timeout / 熔断 / watchdog / 截断),逐条回读源码钉死数值与生效链路。
> 证据分级:**【实锤】**= 文件:行号可复查;**【推断】**= 依赖框架默认行为,未实测。
> 范围:runtime/ 九 crate + webapp 前端;MCP 插件自身内部(如 web-multisearch 的上游超时)不在本轮逐盘,见 §六。

---

## 一、先回答两个现象

### 1.1 GitHub 源码下载 60 秒超时(真凶链路【实锤】)

四层钳制叠加,层层都过 60 秒这道门:

1. `system.exec` 的能力标牌(manifest)写死 `timeout_ms: 60000` —— `system_exec.rs:42`;
2. 回合执行器给每个异步能力的 deadline = 标牌值钳到 [100ms, 600s] —— `capability.rs:731`;
3. exec 执行体内部再取 **min(模型自传 timeout_ms, deadline)** —— `system_exec.rs:80-84`;
4. 模型自传 timeout_ms 本身先被钳到 [1s, 300s](`system_exec.rs:82`),但 min 之后再被第 3 步压回 60s —— **模型再聪明也传不进去,60s 是铁顶**。

结果:VPS 网络 `git clone` 中型仓库 >60s → `AsyncCallError::Timeout` → 界面报超时。本地 Windows 网络快有时 <60s 侥幸通过,所以「本地好好的,远程不行」。

台账状态:BACKLOG 此前只有相邻条目(§3「异步执行器并发排队致工具假性超时」),exec 60s 硬顶**未专列**,本轮已登记(见 BACKLOG §1)。

修法候选(**未动工,待用户裁决**):
- 方案 A:manifest `timeout_ms` 60000 → 300000(天花板 600s 之内,一行改动,`system_exec.rs:42`);
- 方案 B:环境变量化(如 `BOEN_EXEC_TIMEOUT_MS`),装配时读入覆盖 manifest 值。

### 1.2 「同命令同参 5 次退出」是什么【实锤】

- 这是 **v0.0.10 故意上线的防空转熔断**(`spawn.rs:127`),来历 = 2026-09-07 VPS 过夜评审死循环事故的根治措施之一,不是故障。
- 判定:同一工具名 + **逐字符完全一致**的入参,连续第 5 次调用 → 触发熔断,给模型回一句「(检测到连续 5 次调用相同工具与完全一致的入参,已触发防空转熔断保护。)」并终止本轮工具循环(`spawn.rs:384-411`,文案 `spawn.rs:409/671`)。
- 两个放宽点:参数有任何一字不同就重新计数;只记最近 20 次调用的滑动窗口(`spawn.rs:405-407`)。
- 误伤场景:真的需要重复同一命令等外部状态变化(如轮询下载进度)。正确姿势是让模型换参数(加 sleep、改输出路径)或拆小步,而不是调大阈值。

---

## 二、全量清单

### A. AI 干活通道(工具执行层)

| # | 限制 | 数值 | 证据【实锤】 | 可调性 |
|---|---|---|---|---|
| A1 | **exec 单条命令超时** | 默认 60s;模型可传 1s~300s 但被 min() 钳死,**实际恒 ≤60s** | `system_exec.rs:29/42/80-84` + `capability.rs:731` | 写死,需改代码(§1.1 修法候选) |
| A2 | exec 输出截断 | 16,000 字符(超出标记 truncated) | `system_exec.rs:20/129-132` | 写死 |
| A3 | 同工具同参连续 5 次 → 熔断 | 5 次;滑动窗口 20 条签名 | `spawn.rs:127/384-411` | 写死(不建议动) |
| A4 | 回合侧工具等待 | 非审批工具 60s;审批类 300s,超时报「审批等待超时(审批单已过期)」 | `spawn.rs:534/550-556` | 写死;并发排队假超时已在 BACKLOG §3 |
| A5 | 每次模型调用超时 | 默认 120s(2026-09-03 由 30s 上调) | `runtime.rs:39-50`;PLAYBOOK §4 | **BOEN_TURN_TIMEOUT_SECS 环境变量可调** |
| A6 | 模型失败重试 | 供应商链内最多 3 次 | `spawn.rs:56-62` | 写死 |
| A7 | fs.search 结果条数 | 默认 80,模型可传 max_results,钳 500 | `fs_tools/ops.rs:14-15/76-77` | 模型参数可调 |
| A8 | fs.search/read 输出 | 输出 16,000 字符封顶;>1MB 文件搜索直接跳过;read 单次 10,000 行 | `fs_tools/ops.rs:16-17/26/212/297/345` | 写死 |
| A9 | fs.read / fs.write+edit 大小 | 各 16MB(读写对等,2026-09-07 17a5ec5) | `fs_tools/ops.rs:19-22/313/379` | 写死 |
| A10 | skill 脚本(wasm) | 默认超时 10s(manifest 可带,下限 100ms);fuel 计量 2×10⁹ 防死循环烧 CPU | `skill_wasm.rs:23-26/132/164` | manifest 可配 |
| A11 | MCP stdio 写超时 | 10s | `mcp.rs:23` | 写死 |
| A12 | MCP 工具调用超时 | **默认 30s**,mcp.json 每条目 `tool_timeout_ms` 可配 | `mcp.rs:20/1165-1168/844` | 配置可调 |
| A13 | 远程 MCP HTTP 请求 | 60s 硬顶 | `mcp.rs:704-708` | 写死 |
| A14 | MCP 子进程重生熔断 | 60s 滑动窗口内重生 ≥3 次(默认 restart_limit)→ 拒绝再生 | `mcp.rs:533-548/1170-1171` | restart_limit 配置可调 |

### B. 一次对话的整体(回合/会话层)

| # | 限制 | 数值 | 证据【实锤】 | 可调性 |
|---|---|---|---|---|
| B1 | 流式应答硬顶 | 900s(15 分钟,v0.0.11 由 180s 上调);期间每 10s 发 keepalive | `openai_compat.rs:352-370` | 写死 |
| B2 | 非流式聚合等待 | 180s | `openai_compat.rs:280` | 写死 |
| B3 | 前端看门狗 | 60s 收不到任何字节 → 前端主动断开(靠 B1 的 keepalive 喂狗) | `webapp/src/w1/runtime.tsx:249-252` | 写死 |
| B4 | 任务预算 | 默认 max_tokens 100 万 / max_turns 1000 回合 | `task_ops/persist.rs:48-53` | Task 合同可配 |
| B5 | 任务 Watchdog | 15 分钟无进展=停滞通告;累计 24h=转 blocked;扫描节拍 60s;重复阈值 3;**等审批时豁免** | `watchdog.rs:16-22/34` | 合同默认值,Task 级可配 |
| B6 | 自动驾驶(autorun) | 默认 6 轮,钳 1~50;连续两轮完全相同输出=停滞 blocked | `autorun.rs:78/196` | 参数可调 |
| B7 | 历史回喂 | 只喂最近 20 轮且总量 ≤24,000 字符——**更早的对话模型看不见**(翻旧账要靠工具去查) | `turn/history.rs:4-6/104-123` | 写死 |
| B8 | 审计条目截断 | 单条 16KB(内部记录,不影响功能) | `turn/audit.rs:5` | 写死 |

### C. 模型通道层

| # | 限制 | 数值 | 证据【实锤】 | 可调性 |
|---|---|---|---|---|
| C1 | HTTP Provider 熔断 | 连续 3 次失败 → unavailable 冷却 30s(半开探测失败会重开冷却);401/403 配置错不计入 | `provider_health.rs:16-18/48-63`;`openai_http.rs:694` | 写死 |
| C2 | MCP 重连探针封禁 | 3 次封禁 | `provider_health.rs:19`;`capability.rs:683` | 写死 |
| C3 | HTTP 客户端兜底超时 | deadline 缺失时 120s | `openai_http.rs:352/474`;`glm_http.rs:137` | 写死 |

### D. 门户与管理面

| # | 限制 | 数值 | 证据【实锤】 | 可调性 |
|---|---|---|---|---|
| D1 | 门户登录防爆破 | 连续 5 次失败锁 15 分钟 | `portal.rs:28-29/351` | 写死 |
| D2 | 密码/会话 | 密码 ≥6 位;登录 Cookie 30 天(Max-Age=2592000) | `portal.rs:279/315/409` | 写死 |
| D3 | 文件预览 | 512KB | `webadmin.rs:66/1508` | 写死 |
| D4 | 打包下载(zip) | 总量 256MB / 条目 5000,超一即拒 | `webadmin.rs:1573/1920-1931` | 写死 |
| D5 | 批量删除 | 单次最多 100 项 | `webadmin.rs:1661-1671` | 写死 |
| D6 | 目录浏览(选择器) | 单目录最多列 1000 条 | `webadmin.rs:1726/1759` | 写死 |
| D7 | 上下文透视页 | 尾读 2MB / 120 条;跨会话检索 limit 钳 1~200(默认 50) | `webadmin.rs:2282-2288/2300-2304` | limit 参数可调 |
| D8 | 事件轮询 | limit 默认 100,钳 1~1000 | `handlers.rs:318` | 参数可调 |
| D9 | 检查更新/在线升级 | GitHub API 20s / 升级包下载 600s / 升级子进程绑定重试 60s | `about.rs:101/359`;`boenmind-server.rs:72` | 写死 |
| D10 | 管理面内部杂项 | 插件探活/重载等内部等待 5~10s | `webadmin.rs:415/1993`;`workspace_admin.rs:211` | 写死 |

### E. 基建隐含

| # | 限制 | 数值 | 证据 | 分级 |
|---|---|---|---|---|
| E1 | HTTP 请求体上限 | 2MB(axum 0.8 默认;全仓未见 DefaultBodyLimit 放宽) | `runtime/Cargo.toml:41` + rg 全仓无显式设置 | 【推断】未实测 |

---

## 三、VPS 场景冲突排行(疼的程度排序)

1. **exec 60s 铁顶**(A1)——本次 GitHub clone 死因;VPS 网络下 clone/冷编译/大下载必撞。
2. **工具轮 60s 等待 + 并发排队假超时**(A4)——BACKLOG §3 已登记,fs.read 洪峰时排队 >60s 被误判超时。
3. **MCP 工具默认 30s**(A12)——联网搜索、抓网页类工具在 VPS 上吃紧;可在 mcp.json 逐插件调。
4. **模型调用 120s**(A5)——网关慢时可 `BOEN_TURN_TIMEOUT_SECS` 上调(唯一官方环境变量旋钮)。
5. **流式 900s**(B1)——超过 15 分钟的单回合会被掐断;长分析任务要拆步。
6. **历史回喂 20 轮/24K 字符**(B7)——长会话后段模型「忘了」开头,属设计取舍非故障。

## 四、现成可调旋钮(不用改代码)

| 旋钮 | 调法 | 管什么 |
|---|---|---|
| `BOEN_TURN_TIMEOUT_SECS` | systemd `Environment=` 或启动环境(PLAYBOOK §4 已载) | 每次模型调用超时(默认 120s) |
| mcp.json `tool_timeout_ms` | 设置页插件配置 / 数据目录 mcp.json | 该 MCP 插件全部工具的单次调用超时(默认 30s) |
| mcp.json `restart_limit` | 同上 | MCP 子进程故障循环熔断阈值(默认 3) |
| skill manifest `timeout_ms` | 技能定义 | 单个技能脚本超时(默认 10s) |
| autorun `max_turns` | 调用参数 | 自动驾驶轮数(默认 6,上限 50) |
| fs.search `max_results` | 模型侧参数 | 搜索结果条数(默认 80,上限 500) |

VPS 上加环境变量的可粘贴步骤(以 systemd 为例):
```bash
sudo systemctl edit boenmind   # 加入:
# [Service]
# Environment=BOEN_TURN_TIMEOUT_SECS=240
sudo systemctl restart boenmind
```
注意:这只调**模型调用**超时,调不了 exec 的 60s(那是 A1,要改代码)。

## 五、必须改代码才能动的(建议与位置)

| 项 | 位置 | 候选修法 |
|---|---|---|
| exec 60s 铁顶 | `system_exec.rs:42` + `capability.rs:731` | A)manifest→300000;B)env 化。已登记 BACKLOG 待裁决 |
| 工具轮等待 60s/300s | `spawn.rs:534` | 随 BACKLOG §3 并发排队条目一起评估 |
| 流式 900s | `openai_compat.rs:359` | 如需长任务再上调 |
| 历史回喂 20 轮/24K | `turn/history.rs:4-6` | 加大=上下文费用上升,需权衡 |
| 5 次熔断阈值 | `spawn.rs:384-411` | 不建议动;误伤靠参数微变规避 |
| 各类大小上限(16MB/256MB/5000 条/512KB…) | 见清单 D/A 区 | 按需逐个调,均一行改动 |

## 六、未核实清单

- E1 axum 2MB 默认请求体上限:依据 axum 0.8 框架默认行为 + 全仓无显式放宽的 rg 证据,**未实测**(可发 >2MB POST 验证 413)。
- VPS 上是否已设 `BOEN_TURN_TIMEOUT_SECS`:按「VPS 不主动访问」纪律未登录核实。
- MCP 插件内部超时(如 web-multisearch 对上游搜索引擎的超时):不在本轮范围,插件仓内另行盘点。
- 前端除 60s 看门狗外的隐性限制(如输入框长度):未逐盘。

## 七、复核指引(供跨 AI 验证)

复现扫描所用模式(仓库根执行):
```bash
rg -n "from_secs\(|from_millis\(" runtime/crates --type rust -g '!target/**'
rg -n "clamp\(|MAX_|_LIMIT" runtime/crates --type rust -g '!target/**'
rg -n "熔断|watchdog|truncat" runtime/crates --type rust -g '!target/**' -i
```
每条清单的证据列可直接 `文件:行号` 打开对照;数值如与源码不符以源码为准并回报。
