# 代码回看报告（2026-08-15）

> 触发：用户拍板"编程前先来一次回头看"——原意澄清为**对已完成代码的质量回看**
> （架构符合度/冗余/精炼/优雅/技术栈），非立项评估（立项材料另见
> REVIEW_BEFORE_CODING_APP.md）。范围 = 阶段 0/1 全部产物 + 存量 bm-core/bm-server。
> 方法：三个并行审查（内核层/引擎层/组装层）+ 架构红线与技术栈自审。
> 修复已合入本报告对应 commit。

## 结论摘要

**整体质量：扎实。** 三层审查均确认：依赖方向机器守卫有效（bm-protocol 零依赖、
内核不依赖上层）、事件日志崩溃恢复链完整（事务+UNIQUE+repair_heads+Interrupted 补写）、
压缩 fail-safe 语义有测试固化、安全面（CORS/SSRF/明文 key 权限/日志不打 payload）到位。
未发现推翻性架构问题。

**发现 3 个 P0（均已修复）**、**一批 P1（大部分已修复）**、**P2 若干（精选修复+记录）**。

## P0 修复（正确性风险）

| # | 问题 | 修复 |
|---|---|---|
| 1 | **硬触发压缩后 `last_real_input` 不重置**（bm-loop engine.rs）：压缩成功后 max(粗估, 旧真实值) 恒超窗 → 压缩成功也必判失败回合，错误信息误导 | 压缩成功后 `last_real_input = 0`（投影已重建，旧 usage 失效） |
| 2 | **会话串行保证可被并发打破**（bm-server bm_engine.rs `get_or_create_loop_agent`）：check-then-build-then-insert 三段式在并发（chat + steward 24×7 心跳、参数重建窗口）下产生两个存活 agent 并行写同一事件日志，违背"事件日志唯一状态源" | `LoopSessionEntry` 加 `serial` 会话串行锁；替换条目时**保留旧锁**（在飞回合的锁）；两入口（chat_bm/run_steward_turn）先取串行锁再跑回合；返回 map 中实际生效的组合 |
| 3 | **投影合并绕过 (turn, step) 守卫**（bm-kernel projection.rs）：AssistantMessage 合并第二分支（"空内容+工具调用占位"）不校验 turn/step，跨步塌缩污染模型输入；`attach_tool_call` 同样无守卫 | 合并条件收为纯 (turn, step) 匹配；attach_tool_call 加同款守卫（测试 tool_placeholder 场景不变仍绿） |

## P1 修复（冗余/反模式）

| 项 | 内容 |
|---|---|
| checkpoint.rs 死模块 | bm-storage-turso `checkpoint.rs` 整模块（120 行）零生产调用（fsync 由 synchronous=FULL 直接实现），删除 |
| `loop_engine` 死配置字段 | bm-core AppConfig 字段全仓零读取方（前端选择器已随 pi 废除移除），删除 + 过时注释清理 |
| 过时注释（pi 参照系残留） | bm_engine 模块头/lib.rs init_compat 注释/thinking.rs 模块注释（"pi 运行时仍是最终权威"→ 执行权威 = bm-loop reasoning_effort 映射） |
| steward 路由错误形状 | 裸字符串 400/502 → 全站统一 `api_error`（前端 toast 能读 error 字段） |
| flusher 错误路径任务泄漏 | EventFlusher 加 `Drop`（置 done+notify，写线程自然退出）——`?` 提前返回不再挂起写线程/泄漏 store Arc |
| bus parallel panic 哨兵 | panic 载荷丢失 + `"task_panic: N"` 字符串混入结果数组 → `tracing::error!` 记录 + 结果纯净（bm-kernel 补 tracing 依赖，不违背依赖方向守卫） |
| ToolCallStart 重复执行风险 | 引擎消费侧按 id 去重（上游重复发 name 帧不会双倍执行工具，与 MessageEnd 去重同纪律） |
| **管家失败风暴**（审查连带发现） | dispatch_steward_round 失败路径不清 next_wake_at（到点值残留 → 每 10s 重投失败回合）；`clear_next_wake()` 新方法，dispatch 失败 + inject 失败均回退（与 §〇·五 30④"失败=0 静默"注释对齐）+ 单测 |
| clear_session 非事务 | 两条 DELETE 各自 autocommit → 补 BEGIN/COMMIT/ROLLBACK（与 append 事务同模式） |

## P2（精选修复 + 记录）

已修：port.rs subscribe 过时注释（A5 已落地为 kernel 级订阅）；`startup_sent` 语义注释（每 agent 一次非每会话）。

记录未修（附理由）：
- **inbox 双队列未接线**（bm-loop next-step 队列是丢弃型脚手架）——A6 设计承诺 vs 实现差距；编程应用 M2（活任务清单）时按真实需求接线或删除，现不动
- **prompt_hash 不覆盖注入面**（on_request 改写 payload 后 hash 是旧的）——审计锚点契约降级声明 or 每步重算；影响双开对比对账，M1 前处理
- `EventKind::name()` 分配（可 Cow）、error.rs code_str 双份维护、event_log surface_op/source_seqs 只写不读（表结构冗余）、read() 8 分支重复——均为低收益重构，留待自然重构
- forked_at 迁移不回填存量 fork 行——旧数据影响小，记录
- `PI_SUBAGENT_*` env 命名残留——改名待拍板（登记于 HANDOFF）
- BM_STEWARD_* env 散落 4 处——集中化（StewardConfig）价值明确，M1 后做
- 15min 超时 sleep 任务驻留（3 处）——watch 通道断开后无害，M1 后统一处理
- 架构文档 §5.1 两处偏差（fork 无事件类型、压缩锁恢复未实现）——文档标注或补实现，随 M2 决定

## 技术栈审查（R5）

| 选择 | 判定 | 说明 |
|---|---|---|
| turso（异步 SQLite） | ✅ 合适 | 文件兼容/单写者 Mutex 语义清晰；替代（limbo）无生态收益，不动 |
| rquickjs 0.11 + swc | ✅ 合适 | bm-compat 是拷入层，上游同栈；wasm-host 留口未开 |
| tokio | ✅ 合适 | 全栈一致；bus parallel 用 JoinSet 合理 |
| axum | ✅ 合适 | 路由组织清晰，28 路由无重复模式 |
| 事件日志 + 投影（自研） | ✅ 设计正确 | 崩溃链/压缩遮蔽/订阅 RAII 全有测试；这是核心资产 |
| SSE 手写解析（SseFrameBuf） | ✅ 够用 | 60 行有单测，换 eventsource-stream 不值 |

## 亮点（保持）

- 崩溃恢复姿势完整自洽（事务 + UNIQUE + repair_heads + recover_interrupted_turns 四道防线）
- 压缩事务 fail-safe 与 v0.17 不变式逐条对应，测试固化
- 工具结果裁剪双防线（5MB 硬顶 + 窗口/2 预算，写入/投影双点）配 7 测试
- 日志纪律（payload 不打日志、memorize 只记字符数）
- chat.rs 精简彻底（pi 分支删除后零双写残留）
- 注释质量高（实现期修正均有依据）

## 验证

bm-kernel 55 + bm-storage-turso 34 + bm-loop 32 + bm-server 104 + bm-core 56 + bm-compat 20（5 套件）全绿；clippy 双档零 lint（pre-push 门禁实证）。
