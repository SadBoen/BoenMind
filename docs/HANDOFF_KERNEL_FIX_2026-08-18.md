# HANDOFF：交叉审查四批修复完成交接（2026-08-18）

> 状态：**四批修复全部完成并已推送**。本交接承接 `docs/HANDOFF_M4_FINISH_2026-08-18.md`
> （其 §4 遗留表 A/B 挂账已随审查轮勾销，C/E 已在本轮落地，D 后置）。下一轮 =
> **插件/M5 主线开题**（supervisor 蓝绿 + IPC + team 插件进程），或 P2-D session-query 索引。
> 审查报告见 `docs/REVIEW_DSH_RUST_CORE_2026-08-18.md`（三 SKILL + Grok 四票交叉，32+1 项矩阵）。

---

## 1. 一句话交接

**四票交叉审查（code-architecture / code-review / ln-24 / Grok）暴露的核心缺陷全部修复**：
P1×3（事件瀑布 + 恢复语义 + 实时 seq）、P2×8（安全栅栏/契约/守卫）、核心残留×5
（时间戳保真/thinking 接线/cancel 竞态/abort 响应/fork 清理）、P2 收尾×2（settings 文件层/
未知块过滤）。当前基线：cargo test --workspace 100 全过、clippy 零警告、verify-gate1 ALL PASS。

---

## 2. 本轮修复清单（commit 见 git log）

### 2.1 P1 批（b498e93）——事件瀑布三条实锤 + seq 播种
- **run_turn 补 Turn Started append**：turn = 日志 max+1（对齐 DSH 每回合 turn/start）。
  修复 turn 恒 1、wire 无 turn/start 帧、重启恢复 blank 判定恒 true（有历史会话被当空会话）。
- **四条终态路径经 close_turn 补 Step Ended** + persist 失败 fail-loud 传播（原 `let _ =` 静默吞）。
- **repair_interrupted_turn 改追加 closers 不截断**：Step Ended + `Turn Ended{Interrupted}`
  闭合孤儿回合尾部；事件日志唯一事实源完整保留（不越 Turn Ended 删历史——已闭合错误回合的
  requestId 审计事实、取消后的后续回合历史都不再丢失）。`TurnEndReason::Interrupted` 从
  "loop 从不发出"变体变为恢复期孤儿回合闭合语义。
- **attach_event_bus 按 live 表历史 wire 长度播种 seq 游标**：启动恢复先于 attach；
  修复重启后实时 seq 归零撞历史基线、被前端水位丢弃的问题。
- 回归测试 5 条：turn 递增 / 修复保历史 / 取消→新回合→restore 集成 / 错误回合+后续恢复 / seq 播种。

### 2.2 P2 批（c86cce7）——安全与契约加固 8 项
- EventBus clone 共享 next_id 计数器（防 Disposer 误删，BUG-003）。
- /api/respond 与 /api/session.export 补栅栏 A（DNS-rebinding 审批面/日志 ZIP 外流）。
- Origin 校验带端口比对（跨端口 localhost 不得过闸）。
- #33（Grok 独中）：host.listDirectory/createDirectory 入 PRIVILEGED_METHODS loopback-pin。
- host.openPath 拒绝 cmd 元字符与控制字符（SEC-003 注入面）。
- 重复 session.create 拒绝（session-exists，BUG-007）。
- LLM HTTP 加 300s 读超时（挂死流不再永久占用会话）。
- 边界守卫重写：toml 解析 + web-server 入层表 + 未知 crate 硬失败（原行级字符串解析可绕过）。

### 2.3 核心残留批（e8abd0a）——台账核心项清零 5 项
- **时间戳/seq 保真**：`SessionPersistPort::load_events` 改回返回 `SessionRecord`
  （含磁盘 seq+timestamp）；restore 直接沿用落盘时间不重造；rewrite_events 保留原时间戳
  只给新增 closers 打新时间。回归断言：恢复后首条事件时间 ≤ 写入时刻。
- **resolve_thinking 生产接线**：build_request 改 Result（消除 translate 的 expect panic 面）；
  stream_inner 流内完整构造（translate/resolve_thinking 失败以结构化 finish 显错，不 panic
  不静默吞）；DeepSeek 系模型装配声明 reasoning 档位 → `thinking:{type:enabled}` +
  `reasoning_effort:high` 真正上 wire（此前只有单测活着）。
- **cancel 竞态收口**：ReactLoopAgent 加 pending-cancel 原子位，信号安装前的 abort 请求不丢
  （回归测试验证模型收到 aborted signal）。
- **abort 流中响应性**：SSE 循环顶部 is_aborted 同步预检，持续 Ready 的流不再饿死取消。
- **fork 失败清理孤儿半会话**（内存 + 磁盘 delete_session）。

### 2.4 P2 收尾批（7b12a7c）——M4 交接 P2 队列 C/E
- **C settings/credentials 文件层**：`~/.boenmind/settings.json` 原子写盘（tmp+rename，
  Unix 0600）；写面成功后持久化（settings value/revision + credentials 全量单文件）；
  启动加载恢复并重放 provider 覆盖（baseURL 覆盖回适配器、`*_API_KEY` 凭据回填 adapter key）；
  损坏文件 fail-soft 回退空态。重启不再静默丢配置/凭据。
- **E BlockAssembler 未知块类型**：不再静默 flatten 成 Text，协议外类型不产出消息块
  （不污染投影/模型输入）；assemble 改 Option，blocks() 过滤。
- 回归测试 2 条：文件层持久化重载 / 未知块过滤。

---

## 3. 验证矩阵

| 项 | 结果 |
|---|---|
| cargo test --workspace | **100 全过**（91 → 100，+9 回归；累计覆盖 turn 递增/修复保历史/seq 播种/时间戳保真/pending-cancel/settings 重载/未知块过滤/Origin 端口/bus 克隆隔离） |
| clippy --workspace --all-targets -D warnings | 零警告 |
| verify-gate1.sh | ALL PASS（roundtrip/abort@1/abort@2/resume/verify-tail/隔离） |
| conformance 17/17 / gate25 / m3-r3 / hot-replace | **未重跑**（上轮基线；本轮动了 openai.rs wire 与 web-server 行为面，**下轮开工前先补跑**——跑法见 §5） |

---

## 4. 遗留/挂账（下轮开题候选）

| # | 项 | 说明 |
|---|---|---|
| D | session-query 全套（SESSION_QUERY_* / cursor / SQLite 索引） | M4 P2 队列剩余项，纯性能/可扩展，不阻塞任何事 |
| — | LlmError.retryable 死字段 | 全库零消费方（429/SERVER 结构化错误标记从未被读）；随未来重试接线时删 |
| — | supervisor 完整化 | **M5 主线**：蓝绿替换 + 崩溃计数 + IPC 协议版本化+鉴权（架构 §五·7 升格项） |
| — | team 插件进程 | = 真实 subagent 执行（Rust 独立进程 + supervisor + IPC，与热升级蓝绿同批工程）；wire 契约在 web-server、执行在插件、内核不动（M3 定调） |
| — | 热升级过渡态验收 | supervisor 蓝绿 + 健康检查 + WS 重连兜底；数据安全已由状态外置 + kill-9 恢复消除 |

---

## 5. 环境与纪律（沿用）

- **每轮先杀 web-server**：`taskkill //F //IM web-server.exe`（残留进程占端口），收尾再杀一次。
- **验证三件套**：`cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings`
  + `bash kernel/scripts/verify-gate1.sh`。
- **conformance/gate25/m3-r3/hot-replace 补跑**：conformance 走 `.tmp/dsh-trace-recorder.mjs`
  Node 轨迹对比（固定端口 3081）；gate25/m3-r3 走 `.tmp/gate25-verify.mjs`（端口 3079，
  全新 db + BM_TEST_HOOKS=1）；hot-replace 走 `kernel/scripts/hot-replace-verify.mjs`（端口 3082）。
  跑前起 release web-server（debug exe 2GB 超 PE 限制，见记忆 build-debug-exe-2gb-pitfall）。
- **文件地图**：
  - 事件瀑布/修复：`kernel/kernel-loop/src/lib.rs`（run_turn/close_turn）、
    `kernel/kernel-assembly/src/lib.rs`（repair_interrupted_turn/restore_session）
  - 存储契约：`kernel/kernel-contracts/src/ports.rs`（load_events→SessionRecord）、
    `kernel/kernel-storage/src/lib.rs`
  - 适配器/thinking：`kernel/kernel-llm/src/openai.rs`（stream_inner/build_request_with/resolve_thinking）
  - 壳层/安全：`kernel/web-server/src/trust.rs`（栅栏 A/B/特权表）、`lib.rs`（respond/export 栅栏）、
    `api.rs`（seq 播种/settings 文件层/重复 create/fork 清理）
  - 边界守卫：`kernel/kernel-assembly/tests/crate_boundaries.rs`
  - 审查报告：`docs/REVIEW_DSH_RUST_CORE_2026-08-18.md` + `docs/review-dsh-rust-core-2026-08-18/`
    （code-architecture-report / code-review-QUESTIONS / ln24-audit / grok-core·shell-review）
