# HANDOFF —— 阶段 1 开工交接（两线并行）

> **2026-08-14 公司远程轮续接（B5 权限桥）**：**BridgeServices 替换 UnwiredServices**（compat_engine.rs）：① `request_approval` 接 PermissionBridge 同款询问链——permission_pending 注册 → `send_permission_request` SSE 推 PermissionRequest → 60s 等决策 → 超时 fail-closed（决策记忆留 B6 前补丁：pi 路径由上游写 extension-permissions.json，bm 路径暂每次询问）；② **http 端口 reqwest 真实现**（镜像 legacy HttpConnector 简化形：除 GET 外一律 POST、响应 {status, headers, body/body_bytes}）——web_search 网络能力就位；③ **current_session 路由**：CompatCmd::Execute 带 session_id，命令循环串行内 set/clear（thread-local 语义），询问路由到发起会话的 SSE 通道；exec/session/ui/events/execute_tool 端口仍 unwired（B6 前补齐内置工具集）；AppState 组件共享重构（session_streams/permission_pending 先建于 serve_inner，CompatEngine 建于 AppState 之前只拿组件）。**真实验收通过**（release+embed + 隔离 home + 真实 web-search 插件含真 key，MiniMax-M3）：**web_search×2 + web_fetch×1 三次真实工具调用全成功**（is_error:false，http hostcall 真发真回）→ 事件日志 3×tool/call↔3×tool/result + turn 闭环 → 模型多轮迭代汇总。查实：ExtensionPolicy::default = Prompt 模式且 http 在 default_caps（默认放行不弹窗）；exec/env 在 deny_caps。commit a365789，main 已推送。
> 下一轮：**B6 全链路验收 + 30 轮双开对比**：① 补 exec/execute_tool/session/ui/events 端口（内置工具集：file 读写/grep/bash——web-scraping/ctx-compactor 依赖）+ 决策记忆（permission_pending 回传写 extension-permissions.json 同款）+ 心跳 TaskProgress + thinking 档位映射；② 现有三插件（web_search/web_fetch/ctx-compactor）在 bm 引擎全链路；③ pi/bm 30 轮 A/B 对比（同压缩方法论）后拍切换。

> **2026-08-14 公司远程轮续接完成（B4 工具执行方向）**：**bm-compat 加 execute_tool 桥**（execute.rs：镜像 legacy execute_extension_tool_sharded 单 runtime 版——`__pi_execute_tool`(bridge_secret,name,call_id,input,ctx)→`__pi_task_start` 挂任务→复用 B3 await_js_task 泵循环；tests/execute.rs 2 用例真 TS 插件执行回读全绿；CI test/clippy 加 --test execute）；**bm-server 加 CompatEngine**（compat_engine.rs：HostThread 含 Rc 非 Send→专用线程+命令通道（Load/Execute/Tools 三命令 oneshot 回结果，天然串行）；UnwiredServices 六端口 B4 全"unwired"、request_approval fail-closed（B5 接）；启动加载启用插件→ExtensionToolDef→bm-loop ToolDef 快照；QuickJsToolExecutor 替换 NoopExecutor（compat None 时兜底）；AppState.compat + serve_inner init_compat）。**真实验收通过**（release+embed + 隔离 home + 真实 hello 插件，MiniMax-M2.5 真实调用）：工具全链路闭环——SSE toolCallStart/toolCallEnd + 事件日志 tool/call↔tool/result（两回合）+ 多步循环（step1 工具→step2 汇总→completed）。commit 548b6a2 + cb92d59，main 已推送。
> 本轮查实/坑：① MiniMax M3/M2.5 对工具指令的服从性飘忽（M3 两次拒调、M2.5 一次拒调一次调）——**不是我们栈的问题**（直连官方 API 对照实验 + payload 比对确认：schema 干净、tools=1 上 payload）；**payload 全文不打日志**（含用户消息，观测只留 tools 计数）；② 验收方法论教训：bm 路径 SSE 事件面必须与 pi 对齐（TextDelta/ToolCallStart/ToolCallEnd/Done），当初"工具没调"是缺工具 SSE 事件的误判——已补（StreamHooks 实现 on_tool_pre/post，零新钩子）；③ CompatEngine Drop = 通道 sender 落下→命令循环退出→join（专用线程收尾）。
> 下一轮：**B5 权限桥**（HostServices::request_approval 接现有 PermissionBridge + UnwiredServices 六端口换真实现——网络 hostcall 让 web_search 全链路）+ 切片② 顺手件（thinking 档位映射 + 心跳 TaskProgress——注意与 chunk 批写同库的锁竞争）→ B6 全链路 + 30 轮双开对比。

> **2026-08-14 白天轮续接完成（A6 接线 切片①）**：**BM_LOOP_ENGINE=bm 开关落地**——新模块 bm-server/src/bm_engine.rs（StreamHooks 流式通道/NoopExecutor 空工具兜底/provider→LlmConfig 桥接（用户端点优先+官方回退，去尾斜杠）/会话级 agent map（换 provider/model 即重建，日志是唯一状态源）/watch 取消通道 + 停止按钮 + 15min 超时）；bm-loop 加 `LoopHooks::on_stream_chunk` 钩子（TextDelta 处调用，与日志同源同序，RecorderHooks 断言更新）；chat.rs 双路径分流（会话校验/命名/add_message 共享前缀，bm 路径下 loop 拥有事件日志全生命周期，SSE 前端形状零改动；session_streams 统一 unbounded）。**真实验收通过**（release+embed + 独立 BOENMIND_HOME，MiniMax-M3 真实流式）：① turn 1 九事件完整闭环（user/message→request/header+prompt_hash→turn/start→step/start→3×assistant/chunk→assistant/message→turn/end completed）；② 多回合投影重建正确（turn 2 模型准确回忆上一条问题）；③ 停止按钮 → turn/end cancelled；④ 客户端断开自动取消（curl head 截断触发 StreamHooks 关闭检测）。pi 路径回归通过（15 事件闭环、真实流式正常）。commit 4d227d0 + ee4dc5c（Cargo.lock），main 已推送。
> 观察坑入账：**pi 路径双写偶发 `database is locked`**（5s 心跳任务进度写 与 chunk 批并发写同库；bm 路径零锁错误——切片①无心跳并发写）——**已修一半（commit 1d4f3c9）**：bm-core db / bm-storage event_log / checkpoint 三处连接加 `busy_timeout=5000`（SQLite 内部等待替代立即报错）；残留：>5s 争用仍会丢 batch（写线程重试队列）+ 心跳/chunk 合并写通道，留双开对比期深查。验收环境注意：BOENMIND_HOME 布局 = `$HOME/.boenmind/config.toml`；硬杀进程后残留 wal 锁会自行恢复。
> 下一轮：**B4 工具方向**（bm-compat ExecuteTool 桥 → ToolExecutor 真实现 → ToolRegistry 汇合）→ B5 权限桥 → B6 全链路 + 30 轮双开对比；切片② 顺手补 thinking 档位映射（当前 bm 路径忽略 thinking，日志有提示）。

> 2026-08-14 夜轮续接完成（B2 + B3）：① **B2 host 线程**落地（bm-compat/src/host.rs ~300 行：drain→policy 裁决+六端口分发→complete 攒批→tick→二轮补收，镜像 legacy pump_js_runtime_once_for_owner；HostServices 六端口 + request_approval fail-closed 询问口预埋 B5；check_capability 三模式裁决）；② **bm-compat 入 workspace + CI 门禁**（members 登记、test/clippy 两行、B1 存量 lint 经 manifest [lints] 表放行——红线拷贝文件逐字节一致；unsafe_code forbid 对齐 legacy；全量回归 237 测试全绿）；③ **B3 加载路径**（load.rs：JsExtensionLoadSpec verbatim 提取 + load_extension 桥接 + await_js_task 泵循环复用 HostThread；**真 TS 插件加载全链路实测**：swc 转译→init(pi)→registerTool→get_registered_tools 读回，4 用例全绿）。commit 3366cab（B2）+ 282d629（B3），main 已推送。
> 查实两协议：__pi_load_extension resolve `true`（布尔成功标志，注册面走 get_registered_tools）；插件入口须 default-export init 函数（loader 自带 export shape 五级归一化回退）。
> 下一轮：**A6 接线**（开关 + bm-loop 路径 + SSE 流式通道 + 空工具 ToolExecutor，见 §九·三 设计）→ B4（bm-compat 工具执行方向）→ 双开对比。

> 2026-08-14 夜自主运行轮完成（用户睡觉，ZCode 自主推进 4 小时）：主线 A 除 A6 主体外全部落地（A1-A5+A7），回收站 C1/C2 完成，L9 架构依赖测试落地，proptest 承诺兑现，B1 前置（骨架+依赖图谱）就位，A6 骨架 crate（bm-loop）建立。**全部 25 测试套件全绿 + 两档 clippy 门禁清零 + CI 恢复绿灯。**

> 2026-08-14 夜交接（最终版，已推送）。**状态：10 拍板点已全部拍板；阶段 1 前置小修全部落地（181 测试全绿）；重构决策已执行（legacy 旧代码文件夹）；LoopX 研读吸收完成（L1-L17）；两大主线任务分解完毕，下一轮直接开工。**
> 交接原因：用户开新对话续接。

## 〇、本次会话 commit 索引（main 已推送，工作区干净）

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
6. **CI 拆并行 job**（用户拍板"下次"）：质量门 test/clippy×2/前端拆 4 个并发 job，预计 15min → 6-9min；方案细节本对话已给出（并行 job / 大核 runner / sccache 三选项）。
7. **前后端分离三点（✅ 2026-08-14 用户定调入架构 v0.15）**：分离原则贯穿设计（[[separation-principle-throughout]]）；用户定调"便携版/Docker 都只是初级阶段的产物"→ 分发形态 ≠ 设计脊梁。收敛结论：① 静态目录服务（`BOENMIND_STATIC_DIR`）登记为**阶段 4 前端隔离的前置小件**（后端永不内嵌前端为默认形态）；② `embed` feature 保留为便携版/Docker 的**打包选项**并标注"打包层非设计层"（Cargo.toml 注释已含，补 README 标注即可）；③ 阶段 4 大动作范围不变。**动作时点：阶段 4 前置小件，不阻塞本阶段。**

## 十、本轮后半段新决策（2026-08-14 夜，用户两条指令）

### 10.1 重构决策：全新项目 + legacy 旧代码文件夹（已执行）

用户原话："重构后，可以理解为一个全新的项目，前面的基于 pi-agent-rust 的部分，可以吸收，但不要限制你的发挥，可以把它们的代码移动到一个专门的文件夹中，叫旧代码。吸收一部份就删除一部份，直到完全没用了，就删除掉。"

- **已执行**：`backend/vendor/` → `backend/legacy/`（pi_agent_rust + asupersync + UPSTREAM_PATCHES.md）；移出 workspace 成员（仍为 bm-core/bm-server path 依赖，生产照跑；上游 test 目标不再编译 → P11 桩删除）；P12 登记（tokio 显式版本）。bm-core/bm-server `cargo check` 已过。
- **方针**：心态上 BoenMind 是全新项目；每吸收一个能力出 legacy（自研/插件形态）就删对应 legacy 代码；自研不受 pi 形态约束；终点 = legacy 删空（阶段 6 完成态）。详见架构文档 §十三。

### 10.2 LoopX 借鉴清单（用户点名"看到一个 loopx 项目"）

浅克隆 D:/96_CoderWorld/loopx（huangruiteng/loopx，Python，长时 agent 团队的状态内核）。吸收清单 L1-L14 已入架构文档 §3.6，**要点**：四角色职责模型（观察≠转移、回执≠进度——把关链参照）/ 回合决策词汇表（TurnEndReason 扩展参照）/ 配额 should-run + 交互契约 / **任务认领租约**（100 小弟并行协调模式）/ **架构依赖测试**（铁律 3 从人工审计升级 CI 机器强制——建议尽早落地）/ 交接包与审查包 / dreaming 只建议不执行（Steward 参照）/ 前场后场分离。**定位关系**：我们是 session runtime，LoopX 是 goal-level 控制投影——Steward/目标域按投影接入，不自造第二运行时（L14）。

**下一轮顺手可做**：bm-protocol 零依赖 + bm-kernel 不依赖 bm-server/bm-core 的**架构依赖测试**（L9，成本极低、锁死铁律 3）。

## 九、下一轮续接建议开场

> 继续 BoenMind 阶段 1。交接见 docs/HANDOFF_KERNEL_PHASE1.md。**公司远程轮已落地（B4 工具方向，commit 548b6a2 + cb92d59）**：execute_tool 桥 + CompatEngine + QuickJsToolExecutor + SSE 工具事件；真实验收通过（hello 工具全链路闭环）。注意本仓库可能有多会话并行推进（busy_timeout 修复来自另一会话）——开工前先 git pull。
>
> **下一轮动手顺序**：① **B5 权限桥**（bm-server 的 UnwiredServices 换真实现：request_approval 接现有 PermissionBridge——HostServices 是 async trait 端口，桥接层在 bm-server 侧把询问转发给 `send_permission_request`/`permission_pending` 通道；六端口逐一接：http（reqwest+SSRF 防护复用 bm-core providers 校验思路）让 web_search 全链路可跑、execute_tool 端口接内置工具集）；② **切片② 顺手件**：thinking 档位映射（当前 bm 路径忽略 thinking，日志有 `bm.loop_thinking_ignored` 提示）+ 心跳 TaskProgress（任务状态条；busy_timeout 已修，注意心跳与 chunk 批写并发仍留观察）；③ **B6 全链路验收 + 30 轮双开对比**（同压缩 A/B 方法论，现有 TS 插件 web_search/web_fetch/ctx-compactor 全链路）后拍切换。
>
> **注意坑**（详见 §〇·五 + 本轮新查实）：Disposer 纪律、turso 绑定形态、fork 超头拒绝、跨分支投影逐段折叠、standalone 起服务 `--features embed`、loop 读回写入前屏障冲刷；**B2/B3 坑**：HostServices 用 `#[async_trait]`；bm-compat 测试走 `--test host --test load --test execute`；插件入口须 default-export init；`__pi_load_extension` resolve 布尔 true；**A6 接线坑**：LoopHooks 钩子全是同步方法（内部锁用 std Mutex 不可 tokio Mutex）、session_streams 已统一 unbounded（新增使用者勿再造 bounded）、BM_LOOP_ENGINE 读 env 每次请求判（双开对比期足够）；**B4 坑**：CompatEngine 专用线程（HostThread 含 Rc 非 Send，命令通道 oneshot 回结果）、bm 路径 SSE 事件面须与 pi 对齐（TextDelta/ToolCallStart/ToolCallEnd/Done）、payload 全文不打日志（含用户消息，观测只留 tools 计数）。

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
| A6 ReactLoopAgent | ✅ 主体 + ✅ 接线（切片①） | bm-loop（run 循环/流式 client/压缩双触发/on_stream_chunk 钩子）+ bm-server bm_engine.rs（开关/流式通道/provider 桥接/会话 map/取消）；**真实验收通过**（4 项见 §〇 白天轮）；**B4 工具方向 + B5 权限桥 + 双开对比 = 下一轮** |
| A7 迁移链骨架 | ✅ | FORMAT_MIGRATIONS + migrate 读路径全接 |
| C1 超期自动清除 | ✅ | purge_orphaned_events + 每日后台任务（90 天，env 可调） |
| C2 用户主动清除 | ✅ | DELETE /api/sessions/{id}/events + 前端菜单 |
| B1 拷入 6 文件 | ✅ | 6 文件 45K 行逐字节一致 + shim（零行为 stub）+ 精简 build.rs；standalone check 零错误；B2 完成时已入 workspace members |
| B2 host 线程 | ✅（夜轮续接） | bm-compat/src/host.rs：drain→policy 裁决+六端口分发→complete 攒批→tick 泵循环（镜像 legacy pump_js_runtime_once_for_owner）+ check_capability 三模式 + request_approval 询问口；tests/host.rs 7 用例全绿（含 QuickJS 真链路）；入 workspace + CI 门禁（3366cab） |
| B3 加载路径 | ✅（夜轮续接） | bm-compat/src/load.rs：JsExtensionLoadSpec（verbatim 提取）+ load_extension（root 注册→__pi_load_extension→__pi_task_start→await_js_task 复用 HostThread 泵）+ PROTOCOL_VERSION；tests/load.rs 4 用例全绿（真 TS 插件加载注册工具/坏入口不挂起）；282d629 |
| B4 工具执行方向 | ✅（公司远程轮） | bm-compat execute.rs（__pi_execute_tool→task→泵循环，2 用例）+ bm-server CompatEngine（专用线程/命令通道/工具快照/UnwiredServices）+ QuickJsToolExecutor；**真实验收通过**（hello 插件工具全链路：SSE 工具事件 + 日志 tool/call↔result + 多步循环）；548b6a2 + cb92d59 |
| B5 权限桥 | ✅（公司远程轮） | BridgeServices（request_approval 询问链接 PermissionBridge 同款机制 + http 端口 reqwest 真实现 + current_session 路由）；**真实验收通过**（web_search×2 + web_fetch×1 真实搜索全成功）；a365789 |
| B6 | ⏳ | 内置工具集端口补齐（exec/execute_tool/session/ui/events）+ 决策记忆 + 心跳 + thinking 映射 → 三插件全链路 → 30 轮双开对比后拍切换 |
| proptest 承诺 | ✅ | 60 用例属性测试（InMemory）+ 回归种子入仓 |
| CI 门禁 | ✅ | 全量 25 套件全绿 + 双档 clippy 清零 + **GitHub CI 质量门绿灯**（另修存量 rust-cache/磁盘两故障，见 commit 索引） |
| 真实验收 | ✅ | 浏览器实测两轮（无工具 + web_search/web_fetch 双工具链），event_log 197 事件真序闭环（§八·1） |
| 前端分离三拍板点 | ✅ 定调入架构 v0.15 | 分离原则贯穿设计；便携版/Docker = 分发形态非设计脊梁；静态目录服务 = 阶段 4 前置小件（§八·7） |
| CI 拆并行 job | 📋 下次 | 用户拍板：test/clippy×2/前端拆 4 并发 job（方案细节见 §八·6） |
