# HANDOFF —— 阶段 0 最小内核完成（BoenMind 2.0）

> 2026-08-14 夜交接。**状态：阶段 0（T0-T13）全部完成并推送（4 commit），84 测试全绿 + clippy 零警告。**
> 交接原因：会话上下文将满，新开对话续接（沿用上次交接的模式）。

## 一、一句话现状

用户睡前授权"就交给你了"（拍板点 1/3 视为已拍板），从 T0 开工，**阶段 0 最小内核四件套 + 会话事件日志双写全部落地**。下一步 = 阶段 1（agent-loop 移植 / pi-compat / 分支 UI）。

## 二、本次会话提交（main 分支，已推送）

| commit | 内容 |
|---|---|
| `255b2ac` | 内核四件套：bm-protocol（契约层，零运行时依赖）/ bm-kernel（Ctx/Registry/EventBus 四分发/Loader/EventLog 内存实现/校验器/SurfaceProjection）/ bm-storage-turso（turso 落库 + checkpoint + DualWriter）+ 分支 fork（T12 随 T9）+ 集成测试四套件 |
| `ae72750` | T11 双写：bm-server chat 在现有落库同时写事件日志（UserMessage/TurnStart/工具/AssistantMessage/TurnEnd，失败不阻断主链路）+ dual_write_30rounds 验收测试 |
| `f2771a8` | 文档更新：HANDOFF/实现方案标注实现期修正与 partial |
| `3b9d5b7` | 补录：SurfaceProjection 占位合并修正 + Cargo.lock（上一条漏 add） |

## 三、新代码地图（API 面速览）

```
backend/crates/bm-protocol/    契约层（零依赖：serde+serde_json）
  ├─ ids.rs    SessionId/BranchId/SeqNo/CallId（typed 包装）
  ├─ event.rs  CoreEvent（强类型 enum，type="turn/start" 风格）+ CustomEvent + SessionEvent 信封
  ├─ surface.rs SurfaceOp（Append/Replace 压缩遮蔽）
  ├─ port.rs   EventStorePort（BoxFuture 手写签名）+ EventQuery + BranchHead
  ├─ policy.rs Capability + PolicyEvaluation（阶段 2 把关链用）
  └─ error.rs  ErrorCode（seq_gap/unknown_required_event/surface_violation…）+ ProtocolError
backend/crates/bm-kernel/      内核实现
  ├─ lib.rs    KernelBuilder/Kernel（event_log()/ctx()/emit()/on()/service()/install_plugin()）
  ├─ ctx.rs    Ctx（插件唯一视角：service/register_service/on/around/on_async/emit/waterfall/parallel/serial）
  ├─ registry.rs 按 key 注册/取服务（Any 注册表，重复拒绝）
  ├─ bus.rs    EventBus（emit 同步观察/waterfall 短路/parallel 并发扇出/serial 按序）
  ├─ loader.rs Manifest（plugin.json）+ 依赖就绪检查 + 可逆副作用
  ├─ plugin.rs Plugin trait + Disposer（drop 即撤销）
  ├─ event_log.rs EventLog（append/append_batch/replay/derive_messages/fork/head_seq）+ InMemoryEventStore
  ├─ validation.rs EventValidator（check_next_seq/verify_lossless/guard_unknown/verify_replay/check_replace_interval）
  └─ projection.rs Projection trait + SurfaceProjection（消息面重建）
backend/crates/bm-storage-turso/ 存储后端
  ├─ event_log.rs TursoEventStore（EventStorePort 实现）+ MIGRATE_EVENT_LOG（event_log/branch_heads 表）
  ├─ checkpoint.rs CheckpointStore（mark_interrupted/mark_clean/recover）
  └─ dual_write.rs DualWriter（append/append_batch/best_effort + ok/failed 计数）
backend/tests/event_log/       集成测试（bm-event-log-tests）
  ├─ replay_determinism.rs  30 轮流重放两次字节一致 + 内存/turso 消息面一致
  ├─ ignorable_guard.rs     未知事件 ignorable 跳过/必需拒绝 + Custom 永远已知
  ├─ fork_branch.rs         三维寻址/超头拒绝/parent 追踪/turso 持久化
  ├─ checkpoint_recovery.rs 请求边界 fsync + interrupted 恢复
  └─ dual_write_30rounds.rs 30 轮双写验收（chat.rs 同序列）+ turn 号递增
```

## 四、已查证事实（新对话勿重复调研）

1. **seq 不用 AUTOINCREMENT**（对方案 Schema 的实现期修正）：全局计数与"分支内 seq 连续"矛盾（跨分支事件打洞），且事务回滚后 AUTOINCREMENT 不回用号码 → 应用层分配（锁内读 head → +1 → 显式 INSERT），UNIQUE(session_id,branch_id,seq) 兜底。
2. **trait object 不能进 Any 注册表**：`Arc<dyn EventStorePort>` 无法注册/取回（downcast 需要 Sized）→ Registry 里"event_store"只放就绪标记 `Arc<()>`，真身经 `Kernel::event_store()` / `Ctx::event_store()` 特例取用。
3. **bm-protocol 零依赖的实现**：async-trait 用 BoxFuture 手写签名替代（`BoxFuture<'a, T> = Pin<Box<dyn Future + Send + 'a>>`）；async move 块捕获引用参数需提前 clone（否则 lifetime 报错）。
4. **tokio::sync::Mutex 不可重入**：同一线程连续 lock 死锁（chat.rs usage 统计曾踩，已修为先取 guard 再算）。
5. **turso 参数绑定不支持 Option 混合长度**：query 参数按 sql 形态显式选择绑定元组（read 的 8 分支 match）。
6. **未知 ignorable 事件占用 seq 后后续 append 会撞 UNIQUE**：注入测试须同步更新 branch_heads（模拟未来版本内核的完整语义）。
7. **SurfaceProjection 合并规则**：ToolCall 事件在无 assistant 前文时创建"占位 assistant"（content 空挂工具），后续 AssistantMessage 填充该占位而非新建（双写场景消息面 60 条而非 90）。
8. **verify_replay 只查严格递增、不查连续**：ignorable 跳过后留空洞合法（D2 语义）；连续性由写入路径（单写者 + check_next_seq）保证。

## 五、partial 标注（能力矩阵诚实，勿宣称已交付）

- `ToolResult.output` 暂不落日志（chat.rs 双写处 output 空串）——agent-loop 移植（阶段 1）时补；
- `EventStorePort::subscribe`（replay-prefix + tail）未实现——阶段 1 事件流推送；当前 read + head_seq 轮询等效；
- 删除会话时事件日志不联动清理——阶段 1 接 delete 事件流；
- 阶段 0 双写容错：事件日志写失败仅告警计数，主链路不受影响（Option<Arc<DualWriter>>，None 即跳过）。

## 六、待验证 / 待拍板

1. **真实验收（用户醒了可做）**：release 构建启动 bm-server，看日志 `事件日志双写已启用`；聊几轮后查 `~/.boenmind/boenmind.db` 的 event_log 表（sqlite3 CLI：`SELECT seq, session_id, type FROM event_log ORDER BY seq`）；或加一个只读查询路由。模拟验收已全绿（30 轮重放字节一致 + 消息面 60 条）。
2. **阶段 1 立项（拍板点）**：agent-loop 移植（bm-core/agent.rs 事件 → 事件日志完整化：补 ToolResult.output、压缩事件接入、MemoryWrite/TodoWrite）vs pi-compat（QuickJS 引擎拆出，见 HANDOFF_EVERYTHING_IS_PLUGIN §五 拆法 A）——建议先 agent-loop 移植（让事件日志"完整可信"再谈插件化）。
3. **前端隔离机制（拍板点 2）**：iframe/WebComponent/联邦——阶段 4 才需要，可继续后拍。
4. **运行期视角**：AppState.dual_writer 已接线；将来把 EventLog 的 head_seq/replay 接到某只读 API（如 /api/sessions/{id}/events）即可在前端看事件流——阶段 1 范围外，先不动。

## 七、新对话续接建议开场

> 继续 BoenMind 2.0。交接见 docs/HANDOFF_KERNEL_PHASE0.md（阶段 0 完成：T0-T13，84 测试全绿，4 commit 已推送）与 docs/HANDOFF_EVERYTHING_IS_PLUGIN.md（架构四层）。待办：① 真实验收（release 起服务聊几轮查 event_log 表）；② 阶段 1 立项（建议先 agent-loop 移植补 ToolResult.output/压缩事件，再 pi-compat）。

## 八、Git 状态

- main 最新：`3b9d5b7`，工作区干净，全部已推送（github.com 直连正常）
- 记忆：MEMORY.md 有 kernel-phase0-complete 条目（本交接的浓缩版）
