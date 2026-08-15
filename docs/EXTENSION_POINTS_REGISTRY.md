# 扩展点消费者登记表（2026-08-16 起）

> 纪律（三工具审查轮定调）：**每个扩展点（服务面/挂点/契约变体）必须登记一行**——
> 它的消费者是谁、状态如何。新增扩展点必须同时登记（防静默扩扩展点、防"谜之空货架"）。
> 状态：`消费中`（有生产消费者）/ `待接线`（有接线方，时机未到）/ `建议清理`（真死代码）。
> 依据：docs/review-tools-2026-08-16/EXTENSION_POINTS_ASSESSMENT.md

## 服务面（14 面）

| 面 | 消费者 | 状态 |
|---|---|---|
| memory | 会话侧全局单例（bm_engine build_loop_agent，P2-2 闭环） | 消费中 |
| llm | bm_engine 经 port 解析 LlmConfig | 消费中 |
| provider | LlmPortImpl 经此取官方端点/协议形状（方案 A，2026-08-16）；/api/providers/presets 同源 bm-core 表 | 消费中 |
| skill | routes/skills.rs | 消费中 |
| session | routes/sessions.rs | 消费中 |
| stats | routes/sessions.rs（usage） | 消费中 |
| settings | routes/settings | 消费中 |
| gate | chat.rs respond_permission 回传 | 消费中 |
| event_store | kernel 内部持有 | 消费中 |
| compactor | bm_engine 经 port 取压缩策略（P2-3） | 消费中 |
| tools | 无 lookup——TS 插件经服务面查工具时 | 待接线 |
| notify | 无 lookup——前端事件流/总线直推时 | 待接线 |
| scheduler | 无 lookup——第二个调度器（插件调度/Goal 驱动）时 | 待接线 |
| credentials | 无 lookup——LlmPort 拆 key 独立通道时（A-11） | 待接线 |

## LoopHooks 挂点（12 个）

| 挂点 | 消费者 | 状态 |
|---|---|---|
| on_request | StreamHooks（记忆注入+角色注入） | 消费中 |
| on_stream_chunk | StreamHooks（SSE 文本流）+ SubagentHooks | 消费中 |
| on_tool_pre | StreamHooks（工具卡片事件） | 消费中 |
| on_tool_post | StreamHooks（工具结束事件） | 消费中 |
| on_pre_step | 无——步级审计/统计插件 | 待接线 |
| on_request_error | 无——失败重试策略插件 | 待接线 |
| on_turn_stopping | 无——回合停止钩子（收尾/清理插件） | 待接线 |
| on_context_build | 无——上下文构建观察者 | 待接线 |
| on_compact_begin | 无——压缩事务观察者 | 待接线 |
| on_compact_end | 无——压缩事务观察者 | 待接线 |
| on_turn_end | 无——回合结束观察者/审计 | 待接线 |
| on_provider_select | 无——模型路由/成本策略插件 | 待接线 |

## 其他扩展点

| 扩展点 | 接线方 | 状态 |
|---|---|---|
| EventBus（4 分发模式） | TS 插件域事件（app/*）注册时（阶段 3） | 待接线 |
| declare_event! 宏 | 插件域事件类型层（阶段 3，命名风格届时对齐） | 待接线 |
| 回合队列 enqueue_turn/run | M3 回合内注入（inject 不唤醒语义） | 待接线 |
| StepEnd 契约变体 | 步级审计/回放分析 | 待接线 |
| SessionEndSeed 契约变体 | 会话终结事件化（回收站事件化时补写者） | 待接线 |
| HeaderReason::Resume | M3 断点续跑 | 待接线 |
| fork 分支机制 | M3 session.* 分支工具（文档 §5.1 已标注） | 待接线 |
| pi_name 24 路映射 | 已并入 LLM provider 插件化（方案 A）：删除；stable_id 进 ProviderPort 协议（bm-protocol/port.rs + bm-core ProviderConfig::descriptor，2026-08-16 落地） | 已完成 |
