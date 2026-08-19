# HANDOFF：QuickJS 桥落地（引擎 + manifest 最小权限授面 + 接真 LLM + tools/session 面）（2026-08-19）

> 状态：**§5.2 + §5.3 + §5.4 + §5.5 已落地并全绿**（09f296f + b282257 + f5261c4 + 本轮）。
> quickjs-bridge 从占位 crate 变为可用引擎：`HostApi` 注册进 rquickjs 全局 `host`
> + 异步泵打通 + manifest 驱动装载（按声明面最小权限授面）+ 真 LlmPort 接线
> （JS 插件用 `host.llm.complete` 打真实 provider）+ tools/session 面接线
> （ToolRegistry+ToolGate / SessionStore，拉模型投影）+ 真 JS 插件跑通。
> 28+15 测试全绿；workspace + clippy + gate1 全过。
> 下轮：§5.6 插件运行时接入（目录插件注册表）+ config 面占位升级。

---

## 1. 一句话交接

QuickJS 桥主线的落地顺序步骤已完成四步：**§5.2 引擎**（`HostApi` 注册进 rquickjs
全局 `host`，异步泵打通）；**§5.3 manifest 驱动装载**（`plugin.json` 声明 host 面 →
按声明最小权限授面，未声明面不注入 JS）；**§5.4 接真 LLM**（桥层默认
`llm_complete_stream` 经新增 `llm_port()` 走内核 `LlmPort`，组合根 `Runtime::js_bridge`
把聚合 LLM 接进桥）；**§5.5 tools/session 面接线 + 真 JS 插件跑通**（`LoadedPlugin`
+ `Runtime::js_bridge` 组合，JS 插件调 llm/tools/session 全链路）。桥层本身已可跑
JS 插件打真 provider（log/config/tools/llm/session 全 face）。

## 2. 落地清单（四个 commit）

| Commit | 内容 |
|---|---|
| `09f296f` | §5.2 引擎：`bm/quickjs-bridge/src/js.rs` 的 `JsBridge`；HostApi 8 面全注册；JSON 字符串跨桥 + JS 包装层；`exec`/`exec_async`/`eval_value`/`call_async` 四入口 |
| `b282257` | §5.3 manifest：`bm/quickjs-bridge/src/plugin.rs`（新）`JsPluginManifest` + `ALL_HOST_FACES` 面白名单 + `LoadedPlugin::load`；`JsBridge::new_with_faces` 按面注册 + 动态包装层 |
| `f5261c4` | §5.4 接真 LLM：`bm/quickjs-bridge/src/host.rs`（新）+ `HostApi::llm_port()` + 默认 `llm_complete_stream`；`bm/assembly/src/js_host.rs`（新，`RealHost`）+ `Runtime::js_bridge(faces)` 装配入口 |
| 本轮 | §5.5：`RealHost` 接 `ToolRegistry`+`ToolGate` / `SessionStore`（tools/session 面）；真 JS 插件全链路测试；**修 qjs 堆损坏**（去掉常驻 `rt.drive()` 泵） |

## 3. 验证矩阵（全绿）

| 项 | 结果 |
|---|---|
| `cargo test -p quickjs-bridge` | 28 全过（原 16 + §5.4 新增 12） |
| `cargo test -p bm-assembly` | 15+1+6 全过（新增 tools/session 面 2 + 真 JS 插件 2 + swap 后 js_bridge 1） |
| `cargo test --workspace` | 全 suite ok，0 FAILED（quickjs-bridge 8 连跑稳定） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `bash scripts/verify-gate1.sh` | GATE1: ALL PASS |

## 4. 关键机制（下轮不重踩）

**异步泵架构（§5.5 修订，重要）**：rquickjs 0.6 的 `Async` fn 注册成 JS Promise 后
future 压进 runtime spawner。驱动方式是 `async_with!`（`AsyncContext::async_with`）：
`WithFuture::poll` **内部持锁并驱动 spawner**（`spawner.poll` + `execute_pending_job`），
**无需单独 spawn `rt.drive()` 泵线程**——官方 `async_test_case` 同样不 spawn。
> ⚠️ §5.2 曾加常驻 `rt.spawn(js_rt.drive())` 泵，实测导致 **`0xc0000374` 堆损坏**
> （异步路径间歇崩，mem::forget 探针证明与 Drop 无关）：pump 线程与 `block_on` 线程
> **双线程并发 poll 同一 qjs spawner**。去掉后单线程驱动，8 连跑稳定。**不要再加 drive 泵。**

**跨桥类型（rquickjs 0.6 的坑）**：
1. `IntoJsFunc` 闭包不能返回带 `'js` lifetime 的 `Value`/`Ctx` → host 原始函数统一返回
   **JSON 字符串**（`Result<String>`），JS 侧包装层 `JSON.parse` 还原。
2. `eval_promise` resolve 出**模块命名空间**而非脚本值 → 异步结果走「顶层 `await` 写
   全局变量 + 同步 `eval_value` 读回」（`call_async`）。IIFE 不被顶层 await 驱动。
3. `Async` fn 同步调用返回 Promise：包装层 `host.tools.invoke`/`host.llm.complete`
   必须 async 方法，调用方 await。

**接真 LLM（§5.4，组合根唯一装配纪律）**：quickjs-bridge 不依赖 provider/web-server——
`HostApi::llm_port()`（默认 `LLM_UNAVAILABLE` 诚实失败）+ 默认 `llm_complete_stream`
（经 `llm_port()` → `GenerateOptions` → `LlmPort::stream` → 块流翻译成 `CompletionChunk`）。
真实装配在 **bm-assembly**：`js_host::RealHost` 把 `Runtime.llm`（与 agent-loop 共享
同一 `Arc`，swap_llm 下一请求生效）接进桥；`Runtime::js_bridge(faces)` 是桥装配唯一入口。

**tools/session 面（§5.5）**：`RealHost` 接 `ToolRegistry`+`ToolGate`——未注册
`tool-not-found`（对齐 MockHost 契约）、注册未启用 `tool-disabled`（fail-closed）、
清单只列已启用；接 `SessionStore`——`append` 反序列化内核 `SessionEvent`（外部 tag
形状 `{"UserMessage":{...}}`，**不是** `{type:...}`）追加 / `get` 快照 / `poll` 游标续读
（拉模型，禁止 JS 回调重入）。

**测试纪律（本轮踩坑）**：`JsBridge` 内部 `block_on` 自带 runtime——**不能在
`#[tokio::test]` 里调桥方法**（`Cannot start a runtime from within a runtime`）；
用 `#[test]` + `futures::executor::block_on` 包非桥异步。

**最小权限授面（§5.3）**：面粒度=单方法；manifest `host` 缺省=空集；未知面名拒绝解析；
未授面访问抛 ReferenceError（测试钉死）。权限治理单点 = 面白名单 + manifest。

## 5. 下轮指针

1. **§5.6 插件运行时接入**（QuickJS 桥主线下一实质步骤）：`Runtime::js_bridge` +
   `LoadedPlugin` 收敛为「目录插件注册表」——扫描 plugins 目录 → manifest 装载 →
   按面授面；config 面从占位升级（settings 白名单 + 永不含 secret）。
2. **待办 2：dsh-rust-plugins 更新流程**——源仓已打 tag `absorbed-into-boenmind-2026-08-19`
   锁 commit，台账 `docs/PLUGIN_ABSORPTION_LEDGER_2026-08-19.md`；后续按台账流程吸收。
3. **待办 3：web-server 验证脚本路径**——conformance / gate25 / m3-r3 脚本如需跑，
   用新路径（hot-replace / hot-upgrade-transition 已更新为新布局）。

## 6. 环境纪律（沿用）

- 每轮先杀 web-server：`taskkill //F //IM web-server.exe`
- 跑服务用 release（debug exe 2GB 超 PE 限制）
- 验证三件套：cargo test / clippy / gate1（BoenMind 主仓）
- rquickjs 版本锁 0.6.2（crates.io 0.12.2 不在本地缓存，离线不可用）
