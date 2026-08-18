# HANDOFF：M4 完成交接——官方测试集主线 P0+P1 全落地（2026-08-18）

> 状态：**M4 全部任务完成并已推送**。本轮做了交接文档 `docs/HANDOFF_M4_2026-08-18.md`
> 里列的 P0 三项 + P1 五项 + 官方 spec 镜像单测收尾，以及本交接 §4 挂账 A/B
> （归因头 + requestId 投影，随附轮完成）。下一轮 = 从 §4 的 P2 队列开题
> （都是 P0/P1 之后的自然延续），或回插件面续建。验证矩阵全绿见 §3。

---

## 1. 一句话交接

**P0 三项（错误归一化 / 上下文超限·配额内容分类 / abort 语义）与 P1 五项（wire 形状补全）
全部落地，官方 spec 镜像单测新增 23 条**。当前基线：cargo test --workspace 90 全过、
clippy 零警告、conformance 17/17、gate25 PASS、m3-r3 PASS、hot-replace ALL PASS。
下一轮无需再动 P0/P1 代码（除 §4 列出的挂账项），可直接从 P2 开题或插件面续建。

---

## 2. 本轮落地清单（commit 见 git log）

### 2.1 P0-1 错误归一化（对齐 `adapter-failure.spec.ts` + `service.spec.ts`）
- `kernel-contracts::LlmError` 加结构化字段：`code/status/provider_retry_after_ms/request_id`
  （serde 缺省 None 不破坏既有构造）；`structured(...)` 构造器 + `to_failure()` 归一化
  （message 空 → `"LLM adapter failed"`、code 缺失 → `UNKNOWN`，镜像 normalizeLlmFailure 兜底）。
- 新增 `kernel-contracts::error::FailureInfo`（LlmFailure 终态形状：message/code/status/
  provider_retry_after_ms/request_id）。
- `FinishReason::Error` 加 `extra: Option<FailureInfo>`：wire 携带结构化事实时逐字透传
  （status/providerRetryAfterMs/requestId），None 时仍只出 message/code（toEqual 精确形状）。
- 所有既有 `FinishReason::Error{...}` 构造点补 `extra: None`（openai.rs 7 处 + multi.rs 1 处）。

### 2.2 P0-2 上下文超限/配额内容分类（对齐 `adapter.spec.ts` HTTP 分类用例）
- `map_http_code(status, body)` 重写为镜像 DSH `httpErrorCode`：
  - 401/403 → AUTH（最优先）；quota 判词（任意 status）→ `QUOTA`；429 → RATE_LIMIT；
    400 + 上下文超限判词 → `CONTEXT_WINDOW_EXCEEDED` else INVALID_REQUEST；
    `status >= 500` → SERVER（502 同样归 SERVER，修正旧实现的仅 500/503）；其余 → HTTP_\<status\>。
  - 消息取 `error.message`（JSON），无则状态行 `HTTP <status>`（对齐 spec 两条 keep-status-line 用例）。
- 判词正则逐字镜像 DSH `error.ts`：`is_context_window_exceeded`（5 条正则）、
  `is_quota_exceeded`（5 条正则），分类 detail = error.code + type + message 拼接。
- `parse_retry_after`（秒数或 HTTP-date；0/非法/过去 → None）+ 读
  `x-request-id` 回退 `x-deepseek-request-id` 作为结构化事实随 failure 上 wire。
- 新增依赖：`regex`（workspace）、`chrono`（kernel-llm）、`tokio`（kernel-contracts）。

### 2.3 P0-3 abort 语义（对齐 `adapter.spec.ts` abort 用例；本轮最大块）
- `kernel-contracts::AbortSignal`：`AtomicBool`（同步查询）+ `tokio::sync::watch`
  （`wait_aborted()` 异步等待无竞态；**永久 receiver 保底**——watch 的 send 在无活跃
  receiver 时返回 Err 且不更新值，须持有 `_keep_alive` 保证任意时序 abort 都生效）。
  `abort()` 幂等；Clone 共享。
- `GenerateOptions.signal: Option<AbortSignal>`（`#[serde(skip)]` 不上 wire/不落盘）。
- openai.rs 三处穿透：
  1. **预 abort**：请求发出前已 abort → 直接终态 `Finish(Cancelled)`，不碰传输；
  2. **send() 阶段**：`tokio::select!{biased; send_fut, wait_aborted}`——响应头到达前
     abort 即终态；abort 后的连接错误一律按取消呈现（对齐 DSH `if (signal.aborted) throw error`）；
  3. **流中**：主循环 select 竞争"下一块"与 `wait_aborted`——挂起的网络读可被打断，
     恰一个 aborted finish chunk（select_biased 优先流分支防正常 EOF 误判）。
- `ReactLoopAgent`：`cancel` 槽 + `abort()` 方法；`run_turn` 每回合新建信号注入
  `GenerateOptions`，Drop guard 清理槽位（跨 step 共用同一信号）。
- loop 消费 `FinishReason::Cancelled` → `TurnEndReason::Aborted{reason}`（独立 reason，
  不再归 Error{ABORTED}）；wire 形状 `{kind:'aborted', failure:{code:'ABORTED'}}` 已由
  `FinishReason::to_wire` 保证。
- web-server `session.cancel`：找到 session → `agent.abort()`（原空实现 M1 注释移除）。

### 2.4 P1-1/2/3 wire 形状（对齐 `serialize.spec.ts` + `adapter.spec.ts`）
- `build_request` 恒带 `stream_options:{include_usage:true}`。
- `GenerateOptions` 加 `reasoning_effort/thinking/purpose`；`resolve_thinking` 镜像
  `resolveThinking`：未知档位 → UNSUPPORTED_REASONING_EFFORT；`off` → thinking disabled
  且绝不上 effort；`low/high/max` → enabled + effort；purpose=session-title/compaction
  强制 disabled；deployment 锁 disabled + 显式非 off effort → 拒绝。
  wire 输出 `thinking:{type}` + `reasoning_effort`（省略时不上）。
- `translate_messages` 重写镜像 `serializeMessages`：text 块合并；tool-result 独立
  `role:tool` 消息（空输出哨兵 `'(no output)'`）；混合 user text+tool-result 拆多条；
  assistant 空串 content 永非 null；reasoning passback 只在 tool-call 轮
  （`reasoning_content`）；image 拒收点预留（内核 enum 无 image 变体，未来加块即拒
  UNSUPPORTED_CONTENT）。返回 `Result`（拒收路径将来用）。

### 2.5 P1-4 settings-conflict revision（对齐 `api-proxy-config.spec.ts`）
- `AppState.settings_revisions` 单调计数；写成功 +1；写带 `expectedRevision` 不匹配 →
  `settings-conflict{ns, expected, actual}`；`settings_view` 带真实 revision。

### 2.6 P1-5 session.search astral 边界（发现并修真 bug）
- `make_snippet` 原实现 `text.find`（字节偏移）与 `chars().skip(start_char)`（字符数）
  混用——query 前的多字节字符（中文/emoji）会把窗口起点算偏、甚至丢匹配词。
  修复：字节偏移先换算成 char 位置再取窗口；切分全走 `chars()` 绝不劈 surrogate pair。

### 2.7 官方 spec 镜像单测（+23 条，合计 90）
- openai.rs 新增：分类判词表（service.spec 6+4、quota 表 5+3、413 优先）、HTTP 状态行保留、
  Retry-After 秒/date/非法、`to_failure` 兜底、failure wire 形状、预 abort、流中 abort
  （mock TCP 端点 + 延迟首帧）、wait_aborted 幂等×2、SSE 序列（空首增量不开块 + 推理/
  文本独立块 + 默认 stop）、空流 STREAM_CLOSED、DONE 后停止、MALFORMED_RESPONSE、
  EMPTY_RESPONSE after usage、stream_options、resolveThinking 全分支、空输出哨兵、
  混合拆条、passback、tools 数组、thinking/effort wire。
- web-server api.rs 新增：snippet astral 不劈 surrogate pair、字节偏移回归（修复前丢
  query 词）、空白折叠。

---

## 3. 验证矩阵（本轮全绿）

| 项 | 结果 |
|---|---|
| cargo test --workspace | **91 全过**（基线 67 → 91，+24 镜像单测） |
| cargo clippy --workspace --all-targets | 零警告 |
| conformance（3081） | 17/17 |
| gate25（3079，全新 db + BM_TEST_HOOKS=1） | PASS |
| m3-r3（3079 同上） | PASS |
| hot-replace-verify（3082 自起） | ALL PASS |

## 4. 遗留/挂账（下一轮开题候选）

| # | 项 | 说明 |
|---|---|---|
| ~~A~~ | ~~请求归因头 `x-deepseek-harness-session-id/user-id` + user-agent~~ | ✅ **已落地**（随附轮）：openai.rs 恒发 user-agent（`boenmind/0.1.0 (+url)`）+ `x-deepseek-harness-user-id`（`~/.boenmind/.anonymous-user-id` 持久 UUID v4，`wx` 独占创建防并发、best-effort 不阻塞）+ session-id（按需）+ compact（compaction 用途）；list_models_remote 同带 |
| ~~B~~ | ~~`requestId` 上 message/历史~~ | ✅ **已落地**（随附轮）：`TurnEndReason::Error.request_id` 从 finish failure 结构化事实投影 → events wire `error.requestId`（无则省略） |
| C | settings/credentials 文件层（0600/注释保留/热发布/writer 锁） | P2 登记，挂存储工程 |
| D | session-query 全套（SESSION_QUERY_* / cursor / SQLite 索引） | P2 登记，插件面 |
| E | BlockAssembler replayState 剪枝 / message 冻结 / retry-policy schema | P2 登记 |
| F | AbortSignal 线程性 | 现基于 tokio watch（仅 tokio 场景可靠）；无 tokio 场景只有 is_aborted 同步查询，够用 |
| G | 归因头做后端按需扩展时注意 hot-replace 脚本断言不变 | 改 openai.rs 后跑 hot-replace-verify |

## 5. 环境与纪律（沿用，无新增）

- 开工杀残留：`taskkill //F //IM web-server.exe`；收尾同杀 + `rm -rf .tmp/*`。
- 固定端口：conformance=3081、gate25/m3-r3=3079、hot-replace=3082（自起）。
- 新流协议铁律：**错误一律以 finish 呈现**（Err chunk → loop torn 分支覆盖真码 + 双回合收尾）。
- 真 provider 验证计费：非必要不跑 m3-real-provider-verify.mjs。
- `.tmp/dsh-src`、`.tmp/limbo-src` 源码快照勿提交勿删（git clean 可重拉）。

## 6. 文件地图（新增/变更）

| 文件 | 内容 |
|---|---|
| `docs/HANDOFF_M4_FINISH_2026-08-18.md` | 本交接（下轮指针） |
| `docs/CONTRACT_LEDGER_DSH.md` | 实施进度段追加 M4 段 |
| `docs/HANDOFF_M4_2026-08-18.md` | 已标注完成（状态段指向本交接） |
| `kernel/kernel-contracts/src/error.rs` | LlmError 结构化字段 + FailureInfo |
| `kernel/kernel-contracts/src/llm.rs` | AbortSignal + GenerateOptions.signal/reasoning_effort/thinking/purpose + FinishReason.extra |
| `kernel/kernel-llm/src/openai.rs` | map_http_code 内容分类 + parse_retry_after + abort 穿透 + translate 重写 + resolve_thinking + stream_options + 23 镜像单测 |
| `kernel/kernel-llm/src/multi.rs` | extra:None |
| `kernel/kernel-loop/src/lib.rs` | agent.abort() + 回合信号 + Cancelled→Aborted |
| `kernel/web-server/src/api.rs` | session.cancel 接线 + settings revision + make_snippet char 修复 + snippet 测试 |
