# REVIEW_B — BoenMind v2 假设校验报告（code-review 深度审读）

> 审读者：架构师 B（code-review SKILL Phase 1 深度审读模式）
> 日期：2026-08-17
> 评审对象：`docs/design/DSH_PROJECT_V2_2026-08-17.md`（下称"计划"）
> 审读材料：
> - dsh 官方源码 `D:/96_CoderWorld/deepseek-harness`（TS/Cordis，npm 0.1.0-rc.6）
> - bobleer Rust 版 `D:/96_CoderWorld/bobleer-dsh-rust`（1 commit：`4abdbf9`）
> - 计划原文 `D:/96_CoderWorld/BoenMind/docs/design/DSH_PROJECT_V2_2026-08-17.md`
> 范围：只审读、不修复、不改任何源文件。
> 结论摘要：**计划假设 ① 基本属实但有两处关键方向性错误（mux 方向写反；trust fence 的 loopback-pin 特权面被漏掉）；② 漏了至少 5 个对外面（/api/respond、GET /api/session.export、/plugins/events HMR、SSE 备选 carrier、__ModuleLoader__）；③ bobleer 分层可借鉴其 crate 边界与 port 抽象，但其"缺位"恰是计划要做的最核心部分，只能当内核语义样板，不能当 web-server 样板。**

---

## 〇、一句话总结

dsh 的"接口合同"不是 6 面，而是 **HTTP POST RPC envelope 面 + 两条 WS downlink 面 + SSE 备选 carrier 面 + 静态 SPA 面 + 两个 boot 协议面 + 一个 HMR SSE 面 + trust fence 双栅栏（Host 栅栏 + loopback-pin 特权面）**。计划的 6 面表格有 2 处事实错误、漏掉 5+ 个面；事件"挂点"须按三层区分（wire 层 Mux/Host 帧 / 持久化 SessionEvent 层 / 进程内 cordis 事件层），计划把进程内事件（agent/pre-step、tool/execute）错当成对外契约引用。bobleer 的 contracts→execution→services→adapters→assembly→apps 分层和 ports 抽象值得借鉴，但它**没有 web-server/HTTP/WS/RPC/前端协议任何实现**，且缺 scope/dynamic loading/事件四模式，作为"兼容层样板"毫无覆盖。

---

## 一、逐项校验表：计划假设 → 代码证据 → 判定

| # | 计划假设/表述 | 代码证据 | 判定 |
|---|---|---|---|
| H1 | 合同共 6 面：HTTP `/api/*` | `packages/client/connection/src/api-path.ts:8` `API_PATH='/api'`；`rpc-host.ts:144-188` 每 endpoint 是 **POST + envelope（ClientRequest full form）**，business 错误恒 200 + ServerResponse | **证实但需修正**：`/api/*` 不是普通 REST，是 POST envelope RPC（`{type:'client-request',rpcId,method,payload}`），方法名即路径段（`POST /api/session.list`）。HTTP status 只表达 carrier 层（404/415/400/500），业务错误 200。另有 `POST /api/respond`（client-response 上行，非 RPC 方法，见 rpc-map.ts:2-4 注释 + fetch/handler.ts:296-300）——**计划漏了这半面** |
| H2 | WS `/api/events.mux` = "浏览器→宿主 上行复用流（mux-frame，一连接多路请求）" | `websocket-downlink.ts:109-111` 客户端发消息 → `websocket.close(1008,'downlink only')`；`events.ts:47-56` mux 是 `AsyncIterable<RpcRequest<MuxFrame>>` 下行；`connection.ts:128` 浏览器侧 `api.events.mux()` 是 pump 读取 | **证伪（方向写反）**：mux 是**宿主→浏览器**的下行聚合事件流（"mux"=多会话复用），不是浏览器→宿主的上行。上行只有 HTTP POST。计划 §3.2 面 2 的"浏览器→宿主 上行复用流"把方向完全写反 |
| H3 | WS `/api/events.host` = 宿主→浏览器 下行事件流（两条 server-to-browser 流） | `events.ts:127-155` HostFrame 9 变体；`websocket-downlink.ts:77-82` handleHost 泵送 `api.events.host()` | **证实**：host 是全局信息流（session 增删/运行态翻转/workspace）。但注意两流都在 `packages/host/apiproxy/src/fetch/handler.ts:254-258` 有 **SSE 备选 carrier**（GET 直出 text/event-stream），计划漏了 |
| H4 | 静态 SPA + 405/403/200 兜底/octet-stream | `frontend-static/src/index.ts:37-86`（MIME 表 + `403` traversal + 200 SPA fallback + `application/octet-stream`）；`:101-105` 非 GET/HEAD 405；`webserver/index.ts:125-131` fallback 单座 | **证实**（边界行为逐字符合计划 §3.4） |
| H5 | `/plugins/<id>/client.js` 前端 bundle（`__ModuleLoader__` 注册） | `client/modules/src/index.ts:421-457` serveBundle（405/404/`client.js.map`）；`client/modules/src/client/manifest.ts:159-171` `__ModuleLoader__`、`__DSH_BOOT__`、`__DSH_MODULES__` 三个 window 槽 | **证实但需修正**：boot 协议实为 **3 个 window 槽**（`__DSH_BOOT__` 数据 + `__ModuleLoader__.load` 注册 + `__DSH_MODULES__` kernel handoff），且 `url` 带 `?rev=` cache-busting，`/plugins` 前缀是**所有** bundle 共用的 prefix 路由（含 scope 斜杠名），计划只写了 `__DSH_BOOT__` 一面 |
| H6 | `__DSH_BOOT__` 注入 index.html（tapIndex 变换） | `client/modules/src/index.ts:168-175` injectBootManifest（`<head>` 首个 script，`<` 转义）；`webserver/index.ts:139-145` tapIndex 注册；`frontend-static/index.ts:96-97` renderIndex 应用 | **证实** |
| H7 | trust fence：只信 loopback + `--trusted-host` | `client/connection/src/api-request-trust.ts:96-123` Host 栅栏 + sec-fetch-site + Origin 栅栏；`loopback-hostname.ts` | **证实但需修正**：还有 **PRIVILEGED_METHODS 20 个方法的 loopback-pin**（`index.ts:89-119,145-149`：settings.*、credentials.*、agentPreset.read/copy/openDocument/remove、host.pickDirectory/openPath、llm.discoverModels 即使 trustedHosts 部署也强制 loopback）。计划说"直接沿用（绑定 127.0.0.1 默认姿态）"，但**双栅栏**里第二个栅栏（方法级 pin）被漏掉——LAN 部署时这是行为分叉点 |
| H8 | 挂点集合 = agent/*、turn/*、step/*、tool/* 事件 + ui-slots 槽位，是对外契约 | 见 §二清单 | **需修正（最重要）**：事件分三层。**wire 层**只有 MuxFrame 9 种 + HostFrame 9 种；**持久化层** SessionEvent 46 种（known-event-types.ts）；**进程内 cordis 层** agent/* 11 个 + tools/* 5 个 + 若干（只同进程插件可见，**不上 wire**）。ui-slots 是**纯前端编译期契约**（SlotMap declare module 合并，与后端无关）。计划 3.4 引用的 `agent/pre-step`、`turn/stopping`、`tool/execute` 全是**进程内 cordis 事件**，不在 wire 上——Rust 兼容层只需对 wire 帧逐字一致，进程内事件是宿主内部自由实现 |
| H9 | bobleer 分层 crate = "该抄的抄、该砍的砍"的样板 | 见 §三 | **部分证实**：crate 分层与 port 抽象值得借鉴；但**没有 web-server/RPC/前端协议任何实现**，作为计划最大工程点（web-server 兼容层）的样板覆盖为零 |
| H10 | 计划 §3.4 "挂点集合一致——一个不多一个不少" | Mux/Host 帧有 `stream/error`、`session/queue`、`session/jobs`、`session/projection` 等控制帧 | **需修正**：挂点不是"一个不多一个不少"能穷举的——`session/queue`/`session/jobs` 是**全量快照帧**，`host/remote-event` 是**开放转发通道**（allowlist 在 api/remotes，未来可加），`session/projection` 的 value 是 wide 开放槽。这些开放/扩展点在计划里没提 |

---

## 二、dsh 实际挂点/契约清单（计划漏掉的已标注）

### 2.1 wire 层：MuxFrame（mux 下行流帧，events.ts:69-108 / events.schema.ts:43-67）

| 帧类型 | 负载摘要 |
|---|---|
| `session/event` | `{sessionId, event: SessionEvent, view?: ToolEventView}`（原始会话事件透传，view 为宿主计算的渲染意图，**不持久化**）|
| `session/subscribed` | `{sessionId, lastSeq}`（重连基线）|
| `approval/requested` | `{sessionId, approvalId, toolName, callId?, reason?}`（**answerable server-request，rpcId 复用**）|
| `approval/resolved` | `{sessionId, approvalId, outcome: allowed-once\|rejected\|cancelled\|unavailable}` |
| `question/requested` | `{sessionId, questions: AskUserQuestionItem[]}`（answerable，rpcId 复用）|
| `question/resolved` | `{sessionId, questionRpcId, outcome: answered\|cancelled}` |
| `session/queue` | `{sessionId, items: {id, placement: queued\|steering\|context, message}[]}`（**全量快照**，非事件）|
| `session/jobs` | `{sessionId, jobs: JobView[]}`（**全量快照**）|
| `session/projection` | `{sessionId, key, value: unknown, seq}`（**开放值槽**）|
| `stream/error` | `{error: RpcError}` |

### 2.2 wire 层：HostFrame（host 下行流帧，events.ts:127-155）

`host/session-added`（含 `blank/parentSessionId/origin:'subagent'/cwd/agentPreset`）、`host/session-removed`、`host/session-status{running}`、`host/agent-error`、`host/workspace-changed`、`host/workspace-removed`、`host/workspace-order-changed`、`host/archived-sessions-changed`、`host/remote-event{event, args}`（**开放转发通道**，allowlist 见 api/remotes/src/remote-events.ts:17-29 共 11 事件）、`stream/error`。

### 2.3 持久化层：SessionEvent（会话日志事件，known-event-types.ts:19-64 共 46 种）

- **core 核心 14 种**（core/session/src/types.ts:236-333）：`turn/start`、`turn/end{reason}`、`step/start`、`step/end`、`user/message`、`assistant/chunk`、`assistant/message{usage?}`、`tool/call{callId,name,arguments}`、`tool/result{message,error?,meta?}`、`todo/write`、`request/header`、`request/context`、`session/end-seed`（外加扩展 `agent/inbox/spliced`）。
- **扩展 32 种**（插件/外部包注册）：`agent-preset/selected`、`approval/asked/decided/policy`、`command/run/done`、`compaction/start/prune/summary/end`、`feedback/record`、`goal/change`、`hook/invoked/result`、`llm/retry/retry-started`、`permission/preset`、`plan/mode`、`sandbox/mode`、`schedule/change`、`session/title(-llm-request)`、`subagent/descriptor`、`tool-workflow/agent-start/agent-end/run-start/run-end`、`tool/code-dispatch(-start)`、`web/deepseek-search-llm-request`。
- **注意**：`SessionEvent` 信封是 `{type, seq, time, data, sourceEventSeqs?, surfaceOp?, ignorable?}`，schema 是**严格信封 + wide data**（sessions.schema.ts:41-49），`ignorable` 是版本演进逃生口。

### 2.4 进程内 cordis 事件层（**不上 wire，宿主内部自由实现**，计划误当对外契约）

- **agent/\* 11 个**（core/agent/src/runtime-types.ts:159-290 + dispatch.ts）：`agent/created`、`agent/disposed`、`agent/status`、`agent/inbox/inserted/claimed/discarded`、`agent/session-start`、`agent/pre-step`（waterfall）、`agent/request`（waterfall）、`agent/request-error`（waterfall）、`agent/turn-stopping`（serial）、`agent/error`。
- **tools/\* 5 个**（core/tools/src/index.ts:142-207）：`tools/pre-execute`（waterfall）、`tools/execute`（waterfall）、`tools/post-execute`（waterfall）、`tools/code-dispatch-log`（waterfall）、`tools/result`（emit）、`tools/change`（emit）。
- **其他**：`session/created`、`session/disposed`、`session/event`、`session/flush`（**parallel 模式**，core/session/src/index.ts:83）、`subagent/start/end`、`system-prompt/assemble`、`approval/request`、`goal/changed`（scope 路由表见 core/scope/src/scoped-events.generated.ts:10-37）。
- **四模式**：emit / serial / waterfall / **parallel**（parallel 仅 `session/flush`）。

### 2.5 计划漏掉的对外面（H 级修正项）

1. **`POST /api/respond`**（fetch/handler.ts:296-300 + rpc.ts:179-183）：approval/question 应答上行（ClientResponse 带 rpcId），返回 `RpcReceipt`。无此面则审批/提问交互全断。
2. **`GET /api/session.export?sessionId=...`**（downloads.ts + fetch/handler.ts:260-271）：session log ZIP 下载，host-only、无 envelope，计划 §二没提。
3. **`/plugins/events` HMR SSE 通道**（client/hmr/src/index.ts:166-191）：graph/rebuilt 帧广播，`EVENTS_ENDPOINT`。dev 下前端热更新依赖。
4. **SSE 备选 carrier**（fetch/handler.ts:254-258）：`GET /api/events.mux`/`/api/events.host` 也接受 SSE（EventSource），供 in-process/Electron carrier。浏览器主路径用 WebSocket；**Rust 版若只实现 WebSocket，Tauri/file:// IPC 或未来非浏览器消费方会断**。
5. **`window.__ModuleLoader__` 与 `window.__DSH_MODULES__`**（manifest.ts:159-171）：前端插件注册与 kernel handoff 槽，计划只写了 `__DSH_BOOT__`。
6. **`/plugins/<id>/client.js.map`**（modules/src/index.ts:434-439）：source map 单独后缀路由。
7. **trust fence 的 loopback-pin 特权表**（connection/src/index.ts:89-119）：20 个方法在 trustedHosts 部署下仍强制 loopback。
8. **图片上传无独立面**：是 `session.prompt` payload 里的 base64 `{type:'image',mediaType,data,name}`（sessions.schema.ts:282-285），受 `160MiB` body cap 约束（http-bridge.ts:12）。

---

## 三、bobleer 分层样板评估

### 3.1 实际状态（1 commit `4abdbf9`，~8390 行 Rust）

- **分层真实存在且完整**：contracts（core-types/events/runtime-ports）→ execution（agent-stream/session/system-prompt/tool-contracts/agent-runtime/agent-loop）→ services（credentials/persist/fs/subprocess/shell）→ adapters（llm-deepseek/llm-mock）→ assembly（core）→ interfaces（acp）→ apps（cli）。`docs/architecture.md` 与 `check-crate-boundaries.py` 强制单向依赖。
- **README 定位诚实**："This is not a Cordis port. Composition is Cargo features plus an assembly crate."（delivery profile = Cargo feature 闭包，`assembly/core/Cargo.toml`）。
- **实现程度**：turn/step 循环、session 日志（15 种事件 body，非 46）、EventBus（emit/waterfall/serial 三模式）、ports 抽象完整、ACP stdio server、DeepSeek SSE adapter、CI（fmt/clippy/test/dump-config/边界脚本）。

### 3.2 值得借鉴（可抄）

| 点 | 证据 | 对我们的价值 |
|---|---|---|
| crate 边界单向依赖 + 静态检查脚本 | `scripts/check-crate-boundaries.py` | 直接抄——BoenMind 微内核同款纪律 |
| ports 抽象（LlmPort/FsPort/ShellPort/SubprocessPort/CredentialsPort/SessionPersistPort/PluginRuntimePort） | `contracts/runtime-ports/src/lib.rs:66-216` | `PluginRuntimePort::availability()` fail-loud 的"未注册即明确失败"模式契合 supervisor 计划 |
| EventBus 的 Disposer 生命周期与 waterfall/serial/emit 三模式 | `contracts/events/src/bus.rs:138-238` | 事件系统骨架可借鉴，但见 3.4 缺位 |
| 装配 = Cargo features + assembly crate（不做运行时动态加载） | `assembly/core/src/lib.rs` | 与计划"编译产物分发"路线一致 |
| 错误分层（PortError kind + fail-loud） | runtime-ports:17-50 | 插件/工具门控可复用 |

### 3.3 坑（抄的时候要避开）

1. **EventBus 无 scope**：bobleer 的 bus 是**全局单总线**（bus.rs:71-77），没有 dsh 的 `createScope`/agent 作用域过滤（dsh core/scope/src/）。多 agent 并发时事件路由不到目标 agent——BoenMind 若做进程内多会话，必须加 scope。
2. **事件四模式只实现三模式**：缺 `parallel`（dsh 的 `session/flush` 用）。bobleer 只有 emit/waterfall/serial。
3. **tool-contracts 只有 execute**，没有 pre-execute/post-execute/code-dispatch-log 三个 waterfall（对比 dsh core/tools/src/index.ts:142-207）——拦截器（审计/门控/超时）无处挂。
4. **持久化只有 JSONL save/load 整串**（SessionPersistPort:179-182），无 sqlite、无 session-query/搜索。
5. **错误处理粗糙**：agent-loop 大量 `Result<_, String>`（lib.rs:248 等），错误信息丢失类型。
6. **无 web-server/HTTP/WS/RPC/前端协议任何实现**——这对计划的最大工程点（兼容层）完全无样板价值。

### 3.4 缺位（计划要借鉴 bobleer 但 bobleer 没有的部分）

- **dynamic loading**：`PluginRuntimePort` 是占位 `NotAvailable`（runtime-ports:186-205），"later Plugin Host" 只是注释。计划的 supervisor/插件进程化没有样板。
- **scope**：无。
- **事件四模式**：无 parallel。
- **REST/WS/envelope 层**：无。计划 §三 web-server 兼容层必须另起炉灶，bobleer 一点帮不上。
- **settings/credentials/workspace/attachment/session-title/session-search/goals/subagents/jobs/projection**：全部缺失。

**结论**：bobleer 作为"内核语义 + crate 分层 + 边界纪律"样板合格；作为"web-server 兼容层"样板不合格（覆盖率 0%）。计划 §三把 bobleer 当主要参照物之一，应降级为仅内核层参照。

---

## 四、按 code-review 维度分类的关键发现（含严重级）

### ARCH（架构）

- **ARCH-001 [严重-Critical]**：计划 §3.2 面 2 把 `/api/events.mux` 方向写反（"浏览器→宿主 上行复用流"），实际是宿主→浏览器下行聚合流，且 WS 为 downlink-only（客户端上行消息被 close(1008)）。方向性错误会让 M2 门禁 2 的实现/验收走错路。证据：`websocket-downlink.ts:109-111`、`events.ts:47-56`。
- **ARCH-002 [严重-Critical]**：计划的"挂点集合一致"将三层事件混为一谈。对外（wire）契约只有 MuxFrame/HostFrame + SessionEvent 信封；`agent/pre-step`、`tool/execute`、`turn/stopping` 是进程内 cordis 事件，Rust 宿主不需要逐字复刻。若照计划"逐字一致"去实现进程内事件，是巨大浪费。证据：`runtime-types.ts:231-290` 与 `api/events.ts` 分属不同层。
- **ARCH-003 [高]**：漏掉 `POST /api/respond` 与 `GET /api/session.export` 两个 HTTP 面；漏掉 SSE 备选 carrier（in-process/Electron/Tauri 场景）。证据：`fetch/handler.ts:254-300`。
- **ARCH-004 [高]**：boot 协议实为 3 个 window 槽（`__DSH_BOOT__`/`__ModuleLoader__`/`__DSH_MODULES__`）+ `?rev=` cache-busting + `/plugins/*/client.js.map`，计划只列 `__DSH_BOOT__` 一面。证据：`manifest.ts:159-171`、`modules/src/index.ts:434-439`。
- **ARCH-005 [中]**：`session/queue`、`session/jobs` 是**全量快照帧**而非增量事件；`session/projection` 与 `host/remote-event` 是开放扩展槽。兼容层必须支持"全量快照语义"（重连收敛），计划未提。证据：`events.ts:84-107`。

### SEC（安全）

- **SEC-001 [高]**：计划漏掉 trust fence 第二道栅栏——**20 个特权方法 loopback-pin**（`settings.*`、`credentials.*`、`agentPreset.read/copy/openDocument/remove`、`host.pickDirectory/openPath`、`llm.discoverModels` 等，即使 trustedHosts 部署仍强制 loopback）。Rust 兼容层若不实现，LAN 部署时配置/密钥面直接暴露；若实现而前端感知不到，行为分叉。证据：`client/connection/src/index.ts:89-119,145-149`。
- **SEC-002 [中]**：frontend-static 的 traversal 检查用 `resolve(normalize(join(distRoot, pathname)))` + `sep`（Windows 路径），且 `decodeURIComponent` 在 webserver 层之后（frontend-static:60-64,108）。Rust 版必须复制 `sep` 语义与 decode 时序，否则路径穿越/编码差异。证据：`frontend-static/src/index.ts:60-64`。
- **SEC-003 [中]**：api-request-trust 的 `assertTrustedAuthority` 拒绝非 canonical 条目（`index.ts:54-58`）——恶意 `--trusted-host` 配置会在 load 期 loud fail。兼容层应保留该 fail-loud 语义。

### PERF（性能）

- **PERF-001 [中]**：`http-bridge.ts` 每请求在内存缓冲整个 body（cap 160MiB）后构造 fetch Request；WS 下行 `pump` 逐帧 `JSON.stringify` 串行 send 且无背压（websocket-downlink.ts:23-34,118-137）。Rust 版需自行设计等价 body 上限与 WS 背压，不能照抄 Node 桥。
- **PERF-002 [低]**：`client/modules` 的 `pkgMeta` 缓存"永不失效"（modules/src/index.ts:190-191）——插件集变化要重启才生效。计划若在 Rust 宿主支持运行时插件集变更，此处语义要显式决策。

### QUAL（质量）

- **QUAL-001 [中]**：dsh 每 endpoint 有 zod schema（双端解析：宿主 side 严格、客户端 side 也严格，`fetch/client.ts:348` 二段 parse）。契约台账 `CONTRACT_LEDGER_DSH.md` 应把 schema 当作源，而不是手抄 TS 接口——Rust 版用 JSON Schema（bobleer 已用 `jsonschema` crate）可自动从 zod 输出对齐。
- **QUAL-002 [低]**：`HostFrame`/`MuxFrame` 的 `session/event` 载荷是"严格信封 + wide data"，未知事件类型靠 `ignorable` 逃生。Rust 版事件模型必须保留 wide-data + ignorable，不能枚举死。

### BUG（潜在 bug）

- **BUG-001 [高]**：计划 §3.2 面 2 方向错误若进入 M2 实现，会在"上行 mux"上投入错误实现。属于计划级 bug。
- **BUG-002 [中]**：`webserver/index.ts:152` 用 `new URL(req.url).pathname` 取路径（未 decode），而 frontend-static 里再 `decodeURIComponent`（:108）——**decode 只做一次且在静态服务层**。Rust 路由层如果先 decode，`%2F` 等路径会分叉（路由匹配与静态服务的 decode 位置不同）。证据：webserver:152 vs frontend-static:108。
- **BUG-003 [低]**：WS upgrade 的 403 拒绝发生在 trust 检查失败时（connection/src/index.ts:184-187），而 HTTP 前缀路由 403 在 bridge 层（:165-169）——同 fence 两种拒绝路径，Rust 版需保持一致。

### IMP（改进/建议）

- **IMP-001 [高]**：契约台账应该直接以 `packages/host/apiproxy/src/api/*.schema.ts`（zod 权威）为源自动生成，而非人工抄写——计划 §3.4 的 CONTRACT_LEDGER 建议改为"从 zod schema 生成 Rust 类型 + JSON Schema 双出"。
- **IMP-002 [中]**：`host/remote-event` 转发 allowlist（api/remotes/src/remote-events.ts:17-29）是开放通道——Rust 版应把它设计成可配置白名单，而不是固定 11 事件。
- **IMP-003 [中]**：`llm.discoverModels` 让宿主向客户端指定 URL 发 GET（index.ts:79-83 注释），是 SSRF 风险点，被 pin 在 loopback。Rust 兼容层必须同样 pin，且文档注明原因。

---

## 五、给架构文件的修正建议（分级）

### P0（开工前必须改）

1. **修正 §3.2 面 2 方向**：`/api/events.mux` = 宿主→浏览器下行聚合流（多会话复用），浏览器不在此面上行；上行 = HTTP POST。§3.2 表格第 2 行改述。
2. **新增面**：在 6 面之外补为 **9 面**：①HTTP POST RPC envelope（含 `/api/respond` 应答上行）②GET `/api/session.export` 下载 ③WS `/api/events.mux` 下行 ④WS `/api/events.host` 下行 ⑤SSE 备选 carrier（GET events.mux/host，Electron/in-process）⑥静态 SPA ⑦`/plugins/*/client.js(.map)` ⑧`__DSH_BOOT__`+`__ModuleLoader__`+`__DSH_MODULES__` boot 协议 ⑨`/plugins/events` HMR SSE。外加 trust fence **双栅栏**（Host 栅栏 + 20 方法 loopback-pin）。
3. **§3.4 挂点集合改写为三层**：wire 层（MuxFrame 9 + HostFrame 9 + SessionEvent envelope wide-data+ignorable）逐字对齐；进程内 cordis 层（agent/*、tools/*）为宿主内部自由实现，不承诺对外；`session/queue`、`session/jobs`、`session/projection`、`host/remote-event` 按全量快照/开放槽语义实现。

### P1（M2 规划期）

4. **契约台账改为 schema 驱动**：以 `packages/host/apiproxy/src/api/*.schema.ts` 为权威源，自动生成 Rust 类型与 JSON Schema；验收从"逐字对齐"改为"以官方 zod schema 生成的 JSON Schema 双向校验 + 真实前端差分验收"。
5. **图片上传面写清**：`session.prompt` 内 base64 + 160MiB body cap + 规范 base64 校验（api-proxy.ts:142-148），不是独立上传接口。
6. **路径处理时序写进契约**：路由匹配用未 decode pathname，decode 只在静态服务层一次（webserver:152 vs frontend-static:108）。
7. **明确 SSE 是否一期实现**：浏览器主路径只需要 WebSocket；SSE 仅 Tauri/in-process 需要。建议 P0 只做 WS，SSE 在 Tauri 壳期补，但契约台账仍要记录。

### P2（细化/文档）

8. **bobleer 定位降级**：从"兼容层样板"改为"内核语义 + crate 边界样板"；兼容层在 bobleer 中覆盖率为 0%。
9. **scope/事件四模式/动态加载**：bobleer 缺这三样；计划 M1 微内核应在内核期就内建 agent scope（Rust 版 `createScope`），否则多会话并行时事件路由失真。
10. **`llm.discoverModels` 等特权方法**在 Rust 版必须 loopback-pin，并在 CONTRACT_LEDGER 中列出完整 20 方法表（避免 LAN 部署安全回归）。

---

## 附：证据文件索引（绝对路径）

- dsh 客户连接：`D:/96_CoderWorld/deepseek-harness/packages/client/connection/src/api-path.ts`、`websocket-downlink.ts`、`http-bridge.ts`、`api-request-trust.ts`、`rpc-host.ts`、`rpc.ts`、`client/web-api-client.ts`、`client/connection.ts`、`client/rpc.ts`
- dsh host webserver：`packages/host/webserver/src/index.ts`
- dsh SPA：`packages/host/frontend-static/src/index.ts`
- dsh 前端模块：`packages/client/modules/src/index.ts`、`packages/client/modules/src/client/manifest.ts`
- dsh HMR：`packages/client/hmr/src/index.ts`
- dsh api-gateway：`packages/api/gateway/src/index.ts`
- dsh apiproxy 合同：`packages/host/apiproxy/src/api/index.ts`、`api/rpc-map.ts`、`api/rpc.ts`、`api/events.ts`、`api/events.schema.ts`、`api/sessions.schema.ts`、`api/host.ts`、`api/settings.ts`、`api/downloads.ts`、`fetch/handler.ts`、`fetch/client.ts`
- dsh 核心挂点：`packages/core/agent/src/runtime-types.ts`、`dispatch.ts`、`packages/core/agent-loop/src/agent.ts`、`tool-calls.ts`、`packages/core/session/src/types.ts`、`known-event-types.ts`、`packages/core/tools/src/index.ts`、`packages/core/scope/src/scoped-events.generated.ts`
- dsh 转发白名单：`packages/api/remotes/src/remote-events.ts`
- dsh 装配：`packages/bundle/web-app/cordis.patch.yml`、`packages/bundle/web-app/src/index.ts`、`startup.ts`
- bobleer：`D:/96_CoderWorld/bobleer-dsh-rust/Cargo.toml`、`docs/architecture.md`、`docs/design.md`、`src/crates/contracts/events/src/bus.rs`、`session_event.rs`、`src/crates/contracts/runtime-ports/src/lib.rs`、`src/crates/execution/tool-contracts/src/lib.rs`、`src/crates/execution/agent-loop/src/lib.rs`、`src/crates/execution/session/src/lib.rs`
