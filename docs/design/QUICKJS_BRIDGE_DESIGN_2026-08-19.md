# QuickJS 桥设计基线（2026-08-19）

> 状态：**落地中**。host 面契约已定稿（§4）+ rquickjs 桥已实现（§5.2 完成：
> `HostApi` 注册进全局 `host` + 异步泵打通，11 测试全绿）；§5.3 manifest 装载、
> §5.4 接真 LLM 待后续。本文件是实现的契约，不是 wiki。

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

## 6. 相关文件

- 实测数据：`.tmp/quickjs-bench/QUICKJS_RUNTIME_REPORT_2026-08-19.md`
- grok 评审：`.tmp/grok-quickjs-plan-review.md` / `.tmp/grok-arch-final-review.md`
- 占位 crate：`bm/quickjs-bridge/src/lib.rs`
