# 工具A：code-architecture 独立审查报告

> 审查日期：2026-08-16 ｜ 方式：只读代码审查（未修改任何文件）
> 审查工具：code-architecture skill（Step 3A Architecture Review 模式）
> 交叉验证：本报告与另两把工具的审查并行独立产出，未互相通信。

## 一、审查概览（范围/方法）

- **范围**：`backend/crates/**`（bm-protocol / bm-kernel / bm-loop / bm-storage-turso / bm-server / bm-core / bm-memory / bm-compactor / bm-compat）、`backend/plugins/**`（6 个 TS 插件）、`backend/tests/event_log/**`、`frontend/src/**`。排除 vendor/target/node_modules/dist/docs/artifacts 等。
- **方法**：按 code-architecture skill 的 Step 3A 执行——先读架构意图文档（docs/everything-is-plugin-architecture.md、docs/HANDOFF_KERNEL_PHASE1.md），再逐 crate 读源码（重点：契约层/内核四件套/事件日志/服务面/装配层/双引擎/前端注册表），最后做交叉 grep 验证（消费者盘点、死代码、重复实现、依赖方向）。每条发现均有 文件:行号 + 可验证证据；不确定项标注"待验证"。
- **代码事实优先**：文档与代码矛盾处以代码为准，矛盾点记录在文（见 §五 第 29 条）。

## 二、现状架构映射（用自己的话）

BoenMind 是一个"事件日志为中心"的单机 AI 运行时，实际分层如下（与文档的分层图有出入，见后）：

```
bm-protocol（纯契约，零运行时依赖）
   └─ EventStorePort / 12 个服务面 Port trait / CoreEvent 信封 / declare_event! 宏
bm-kernel（最小内核：KernelBuilder → Registry/Loader/EventBus/EventLog 语义层）
   └─ 内存事件存储 InMemoryEventStore；EventLog 承诺 append/replay/fork/订阅语义
bm-storage-turso（EventStorePort 的 SQLite/turso 实现：单写者 Mutex + 显式 seq + repair_heads）
bm-loop（自研 ReactLoopAgent：turn/step 双层循环 + EventFlusher 真序落盘 + 压缩事务协议 + LoopHooks 12 挂点）
bm-compactor / bm-memory（"插件"crate：经 KernelBuilder 以 Plugin trait 装配 / 以 LoopHooks 挂进循环）
bm-core（领域层：配置/DB/插件管理/技能/更新/refine——不依赖内核）
bm-server（装配层 + axum HTTP 面：AppState 15 个字段；CompatEngine 专用线程跑 QuickJS；服务面注册；Steward 调度）
bm-compat（vendored QuickJS 引擎：PiJsRuntime + hostcall 队列 + 插件事件分发——插件轨的执行侧）
前端（React + zustand 单 store + dockview 布局：APP/SETTINGS/VIEWS 三张注册表 + 双 DE 壳）
```

**实际数据流**：`POST /api/chat` → chat.rs 校验/落库 → bm_engine::chat_bm → 取/建会话级 `ReactLoopAgent`（串行锁 + agent 锁）→ run_turn：每步从事件日志投影构建 OpenAI payload → 流式落 assistant/chunk 事件（EventFlusher 攒批）→ 工具经 QuickJsToolExecutor 分发（subagent/set_wake/todo/内置工具/QuickJS 插件四路）→ TurnEnd；前端经 SSE 收 AgentStreamEvent，从 `/api/sessions/{id}/events` 订阅事件流做 todo 投影。

**三个关键现实（代码事实，与文档自述一致）**：
1. "万物皆插件"实际是三轨并存：QuickJS 插件轨（6 个 TS 插件，经 bm-compat）、loop 契约轨（Compactor/LoopHooks/MemoryFilePlugin）、组装层编译内置（内置工具/Steward/subagent/pdf-omni 核/终端）。
2. 内核四件套中只有事件日志 + 注册表被生产使用；**EventBus（四种分发模式）与 declare_event! 宏在生产路径零消费**。
3. 服务面 13 面全部注册，但 5 面无消费者、注册姿势三种并存（见发现 1）。

## 三、亮点（做得好的，具体）

1. **契约层纯净性名副其实**：bm-protocol 仅 serde/serde_json 依赖（Cargo.toml），Port trait 用 BoxFuture 手写避免 async-trait——"契约 crate 零运行时依赖"是物理锁而非口头承诺。
2. **事件日志语义实现严谨**：turso 实现把"读 head → 分配 seq → INSERT + upsert_head"全部收进单事务（bm-storage-turso/src/event_log.rs:306-364），早期审查发现的"单条 append 非事务 / N+1 重查"两个问题均已修复；repair_heads 启动自愈、busy_timeout 多实例兜底、ignorable 守卫 + UnknownRequiredEvent 拒绝重建，环环有测试。
3. **真序事件冲刷器（EventFlusher）设计干净**：屏障（barrier）语义保证"读回自己的写入前必落盘"（bm-loop/src/engine.rs:758-872），Drop 兜底防写线程挂起泄漏 Arc——并发正确性靠结构而非纪律。
4. **压缩事务的"骨架/手脚"拆分到位**：事务协议（三事件 + Replace 遮蔽 + fail-safe）留在 bm-loop，策略（水线/尾部/摘要 prompt）在 bm-compactor 插件，参数公开可变——v0.17 越界修正的执行形态正确且被测试固化。
5. **会话串行锁 + agent 重建窗口处理有真功夫**：重建时保留旧条目的 serial 锁（bm_engine.rs:521-530）、sweeper 双重确认 + try_lock（lib.rs:675-693）、StreamHooks attach/detach 生命周期——这类"写者正确性"细节是长跑服务的地基，注释解释了每处竞态。
6. **Steward 三件套落地完整且克制**：BM_STEWARD_* env 唯一读取点（steward.rs:41-44）、治理夹区间、静默窗口 watchdog（head_seq 变化检测防模型挂死）、失败不重试防失败风暴——每个风险都有对应机制，且"未启用 = 零开销"。
7. **服务面作为渐进替换而非闸门**：消费方"经 port、退化直调"（service_faces.rs:86-124）虽是双路径样板（见发现 13），但方向正确——接线失败不阻断主链路，符合"事件日志是渐进式吸收的新家"哲学。
8. **前端注册表纪律好**：APPS/SETTINGS/VIEWS 三张表驱动双 DE 壳（App.tsx:51-59），应用=注册一行，TypeScript Record 类型保证漏改编译报错；dockview 封装在 DockLayout 内不散用上游 API。
9. **内置工具裁剪预算链完整**：写点裁剪（engine.rs:622-625）+ 读点裁剪（projection_to_openai_messages:1006）+ 5MB 硬顶，且 meta 记录原始字节数——"日志存超限结果导致会话永久 413"这类历史坑被系统性防住。
10. **测试文化扎实**：tests/event_log 独立集成套件（replay 确定性/ignorable/fork/checkpoint/proptest）、每 crate 单测覆盖竞态语义、TS 插件头注释完整记录沙箱事实（VFS 不落盘、http 仅 GET/POST）——代码里注释即文档的质量很高。

## 四、发现清单

编号 | 类别 | 严重度 | 位置 | 观察与建议
----|------|--------|------|----------
1 | 架构 | High | backend/crates/bm-server/src/lib.rs:418-554；bm-protocol/src/port.rs | **服务面 13 面"注册完毕"但 5 面无任何消费者**。注册 13 面：build 期 7（memory/settings/stats/llm/skill/session/credentials）+ 插件 1（compactor）+ 内置 1（event_store）+ 运行期 4（tools/notify/gate/scheduler）。grep 全仓验证消费者：`port::<dyn CredentialsPort/MemoryPort/ToolsPort/NotifyPort/SchedulerPort>` 零命中；有消费者的仅 8 面（llm/skill/session/stats/settings/gate/event_store/compactor）。HANDOFF 称"13 服务面全部注册为 kernel 服务"属实，但"接线完毕"言过其实。**建议**：① 按项目自己的 YAGNI 判据（"第一个第二实现出现时"）删除 5 个无消费面，或至少让 CredentialsPort 被 LlmPortImpl 真正消费（现 api_key 读取是复制粘贴，service_faces.rs:168-177 vs 133-155 各读一遍 config）；② 统一注册姿势（见发现 9）。
2 | 架构 | High | bm-kernel/src/bus.rs（全文）；bm-protocol/src/event.rs:267-311 | **内核事件总线（四件套之一）生产零消费**：`emit/waterfall/parallel/serial/around/on_async` 在 bm-server/bm-memory/bm-compactor 全部无调用点（grep 实证，仅 bm-kernel 自身测试在用）；配套的插件域 `declare_event!` 宏（"插件域注册式已落地"的卖点）同样零生产使用（唯一实例在 event.rs:492 的测试）。文档 §15.1 已自认"内核未接线"，但 bm-compactor 接线轮之后总线与宏仍未激活。**建议**：拍板二选一——(a) 收缩总线为 on/emit 两模式，waterfall/parallel/serial 等第二个真实消费者出现再恢复（当前是纯测试代码）；(b) 在 SERVICE_FACES 图纸登记总线启用里程碑（如 QuickJS 插件事件轨退役时），避免无限期双轨。
3 | 架构 | High | bm-server/src/bm_engine.rs:439-535, 640-714；bm-server/src/lib.rs:667-695 | **会话并发模型复杂且三处重复编排**：串行锁 + agent 锁 + bm_aborts 身份匹配 + StreamHooks attach/detach + sweep try_lock 二次确认 + 重建保留旧 serial 锁——机制都对，但"取 agent→取锁→attach→跑→detach→清理"这套协议在 chat_bm（552-714）与 run_steward_turn（909-1039）各写一遍，未来第三个回合源（目标驱动 Goal）会再复制。**建议**：抽 `SessionRunner::run_prompt(state, session, source, 是否接SSE/是否建task)` 公共封装，把超时/心跳/静默窗口/清理全部收进去（顺带消除发现 4 的重复）。
4 | 复用 | Medium | bm_engine.rs:552-714 vs 909-1039 | **chat_bm 与 run_steward_turn 编排代码高度平行**：各自 15min 超时 task、心跳/静默 watchdog、attach/detach、收尾 add_message+touch_session，~150 行重复逻辑仅参数不同。合并进统一回合运行器后，Steward 只是"回合源 + 无 SSE"的配置差异（这正是架构 §14.1 宣称的"共用同一套循环内核"）。
5 | 架构 | Medium | bm-storage-turso/Cargo.toml；src/event_log.rs:210-230 | **存储实现 crate 依赖内核语义层**：bm-storage-turso 依赖 bm-kernel（recover_interrupted_turns 用 EventLog/SurfaceIntent）。当前无环（kernel 不依赖 storage），方向可辩护，但 EventStorePort 的第二个实现（storage-jsonl）将被迫复制该耦合；且 bm-server 在 5+ 处直接 `EventLog::new(kernel.event_store())` 重建语义层（lib.rs:371,503；bm_engine.rs:371,400；compat_engine.rs:503），EventLog 语义层实际悬浮在装配层手里。**建议**：把 recover 逻辑移到 bm-server 或 bm-kernel，让 storage 只实现 Port；给 AppState 加 `event_log()` 便捷方法（已有 event_log_of 但仅 todo 面用，lib.rs:757-762）。
6 | 架构 | Medium | bm_engine.rs:856-865；chat.rs:114-117；compat_engine.rs:369-428 | **双写过渡未闭环：消息面事实源仍是 SQLite messages 表**。loop 拥有事件日志全生命周期（文档宣称"唯一事实源"），但前端历史从 messages 表读、compat 的 getmessagesurface 也降级读 messages 表；todo 已闭环（todo/write 快照 + EventQuery 过滤），消息面没有。文档把切换列为阶段 4（前端投影引擎），但**没有完成判定条件**——建议登记显式里程碑（如"前端 ChatPane 改读 /api/sessions/{id}/events 投影"），防止双写永久化。
7 | 架构 | Medium | bm-server/src/lib.rs:425-437 + bm_engine.rs:360-363；bm-memory/src/lib.rs:58-73 | **记忆双实例**：kernel 注册全局 memory 面（MemoryPortAdapter 包 Mutex<MemoryFilePlugin> 单例，**零消费者**）；每会话 build_loop_agent 又 open 一个 MemoryFilePlugin（同一 facts.md），靠"append 容忍并发写"注释自圆其说。全局单例与每会话实例并存且互不感知（一方 remember 另一方内存态不同步，重开才可见）。**建议**：会话实例改为经 kernel `memory` 面取全局单例——这正是"服务面 = 承诺 API"设计出来的场景；若嫌锁争用，就把全局注册删掉（消灭 5 个无消费面之一）。
8 | 其他 | Medium | compat_engine.rs:826-846 | **权限决策记忆 fallback 链嵌套三层 + 末级 panic**：app_dir 的 extension-permissions.json 打不开 → 降级 tempdir → 再降级 temp_dir+uuid 文件名，末级 `.expect("临时决策记忆不可用")` 会在启动路径 panic。降级后决策记忆静默换位置，用户"总是允许"的选择跨重启丢失且无提示。**建议**：单层 fallback（失败仅 warn + 内存空 store），去掉 expect。
9 | 架构 | Low | lib.rs:517, 523, 529, 553 vs 433-470 | **服务面注册姿势三轨并存且吞错**：build 期 `.with_port(...)` 重复 key 会 fail-fast（lib.rs:125-129），而运行期 `let _ = kernel.ctx().register_port(...)` 把 AlreadyRegistered 静默吞掉（4 处）——若未来两个面重名，一处崩溃一处静默，行为不一致。**建议**：运行期注册也 propagate 错误（启动期注册失败本就该 fail-fast，lib.rs:417 注释自认"装配失败 = 编程错误"）。
10 | 架构 | Low | bm_engine.rs:376-380 | **context_window 硬编码 128_000**（注释"暂取默认"）：压缩水线、窗口预算、工具结果裁剪预算全部由它驱动（window_tool_budget_bytes = window/2），而不同模型窗口差异巨大（64K vs 200K+）——模型窗口与硬编码不符时，压缩判定和工具裁剪会系统性误判。文档 backlog 有"从模型注册表换算"，建议提升优先级。
11 | 其他 | Medium | service_faces.rs:133-155, 168-177 | **api_key 在 Port 边界走 JSON 明文流转**：LlmPort.resolve_config 把 api_key 序列化进 serde_json::Value 返回，bm_engine.rs:295-305 再反序列化回 LlmConfig——密钥在进程内 JSON 里绕一圈，任何一层加日志/透传即泄漏（CredentialsPort 注释承认"明文，仅宿主内部"）。**建议**：LlmPort 返回掩码视图 + 单独 `api_key(provider_id)` 通道供 OpenAiClient 构造，或让 LlmPort 直接返回 LlmConfig 的 JSON 但剥离 key 字段。
12 | 精简 | High | bm-loop/src/points.rs:53-98 | **LoopHooks 12 挂点中 8 个无生产消费者**：生产实现只有 StreamHooks 4 个（on_request/on_stream_chunk/on_tool_pre/on_tool_post，bm_engine.rs:217-261）、MemoryFilePlugin 1 个（on_request）、SubagentHooks 1 个（on_stream_chunk）；on_pre_step / on_request_error / on_turn_stopping / on_context_build / on_compact_begin / on_compact_end / on_turn_end / on_provider_select 仅默认空实现（前三个只在 engine_tests.rs 出现过）。挂点文档称"每个 = 一个真实需求"，但"需求"尚无人认领。**建议**：按项目 YAGNI 判据回删无消费者挂点（engine 调用点同步删），或建"挂点消费者登记表"注释，杜绝静默扩挂点。
13 | 精简 | Medium | service_faces.rs:86-124；routes/skills.rs:15-59；routes/sessions.rs:29,321 | **"经 port、退化直调"双路径样板重复 6+ 处**：settings/skill/session/stats 每个消费方都写 `if kernel.port OK then port else direct`。而 kernel 与 dual_writer 同生共死（lib.rs:418-483：kernel None ⟺ 事件日志不可用），退化分支现实中不可达（kernel 可用性 == port 可用性，注册都在同一 build 里）。**建议**：收敛为一个 `AppState::port_or<T>(key, fallback)` helper，或直接断言 kernel 存在删除退化路径——双重实现让"服务面可替换"停留在装饰层面。
14 | 精简 | Medium | bm-loop/src/engine.rs:929-931；frontend/src/components/chat/ChatWindow.tsx；frontend/src/components/team/ExpertTeamDocs.tsx | **死代码三件**：① `clip_tool_output`（5MB 硬顶版）生产零调用，仅 engine_tests 使用（生产全走 with_budget 版）；② 前端 ChatWindow.tsx（ChatPane 的薄包装，全仓零引用）；③ ExpertTeamDocs.tsx（零引用）。建议删除（git 历史可查），或对有意保留的（如 ChatWindow 作为形态示例）加 `// kept as reference` 标注。
15 | 精简 | Low | chat.rs:157-193 vs compat_engine.rs:144-166 | **权限询问双通道并存**：GatePort（chat.rs respond_permission 经 kernel 面）+ BridgeServices.request_permission 直连 permission_pending 表 + send_permission_request——同一询问表，一半经面一半直连。gate 面目前只有 chat.rs 一个消费者（服务面 #14 的"消费方"就是自己），询问链的真实实现（compat 侧）不经过面。**建议**：compat 侧询问链改为经 gate 面回传，或删除 gate 面。
16 | 精简 | Low | bm-server/src/pdf_omni/**（~1500 行）+ bm-core/src/refine.rs + updates.rs + terminal.rs | **"万物皆插件"的例外面**：pdf-omni 核/refine 审批/自更新/终端全部编译内置在 bm-server/bm-core，pdf-omni 的 TS 插件是 loopback 薄壳（工具执行 = 调自身 HTTP 端点，compat_engine 之外再经一轮 HTTP）。文档已如实标注"组装层编译内置三轨"，属既定过渡；建议在 SERVICE_FACES 图纸或 HANDOFF 登记"内置面插件化"的迁移顺序（pdf-omni 的 loopback 链路每次工具调用多一次 HTTP 往返，成本虽小但可见）。
17 | 复用 | Medium | bm-loop/src/engine.rs:1022-1032 | **prompt_hash 实现注释过期 + 潜在重复**：`prompt_hash_of_parts` 注释称"与 bm-server chat.rs 的 prompt_hash_of 同构"——chat.rs 已无该函数（pi 删除轮随删），注释指向不存在代码；同构函数现在只有一份（好事），但 hash 这种纯函数应上移 bm-protocol（零依赖层）供所有调用方（含未来 request/header 校验）复用。
18 | 复用 | Medium | compat_engine.rs:1079-1113；builtin_tools.rs；TS 插件侧 | **工具结果 content-blocks 协议（`{content:[{type:"text"}]}`）在四侧各解释一份**：compat_engine::tool_result_text（1099-1113）、content_blocks（1079-1089）、builtin_tools 输出、插件 TS 侧 blockText（coding-memory/index.ts）——同一形状协议 Rust 两侧 + TS 两侧重复实现解析/拼接。**建议**：协议形状进 bm-protocol 文档化 + Rust 侧一个 helper，TS 侧保持（跨语言无法共享，至少把形状写进协议文档单一来源）。
19 | 复用 | Low | lib.rs:757-762 vs 5 处 EventLog::new | 见发现 5 后半：`event_log_of` 便捷方法存在但只有 todo 面用，其余 5 处仍从 dual_writer/kernel 拆 Arc 手工构造。统一走 AppState::event_log() 可消除重复且避免每次拆包。
20 | 其他 | Low | service_faces.rs:295-307 | **NotifyPortImpl::push 静默丢事件**：try_lock 失败或 serde 往返失败一律返回 false 且无日志——SSE 前端通道在高频推送（每个 chunk）下若短暂锁争用，事件静默丢失（前端流断但后端不知道）。建议至少加 debug 日志/计数器（事件量小时成本可忽略，但"静默丢"违背项目自己的"诚实标注"风格）。
21 | 其他 | Low | compat_engine.rs:258 | **http hostcall 每次调用新建 reqwest::Client**：连接池/TLS 会话零复用，在插件串行执行通道上叠加连接建立成本（web-search 多源搜索每源一次）。建议 BridgeServices 持共享 Client（Arc）。
22 | 其他 | Low | 多处 | **文档-代码漂移（低危但会误导后续审查）**：① HANDOFF 称 LoopHooks 10 挂点，代码 12 个（points.rs）；② lib.rs:414-416 注释称第一批面是 "memory/settings/stats"，实际注册 7 面（+4 运行期）；③ event.rs:313-317 SESSION_FORMAT_VERSION 注释整段重复两遍；④ service_faces.rs:58 `}impl` 同行排版；⑤ 架构 §15.1 说内核 6060 行（协议 908+内核 2975+loop 2177），实测 loop 已远超（engine 1041 + llm 613 + tests 1008+，单 engine.rs 就 1041 行）——建议文档行数口径更新或删除（行数数字必然漂移，删掉更诚实）。
23 | 架构 | Medium | bm-server/src/lib.rs:391-483 | **serve_inner 装配段单函数 90+ 行闭包嵌套**：kernel 构建塞在一个 `.map(|d| { ... })` 闭包里（418-483），内部再嵌 shared_config/memory/7 个 with_port/插件——装配层是唯一"上帝函数"，所有面的生命周期决策（哪些 build 期、哪些运行期）挤在一处。可辩护（装配点集中），但 13 面铺开后建议拆 `build_kernel(store, config, db) -> Kernel` 独立函数 + 每面一个小构造器，与 service_faces.rs 模块化对齐。
24 | 架构 | Low | lib.rs:340-342 | `std::env::set_var("PI_SUBAGENT_PROVIDER_ID")` 进程级环境变量：桌面壳与独立二进制同进程场景下，多 provider 会话的子代理仍用全局默认（注释已承认，expert-team.md 阶段 1 语义）。既定限制，登记即可。
25 | 其他 | Medium | bm-engine.rs:796-825 + lib.rs:545-547 | **invalidate_loop_agents 后 startup 事件重发耦合**：技能/插件配置变更 → 清空 loop_agents → 下次 prompt 重发 startup 插件事件（ctx-compactor 借此重新加载项目配置）。机制自洽（startup_sent 重置），但"配置变更 → 重发 startup"的因果埋在事件名里，未来插件侧改 startup 语义时无审计锚点。建议把重发原因写进 startup payload（reason: config_changed）。
26 | 精简 | Low | frontend/src/stores/app-store.ts（836 行） | **前端单 store 承载三域状态**：桌面壳窗口状态（openApps/focusedApp/minimized）+ 聊天流状态（streamingText/streamingToolCalls/taskProgress）+ 会话/项目状态同存一个 zustand store。M2 规模下可接受（zustand 切片后可拆），但"专家团队多会话并行"（已拍板属模型层）到来时 streaming 系列状态将无法表达多会话，建议届时按 slice 拆分——现在登记不动作。
27 | 架构 | Low | frontend/src/lib/dock-views.tsx + components/layout/DockLayout.tsx | **布局快照 key 版本化重置用户布局**（HANDOFF 自述"代价=用户自定义布局随版本重置"）：DEFAULT_LAYOUTS 演进 bump 时用户自定义布局全丢。已登记的已知代价，建议顺手做快照指纹迁移（架构 §四·C 预留了精细迁移），防止发布节奏下反复重置惹恼用户。
28 | 其他 | 待验证 | bm-core/src/plugins.rs:20（BUILTIN_PLUGINS）；backend/plugins/ 各 extension.json | **出厂插件与仓库目录的同步机制待验证**：BUILTIN_PLUGINS 声明内置插件 id，实际目录型插件随包发布——"用户卸载后不再恢复"的 removed 记录与插件文件物理存在的关系未细读（ensure_builtin_plugins 只保证装，不保证删文件）。若用户手删目录后 ensure 会重新预装（removed 列表兜底），行为闭环，但未验证，标注待验证。
29 | 架构 | Low | 文档 vs 代码 | **文档与代码的最大结构性落差（两边都诚实，但需拍板）**：文档反复承诺"前端日志投影引擎 SDK 四件套"（§6.3）与"事件域注册式插件事件"，代码里前端仍是 REST + 手写 fetch 事件流（api/client.ts:686 subscribeEvents），事件域只有 todo/write 一个真实案例。落差已入阶段 4 backlog，但**没有"不再实现"的判据**——建议明确：若阶段 4 前出现第二个"事件投影消费方"，就做投影引擎；否则把 §6.3 的 SDK 承诺降级为设计意向。

## 五、结论

**诚实总评**：这是一份**执行力强、文档诚实度高、测试纪律好**的中型代码库，骨架（事件日志 + 自研 loop + 服务面）比绝大多数同规模项目扎实——单写者事务、真序落盘、压缩 fail-safe、会话串行这些"写者正确性"细节不是嘴上说的，是代码和测试钉死的。三轨并存、内核未接线、服务面半消费这些"债"**全部被文档如实登记过**，这在业内少见。

但也要直说三个结构性问题：

1. **"先建后接"的抽象前置**：内核四件套、12 挂点 LoopHooks、declare_event! 宏、13 服务面——大量扩展点先于消费者建成，且项目自己的 YAGNI 判据（"第一个第二实现出现时"）没有被执行（bm-compactor 是第一根接线，但总线/宏/8 个挂点/5 个面至今无第二实现）。**这不是缺陷堆积，是决策惯性**：每轮都"顺手把面铺开"，规模会随轮次线性膨胀。
2. **装配层是唯一瓶颈点**：13 面的生命周期、双引擎、双写、Steward 全部在 serve_inner 一个函数 + AppState 15 字段里握手，任何一面演化都要动这里——这是当前架构最脆的接口。
3. **"唯一事实源"尚未闭环**：todo 已闭环，消息面还在双写过渡，前端投影引擎缺席——文档承诺与代码现实之间的差距需要显式里程碑而非无限期"渐进"。

**优先级建议**（按性价比）：先做 1（删/收无消费者面与挂点，一次清理定调）→ 2（服务面注册姿势统一 + 装配函数拆分）→ 11（密钥 JSON 流转）→ 6/29（消息面闭环判据）→ 其余随轮清理。

---
*本报告为工具A（code-architecture）独立产出；与工具B、工具C的报告交叉对照时，若发现同一问题不同定级，以各自证据链为准。*
