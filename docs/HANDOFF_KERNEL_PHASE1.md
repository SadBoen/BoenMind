# HANDOFF —— 阶段 1 开工交接（两线并行）

> 2026-08-14 夜交接。**状态：复核 10 拍板点已全部拍板；阶段 1 前置小修全部落地并测试全绿（181 测试）；两大主线任务分解完毕，下一轮直接开工。**
> 交接原因：会话上下文过半，用户睡觉期间/新对话续接。

## 一、一句话现状

用户逐项拍板 10 项（两线并行/回收站+超期清除/技术 7 条全做/任务量加大），前置小修已全部实现：**事件版本化、append 事务化+自愈、读性能（去 N+1 + count）、PortBox、deferred 拓扑、per-plugin 卸载、全局游标留口、CI 门禁、vendor P11 修复**。下一步 = 主线 A（agent-loop 移植）+ 主线 B（pi-compat）**并行**。

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

## 三、本次会话已落地（待提交/已提交 commit 索引）

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
| vendor P11 | `tests/common/mod.rs` 最小桩 TestHarness（new/temp_path/log/record_artifact），补回 vendored 刻意不带的 tests/ 引用缺失；台账登记 P11；bm-core updates.rs 清 unused import | vendor/pi_agent_rust/tests/common/mod.rs, UPSTREAM_PATCHES.md |

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

## 九、下一轮续接建议开场

> 继续 BoenMind 阶段 1。交接见 docs/HANDOFF_KERNEL_PHASE1.md（拍板全录、前置小修全落地 181 测试全绿、两线任务分解 A1-A7/B1-B6）。先干 A1（真序事件+ToolResult.output，需先查 pi AgentEvent 工具输出位置）和 B1-B3（拆法 A 拷文件+host 线程），两者解耦可并行。注意 Disposer 纪律与 turso 绑定形态两坑。
