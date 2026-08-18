# Architecture Audit — BoenMind Rust 微内核（kernel/，已实现架构适配度）

**Checklist: 44/44 complete**
**Incomplete: None**

审计日期：2026-08-18。审计人：ln-24-architecture-auditor SKILL 独立审查代理（只读）。
范围：kernel/ 工作区 8 个核心 crate + `headless`/`web-server` 消费方接线；排除 `target/`、`Cargo.lock`。
方法：`cargo metadata --no-deps` 解析依赖拓扑；核心源码逐文件读；`cargo test --workspace`（91/91 过，22 个套件）、`cargo clippy --workspace --all-targets -- -D warnings`（零警告）、`bash scripts/verify-gate1.sh`（ALL PASS）作为可复现分析证据（写入 target/ 的诊断缓存已披露）；官方 DSH 源码 `deepseek-ai/deepseek-harness`（master）经 GitHub API 逐行核实作为权威契约参照。

---

**Verdict: FAIL**

判定依据（SKILL §5）：存在两条已证实的 P1 正确性/结构缺陷——(1) 回合级事件瀑布缺失 `Turn Started`（turn 编号永不递增、重启后会话一律被分类为 blank）；(2) 错误/取消回合以未配对的 `Step Started` 收尾，interrupted-turn 修复会把"已闭合的错误回合"连同其诊断证据（M4 新增的 `requestId`）整段截断；若取消回合之后又跑了新回合，重启修复会**删除取消点之后的所有后续回合历史**（唯一事实源中的数据丢失）。其余方面（分层、契约台账、错误归一化、abort 语义、settingsNs 热替换、信任栅栏）实现健康。

---

## 1. Actual architecture

### 1.1 模块、边界、入口点、外部系统

**10 crate 单 workspace**（`kernel/Cargo.toml` members），`cargo metadata --no-deps` 实测内部依赖边：

```
kernel-contracts   -> (leaf)
kernel-session     -> contracts
kernel-llm         -> contracts (+reqwest/rustls)
kernel-tools       -> contracts
kernel-storage     -> contracts (+rusqlite bundled)
kernel-supervisor  -> contracts
kernel-loop        -> contracts, session, tools
kernel-assembly    -> contracts, session, llm, tools, storage, loop, supervisor
headless  (bin)    -> assembly, contracts, llm
web-server (bin+lib)-> assembly, contracts, llm, loop, session   (session 为未使用依赖)
```

- **layer 5** `kernel-contracts`：端口 trait（LlmPort/SessionPersistPort/FsPort/ShellPort/PluginRuntimePort）、事件词汇（SessionEvent/TurnEvent/StepPhase/TurnEndReason）、DTO（StreamChunk/FinishReason/TokenUsage/GenerateOptions/AbortSignal）、统一错误（PortError/LlmError/FailureInfo/ToolError）。
- **layer 4**：`kernel-session`（append-only 日志 + 投影 + SessionStore + EventBus 观察者）；`kernel-llm`（ScriptLlm mock、OpenAICompatLlm 流式适配器、MultiProviderLlm 路由聚合）；`kernel-tools`（ToolRegistry + ToolGate fail-closed）；`kernel-storage`（SqlitePersist：WAL + synchronous=FULL + 单事务 append，`rewrite_events` 落盘修复）；`kernel-supervisor`（子进程 spawn/kill/restart 宿主）。
- **layer 3**：`kernel-loop`（ReactLoopAgent：turn/step 瀑布、BlockAssembler、logged-means-persisted、per-session model override + abort 槽）。
- **layer 2**：`kernel-assembly`（组合根 `Runtime`：create_session/restore_session/repair_interrupted_turn）。
- **layer 1 消费方**：`headless` 二进制（门禁 1 载体：roundtrip/abort/resume/verify-tail/dump）；`web-server`（axum 协议兼容层：RPC 信封、双 WS 下行、52 RPC 方法、信任双栅栏、settingsNs/credentials 写面）。

**入口点**：`headless/src/main.rs`（CLI 子命令）；`web-server/src/main.rs`（`Runtime::headless_with_max_steps` → 真 provider 装配 `--config` → `AppState::assemble` → `attach_event_bus` → 启动恢复（restore_session 全部会话）→ axum serve）。

**外部系统**：LLM provider HTTP（OpenAI 兼容 `/chat/completions`、`/models`）；SQLite 文件（`--db`）；前端静态快照（`kernel/web-server/frontend/`）；DSH 前端浏览器客户端。**进程边界**：web-server 单进程 + tokio 多线程；插件子进程宿主（kernel-supervisor）已实现但**未被装配**（见 LN-006）。

**部署单元**：web-server 二进制（唯一交付面）；headless 二进制（验收载体）。独立失败边界：web-server 崩溃 → SQLite WAL 恢复；LLM 端失败 → finish 呈现（不产 Err chunk）。

### 1.2 关键流程与运行期接线（静态追踪 + 单元测试序列钉死）

1. **会话创建**：`Runtime::create_session` → SessionStore.create（内存 Session + SessionStarted 事件）→ `SqlitePersist::create_session`（单事务 header + seq=1）。失败时内存会话残留（边界 case，未入账为 finding）。
2. **回合瀑布**（`ReactLoopAgent::run_turn`）：`next_turn()`（从日志中 Turn Started 求 max+1——**但 loop 从不 emit Turn Started，恒为 1**，见 LN-001）→ append UserMessage → 每 step：append Step Started → `Session::derive_messages()` 投影 → `gate.enabled_schemas` → `llm.stream` 逐块消费（原始 chunk 逐块入日志 `AssistantChunk`，BlockAssembler 累积）→ finish 分派 → 工具调用则 ToolCall/ToolResult 逐个入日志 → Step Ended → Turn Ended{reason}。
3. **持久化纪律**：每个事件 append 后立即单事件事务落盘（`persist()`，WAL+FULL）。每次 append = 一次 fsync（M1 明确接受的权衡，不做 finding）。
4. **abort 语义**：`AbortSignal`（AtomicBool + tokio watch + 永久 receiver 保底）→ `GenerateOptions.signal` → OpenAICompatLlm 三处穿透（预 abort 不碰传输 / send 阶段 select_biased / 流中 select 打断挂起读）→ `Finish(Cancelled)` → loop 转 `TurnEndReason::Aborted`。`session.cancel` RPC → `ReactLoopAgent.abort()`。
5. **settingsNs 热替换**：`settings.update/mutate`（api.rs）→ `adapter.set_base_url_override`；`credentials.set/unset {ID}_API_KEY` → `set_api_key_override`；每请求 `effective_base_url/effective_api_key` 解析，写后下一请求生效。
6. **kill-9 恢复**：启动时 `restore_session` 全部会话 → `load_events`（**丢弃 seq/timestamp 列**，LN-005）→ `repair_interrupted_turn`（尾部未配对 Step/Turn Started 截断）→ `rewrite_events`（事务内 DELETE+INSERT，**全部事件重盖时间戳**，LN-005）→ SessionStore.restore。
7. **WS 下行**：`EventBus.on_event` → `attach_event_bus` 按会话翻译游标（seq 从 0 连续）+ `events_tx` broadcast → `mux_loop` 包 `session/event` 帧。mux 连接时按 `running || !blank` 发 `session/subscribed` 基线（**blank 恒 true 的问题见 LN-001**）。

### 1.3 文档状态、目标状态、迁移阶段与已证实漂移

| 工件（docs/） | 分类 | 说明 |
|---|---|---|
| design/DSH_PROJECT_V2_2026-08-17.md | current（宪法/目标态） | 采纳意图；实现已超出其面清单（settings/credentials 面为 REVIEW_C 追加） |
| CONTRACT_LEDGER_DSH.md | current | 实现进度台账，逐里程碑勾销；M4 段为最新 |
| DSH_TESTSUITE_ALIGNMENT_2026-08-18.md | current | P0/P1 已落地、P2 挂账台账 |
| HANDOFF_M4_FINISH_2026-08-18.md | current | 当前迭代指针 |
| HANDOFF_M2/M25/M3/M3_COMPLETE/M3_FINISH/M4 | superseded | 里程碑交接，被后续覆盖 |
| HANDOFF_DSH_V21_2026-08-17.md | superseded | M1 原始指针 |
| review-dsh-v2/REVIEW_A/B/C | accepted（决策期基线） | 2026-08-17 对 TS dsh + bobleer 的采用决策审计；本审计以其 P1-C/D、P2-E/F 为对照基线 |
| conformance/*.mjs | current（验收工具） | gate25/m3-r3/hot-replace/conformance 双后端轨迹 diff；本次未执行（需起服务占固定端口），以 cargo test+clippy+gate1 替代并披露 |

**已证实漂移（文档 vs 可执行结构）**：
- kernel/README.md 分层图**完全未列 web-server**；边界守卫也不覆盖它（LN-003）。
- kernel-contracts/src/ports.rs:89 注释"PluginRuntimePort M3 接 supervisor 完整实现"——**不属实**（LN-006）。
- kernel-supervisor 模块注释声称"崩溃重启"——未实现（只有手动 kill/restart）（LN-006）。
- README/headless 声称"turn/step waterfall 对齐 dsh"——Turn Started 从未产生（LN-001）。
- HANDOFF_M4_FINISH §5 与 loop 注释已承认 Err chunk 会把错误码覆盖成 LLM_STREAM，但 LlmPort 契约文档仍写着"错误以 Err 形式结束流"（LN-004）。

**支配组织模型**：layer-first（契约→能力→编排→组合根）+ 端口/适配器 + 事件日志唯一事实源；web-server 为协议镜像壳。无竞争模型（无 domain-first 残留）。

**Git 状态**：仓库干净（审查开始时）；审查期间仅产生 `docs/review-dsh-rust-core-2026-08-18/`（本报告目录，调用方创建）与 `target/` 诊断缓存；未修改任何代码或架构文档。

---

## 2. Fitness summary

| Area | Status | Evidence |
|---|---|---|
| Pattern fitness and ownership | CONCERNS | 端口/适配器分层真实有效（LlmPort 3 实现、SessionPersistPort 2 实现）；事件日志唯一事实源 + 单事务原子发布兑现（kernel-storage/src/lib.rs:148-197）；logged-means-persisted 逐事件落盘兑现。扣分点：turn/step 瀑布不完整（LN-001）、回合终态路径不闭合 step（LN-002）、supervisor 能力已实现未装配（LN-006）。抽象无"为模式而模式"：FsPort/ShellPort 为零消费者零实现的预留词汇（未过 materiality 门槛，仅观察项）。 |
| Contracts and boundaries | CONCERNS | 错误归一化（LlmError/FailureInfo/to_failure）、wire 形状精确断言（serde skip 字段 + 镜像单测）、fail-loud 纪律一致。扣分点：LlmPort 契约文档与可执行约定矛盾（LN-004）；`SessionPersistPort::load_events` 丢弃 seq/timestamp（LN-005）。会话恢复校验（seq 连续、header 匹配）完备。 |
| Dependency topology | CONCERNS | 核心 9 crate 严格向下依赖（cargo metadata + crate_boundaries.rs 双证），无环、无向上边。扣分点：守卫不覆盖 web-server（LN-003）；web-server 直接依赖 kernel-loop/kernel-llm 并声明未使用的 kernel-session。 |
| Physical structure and configuration | PASS | 配置归属清晰：LLM 配置（provider_config 类型化、config>env>keyless 优先级）、settings/credentials 内存写面（shell 拥有）、`--max-steps`/`--trusted-host` CLI、启动校验 fail-loud、环境读取局限于 provider_config + BM_TEST_HOOKS 测试钩子（生产缺省关闭）。密钥不进 GenerateOptions、不进日志。组合根职责明确（assembly + shell 二次装配）。 |

---

## 3. Findings

| Priority | Problem | Evidence and justification | Required resolution |
|---|---|---|---|
| **P1** (LN-001) | **Turn Started 从未被 loop 产出**：`run_turn` 全路径只 append `Turn Ended`，从不 append `TurnEvent::Started`；`next_turn()` 从日志 Turn Started 求 max+1 恒为 1——同一进程内连续回合、kill-9 续跑后 turn 编号都停在 1；wire 上永不出现 `turn/start`；web-server 重启恢复用 `has_turn_start` 判定 blank，**所有会话重启后恒为 blank=true**（session.list 与 mux `session/subscribed` 基线均受影响）；events.rs 的 turn/start 翻译分支与 repair 的 turn 配对分支成为死代码 | kernel-loop/src/lib.rs:286-297（next_turn 依赖 Turn Started）、:312-577（run_turn 全文无 Turn Started append）；web-server/src/main.rs:259-273（`blank = !has_turn_start`）；web-server/src/events.rs:51-55（死分支）。对照权威契约：DSH agent-loop 在开回合时 `session.append('turn/start', { turn })` 且 lastTurn 由 turn/start 推导（deepseek-harness master `packages/core/agent-loop/src/agent.ts:92,255`，经 GitHub API 核实） | 在 `run_turn` 开回合处（UserMessage 之后、首个 Step Started 之前）append `Turn(TurnEvent::Started { turn })`，对齐 DSH agent.ts:255；同步修正 repair 的 turn 配对与 events.rs 翻译即自然激活。迁移风险低（纯增量事件）；等效目标形状：由 assembly/web-server 在调 run_turn 前补发亦可，但推荐 loop 内部（单一所有权）。参考：[deepseek-harness agent.ts#L255](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/src/agent.ts#L255) |
| **P1** (LN-002) | **错误/取消回合以未配对 Step Started 收尾；恢复修复会截断已闭合的错误回合并连带删除其后全部历史**。四个终态路径（流 Err chunk、Finish 缺失、Finish(Cancelled)、Finish(Error)）都只 append Turn Ended 不 append Step Ended——活进程内日志即违反"已落盘部分必须配对完整"不变式（headless verify_tail 会拒绝此类日志）。恢复时 `repair_interrupted_turn` 从尾部扫到未配对 Step Started 即 `truncate(cut)`：① 把该回合的 Turn Ended{Error/Aborted}（含 M4 刚加的 requestId 诊断投影）整段删除；② 若取消回合之后又跑过新回合（常见：用户取消→再发消息），修复在更早的未配对 Step Started 处一刀切，**取消点之后的全部后续回合历史从唯一事实源中丢失**。TurnEndReason::Interrupted 变体因修复不合成 closers 而永不出现在日志 | kernel-loop/src/lib.rs:384-402（Err chunk → Turn Ended only）、:416-431（Finish 缺失）、:438-451（Cancelled）、:452-471（Finish(Error)）——四条路径均无 Step Ended；kernel-assembly/src/lib.rs:185-225（repair 遇未配对 Step Started 即 cut=idx 并 truncate，Turn Ended 不参与配对）；headless/src/main.rs:232-261（verify_tail 对未配对 Step Started 判 torn，证明两者矛盾）。对照权威契约：DSH `commitRepair` 截断 torn 尾部后**追加合成 closers** 而非静默截断（deepseek-harness master `packages/session/session-persistence-jsonl/src/index.ts:436-442`，经 GitHub API 核实）。当前门禁漏检原因：headless abort 模式只构造单回合尾部 torn，不覆盖"闭合错误回合 + 后续回合"序列 | 最小安全修复二选一（或结合）：(a) loop 四条终态路径在 Turn Ended 前补 append `Step Ended`（配对闭合，Turn Ended{Error.requestId} 保留）；(b) repair 改为 DSH 式"截断 torn 前缀 + 追加 closers"：遇尾部 Turn Ended 视为闭合其上方的 open step，不再越过 Turn Ended 继续向更早未配对 Started 截断。修复后补一条回归测试：取消回合→新回合→restore，断言两回合事件与 requestId 全保留。参考：[deepseek-harness index.ts#L436-L442](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/session/session-persistence-jsonl/src/index.ts#L436-L442) |
| **P2** (LN-003) | **边界守卫不覆盖 web-server**：crate_boundaries.rs 的 `layer_of` 表（layer 1-5）不含 web-server → 其 manifest 被静默跳过（`checked>=8` 也测不出）；README 分层图同样未列 web-server。web-server 越过组合根直接依赖 kernel-loop（ReactLoopAgent、DEFAULT_MAX_STEPS）与 kernel-llm（适配器类型），并声明了完全未使用的 kernel-session 依赖。声明的"cargo test --workspace 即全工作区门禁"与可执行守卫范围不符 | kernel-assembly/tests/crate_boundaries.rs:16-25（层表无 web-server）、:56-57（未知 crate 直接 continue）；web-server/Cargo.toml（依赖 kernel-loop/kernel-session/kernel-llm）；web-server/src/api.rs:13、main.rs:88（直接消费 kernel_loop 符号）；全库 grep `kernel_session` 在 web-server/src 零命中（未使用依赖）。cargo metadata 确认 web-server 依赖边为 assembly+contracts+llm+loop+session | 将 web-server 登记进守卫层表（建议 layer 1 壳层，仅允许 assembly+contracts+llm 白名单边，或明示允许的直连集合）并让门禁覆盖全部 workspace 成员（checked 数断言改为 members 数）；同时移除未使用的 kernel-session 依赖。配置化禁止规则的可复用机制见官方 [cargo-deny bans 文档](https://embarkstudios.github.io/cargo-deny/checks/bans/)（已核实：官方工具文档，支持按 workspace 成员声明 deny 规则） |
| **P2** (LN-004) | **LlmPort 错误交付契约与可执行约定互相矛盾**：契约文档规定"错误以 Err 形式结束流"（kernel-contracts/src/llm.rs:345-348），而全部真实适配器（OpenAICompatLlm/MultiProviderLlm）按"错误一律 finish 呈现"实现；loop 的 torn 分支把任何 Err chunk 硬编码为 `code:"LLM_STREAM"`、`request_id:None`（kernel-loop/src/lib.rs:388-401），丢弃 LlmError 已带的结构化事实。任何按书面契约实现的未来适配器，其 QUOTA/AUTH/TRANSPORT/requestId 事实会在日志与 wire 上被拍平丢失。团队已两轮在此处修真 bug（M3.5"流错误统一 finish 呈现"），契约文本却未同步 | kernel-contracts/src/llm.rs:345-348（书面契约）；kernel-llm/src/openai.rs:456-466,501-549,611-619,673-679,812-819（finish 呈现各错误）；kernel-llm/src/multi.rs:45-62（NO_ADAPTER finish）；kernel-loop/src/lib.rs:388-401（Err → LLM_STREAM 硬编码）；docs/HANDOFF_M4_FINISH_2026-08-18.md §5（铁律："错误一律以 finish 呈现"）。权威契约：DSH llm types 中 error/aborted 是携带 LlmFailure 的 finish kind（deepseek-harness master `packages/llm/llm/src/types.ts:120-121`，经 GitHub API 核实） | 把 LlmPort 契约文档改写为可执行约定："协议/业务错误一律以 Finish(Error/Cancelled) 呈现；Err 仅限 torn 传输中断"；同时 loop 的 torn 分支改为经 `LlmError::to_failure()` 归一化透传 code/status/request_id，消除硬编码。单次文档+代码同步改动，无迁移风险。参考：[deepseek-harness llm/types.ts#L120-L121](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/llm/llm/src/types.ts#L120-L121) |
| **P3** (LN-005) | **事件 seq/timestamp 在恢复链路上被丢弃/重写**：`load_events` 只返回 `Vec<SessionEvent>`（seq/timestamp 留在 DB 列上被丢弃）；restore_session 用 `Utc::now()` 重造全部 record；`rewrite_events` 对每个事件重盖 `Utc::now()` 时间戳。唯一事实源在每次 kill-9 修复后丢失原始时间线（wire replay 的 `time` 不再等于原始发生时间，dsh replay 保真语义受损）；seq 因按索引重排尚可保持，timestamp 则不可恢复 | kernel-storage/src/lib.rs:200-219（load_events 丢弃列）、:253-284（rewrite_events 全量重盖时间戳）；kernel-assembly/src/lib.rs:141-147（恢复时 SessionRecord::new 现时戳）。权威契约：DSH wire 事件信封携带 per-event `time` 且 seq 连续（deepseek-harness master `packages/core/session/src/types.ts`，经 GitHub API 核实） | 把 seq+timestamp 纳入事件 JSON 载荷（或让 load_events 返回带列信息的记录），rewrite_events 保留原始 timestamp（修复仅重排/修剪，不改时间戳）。改动局部于 kernel-storage + assembly 恢复路径；旧库兼容性：新库从 JSON 读、缺失则回退列值即可。参考：[deepseek-harness core/session/types.ts](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/session/src/types.ts) |
| **P3** (LN-006) | **kernel-supervisor 已实现但未装配 + 能力声明失真**：kernel-assembly 声明依赖 kernel-supervisor 却从未构造/使用 Supervisor；PluginRuntimePort 仅有 UnavailablePluginRuntime 一个实现，`Runtime::plugin_availability()` 恒为 Unavailable；ports.rs 注释称"M3 接 supervisor 完整实现"不属实；supervisor 模块注释声称"崩溃重启"但只有手动 kill/restart。死依赖参与编译、能力探测面误导后续装配工作 | kernel-assembly/Cargo.toml（dependencies 含 kernel-supervisor）；kernel-assembly/src/lib.rs（全文无 kernel_supervisor/Supervisor 使用，grep 证实）；kernel-contracts/src/ports.rs:89-105（唯一 PluginRuntimePort 实现 = Unavailable）；kernel-supervisor/src/lib.rs:1-5,96-197（无崩溃重启逻辑）；kernel-supervisor 之外全库 grep `Supervisor::` 零命中 | 二选一：把 Supervisor 装配进 Runtime（PluginRuntimePort 实现：availability 探针 + spawn/kill/restart 门面），或从 kernel-assembly 移除该依赖并订正两处注释（"M3 接 supervisor"、模块头"崩溃重启"）。建议先用官方工具 [cargo-udeps](https://github.com/est31/cargo-udeps)（已核实：README 明示"Find unused dependencies in Cargo.toml"）扫一遍 workspace 死依赖，避免同类问题复发 |

已过 materiality 与 acceptable-alternative 门槛、未入账的候选（依据 SKILL §5 拒收）：逐事件单事务 fsync 成本（M1 显式拍板、存储引擎调研已做、单进程规模合理）；EventBus 仅 emit 无 scope 过滤（dsh mux 同全量转发、客户端按会话过滤，无具体代价）；`Runtime` 字段公开允许壳层二次装配（两阶段组合根的实际用法，无证据成本）；session.rename title 不持久化（壳层状态，P2 挂账范畴）；web-server 中 `let _ = agent.run_turn(...)` 吞错（结果已入事件日志并广播）；FsPort/ShellPort 零实现零消费者（预留词汇，无现行成本）；api.rs 单文件 1714 行与 rpc_m3 聚合模块（偏好问题）。

---

## 4. Evolution order and residual risks

**修复顺序（按前置依赖与风险降低）**：
1. **LN-002**（立即）：四条终态路径补 Step Ended + repair 不越过 Turn Ended 截断。消除数据丢失；LN-001 修复后 repair 的 turn 配对才真正参与，故 LN-002 的 repair 侧修复须与其共存设计。
2. **LN-001**（同轮）：run_turn 开回合补 Turn Started。修复重启 blank 分类与 turn 编号；回归测试覆盖"取消→再发→重启"全序列。
3. **LN-004**（随后，低成本）：契约文档与 torn 分支归一化同步。
4. **LN-003**（随后）：守卫覆盖 web-server + 移除未使用依赖。
5. **LN-005**（P3）：seq/timestamp 保真；旧库兼容读。
6. **LN-006**（P3，随插件面开题时一起做）：装配或删除 supervisor 依赖。

**接受的例外（继续有效）**：M1 单事件事务/逐事件 fsync（logged-means-persisted 的强度与写入成本的权衡）；单进程内存态 settings/credentials/goal（文件层登记于 HANDOFF_M4_FINISH §4 P2 队列）；无 retry 瀑布（retry-policy 挂账）；REVIEW_C P2-E 的 bus scope 过滤（单进程 + 客户端过滤语义与 dsh 一致）。

**残余风险/盲点**：
- 本审计未运行 conformance/gate25/m3-r3/hot-replace 四个 Node 轨迹验收（需起服务占固定端口）；以 cargo test 91/91 + clippy 零警告 + gate1 ALL PASS 替代，wire 级回归信任团队既有台账。
- 未做运行时观测（WS/SSE 实流），所有接线为静态追踪 + 单元测试钉死。
- `Runtime::create_session` 持久化失败时内存会话残留（未入账：无现行代价，建议随 LN-006 一并清理）。
- web-server 每会话 `session/subscribed` 基线依赖 `blank`/`running` 内存态，重启即失真（LN-001 修复后解决）。
- LLM 适配层 translate/resolve_thinking 的 `expect("translate")`（openai.rs:324）当前不可 panic（无 Err 路径），未来加入 image 拒收路径时需先改为错误传播。
