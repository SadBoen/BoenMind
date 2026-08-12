# BoenMind 上下文压缩补强 —— 任务计划

> 创建：2026-08-12。目标：吸收 Hermes 与 context-mode 的优点，补强 BoenMind 所用的
> pi_agent_rust（vendored v0.2.0）上下文压缩。原则：**能不动核心代码就不动**（pi-vendor-patch-policy）。

## 0. 需求（用户原话拆解）

1. 探测模型支持的上下文长度；
2. 默认 50% 压缩水线，尾部保护学习 Hermes；
3. 前两条**按模型可配置**；
4. 吸收 Hermes 和 context-mode 的优点，做成 **pi_agent_rust 插件格式**，能不动核心代码就不动；
5. 执行者如有更好方案可融入。

## 1. 背景（已查证，勿重复调查）

### 1.1 pi 引擎压缩现状（vendored，`backend/vendor/pi_agent_rust`）

- 摘要压缩已内建：触发条件 `should_compact`（`src/compaction.rs:817`）= 占用 ≥ 窗口 − reserve；
  切点 `find_cut_point`（`src/compaction.rs:998`）从尾部凑 `keep_recent_tokens` 预算、二分选最大
  合法切点、不拆工具组/回合；摘要模板 `SUMMARIZATION_PROMPT`（`src/compaction.rs:1068`）；
  增量更新 `UPDATE_SUMMARIZATION_PROMPT`（并入 previous_summary）。
- SDK 默认（**固定绝对值，非比例**）：`reserve_tokens = 16384`、`keep_recent_tokens = 20000`
  （`src/config.rs:596,603`）；`context_window_tokens` 从模型注册表读，声明为 0 则 fallback 128K
  （`src/sdk.rs:1777-1787`）。
- **`SessionOptions` 没有任何 compaction 覆盖字段**（`src/sdk.rs:286`），只有
  `auto_compaction_enabled: bool`。→ 按模型配置必须给 SDK 打最小补丁（见 3.2）。
- 压缩跑在异步 worker（`src/compaction_worker.rs`），事件 `AutoCompactionStart/End`
  （`src/agent.rs:1015-1018`）。

### 1.2 扩展（插件）机制现状

- 格式：单文件 `.ts` 或含 `extension.json` 的目录；WIT 协议 `init / handle-tool / handle-slash /
  handle-event / shutdown`（`docs/wit/extension.wit`）；扩展工具经 `ExtensionToolWrapper` 注入
  ToolRegistry（`src/extension_tools.rs:99`）。
- 事件面（`src/extensions.rs:9648-9700` `ExtensionEventName`）：
  - ✅ `ToolExecutionStart/Update/End`（带 tool_name/args/partial_result）→ 可做事件落库；
  - ✅ `ToolCall`（pre-exec 可阻塞）→ 可做路由强制；
  - ✅ `ToolResult`（post-exec **可修改**，payload `{call_id, output, is_error}`，`src/extensions.rs:478`）
    → 可做零成本修剪；
  - ⚠️ **`AutoCompactionStart/End` 明确不派发给扩展**（`src/extensions.rs:19646-19653` 注释
    "Session-level compaction/retry events are not dispatched to extensions"）；
  - ❓ `SessionBeforeCompact` / `SessionCompact` 存在（`src/extensions.rs:9675-9676`）——触发来源
    未确认（可能是手动压缩/分支切换），Phase 0 验证。
- BoenMind 加载方式：`SessionOptions.extension_paths` ← `~/.boenmind/extensions/`
  （`backend/crates/bm-core/src/plugins.rs`）；**斜杠命令（handle-slash）走 pi CLI interactive 层，
  BoenMind 聊天应用不可用** → 插件只能靠工具 + 事件，不依赖斜杠命令。

### 1.3 Hermes 做法（参考）

- 水线 50%（`threshold`），尾部 = `threshold × target_ratio(0.2)` ≈ 窗口 10% 的 token 预算 +
  `protect_last_n = 20` 条消息兜底 + 保护头前 3 条；
- 压缩前**零成本修剪**：>200 字符的旧 tool_result 替换为占位符；
- 摘要模板与 pi 几乎同构（Goal/Constraints/Progress/Key Decisions/Next Steps/Critical Context）。

### 1.4 context-mode 做法（参考）

- 沙箱执行工具（ctx_execute：数据留在沙箱，只回结论，315KB→5.4KB）；
- 事件落库：文件编辑/git/任务/决策 → SQLite+FTS5，压缩后按需 BM25 检索恢复；
- 路由强制（hooks 拦截大输出工具）。

> 实现立场（用户明确，2026-08-12）：**全部为自研全新代码，仅借鉴上述思想/行为，不复制任何
> 第三方代码**——思路本身不受版权约束，context-mode 的 ELv2 许可证不构成障碍。架构上仍需
> 重做而非移植的原因：context-mode 是 MCP server + hooks 架构，BoenMind 没有 MCP client
> 也没有 hook 对接，而 pi 扩展机制原生提供等价事件（见 1.2）。

## 2. 架构分层

```
┌─ 层 A：会话配置（需求 1/2/3）─────────────────────────────┐
│  bm-core：模型上下文探测 + 按模型配置（config.toml）       │
│  + vendored pi 唯一最小补丁：SessionOptions 加 compaction 覆盖字段 │
└────────────────────────────────────────────────────────────┘
┌─ 层 B：扩展插件（需求 4，0 碰 pi core）───────────────────┐
│  ~/.boenmind/extensions/ 下的 .ts 扩展：                 │
│  ① ctx_execute 沙箱执行工具（Think in Code）             │
│  ② ToolResult 零成本修剪（大输出 → 占位符 + 落库）       │
│  ③ ToolExecutionEnd 事件持续落库 + 检索工具（简易 BM25） │
└────────────────────────────────────────────────────────────┘
```

关键设计决策（融入的第 5 点建议）：

- **持续索引替代"压缩前索引"**：AutoCompaction 事件不派发给扩展（已验证），且持续索引更简单
  更稳——每次 `ToolExecutionEnd` 就落盘，压缩发生时数据已在库里。
- **修剪必须配检索**：占位符里给出检索指引（如 `[工具输出已修剪，检索 key: ...]`），否则模型
  丢失信息——这是 context-mode 的教训。
- **秘密扫描**：bash 输出可能含 API key，落库前按常见 key 格式正则过滤（学 pi-hermes-memory）。
- **索引按项目/会话分桶**：新会话干净起点（学 context-mode 的 fresh session 语义），避免跨项目污染。
- **探测区分"声明窗口"与"实际窗口"**：models.json 的 context_window 可能不准 → 提供配置覆盖。
- **vendor 补丁最小化**：只加字段+三行逻辑，打 `// BoenMind 补丁` 标记，按 upstream-issues-policy
  提上游 issue 建议 SDK 支持 per-session compaction settings。

## 3. 任务清单

### Phase 0 —— 遗留验证（半天内，全部只读）

- [x] `SessionBeforeCompact`/`SessionCompact` 的派发来源（grep 谁 emit 这两个事件；若来自手动
      compact/SDK 方法，记录其触发路径，决定二期是否利用）。
- [x] `ToolResultEventResult` 的响应格式：扩展如何返回修改后的结果（读
      `src/extension_events.rs` 的 `ToolResultEventResult` 定义与调用点）。
- [x] 修剪对会话存储的影响：ToolResult 修改是只影响"喂给模型的上下文"还是也会写入 session
      store 的持久化消息（若会写存储，修剪后回放/历史会丢原文——决定占位符格式要自描述）。
- [x] 扩展 FS connector 能力：扩展能否写 `~/.boenmind/` 下文件（读 `src/extensions/fs_connector.rs`
      与扩展政策 `resolve_extension_policy_with_metadata`，默认 Prompt 模式允许哪些）。
- [x] QuickJS 运行时对 JS 大小/执行时长/内存的限制（`src/extensions_js.rs`），决定沙箱脚本上限。
- [x] `branch_summary_reserve_tokens`（`src/config.rs:614` 默认继承 compaction reserve）在
      BoenMind 是否被使用——若被用，50% 水线会波及它，需单独设置。
- [x] BoenMind 内置插件文件的实际存放路径（`plugins.rs` 的 BUILTIN_PLUGINS 内容从哪来），
      决定新插件随仓库分发的位置。

### Phase 1 —— 层 A：探测 + 按模型配置（需求 1/2/3）

- [x] **模型上下文探测**：读 pi 模型注册表（bm-server 启动已 sync 的 models.json）每个模型的
      `context_window`，建 `模型 → 窗口` 映射（bm-core 新模块或 config 段）。
- [x] **按模型配置**：config.toml 新增 `[compaction_overrides.<model>]`（或等价的 bm-core 配置）：
      `context_window`（覆盖探测值）、`watermark`（默认 0.50）、`keep_recent_ratio`（默认 0.10）、
      `keep_recent_floor`（默认 4000 token 下限）。
- [x] **vendor 最小补丁**：`src/sdk.rs` `SessionOptions` 增加
      `compaction_settings: Option<ResolvedCompactionSettings>`（`#[serde(default)]` 等保持
      兼容），`sdk.rs:1777` 构造时优先使用。**只动 SDK 入口，`compaction.rs` 引擎逻辑一行
      不改**。打 `// BoenMind 补丁` 标记 + 提上游 issue（建议上游支持 per-session compaction
      覆盖）。说明：`context_window_tokens` 已按模型生效（models.json），本补丁只补
      水线/尾部的 per-session 注入；未配置的模型走现有全局行为（向后兼容）。
- [x] **bm-core 接线**：`create_session_handle`（`bm-core/src/agent.rs:64`）按 model 查配置，
      组装 `ResolvedCompactionSettings`（watermark×窗口 → reserve；keep_recent = max(ratio×窗口,
      floor)）传入 SessionOptions。
- [x] 单元测试：水位计算、fallback（无配置 → 现有行为不变）、窗口=0 的模型。

### Phase 2 —— 层 B：插件 MVP（需求 4，0 碰 pi core）

- [x] 扩展骨架：`extension.json` + `handle-tool` 注册以下工具（参考 vendored 官方示例的写法，
      纯类型级依赖，QuickJS 可直接加载）。
- [x] **① `ctx_execute` 沙箱工具**：接收 `(language, code)`，在 QuickJS 内执行 JS（fs 走
      FS connector），只返回 `console.log` 结果与退出状态；限制脚本大小/时长（Phase 0 结论）。
- [x] **② 零成本修剪**：`handle-event` 的 `ToolResult` 事件——`output` 超过阈值（默认 200
      字符，可配置）时替换为占位符（含检索 key），原文写入索引库。修剪阈值/占位符格式可配置。
- [x] **③ 事件落库 + 检索**：`ToolExecutionEnd` 持续落盘（JSONL：时间/项目/会话/tool/文件名/
      摘要/原文引用 + 秘密扫描过滤）；`search` 工具做简易关键词检索（倒排或扫描 + 排序，规模
      小，不需要真 FTS5；如 QuickJS 有 sqlite 绑定则优先）。
- [x] 索引按项目分桶 + 会话清理语义（不跨项目检索）。

### Phase 3 —— 整合与测试

- [x] `cargo test -p bm-core -p bm-server` 全绿（含 Phase 1 新测试）。
- [x] 端到端：长会话触发压缩（日志/事件确认触发水位 = 50% 窗口）；修剪后模型仍能通过
      `search` 找回被修剪内容；`ctx_execute` 跑通一个真实用例。
- [x] GUI 冒烟（浏览器实测）：工具调用可视化块正常、插件工具正常显示、无回归。
- [x] 边界：小窗口模型（32K）、大窗口模型（1M）、models.json 无该模型、修剪与回放并存。

### Phase 4 —— 可选增强（不做也不影响验收）

- [ ] Hermes 式消息数兜底（`protect_last_n = 20`）：需改 `find_cut_point`（vendor 第二处小补丁，
      打标记 + 提 issue）。
- [ ] 路由强制：`ToolCall` 事件阻塞大输出内置工具（read/grep）并引导用 `ctx_execute`
      （体验细节需验证，可做成配置开关）。
- [ ] 压缩水位观测：把触发时水位暴露到前端（复用 ToolExecutionEnd 事件或单独事件通道）。
- [x] `SessionBeforeCompact` 若在 Phase 0 确认可用 → 加"压缩前把未落库事件冲刷入库"。

## 4. 验收标准

1. 按模型生效：某模型配置 50% 水线后，压缩触发点 ≈ 窗口的 50%（可观测）；
2. 尾部保护 ≈ 窗口 10% 原文保留（Hermes 同款语义；消息数兜底为可选加分项）；
3. 大工具输出在进模型前被修剪为占位符，且可通过检索工具找回原文；
4. vendored pi 代码仅有**一处打标记的最小补丁**（SessionOptions），其余 0 改动；
5. 新插件不依赖斜杠命令，纯工具 + 事件，BoenMind 聊天应用内可用；
6. 既有功能无回归（i18n/skill/提供商预设/工具调用可视化）。

## 5. 风险

| 风险 | 缓解 |
|---|---|
| models.json 的 context_window 不准 → 水位失真 | 配置覆盖探测值；文档说明以 provider 文档为准 |
| 50% 水线使压缩更频繁 → 摘要 LLM 调用成本上升 | 增量摘要已存在；水线可配置回退；reserve 语义兼作输出预留 |
| ToolResult 修改若写入会话存储 → 历史原文丢失 | Phase 0 验证；占位符自描述 + 原文在索引库可查 |
| QuickJS 限制沙箱脚本能力 | Phase 0 摸清上限；ctx_execute 设计为"小脚本出结论"而非重执行 |
| 扩展 FS 写权限受限 | Phase 0 验证；必要时调整扩展政策或改用已有 hostcall |
| vendor 补丁与上游升级冲突 | 单点小补丁 + 标记 + 上游 issue（按 pi-vendor-patch-policy） |

## 6. 交付物

- [x] 本计划文件的执行记录（勾选完成项）
- [x] bm-core：模型窗口探测 + 按模型压缩配置 + 单测
- [x] vendored pi：SessionOptions 补丁（打标记）+ 上游 issue（issue 链接见下方执行记录）
- [x] 插件：ctx_execute + 修剪 + 落库检索（.ts 扩展，随仓库分发）
- [x] 测试与 GUI 验证证据（截图见 gui-test-screenshots/）

## 7. 执行记录（2026-08-12）

### Phase 0 验证结论（修正两处计划假设）

| 调查项 | 结论 |
|---|---|
| `SessionBeforeCompact`/`SessionCompact` | ✅ 自动压缩时派发（后台 worker `agent.rs:8731` + 同步 `:8950`），可 cancel / 可提供自定义 summary（官方示例 custom-compaction.ts）；`SessionCompact` 压缩条目创建后派发。`AutoCompactionStart/End`（AgentEvent）确认不派发给扩展。 |
| `ToolResultEventResult` | `{content: Option<Vec<ContentBlock>>, details: Option<Value>}`；None 保留原样；payload `{type, toolName, toolCallId, input, content, details, isError}`；last-result 语义。 |
| 修剪写入会话存储 | ✅ **会写**：`apply_tool_result_hook`（agent.rs:3057）修改 output → `ToolResultMessage` → session store。→ 占位符必须自描述（含 key + 摘要），原文入索引。 |
| 扩展 FS | node:fs 走 VFS：**读有 host 回退（真实文件），写是内存虚拟层（不落盘）**。写权限根 = 宿主进程 cwd（非会话 working_dir）。**真实落盘必须走 `pi.tool("write")` hostcall**（capability write ✓）。 |
| QuickJS 限制 | 堆上限 256MB（policy max_memory_mb）；事件超时 5s（actionable）/info 事件更短；工具超时 60s；interrupt_budget 默认 None。 |
| `branch_summary_reserve_tokens` | 仅 interactive/tree_ui.rs（CLI 树 UI）使用，BoenMind 不受影响。 |
| 内置插件路径 | `ensure_builtin_plugins` 从 vendored examples 复制；本任务新增 `repo_plugin_dir`（backend/plugins/<id>/，目录型插件随仓库分发）。 |
| ⚠️ **计划修正 1**：`tool_execution_start/update/end` 扩展事件只在 CLI/rpc 路径派发（EventCoalescer 接线于 main.rs/rpc.rs/interactive），**SDK 路径（BoenMind）收不到**。→ 落库放在 `tool_result` handler（原文在手），不依赖 ToolExecutionEnd。 |
| ⚠️ **计划修正 2**：扩展 FS 的 workspace root = **宿主进程 cwd**。bm-server 从 backend/ 启动时，插件写 `~/BoenMind/.boenmind/` 会被 deny（fail-open 吞错）。插件内用 `pi.tool("write")` 绕开（宿主工具按会话 cwd 解析）。 |

### 实施要点

- **vendor 补丁**：`src/sdk.rs` 三处（SessionOptions 字段 + Default + `unwrap_or_else` 回退），打 `// BoenMind 补丁` 标记；`compaction.rs` 引擎零改动。上游 issue：
  <https://github.com/Dicklesworthstone/pi_agent_rust/issues/160>
- **bm-core**：`compaction.rs` 新模块（`[compaction]` 段 + `overrides.<pi>/<model>`，watermark 默认 0.50 / keep_recent_ratio 0.10 / floor 4000；探测 = 配置覆盖 → models.json `contextWindow`（sync 时写入）→ 128K 兜底）；`create_session_handle` + chat.rs 接线。
- **插件** `backend/plugins/ctx-compactor/`：ctx_execute（间接 eval 沙箱执行 + console 捕获）、tool_result 修剪（默认 200 字符阈值，可配置 `.boenmind/ctx-compactor.json`）、ctx_search（JSONL 索引词频检索，按项目分桶于 cwd 下 `.boenmind/ctx-index/`）、session_before_compact 观测日志。秘密扫描（API key 正则 → [REDACTED]）落库前过滤。
- **ctx_execute 实现陷阱**：`new Function("__code", body)` 中 `__code` 是变量绑定而非文本替换 —— 代码字符串会被当表达式求值、**从未执行**（node 复现确认）。改为 `(0, eval)("(async () => { " + code + " })()")` 间接 eval 执行。

### 验证证据（日志/行为）

- 单测：bm-core 24 个全绿（新增 compaction 8 项 + sync contextWindow 写入 1 项）。
- 注入生效：`bm.debug.maybe_compact` 显示 `context_window_tokens: 4000, reserve_tokens: 2000`（50% 水线 = 窗口一半）。
- 压缩触发：tokens_before=17418（≥ 2000）→ `prepare_compaction` Some → `dispatch_before_compact` 派发 → 插件日志 `compaction triggered, tokensBefore=17418` → 默认压缩继续。
- 修剪+找回：bash `seq 500 600`（404 字符 > 200）→ 占位符（含 key）→ 模型 6 次 ctx_search 找回摘要（500–549）→ 正确回答；索引 `entries.jsonl` 真实落盘。
- ctx_execute：`console.log(Math.pow(7, 8))` → `5764801`；GUI 实测 3⁵ = 243。
- GUI 冒烟（浏览器 + MiniMax 视觉确认）：插件页显示 ctx-compactor（清单目录/内置示例/已启用）；工具调用块绿色显示、可折叠；无布局回归。截图：`gui-test-screenshots/ctx-compactor-plugin-page.png`、`ctx-compactor-toolblock-243.png`。
- 边界：小窗口（4000）触发 ✓；models.json 无窗口模型不写 contextWindow（fallback 128K，单测）✓；回放：BoenMind db 不存工具输出，修剪只影响模型上下文，无历史丢失风险 ✓。
