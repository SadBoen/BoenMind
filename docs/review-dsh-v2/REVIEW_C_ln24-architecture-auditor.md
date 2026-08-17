# REVIEW C — ln-24 架构适配审计（dsh 官方 × bobleer Rust vs 计划 v2 采用方式）

日期：2026-08-17
审计人：架构师 C（按 ln-24-architecture-auditor SKILL 方法论，只读审计）
审计对象：**已实现系统** `D:/96_CoderWorld/deepseek-harness`（TS/Cordis）与 `D:/96_CoderWorld/bobleer-dsh-rust`（Rust，1 commit）
对照上下文：`D:/96_CoderWorld/BoenMind/docs/design/DSH_PROJECT_V2_2026-08-17.md`（仅作"采用意向"读，不作为被审计对象）

**Checklist: 24/24 complete**
**Incomplete: None**

说明（达成方式）：SKILL 五段 checklist 全部执行完毕；边界脚本 `check-crate-boundaries.py` 在本机（Windows 无 python）无法原生运行，已用同逻辑的 Node 仿真验证（20 个 manifest，downward-only 通过），并逐条人工核对 Cargo.toml 依赖；运行期 WS/SSE 语义为静态追踪（有明确代码路径），无运行时观测。

---

## 总览结论（两系统 × 计划采用方式）

计划 v2 的三条采用路线：**①前端全套借 dsh 生态（web-server Rust 兼容层逐字复刻前端契约）②借鉴 bobleer 分层 crate 样板 ③自建 Rust 微内核 + 插件进程化 + 状态外置**。

- **①适配度：CONCERNS（偏高，但有契约事实错误与空白面）**。dsh 前端契约真实存在、可逐行读、组合式（每面由独立包注册）——计划 §3.2/§3.3 的可行性判断成立。但计划对契约的**形状与所有权**有事实错误（见 P1-A/B/C/D），且遗漏了两个大契约面（settings 配置面、credentials 凭据面）与 trust fence 的完整语义。契约的"所有者"确实分散，但分散方式比计划描述的更深：物理路由在一处（client-connection），业务词汇在领域包（dsh-session/dsh-tools/dsh-llm 等），线框 schema 又单独成包。这对 Rust 兼容层的含义是：**契约台账必须按"面 → 物理归属 → 词汇归属 → schema 归属"四层记，不能按包平铺**。
- **②适配度：CONCERNS（样板本身健康，但照搬会继承三个缺口）**。bobleer 分层是**真实落地**（20 crate 边界脚本通过、事件总线模式完整实现、model-visible⟺logged 断言有代码），不是只有文档。但它缺 dsh 的三个语义：**平行事件模式/冲刷屏障（session/flush）**、**作用域过滤分发（scope）**、**动态/可替换加载（PluginRuntimePort 只有 Unavailable 变体）**。更关键的是其**持久化只在任务结束时整写 JSONL（tmp+rename），无崩溃恢复、无 torn-tail 修复**——若照搬为微内核事实源，直接违反"Linux 服务器长期运行 + 状态外置"目标。
- **③（对照参考）**：dsh 的"原子性所有者"在**持久化后端**（fsync + link 发布 + torn-tail 修复），dsh 的可替换缝（compaction/subagent/storage/session-persistence）是**进程内 provider 注册表**，与计划要自建的**进程级 supervisor 隔离**是两套机制——前者是前端不可见的宿主内部缝，后者是分发/隔离缝。二者不可混用，计划的 M3 插件进程化与 dsh 前端契约无冲突（前端只钉词汇与线框，不钉宿主内组装方式）。

**Verdict: CONCERNS**（无 P0 正确性/安全缺陷，存在多处 P1 契约事实错误与缺失面，均为"实现前必须修正"，不阻塞方向本身）。

---

## 1. 发现分级总表

| # | 级 | 问题 | 证据（路径+行号） | 对计划 v2 的后果 |
|---|---|---|---|---|
| A | **P1** | §3.2 契约表两处**形状错误**：face1 不是 REST，是 POST-only RPC 信封；face2 mux 方向写反 | `packages/client/connection/src/client/rpc.ts:25-45`；`packages/host/apiproxy/src/api/rpc.ts:152,172`；`packages/client/connection/src/websocket-downlink.ts:49`"Client messages are a protocol violation: upstream traffic remains on HTTP"；`client/connection/src/index.ts:150` GET 两个 WS 路径回 426 | 按计划表实现会做错上行通道与 HTTP 方法语义；M2 合同台账必须以源码为准改写 |
| B | **P1** | 契约"所有者"列错误/不全：`/api` 路由在 **client-connection** 不在 api-gateway；事件流由 **host-apiproxy** 生产（不是 agent/session 插件直发）；RPC 分发在 **dsh-api-gateway**（Typert）；host-apiproxy 与 api-gateway 是两个网关 | `packages/client/connection/src/index.ts:161-173,193-194`；`packages/api/gateway/src/index.ts:104-111`；`packages/host/apiproxy/src/index.ts:69-126`；`packages/bundle/web-app/cordis.patch.yml:97-101,165-166` | 台账四层归属必须重画（§3）；否则 Rust 兼容层找不到契约真正的权威源码 |
| C | **P1** | 遗漏最大契约面：**settings 配置面**（describe/update/replace/mutate + 修订冲突 + secret 脱敏）与 **credentials 凭据面**（set/unset/describe）是前端 ui-settings/ui-settings-* 直接消费的 RPC，且被 **PRIVILEGED_METHODS 钉死在 loopback** | `packages/client/connection/src/index.ts:89-119`；`packages/settings/settings/src/index.ts:479-575`（describe/update/replace/mutate）、`redact.ts`；`packages/credentials/credentials-local/src/index.ts` | M2 若不做这两面，设置页白屏/禁写，凭据页不可用；trust fence"直接沿用"低估了 loopback 钉扎语义（§4） |
| D | **P1** | 原子性缺口：bobleer 持久化**只在任务结束整写**（`runtime.rs:306` save_session），无 append-only 运行中落盘、无 torn-tail 修复；dsh 的原子性所有者是持久化后端（write-behind 批 + fsync + link 发布 + commitRepair） | `bobleer/src/crates/assembly/core/src/runtime.rs:299-313`；`bobleer/src/crates/services/persist/src/lib.rs:28-60`；`dsh/packages/session/session-persistence-jsonl/src/index.ts:436-444,529-569,651-689,292-304` | M1 若借 bobleer 样板做"append-only SessionEvent 唯一事实源"，会继承无崩溃恢复；与"Linux 长期运行 + 状态外置"矛盾。微内核必须实现 committed-prefix + 修复语义 |
| E | **P2** | 事件模式/语义缺口：dsh 五种分发模式（emit/parallel/serial/bail/waterfall，`vendor/cordis/src/events.ts:32`），bobleer 只有 emit/waterfall(2 种)/serial；**session/flush 平行持久化屏障**（`packages/core/session/src/index.ts:78-85`）与 **scope 作用域过滤分发**（`packages/core/scope/src`，session/event 按 agent 过滤）在 bobleer 缺失 | `bobleer/src/crates/contracts/events/src/bus.rs:65-76`（只有 emit/waterfall_pre_step/waterfall_request/serial_turn_stopping）；`packages/core/session/src/index.ts:42-86,48`"Scope-filtered dispatch" | 内部实现可自由，但"model-visible⟺logged"的冲刷时机与多会话事件路由语义需自研；兼容层 mux 的 per-session 订阅基线（session/subscribed lastSeq）必须实现 |
| F | **P2** | 动态性缺位：bobleer 组合是编译期 feature 闭合，`PluginRuntimePort` 只接 `UnavailablePluginRuntime`；计划 M3 要进程化插件——bobleer 只提供了**端口占位**，无动态加载/运行期注册可借鉴 | `bobleer/src/crates/assembly/core/src/runtime.rs:88,230-245`；`docs/design.md:14`"不移植 Cordis/Loader/HMR/插件树"；`docs/architecture.md:12`"a later Plugin Host would sit behind PluginRuntimePort" | 借鉴价值=端口形状（fail-loud NotAvailable 语义很好），不是实现；supervisor 需全新设计，别指望样板 |
| G | **P2** | 边界脚本盲区：`check-crate-boundaries.py` 只查 `dsh-*` workspace 依赖且方向编号让 **interfaces 依赖 assembly 合法**（dsh-acp → dsh-core 携 headless feature 全量） | `bobleer/scripts/check-crate-boundaries.py:41-52,71-72`（`there < here_rank` 才报错，interfaces=1 依赖 assembly=2 不触发）；`src/crates/interfaces/acp/Cargo.toml:8` `dsh-core = { features = ["headless","llm-mock"] }` | 借鉴时补一条规则：接口/入口 crate 不得依赖 assembly；否则 ACP/CLI 变成全量组装 |
| H | **P3** | dsh 可替换缝与计划进程化是两套机制；前端钉死的硬契约（wire schema、事件词汇、seq、boot graph、SPA 语义）不可替换 | `packages/compaction/compaction/src/index.ts`（SD）、`packages/subagent/subagent/src/index.ts:1-30`（多 provider 注册表）、`packages/storage/storage-*`、`packages/session/session-persistence/src/coordinator.ts:122-126`（第三方后端可直接实现 seam） | M3 进程化不会撞前端契约；但也别把 dsh 的"provider 缝"当作前端契约的一部分复刻 |

---

## 2. 发现详情（含建议/观察）

### P1-A：§3.2 契约形状错误（必须修正）

`DSH_PROJECT_V2` §3.2 表：

- **face 1 "HTTP /api/* = REST 请求面（会话/模型/设置/工具…）"** —— 实际不是 REST。上行是**单一 POST** 到 `/api/<channel>/<endpoint>`，body 是 `{type:'client-request', rpcId, method, payload}` 信封，应答 `{rpcId, result:{ok}|{ok:false,error}}`（`packages/client/connection/src/client/rpc.ts:25-45`；`api/rpc.ts:152,172`）。无 GET 列表/状态码语义，HTTP 状态码只有 200/403/404/405/426 等粗粒度。
- **face 2 "WS /api/events.mux = 浏览器→宿主 上行复用流"** —— **方向写反**。`events.mux` 是**宿主→浏览器下行**事件流；WS 上行被协议禁止（`websocket-downlink.ts:109-111` 收到消息即 close 1008；`index.ts:150` 对 GET 两个 WS 路径回 426 "upgrade required"）。上行全部走 HTTP POST。
- **face 6 `__DSH_BOOT__` "注入 index.html 的启动图（tapIndex 变换）"** —— 对，但注入者、消费者、格式要记清：注入=client-modules `injectBootManifest`（`packages/client/modules/src/index.ts:168-175,246`），消费者=`packages/client/web/src/boot.tsx:98` 解析 `WebBootGraph`（`client/manifest.ts`），graph 结构 `{rev, entries:[{id,url,rev,inject?,immediately?}]}`（`client/modules/src/index.ts:149-158`）。

**要求**：合同台账按源码重画，不得引用计划表的形状描述。

### P1-B：契约物理归属与词汇归属分离（必须修正）

`web-server` 兼容层要复刻的契约，物理上由 **5 个包**注册，词汇上由**更多领域包**拥有：

| 面 | 物理注册/生产（包:行） | 词汇/schema 归属（包） |
|---|---|---|
| `/api` prefix 路由 | client-connection `index.ts:161-173`（含 trust fence） | — |
| `/api/<channel>/<endpoint>` POST 分发 | **dsh-api-gateway** `index.ts:90-112`（Typert `intercept('/api')`，按 `typertRemote` 标记解析） | 每个声明 remote 的 Service 包（session/settings/credentials/llm/host/…） |
| `ctx.apiProxy` 业务面 + fetch handler | **dsh-host-apiproxy** `index.ts:69-126` + `api-proxy.ts:1106` | `api/*.schema.ts`（zod）与 `api/*.ts`（rpc-map） |
| `events.mux` / `events.host` 流 | client-connection `websocket-downlink.ts:64-82` 泵取 **host-apiproxy** `api-proxy.ts:3429-3532` | `api/events.ts:69-155`（MuxFrame/HostFrame union）+ `api/events.schema.ts`（前端 import 的 zod） |
| SPA fallback | **frontend-static** `index.ts:112-127`（fallback seat，403/200/405/octet-stream） | distIndex 由 **bundle/web-app** 提供（`cordis.patch.yml:97-101` + `web-app/src/index.ts`） |
| `/plugins/<id>/client.js` + 图 | **client-modules** `index.ts:242,421-457` | `dsh.client` 包声明 + boot graph 结构 |
| `__DSH_BOOT__` | client-modules `tapIndex` `index.ts:246` | `client/manifest.ts` |
| 事件词汇 | session `append` 发 `session/event`（`core/session/src/index.ts:76,604-651`） | **dsh-session** `SessionEvent` 类型 + seq 语义 |
| tool view（mux 帧里的 `view`） | host-apiproxy `api-proxy.ts:3488-3493` | **dsh-tools/presentation**（`api/events.ts:22` 注"dsh-tools remains its owner"） |

**对 Rust 兼容层的含义**：
1. 兼容层不是一个"前端后端"，而是 **HTTP 运输层（复刻 client-connection 语义）+ RPC 分发层（复刻 dsh-api-gateway 语义）+ 业务实现（自研微内核，但要发出 dsh 词汇的事件）** 三层组合。前端唯一可见的接缝是"transport 语义 + 信封 + 帧 + 词汇"，可参考 `dsh-client-connection` 的 client 半（`AbstractApiClient`/`WebApiClient`/`ConnectionController`）作为验收代理。
2. 事件流的生产不是"agent/session/tools 各插件直发"——session 事件由 **session 服务**在 append 时广播 `session/event`（`core/session/src/index.ts:604-651`），host-apiproxy 监听并转成 mux 帧（`api-proxy.ts:3475-3494`）。Rust 微内核里"谁 append 谁广播"要按此语义建，且广播必须**在 log 提交之后**（post-commit fire-and-forget，`index.ts:66-76`）。

### P1-C：settings / credentials 契约面缺失（必须修正）

计划 §3.2 六面只覆盖传输。但前端设置生态直接消费：

- `settings.describe`（返回每个已注册 namespace 的 schema+resolved+base+user+revision，带 **secret 脱敏** `redactSecrets`，`settings/src/index.ts:479-511`）、`settings.update/replace/mutate`（`:534-575`，merge/replace/path-op 三写模式）、`SettingsConflictError` 修订冲突（`:164-183`）、resolve 顺序 = **schema 默认 → 组合 base → 用户文档**（`:696-710`）。
- `credentials.set/unset/describe`（`credentials/credentials/src` + `credentials-local/src/index.ts` 四层优先：继承环境 > `$DSH_HOME/.credentials.yaml` > cwd/.env > home/.env；CredentialRef 只带环境变量名）。
- `agentPreset.*`（read/copy/openDocument/remove，`preset/agent-presets/src`）。
- 以上全部被 `PRIVILEGED_METHODS` 钉死在 loopback（`client/connection/src/index.ts:89-119`），即使部署了 trustedHosts 也不放行。

**要求**：合同台账增加第七、八面（settings 面、credentials+agentPreset 面），并把 trust fence 实现为三段（Host 环回/trusted 判定 + cross-site 拒绝 + PRIVILEGED_METHODS loopback 钉扎），不是计划的"绑定 127.0.0.1 默认姿态"一句话。注意 settings 的 revision 语义：前端传 `expectedRevision`，我们的 Rust 实现必须同步，否则设置 UI 冲突检查失效。

### P1-D：bobleer 持久化不可照搬为事实源（必须修正）

- bobleer：`save_session` 只在 `run_task` 结束时调用（`assembly/core/src/runtime.rs:299-313`），整写 JSONL（tmp+rename，`services/persist/src/lib.rs:28-60`）。运行中事件只存在内存 Session；进程崩溃 = 丢失整轮。
- dsh：write-behind 批处理（`session-persistence/src/write-behind.ts`，200ms 窗 + `session/flush` 屏障）+ append+fsync（`session-persistence-jsonl/src/index.ts:651-689`，失败回滚 truncate 到 before 尺寸）+ 原子 materialize（link() EEXIST 防撞 + 目录 fsync，`:529-569`）+ torn-tail 修复（`readZstdPrefix` 的 `tornMarker{truncateTo,recoveredEvents}` + `commitRepair:436-444` 截断+补 closers）+ 稳定读（`readStableFile:292-304` stat 修订循环）。

**要求**：微内核 M1 的"append-only SessionEvent 唯一事实源"要按 dsh 语义实现：运行中批落盘 + fsync + 崩溃后 committed-prefix 恢复 + 补齐中断 turn 的 closers。这是"Linux 长期运行无感重起"的硬前提，也是 M2 验收（同一前端对比行为）能通过的持久性基础。

### P2-E：事件模式与作用域（建议）

bobleer 的 EventBus 只有 emit / waterfall×2 / serial（`contracts/events/src/bus.rs:65-76`）。dsh 有五种 `DispatchMode`（`vendor/cordis/src/events.ts:32`：emit/parallel/serial/bail/waterfall），其中 **parallel 用于 `session/flush` 持久化屏障**（`core/session/src/index.ts:78-85`）。bobleer 无此屏障——因为它在任务结束才持久化。我们自研时给 EventBus 加 parallel 模式 + flush 屏障即可（小改动），但这是借鉴 bobleer 时必须主动补的缺口，否则"模型可见即已落盘"断言只能做内存级。

作用域：dsh `session/event` 是 scope 过滤分发（agent 作用域监听者只收自己 agent 的会话，`core/session/src/index.ts:48`），bobleer 无 scope。前端 mux 是**按会话聚合**的（订阅基线 `session/subscribed` lastSeq + `session/event` 按 sessionId 分路，`api/events.ts:70`），所以作用域是宿主内语义，不影响对外契约——但多会话并行时 Rust 侧需要等价的按会话路由。

### P2-F：bobleer 动态性缺位与计划 M3（建议）

bobleer 明确"不移植 Cordis/Loader/HMR/插件树"，组合=feature 闭合，`PluginRuntimePort` 只接 `UnavailablePluginRuntime`（`assembly/core/src/runtime.rs:88`；`--dump-config` 显示 `plugin_runtime: unavailable`）。其价值是**端口形状**：`NotAvailable` fail-loud（`docs/architecture.md:12`"a later Plugin Host would sit behind PluginRuntimePort and fail loud while unregistered"），以及 `register()` 返回 Disposer 的"注册即 effect"纪律（`AGENTS.md:20`）。计划 M3 的 supervisor 进程化是全新设计，样板不给实现。风险提示：把"借鉴 bobleer 分层"误读为"已有进程化基础"会拖延 M3。

### P2-G：边界脚本盲区（建议）

`check-crate-boundaries.py:71-72` 只报 `there < here_rank`（向更低层依赖）；interfaces=1 依赖 assembly=2 不触发——`dsh-acp` 直接依赖 `dsh-core {features=["headless","llm-mock"]}`（`src/crates/interfaces/acp/Cargo.toml:8`），即 **接口层拖进整个 headless 组装**。借鉴时补规则："apps/interfaces 不得依赖 assembly；assembly 以外的层不得依赖 delivery feature 组合"。脚本本身已在 CI（`.github/workflows/ci.yml`）且静态验证通过，是健康样板。

### P3-H：可替换缝与硬契约的分野（观察）

- **可替换**：compaction（SD+2 provider）、subagent（SD + spawn-in-process/-fork/-acp 多 provider）、storage（SD + json/sqlite/domain）、session-persistence（SD，第三方可直实现后端）、llm 适配器（注册表）。全是**进程内 provider 缝**，前端不可见。
- **不可替换（前端钉死）**：wire schema（`api/*.schema.ts` 的 zod，前端直接 import）、事件词汇（dsh-session）、seq/lastSeq 语义、RPC 信封、boot graph 结构、`/plugins/*/client.js` 与 SPA fallback/403/405 语义、trust fence。
- **含义**：Rust 兼容层的"逐字对齐"只需覆盖**硬契约**；宿主内把"provider 缝"做成进程还是 trait 注册表，前端无感知。计划的"挂点集合一致（一个不多一个不少）"应限定在硬契约面，不要为 dsh 的内部 provider 缝逐一留口。

---

## 3. 契约与边界专题：dsh 前端契约所有权图谱 + 兼容层边界建议

```
（前端 = dsh web-app 产物，@deepseek-ai/dsh-web-frontend + client 生态）

┌─ 传输/信封层（复刻 client-connection，权威：packages/client/connection/src）
│   ├─ /api prefix 路由 + trust fence（api-request-trust.ts + index.ts:161-173,89-119）
│   ├─ POST RPC 信封  client-request/server-request + rpcId（client/rpc.ts; api/rpc.ts）
│   ├─ /api/events.mux + /api/events.host 下行 WS（websocket-downlink.ts, index.ts:150,193-194）
│   └─ 前端重连控制 ConnectionController（client/connection.ts，指数退避+严格握手）
│
├─ 分发层（复刻 dsh-api-gateway，权威：packages/api/gateway/src/index.ts）
│   └─ Typert 方法分发 / 参数解析 / 错误分类（TypertGatewayError code 枚举）
│
├─ 业务面（Rust 自研微内核实现，但要发 dsh 词汇）
│   ├─ 领域 RPC：session / settings / credentials / agentPreset / host / llm / subagents
│   │   └─ 词汇 schema 权威：packages/host/apiproxy/src/api/*.schema.ts（zod，前端 import）
│   └─ 事件流生产：session append → session/event → host-apiproxy 转 mux/host 帧
│       └─ 帧 union 权威：api/events.ts；事件类型权威：packages/core/session（SessionEvent）
│
└─ 静态/引导层
    ├─ SPA fallback（frontend-static fallback seat：403/200/405/octet-stream）
    ├─ /plugins/*/client.js + graph（client-modules）
    └─ __DSH_BOOT__ 注入（client-modules）→ 消费（client/web/src/boot.tsx）
```

**边界建议**：
1. 兼容层内部三件套分开：`transport`（HTTP+WS+envelope+fence）、`dispatch`（endpoint 注册+参数校验）、`domains`（微内核语义转 dsh 词汇）。禁止 transport 直接调 domain 业务，禁止 domain 发非 dsh 词汇事件。
2. 验收代理 = dsh 官方前端 + `dsh-client-connection` 的 client 半（`AbstractApiClient`/`ConnectionController`），按 M2 门禁"同一前端分别连 Node 后端与 Rust 兼容层行为对比"逐项勾销。
3. 前端升级 = 台账 diff（计划已定），台账来源改为本文 §3 的权威路径清单。

---

## 4. 配置归属专题：要承接的 dsh 配置面清单与所有权

| 面 | dsh 所有权（包） | 语义要点（证据） | Rust 兼容层动作 |
|---|---|---|---|
| settings 运行时 | `packages/settings/settings`（SD，`ctx.settings`） | namespace 注册 = schema+base+用户层；resolve=默认→base→用户；三写模式 update/replace/mutate；revision 冲突；secret 脱敏 | **必须实现** settings RPC 域 + 三层 resolve + revision + 脱敏 |
| settings 存储 | `packages/settings/settings-file` | 单 YAML/JSON（`$DSH_HOME/settings.yaml`）；leaf-diff 注释保留（`patchNode`）；atomic write + 跨进程文件锁；热重载 | 可换 Rust 实现（toml/yaml+锁），对外行为 = describe 结果与 revision 单调性 |
| 组合 base 层 | profile/bundle patch（`packages/boot/app-boot/src/profile.ts` + `vendor/include`） | **patch 覆盖语义 = 每层整行替换 config（last-write-wins per row，非深合并）**，`vendor/include/src/index.ts:121-124`；`web-app/cordis.patch.yml:13-15` 注释明示 | 宿主内部可自定（Rust 配置组装不暴露给前端）；**不需要复刻**到前端面 |
| credentials | `packages/credentials/credentials` + `-local` | 四层优先（env > managed yaml > cwd/.env > home/.env）；CredentialRef=环境变量名；0600 权限检查 | **必须实现** credentials RPC 域（set/unset/describe）+ 存储（若不用 env 则自建密钥文件，0600） |
| agentPreset | `packages/preset/agent-presets` | read/copy/openDocument/remove 为 loopback-pinned 方法 | M2 需提供 agentPreset.read 等，否则会话创建/预设 UI 不可用 |
| 前端消费面 | `ui-settings*`、`ui-model-selection`、`ui-permission-presets` | describe 时脱敏（wire 必须 redact） | Rust 侧任何 wire 输出过 `role('secret')` 字段前必须脱敏（`settings/src/redact.ts`） |

关键点：计划的 §3.2 六面里没有 settings/credentials/agentPreset 三个 RPC 域，但前端设置页/模型页/预设 UI 直接消费。**这是 M2"聊天闭环"之后第一个会白屏/禁用的面**。

---

## 5. 原子性/故障专题

**dsh（已实现，可作规格）**：
- 会话日志原子性：每批 append = 编码 → writeFile → fsync，失败 `rollbackAppend` 截回 before 尺寸再抛（`session-persistence-jsonl/src/index.ts:651-689`）；materialize = temp+fsync+link()（EEXIST 防并发撞）/ Win32 发布 + 目录 fsync（`:529-592`）；稳定读 = stat 修订循环（`:292-304`）。
- 崩溃恢复：torn 尾帧 → `tornMarker{truncateTo,recoveredEvents}` → `commitRepair` 截断 + 补 `interruptedTurnClosers`（`coordinator.ts:891-933`；`jsonl/index.ts:436-444`）。事件若在最后完整提交点之后，靠 replays 的前缀一致性，不含未提交部分。
- WS 断连恢复：前端 `ConnectionController` 指数退避重连（500→10k ms）+ 严格握手（两流 onOpen + host.describe 成功才 onConnected，`client/connection.ts:107-169`）；`events.mux` 的 `since` 参数 **v1 未实现**，重连 = 重开流 + 前端按会话 refetch history（`api/events.ts:56`；`client/runtime/src/client/sessions/session.ts:416,646`）；服务端 mux-open 会重放订阅基线（`session/subscribed`+lastSeq）、pending 审批/提问（rpcId 复用）、`session/queue`/`session/jobs` 快照（`api-proxy.ts:3430-3469`）。
- 进程崩溃：持久化恢复如上；前端靠重连 + 全量 refetch 收敛（前端不依赖服务端会话内存）。

**bobleer（现状，缺口）**：整写、无运行中批落盘、无 torn-tail 概念、无恢复补 close；`SESSION_FORMAT_VERSION=0` 无兼容承诺（`docs/architecture.md:55`）。

**对计划 v2 的要求**：
1. 微内核事实源 = 运行中 append + fsync + committed-prefix 恢复（dsh 语义），M1 就绪。
2. 兼容层 `events.mux` 必须实现**订阅基线重放**（lastSeq + pending 帧 rpcId 复用 + queue/jobs 快照），否则重连的同一前端会丢帧（前端 v1 语义如此，不能要求前端改）。
3. `session/queue`、`session/jobs` 等瞬态快照帧是"非持久状态的外置呈现"——正好对应计划的"状态外置"：这些状态（inbox、jobs）在 dsh 是宿主内存态，前端依赖全量快照帧收敛。Rust 侧对应 supervisor/插件状态也应能全量重建（M3 蓝绿替换会话不中断的验收正是靠这个）。

---

## 6. 对架构文件（DSH_PROJECT_V2）的修正建议

**必须修正（P1）**
1. §3.2 face1 改为"**POST-only RPC 信封面**"（`/api/<channel>/<endpoint>`，client-request/server-request + rpcId），删除"REST 请求面"表述。
2. §3.2 face2 改为"**宿主→浏览器下行事件流**"；上行说明改为"HTTP POST 单通道"，并注明 WS 上行被协议拒绝（close 1008 / 426）。
3. §3.2 契约所有者列改写：`/api` 物理路由=client-connection；事件流=host-apiproxy；RPC 分发=dsh-api-gateway；静态面=frontend-static+bundle/web-app；`/plugins`+`__DSH_BOOT__`=client-modules。补"词汇/schema 归属"列（dsh-session、api/*.schema、dsh-tools/presentation）。
4. §3.2 增补两整面：**settings 面**（describe/update/replace/mutate + revision + 脱敏）与 **credentials+agentPreset 面**；trust fence 条款升级为三段式（Host 环回/trusted + cross-site + PRIVILEGED_METHODS loopback 钉扎）。
5. §四 M1 补一条硬约束：**运行中 append + fsync + 崩溃后 committed-prefix/torn-tail 恢复**（对照 dsh `session-persistence-jsonl` 语义），并标注"借鉴 bobleer 仅借分层与端口形状，持久化语义照 dsh"。
6. §五.1 team 子代理"进程隔离天然数据隔离"与 dsh subagent provider 缝区分：前端只消费 `subagents.*` RPC 与 `session/subscribed` 帧，进程形态是宿主内部选择。

**建议（P2）**
7. 借鉴清单里加"补 parallel 分发模式 + 冲刷屏障（等价 dsh `session/flush`）"，并把"model-visible⟺logged 断言"落到 flush 屏障而非仅内存。
8. 借鉴清单里注明 `check-crate-boundaries.py` 盲区并补规则："apps/interfaces 不得依赖 assembly；非 assembly 层不得开 delivery feature"。
9. M3 supervisor 标注"与 dsh 无对照，全新设计；仅借鉴 PluginRuntimePort 的 NotAvailable fail-loud 与 register→Disposer 纪律"。
10. 风险表补充：settings/credentials 面工程量（易低估，涉及脱敏+修订冲突+凭据文件权限）。

---

## 7. 演进顺序与残余风险

**演进顺序**（按前置与风险削减）：
1. M2 前重画合同台账（本文 §3/§4 为索引）——修正 P1-A/B/C 的形状与归属错误。
2. M1 内核持久化先做"运行中落盘 + committed-prefix 恢复"（P1-D），再谈 loop 语义对齐。
3. M2 聊天闭环 → settings/credentials/agentPreset 面 → trust fence 三段式 → mux 订阅基线重放。
4. M3 supervisor 与 dsh 硬契约解耦独立推进。

**残余风险/盲点**：
- 本审计无运行时观测（WS 帧、zod schema 的实际序列化样本）；合同台账以源码为准已可开工，但 schema 的**逐字字段名**建议在台账中从 `api/*.schema.ts` 提取而不是从类型推断。
- bobleer 边界脚本在 Windows 无 python 的 CI 上不可运行（计划 CI 是 Rust 质量门）——要么容器带 python，要么移植为 Rust 检查器（P2 项）。
- dsh 的 sqlite 持久化后端（`session-persistence-sqlite`）未深读；若计划 M1 用 sqlite 而非 JSONL，事务/恢复语义需单独对照该后端。
- `dsh-api-remotes`（remote-events 允许名单，`API_REMOTE_FORWARDED_EVENTS`）转发宿主事件到前端 `host/remote-event` 帧——Rust 侧若不实现，前端遥测/远程事件 UI 缺失；已列入台账候选但未细审。
