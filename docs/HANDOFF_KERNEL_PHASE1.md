# HANDOFF —— 阶段 1 开工交接（两线并行）

> **当前状态（2026-08-14 pi 废除第②步完成：subagent 子进程已脱离 pi SDK）**：主线 A（A1-A7）+ 主线 B（B1-B6）**全部落地**；**Steward 轮**（管家自我驱动三件套，见 §〇 索引）；**pi 废除第②步（本会话）**：subagent_child.rs 子进程换 **bm-loop**（InMemory 事件日志 + BuiltinTools 执行器 + OpenAiClient 直连），协议形状逐字段对齐 pi AgentEvent serde（父侧 ingest_child_event 零改动），`create_child_session_handle` 死代码删除；**真实验收通过**（手工 spawn 子进程协议流正确 + 聊天全链路：父代理调 subagent → 子进程答 "2" → 父代理转述）。**阶段 1 完成态：自研引擎默认 + 管家自主醒来 + 子代理也跑在自研引擎上——pi 只剩 chat 主路径回退通道与 legacy 待删。**
> - 阶段化废除 pi 的剩余：② chat.rs pi 分支删除（**回退通道观察期未满，留用户拍板**——阶段①承诺 pi 保留 1-2 个版本观察）③ pi 目录（models.json/skills）替换自有设施后 legacy 删空（§十三终点）
> - A 线：执行级事件日志（A1-A5）+ 自研 loop（A6 主体 + 接线：流式/工具/取消）+ A7 迁移骨架
> - B 线：B1 拷入 + B2 host 线程 + B3 加载路径 + B4 工具执行方向 + B5 权限桥（http 真实现）+ B6 收口（内置工具集端口/决策记忆/插件事件推送/切片②顺手件）
> - **v0.17 压缩策略插件化拆解**：bm-loop 只留 Compactor 接口 + 事务协议，bm-compactor 新插件 crate（参数插件自治，可换可关）
> - 双开对比（产物 artifacts/2026-08-14-dual-compare/）：pi 888.6K 发送量 / 峰值 94.1K / 49min vs bm 2263.0K（∑input 口径，缓存命中 2138K）/ 峰值 205.7K / 39.5min——质量不降（记忆 5/5 vs 4/5）；0.5 水线复测已压到 pi 量级以下（artifacts/2026-08-14-waterline-retest/）
> - **下一步**：Steward 轮续接（OS 层静默窗口定时器/管家低成本模型/inject 接桌面壳——§14.5 候选）或 pi 废除收尾（chat.rs pi 分支删除待拍板/legacy 删空）。都可直接开工。
>
> **轮次历史**（细节见 §〇 commit 索引；查实/坑全量见 §〇·五）：
>
> | 轮次 | 内容 | commit | 关键坑/查实 |
> |---|---|---|---|
> | pi 废除第②步 | subagent 子进程换 bm-loop（脱离 pi SDK）+ create_child_session_handle 死代码删除 + 真实验收 | 4997e8b | 协议形状逐字段对齐（见 §〇·五 31）；子进程无插件引擎（工具面=内置∩csv） |
> | Steward 轮 | 管家自我驱动三件套（调度器/inject 通道/next_wake_at 落点 + set_wake 工具 + 覆盖式管家提示词）+ 真实验收 4 回合闭环 | d6ba73d | 模型拒绝扮演管家（身份冲突+历史污染 → 覆盖式提示词+新会话验收）；验收操作见 §〇·五 30 |
> | 夜自主轮 | A1-A5+A7 / C1/C2 / L9 / proptest / 骨架 / CI 修复 | 2cde412..951ea48 | Disposer 纪律、turso 绑定形态、fork 超头拒绝、standalone 起服务 `--features embed` |
> | 白天轮 | 真实验收（197 事件）+ B1 拷入 + A6 主体 + 架构 v0.15 | 1953d3e..2ff4b8a | fresh session 投影=空、embed feature 坑 |
> | 夜轮续接 | B2 host 线程 + B3 加载路径 + bm-compat 入 workspace | 3366cab, 282d629 | HostServices 用 `#[async_trait]`；测试走 `--test host/load/execute`；插件入口 default-export init；`__pi_load_extension` resolve 布尔 true |
> | 白天轮续接 | A6 接线 切片① | 4d227d0, ee4dc5c | pi 双写偶发 `database is locked`（1d4f3c9 已加 busy_timeout=5000，>5s 争用残留观察）；BOENMIND_HOME 布局 = `$HOME/.boenmind/`；硬杀进程 wal 锁自行恢复 |
> | 公司远程轮 | 架构 v0.16 寄生关系定调 + B4 工具方向 | cd2e4e0, 548b6a2, cb92d59 | MiniMax 工具服从性飘忽（非栈问题）；payload 全文不打日志；bm 路径 SSE 事件面须与 pi 对齐；CompatEngine 专用线程（HostThread 含 Rc 非 Send） |
> | 公司远程轮续 | B5 权限桥 | a365789, cb1ae10 | ExtensionPolicy::default=Prompt 且 http 在 default_caps（默认放行）、exec/env 在 deny_caps；决策记忆（extension-permissions.json 同款）留 B6 |
> | B6 收口轮 | 内置工具集端口 + 决策记忆 + 插件事件推送 + 切片② | f81594b, 8b6b725 | 桥调用约定实证（首个 secret 实参不绑定 JS 形参）；tool_result 事件 content 须对齐 legacy ContentBlock 数组；内置工具须进模型可见面（pi BUILTIN_TOOL_NAMES 全开同款）；SELF_TOOLS 跳过 web_search 是插件设计非 bug；目录型插件须 extension.json；debug exe 2GB 坑（CARGO_PROFILE_DEV_DEBUG=0）

## 〇、本次会话 commit 索引（main 已推送，工作区干净）

### CI 提速轮（质量门三件套，2026-08-14 用户要求）

| commit | 内容 |
|---|---|
| `9d43884` | **ci(quality)：质量门提速三件套**——① concurrency cancel-in-progress（同 ref 连续 push 只跑最新一轮，消灭多轮排队）② changes 前置 job（dorny/paths-filter v3，纯 docs 提交跳过 Rust 3 job；tag/手动触发强制全跑）③ sccache（mozilla/sccache-action v0.0.11，GHA backend 2G）对象级编译缓存。**根因实证**（查 CI 日志）：rust-cache 命中仅 ~879MB，pi 依赖树 debug 产物 10GB+，每轮仍重编大部分依赖 → quality-test 20min（clippy 两 job --no-deps 只要 2-3min）；预期 test 20min → 5-8min |

### pi 废除第②步（subagent 子进程换 bm-loop，2026-08-14）

| commit | 内容 |
|---|---|
| `4997e8b` | **feat(subagent)：pi 废除第②步**——subagent_child.rs 子进程从 pi SDK（create_child_session_handle/prompt_with_abort）换 **bm-loop**：InMemory 事件日志 + BuiltinTools 执行器 + OpenAiClient 直连（resolve_llm_config 提 pub(crate) 复用）；无 PI_CODING_AGENT_DIR/models.json 同步；SubagentHooks 流式 delta → message_update（assistantMessageEvent.delta 增量 + message 累积 partial），回合结束补发 message_end + agent_end（messages 兜底）；工具面 = 角色 csv ∩ 内置 7 名（hashline_edit 等 pi 特有工具忽略）；compactor None（单轮短任务）；15min 超时兜底防孤儿化；bm-core create_child_session_handle 死代码删除。**真实验收**：手工 spawn 子进程协议流（增量→message_end→agent_end 回答正确）+ 聊天全链路（父代理调 subagent → 子进程答 2 → 父代理转述）。3 新测试（协议 pointer 对齐/兜底形状/工具交集），107 测试全绿、clippy --lib 零 lint |
| `8bef21e` | **fix(memory)：memorize 英文触发词收紧 remember → remember that**（顺手件·记忆冒烟真实踩坑，见 §〇·五 23 更新）+ 回归测试 3 例 |

### Steward 轮（管家自我驱动三件套，2026-08-14）

| commit | 内容 |
|---|---|
| `d6ba73d` | **feat(steward)：管家自我驱动三件套落地（架构 §14.1/§14.2 → §14.5）**：① steward.rs——StewardStore（`$BOENMIND_HOME/steward.json` 原子写/损坏容错）+ 治理夹区间（pacing-min/max 默认 300s~86400s，env `BM_STEWARD_PACING_MIN_S/MAX_S` 可调）+ set_wake 工具定义/执行（只进管家会话工具面；首调自举/非管家拒绝/0=静默）+ should_wake/note_round_done/in_flight 防重（10 单测）；② bm_engine.rs——run_steward_turn（Goal/Inject 回合，内部通道不碰 session_streams、无 task 记录、15min 超时、落库同 chat 语义）+ spawn_steward_scheduler（10s tick 到点投喂、失败静默防风暴、会话已删清登记）+ 管家会话追加覆盖式身份提示词 STEWARD_SYSTEM_PROMPT；③ routes/steward.rs——POST `/api/steward/inject`（事件→Inject 回合，可带 wake_after_seconds）+ GET `/api/steward/status`。管家会话 = `BM_STEWARD_SESSION` env 指定；未启用 = 零开销。**真实验收 4 回合闭环**（§〇·五 30），104 测试全绿、clippy --lib 零 lint |

### 工具轮（code-graph MCP 装机，2026-08-14 晚间）

| commit | 内容 |
|---|---|
| `7e040d4` | **chore**：`.code-graph/` 入 gitignore（@sdsrs/code-graph 首次运行自动生成的本地索引目录）。**工具装机（ZCode 侧，非仓库代码）**：`~/.zcode/v2/config.json` 新增 `mcp.servers.code-graph-mcp`（npx -y @sdsrs/code-graph serve）——查证录 29 |

### 里程碑轮（subagent 接入 + 默认反转 bm + 验收，2026-08-14）

| commit | 内容 |
|---|---|
| `d4bc5c9` | **feat(engine)：切换自研底座**：① subagent_tool.rs（pi 父侧 subagents.rs 忠实移植：角色发现 frontmatter/三模式请求校验/spawn 子进程/stdout JSON 事件流摄取/结构化结果块 P9 同款；tokio 化——JoinSet+Semaphore 并发、kill_on_drop 取消传播、无 futures 依赖；9 单测）接线 bm_engine 注册 ToolDef + QuickJsToolExecutor 分派；② resolve_loop_engine 默认 pi→bm（自研默认，pi 回退通道）+ 前端 i18n×4 文案同步。**子进程（subagent_child）仍以 pi SDK 跑隔离会话——废除 pi 第②步再换 bm-loop** |
| `80ef79f` | **docs(arch)：v0.18 用户讨论吸收**：§十四 管家自我驱动（回合源三分法 Human/Inject/Goal 协议已预留 + 调度器/next_wake_at 自调节奏=记忆非常量 + 静默协议）+ OpenClaw 外部思路六条（heartbeat/cron 分离、next_check pacing-min/max、HEARTBEAT_OK、observe→decide→dispatch→verify、成本杠杆、统一事件队列）+ APP 分层调用纪律（确定性操作直调宿主端口不走 Agent）+ pi 废除三阶段（①已执行）+ 迭代清单补两件 |

**验收三连（release+embed、隔离 BOENMIND_HOME 复制 config.toml、无 BM_LOOP_ENGINE env）**：① 基础流式（textDelta+done）② ls 工具（toolCallStart/End is_error:false）③ subagent 全链路（父代理调用 subagent 工具 → 子进程 pi SDK 隔离会话回答 "2" → 父代理转述正确）。日志 5× `bm.prompt_usage`（3 回合 5 步）证实默认即 bm 引擎。

### 白天轮（413 修复 + 切换机制接线，2026-08-14）

| commit | 内容 |
|---|---|
| `e0e1224` | **fix(loop)：413 会话污染修复（§〇·五 21，切换前待办清零）**：`clip_tool_output` 双点防线——写入时 >5MB 截断留头 60%/尾 40%（预算=上限−1024 保证裁剪结果≤上限、重复裁剪幂等，UTF-8 char 边界安全，meta 记 truncated/original_bytes）+ 投影时同款裁剪（旧版污染日志/pi 路径原始写入自愈，模型请求永不携带超限内容）；run-compare.mjs 回复记录 200K 截断（§〇·五 21②——查实：SSE 工具事件不带 content，脚本唯一内容通道是 textDelta 回显，防回显撑爆 out）；5 新测试（边界/幂等/写入链/投影自愈/超限集成）全绿，clippy 清零 |
| `bc3b299` | **feat(engine)：切换机制接线（§九·1 的机制部分，默认值未反转）**：AppConfig.loop_engine 持久化字段（settings 同款）+ `resolve_loop_engine` 优先级 env(双开通道)>settings(前端开关)>默认 pi（**反转点收敛此一处**，拍板后改 "pi"→"bm" 即切换）+ chat.rs 调用点接入 + 前端「执行引擎」设置页（三态：跟随默认/自研 bm/上游 pi，AppearanceSettings 卡片同款 + i18n×4 + 导航注册表 engine tab）+ 7 断言优先级测试（**测试抓出 env 空串不穿透 settings 的洞**，修成 and_then 逐层过滤）。后端 68+85 测试全绿、clippy 双档清零、前端 tsc+vite+oxlint 0 错误（2 warning 为 navigation 注册表存量） |

### 并行顺手件轮（governance.memorize + CI 拆并行 + 水线 0.5 复测 + session 集成测试）

| commit | 内容 |
|---|---|
| `e6e09c2` | **feat(compactor)**：默认软水线 0.8→0.5 对齐 pi（双开对比 2263K≈pi 888.6K 的 2.5×、峰值 205.7K vs 94.1K 主因水线差）；对比脚本 run-compare.mjs 加 `--rounds N`（会话标题动态轮数）。**10 轮真实复测**：发送量 617.8K→287.3K（−53.5%）、峰值 202.9K→121.3K（−40%）、首次压缩触发 est 107.1K→65.3K、压后轮末 3.6K vs 144.9K——已压到 pi 量级以下，支持切换前定稿 0.5（SUMMARY 见 artifacts/2026-08-14-waterline-retest/） |
| `86dc798` | **test(compat)**：session 端口 getmessagesurface 集成测试（tests/session.rs 4 用例：pi.session 全链路 op/payload 原样透传 + 宿主返回深度回读 + 错误双路径；tests/common test_thread 泛化任意 HostServices）。查实：pi.session 无白名单、自定义 op 全链路透传，无需 src 改动 |
| `cf6cd33` | **feat(memory)**：governance.memorize 雏形（§九 第 2 条）——remember 入口接通：用户消息命中「记住/remember」→ 提取事实（200 字符上限）→ 记忆插件 remember 落盘；调用点 = run_bm_prompt 挂 attach 前（已持 agent 锁零竞态）；日志只记字符数不落事实全文。governance.rs 10 用例 + 注入链路测试（facts.md → StreamHooks.on_request → system 段；空记忆不注入）。bm-memory lib 已有 5 用例无需补 |
| `6ab80d6` | **ci(quality)**：质量门拆 4 并行 job（quality-test/clippy×2/前端，§八·6 拍板方案）；**补 bm-compactor/bm-memory 门禁**（两 crate 此前从未进 CI test/clippy，含 architecture.rs 架构红线）+ bm-compat 补 --test session |
| `05ccf2e` | fix(loop)：engine_tests 补 LlmUsage cache_read/cache_write 字段（72c6645 结构体扩展的测试侧补齐，上轮漏提交） |

**复测数字（10 轮同口径，SUMMARY.md 详表）**：bm 0.8 组 ∑input 617.8K / 峰值 202.9K(r8) / 首次压缩 r9@est107.1K / 压后 144.9K；bm 0.5 组 287.3K / 121.3K(r9) / r10@est65.3K / 压后 3.6K。0.5 组 12 轮目标第 11 轮起撞 413（会话污染，见 §〇·五 21），10 轮数据有效连续。

### 双开对比轮（pi/bm 30 轮 + v0.17 压缩拆解）

| commit | 内容 |
|---|---|
| `6cbe56d` | **v0.17 压缩策略插件化拆解**：bm-loop 只留 Compactor 策略接口 + 三事件事务协议 + 硬触发兜底（无插件 = 优雅失败不崩不丢历史）；bm-compactor 新 crate（DefaultCompactor：水线 0.8/保留 10%+4000 下限/中部<512 不压/摘要 prompt，参数插件自治）；bm-server 组装层挂默认插件；插件架构方向守卫（禁依赖上层）+ 优雅失败回归测试 |
| `31d4bc9` | docs(arch): v0.17 自我进化定调——§6.9 三定调（效果评估/参数进化插件自治 + 进化=版本化替换待拍）+ 双向奔赴与框架定位（删核心自足性：骨架/手脚分工，跑不跑不是重点）+ compact.rs 越界修正拆法 |
| `72c6645` | **双开对比暴露三修复**：stream_options.include_usage（MiniMax 流式默认 usage:null，token 统计全零）+ cached_tokens 解析（prompt_tokens_details，对齐 pi SDK 口径）+ 压缩水线真实 usage 校准（chars/4 粗估对中文低估约 2×，实测 217K 零触发） |
| `03495e0` | **两个功能推进**：① 记忆插件 bm-memory（facts.md 文件传送带：remember 去重落盘 / open 跨会话加载 / on_request 注入 system 段——§6.1 memory-file 雏形，核心挂点 on_request 第一个真实使用者）；② session 端口补 `getmessagesurface` op（event_log 投影面 = 模型可见历史含压缩遮蔽，双写未启用降级 messages 表）；bm-server 接线（StreamHooks 组合记忆插件 Arc 共享 + init_compat 传投影数据源） |
| `297a4ca` | chore(loop): compact 8 参数 clippy 告警清零 |

### B6 收口轮（内置工具集端口 + 决策记忆 + 插件事件 + 切片②）

| commit | 内容 |
|---|---|
| `f81594b` | **B6 第一批**：bm-compat events.rs（dispatch_extension_event 桥：`__pi_dispatch_extension_event`→task 泵，+2 集成测试+CI 门禁）；bm-server builtin_tools.rs（read/write/edit/grep/find/ls/bash 自研忠实子集，参数/返回形状对齐 pi ToolOutput，递归防护=只查内置表）+ permission_store.rs（extension-permissions.json 决策记忆，格式兼容 legacy permissions.rs，无 fs4/chrono）；compat_engine.rs 六端口换真实现（execute_tool→内置集 / exec→{stdout,stderr,code,killed} / session→会话 DB 子集 / ui→confirm=false / events→active tools）+ 决策记忆命中直返+always 回写 + DispatchEvent 命令；bm_engine.rs startup 每会话懒发 + tool_call/tool_result fire-and-forget + thinking 档映射（七档折叠 reasoning_effort）+ 心跳 TaskProgress（与 pi 同构）；AppState.db 改 Arc<Db> |
| `8b6b725` | **B6 真实验收修复**：内置工具进模型可见面（BuiltinTools::definitions → ToolRegistry，对齐 pi BUILTIN_TOOL_NAMES 全开；QuickJsToolExecutor 按名分派内置/插件）；tool_result 事件 content 形状对齐 legacy ContentBlock 数组（content_blocks 修复 + 单测）；观测日志 bm.plugin_event_done（事件名/结果摘要/ctx_cwd，不打 payload）；events.rs 补 handler 内 hostcall 泵测试。**真实验收全链路通过**（10 轮会话：startup/web_search×50+/web_fetch/pi.tool write 落盘/内置 read/ctx-compactor 修剪落库/心跳/event_log 435 事件闭环；验收会话与 probe 插件已清理） |

### 公司远程轮（B4 工具执行方向）

| commit | 内容 |
|---|---|
| `548b6a2` | **B4 落地**：bm-compat execute.rs（execute_tool 桥：__pi_execute_tool→__pi_task_start→await_js_task 泵循环，镜像 legacy 单 runtime 版）+ tests/execute.rs 2 用例 + load::next_task_id pub(crate) + CI test/clippy 加 --test execute；bm-server compat_engine.rs（CompatEngine 专用线程+命令通道+UnwiredServices 六端口+工具快照 + QuickJsToolExecutor）+ AppState.compat + serve_inner init_compat + bm_engine ToolRegistry 汇合（NoopExecutor 退役） |
| `cb92d59` | bm 路径 SSE 工具事件：StreamHooks 实现 on_tool_pre/post 发 ToolCallStart/End（前端工具卡片零改动）；llm.rs 请求观测只留 tools 计数（payload 含用户消息不打日志） |

### 白天轮续接（A6 接线 切片①：开关 + 空工具跑通）

| commit | 内容 |
|---|---|
| `4d227d0` | **A6 接线落地**：bm-loop 加 `LoopHooks::on_stream_chunk`（points.rs + engine.rs TextDelta 调用点，与日志同源同序；engine_tests RecorderHooks 断言 stream 事件）；bm-server 新模块 bm_engine.rs（StreamHooks 流式通道+断开自动取消 / NoopExecutor 空工具兜底 / provider→LlmConfig 桥接 / 会话级 loop_agents map + 重建零损失 / bm_aborts watch 取消 + 停止按钮 + 15min 超时）；chat.rs `BM_LOOP_ENGINE=bm` 分支（共享前缀：会话校验/命名/add_message；bm 路径 loop 拥有日志全生命周期）；session_streams 统一 unbounded（pi 转发 task/心跳/权限桥同步适配）；lib.rs AppState 两新字段 + sweeper 扩展 + BmAbortEntry 别名 |
| `ee4dc5c` | 补 bm-server→bm-loop 依赖入 Cargo.lock |

### 夜轮续接（B2 host 线程 + B3 加载路径 + bm-compat 入 workspace）

| commit | 内容 |
|---|---|
| `3366cab` | **B2 host 线程**：host.rs（drain→policy 裁决+六端口分发→complete_hostcalls_batch→tick→二轮补收；HostServices 六端口 async_trait + request_approval fail-closed；check_capability 三模式+per-extension 覆盖）+ tests/host.rs 7 用例（六 kind 路由/政策拒绝/审批放行/QuickJS 真链路 eval→pump→complete）；**bm-compat 入 workspace members** + CI 门禁两行（--test host / clippy -D warnings）+ manifest [lints] 表放行 B1 存量（红线逐字节）+ unsafe_code forbid；全量回归 237 绿 |
| `282d629` | **B3 加载路径**：load.rs（JsExtensionLoadSpec verbatim 提取 10195-10276 + load_extension：root 注册→__pi_load_extension→__pi_task_start→await_js_task 泵循环复用 HostThread::pump_once + take_js_task_state 三态）+ tests/load.rs 4 用例（spec 派生/缺失拒绝/**真 TS 插件加载注册工具**/坏入口 rejected 不挂起）+ tests/common 抽共享 MockServices；CI 两行补 --test load；查实：插件入口须 default-export init、load 桥 resolve 布尔 true |

### 白天轮（真实验收 + B1 + A6 主体 + 架构 v0.15）

| commit | 内容 |
|---|---|
| `2ff4b8a` | **B1 拷入执行**：6 文件 45K 行逐字节一致拷入 bm-compat + shim（extensions 1.2K 行符号提取零 stub / tools 进程隔离四符号 / provider_metadata 全量表 / http/crypto/buffer/s3_fifo 整文件）+ 精简 build.rs + 4 源资产 + wasm-host 留口；standalone check 零错误；.gitignore 排除 standalone target/lock |
| `fb21d0c` | **A6 主体**：bm-loop run 循环（UserMessage→header+prompt_hash→步循环→流→工具→软触发→TurnEnd）+ OpenAI 兼容流式 client + 压缩双触发引擎 + ToolGate 拒绝语义 + EventFlusher 屏障冲刷；**内核修**：fresh session 投影 = 空而非 unknown branch；bm-protocol 根导出 core_type_name；L9 守卫 tests/architecture.rs |
| `db1ea7d` | **真实验收通过**：event_log 197 事件两回合真序闭环（浏览器实测）；实测观察 chunk 写放大 ≈ 每 chunk 一批事务（优化留 A6 切换后）；§八·1 补 embed feature 坑 |
| `1953d3e` | **架构 v0.15**：铁律 1 扩写（中间抽象层三层图式 + 分发形态纪律：便携版/Docker = 初级阶段产物）；§7.2 分发形态定位段；交接完成度表收口（下一轮 = B2 + A6 接线） |

### 夜轮（A1-A5+A7 / C1+C2 / L9 / proptest / 骨架 / CI 修复）

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
| `951ea48` | 交接文档更新（§〇·五 查证/决策录 + 九·二 完成度表） |
| `d210cf6` | **CI 修复 ①**：rust-cache 钉 v2.9.1（Node 20 EOL → GitHub 强制 Node 24，v2 浮动标签旧版在 rust-cache 步静默死亡 = 今日全红根因） |
| `373cd64` | **CI 修复 ②**：质量门 job 加 `CARGO_PROFILE_DEV_DEBUG=0` + `CARGO_INCREMENTAL=0`（test+clippy 双编译撑爆 14GB 盘） |
| `1deddcb` | 补 bm-loop 入 Cargo.lock + proptest 回归种子入仓（[Fork] 最小失败用例） |
| — | **CI 质量门已全绿**（26min 冷启动 → 15min 热缓存）；**用户拍板：CI 拆并行 job 下次再做** |

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
10. **CI 两个根因已修**（今日存量故障，修复已推送）：rust-cache 必须 ≥v2.9.1（Node 20 EOL，GitHub 2026-06 起强制 Node 24）；质量门 job 已加 `CARGO_PROFILE_DEV_DEBUG=0`+`CARGO_INCREMENTAL=0`（test+clippy 双编译共盘会爆 14GB）。改动钉在 .github/workflows/release.yml。
11. **桥调用约定（B6 实证）**：Rust `Function::call((secret, a, b, ...))` 多实参时**首个 secret 元素不绑定 JS 形参**（两次对照实验 + events 集成测试钉死；rquickjs 0.11 机制未深究，行为已实证）。bm-compat 所有桥调用保持此约定（events.rs 与 execute.rs/load.rs 一致）。
12. **tool_result 插件事件形状**：content 必须是 ContentBlock 数组（`[{type:"text",text}]`），与 legacy ToolResult 事件对齐——传整包 meta 会让 ctx-compactor 的 extractText 拿到空文本静默跳过（验收踩坑，content_blocks 修复）。
13. **内置工具须进模型可见面**：仅实现 execute_tool 端口不够——模型看不到 read/write 的 schema 就不会调（pi 路径 BUILTIN_TOOL_NAMES 全开同款）。BuiltinTools::definitions → ToolRegistry，executor 按名分派。
14. **SELF_TOOLS 跳过 web_search 是设计**：ctx-compactor 故意不修剪 web_search/web_fetch/subagent（函数返回值需模型完整读取，见插件注释）——排查"事件到了但没落库"先看插件自身的过滤逻辑。
15. **B6 验收操作坑**：目录型插件须 extension.json 清单（enabled_extension_paths 靠它识别）；Windows curl 中文 JSON 会报 invalid unicode（用英文消息）；改插件源须同步 `~/.boenmind/extensions/` 副本；本地跑 bm-server 测试用 `CARGO_PROFILE_DEV_DEBUG=0`（debug exe 2GB 坑）。
16. **MiniMax 流式默认 usage:null**：OpenAI 兼容 API 流式必须显式 `stream_options.include_usage=true` 才回 usage（pi SDK 已带，自研 client 初版没带 → token 统计全零、压缩校准无源）。修法在 bm-loop engine.rs build_payload。
17. **MiniMax 缓存字段**：命中在 `usage.prompt_tokens_details.cached_tokens`（对齐 pi SDK openai.rs 解析）；`prompt_tokens` 是全量（含命中）。**口径差异**：pi 的 input=未命中部分（cache_read=命中，发送量=input+cache_read）；bm 的 input=全量（cache_read=命中子集，发送量=∑input）——对比分析时不可直接相加。
18. **chars/4 粗估对中文低估约 2×**：压缩水线判定必须用 max(粗估, 上一请求真实 usage input)——否则中文手册场景水线形同虚设（实测上下文涨到 217K 零触发）。修法：engine.rs `last_real_input` 校准。
19. **pi 会话句柄断连后坏死**：SSE 断连后 pi 引擎未清理句柄，后续 prompt 1 秒即 false（total_tokens=0）——重启服务 + run-compare `--resume` 可续（重启后 MiniMax 缓存口径变化，报告注明）。
20. **双开对比操作**：两引擎同 prompt 集（rounds.mjs 30 轮）、独立工作区（COMPARE_WORKDIR）、RUST_LOG 须含 `bm_loop=info`（bm 引擎 usage 日志在 bm_loop::engine 不在 bm_server）；分析用 analyze.mjs（pi 组）+ 口径换算（bm 组发送量=∑input）。
21. **MiniMax 413 会话污染（✅ 已修复 e0e1224）**：MiniMax 请求体上限 128MB——某工具返回 ~207MB 内容（约 96s 慢调用，疑 web 抓取；payload 不打日志无法定位具体工具）落进 event_log 后，该会话每次重建历史都超限、永久 413（重试/重启服务均无效，投影从 event_log 重建）。修复=bm-loop `clip_tool_output` 双点（写入裁剪防新污染 + 投影裁剪自愈旧污染/pi 路径原始写入），>5MB 截断留头 60%/尾 40%、预算=上限−1024（幂等）、meta 记原始字节；复测脚本 reply 记录 200K 截断（查实：SSE 工具事件无 content，脚本唯一内容通道是 textDelta）。本坑与水位参数无关（模型工具行为方差）。
22. **压缩事务计数更正**：交接此前写 bm"4 次压缩事务"，日志实为 5 次（`bm.loop_compacted`：turn 9/12/16/21/30，可能少计 turn 30）；0.5 水线下触发点前移（65K）且压得更狠（压后轮末 3.6K），长程频次会更高。
23. **governance.memorize 雏形规则**（cf6cd33 + 8bef21e）：触发词「记住」/「**remember that**」（英文大小写不敏感、只取第一处）——**裸 "remember" 已于 8bef21e 收紧**：英语常用词（"Do you remember..."）任意位置命中会把问句后半截误当事实写入（记忆冒烟真实踩坑），"remember that" 才是明确陈述句引导；"remember to"（待办语义）不触发。提取触发词后文本、去前导分隔符、200 字符硬截断；负向指令/多事实/纠错/LLM 提炼都留 Steward 轮。事件 bm.memory_remembered 只记字符数（用户内容不打日志纪律）。
24. **CI 4 并行 job 拆法**（6ab80d6）：quality-test / quality-clippy（存量 bm-core/bm-server --lib，编译最重单独成 job）/ quality-clippy-kernel（内核四件套+bm-compactor/bm-memory --all-targets + bm-compat）/ quality-frontend；全无 needs 依赖；Rust job 均带 CARGO_PROFILE_DEV_DEBUG=0 + CARGO_INCREMENTAL=0；rust-cache 钉 v2.9.1（Node 20 EOL 坑）。
25. **切换机制接线（bc3b299，查证录补）**：引擎选择优先级 = env `BM_LOOP_ENGINE`（双开对比调试通道）> settings `loop_engine`（前端开关持久化）> 默认；**反转点收敛在 `bm_engine::resolve_loop_engine` 一处**（d4bc5c9 已反转为 "bm"）。坑：`Option::or` 对 Some("") 不穿透——env 设了空串会顶掉 settings 值，须 and_then 逐层 trim+过滤（测试抓出）。前端开关三态（None=跟随默认 / "bm" / "pi"）：null 保存为 undefined（JSON 序列化丢弃字段，后端 serde default 回落 None）。
26. **subagent 父侧移植查证（d4bc5c9）**：协议——父 spawn 当前 exe `--mode json --print --no-session --tools <csv> [--model/--thinking/--append-system-prompt] Task: <task>` + env PI_CODING_AGENT_DIR/PI_SUBAGENT_PARENT_PID/PI_SUBAGENT_DEPTH；子 stdout 逐行 JSON（message_update 增量/message_end 权威/agent_end messages 兜底），stderr 不参与协议。**坑①**：`(&BTreeMap).clone()` 克隆的是**引用**（`Clone for &T` 返回 &T）→ Arc 包成 `Arc<&BTreeMap>` 生命周期逃逸 E0521，须 `(*agents).clone()`；**坑②**：tokio `lines().next_line()` 返回 `io::Result<Option<String>>`，select 分支要 transpose。取消传播 = `kill_on_drop(true)`（上游 AgentCx checkpoint 的等价物）；进度回调省略（ToolExecutor 无此口）。**子进程当前仍以 pi SDK 跑隔离会话**（subagent_child.rs 未动）——废除 pi 第②步换 bm-loop。
27. **OpenClaw 六条吸收（80ef79f，架构 §14.2）**：heartbeat 与 cron 分离为两原语 / next_check 自调节奏 + pacing-min/max 治理夹区间 / HEARTBEAT_OK 静默 / observe→decide→dispatch→verify 状态机 / 24×7 成本杠杆 / 事件驱动统一队列（与 inbox 双队列同构）。来源 openclaw/openclaw #110950、PR #110978、OpenHarness。
28. **验收操作坑（本会话新）**：`/api/sessions/{id}/events` 是 SSE 流式订阅（replay-prefix + tail 轮询），curl 会挂住——验证事件用 messages 面或日志 grep；隔离验收环境 = `BOENMIND_HOME=<新目录>` + 复制真实 config.toml（含 api_key），ensure_builtin_agents 启动时自动建 agents/default.md；Windows curl 中文 JSON 报 invalid unicode（用英文消息）。
29. **code-graph MCP 装机（工具轮，用户拍板"先装上先用着"）**：首选 suatkocar/codegraph **不支持 Windows**（npm 只发 darwin/linux，EBADPLATFORM）→ 改用 **@sdsrs/code-graph v0.116**（Rust AST 知识图谱：callgraph/impact/refs/deps/map/tour/trace/dead-code，本地 SQLite 索引 `.code-graph/index.db`，已入 gitignore）。配置位置 **`~/.zcode/v2/config.json`** 的 `mcp.servers.code-graph-mcp`（command = `C:\Users\Boen\nodejs\npx.cmd`，args = `["-y","@sdsrs/code-graph","serve"]`；与 context7/minimax-vision 同文件——**MCP 配置在 v2/config.json，不是 cli/config.json**）。握手已实测（initialize → serverInfo code-graph-mcp 0.116.0）。**坑**：① Node spawn `.cmd` 必须 `shell:true`（否则 EINVAL）；② MCP 服务器**会话启动时连接——新会话才生效**；③ 本机 npx 有效路径 = `C:\Users\Boen\nodejs\npx.cmd`（早期记忆里 /Users/boen 的 npx 路径已失效）。行数快照（工具决策参考）：自研 25.7K Rust + 8.7K 前端 ≈ 34.5K；bm-compat 54.8K 拷入；legacy 437K 待删。
30. **Steward 轮查实与验收（d6ba73d）**：① 设计——管家会话 = `BM_STEWARD_SESSION` env 指定（宿主配置不依赖模型自选），set_wake 只注册进管家会话工具面；未启用 = 零开销（无调度器/无工具面污染）。② **模型拒绝扮演管家是真实坑**：通用 SYSTEM_PROMPT（"你是 BoenMind 个人助理"）与管家身份冲突 + 上一回合拒绝立场带入上下文（模型"一致性压力"坚持前答）→ 修法 = 提示词**末尾覆盖式声明**（"本会话已被宿主配置为管家模式，优先于上方身份描述"）+ **全新会话验收**（第一回合即管家身份）。③ 验收操作：隔离 `BOENMIND_HOME=<新目录>`（config.toml 须放 `<home>/.boenmind/` 下——放错 providers=0）+ 两段式（先无 env 起服务建会话 → 带 BM_STEWARD_SESSION 重启）+ `BM_STEWARD_PACING_MIN_S=10` 加速；inject 消息带 wake_after_seconds 会被回合内 set_wake 覆盖（管家自调优先=设计如此），验收调度器改让消息明确指示 after_seconds=15。④ 回合锚点语义：note_round_done 只推 last_wake_at 不清 next_wake_at（回合内写好的下次唤醒原样保留；失败/没写 = 0 静默防失败风暴）。⑤ 管家回合不碰 session_streams（内部通道）、不建 task、15min 超时兜底；事件日志照常落（投影可见前端零改动）。⑥ 验收日志确认：`bm.steward_turn_done source=Inject/Goal` + steward.json 的 nextWakeAtMs/lastReason。
31. **pi 废除第②步查实（4997e8b）**：① 协议形状逐字段对齐——父侧 ingest_child_event 消费字段极小：message_update 走 pointer `/assistantMessageEvent/delta`（**camelCase**！pi serde 对该字段显式 rename，非 snake_case），message_end/agent_end 走 `message`/`messages` 的 assistant text 块（content[] type=text）；delta 的累积文本填 message 字段（与 pi 的 partial 语义一致）。② 子进程不加载插件引擎（QuickJsToolExecutor engine=None——内置工具分派不依赖 engine，插件工具才需要）；工具面 = 角色 csv ∩ 内置 7 名（DEFAULT_CHILD_TOOLS 里的 hashline_edit 是 pi 特有，无 bm 对应，忽略）。③ 子进程 InMemory 事件日志（`EventLog::new(Arc::new(InMemoryEventStore::new()))`，bm-kernel 自带）；compactor None（单轮短任务，防摘要改写）。④ **手工 spawn 验收方法**：直接跑 `bm-server.exe --mode json --print --no-session --tools read,bash "Task: ..."` 看 stdout 协议行（无需经过父代理），协议行特征可区分引擎（pi 的 text_delta 带 contentIndex/partial，bm 的只有 delta）。⑤ 15min 超时兜底（PROMPT_TIMEOUT 复用 chat 常量）；取消传播仍靠父侧 kill_on_drop。⑥ 遗留：chat.rs pi 分支删除**待拍板**（阶段①承诺 pi 回退通道保留 1-2 个版本观察，观察期未满）。

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

1. **真实验收**（✅ 2026-08-14 浏览器实测通过）：event_log 197 事件、两回合真序闭环（无工具 + web_search/web_fetch 双工具链，turn/start↔end、tool/call↔result 全配对，branch_heads 同步）；实测观察：chunk 写放大 ≈ 每 chunk 一批事务（notify 即排空，流式均匀到达攒批有限）——优化留 A6 切换后。**⚠️ standalone 起服务必须带前端内嵌 feature**（桌面壳自己 serve 前端不用带）：`cargo build --release -p bm-server --features embed`，否则 `/` 404 空页面。
2. **A1 的 chunk 落盘策略**：逐 chunk append vs 攒批（token 级回放保真 vs 写放大）——已实现"写线程全量排空攒批"（每批一个事务），写放大优化留 A6 切换后统一做。
3. **超期清除天数**（C1 的 N）——默认 90 天，`BM_ORPHAN_PURGE_DAYS` 环境变量可调，实现期调优。
4. **自研 loop 替换切换开关**：A6 主体已落地（bm-loop：run 循环/LLM client/压缩双触发，25 测试全绿）——下一步 = bm-server 接线（开关 + ToolExecutor 先接 pi 插件工具）+ pi loop 与新 loop 并行双开对比（同压缩 A/B 方法论），拍板切换时机。
5. **proptest 承诺**：已兑现（60 用例属性测试，见 §九·二）。
6. **CI 拆并行 job**（✅ 已落地 6ab80d6，2026-08-14 夜轮）：质量门 test/clippy×2/前端拆 4 个并发 job（拍板方案；sccache/大核 runner 两选项未采用，保留后续调优空间）。
7. **前后端分离三点（✅ 2026-08-14 用户定调入架构 v0.15）**：分离原则贯穿设计（[[separation-principle-throughout]]）；用户定调"便携版/Docker 都只是初级阶段的产物"→ 分发形态 ≠ 设计脊梁。收敛结论：① 静态目录服务（`BOENMIND_STATIC_DIR`）登记为**阶段 4 前端隔离的前置小件**（后端永不内嵌前端为默认形态）；② `embed` feature 保留为便携版/Docker 的**打包选项**并标注"打包层非设计层"（Cargo.toml 注释已含，补 README 标注即可）；③ 阶段 4 大动作范围不变。**动作时点：阶段 4 前置小件，不阻塞本阶段。**

## 十、本轮后半段新决策（2026-08-14 夜，用户两条指令）

### 10.1 重构决策：全新项目 + legacy 旧代码文件夹（已执行）

用户原话："重构后，可以理解为一个全新的项目，前面的基于 pi-agent-rust 的部分，可以吸收，但不要限制你的发挥，可以把它们的代码移动到一个专门的文件夹中，叫旧代码。吸收一部份就删除一部份，直到完全没用了，就删除掉。"

- **已执行**：`backend/vendor/` → `backend/legacy/`（pi_agent_rust + asupersync + UPSTREAM_PATCHES.md）；移出 workspace 成员（仍为 bm-core/bm-server path 依赖，生产照跑；上游 test 目标不再编译 → P11 桩删除）；P12 登记（tokio 显式版本）。bm-core/bm-server `cargo check` 已过。
- **方针**：心态上 BoenMind 是全新项目；每吸收一个能力出 legacy（自研/插件形态）就删对应 legacy 代码；自研不受 pi 形态约束；终点 = legacy 删空（阶段 6 完成态）。详见架构文档 §十三。

### 10.2 LoopX 借鉴清单（用户点名"看到一个 loopx 项目"）

浅克隆 D:/96_CoderWorld/loopx（huangruiteng/loopx，Python，长时 agent 团队的状态内核）。吸收清单 L1-L14 已入架构文档 §3.6，**要点**：四角色职责模型（观察≠转移、回执≠进度——把关链参照）/ 回合决策词汇表（TurnEndReason 扩展参照）/ 配额 should-run + 交互契约 / **任务认领租约**（100 小弟并行协调模式）/ **架构依赖测试**（铁律 3 从人工审计升级 CI 机器强制——建议尽早落地）/ 交接包与审查包 / dreaming 只建议不执行（Steward 参照）/ 前场后场分离。**定位关系**：我们是 session runtime，LoopX 是 goal-level 控制投影——Steward/目标域按投影接入，不自造第二运行时（L14）。

**顺手件（✅ 已落地 2cde412）**：bm-protocol 零依赖 + bm-kernel 不依赖 bm-server/bm-core 的**架构依赖测试**（L9，CI 强制）。

## 九、下一轮续接建议开场

> 继续 BoenMind 阶段 2（Steward 轮续接/pi 废除收尾）。交接见 docs/HANDOFF_KERNEL_PHASE1.md。**pi 废除第②步收口（本会话）：subagent 子进程已换 bm-loop**——子代理脱离 pi SDK，协议形状对齐父侧零改动，真实验收通过（架构 §14.4 ②前半）；**Steward 轮（上一会话）**：管家自我驱动三件套 + 真实验收（§14.5）。开工前仍先 git pull。
>
> **下一轮动作（都可直接开工，无需再拍板）**：
> 1. **Steward 轮续接（§14.5 候选，增量为主）**：OS 层静默窗口定时器（宿主侧"1 分钟无汇报主动上报"）、管家回合可换低成本模型（成本杠杆——LoopConfig 按会话/角色解析 provider）、inject 通道接桌面壳/系统事件、steward 状态前端展示（/api/steward/status 已有）。
> 2. **pi 废除收尾**：chat.rs pi 分支删除（**待用户拍板**——阶段①承诺 pi 回退通道保留 1-2 个版本观察）；legacy 删空（③：models.json/skills 替换自有设施后 legacy 删空）。
> 3. 可选顺手件：新会话记忆注入真实冒烟（facts.md → bm 引擎聊天看注入）、双开 30 轮全量复跑（0.5 水线 + 413 修复后验证预测）、code-graph MCP 实测（新会话生效）。
>
> **注意坑**（详见 §〇·五 16-31 + 既有全量）：MiniMax 流式须 stream_options.include_usage、缓存字段在 prompt_tokens_details.cached_tokens、pi/bm 两组 input 口径不同（对比换算）、chars/4 中文低估须真实 usage 校准、pi 句柄断连坏死重启+resume、双开 RUST_LOG 须含 bm_loop=info、413 已修（clip_tool_output 双点）、压缩事务实际 5 次（§〇·五 22）、bm-memory 每会话 open facts.md、桥调用首参 secret 不绑定形参、tool_result 事件 content 用 ContentBlock 数组、内置工具 schema 要注册进 ToolRegistry、SELF_TOOLS 跳过搜索类工具是设计、目录型插件须 extension.json、Disposer 纪律、turso 绑定形态、fork 超头拒绝、standalone 起服务 `--features embed`、bm-compat 测试走 `--test host --test load --test execute --test events --test session`、CompatEngine 专用线程、payload 全文不打日志、**引擎优先级 env>settings>默认 bm（反转点 resolve_loop_engine 一处）**、**subagent 坑：(&BTreeMap).clone() 克隆引用须 (*x).clone()、lines().next_line() 返回 Result<Option> 要 transpose、kill_on_drop 传播取消**、`/api/sessions/{id}/events` 是 SSE 流 curl 会挂、Windows curl 中文 JSON 报 invalid unicode、**Steward 坑（§〇·五 30）：管家提示词须覆盖式声明置尾、验收用全新会话（身份历史污染）、inject 的 wake_after_seconds 会被回合内 set_wake 覆盖、验收加速 BM_STEWARD_PACING_MIN_S=10**、**subagent 子进程坑（§〇·五 31）：协议 pointer 是 camelCase /assistantMessageEvent/delta、子进程无插件引擎（工具面=内置∩csv）、手工 spawn 验收法看协议行特征区分引擎、create_child_session_handle 已删**。

## 九·三、A6 接线设计（2026-08-14 夜轮定稿 → 白天轮切片①落地）

> 目标：`BM_LOOP_ENGINE=bm` 时 chat 走自研 bm-loop，与 pi 引擎并行双开（无工具会话先行，工具方向 B4）。

1. **开关**：环境变量 `BM_LOOP_ENGINE`（`pi` 默认 / `bm` 自研）。AppState 无需加字段——chat handler 读 env 分支即可（双开对比期无需 UI 开关；拍板切换后默认值反转）。
2. **bm-server 依赖 bm-loop**：Cargo.toml 加 path 依赖（bm-loop 不依赖 bm-core，铁律 3 保持）。
3. **engine SSE 流式通道**（bm-loop 改动，前置件）：`run_turn` 现在内部消费 LlmEvent（TextDelta 落日志），无对外输出。加 `stream_tx: &mpsc::UnboundedSender<LlmEvent>` 参数（或 LoopHooks 加 `on_stream_chunk` 钩子——**倾向后者**：钩子形态插件可挂，参数形态每调用方传一遍）。TextDelta 处调用。
4. **OpenAiClient 桥接**：bm-core 的 provider 配置（base_url/api_key/thinking 档位）→ `llm::LlmConfig`。参照 bm-core 现有 provider 解析（resolve_provider / models.json 同步逻辑）；thinking 档位映射复用 thinking-tiers 白名单逻辑。
5. **会话状态**：每 session 一个 ReactLoopAgent（对齐 AgentSessionEntry 模式，加 `loop_agents: Arc<Mutex<HashMap<String, ...>>>`）；**恢复语义** = EventLog 是唯一状态源，进程重启后新建 agent 从日志恢复（turn_count 以日志 TurnStart 计数为准，begin_turn_at 已就位）。
6. **双写衔接**：bm 路径下 loop 直接写 event_log（不再走 chat.rs 的"收尾拼 batch"——A1 的 LogItem 队列是 pi 路径专用）。SSE 前端事件流沿用现有 AgentStreamEvent 形状（前端零改动）。
7. **取消**：现有 aborts map 的 watch 通道接入 run_turn 的 cancel 参数。
8. **验收**：release（`--features embed`）起服务 → `BM_LOOP_ENGINE=bm` → 聊天流式正常 + event_log 投影正确 + 停止按钮可用；然后 pi/双开 A/B 对比（同 30 轮压缩方法论）。
9. **切片**：① 开关+空工具跑通（本轮）；② B4 工具接线（ExecuteTool 方向，下一块）；③ B5 权限桥；④ B6 全链路 + 双开对比。

## 九·二、本轮完成度表

| 项 | 状态 | 位置/说明 |
|---|---|---|
| L9 架构依赖测试 | ✅ 落地（CI 强制） | bm-protocol/bm-kernel/bm-storage tests/architecture.rs |
| A1 真序事件 + ToolResult.output | ✅ | bm-server/chat.rs（LogItem 队列+写线程）；pi 输出位置已查实 |
| A2 request/header + prompt_hash | ✅ | EpochHeader.prompt_hash（覆盖 BoenMind 注入面） |
| A3 fork 父前缀折叠 | ✅ | forked_at 列 + visible_segments 逐段折叠 |
| A4 Interrupted 启动补写 | ✅ | recover_interrupted_turns（幂等） |
| A5 subscribe + SSE | ✅ | bm-kernel subscribe_events + /api/sessions/{id}/events |
| A6 ReactLoopAgent | ✅ 主体 + ✅ 接线（切片①） | bm-loop（run 循环/流式 client/压缩双触发/on_stream_chunk 钩子）+ bm-server bm_engine.rs（开关/流式通道/provider 桥接/会话 map/取消）；**真实验收通过**（4 项见 §〇 白天轮）；**双开对比 = 下一轮（B6）** |
| A7 迁移链骨架 | ✅ | FORMAT_MIGRATIONS + migrate 读路径全接 |
| C1 超期自动清除 | ✅ | purge_orphaned_events + 每日后台任务（90 天，env 可调） |
| C2 用户主动清除 | ✅ | DELETE /api/sessions/{id}/events + 前端菜单 |
| B1 拷入 6 文件 | ✅ | 6 文件 45K 行逐字节一致 + shim（零行为 stub）+ 精简 build.rs；standalone check 零错误；B2 完成时已入 workspace members |
| B2 host 线程 | ✅（夜轮续接） | bm-compat/src/host.rs：drain→policy 裁决+六端口分发→complete 攒批→tick 泵循环（镜像 legacy pump_js_runtime_once_for_owner）+ check_capability 三模式 + request_approval 询问口；tests/host.rs 7 用例全绿（含 QuickJS 真链路）；入 workspace + CI 门禁（3366cab） |
| B3 加载路径 | ✅（夜轮续接） | bm-compat/src/load.rs：JsExtensionLoadSpec（verbatim 提取）+ load_extension（root 注册→__pi_load_extension→__pi_task_start→await_js_task 复用 HostThread 泵）+ PROTOCOL_VERSION；tests/load.rs 4 用例全绿（真 TS 插件加载注册工具/坏入口不挂起）；282d629 |
| B4 工具执行方向 | ✅（公司远程轮） | bm-compat execute.rs（__pi_execute_tool→task→泵循环，2 用例）+ bm-server CompatEngine（专用线程/命令通道/工具快照/UnwiredServices）+ QuickJsToolExecutor；**真实验收通过**（hello 插件工具全链路：SSE 工具事件 + 日志 tool/call↔result + 多步循环）；548b6a2 + cb92d59 |
| B5 权限桥 | ✅（公司远程轮） | BridgeServices（request_approval 询问链接 PermissionBridge 同款机制 + http 端口 reqwest 真实现 + current_session 路由）；**真实验收通过**（web_search×2 + web_fetch×1 真实搜索全成功）；a365789 |
| B6 | ✅ 全部落地 + ✅ 双开对比完成 | 内置工具集端口（execute_tool/exec/session/ui/events 真实现）+ 决策记忆 + 插件事件（startup/tool_call/tool_result）+ 切片②（thinking 映射/心跳）+ **30 轮双开对比**（bm：30 轮无中断/4 压缩事务/记忆 5/5；发送量 2263K vs pi 888.6K，主因水线 0.8 vs 0.5 可调）——**切换时机待用户拍板** |
| proptest 承诺 | ✅ | 60 用例属性测试（InMemory）+ 回归种子入仓 |
| CI 门禁 | ✅ | 全量 25 套件全绿 + 双档 clippy 清零 + **GitHub CI 质量门绿灯**（另修存量 rust-cache/磁盘两故障，见 commit 索引） |
| 真实验收 | ✅ | 浏览器实测两轮（无工具 + web_search/web_fetch 双工具链），event_log 197 事件真序闭环（§八·1） |
| 前端分离三拍板点 | ✅ 定调入架构 v0.15 | 分离原则贯穿设计；便携版/Docker = 分发形态非设计脊梁；静态目录服务 = 阶段 4 前置小件（§八·7） |
| CI 拆并行 job | ✅ 落地（6ab80d6） | 4 并行 job（test/clippy×2/前端）+ bm-compactor/bm-memory 补门禁 + --test session；§八·6 方案 |
| governance.memorize 雏形 | ✅ 落地（cf6cd33） | remember 入口接通（记住指令 → facts.md）+ 注入链路测试；终态留 Steward 轮 |
| 水线 0.8→0.5 + 复测 | ✅ 落地（e6e09c2） | 10 轮真实复测：发送量 −53.5%、峰值 −40%、压后轮末 3.6K；数据支撑拍切换 |
| session 集成测试 | ✅ 落地（86dc798） | tests/session.rs 4 用例（pi.session 全链路）；查实无白名单透传 |
| 413 会话污染修复 | ✅ 落地（e0e1224） | clip_tool_output 双点（写入裁剪 + 投影自愈）+ 复测脚本 reply 截断；5 测试全绿——**切换前待办清零** |
| 切换机制接线 | ✅ 落地（bc3b299） | settings.loop_engine 持久化 + env>settings>默认优先级（反转点一处）+ 前端三态选择器 + i18n×4 |
| subagent 接入 bm 引擎 | ✅ 落地（d4bc5c9） | subagent_tool.rs 父侧忠实移植（三模式/角色发现/事件摄取/结构化块）+ 接线 + 9 测试；子进程换 bm-loop 留废除第②步 |
| 默认引擎反转 bm | ✅ 落地（d4bc5c9） | resolve_loop_engine 默认 pi→bm；pi = env/前端开关回退通道；**阶段 1 完成态达成** |
| 真实验收三连 | ✅ 通过（本会话） | release+embed 无 env 默认即 bm：基础流式 / ls 工具 / subagent 全链路（子进程答 "2" 父代转述正确） |
| 架构 v0.18 | ✅ 落地（80ef79f） | §十四 管家自我驱动 + OpenClaw 六条 + APP 分层调用 + pi 废除三阶段；迭代清单补两件 |
