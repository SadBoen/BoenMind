# 官方测试集通过性差距登记（DSH vitest → Rust 内核对齐台账）

> 日期：2026-08-18。来源：`deepseek-harness` 源码 `packages/*/tests/*.spec.ts`（650 个
> spec 文件，npm 发布包已裁剪测试——源码 shallow clone 于 `.tmp/dsh-src`）。
> 方法：三子代理并行盘点 llm / session / host+settings+credentials+workspace+interaction
> 面官方断言 → 逐条对照 Rust 实现。**"通过官方测试集"的正解 = 我们的 Rust 实现逐条对齐
> DSH 源码的 wire 契约**（测试测什么我们就对齐什么）。

---

## 0. 一句话

**官方测试集 650 spec 已盘点核心面，wire 契约对照完成**。本轮修 7 处差距（assistant
content 空串、HTTP 状态映射词汇、NO_ADAPTER、流错误统一 finish 呈现、settingsNs 热替换、
keyless 装配、MAX_STEPS 可配置）；剩余差距按 P0/P1/P2 分级登记。存量验证全绿。

---

## 1. 已对齐（盘点确认，含本轮修复）

| # | DSH 契约（spec 依据） | 本轮状态 |
|---|---|---|
| 1 | chunk 序列 `block-start→delta*→block-end→usage→finish`，usage 先于 finish、finish 恰一个在末 | 已对齐（M3 收尾） |
| 2 | finish reason 词汇（stop/tool-calls/max-tokens/error/aborted） | 已对齐 |
| 3 | index 语义（reasoning/text/tool-call 独立 index、并行工具按 wire index） | 已对齐 |
| 4 | reasoning_content 非空即解析（无条件协议行为） | 已对齐（d08f88a） |
| 5 | usage 缓存剔除 disjoint（prompt_tokens − cached） | 已对齐 |
| 6 | `[DONE]` 缺失 → STREAM_CLOSED；畸形 JSON → MALFORMED_RESPONSE | **本轮修**：原 Err chunk 会覆盖真码，统一改 finish 呈现 |
| 7 | 错误码 HTTP 映射 401/403→AUTH、429→RATE_LIMIT、400→INVALID_REQUEST、500/503→SERVER、其余→HTTP_\<status\> | **本轮修**（openai.rs `map_http_code`，原来恒 error 无码） |
| 8 | 未注册 provider → NO_ADAPTER（终态 error finish，`no adapter registered`） | **本轮修**（multi.rs，原 NO_PROVIDER） |
| 9 | assistant tool-call 轮 content 用空串而非 null（API 400 拒 null） | **本轮修**（translate_messages，原来 `Value::Null`） |
| 10 | keyless 装配合法（MISSING_CREDENTIAL 在请求时而非装配时）+ 错误消息引导语 | **本轮修**（provider_config keyless 不跳过 + openai keyless finish） |
| 11 | 动态配置每请求解析（settings 写 baseURL + credentials 写 key → 下请求生效） | **本轮修**（settingsNs 热替换，`scripts/hot-replace-verify.mjs` 15/15） |
| 12 | settings ns 每插件一个、describe 列出（llm.\<id\>） | **本轮修**（settings_describe 动态 ns） |
| 13 | credentials ref 只 POSIX 标识符、空值拒、值永不出域 | 已对齐（M3） |
| 14 | settings 未知 ns → settings-rejected{ns}；update/replace/mutate 形状 | 已对齐（M3/M2.5） |
| 15 | respond 收据 {accepted:true\|false, reason:not-pending\|bad-response} | 已对齐（M3 收尾） |
| 16 | approval/question 帧、mux 重开重放同 rpcId | 已对齐（M3 收尾） |
| 17 | turn/end reason 词汇（completed/max-tokens/interrupted/error{message,code}） | 已对齐 |
| 18 | RPC 信封四象限 + `{ok:true}` void 合法 | 已对齐 |
| 19 | 崩溃恢复（interrupted 修补、kill-9 恢复） | 已对齐（M1/M3） |
| 20 | MAX_STEPS 数值可配置（LoopRuntime 字段 + CLI） | **本轮修**（`--max-steps`） |

---

## 2. 剩余差距（登记，按优先级）

### P0（核心语义，下轮优先）

| DSH 契约 | 差距 | 影响 |
|---|---|---|
| 错误归一化 `normalizeLlmFailure`：adapter 任意拒绝（非 Error/恶意 coercion/访问器字段/畸形 failure 快照）→ `{message, code:'UNKNOWN'}`；结构化 LlmError `{message,code,status,providerRetryAfterMs,requestId}` 原样入终态 | 我们的 LlmError 只有 message（+finish code）；无 status/requestId 结构化事实、无 UNKNOWN 归一化路径 | 真 provider 出错时 wire 缺结构化字段（Retry-After/x-request-id） |
| HTTP 上下文超限分类：400+`context_length_exceeded` 等 → CONTEXT_WINDOW_EXCEEDED；429+`insufficient_quota` → QUOTA_EXCEEDED；413 状态优先 | 未分类（`map_http_code` 只按状态） | 错误语义粗 |
| 请求头：`x-deepseek-harness-session-id`、`x-deepseek-harness-user-id`、user-agent 身份 | 未发 | 平台归因/日志审计缺 |
| abort 语义：请求 signal abort → 终态 `finish{kind:'aborted', code:'ABORTED'}` 且仅此一个 chunk | 无（kernel-loop 无 cancel 端口，M1 已注） | 流式中止协议未对齐 |

### P1（wire 形状补全）

| DSH 契约 | 差距 |
|---|---|
| `stream_options:{include_usage:true}` 恒随请求 | 我们只发 stream:true |
| reasoning_effort/thinking 配套（disabled 时绝不出 effort；purpose:'session-title'/'compaction' 强制 off） | 无（reasoning 能力已配置声明，effort 词汇未上 wire） |
| tool-result 空内容哨兵 `'(no output)'`、混合 text+tool-result 拆多条 wire 消息、image block → UNSUPPORTED_CONTENT | 未对齐（当前全拼文本） |
| translate `reasoning_content` 透传回 tool-call 轮（serialize passback rule） | 未做（推理块不进请求；passback 规则在重试/续跑场景才显形） |
| settings 冲突 revision：`settings-conflict{ns, expected, actual}` | 无（web-server settings 无 revision） |
| session.search 上限细节：snippet ≤240 不劈 astral、单页 ≤20 项 | 部分（≤240 有，astral 边界待核） |
| host.describe `canOpenPath` 平台事实（win32 恒 true） | 查过（静态 true，合理但未按平台区分） |
| workspace.create/rename/insertBefore 错误码细分（workspace-invalid-path/workspace-name-conflict/workspace-move-invalid） | 未细分（当前笼统） |
| goal.edit 需至少一个替换字段（bad-request） | 待核（m3-r3 40/40 覆盖主链） |
| subagent 空态错误码全表 | 已对齐（M3 收尾 4 法） |

### P2（随插件/存储工程）

| DSH 契约 | 差距 | 归属 |
|---|---|---|
| BlockAssembler replayState 剪枝（max-tokens 丢 tool-call 块同步剪） | 无 replayState 概念 | 内核（低优先） |
| message 冻结/identity（createUserMessage 深冻结） | 无 | 内核（低优先） |
| retry-policy schema（mode/maxRetries/retryableCodes） | 无重试策略 | 插件 |
| settings/credentials 文件层（0600、注释保留、热发布、writer 锁） | web-server 内存态（持久化后置） | 插件/存储 |
| session-query 全套（SESSION_QUERY_* 错误码、cursor、tracing、SQLite 索引） | 未实现（M3.5 后） | 插件（session-query） |
| workspace 持久域（bootstrap、pendingMutation 恢复、order 持久） | web-server 内存态 | 插件/存储 |
| title 服务（session/title 事件、provider、pin 语义） | 无 | 插件 |
| session/jobs 帧、queue 语义（updateQueue 目前 wire 空态） | 无 | 插件（jobs） |
| telemetry（ledger/OTel） | 无 | 插件 |
| gateway Typert 错误码全套 | 我们走简易 dispatch（行为等价、码集不同） | 内核（低优先） |

---

## 3. 下轮指针（按用户定调：核心收尾 → 官方测试集）

1. **P0 三项**：错误归一化（LlmError 结构化事实 + UNKNOWN 回退）、上下文超限/配额分类、
   abort 语义（给 kernel-loop 加 cancel 端口）。这些是 wire 行为，属内核不属插件。
2. **P1 补形状**：stream_options、reasoning_effort/thinking 上 wire、tool-result 哨兵、
   settings-conflict revision、session.search astral。
3. **官方测试集执行化**：可把 DSH 官方 `translate.spec.ts`/`sse.spec.ts`/`serialize.spec.ts`
   的断言逐步镜像成 Rust 单测（现 openai.rs 已镜像 translate 场景）；llm-deepseek 的
   mock-server 模式可复刻成 Rust 集成测试（本地 mock HTTP 端点已证可行——hot-replace-verify）。

## 4. 纪律

- 源码快照在 `.tmp/dsh-src`（shallow clone，勿提交；需要时重新拉取或归档到 docs/archive）。
- 已勾销项修回归：改 openai.rs/multi.rs 后跑 `cargo test -p kernel-llm` + hot-replace-verify。
- 新流协议铁律：**错误一律以 finish 呈现**（Err chunk 会被 loop torn 分支覆盖真码并双回合收尾）。
