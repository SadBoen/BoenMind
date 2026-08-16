# 扩展点消费者登记表（最后核对：2026-08-17）

> 纪律（三工具审查轮定调）：**每个扩展点（服务面/挂点/契约变体）必须登记一行**——
> 它的消费者是谁、状态如何。新增扩展点必须同时登记（防静默扩扩展点、防"谜之空货架"）。
> **活文档禁止手写「13 面 / 14 面 / 10 挂点」——以此表为唯一计数器。**
>
> 状态：`消费中`（有生产 lookup 或热路径依赖）/ `已注册`（kernel 已挂、尚无第二消费者 lookup）/ `待接线`（有接线方，时机未到）/ `建议清理`（真死代码）。
> **注册 ≠ 消费中。** 图纸 `docs/archive/SERVICE_FACES_2026-08-15.md` 止于当时 13 面，已 superseded。
> 依据：docs/review-tools-2026-08-16/EXTENSION_POINTS_ASSESSMENT.md；2026-08-17 架构文件交叉审查校准。

## 服务面（协议 14 + 运行期 mcp）

| 面 | 消费者 | 状态 |
|---|---|---|
| memory | 会话侧全局单例（bm_engine build_loop_agent） | 消费中 |
| llm | bm_engine 经 port 解析 LlmConfig | 消费中 |
| provider | LlmPortImpl 取官方端点/协议形状；/api/providers/presets 同源 bm-core | 消费中 |
| skill | routes/skills.rs | 消费中 |
| session | routes/sessions.rs | 消费中 |
| stats | routes/sessions.rs（usage） | 消费中 |
| settings | routes/settings | 消费中 |
| gate | chat.rs respond_permission 回传 | 消费中 |
| event_store | kernel 内部持有 | 消费中 |
| compactor | bm_engine 经 port 取压缩策略 | 消费中 |
| credentials | bm_engine 补 api_key（LlmPort JSON 不再带 key）；插件不得经此取密钥 | 消费中 |
| scheduler | set_wake 优先 SchedulerPort（**仅管家启用时注册**） | 消费中（条件注册） |
| tools | 已 register_port，快照与 compat 共享；TS 插件 lookup 工具时 | 已注册（插件 lookup 待接线） |
| notify | 已 register_port（session_streams）；前端事件流/总线直推时 | 已注册（插件 lookup 待接线） |
| mcp | `Arc<dyn bm_mcp::McpService>`，契约在 **bm-mcp** 不在 bm-protocol；`build_loop_agent` / McpGate。启动时零 server 则整面 None（审查 C P1） | 消费中（有连接时才注册） |

## LoopHooks 挂点（12 个）

| 挂点 | 消费者 | 状态 |
|---|---|---|
| on_request | StreamHooks（记忆注入+角色注入） | 消费中 |
| on_stream_chunk | StreamHooks（SSE 文本流）+ SubagentHooks | 消费中 |
| on_tool_pre | StreamHooks（工具卡片事件；裁决在 executor） | 消费中 |
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
| declare_event! 宏 | 插件域事件类型层（阶段 3） | 待接线 |
| 回合队列 enqueue_turn/run | M3 回合内注入（inject 不唤醒语义）。**当前 Steward 走 dispatch_steward_round** | 待接线 |
| StepEnd 契约变体 | 步级审计/回放分析 | 待接线 |
| SessionEndSeed 契约变体 | 会话终结事件化 | 待接线 |
| HeaderReason::Resume | M3 断点续跑 | 待接线 |
| fork 分支机制 | schema+复制 event_log 已落地；session.* 分支工具随 M3 | 部分（发射者待 M3） |
| pi_name 24 路映射 | 已并入 ProviderPort / stable_id（2026-08-16） | 已完成 |

## 产品扩展点（非 Port）

| 扩展点 | 消费者 | 状态 |
|---|---|---|
| 皮肤目录 `skins/` + `data-skin` | 外观设置 / Appearance | 消费中 |
| 插件 `settingsSchema` | 设置中心扩展 tab | 消费中 |
| skill `settings.json` + `settings.value.json` | 设置中心 / skills API | 消费中 |
| MCP 三源发现（config.toml / 标准路径 / TS registerMcpServer） | bm-mcp + serve_inner | 消费中 |
| `[apps.<id>]` + plugin_scopes/skill_scopes | 设置中心 + 引擎按 session.app 过滤 | 消费中 |
| 聊天插入排队（前端 store，非 enqueue_turn） | ChatPane / composer | 消费中 |
