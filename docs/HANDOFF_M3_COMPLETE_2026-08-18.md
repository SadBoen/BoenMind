# HANDOFF：M3 完成——RPC 52/52 + LLM 挂载点对齐 + 热升级过渡态（2026-08-18）

> 状态：**M3 已干到完成**。respond pending 表、projection 槽、goal.*、attachment/updateQueue/
> openDocument/openPath、session.export、subagent.* wire 空态、agentPreset 5 法、LLM 层挂载点
> 对齐、热升级过渡态全部落地并验证。上一指针 `docs/HANDOFF_M3_2026-08-18.md` 与
> `docs/HANDOFF_M3_FINISH_2026-08-18.md` 的目标项已勾销。

---

## 0. 一句话交接

**M3 完成态**：RPC 面 28 → **52/52**；respond 从恒 not-pending 变真 pending 表（approval 先/
question 后 + mux 重开重放）；session.export 从恒 404 变真 ZIP；LLM 层 StreamChunk 词汇、
resolveModel 元数据、usage 缓存剔除、原始 chunk 入日志全部对齐 DSH 源码；热升级过渡态（kill-9
恢复 + 同端口重启 + 续跑）验收 PASS。**遗留 → M3.5**：goal 自动续跑（goal 插件）、team 插件
真实子代理执行、settingsNs 热替换（registerAdapter 写面）。

---

## 1. 完成项（本轮）

### LLM 层挂载点对齐（对照 DSH 核心源码，逐条有源码依据）
| 挂载点 | DSH 源码 | 本轮落地 |
|---|---|---|
| StreamChunk 词汇 | `packages/llm/llm/src/types.ts`（block-start/text-delta/reasoning-delta/tool-call-delta/block-end/usage/finish） | `kernel-contracts::StreamChunk` 重写 + `to_wire()`；index 关联交错增量、block-end 携带组装块、usage 先于 finish |
| SSE 翻译 | `llm-deepseek/src/translate.ts`（[DONE] 收尾、usage 缓存剔除、EMPTY_RESPONSE、畸形→MALFORMED_RESPONSE） | `openai.rs stream_inner` 重写为同状态机；官方 `translate.spec.ts` 场景已镜像到 Rust 单测（map_finish_reason/map_usage/空首增量不开块） |
| resolveModel 元数据 | `llm/src/index.ts:219` + `adapter.ts:175` | `LlmPort::resolve_model()` + `LlmResolvedModelInfo`（contextWindow/maxTokens/reasoning）；`llm.models` 组形状对齐 modelProviderGroupSchema |
| 原始 chunk 入日志 | `core/agent-loop/src/agent.ts:349` | `AssistantChunk { chunk }` 存 raw chunk（含 finish），replay 保真 |
| usage 随 assistant/message | `agent.ts:381-389` | `AssistantMessage { content, usage }`，wire `data.usage` 只随 assistant/message |
| turn/end reason | `core/session/src/types.ts`（TurnEndReasonMap） | `TurnEvent::Ended { reason }`：completed/max-tokens/error（MAX_STEPS/LLM_STREAM/EMPTY_RESPONSE） |
| ToolCall.arguments | `core/session`（模型原始 JSON 文本未解析） | `ToolCall.arguments: String`（wire 透传保真；执行侧 JSON.parse 兜底 Null） |

### RPC 52/52（新实现 24 法）
- respond pending 表：approval 先/question 后、整批校验（数量/id/selected 唯一/multiSelect/
  option label/custom trim）、`approval/resolved`/`question/resolved` 广播、question 取消
  （ok:false+cancelled）accepted。**登记点留扩展**：审批/提问工具调 `PendingRegistry::register_*` 即接入。
- 测试钩子：`BM_TEST_HOOKS=1` 时 `_test.registerApproval|registerQuestion`（生产缺省关闭）。
- session/projection 槽：`Mutex<HashMap<key,(seq,value)>>` 单调 seq + `session/projection` 帧 +
  history tail projections（asOfSeq = wire 长度-1）。
- goal.* 6 法：web-server 内存态最小桥（wire 契约层），CAS ref（stale → goal-conflict），
  'goal' 投影 + clear 墓碑 null。**自动续跑语义 = goal 插件（M3.5）**。
- session.attachment（日志引用表 → attachment-error）、session.updateQueue（queue-item-not-found）、
  settings.openDocument（opened:true）、host.openPath（OS 打开，唯一真副作用）。
- subagent.* 4 法：无 team 插件时 list → parentAvailable:false / 其余 → subagent-parent-unavailable
  （不装死）。
- agentPreset 剩余 5 法：select（blank 校验 → agent-preset-locked）、read/copy/openDocument/remove
  （无 authoring 预设 → agent-preset-not-found）。
- session.export：真 ZIP（zip crate 2.x，`session.jsonl` JSONL），400/404 分支 + content-disposition。
- **启动恢复**：main.rs 启动时 restore 全部持久化会话（kill-9 恢复语义落到真实重启路径）。

### 热升级过渡态（门禁 3 半验收）
`scripts/hot-upgrade-transition-verify.mjs`：起 A(3079) → 真回合 → kill -9 → 起 B(同端口) →
host.describe/session.list/history 完整 → 可继续 prompt → PASS。

## 2. 验证矩阵（全过）
| 项 | 结果 |
|---|---|
| m3-r3-verify（respond 三分支/重放、goal 全链、projection、attachment/updateQueue/openDocument/openPath、subagent 空态、agentPreset 5 法、export ZIP） | **40/40 PASS** |
| gate25（**全新 db**） | PASS |
| conformance diff-traces | **17/17 PASS** |
| m3-r2（search/fork/host 流） | 15/15 PASS |
| hot-upgrade-transition | PASS |
| cargo test --workspace | 62 全过 |
| clippy --workspace --all-targets | 零警告 |

## 3. 遗留 → M3.5（登记）
1. **goal 自动续跑** = goal 插件（web-server 只做 wire + 投影；不实现"武装目标自动续跑"策略）。
2. **team 插件** = 真实子代理执行（Rust 独立进程 + supervisor + IPC，与热升级蓝绿同批工程）。
3. **settingsNs 热替换**（registerAdapter 写面）：settings 命名空间写 provider 配置 + 热替换。
4. kernel-loop `MAX_STEPS` 数值可配置：已加 `LoopRuntime::max_steps` 字段（装配默认 32），
   web-server 未暴露 CLI 开关（低优先）。
5. `session.attachment` 数据存储（当前无附件事件源；日志引用表已就位）。
6. SSE 备选流（面 9）host 流仍为简版（只发注释行）——浏览器优先 WS，低优先。

## 4. 纪律与坑（沿用）
- 开工/收尾杀残留：`taskkill //F //IM web-server.exe`；临时 db 清理 `rm -f *.db*`。
- gate25 用全新 db；WS 验证用固定收集窗口（400ms 静默）防 close 竞态；锁纪律（workspace 写先
  drop 再 broadcast）；serde_json 无 preserve_order（键字母序）。
- 测试钩子须 `BM_TEST_HOOKS=1` 启动（m3-r3-verify 依赖）；m3-real-provider-verify 走真 API 计费。
- **新流协议注意**：每回合 4 个 assistant/chunk（block-start/text-delta/block-end/finish）入日志
  ——history/seq 断言按此写（gate25 已改结构性断言）。

## 5. 台账
`docs/CONTRACT_LEDGER_DSH.md` 实施进度段已更新至 M3 收尾（52/52）。
