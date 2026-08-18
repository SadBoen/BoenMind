# Grok 越界专项审查（shell 组）

> 通道: ZCode 自定义 provider grok-4.6 @ https://api.apikey.fun/v1（未传 thinking 参数，catalog 默认 reasoningEffort=high，思考随 reasoning_content 返回）

## 思考过程（reasoning_content）

The user wants a security/architecture review focused on "越界" (boundary violations) of the BoenMind Rust microkernel shell/adapter layer. I have all the source inline. I need to produce a Chinese Mark...

## 审查结论（content）

# 壳层/适配层越界审查（Grok）

审查范围：`kernel-llm`、`web-server`、`headless`。判据以可执行行为为准。

---

## 发现

### GROK-S-01 — P1 — 壳层 poke `Runtime` 全 pub 字段装配 LLM
**位置：** `web-server/src/main.rs:248-250`；`headless/src/main.rs:87-99`

**证据：**
```248:250:web-server/src/main.rs
        runtime.llm = Arc::new(MultiProviderLlm::new(ports));
        runtime.provider = default_provider;
        runtime.model = default_model;
```
```87:89:headless/src/main.rs
    // Runtime 字段公开可改，直接替换 llm。
    rt.llm = Arc::new(kernel_llm::ScriptLlm::new(
```

**越界理由：** 组合根本应是 `kernel-assembly` 的构造 API。壳层在 `headless_with_max_steps` / `headless` 之后改内部端口，等于把装配契约撕开。

**修复：** `Runtime` 字段改 `pub(crate)` 或提供 `Runtime::with_llm(...)` / `replace_llm` 受控方法；壳层只调组合根。

---

### GROK-S-02 — P1 — web-server 越层依赖 `kernel_loop` / `kernel_llm` 并操作会话
**位置：** `web-server/src/api.rs:10-12, 24-26, 154-156`；`web-server/src/main.rs:17-19`

**证据：**
```10:12:web-server/src/api.rs
use kernel_assembly::Runtime;
use kernel_contracts::llm::LlmModelInfo;
use kernel_contracts::session::{SessionEvent, SessionHeader, SessionId};
use kernel_loop::ReactLoopAgent;
```
```154:156:web-server/src/api.rs
pub struct ProviderRuntime {
    ...
    pub adapter: Option<Arc<kernel_llm::OpenAICompatLlm>>,
```
`session_fork` 直接 `agent.session().append` + `runtime.persist.append_events`（`api.rs` 约 646–654）。

**越界理由：** 兼容层应只依赖 `kernel-assembly` + `kernel-contracts`。持有 `ReactLoopAgent`、具体 `OpenAICompatLlm`、手写 fork 持久化序列，是越层接线 + 内核语义泄漏。

**修复：** 会话句柄与 fork/restore 全部收进 assembly 门面；web-server 只拿 `SessionId` 与端口 trait。

---

### GROK-S-03 — P1 — 壳层复刻翻译游标 / wire seq
**位置：** `web-server/src/api.rs:268-294`；`web-server/src/events.rs:36-174, 238-256`

**证据：** `attach_event_bus` 每会话持有 `EventTranslator` + 自增 `seq`；`translate_events` 再实现一套「跳过 SessionStarted、seq 从 0」规则。`ws.rs` mux 基线又用 `translate_events(&events).len() - 1` 算 `lastSeq`。

**越界理由：** 配对/游标/seq 是内核投影语义。双实现（总线实时 vs history 批译）必然漂移（注释已承认 SessionStarted 占 `record.seq` 但不能用）。

**修复：** 内核发出已编号的 wire 投影或稳定 `seq`；壳层只转发。

---

### GROK-S-04 — P2 — `session.fork` 在壳层实现追加/持久化序列
**位置：** `web-server/src/api.rs:584-668`

**证据：** 壳层扫描 `TurnEnded` 锚点、改 header、循环 `append` + `persist.append_events`。

**越界理由：** 事件追加顺序与 torn 修复本属 persist/loop。fork 失败中途可留下半会话。

**修复：** `Runtime::fork_session(source, at_seq)` 原子实现。

---

### GROK-S-05 — P2 — LlmPort：「错误以 Err 结束流」vs 全程 Finish 呈现
**位置：** `kernel-llm/src/openai.rs:469-476, 492-504, 520-528`；`kernel-llm/src/multi.rs:48-61`

**证据：** 未知 provider、缺 key、TRANSPORT、HTTP 错、畸形 SSE、STREAM_CLOSED 全部 `yield Ok(Finish(Error{code}))`，注释写明「不产 Err，否则 loop torn 把 code 盖成 LLM_STREAM」。

**越界理由：** 若契约写的是 `Err` 结束流，适配器为迁就 loop torn 改契约，错误语义泄漏到适配器；`request_id` 仅在 HTTP 非 2xx 的 `extra` 里，流中 TRANSPORT 无 `requestId`。

**修复：** 契约明确「终态只走 Finish」并改 loop torn；或适配器 `Err(LlmError)` 且 loop 保留 `code`/`requestId`。不要两边各写一半。

---

### GROK-S-06 — P2 — `thinking`/`reasoning_effort` 上 wire 不完整
**位置：** `kernel-llm/src/openai.rs:387-394`

**证据：**
```387:394:kernel-llm/src/openai.rs
        if let Ok(Some((thinking, effort))) =
            Self::resolve_thinking(request, None, None)
```
`adapter_thinking`/`adapter_effort` 恒 `None`。`resolve_thinking` 的 deployment 锁、模型档位只在单测 `thinking_resolution_mirrors_serialize_spec` 里活着。`build_request` 对 `translate_messages` 用 `.expect("translate")`（约 361 行），失败会 panic 而非 Finish。

**修复：** 从 `resolve_model` / 静态清单传入 reasoning 默认与锁；`translate` 失败 `yield Finish(Error)`；禁止 `expect`。

---

### GROK-S-07 — P0 — Host/Origin 栅栏未覆盖 `/api/respond` 与 `/api/session.export`
**位置：** `web-server/src/lib.rs:164-214`（respond）、`216-286`（export）；对比 `handle_rpc`/`handle_ws_upgrade` 有栅栏

**证据：** `handle_respond` 只查 `Content-Type`；`handle_session_export` 只查 query。二者均未调用 `is_trusted_api_request`。

**越界理由：** DNS-rebinding / 跨站页面对 loopback 发 POST 可通过审批、GET 可导出全部会话 JSONL。特权表再严也绕不开这两条。

**修复：** 与 RPC 相同套用栅栏 A；export 另考虑鉴权。SSE 若将来挂路由必须同样加栅栏（当前 `router` 未挂 SSE，见疑点）。

---

### GROK-S-08 — P1 — 特权表与「特权」实现不一致；Origin 丢端口
**位置：** `web-server/src/trust.rs:7-24, 148-163`；`api.rs` 中 `host.listDirectory`/`host.createDirectory` 注释「特权」但不在 `PRIVILEGED_METHODS`

**证据：** 15 项表无 `host.listDirectory`、`host.createDirectory`。`extract_url_host` 丢弃 Origin 端口，只比 hostname：`http://127.0.0.1:evil` 对 `Host: 127.0.0.1:3080` 可通过 Origin 段。

**越界理由：** LAN `--trusted-host` 时目录枚举/创建不走 loopback-pin。Origin 端口语义与 Host 规范化不对称。

**修复：** 凡改文件系统的方法进特权表并 pin loopback；Origin 比较 `host:port`（`:80` 归一与 `parse_authority` 一致）。

---

### GROK-S-09 — P0 — `host.openPath` 命令注入面
**位置：** `web-server/src/rpc_m3.rs:67-87`

**证据：**
```73:77:web-server/src/rpc_m3.rs
        let opened = std::process::Command::new("cmd")
            .args(["/C", "start", "", path])
            .spawn()
```
Unix：`xdg-open` + 原始 `path`。无 scheme/路径白名单。Windows `cmd /C` 对 `&|^` 等元字符敏感。该方法在特权表内，但若 GROK-S-07 类入口或 Origin 松动被打到，即 OS 副作用。

**修复：** 拒绝控制字符与 `cmd` 元字符；Windows 用 `ShellExecuteW`/`cmd /C start ""` 前严格校验为现存绝对路径；禁止 UNC/`file:` 混用未定义行为。

---

### GROK-S-10 — P1 — SSE 行缓冲无界 + 无读超时
**位置：** `kernel-llm/src/openai.rs:102-107, 530-532, 577+`

**证据：** `Client::builder()` 仅 `connect_timeout(15s)`，无 `timeout`/`read_timeout`。`line_buf: Vec<u8>` 只在 `\n` 处 drain，恶意/故障上游可无限拉长一行。

**越界理由：** 适配器信任边界被上游 SSE 撑爆（内存 DoS）；挂死读破坏 fail-loud。

**修复：** 行长上限（如 1–8MiB）超限 `MALFORMED_RESPONSE`；设置整体/逐块读超时。

---

### GROK-S-11 — P2 — 错误路径吞结果 / `block_on` 嵌套
**位置：**  
- `web-server/src/api.rs:221-226, 290, 1006`：`let _ = host_events_tx.send`；`let _ = tx.send`；`let _ = agent.run_turn`  
- `web-server/src/rpc_m3.rs:389-396`：`futures::executor::block_on` 查 persist  
- `web-server/src/main.rs:281-282, 329+`：启动时 `block_on(list/restore)`，随后再 `Runtime::new().block_on`  
- `api.rs:395-397`：`list_sessions().await.unwrap_or_default()` 失败变空列表

**越界理由：** 回合失败静默；在已有 tokio 工作线程上 `block_on` 有死锁风险；持久化错误装成「无会话」。

**修复：** `run_turn` 失败广播 `host/session-status` + 错误事件；`parent_available` 改为 `async`；restore 用同一 runtime；list 失败返回 `internal`。

---

### GROK-S-12 — P3 — headless 壳层手写事件落盘模拟 kill-9
**位置：** `headless/src/main.rs:147-185`

**证据：** abort 模式绕过 loop，直接 `append` + `persist.append_events` 再 `process::abort()`。

**越界理由：** 门禁故意越层，可接受为测试夹具，但把「配对完整」规则又抄了一份 `verify_tail`（与内核 repair 双实现）。

**修复：** 断点注入放进 loop 测试钩子；tail 断言复用内核 API。

---

## 疑点

- **SSE 路由：** `ws.rs` 有 `handle_mux_sse`/`handle_host_sse`，`router` 未挂载；mux SSE 无界推送目前不可达。若后续挂上且不加栅栏，升级为 P0。
- **Cargo 未使用依赖：** 本包未给 `Cargo.toml`，无法证实「声明未用内核 crate」。从 `use` 看 web-server **使用了** `kernel_loop`/`kernel_llm`，问题是不该直接依赖，而非死依赖。
- **`list_models` 远程 vs 契约：** `list_models` 只回静态清单，探测在 `list_models_remote`（web-server `llm.discoverModels` 直调适配器）——又一次越层，并入 GROK-S-02。
- **goal 状态机在 web-server：** 注释自称 wire 在壳、语义在插件；当前无插件则壳层已实现 CAS/投影，属有意分层，暂不单列 P 级。

---

## Verdict：**FAIL**

P0（未栅栏的 respond/export、`openPath` 注入）足以否决「信任边界闭合」。另有系统性越层装配与 seq/fork 双实现。

### 修复顺序
1. 给 `/api/respond`、`/api/session.export`（及未来 SSE）套 Host/Origin + 特权策略（GROK-S-07）。  
2. 硬化 `host.openPath`；`listDirectory`/`createDirectory` 纳入 loopback-pin；Origin 带端口比较（GROK-S-08/09）。  
3. SSE 行长与读超时（GROK-S-10）。  
4. 停止 poke `Runtime`；LLM/会话门面收入 assembly（GROK-S-01/02/04）。  
5. seq/翻译收进内核；LlmPort 终态与 torn 对齐；thinking 真上线（GROK-S-03/05/06）。  
6. 去掉吞错与嵌套 `block_on`（GROK-S-11）。
