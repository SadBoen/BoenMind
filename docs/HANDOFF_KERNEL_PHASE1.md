# HANDOFF —— 阶段 1 开工交接（两线并行）

> **2026-08-14 夜自主运行轮完成**（用户睡觉，ZCode 自主推进 4 小时）：主线 A 除 A6 主体外全部落地（A1-A5+A7），回收站 C1/C2 完成，L9 架构依赖测试落地，proptest 承诺兑现，B1 前置（骨架+依赖图谱）就位，A6 骨架 crate（bm-loop）建立。**全部 25 测试套件全绿 + 两档 clippy 门禁清零 + CI 恢复绿灯。**
> 下一轮：真实验收（用户配合）→ B1 拷入执行 → A6 主体（最优先顺序见 §九·二）。

> 2026-08-14 夜交接（最终版，已推送）。**状态：10 拍板点已全部拍板；阶段 1 前置小修全部落地（181 测试全绿）；重构决策已执行（legacy 旧代码文件夹）；LoopX 研读吸收完成（L1-L17）；两大主线任务分解完毕，下一轮直接开工。**
> 交接原因：用户开新对话续接。

## 〇、本次会话 commit 索引（main 已推送，工作区干净）

| commit | 内容 |
|---|---|
| `2cde412` | L9 架构依赖测试落地：bm-protocol 白名单守卫 + bm-kernel/bm-storage 上层依赖禁止（负向验证过） |
| `2f74f6c` | CI 质量门修复：pdf_omni 15 处存量 clippy lint 清完（main 当时已红） |
| `c561489` | **A1 真序事件**：回调内实时 append（LogItem 队列+写线程攒批），ToolResult.output 落日志，投影按 (turn,step) 归并 |
| `a7f649e` | **A2** request/header + prompt_hash（sha256 长度前缀分段，覆盖 BoenMind 注入面） |
| `bf46d04` | **A4** TurnEndReason::Interrupted + 启动补写（unclosed_turns 查询幂等） |
| `59183ca` | **A3** fork 父前缀折叠（BranchHead.forked_at + 逐段折叠，修跨分支串味 bug） |
| `58ec126` | **A5** subscribe_events（replay-prefix + 250ms tail 轮询）+ SSE 路由 `/api/sessions/{id}/events` |
| `677ae9a`/`f25b669` | **A7** 事件格式迁移链骨架（FORMAT_MIGRATIONS + EventValidator::migrate，读路径全接） |
| `48f3b0d` | **C2** 用户主动清除事件日志（Port.clear_session + DELETE 路由 + 前端菜单 4 语言） |
| `e329c8d` | **C1** 回收站超期自动清除（孤儿会话 90 天，BM_ORPHAN_PURGE_DAYS 可调，每日后台任务） |
| `85b21a4` | **B1 前置**：bm-compat 骨架 + 6 文件依赖图谱 + UPSTREAM_PATCHES 同步纪律（45K 行拷入待下一轮） |
| `087c51d` | **proptest 承诺兑现**：任意操作序列的 append/replay 一致性属性测试（60 用例） |
| `8306e81` | **A6 骨架**：bm-loop crate（ToolRegistry/五扩展点/inbox 双队列）入 workspace+CI |
| `e80582c` | proptest 计数器 clippy 修（enumerate） |

## 〇·五、本轮查证与实现期决策（新对话勿重复调研）

1. **pi 工具输出位置（A1 前置，已查实）**：`ToolExecutionEnd { result: ToolOutput { content, details, is_error } }`（agent.rs:1006）；流式文本 = `MessageUpdate.assistant_message_event: TextDelta`（思考已由 pi 并入正文）；step 边界 = `MessageStart`（每次流式响应一次，agent.rs:1990+）；`AutoCompactionStart/End` 映射为 CompactionStart/End（摘要区间语义留 A6 自研压缩引擎）。
2. **chunk→message 归并**：投影按 (turn, step) 匹配（SurfaceMessage 增 turn/step 字段）；跨分支折叠必须**逐段新建投影**（首版整流折叠被测试抓到"子分支首条消息覆盖父前缀末条"串味）。
3. **fork 点快照**：branch_heads 增 forked_at 列（A3 增量迁移 pragma_table_info 探测），fork 时记录父 head。
4. **A5 轮询订阅**：250ms tail 轮询（阶段 1 实现；A6 落位后换内核总线直推）；SSE 用 unbounded channel + watchdog（1s 探测 receiver drop）。
5. **migrate 骨架**：读路径（replay/derive_messages/visible_events/subscribe prefix）全走 EventValidator::migrate；v0 无迁移步骤 → MigrationUnavailable（与版本化之前拒绝语义一致）。
6. **fork 设计约束**：main 须先有事件才可 fork（超头拒绝）；fork 出的空分支有头行（head 0）可直接再 fork。
7. **clippy 现状**：本地 1.97 新 lint（get_first 等）——pdf_omni 存量已清；新代码需过两档门禁。
8. **proptest 在 async 块内用 assert_eq! 不用 prop_assert_eq!**（宏返回类型冲突）；1.11.0 本地已缓存。
9. **bm-compat 拷入依赖面**（DEPENDENCIES.md 详表）：6 文件 45K 行；scheduler/queue/lane 几乎自包含；extensions_js 仅依赖 8 个 crate 内模块（extensions 40 次引用最多）；5 符号 = ExtensionPolicyMode/ExtensionPolicy/PolicyProfile（extensions.rs）+ HostcallKind/PiJsRuntime（extensions_js.rs 自带）。

## 一、一句话现状

用户逐项拍板 10 项（两线并行/回收站+超期清除/技术 7 条全做/任务量加大），前置小修已全部实现；**pi 旧引擎已物理移入 backend/legacy（生产照跑、测试不再拖累），BoenMind = 全新项目**；LoopX 吸收清单 L1-L17 入架构文档。下一步 = 主线 A（agent-loop 移植）+ 主线 B（pi-compat）**并行**，顺手先加架构依赖测试（L9）。

## 二、拍板记录（2026-08-14 晚，用户逐项确认）

1. ✅ 阶段 1 **两线并行**（agent-loop 移植 + pi-compat 同时开工，Token 并行）
2. ⏳ 真实验收（release 起服务查 event_log 表）——用户睡醒后配合
3. ✅ 事件版本化现在加（已做，见 §三）
4. ✅ fork 投影折叠父前缀（未做，主线 A 内 A6）
5. ✅ 删除 = 保留回收站 + 超期自动清除 + 用户主动清除（设计落架构文档；实现见 §五 C1/C2）
6. ✅ PortBox（已做）
7. ✅ deferred 插件拓扑（已做）
8. ✅ CI 纳入四 crate（已做）
9. ✅ 全局游标 GlobalSeq 类型留口（已做）
10. ✅ 前端隔离后拍（阶段 4，不动）

## 三、前置小修清单（已全部落地，commit 4b85bb1）

| 项 | 内容 | 位置 |
|---|---|---|
| 事件版本化 | `SessionEvent.version` + `SESSION_FORMAT_VERSION=1`（serde default=0，旧数据解析为 0）；`EventValidator::check_version` 在 replay 拒绝不符版本（`format_version_mismatch`） | bm-protocol/event.rs, bm-kernel/validation.rs |
| append 事务化 | 单条 append 与 batch 同语义：BEGIN + INSERT + upsert_head + COMMIT（消除崩溃窗口）；`TursoEventStore::repair_heads()` 启动自愈（head=max(seq)，补缺头行），open 时自动执行 | bm-storage-turso/event_log.rs |
| 读性能 | read 主查询直接带 data 列（去 N+1 逐行重查）；新增 `EventStorePort::count()`（内存 filter + turso COUNT SQL） | bm-storage-turso/event_log.rs |
| turn 计数 | chat.rs 用 `count(sid, bid, Some("turn/start"))` 替代每 prompt 全量 replay（O(n²)→O(1)） | bm-server/chat.rs |
| PortBox | `PortBox<T:?Sized>(Arc<T>)` 进 Any 注册表；`Registry::get_port`/`Ctx::port`/`Kernel::port`；event_store 以 PortBox 注册（插件可按 trait 取用） | bm-kernel/registry.rs, ctx.rs, lib.rs |
| deferred 拓扑 | build 循环分区安装（deps 就绪即装、未就绪挂起；一轮无进展=报"unavailable deps"）；Loader 拆 validate/deps_ready/install；运行期 install_plugin 仍 fail-fast | bm-kernel/lib.rs, loader.rs |
| per-plugin 卸载 | disposers 按插件名分组（安装序）；`Kernel::uninstall_plugin(name)` 组内逆序 fire；Drop 全部逆序；install_plugin 的 try_lock panic 消除（std Mutex） | bm-kernel/lib.rs |
| 一致性小件 | join_all 逆序；parallel 结果按注册序归位（panic 计数附尾）；append_batch 补 Replace 区间校验（批内 max_seen 递增）；fork 名=时间戳+原子计数；`parent_branch: Option<BranchId>`；`source_seqs: Option<Vec<SeqNo>>` | 各文件 |
| GlobalSeq 留口 | `GlobalSeq(u64)` 类型 + 注释（Steward 跨会话观察基线，阶段 5 前落存储） | bm-protocol/ids.rs |
| CI 门禁 | release.yml：test 加四 crate；clippy 拆两行（存量 --lib / 内核四件套 --all-targets -D warnings） | .github/workflows/release.yml |
| vendor P11 | ~~`tests/common/mod.rs` 最小桩~~ **已撤销**：随 legacy 移出 workspace 成员（其 test 目标不再被全量 `cargo test` 编译），桩删除无残留；台账改记 P12（tokio 显式版本，legacy 独立编译） | backend/legacy/UPSTREAM_PATCHES.md |

**验证**：四 crate 78 测试全绿（新增 8：版本拒绝/portbox/拓扑乱序/卸载/parallel 序/repair_heads 自愈等）+ bm-core 68 + bm-server 35 = **181 全绿**；四 crate clippy `--all-targets -D warnings` 零 lint（仅 workspace profile 提示，非 lint）。

## 四、主线 A：agent-loop 移植（任务分解，按序）

> 目标：事件日志从"消息面级事实"（收尾拼的）升级为"执行级事实"（真实顺序/完整输出/压缩可审计）。
> 参照：dsh `core/agent-loop/agent.ts`（496 行）+ dsh 会话事件协议（chunk 逐块/request header 规范化快照/TurnEndReason 六态）。

- **A1 真序事件 + ToolResult.output**：chat.rs 的 `prompt_with_abort` 回调现在是"收尾拼 batch"——改为**回调内实时 append**（TurnStart 已在 run_prompt 开头；回调里按 AgentEvent 序 append StepStart/AssistantChunk/AssistantMessage/ToolCall/ToolResult/TurnEnd）。需查 pi SDK：工具输出在哪类 AgentEvent（现有 map_agent_event 只拿 is_error，输出可能要 ToolOutput 类事件或 MessageEnd 前事件）。**坑**：chunk 事件量大，逐条 append 与批量折中（chunk 攒批 append_batch）。
- **A2 request/header + prompt 快照**：prompt 开头落 `request/header`（provider/model/created_at，reason: initial/change）；EpochHeader 补 `prompt_hash`（system prompt + 工具 schema 的 sha256，模型可见输入的审计锚点）。
- **A3 fork 父前缀折叠投影**（拍板点 4）：SurfaceProjection/derive_messages 支持沿 `parent_branch` 链折叠父前缀（fork 可见分叉点前历史）。branch_heads.parent_branch 已就位。
- **A4 TurnEndReason::Interrupted + 启动补写**：CheckpointStore recover 时对"有 TurnStart 无 TurnEnd"的会话补 `TurnEnd{reason: Interrupted}`（dsh 语义）。
- **A5 EventStorePort::subscribe**（replay-prefix + tail）：SSE 事件流推送（`/api/sessions/{id}/events?after=N`），前端投影引擎前置。
- **A6 自研 ReactLoopAgent（最大块，准内核）**：turn/step 双层、inbox 双队列（next-turn/next-step）、每步从日志投影、五个扩展点（pre-step/request/request-error/tools pre+post/turn-stopping）；LLM client = OpenAI 兼容流式 + 复用 bm-core providers 配置（估 3-5k 行）；压缩双触发（0.8 水线 + overflow 硬触发）接自研压缩引擎后落 CompactionStart/Summary/End 事务。**验收**：替换 pi loop 跑通同一套 30 轮 A/B 压缩对比（方法论已有）。
- **A7 事件格式迁移链骨架**：version bump 时的迁移入口（当前仅拒绝 + 错误码，A7 做 migrate-on-continue 骨架）。

## 五、主线 B：pi-compat（拆法 A，任务分解）

> 目标：vendored QuickJS 引擎作库，pi.dev 200+ 插件当日兼容。已查证：PiJsRuntime 自包含、零 session 耦合；工作量 1-2 周。
> 拆法 A 详述：HANDOFF_EVERYTHING_IS_PLUGIN.md §五。

- **B1 拷入 6 文件 + 5 符号**：`extensions_js.rs / scheduler.rs / hostcall_queue.rs / hostcall_io_uring_lane.rs / embedded_assets.rs / error.rs` + ExtensionPolicy 等 5 符号 → 新 crate（建议 `bm-compat` 或 bm-kernel 子模块）。**上游升级同步这 6 文件的纪律写进 UPSTREAM_PATCHES.md**。
- **B2 host 线程 ~300 行**：`drain_hostcall_requests → HostcallKind 分发 → complete_hostcalls_batch → tick`。
- **B3 加载路径**：eval_file + get_registered_tools；ExtensionBody 协议注册。
- **B4 与内核接线**：QuickJS 运行时 = 插件（ctx 注册 `quickjs` 服务）；工具注册进 A6 自研 loop 的工具注册表（依赖 A6 的工具注册接口，先定接口后并行）。
- **B5 权限询问桥接**：现有 PermissionBridge（vendor P5）复用为 approval 服务。
- **B6 验收**：现有 TS 插件（web_search/web_fetch/ctx-compactor）安装 → 工具可见可调 → 权限弹窗全链路。

## 六、回收站删除行为（拍板点 5 实现）

- **C1 超期自动清除**：bm-server 后台任务（对齐现有 cron/tasks 机制），删 event_log 中 `time < now - N天` 且 session_id 已不在 sessions 表的行（N 建议 90 天，实现期调优）。
- **C2 用户主动清除**：`DELETE /api/sessions/{id}/events`（清该会话事件日志 + 前端入口）。

## 七、已查证事实（新对话勿重复调研）

1. **pi 的 AgentEvent 工具输出位置**：待查（A1 前置——map_agent_event 现有实现只映射了 is_error，输出需找 ToolOutput 类事件）。
2. **turso 参数绑定不支持混用 Option 长度**：SQL 形态显式分支绑定（read 8 分支 / count 2 分支）。
3. **serde(default) 的 version=0 语义**：旧数据无 version 字段解析为 0 → replay 拒绝（比解析失败错误信息更干净）。
4. **Disposer 生命周期纪律**：register_service 等返回的 Disposer 必须交回 apply 的 Vec（丢弃即立即撤销——测试踩过）。
5. **vendor 测试桩**：P11 只覆盖 session_index.rs 用到的 4 成员；上游升级恢复自带 tests/ 时删除桩。
6. **clippy 存量**：bm-core/bm-server 测试代码有历史警告，CI 对它们维持 --lib；四新 crate --all-targets 已零 lint。

## 八、待验证 / 待拍板

1. **真实验收**（用户睡醒后）：release 起 bm-server → 日志"事件日志双写已启用" → 聊几轮 → `sqlite3 ~/.boenmind/boenmind.db "SELECT seq, session_id, type FROM event_log ORDER BY seq"`。
2. **A1 的 chunk 落盘策略**：逐 chunk append vs 攒批（token 级回放保真 vs 写放大）——实现期权衡，先攒批。
3. **超期清除天数**（C1 的 N）——实现期调优。
4. **自研 loop 替换切换开关**：A6 完成后 pi loop 与新 loop 并行双开对比（同压缩 A/B 方法论），拍板切换时机。
5. **proptest 承诺**（实现方案 §6 写了没做）：要不要补——低优先，顺手下轮补。

## 十、本轮后半段新决策（2026-08-14 夜，用户两条指令）

### 10.1 重构决策：全新项目 + legacy 旧代码文件夹（已执行）

用户原话："重构后，可以理解为一个全新的项目，前面的基于 pi-agent-rust 的部分，可以吸收，但不要限制你的发挥，可以把它们的代码移动到一个专门的文件夹中，叫旧代码。吸收一部份就删除一部份，直到完全没用了，就删除掉。"

- **已执行**：`backend/vendor/` → `backend/legacy/`（pi_agent_rust + asupersync + UPSTREAM_PATCHES.md）；移出 workspace 成员（仍为 bm-core/bm-server path 依赖，生产照跑；上游 test 目标不再编译 → P11 桩删除）；P12 登记（tokio 显式版本）。bm-core/bm-server `cargo check` 已过。
- **方针**：心态上 BoenMind 是全新项目；每吸收一个能力出 legacy（自研/插件形态）就删对应 legacy 代码；自研不受 pi 形态约束；终点 = legacy 删空（阶段 6 完成态）。详见架构文档 §十三。

### 10.2 LoopX 借鉴清单（用户点名"看到一个 loopx 项目"）

浅克隆 D:/96_CoderWorld/loopx（huangruiteng/loopx，Python，长时 agent 团队的状态内核）。吸收清单 L1-L14 已入架构文档 §3.6，**要点**：四角色职责模型（观察≠转移、回执≠进度——把关链参照）/ 回合决策词汇表（TurnEndReason 扩展参照）/ 配额 should-run + 交互契约 / **任务认领租约**（100 小弟并行协调模式）/ **架构依赖测试**（铁律 3 从人工审计升级 CI 机器强制——建议尽早落地）/ 交接包与审查包 / dreaming 只建议不执行（Steward 参照）/ 前场后场分离。**定位关系**：我们是 session runtime，LoopX 是 goal-level 控制投影——Steward/目标域按投影接入，不自造第二运行时（L14）。

**下一轮顺手可做**：bm-protocol 零依赖 + bm-kernel 不依赖 bm-server/bm-core 的**架构依赖测试**（L9，成本极低、锁死铁律 3）。

## 九、下一轮续接建议开场

> 继续 BoenMind 阶段 1。交接见 docs/HANDOFF_KERNEL_PHASE1.md。**本轮已落地**（commit 2cde412..e80582c）：主线 A 除 A6 主体外全完成（A1 真序事件+投影归并/A2 request-header+prompt_hash/A3 fork 折叠/A4 Interrupted 补写/A5 订阅+SSE/A7 迁移链骨架）、回收站 C1+C2、L9 架构依赖测试、proptest、B1 前置（bm-compat 骨架+依赖图谱）、A6 骨架（bm-loop crate）。25 测试套件全绿 + 双档 clippy 清零。
>
> **下一轮动手顺序**：① 真实验收（用户配合：release 起服务聊几轮查 event_log 表）；② B1 拷入执行（照 crates/bm-compat/DEPENDENCIES.md 的拷入策略 + shim 最小提取，约 45K 行机械工作）；③ A6 主体（bm-loop 已有骨架：run 循环/LLM client/压缩双触发，B4 的工具注册接口 = ToolRegistry 已定稿）。注意 Disposer 纪律、turso 绑定形态、fork 超头拒绝（main 须先有事件）、跨分支投影逐段折叠四坑（详见 §〇·五）。

## 九·二、本轮完成度表

| 项 | 状态 | 位置/说明 |
|---|---|---|
| L9 架构依赖测试 | ✅ 落地（CI 强制） | bm-protocol/bm-kernel/bm-storage tests/architecture.rs |
| A1 真序事件 + ToolResult.output | ✅ | bm-server/chat.rs（LogItem 队列+写线程）；pi 输出位置已查实 |
| A2 request/header + prompt_hash | ✅ | EpochHeader.prompt_hash（覆盖 BoenMind 注入面） |
| A3 fork 父前缀折叠 | ✅ | forked_at 列 + visible_segments 逐段折叠 |
| A4 Interrupted 启动补写 | ✅ | recover_interrupted_turns（幂等） |
| A5 subscribe + SSE | ✅ | bm-kernel subscribe_events + /api/sessions/{id}/events |
| A6 ReactLoopAgent | 🦴 骨架 | bm-loop crate（ToolRegistry/五扩展点/双队列）；run 循环=主体 |
| A7 迁移链骨架 | ✅ | FORMAT_MIGRATIONS + migrate 读路径全接 |
| C1 超期自动清除 | ✅ | purge_orphaned_events + 每日后台任务（90 天，env 可调） |
| C2 用户主动清除 | ✅ | DELETE /api/sessions/{id}/events + 前端菜单 |
| B1 拷入 6 文件 | 🦴 前置 | 骨架+依赖图谱+台账纪律；拷入待执行 |
| B2/B3/B4/B5/B6 | ⏳ | 依赖 B1 拷入 |
| proptest 承诺 | ✅ | 60 用例属性测试（InMemory） |
| CI 门禁 | ✅ | 全量 25 套件全绿 + 双档 clippy 清零 |
