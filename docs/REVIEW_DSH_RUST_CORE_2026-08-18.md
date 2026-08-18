# DSH 核心 Rust 版（kernel/ 微内核）三 SKILL 交叉审查报告

日期：2026-08-18
方法：三个审查 SKILL 各派一个独立子代理并行审查（互不通信），主代理对全部 P0/P1/P2 发现逐一读源码核实，并对照官方 DSH 源码做外部证据验证，最后合并、去重、重定级。

- **code-architecture**（架构审查模式）→ 17 条关切，verdict：架构健康，无 P0/P1
- **code-review**（深度代码审查 Phase 1）→ 49 条问题（1 P0 / 3 P1 / 20 P2 / 25 P3）
- **ln-24-architecture-auditor**（架构适配度审计，清单 44/44 完成）→ verdict：**FAIL**，2 P1 + 4 P2/P3

三份独立报告：`docs/review-dsh-rust-core-2026-08-18/`（code-architecture-report.md / code-review-QUESTIONS.md / ln24-audit.md）

**附轮（同日）：第四方审查者 Grok 4.6 后台派工**——两份越界专项报告（grok-core-review.md / grok-shell-review.md），对照结果见文末"附轮"章节；Grok 独中 1 条新 P2 并独立复现全部共识 P1。

## 结论

**Verdict：FAIL（对齐 ln-24）——2 条已实锤的 P1 正确性缺陷，其余为 P2/P3 加固面。架构本身健康。**

两个 P1 都发生在同一核心机制（turn/step waterfall 事件瀑布）上，且互相叠加：

1. **Turn Started 从不落日志**（code-review 报 P0、ln-24 报 P1，架构审查漏报）：`ReactLoopAgent::run_turn` 全程只 append `Turn Ended`，从未 append `TurnEvent::Started`。后果链：`next_turn()` 恒返回 1 → 所有回合 turn=1、wire 永远没有 `turn/start` 帧、重启恢复的 blank 判定（`has_turn_start`）恒 true——有历史的会话恢复后全被当成空会话。官方 DSH `agent.ts` 每回合开头 append `'turn/start'`（已核实），这是与上游的事件语义偏差，非设计选择。
2. **错误/取消回合不闭合 Step → 恢复修剪造成历史删除**（ln-24 报 P1，code-review 以 BUG-008 部分命中）：四条终态路径（流 Err / Finish 缺失 / Cancelled / Error-finish）只写 `Turn Ended` 不写 `Step Ended`，日志尾部留下未配对的 `Step Started`；`repair_interrupted_turn` 在尾部第一个未配对 `Step Started` 处**整段截断**——(a) 已闭合的错误回合（含 M4 刚上的 requestId 审计事实）在重启时被连根删掉；(b) 若取消/报错后用户又发过新消息，重启恢复会把**后续所有已闭合回合**一并删除。官方 DSH 在 finally 里同时写 `'step/end'` 与 `'turn/end'`（已核实），我们只写了一半，且修复策略用"截断"代替 DSH 的"修剪 torn 前缀 + 追加 closers"。

## 交叉验证矩阵（合并去重后 32 项，主代理已逐项读码核实）

| # | 最终定级 | 发现 | 证据（已核实） | 命中工具 | 处置 |
|---|---|---|---|---|---|
| 1 | **P1** | Turn Started 从不 append：turn 恒 1、wire 无 turn/start、恢复 blank 恒 true | `kernel-loop/src/lib.rs:312-577`（全库 grep 无生产 append；测试把"无 Turn Started"写进断言序列）；DSH agent.ts 每回合 append turn/start（外部核实） | code-review(BUG-001, P0)、ln-24(LN-001, P1) | 补 append + 回归测试 |
| 2 | **P1** | 四条终态路径不写 Step Ended → repair 截断删除已闭合错误回合及后续全部回合历史 | `kernel-loop/src/lib.rs:388-401,416-431,438-471`；`kernel-assembly/src/lib.rs:185-225`；DSH 在 finally 同时写 step/end+turn/end（外部核实） | ln-24(LN-002)、code-review(BUG-008)、architecture(ARCH-011 部分) | 终态补 Step Ended + repair 不越过 Turn Ended 截断 |
| 3 | **P1** | 重启后实时 wire seq 从 0 重起：attach_event_bus 每会话游标未按历史长度播种，新事件 seq < lastSeq 被前端水位丢弃 | `web-server/src/api.rs:222-244` | code-review(BUG-002)、architecture(ARCH-006) | 恢复时按历史 wire 长度播种游标 |
| 4 | P2 | EventBus::clone 复制共享 slots 但重建独立 next_id 计数器 → 克隆间可发重复监听器 id，Disposer 误删（潜伏：当前仅一个监听器注册在原件上） | `kernel-contracts/src/bus.rs:64-73`；`kernel-assembly/src/lib.rs:92,150` 每会话克隆一次 | code-review(BUG-003)、architecture(ARCH-001) | next_id 改 Arc\<AtomicU64\> 共享 |
| 5 | P2 | 5 处终态 Turn Ended 路径 `let _ = persist` 静默吞持久化失败 → 内存/磁盘分叉，叠加 kill-9 整回合被修剪 | `kernel-loop/src/lib.rs:350,399,427,447,465` | architecture(ARCH-003)、code-review(BUG-006) | 至少 tracing::error + 传播 |
| 6 | P2 | 边界守卫盲区：layer_of 无 web-server 条目（其 5 个内核依赖不受约束）、行级字符串解析可被注释行绕过、未覆盖"全部 workspace 成员"断言 | `kernel-assembly/tests/crate_boundaries.rs:16-25,29-42,75` | 三方一致（ARCH-004/ARCH-002/LN-003） | web-server 入层表 + 改用 cargo metadata |
| 7 | P2 | 持久化 timestamp/seq 列 write-only：load_events 只回 event_json，恢复时全部重造 Utc::now()；rewrite_events 全量重盖时间戳 → 唯一事实源丢失时间线保真 | `kernel-storage/src/lib.rs:200-219,253-284`；`kernel-assembly/src/lib.rs:141-147` | 三方一致（ARCH-002/QUAL-005/LN-005） | seq/timestamp 入事件 JSON 载荷，rewrite 保留原时间戳 |
| 8 | P2 | resolve_thinking 生产路径全死：loop 硬编码 thinking/reasoning_effort=None，build_request 传 (None,None) 且 `if let Ok(Some)` 吞错误——M4 的"镜像上 wire"只有单测活着 | `kernel-loop/src/lib.rs:366-378`；`kernel-llm/src/openai.rs:323-364` | code-review(ARCH-006/BUG-009/QUAL-011) | 请求透传 effort/thinking/purpose + adapter 设置接线 |
| 9 | P2 | LlmPort 契约写"错误以 Err 结束流"，适配器实际全部 finish 呈现（注释自证"否则 loop torn 分支把 code 覆盖成 LLM_STREAM"）；torn 分支硬编码 LLM_STREAM + request_id None，丢结构化事实 | `kernel-contracts/src/llm.rs:345-348`；`kernel-llm/src/openai.rs:542-548`；`kernel-loop/src/lib.rs:388-401` | ln-24(LN-004) | 契约文本改为 finish 呈现；torn 分支经 to_failure() 透传 |
| 10 | P2 | /api/respond 与 /api/session.export 无任何 Host/Origin 栅栏（双栅栏只装 handle_rpc 与 WS 升级），DNS-rebinding 可下载会话日志 ZIP | `web-server/src/lib.rs:80,85,165,196-220,304-354` | architecture(ARCH-014)、code-review(SEC-001) | 栅栏抽 middleware 覆盖全部 API 面 |
| 11 | P2 | Origin 校验丢端口：extract_url_host 只回 hostname，跨端口 localhost Origin 可过闸（比 DSH WHATWG host 语义松） | `web-server/src/trust.rs:133-145` | code-review(SEC-002) | host:port 全量比对 |
| 12 | P2 | host.openPath 走 `cmd /C start "" <path>`，path 含 & / | 即命令注入（loopback-pin 缓解；该方法在特权表内） | `web-server/src/rpc_m3.rs:79-97` | code-review(SEC-003) | 拒绝元字符或改用 explorer.exe 直传 |
| 13 | P2 | session.cancel 与 prompt spawn 竞态：abort 在 run_turn 安装信号前被静默丢弃，回合照跑 | `web-server/src/api.rs:741-750,861-872`；`kernel-loop/src/lib.rs:324-327` | code-review(BUG-004, P1→P2) | 信号安装提前或 pending-abort 标记 |
| 14 | P2 | 流中 select_biased 优先流分支，持续 Ready 的流饿死 abort 分支（取消只在停顿/EOF 生效） | `kernel-llm/src/openai.rs:595-607` | code-review(BUG-005, P1→P2) | 实时流下通常有 Pending 窗口，改为公平/偏 abort |
| 15 | P2 | 重复 session.create（客户端可控 sessionId）静默替换活代理：内存换新、磁盘不变、新旧日志 seq 撞车 | `web-server/src/api.rs:410-435`；`kernel-session/src/lib.rs:166-170` | code-review(BUG-007)、architecture(ARCH-004) | create 检测已存在（live 表或持久化） |
| 16 | P2 | settings/credentials/workspaces/goals/projections 纯内存，重启静默丢失（凭据回退 keyless 可恢复） | `web-server/src/api.rs:150-158` | code-review(IMP-002) | 落盘或明确降级通知 |
| 17 | P2 | LLM HTTP 仅 connect_timeout 15s，无读/idle 超时：挂死流永久占用会话 running | `kernel-llm/src/openai.rs:91-92` | code-review(IMP-003) | 加 read/idle timeout |
| 18 | P3 | LlmError::structured 硬编码 retryable:false（429/SERVER 也标不可重试）——但全库零消费方，现为惰性字段 | `kernel-contracts/src/error.rs:130-145` | code-review(QUAL-002, P2→P3) | 接线或删除字段 |
| 19 | P3 | create_session 先入内存后落盘：persist 失败留内存幽灵会话 | `kernel-assembly/src/lib.rs:88-99` | architecture(ARCH-004) | 失败回滚 store |
| 20 | P3 | session.fork 手工逐条 append+persist 绕开 loop 纪律，中途失败无补偿（孤儿半会话） | `web-server/src/api.rs:661-675` | architecture(ARCH-005) | 失败清理已建会话 |
| 21 | P3 | fork 越界回退文档承诺（台账）未实现，返回 fork-unavailable | `web-server/src/api.rs:636-648` | code-review(BUG-010) | 实现回退或改文档 |
| 22 | P3 | kernel-supervisor 已实现未装配：assembly 声明依赖从不使用、PluginRuntimePort 恒 Unavailable；web-server 声明未使用的 kernel-session 依赖 | `kernel-assembly/src/lib.rs:79`；`web-server/Cargo.toml` | architecture(ARCH-008)、ln-24(LN-006) | 接线或移除死依赖（M5 排期） |
| 23 | P3 | SSE line_buf 无上限：恶意/异常端点可致无界内存 | `kernel-llm/src/openai.rs:552,621` | code-review(SEC-006) | 行长上限 |
| 24 | P3 | 每工具调用重新编译 jsonschema validator | `kernel-tools/src/lib.rs:66-75` | code-review(PERF-003) | 注册期编译缓存 |
| 25 | P3 | 每事件（含每流式 chunk）单事务+fsync：长流磁盘压力（M1 显式拍板，logged-means-persisted 的代价） | `kernel-loop/src/lib.rs:299-306` | architecture(ARCH-017)、code-review(PERF-001)、ln-24（拒收） | 保持现状；演进每 step 批量 |
| 26 | P3 | 同步 sqlite + std Mutex 跨 async（文档已诚实记录取舍） | `kernel-storage/src/lib.rs:100-104` | architecture(ARCH-013)、code-review(PERF-002) | 多会话并发前改 spawn_blocking |
| 27 | P3 | rpc_m3 内 futures::executor::block_on 嵌套 executor | `web-server/src/rpc_m3.rs:263-278` | code-review(QUAL-004) | 改 async 链 |
| 28 | P3 | session.list updatedAt 硬编码 1970（list_sessions 端口只回 id 的粒度问题） | `web-server/src/api.rs:397`；`kernel-storage/src/lib.rs:223-236` | architecture(ARCH-016)、code-review(QUAL-007) | 端口回 (id, updated_at) |
| 29 | P3 | EventBus::emit 吞监听器 panic 且零日志；翻译器 panic 会让实时下行静默死亡 | `kernel-contracts/src/bus.rs:54-61` | architecture(ARCH-007) | tracing::error 记录 |
| 30 | P3 | Session::append 先 fetch_add 后加锁 push：并发 append 时日志向量序≠seq 序 | `kernel-session/src/lib.rs:103-109` | code-review(BUG-011) | 锁内分配 seq |
| 31 | P3 | headless 复刻 loop 的 append+persist 序列，配对算法三处并存已现语义漂移 | `headless/src/main.rs:148-177,232-261`；`kernel-assembly/src/lib.rs:185-225` | architecture(ARCH-010)、code-review(QUAL-003) | 收敛到 Runtime 单一实现 |
| 32 | P3 | 其余 P3 小项（运行时全 pub 字段二次装配、SSE 行长、projection 快照全局、BlockAssembler 未知块降级 Text、test hooks 环境变量门、http base_url 明文、WS Lagged 静默丢、export 整 ZIP 进内存、僵尸进程、死字段等） | 见三份报告 | code-review 为主 | 随轮次清理 |
| 33 | P2 | host.listDirectory / host.createDirectory 注释自称"特权"但不在 PRIVILEGED_METHODS 表（15 项）→ 不走 loopback-pin，LAN trusted-host 配置下暴露目录枚举/创建面 | `web-server/src/api.rs:1277,1370`；`web-server/src/trust.rs:6-20` | **Grok(GROK-S-08) 独中**，三 SKILL 均漏 | 纳入特权表 |

## 定级调整记录（交叉验证结论）

| 原报 | 调整 | 理由 |
|---|---|---|
| BUG-001 P0（code-review） | **P1** | 无数据丢失/安全后果，是系统性事件语义偏差（turn 恒 1、blank 误判）；ln-24 同条报 P1 |
| BUG-004 P1（code-review） | **P2** | 竞态窗口仅 spawn→信号安装之间，后果=回合照跑可恢复；与 BUG-005 叠加才放大 |
| BUG-005 P1（code-review） | **P2** | 真实网络流 chunk 间必有 Pending 窗口，abort 延迟≈一个 chunk 间隔；饿死需持续 Ready 流 |
| QUAL-002 P2（code-review） | **P3** | `.retryable` 全库零消费方，惰性字段无实际影响 |
| ARCH-002（三方 P2/P3 分歧） | **P2** | "事件日志=唯一事实源"是产品核心主张，时间线保真丢失直接伤及审计/导出 |
| ARCH-003 吞 persist 失败 | 维持 P2 | 静默破坏 logged-means-persisted 不变量，且 kill-9 下会被修剪放大 |

## 修复顺序建议（最小安全步进）

1. **P1 批（先修，互相关联）**：① run_turn 开头补 `Turn Started` append（同步改 loop/assembly 两个把"无 Turn Started"写进断言的测试）；② 四条终态路径在 Turn Ended 前补 Step Ended；③ repair_interrupted_turn 改为"修剪 torn 前缀 + 追加 closers"且不越过 Turn Ended 截断（对齐 DSH index.ts 恢复语义）；④ attach_event_bus 游标按历史 wire 长度播种。每条配套回归测试：turn 编号递增、取消→新回合→restore、恢复后实时 seq 续接。
2. **P2 批**：persist 失败传播（5 处）→ 总线 clone id 共享 → 栅栏 middleware（respond/export）→ 边界守卫补 web-server + cargo metadata → resolve_thinking 接线 → LlmPort 契约文本 + torn 分支 to_failure → Origin 端口语义 → openPath 元字符拒绝 → 重复 create 检测 → HTTP 读超时。
3. **P3 批**：随 M5 轮次清理（supervisor 装配、时间戳保真、fork 补偿等）。

## 附注

- 三份审查各自独立完成，未共享发现；交叉验证中主代理通读了 kernel-loop 全部 1077 行、assembly 全部 309 行、bus/边界守卫全文及 web-server/storage/llm 关键区段，全部 P0/P1/P2 发现均经源码确认，无一被驳回。
- 官方对照：DSH `agent.ts` 回合开头 append `turn/start`、流错误/取消在 finally 同时写 `step/end`+`turn/end`（github.com 核实）——P1 两条的外部证据成立。
- 现有门禁（91 测试全过、clippy 零警告、gate25、conformance 17/17）不覆盖 P1 两条序列（"闭合错误回合+后续回合"恢复、turn 编号递增），故未拦截；修复后应把这两条序列纳入 conformance 镜像。

---

# 附轮：第四方审查者 Grok（2026-08-18 同日）

用户安排 Grok 作为第四位独立审查者，重点看**越界**。经后台 API 派工（ZCode 自定义 provider `grok-4.6` @ apikey.fun 中转），分两组内联全部 kernel/ 源码（core 组 134KB / shell 组 272KB）流式完成。

## 通道与"思考模拟"之谜（排障结论）

- **ZCode UI 侧**：Grok 模型条目没有 `reasoning` 声明块（对比 GLM-5.3 有 variants/defaultVariant），ZCode 无从显示思考档位。
- **API 侧实测**：中转 catalog 声明 `supportsReasoningEffort=true`（low/medium/high，默认 high）。**不传参数时思考默认开启**（响应恒带 `reasoning_content`）；显式传 `reasoning_effort:"high"` 或 `thinking:{type:"enabled"}` 均报 `upstream_error`；**`reasoning_effort:"low"` 可用**。
- **超时规律**：默认档（high）在 78k token 载荷下静默推理 >5 分钟 → 中转空闲超时断流（流式停滞、非流式 fetch failed）；`low` 档流式 100 秒内完成。大载荷派工务必 `--effort=low` + 流式。
- **agent 人格坑**：中转注入的 system prompt 把模型塑造成带工具编码 agent——不加约束时它输出 `web_search` 工具调用后自停等待；提示词开头加"你没有任何工具，禁止工具调用语法"硬约束后正常出稿。

## Grok 报告结论

| 组 | 范围 | 发现 | 疑点 | Verdict |
|---|---|---|---|---|
| core | 8 内核 crate + README + Cargo + 边界守卫测试 | 10 | 4 | FAIL |
| shell | kernel-llm + web-server + headless | 12 | 4 | FAIL |

报告原文：`docs/review-dsh-rust-core-2026-08-18/grok-core-review.md` / `grok-shell-review.md`（含 reasoning_content 思考过程）。

## 越界结论交叉对照（Grok ↔ 三 SKILL ↔ 主代理核实）

| Grok | 主题 | 三 SKILL 对应 | 主代理核实 |
|---|---|---|---|
| GROK-C-01/02 (P1/P2) | 边界守卫漏 web-server + 解析可绕过 + 断言过弱 | ARCH-004/LN-003/ARCH-002 | ✓ 已核实 |
| GROK-C-04 (P0) | 错误路径 let _ = 吞 persist，拆掉 logged-means-persisted | ARCH-003/BUG-006 | ✓ 已核实（本报告定 P2，Grok 更严） |
| GROK-C-05 (P0) | 内存 seq/timestamp 与磁盘重写分叉，恢复全伪造 | ARCH-002/QUAL-005/LN-005 | ✓ 已核实 |
| GROK-C-06 (P1) | Err/Finish 双轨 + BlockAssembler 缺省 Finish=Stop | LN-004 | ✓ 已核实（新增"缺省 Stop"角度） |
| GROK-C-07 (P1) | Runtime 全 pub 可 poke + supervisor 已实现未装配 | ARCH-005/ARCH-008/LN-006 | ✓ 已核实 |
| GROK-C-09 (P2) | EventBus 吞观察者 panic（fail-loud 反面） | ARCH-007 | ✓ 已核实 |
| GROK-C-10 (P3) | resolve_model 默认体 unwrap_or_default 吞错 | QUAL-006 | ✓ 已核实 |
| GROK-C-03 (P1) | contracts 层揽了 EventBus/AbortSignal/默认体等业务实现 | ARCH-009 | ✓（Grok 定级更严，本报告 P3） |
| GROK-C-08 (P2) | loop 写死 MAX_STEPS 错误码等策略；工具 JSON 解析失败静默变 Null | BUG-014 相关 | ✓ 新角度 |
| GROK-S-01/02/04 (P1/P1/P2) | 壳层 poke Runtime 字段；web-server 越层直依赖 loop/llm；fork 在壳层手写持久化序列 | ARCH-005/LN-003 | ✓ 已核实 |
| GROK-S-03 (P1) | 壳层双实现翻译游标/wire seq（实时 vs history 批译） | ARCH-006/BUG-002 | ✓ 已核实 |
| GROK-S-05/06 (P2/P2) | LlmPort 契约"Err 结束流"vs 全程 Finish 呈现；thinking/reasoning_effort 只有单测活着 + expect 会 panic | LN-004/ARCH-006/BUG-009/QUAL-001 | ✓ 已核实 |
| GROK-S-07 (P0) | /api/respond 与 /api/session.export 无任何栅栏 | ARCH-014/SEC-001 | ✓ 已核实（本报告 P2，Grok P0） |
| GROK-S-09 (P0) | host.openPath cmd /C 命令注入面 | SEC-003 | ✓ 已核实 |
| GROK-S-10 (P1) | SSE line_buf 无界 + 仅 connect_timeout 无读超时 | SEC-006/IMP-003 | ✓ 已核实 |
| **GROK-S-08 (P1)** | **新增**：host.listDirectory / host.createDirectory 注释自称"特权"但不在 PRIVILEGED_METHODS 表（15 项），不走 loopback-pin | 三 SKILL 均漏 | ✓ 已核实（api.rs:1277,1370 vs trust.rs:6-20）→ 记为**新 P2** |
| GROK-S-11 (P2) | run_turn 吞结果 / block_on 嵌套 / list_sessions 失败变空列表 | QUAL-004/ARCH-003 | ✓ 已核实 |
| GROK-S-12 (P3) | headless 手写事件落盘模拟 kill-9（与内核 repair 双实现） | ARCH-010 | ✓ 已核实 |
| core D-1 疑点 | "run_turn 从不 append Turn Started，是死策略还是漏写事件" | 本报告 P1-1（BUG-001/LN-001） | Grok 独立嗅到同一异常，未定级 |
| shell 疑点 | goal 状态机在 web-server（wire 在壳、语义在插件） | M3 内核越界审查结论 | 一致 |

## 附轮结论

1. **越界主结论四票归一**：本报告 3 条 P1 中的两条（Turn Started 缺失、Step 不闭合→恢复删历史）Grok 以"疑点+契约双轨"形式独立触及；守卫盲区、persist 吞错、seq/时间戳重写、Runtime 可 poke、supervisor 悬空、栅栏缺口全部被 Grok 独立复现。**唯一实质分歧是定级尺度**：Grok 把 persist 吞错、seq 重写、respond/export 无栅栏、openPath 注入定到 P0，本报告交叉验证维持 P2（判据：无即发数据丢失、默认配置 loopback 已缓解），但四票压力提示这些项修复优先级应上浮到 P1 批尾部。
2. **Grok 独中 1 条新 P2**：listDirectory/createDirectory 缺 loopback-pin（注释与特权表不一致）——已核实并并入主矩阵（新增 #33）。
3. **通道知识**：grok-4.6 经此中转派工的正解 = 流式 + `reasoning_effort:"low"` + 无工具硬约束；思考输出在 `reasoning_content` 字段随正文返回，不需要任何额外参数。ZCode UI 想显示思考需在模型条目补 `reasoning` 声明块。
