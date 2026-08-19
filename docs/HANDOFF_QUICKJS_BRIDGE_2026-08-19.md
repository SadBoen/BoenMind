# HANDOFF：QuickJS 桥落地（引擎 + manifest 最小权限授面 + 接真 LLM）（2026-08-19）

> 状态：**§5.2 + §5.3 + §5.4 已落地并全绿**（09f296f + b282257 + 本轮）。
> quickjs-bridge 从占位 crate 变为可用引擎：`HostApi` 注册进 rquickjs 全局 `host`
> + 异步泵打通 + manifest 驱动装载（按声明面最小权限授面）+ **真 LlmPort 接线**
> （JS 插件用 `host.llm.complete` 打真实 provider，与 agent-loop 同一聚合 LLM）。
> 28+11 测试全绿；workspace + clippy + gate1 全过。
> 下轮：§5.5 真 JS 插件跑通（`LoadedPlugin` + `Runtime::js_bridge` 组合）+ tools/session
> 面从占位升级。

---

## 1. 一句话交接

QuickJS 桥主线的落地顺序步骤已完成三步：**§5.2 引擎**（`HostApi` 注册进 rquickjs
全局 `host`，异步泵打通：AsyncContext 专用线程 + `rt.drive()` 泵 Promise）；
**§5.3 manifest 驱动装载**（`plugin.json` 声明 host 面 → 按声明最小权限授面，
未声明面不注入 JS）；**§5.4 接真 LLM**（桥层默认 `llm_complete_stream` 经新增
`llm_port()` 走内核 `LlmPort`，组合根 `Runtime::js_bridge` 把聚合 LLM 接进桥）。
桥层本身已可跑 JS 插件打真 provider（log/config/tools/llm/session 全 face）。

## 2. 落地清单（三个 commit）

| Commit | 内容 |
|---|---|
| `09f296f` | §5.2 引擎：`bm/quickjs-bridge/src/js.rs` 的 `JsBridge`——AsyncContext 跑专用线程独立 tokio runtime，`rt.spawn(rt.drive())` 常驻泵 JS 任务；HostApi 8 面全注册；JSON 字符串跨桥 + JS 包装层；`exec`/`exec_async`/`eval_value`/`call_async` 四入口 |
| `b282257` | §5.3 manifest：`bm/quickjs-bridge/src/plugin.rs`（新）`JsPluginManifest`（plugin.json：id/name/version/entry/host）+ `ALL_HOST_FACES` 面白名单 + `LoadedPlugin::load`（目录→manifest→入口源码）；`JsBridge::new_with_faces` 按面注册 + 动态包装层 `build_wrapper`（只含已授面） |
| 本轮 | §5.4 接真 LLM：`bm/quickjs-bridge/src/host.rs`（新）+ `HostApi::llm_port()` + 默认 `llm_complete_stream`；`bm/assembly/src/js_host.rs`（新，`RealHost`）+ `Runtime::js_bridge(faces)` 装配入口 |

## 3. 验证矩阵（全绿）

| 项 | 结果 |
|---|---|
| `cargo test -p quickjs-bridge` | 28 全过（原 16 + §5.4 新增 12） |
| `cargo test -p bm-assembly` | 11 全过（新增 js_bridge 真 LLM 接线 2 端到端 + RealHost 1） |
| `cargo test --workspace` | 12 suite 全 ok，0 FAILED |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `bash scripts/verify-gate1.sh` | GATE1: ALL PASS |

## 4. 关键机制（下轮不重踩）

**异步泵架构**：rquickjs 0.6 无 Node 事件循环，`Async` fn 注册成 JS Promise 后 future
压进 runtime spawner，**只能由 `AsyncContext` + `AsyncRuntime::drive()` 驱动**
（`Ctx::eval` 不 poll futures）。引擎=AsyncContext 跑专用线程独立 tokio runtime；
HostApi 调用（tools_invoke/llm_complete_stream）在 JS 线程 `block_on` 到**宿主**
runtime——双 runtime 分离防 deadlock。需 `parallel` feature（`tokio::spawn(rt.drive())` 要求 Send）。

**跨桥类型（rquickjs 0.6 的坑）**：
1. `IntoJsFunc` 闭包不能返回带 `'js` lifetime 的 `Value`/`Ctx`（带捕获闭包推断不出统一
   lifetime）→ host 原始函数统一返回 **JSON 字符串**（`Result<String>`），JS 侧包装层
   `JSON.parse` 还原。register 函数须显式 `<'js>` lifetime（Ctx invariant）。
2. `eval_promise`（`JS_EVAL_FLAG_ASYNC`）resolve 出**模块命名空间**而非脚本值 →
   异步结果走「顶层 `await` 表达式写全局变量 + 同步 `eval_value` 读回」（`call_async`）。
   IIFE `(async()=>{})()` 不被顶层 await 驱动，必须直接顶层 `await`。
3. `Async` fn 同步调用返回 Promise：包装层 `host.tools.invoke`/`host.llm.complete`
   必须 async 方法，调用方 await。

**接真 LLM（§5.4，组合根唯一装配纪律）**：quickjs-bridge 不依赖 provider 适配器/
web-server——只加 `HostApi::llm_port()`（默认 `LLM_UNAVAILABLE` 诚实失败），
`llm_complete_stream` 默认实现 = 经 `llm_port()` → `GenerateOptions` → `LlmPort::stream`
→ 逐块翻译成 `CompletionChunk` JSON。真实装配在 **bm-assembly**（唯一组合根）：
`js_host::RealHost` 把 `Runtime.llm`（聚合 `LlmPort`，与 agent-loop 共享同一 `Arc`，
`swap_llm` 后对 JS 插件同样下一请求生效）经 `llm_port()` 接进桥；`Runtime::js_bridge(faces)`
是桥装配唯一入口（manifest 最小权限授面）。

**块流翻译（host.rs）**：`to_kernel_messages`（text-only，未知角色 `UNSUPPORTED_ROLE`）
+ `to_kernel_tools` + `translate_llm_chunks`。text-delta/block-start/block-end/usage 的
子集：text-delta 下发、reasoning-delta 折叠进 text、tool-call-delta 下发（含索引）、
block 起止与 usage 不下发（JS 编排不需要）。**torn 纪律**：流 `Err` 或 Finish 缺失
都补 `STREAM_CLOSED` 终态（与 loop 的 torn 判定一致）。取消：`Cancellation` 订阅 →
`AbortSignal::abort()` → provider 以 `finish{cancelled}` 收尾。

**最小权限授面（§5.3）**：面粒度=单方法（`tools.invoke`/`llm.complete` 独立授面）；
manifest `host` 缺省=空集；未知面名拒绝解析（防拼错静默失效）；未授面访问抛
ReferenceError（测试钉死）。权限治理单点 = 面白名单 + manifest，rquickjs 不注入
fs/fetch。

## 5. 下轮指针

1. **§5.5 真 JS 插件跑通**（QuickJS 桥主线下一实质步骤）：`LoadedPlugin::load` +
   `Runtime::js_bridge` 组合——装载目录插件（plugin.json + main.js）→ 按 manifest
   授面 → JS 里调 `host.llm.complete` 打真 provider（`--config` 装配的聚合 LLM）。
   tools/session 面从 `RealHost` 占位升级：接 `ToolRegistry`（`tools_list`/`tools_invoke`
   经 ToolGate）+ `SessionStore`（`session.*` 拉模型读投影）。
2. **待办 2：dsh-rust-plugins 更新流程**——源仓已打 tag `absorbed-into-boenmind-2026-08-19`
   锁 commit，台账 `docs/PLUGIN_ABSORPTION_LEDGER_2026-08-19.md`；后续按台账流程吸收。
3. **待办 3：web-server 验证脚本路径**——conformance / gate25 / m3-r3 脚本如需跑，
   用新路径（hot-replace / hot-upgrade-transition 已更新为新布局）。

## 6. 环境纪律（沿用）

- 每轮先杀 web-server：`taskkill //F //IM web-server.exe`
- 跑服务用 release（debug exe 2GB 超 PE 限制）
- 验证三件套：cargo test / clippy / gate1（BoenMind 主仓）
- rquickjs 版本锁 0.6.2（crates.io 0.12.2 不在本地缓存，离线不可用）
