# QuickJS 桥设计基线（2026-08-19）

> 状态：**全部落地**。host 面契约已定稿（§4）+ rquickjs 桥已实现（§5.2 完成：
> `HostApi` 注册进全局 `host` + 异步泵打通，11 测试全绿）；manifest 驱动装载
> （§5.3 完成：按 manifest 最小权限授面，16 测试全绿）；§5.4 接真 LLM + §5.5 tools/session
> 面接线 + §5.6 目录插件注册表/config 面已完成；**§6 接业务（web-server `--plugins-dir`
> 装配 + 插件运行时收敛进 `PluginRuntimePort`）亦完成**（2026-08-19 同轮，见交接
> `docs/HANDOFF_QUICKJS_BRIDGE_2026-08-19.md` §6）。
> 本文件是实现的契约，不是 wiki。

## 1. 定位

- **归属**：BoenMind 产品层（`bm/quickjs-bridge`），非核心仓。核心仓保持纯 Rust 内核库，
  便于对齐官方上游；桥只通过内核契约端口（`LlmPort`/`ToolHandler`/`AgentPort`/`EventBus`）暴露宿主 API。
- **目标**：兼容官方 dsh 生态的**纯逻辑 JS 插件**（契约与数据格式对齐：工具 JSON、事件、
  LLM 协议；不承诺运行官方 Node/Cordis 后端插件，不把前端 38 bundle 塞进 QuickJS）。

## 2. 运行时选型（实测 2026-08-19）

- **rquickjs**（Rust 原生 FFI 绑定，crates.io 0.12.2）——native 绑定是效率最高的选择。
  wasm 版（quickjs-emscripten）要带 wasm 运行时、性能慢 20~50%，排除。
- 实测（release，opt3+strip+lto）：exe 1.59MB、常驻 4.3MB（Node 32MB 的 1/7）、fib 5ms。
- 性能边界：纯逻辑毫秒级；字符串/正则/JSON/GC 重负载慢 Node 7~25 倍（140~450ms）。
  **规则：重逻辑一律回调宿主 Rust API，JS 只做编排胶水。**

## 3. 通讯模型（rquickjs 同进程内嵌）

QuickJS 编译进同一进程，通讯 = **同地址空间函数调用**，无 socket、无序列化协议：

| 方向 | 机制 |
|---|---|
| Rust → JS | `ctx.eval` / 调 JS 函数；参数经 `IntoJs` 拷成 JS 值 |
| JS → Rust | 注册的 native function（如 `host.llm.complete`）；参数 `FromJs` 抽回 |
| 大对象 | 句柄：Rust 侧资源 id，JS 只拿 id 操作，避免大拷贝 |
| 异步 | QuickJS **无 Node 事件循环**；接 Rust 异步须宿主驱动 Promise/定时器（见 §4 泵） |

## 4. 宿主 API 面（v1 最小面 + grok 评审补的缺口）

```text
host.log(level, msg)
host.config.get(plugin_id, key)          // 按插件 id 命名空间，禁止任意键（防读 API key）
host.llm.complete(req) → async iter       // 走 LlmPort；流式
host.llm.complete_stream(req, cancel)     // SSE 直通 + 取消 token
host.tools.list() / schema() / invoke(name, json) → {ok, err:{code,retryable}}  // 走 ToolRegistry+ToolGate
host.session.append(event) / get(id) / subscribe()   // 事件日志投影 + 订阅
host.agent.step(…)                        // 禁止 JS 自管循环（避免再写一套 React loop）
```

**禁止默认暴露**：任意 `fs`/`net`/`process`/`require`/动态加载原生模块。rquickjs
**不注入** fs/fetch（只靠文档不够，JS 读 API key 就完蛋）。权限治理单点 = 宿主 API + manifest。

**边界**：
- 类型只跨 `JSON` + 显式 schema；禁止传 Rust trait object / 裸指针。
- 失败模型 `{ok, err:{code,retryable}}` 与 ToolGate 同一套码。
- 一个 JS 插件 = 一个入口模块 + manifest（声明用了哪些 host 面）；组合根按 manifest 授面，默认最小权限。
- Abort/deadline/token 预算必须从宿主贯穿（否则 JS 死循环拖死 runtime）。

## 5. 落地顺序（先 host 面契约测试，再 rquickjs，再接真 LLM）

1. **host 面契约测试**（假 Runtime）：定义 host API 的 Rust trait + JSON 出入参，先测透。
2. **rquickjs 桥接**：把 host trait 注册进 JS 全局；确认异步泵（dedicated thread 或 `async_with`）。
3. **manifest 驱动装载**：`plugins/_manifest.toml` 或 JS 插件自带 manifest，声明 host 面 → 最小权限授面。
4. **接真 LLM**：JS 插件能调 `host.llm.complete_stream` 打真实 provider。
5. **禁止在 JS 里做**：HTTP、磁盘、加密、大 JSON transform、tokenizer、重试/backoff（全进 Rust 插件）。

### 5.1 落地实录（2026-08-19：§5.2 完成）

**实现**：`bm/quickjs-bridge/src/js.rs` 的 `JsBridge`——把 `HostApi` trait 注册进
rquickjs 全局 `host`，异步泵打通。11 测试全绿（原 5 契约 + 6 rquickjs 端到端）。

**异步泵架构（rquickjs 0.6 实测结论）**：
- `Async` fn 注册成 JS Promise 后，future 压进 runtime spawner，**只能由
  `AsyncContext` + `AsyncRuntime::drive()` 驱动**（`Ctx::eval` 不 poll futures）。
- 引擎 = `AsyncContext`，跑在专用异步线程的独立 tokio runtime；`rt.spawn(js_rt.drive())`
  常驻泵 JS 任务（同 rquickjs 官方 `async_test_case` 的 `drive` 用法）。
- HostApi 调用（tools_invoke/llm_complete_stream）在 JS 插件线程 `block_on` 到**宿主**
  tokio runtime——与泵线程独立 runtime 分离，杜绝跨 runtime deadlock。
- 需 `parallel` feature（`tokio::spawn(rt.drive())` 要求 Send）。

**跨桥类型（rquickjs 0.6 的坑）**：
- `IntoJsFunc` 闭包不能返回带 `'js` lifetime 的 `Value`/`Ctx`（带捕获闭包推断不出统一
  lifetime）→ host 原始函数统一返回 **JSON 字符串**（`Result<String>`），JS 侧包装层
  `JSON.parse` 还原成对象——对齐官方 dsh 生态 `JSON.stringify` 风格。
- `eval_promise`（`JS_EVAL_FLAG_ASYNC`）resolve 出**模块命名空间**而非脚本值 →
  异步结果走「顶层 `await` 表达式写全局变量 + 同步 `eval_value` 读回」（`call_async`）。
- `Async` fn 同步调用返回 Promise：包装层 `invoke`/`complete` 必须是 async 方法（调用方 await）。

**host 面**（JS 插件可见）：`host.log` / `host.config.get` / `host.tools.list` /
`host.tools.invoke`（异步）/ `host.llm.complete`（异步）/ `host.session.append|get|poll`。
不注册 `host.agent.step`。

### 5.3 落地实录（2026-08-19：manifest 驱动装载完成）

**实现**：`bm/quickjs-bridge/src/plugin.rs`——`JsPluginManifest`（`plugin.json`：
id / name / version / entry / host 面声明）+ 面白名单 `ALL_HOST_FACES` +
`LoadedPlugin::load`（目录 → manifest → 入口源码）。`JsBridge::new_with_faces`
按声明面注册（未声明面不注入 JS，`host.tools`/`host.llm` 等为 `undefined`）。

**最小权限授面**：
- 面粒度 = 单个方法（`tools.invoke`、`llm.complete` 各自独立授面）。
- manifest `host` 缺省 = 空集（最小）；未知面名拒绝解析（防拼错静默失效）。
- 未授面调用：JS 访问 `host.tools.invoke` 抛 ReferenceError（实测钉死）。
- 包装层动态生成（`build_wrapper`）：只含已授面，未声明面不出现在 `host` 上。

**验证**：16 测试全绿（原 11 + §5.3 新增 5：manifest 解析/去重/最小权限 undefined/
未授面调用抛错/目录装载）；workspace 全过 + clippy 零警告 + GATE1 ALL PASS。

**下轮 §5.4 接真 LLM**：JS 插件用 `host.llm.complete` 打真实 provider——需组合根把
真 `LlmPort`（经 `bm-assembly` 装配的聚合 LLM）接进 `HostApi` 实现，桥层无需改动。

### 5.4 落地实录（2026-08-19：接真 LLM 完成）

桥层无需改动（§5.2 的异步泵 + §5.3 的授面已就绪），只在两侧各加一小块：

- **quickjs-bridge**：`HostApi` 新增 `llm_port() -> Result<Arc<dyn LlmPort>>`，
  **默认实现 = 不可用**（`LLM_UNAVAILABLE` 诚实失败，不假成功）；`llm_complete_stream`
  改为**默认实现**：经 `llm_port()` 取内核端口 → 桥请求翻译成 `GenerateOptions`
  （text-only，未知角色 `UNSUPPORTED_ROLE`，工具声明拷贝成 `ToolSchema`）→
  `LlmPort::stream` → 逐块消费翻译成 `CompletionChunk` JSON（块索引保留，
  reasoning-delta 折叠进 text，usage 不下发）→ `{ok, value:{chunks}}`。取消经
  `Cancellation` 订阅触发内核 `AbortSignal`（provider 以 `finish{cancelled}` 收尾）。
  新增 `host.rs` 模块（`to_kernel_messages` / `to_kernel_tools` / `complete_with_port` /
  `translate_llm_chunks`），torn 纪律：流 Err 或 Finish 缺失都补 `STREAM_CLOSED` 终态。
- **bm-assembly（组合根，唯一装配点）**：新增 `js_host.rs`（`RealHost`）——把
  `Runtime.llm`（聚合 `LlmPort`，与 agent-loop 共享同一 `Arc`，`swap_llm` 后对 JS
  插件同样下一请求生效）经 `llm_port()` 接进桥；其余面（log/config/tools/session）
  留诚实占位。`Runtime::js_bridge(faces)` 是桥装配唯一入口（按 manifest 最小权限授面）。

**验证**：28 测试全绿（原 16 + §5.4 新增 12：默认路径 LLM_UNAVAILABLE、桥消息/工具
翻译、块流 5 案例 + 取消、JS 端到端真端口链路 + 无端口诚实失败）；bm-assembly
11 测试全绿（新增 js_bridge 真 LLM 接线 2 端到端 + RealHost 1）；workspace 全过 +
clippy 零警告 + GATE1 ALL PASS。

### 5.5 落地实录（2026-08-19：tools/session 面接线 + 真 JS 插件跑通 + 修 qjs 堆损坏）

- **tools/session 面接线**：`RealHost` 接 `ToolRegistry` + `ToolGate`（与 agent-loop
  同门控语义：未注册 `tool-not-found`、注册未启用 `tool-disabled` fail-closed、清单
  只列已启用）+ `SessionStore`（`session.append` 反序列化内核 `SessionEvent` 追加 /
  `get` 快照 / `poll` 游标续读——拉模型，禁止 JS 回调重入）。config 面留占位。
- **真 JS 插件跑通**：`LoadedPlugin::load`（plugin.json + main.js）+ `Runtime::js_bridge`
  按 manifest 授面组合——JS 插件里 `await host.llm.complete`（真端口）+ `host.tools.invoke`
  （echo）+ `host.session.append/get`（拉模型投影）全链路测试通过；未声明面在 JS 里
  `undefined`（授面纪律在装配层生效）。
- **修 qjs 堆损坏（`0xc0000374`，异步路径间歇崩）**：§5.2 曾在引擎 runtime 常驻
  `rt.spawn(js_rt.drive())` 泵 JS 任务。实测发现 rquickjs 0.6 的 `async_with!` 展开的
  `WithFuture::poll` **内部持锁并驱动 spawner**（`spawner.poll` + `execute_pending_job`），
  官方 `async_test_case` 从不单独 spawn drive。常驻 drive 泵导致 pump 线程与
  `block_on` 线程**双线程并发 poll 同一 qjs spawner** → 堆损坏（mem::forget 探针证明
  与 Drop 无关，纯运行期竞争）。**去掉常驻 drive 任务后单线程驱动，8 连跑稳定。**

**验证**：quickjs-bridge 28 + bm-assembly 15（新增 tools/session 面 2 + 真 JS 插件
全链路 1 + 最小权限 undefined 1 + swap 后 js_bridge 1）+ 1 边界 + 6 最小三插件；
workspace 全过 + clippy 零警告 + GATE1 ALL PASS。

### 5.6 落地实录（2026-08-19：目录插件注册表 + config 面）

- **目录插件注册表**：`bm/quickjs-bridge/src/registry.rs`（新）`scan_plugins`（递归
  扫描任意深度，plugin.json 缺失目录跳过，损坏插件 fail-loud 不静默跳过，同名 id
  后扫覆盖便于覆盖默认）+ `PluginDir` 只读视图。注册表只做发现+装载（读盘），
  引擎装配仍走组合根（每插件一引擎 = 独立 AsyncRuntime + 上下文，天然隔离）。
- **config 面升级**：`RealHost` 接白名单 `HashMap<"{plugin_id}.{key}", value>`——
  命中返回、未命中 `config-not-found`；**白名单即全部内容，永不返回 secret**
  （credentials 由凭据面管，不进此表）。`Runtime::js_bridge_with_config` 注入。
- **组合根唯一入口**：`Runtime::load_js_plugin(dir)` = `LoadedPlugin::load` →
  按 manifest 授面 → 引擎；`Runtime::scan_js_plugins(dir)` = 目录扫描清单。

**验证**：quickjs-bridge 32（+4 注册表：嵌套扫描/重复 id 覆盖/损坏 fail-loud/视图
排序）；bm-assembly 18（+3：config 白名单命中与未命中 / load_js_plugin 端到端含
config / scan_js_plugins 全清单）；workspace 全过 + clippy 零警告 + GATE1 ALL PASS。

**下轮 §6（已完成，2026-08-19 同轮）**：插件运行时收敛进 `PluginRuntimePort`
（探针变 Ready）+ web-server 装配（`--plugins-dir`）：bm-assembly `JsPluginRuntime`
（每插件一引擎，空清单 Unavailable 诚实失败）+ `Runtime::load_js_plugins_dir` +
`plugin_manifest` 合并 Feature 条目 + web-server `--plugins-dir` 参数（fail-loud）。
详见 `docs/HANDOFF_QUICKJS_BRIDGE_2026-08-19.md` §6。

## 6. 相关文件

- 实测数据：`.tmp/quickjs-bench/QUICKJS_RUNTIME_REPORT_2026-08-19.md`
- grok 评审：`.tmp/grok-quickjs-plan-review.md` / `.tmp/grok-arch-final-review.md`
- 占位 crate：`bm/quickjs-bridge/src/lib.rs`
