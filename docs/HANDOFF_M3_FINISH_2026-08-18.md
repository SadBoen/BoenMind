# HANDOFF：M3 收尾——下一轮目标=把 M3 干到完成（门禁 3 + RPC 52/52）（2026-08-18）

> 状态：**M3 已完成起步+轮 2**（28a1bbe / 1d9cb84）。真 provider 三通道、session.search/fork、
> host 流基线帧/实时广播、credentials 已落地；RPC 面 28/52。门禁 2.5 与 conformance 全绿。
> 本交接 = **下一轮开工指针**：把 M3 剩余项全部干到完成。上一指针 `docs/HANDOFF_M3_2026-08-18.md`
> （起步+轮2版，含完成项细节与坑）。

---

## 0. 一句话交接

**下一轮目标：M3 干到完成 = ① respond pending 表 + approval/question 重放 → ②
session/projection 槽 → goal.\*（6 法）→ ③ session.{attachment,updateQueue} +
settings.openDocument + host.openPath + session.export → ④ 热升级 supervisor 蓝绿 +
过渡态/门禁 3 热升级半验收 → ⑤（最大块，视工程量）subagent.\*（4 法，依赖内核子代理）。
RPC 从 28 推到 52/52；respond 从恒 not-pending 变为真 pending 表；session.export 从恒 404
变为真 ZIP。若 subagent 本轮做不完：门禁 3 的"热升级半"必须完成并验收，"专家团队半"明确挂起
并登记（依赖内核子代理设计）。**

---

## 1. 当前状态（开工前快照）

### 已完成（勿动）
- 传输面 9 面 + 双栅栏；WS mux 基线/实时事件（seq 连续）+ host 基线帧/实时广播。
- RPC 28/52：host.{describe,pickDirectory,listDirectory,createDirectory}、
  session.{list,create,history,prompt,cancel,rename,models,selectModel,search,fork}、
  workspace 全 7 法、llm.{providers,models,discoverModels}、agentPreset.list、skill.list、
  settings.{describe,update,replace,mutate}、credentials.{describe,set,unset}。
- 真 provider 三通道（`--config <toml>`，minimax/deepseek/custom）+ per-session 模型覆盖。
- 验证脚本：`docs/conformance/m3-real-provider-verify.mjs`（11/11）、
  `m3-r2-verify.mjs`（15/15）、`gate25-verify.mjs`（15/15）、`diff-traces.mjs`（17/17）。

### 剩余（本轮目标，按依赖序）
| # | 项 | 现状 | 依赖 |
|---|---|---|---|
| 1 | respond pending 表 + approval/question 重放 | `lib.rs handle_respond` 恒 not-pending | 无 |
| 2 | session/projection 槽（history tail projections + session/projection 帧） | 空壳 | 无 |
| 3 | goal.\*（create/edit/pause/resume/complete/clear） | bad-request | 2 |
| 4 | session.attachment / session.updateQueue | bad-request | 无（updateQueue 语义简化） |
| 5 | settings.openDocument / host.openPath | bad-request | 无 |
| 6 | session.export（ZIP 流式下载） | 恒 404 | 无 |
| 7 | 热升级 supervisor 蓝绿 + 过渡态验收 | supervisor 雏形已有（M1） | 无 |
| 8 | subagent.\*（list/history/prompt/interrupt） | bad-request | **内核子代理**（最大块） |

---

## 2. 逐项实现要点（契约台账 `docs/CONTRACT_LEDGER_DSH.md` 为准）

### 1) respond pending 表（台账 §1 面 7 + §4 断连恢复语义）
- `AppState` 加 pending 表：`Mutex<Vec<PendingInteraction>>`；PendingInteraction =
  `{kind: 'approval'|'question', rpc_id(稳定逻辑 id), session_id, 负载, 登记时间}`。
- 登记点：**当前内核无 approval/question 产生源**（无审批/提问工具）——pending 表先实现
  存储+路由+校验+重放机制，登记点留扩展（未来审批工具/提问工具调用
  `state.register_pending(...)` 即接入）。**验收方式**：直接注入 pending 条目（测试/管理面）
  后走 respond 全链路；mux 重开基线重放 pending 帧。
- `handle_respond` 全逻辑（逐字对齐）：
  - 415 非 JSON / 400 非 JSON 体（已有）；
  - 解析 `ClientResponse{rpcId,result}`；信封失败 → `{accepted:false,reason:'bad-response'}`；
  - rpcId 路由：approval 表先查、question 表后查；无 → `not-pending`；
  - approval 应答：`result.ok:true` 时 value 须 `{sessionId, approvalId, outcome:'allowed-once'|'rejected'}`
    且 approvalId/sessionId 与登记一致，否则 `bad-response`；ok:false → 取消语义；
  - question 应答：`{sessionId, answer:{answers:[{id, selected, custom?}]}}`，整批匹配原问题
    （数量、id 集合、selected 唯一性、multiSelect 约束、option label 集合），不匹配 →
    `bad-response`；`result.ok:false && error.code==='cancelled'` → accepted（用户取消）；
  - 命中且校验过 → `{accepted:true}` 并移除 pending、广播 `approval/resolved` 或
    `question/resolved`（outcome 'answered'）。
- **mux 重开基线**（api-proxy.mux 语义）：连接时每 attached session 一帧 subscribed（已有）→
  重放仍 pending 的 `question/requested` 与 `approval/requested`（**rpcId 原样复用**）。

### 2) session/projection 槽（台账 §3.3 + §4）
- `AppState` 加投影单元注册表：`Mutex<HashMap<String, (Value watermark_seq, Value value)>>`
  （key → (seq, value)，seq 单调，客户端 higher-seq-wins）。
- `session/projection` 帧（mux 下行）：`{sessionId, key, value, seq}`；
- `session.history` tail 页（当前无分页 = 恒 tail）带 `projections:{asOfSeq, values}`。
- 与现有 `attach_event_bus` 联动：投影变更时走 events_tx 广播（事件类型区分）。

### 3) goal.\*（台账 §2 goals 表 + 错误码）
- **无读方法**——状态只走 projection（key `'goal'`）+ history tail projections。
- `goal.create{sessionId, objective≥1, maxGoalRounds?}` → `{ref:{id, revision}}`；
  内部建 GoalRecord{id, objective, rounds, revision:1, phase:'active'|'paused'|'completed'}，
  写 projection 变更 + 事件（goal/change）。
- edit（不改 phase，revision+1）、pause/resume（翻转 armed 状态）、complete（解除+projection
  终态）、clear（留墓碑 + `{cleared:true}`）。
- 本实现是**状态表 + wire 契约**；与内核 loop 的"自动续跑"接合可后置（M3 先保证 wire 全通）。

### 4) session.attachment / session.updateQueue
- attachment：会话日志含 `attachmentId` 引用才回（当前内核无附件事件——实现按"日志引用表"
  查；无引用 → 对应错误）。**简化判据**：台账错误码 `attachment-error {reason}`。
- updateQueue：内核无 queue 语义（prompt 排队 = agent-busy 拒绝）。实现 wire 契约：
  `{accepted:true}`（空操作）+ 未知 itemId → `queue-item-not-found`。**诚实标注：queue 语义
  挂后**，本轮先 wire 对齐。

### 5) settings.openDocument / host.openPath（特权）
- settings.openDocument：`{opened:true}`（无原生文档可开——`hasDocument:false` 已声明；
  返回 opened:true 对齐 wire，前端不弹错）。
- host.openPath：`{path≥1}` → 调 OS 打开（`cmd /C start` / `explorer`，Windows）；
  打开失败 → `opened:false` 或 internal。**注意**：这是唯一真实 OS 副作用方法，验证时
  用无害路径（如临时文件）。

### 6) session.export（台账 §1 面 8 + 比对点 7）
- GET `/api/session.export?sessionId=<id>&includeDescendants=true|false`（缺省 false）；
  query 非法 → 400 `missing or invalid sessionId query parameter`；根缺失 → 404
  `session not found`。
- 响应：`content-type: application/zip` + `content-disposition: attachment;
  filename="dsh-session-<id>.zip"`（id 非 `[A-Za-z0-9_-]` 替换为 `_`）。
- ZIP 结构：根 `session.jsonl`（wire SessionEvent 逐行 JSONL）→ `subagents/<id>/...`（无子代理
  则缺省）→ `media/<attachmentId>.<ext>`（无媒体则缺省）。
- **实现**：加 `zip` crate（或手写 minimal ZIP——std 无 zip；`zip` crate 最省事）；事件日志
  转 JSONL 字符串后写 zip；全量小体积可直接内存构造后返回 bytes。

### 7) 热升级 supervisor 蓝绿（架构 §五·7 + M3 门禁 3 热升级半）
- 已有：`kernel-supervisor`（spawn/kill/restart/健康=Running）+ 状态外置（事件日志唯一事实源）
  + kill-9 恢复（M1 门禁 1）+ WS 重连（M2.5）。
- 本轮做：**蓝绿编排二进制/脚本**（不一定要进 web-server 进程内——独立编排器）：
  1. 新进程起（新版本二进制，临时端口或同端口错开启动期）→
  2. 健康检查（HTTP GET `/api/host.describe` 200 即健康；轮询带超时）→
  3. 切流（**端口接管是核心难点**：浏览器连的是固定端口。方案 A=反向代理层（nginx/hyper 前端
     代理，切 upstream）；方案 B=SO_REUSEPORT（Windows 支持度差）；方案 C=浏览器侧
     引导重连到新端口（前端 WS 重连 + 新 URL））→
  4. 旧进程排空（等流式回合自然结束：轮询旧进程 `session-status` 无 running 或超时强杀）。
- **诚实评估**：无感切流在"浏览器直连固定端口"形态下最务实的是 **C（前端重连到新端口）或
  代理层 A**。最小可行验收 = **过渡态**：起 A(3079) → 真模型流式中 kill -9 → 起 B(3079 同端口)
  恢复日志 → 前端 WS 重连 + 会话历史完整 + 可继续对话（此过渡态链路 M1/M2.5 已分别验证过，
  本轮合流成脚本）。
- 本地形态延续旧版：standalone 单二进制替换 / 壳重启子进程（验签 ed25519 后置）。

### 8) subagent.\*（最大块，依赖内核子代理——单独评估）
- 契约：list{parentSessionId} → {entries, parentAvailable}（child one-shot/continuable +
  diagnostic）；history{parentSessionId, childSessionId, mode, beforeSeq?, maxMessages?} →
  {events, hasMore, projections?}；prompt{..., mode:'continuable', content} → {messageId}；
  interrupt{...} → {accepted:true}。
- **内核缺口**：ReactLoopAgent 是单会话代理，无 parent-child 关联、无 one-shot 子代理回合、
  无 `origin:'subagent'` 会话字段。最小实现：子会话 = 普通会话（header 带 parent 关联字段或
  内存表 parent→children），child 事件日志独立；parent 侧只做"派工"（session.prompt 由
  subagent.prompt 代理到子会话 run_turn）；`agent-busy`/`subagent-*` 错误码全表实现。
- **判据**：若本轮 1-7 完成后剩余时间/复杂度可控则做最小版（one-shot + continuable 表 +
  wire 全通，parentAvailable 语义简化）；否则挂起并更新台账标注"subagent 依赖内核子代理，
  M3 遗留 → M3.5/M4 前置"。

---

## 3. 验证矩阵（每补一组按此验）

| 项 | 验证 |
|---|---|
| respond | 新验证脚本 `m3-r3-verify.mjs`：注入 pending → respond 三分支（accepted/not-pending/bad-response）+ mux 重开重放帧 |
| projection + goal | goal.create → session.history tail 的 projections 含 goal 状态 + session/projection 帧 |
| attachment/updateQueue/openDocument/openPath | curl 逐法：成功形状 + 错误码（queue-item-not-found 等） |
| session.export | curl -o 拉 ZIP → 解压验 session.jsonl 内容 + 头检查 |
| RPC 计数 | 台账 RPC 面 52/52（grep dispatch 分支数） |
| 热升级过渡态 | 脚本：起 A → 真/假流式 kill-9 → 起 B → host.describe/session.history 恢复 → 可继续 prompt |
| 回归 | gate25（**全新 db**）+ conformance 17/17 + workspace 测试 + clippy 零警告 |

---

## 4. 纪律与坑（每轮开工必读）

- **开工第一步杀残留服务**：`taskkill //F //IM web-server.exe`（用户电脑不关机，上一轮进程
  会占端口；用户已明确要求）。收尾也杀一次再清理临时 db/日志（`rm -f *.db* *_*.log`）。
- **固定端口**：gate25/m3-*/diff-traces 都写死端口（3079/3081）——保持"每轮一个干净服务 +
  固定端口"，不要换端口。
- **gate25 用全新 db**：gate25 断言 workspace.list items[0] 是自己建的 workspace——前置脚本
  先建 workspace 会致 FAIL（脚本环境假设，非回归）。
- **WS 验证竞态**：判据即关会丢尾部帧（close 竞态）→ 用固定收集窗口（400ms 静默后 resolve）；
  create 广播帧可能先于 HTTP 响应 → open 回调设 id 后回扫已收集帧（见 m3-r2-verify.mjs）。
- **锁纪律**：workspace 写方法先 drop 锁再 broadcast（workspace_snapshot 要重拿锁，持锁广播
  自死锁）。
- **serde_json 键序**：无 preserve_order，Map 字母序序列化（error < ok）——测试期望按此写。
- **fork/复制事件**：先 create_session（SessionStarted seq1）再逐条 append（seq 从 2 连续）。
- **Windows**：cargo build 前 taskkill web-server.exe（文件占用）；调试走 stderr
  （RUST_LOG=debug，重定向日志块缓冲）。
- **真 provider 验证计费**：m3-real-provider-verify.mjs 每次真实调用 minimax API——热升级
  过渡态验证可用 mock 模式（无 --config），杀进程不产生费用。

## 5. 完成后收尾
- 台账 `docs/CONTRACT_LEDGER_DSH.md` 实施进度段更新（RPC 52/52 + respond/export 勾销）。
- 更新 `docs/HANDOFF_M3_FINISH_2026-08-18.md` 状态段或写新交接（M3 完成态，指向 M4）。
- commit + push（自动推送政策）。
