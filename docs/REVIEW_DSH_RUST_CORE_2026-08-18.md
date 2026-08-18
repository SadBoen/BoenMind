# DSH 核心 Rust 版（kernel/ 微内核）三 SKILL 交叉审查报告

日期：2026-08-18
方法：三个审查 SKILL 各派一个独立子代理并行审查（互不通信），主代理对全部 P0/P1/P2 发现逐一读源码核实，并对照官方 DSH 源码做外部证据验证，最后合并、去重、重定级。

- **code-architecture**（架构审查模式）→ 17 条关切，verdict：架构健康，无 P0/P1
- **code-review**（深度代码审查 Phase 1）→ 49 条问题（1 P0 / 3 P1 / 20 P2 / 25 P3）
- **ln-24-architecture-auditor**（架构适配度审计，清单 44/44 完成）→ verdict：**FAIL**，2 P1 + 4 P2/P3

三份独立报告：`docs/review-dsh-rust-core-2026-08-18/`（code-architecture-report.md / code-review-QUESTIONS.md / ln24-audit.md）

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
