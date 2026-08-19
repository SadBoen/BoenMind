# HANDOFF：QuickJS 桥落地（引擎 + manifest 最小权限授面）（2026-08-19）

> 状态：**§5.2 + §5.3 已落地并全绿**（09f296f + b282257）。quickjs-bridge 从占位 crate
> 变为可用引擎：`HostApi` 注册进 rquickjs 全局 `host` + 异步泵打通 + manifest 驱动装载
> （按声明面最小权限授面）。16 测试全绿；workspace + clippy + gate1 全过。
> 下轮：§5.4 接真 LLM（桥层已就绪，只差组合根把真 LlmPort 接进 HostApi 实现）。

---

## 1. 一句话交接

quickjs-bridge 主线的两个落地顺序步骤已完成：**§5.2 把 `HostApi` trait 注册进
rquickjs 全局 `host`，异步泵打通**（AsyncContext 专用线程 + `rt.drive()` 泵 Promise）；
**§5.3 manifest 驱动装载**（`plugin.json` 声明 host 面 → 按声明最小权限授面，
未声明面不注入 JS）。桥层本身已可跑 JS 插件（log/config/tools/llm/session 全 face）。

## 2. 落地清单（两个 commit）

| Commit | 内容 |
|---|---|
| `09f296f` | §5.2 引擎：`bm/quickjs-bridge/src/js.rs` 的 `JsBridge`——AsyncContext 跑专用线程独立 tokio runtime，`rt.spawn(rt.drive())` 常驻泵 JS 任务；HostApi 8 面全注册；JSON 字符串跨桥 + JS 包装层；`exec`/`exec_async`/`eval_value`/`call_async` 四入口 |
| `b282257` | §5.3 manifest：`bm/quickjs-bridge/src/plugin.rs`（新）`JsPluginManifest`（plugin.json：id/name/version/entry/host）+ `ALL_HOST_FACES` 面白名单 + `LoadedPlugin::load`（目录→manifest→入口源码）；`JsBridge::new_with_faces` 按面注册 + 动态包装层 `build_wrapper`（只含已授面） |

## 3. 验证矩阵（全绿）

| 项 | 结果 |
|---|---|
| `cargo test -p quickjs-bridge` | 16 全过（5 契约 + 6 rquickjs 端到端 + 5 manifest/最小权限） |
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

**最小权限授面（§5.3）**：面粒度=单方法（`tools.invoke`/`llm.complete` 独立授面）；
manifest `host` 缺省=空集；未知面名拒绝解析（防拼错静默失效）；未授面访问抛
ReferenceError（测试钉死）。权限治理单点 = 面白名单 + manifest，rquickjs 不注入
fs/fetch。

## 5. 下轮指针

1. **§5.4 接真 LLM**（QuickJS 桥主线下一实质步骤）：JS 插件用 `host.llm.complete` 打
   真实 provider。**桥层已就绪无需改动**；需组合根（`bm-assembly`）把真 `LlmPort`
   （经 `assemble_providers` 装配的聚合 LLM）接进 `HostApi` 实现——目前 `MockHost`
   是测试用假实现，`HostApi` trait 在 `bm/quickjs-bridge/src/lib.rs`。
   参考：`bm-assembly` 的 `Runtime::apply_llm`（唯一装配出口，swap 聚合 LLM）。
2. **待办 2：dsh-rust-plugins 更新流程**——源仓已打 tag `absorbed-into-boenmind-2026-08-19`
   锁 commit，台账 `docs/PLUGIN_ABSORPTION_LEDGER_2026-08-19.md`；后续按台账流程吸收。
3. **待办 3：web-server 验证脚本路径**——conformance / gate25 / m3-r3 脚本如需跑，
   用新路径（hot-replace / hot-upgrade-transition 已更新为新布局）。

## 6. 环境纪律（沿用）

- 每轮先杀 web-server：`taskkill //F //IM web-server.exe`
- 跑服务用 release（debug exe 2GB 超 PE 限制）
- 验证三件套：cargo test / clippy / gate1（BoenMind 主仓）
- rquickjs 版本锁 0.6.2（crates.io 0.12.2 不在本地缓存，离线不可用）
