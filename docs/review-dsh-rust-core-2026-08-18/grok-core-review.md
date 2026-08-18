# Grok 越界专项审查（core 组）

> 通道: ZCode 自定义 provider grok-4.6 @ https://api.apikey.fun/v1（未传 thinking 参数，catalog 默认 reasoningEffort=high，思考随 reasoning_content 返回）

## 思考过程（reasoning_content）

The user wants a review report focused on "越界" (boundary violations) based on the provided source code. I must not use tools and output the report immediately in Chinese Markdown.
# BoenMind Rust 微内核越界审查报告

## 审查发现

**GROK-C-01** | P0 | kernel-contracts/src/llm.rs:148 | `LlmResolvedModelInfo { ...

## 审查结论（content）

# 越界审查报告（Grok / BoenMind kernel）

审查范围仅限本消息内联源码。判据以可执行行为为准。

---

## GROK-C-01 · P1 · 分层守卫盲区：漏检 `web-server`，层表与 README 不一致

**位置：** `kernel-assembly/tests/crate_boundaries.rs:16-27`、`README.md:18-19`、`web-server/Cargo.toml:8-13`

**证据：**
```16:27:kernel-assembly/tests/crate_boundaries.rs
fn layer_of(crate_name: &str) -> Option<u32> {
    Some(match crate_name {
        "kernel-contracts" => 5,
        ...
        "headless" => 1,
        _ => return None,
    })
}
```
README 写 layer 1 = `headless/web-server`；守卫把未知 crate 直接 `continue`，`web-server` 永不检查。`web-server` 已依赖 `kernel-loop`/`kernel-session`/`kernel-llm`（层 3/4），当前碰巧向下，但门禁对层 1 第二入口无效。

**越界理由：** 边界守卫是分层契约的执行器；漏 crate = 向上依赖可静默合入。

**最小修复：** `layer_of` 加入 `"web-server" => 1`；未知 workspace crate 应变硬失败而非跳过。

---

## GROK-C-02 · P2 · 守卫解析可绕过 / 断言过弱

**位置：** `crate_boundaries.rs:30-44`、`93`

**证据：** 按行 `find('=')` 取左边名字；`[dev-dependencies]` 与正式依赖同等对待；注释行、`# kernel-loop =`、表格外 crate 均可漏报。`assert!(checked >= 8)` 只数目录，不要求 `web-server`/全部层 1。

**越界理由：** 门禁可被格式或未登记 crate 绕过，分层纪律不可执行。

**最小修复：** 用 `toml` 解析 `[dependencies]`/`[build-dependencies]`；未知 `kernel-*` 报错；`checked` 与登记表精确相等。

---

## GROK-C-03 · P1 · 契约层越权实现（contracts 不是“只定义形状”）

**位置：** `kernel-contracts/src/lib.rs:3-6`、`bus.rs:26-62`、`llm.rs:269-287`、`ports.rs:125-136`

**证据：** 文档称“不包含任何业务实现”；实际提供完整 `EventBus`（含 panic 吞没）、`AbortSignal`、`LlmPort::resolve_model` 默认体（`list_models(...).await.unwrap_or_default()`）、`UnavailablePluginRuntime`。

**越界理由：** 层 5 揽了进程内运行时与失败策略，上层无法替换总线/解析失败语义而不改契约 crate。

**最小修复：** `EventBus`/默认 `resolve_model` 下沉到 session/llm/assembly；contracts 只留 trait + DTO。`resolve_model` 至少不得 `unwrap_or_default` 吞 `list_models` 错误。

---

## GROK-C-04 · P0 · logged-means-persisted 在错误路径被 `let _ =` 拆掉

**位置：** `kernel-loop/src/lib.rs:73-76`、`287`、`321`、`349`、`392`、`406`

**证据：** 模块约定“每个事件 append 后立即落盘”。但 MaxSteps / 流 `Err` / 无 Finish / `Cancelled` / `Finish::Error` 均为：
```rust
let _ = self.persist(&rec).await;
return Err(...);
```
内存已 `append`，磁盘失败被丢弃。

**越界理由：** 破坏“事件日志=唯一事实源”与 fail-loud；恢复见不到已“发生”的 Turn Ended。

**最小修复：** 这些路径 `persist` 失败应优先返回 `LoopError::Persist`（或先 persist 再决定是否覆盖为业务 Err）；禁止 `let _ =`。

---

## GROK-C-05 · P0 · 权威源分叉：内存 seq/timestamp 与磁盘重写

**位置：**  
- `kernel-loop/src/lib.rs:263-269`：persist 只提交 `rec.event`，丢弃 `seq`/`timestamp`  
- `kernel-storage/src/lib.rs:192-214`：磁盘 seq = `MAX(seq)+1`，timestamp = `Utc::now()`  
- `kernel-storage/src/lib.rs:296-314`：`rewrite_events` 全表 DELETE+INSERT，seq 重排为 `i+1`，timestamp 全是 `now()`  
- `kernel-assembly/src/lib.rs:163-168`：恢复时 `SessionRecord::new` 再次 `Utc::now()`

**证据：** `SessionRecord` 信封从未进入 SQLite；恢复时时间戳全部伪造。`append_events` 与内存 `next_seq` 无对账，并发/双写可分叉。

**越界理由：** 日志信封本应是事实；存储与恢复重写 seq/时间，破坏对账锚点。

**最小修复：** persist API 写入完整 `SessionRecord`；`rewrite_events` 保留原 timestamp（或单独存 envelope JSON）；恢复用磁盘 timestamp/seq，禁止 `SessionRecord::new` 现造时间。

---

## GROK-C-06 · P1 · 契约“流以 Err 结束=torn”与 loop 用 Finish 呈现错误混用，且默认 Finish=Stop

**位置：** `kernel-contracts/src/llm.rs:5-8`、`261-264`；`kernel-loop/src/lib.rs:428-432`、`353-361`、`363-414`

**证据：** 端口写明错误以 `Err` 结束流、调用方以 **Finish 缺失** 判 torn。`BlockAssembler::finish()` 却 `unwrap_or(FinishReason::Stop)`。loop 另用 `assembler.finish.is_none()` 判 torn，同时又把 `Finish(Error|Cancelled)` 写成 Turn Ended 并 `return Err`——错误既是 Finish 又是 Err，与“Finish 之后无块 / torn=无 Finish”不完全一致。

**越界理由：** 实现把适配器错误策略（Finish vs Err）揽进 loop，契约双轨。

**最小修复：** 文档与实现二选一对齐：要么适配器错误只走 `Err`（Finish 仅正常/abort），要么允许 `Finish(Error)` 且 **不得** 默认 Stop。去掉“缺省 stop”。

---

## GROK-C-07 · P1 · 组合根 `Runtime` 全 `pub` + supervisor 依赖未装配

**位置：** `kernel-assembly/src/lib.rs:33-47`、`80`；`kernel-assembly/Cargo.toml:14`；`kernel-supervisor/src/lib.rs` 全文未被 `Runtime` 引用

**证据：** `llm/store/tools/gate/persist/plugin_runtime/provider/model/bus/max_steps` 全 pub，装配后可被外部 poke。`plugin_runtime` 写死 `UnavailablePluginRuntime`；层 3 的 `Supervisor` 已实现且被 assembly 依赖，但从未注入 `PluginRuntimePort`。

**越界理由：** 组合根职责是一次装配；事后可变字段把策略/端口所有权交给调用方。supervisor 在依赖图内却不在运行时图内，层 3 能力悬空。

**最小修复：** 字段改 `pub(crate)` + getter；提供 `with_plugin_runtime`；M1 可用薄适配把 `Supervisor` 接到 `PluginRuntimePort`，或从 Cargo.toml 去掉未用依赖并在守卫中说明“未装配”。

---

## GROK-C-08 · P2 · loop 把可变策略写进内核默认并吞工具参数错误

**位置：** `kernel-loop/src/lib.rs:51-53`、`348-352`、`478-481`

**证据：** `DEFAULT_MAX_STEPS = 32` 虽可覆盖，但 MaxSteps 的 `code: "MAX_STEPS"`、取消文案、工具 `from_str(...).unwrap_or(Null)` 均在 loop 内写死。非法 JSON 变成 Null 后走 schema/fail-closed，而非 fail-loud。

**越界理由：** 错误码与参数失败策略属装配/插件层；内核应传播解析失败而非静默降级。

**最小修复：** 非法 `arguments` 直接记 `ToolResult { is_error: true, output: parse error }` 且不要当合法 Null；错误码表外置或经 Runtime 注入。

---

## GROK-C-09 · P2 · EventBus 观察者 panic 被吞（fail-loud 反面）

**位置：** `kernel-contracts/src/bus.rs:3-4`、`54-60`

**证据：** `let _ = std::panic::catch_unwind(... listener ...)`。若观察者承担 persist 投影，失败对主链路不可见。

**越界理由：** 与“调用方必须显式处理、绝不静默跳过”（`error.rs:3-5`）冲突。loop 当前是同步 persist 而非 bus，风险在未来把 persist 挂到 bus 时放大。

**最小修复：** 至少计数/日志化 panic；关键订阅（persist）不得经可吞 panic 的总线。

---

## GROK-C-10 · P3 · `LlmPort::resolve_model` 默认吞错误

**位置：** `kernel-contracts/src/llm.rs:273`

**证据：** `let models = self.list_models(provider).await.unwrap_or_default();`

**越界理由：** 契约层默认实现 fail-soft，与 PortError/fail-loud 纪律相反。

**最小修复：** 失败应返回错误或让方法返回 `Result`。

---

## 疑点

- **D-1** `run_turn` 从不 `append(Turn Started)`，只有 Step/Turn Ended。`repair_interrupted_turn` 仍处理 Turn Started。README 称修剪“未配对 Step/Turn Started”。若生产从不写 Turn Started，该分支是死策略还是漏写事件——源码不足以判谁越界。
- **D-2** `web-server` 源码未内联，无法确认是否在 HTTP 层重写事件/seq 或反向依赖。仅能根据 Cargo.toml 判依赖方向。
- **D-3** `kernel-llm` 源码未给，无法核对 mock 是否用 Finish 代替 Err。
- **D-4** `create_session`：`Session::new` 已内存 append SessionStarted，`persist.create_session` 再写一条；若 persist 失败，内存会话已存在——双写/半创建，未看到补偿。更像一致性问题，是否算“权威越界”取决于是否视为同一事实两次。

---

## Verdict：**FAIL**

P0 已破坏“日志唯一事实源”与 logged-means-persisted（错误路径吞 persist、信封/时间戳在存储与恢复被重写）。分层守卫对 `web-server` 与解析方式存在可执行盲区；组合根可 poke 且 supervisor 未装配。

### 建议修复顺序

1. **P0** 去掉 loop 错误路径 `let _ = persist`；persist 失败 fail-loud。  
2. **P0** persist/load/rewrite/restore 使用完整 `SessionRecord`（保留 seq+timestamp）。  
3. **P1** 守卫登记 `web-server` + 未知 crate 失败；收紧 toml 解析。  
4. **P1** 对齐 Llm 流错误契约（Err vs Finish）；去掉默认 Stop。  
5. **P1** Runtime 字段收口；显式装配或删除 supervisor 依赖。  
6. **P2–P3** 工具 JSON fail-loud；EventBus/resolve_model 不再静默吞失败；contracts 去业务实现。
