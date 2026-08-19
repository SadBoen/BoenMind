# HANDOFF：QuickJS 桥落地完成（引擎 + manifest 授面 + 真 LLM + tools/session + 注册表）（2026-08-19）

> 状态：**§5.2–§5.6 全部落地并全绿**（09f296f + b282257 + f5261c4 + ab9bf41 + 本轮）。
> quickjs-bridge 从占位 crate 变为完整可用引擎：`HostApi` 全局 `host` + 异步泵 +
> manifest 最小权限授面 + 真 LlmPort 接线 + tools/session/config 面 + 目录插件注册表。
> 32+18 测试全绿；workspace + clippy + gate1 全过。
> 下轮：桥主线完成，接业务（web-server `--plugins-dir` 装配 / dsh-rust-plugins 吸收）。

---

## 1. 一句话交接

QuickJS 桥主线的落地顺序步骤**已全部完成**：**§5.2 引擎**（`HostApi` 注册进 rquickjs
全局 `host`，异步泵打通）；**§5.3 manifest 驱动装载**（`plugin.json` 声明 host 面 →
按声明最小权限授面，未声明面不注入 JS）；**§5.4 接真 LLM**（桥层默认
`llm_complete_stream` 经 `llm_port()` 走内核 `LlmPort`）；**§5.5 tools/session 面接线
+ 真 JS 插件跑通**；**§5.6 目录插件注册表 + config 面**。JS 插件可完整跑
`host.llm.complete` / `host.tools.*` / `host.session.*` / `host.config.get` /
`host.log`（8 面全通）。

## 2. 落地清单（五个 commit）

| Commit | 内容 |
|---|---|
| `09f296f` | §5.2 引擎：`bm/quickjs-bridge/src/js.rs` 的 `JsBridge`；JSON 字符串跨桥 + JS 包装层；`exec`/`exec_async`/`eval_value`/`call_async` 四入口 |
| `b282257` | §5.3 manifest：`plugin.rs`（新）`JsPluginManifest` + `ALL_HOST_FACES` + `LoadedPlugin::load`；`new_with_faces` 按面注册 |
| `f5261c4` | §5.4 接真 LLM：`host.rs`（新）+ `HostApi::llm_port()` + 默认 `llm_complete_stream`；`js_host.rs`（新 `RealHost`）+ `Runtime::js_bridge` |
| `ab9bf41` | §5.5 tools/session 面 + 真 JS 插件全链路 + 修 qjs 堆损坏（去 drive 泵） |
| 本轮 | §5.6 目录插件注册表（`registry.rs` `scan_plugins`/`PluginDir`）+ config 面白名单 + `Runtime::load_js_plugin`/`scan_js_plugins`/`js_bridge_with_config` |

## 3. 验证矩阵（全绿）

| 项 | 结果 |
|---|---|
| `cargo test -p quickjs-bridge` | 32 全过（注册表 +4） |
| `cargo test -p bm-assembly` | 18+1+6 全过（config 面 + load_js_plugin/scan_js_plugins 端到端） |
| `cargo test --workspace` | 全 suite ok，0 FAILED |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零警告 |
| `bash scripts/verify-gate1.sh` | GATE1: ALL PASS |

## 4. 关键机制（下轮不重踩）

**异步泵架构（§5.5 修订，重要）**：rquickjs 0.6 的 `async_with!` 展开的
`WithFuture::poll` **内部持锁并驱动 spawner**——**无需单独 spawn `rt.drive()` 泵**。
> ⚠️ §5.2 曾加常驻 `rt.spawn(js_rt.drive())` → **双线程并发 poll 同一 qjs spawner →
> `0xc0000374` 堆损坏**（mem::forget 探针证明与 Drop 无关）。去掉后单线程驱动稳定。
> **不要再加 drive 泵。**

**跨桥类型（rquickjs 0.6 的坑）**：
1. `IntoJsFunc` 闭包不能返回带 `'js` lifetime 的 `Value`/`Ctx` → host 原始函数统一
   返回 **JSON 字符串**（`Result<String>`），JS 侧包装层 `JSON.parse` 还原。
2. `eval_promise` resolve 出**模块命名空间** → 异步结果走「顶层 `await` 写全局变量 +
   同步 `eval_value` 读回」（`call_async`）。IIFE 不被顶层 await 驱动。
3. `Async` fn 同步调用返回 Promise：包装层 `host.tools.invoke`/`host.llm.complete`
   必须 async 方法。

**接真 LLM（§5.4）**：`HostApi::llm_port()`（默认 `LLM_UNAVAILABLE` 诚实失败）+
默认 `llm_complete_stream`（`llm_port()` → `GenerateOptions` → `LlmPort::stream` →
`CompletionChunk` JSON）。真实装配在 **bm-assembly**（唯一组合根）：`RealHost` 把
`Runtime.llm`（与 agent-loop 共享同一 Arc，swap_llm 下一请求生效）接进桥。

**tools/session/config 面（§5.5/§5.6）**：tools = `ToolRegistry`+`ToolGate`（未注册
`tool-not-found`、未启用 `tool-disabled` fail-closed、清单只列已启用）；session =
`SessionStore`（append 反序列化内核 SessionEvent **外部 tag 形状** `{"UserMessage":{...}}`，
get 快照 / poll 游标续读，拉模型禁回调重入）；config = 白名单
`HashMap<"{plugin_id}.{key}", value>`（命中返回、未命中 config-not-found，**永不返回
secret**——白名单即全部内容）。

**测试纪律**：`JsBridge` 内部 block_on 自带 runtime——**不能在 `#[tokio::test]` 里调
桥方法**；用 `#[test]` + `futures::executor::block_on` 包非桥异步。

**§5.6 目录插件注册表**：`scan_plugins` 递归扫描任意深度，plugin.json 缺失跳过，
损坏 fail-loud，同名 id 后扫覆盖（便于覆盖默认）；每插件一引擎（独立
AsyncRuntime + 上下文，天然隔离）。组合根入口：`Runtime::load_js_plugin(dir)` /
`scan_js_plugins(dir)` / `js_bridge_with_config(faces, config)`。

## 5. 下轮指针

1. **桥主线完成 → 接业务**：web-server 装配 `--plugins-dir`（扫描 → 逐个
   `load_js_plugin` → 引擎清单注册进 `PluginRuntimePort`，探针变 Ready）；或按
   业务顺序接 dsh-rust-plugins 吸收（见台账）。
2. **待办 2：dsh-rust-plugins 更新流程**——源仓已打 tag `absorbed-into-boenmind-2026-08-19`
   锁 commit，台账 `docs/PLUGIN_ABSORPTION_LEDGER_2026-08-19.md`；后续按台账流程吸收。
3. **待办 3：web-server 验证脚本路径**——conformance / gate25 / m3-r3 脚本如需跑，
   用新路径（hot-replace / hot-upgrade-transition 已更新为新布局）。

## 6. 环境纪律（沿用）

- 每轮先杀 web-server：`taskkill //F //IM web-server.exe`
- 跑服务用 release（debug exe 2GB 超 PE 限制）
- 验证三件套：cargo test / clippy / gate1（BoenMind 主仓）
- rquickjs 版本锁 0.6.2（crates.io 0.12.2 不在本地缓存，离线不可用）
