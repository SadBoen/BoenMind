# M9 实现规格——阶段二第一批:记忆抽屉授权 × 模型真流式 × worker 自主环 v0

> 状态:**冻结(2026-08-30)**。用户拍板(PENDING D-M8-1):第一批 = ③自主环
> 初级版 + ④记忆抽屉框架 + ⑤真流式;⑥远程 MCP、S4 draining、lease 吞吐留
> 后续批次。延续提速方案:三轨合批交付,共享全量回归。
> 前置结算:D-M5-2(memory:user 显式授权执行面)本批闭合;M8-review §6-4
> 六项遗留中「worker 自主环」「模型流式」本批了结 v0。

## 零、总纪律

- 合同只增不破:所有变更 Minor 追加(新事件类型 / 新 wire 方法 / 新配置字段),
  `validate.py` 全绿;既有 golden trace 与测试在默认配置下**零变化**。
- 全部新面可由 MockConnector / 内存态确定性测试;实网调用全批 ≤ 2 次
  (流式连通性验证为主)。
- 单写者纪律:增量(delta)与自主环的每一步都以事实事件经核心循环落账,
  绝不旁路 Event Log。

## 一、S1 记忆抽屉授权框架(M5.7 欠账的执行面;闭合 D-M5-2)

**现状**:memory.rs 的 scope 形态已含四抽屉(memory:user / agent:<id> /
task:<id> / app:<name>),形态校验在 Provider 侧;但 **Broker 不校验
「谁可写哪个抽屉」**——任何持 memory.write 授权的主体都可写 user 抽屉。

**本批做**(enforcement 落在 Broker 决策点,不改 CapabilityProvider 签名):

1. `Broker::decide` 新增 memory.* 专用裁决步(在 builtin 直通之前):
   - principal = `surface:user`(或 user 域 Surface 族)→ 四抽屉按既有
     manifest/审批流放行;
   - principal = `agent:<id>` → 只可写 `memory:agent:<同一 id>`;写其他任何
     抽屉(含 memory:user)须持**显式以该 scope 签发的 Grant**,否则升级审批
     (不静默拒绝,产出可审批事实);
   - principal = task 族(`coord:`/`worker:`)→ 只可写 `memory:task:<同一
     task_id>`,其余同上须专项 Grant;
   - read(memory.search)放宽:任意已授权主体可检索 `memory:user` 与
     自己的抽屉;他人抽屉须 Grant(检索是读,不产生内容污染)。
2. Agent 创建物化默认抽屉 Grant:`memory:agent:<id>` 的 write/search/delete
   永续 Grant(与 model.invoke 的 agent 创建授权同机制,butler.model_grant_for)。
3. **验收测试**(m9_memory_tests):
   - t130 agent 写自己抽屉 ✓(无审批);t131 agent 写 user 抽屉 → 升级审批,
     拒绝后无写入;t132 user 授专项 Grant 后 agent 可写 user 抽屉;
   - t133 跨 agent 抽屉:agent:B 写 agent:A 抽屉 → 升级审批;
   - t134 worker 写 task 抽屉 ✓、写 user 抽屉 → 升级;
   - t135 search:agent 检索 user 抽屉 ✓、检索他人抽屉 → 升级;
   - t136 既有 t88(裸 memory:user 形态拒绝非法形态)不回归。

## 二、S2 模型真流式(增量事件贯穿)

**现状**:OpenAiConnector 非流式(WireRequest.stream 恒 false),回答整段
返回;前端「打字机」缺失即源于此。

**本批做**:

1. 合同追加(Minor):`EventType::ModelContentDelta`(事件名
   `model.content.delta`),payload = `{operation_id, index, delta}`;
   `validate_event_shape` 同步;events 合同文件 + validate.py 绿。
2. 端口追加(默认实现,零破坏):`ModelConnector::invoke_stream(..., on_delta)`
   默认方法 = 调 `invoke` 后把整段 content 作为单个 delta 回调(旧连接器
   不改一行即兼容);OpenAiConnector 覆写:body `stream:true` + SSE 行解析
   (`data: {...}` / `[DONE]`),逐块回调增量,结束聚合为 Completed
   (finish_reason 取最后一块;usage 缺省零值——流式 usage 网关差异如实留档)。
   错误纪律不变:错误分支零响应体/零内容明文。
3. Runtime:`RuntimeConfig` 追加 `model_streaming: bool`(默认 **false**,
   既有测试与 golden trace 零变化;server 装配按环境变量 BOEN_MODEL_STREAM=1
   开启)。开启时 spawn_turn 走 invoke_stream,delta 经命令通道回核心循环,
   逐条 emit `model.content.delta`(index 单调,0 起);completed 事件保持
   现状(全量 content + content_truncated 口径不变)。
4. Surface:web UI 在 events.poll 轮询中对在途 operation 追加渲染 delta
   (XSS 纪律:textContent);CLI `events.poll` 已可原样看到 delta 事件,不改。
5. **验收测试**(m9_stream_tests):
   - t140 MockConnector 流式默认路径(默认 flag 关 → 无 delta 事件,回归零变化);
   - t141 flag 开 → delta 事件序列 index 连续、聚合 == completed content;
   - t142 流式中途取消(cancel)→ 已发 delta 保留,终态 cancelled,无 completed;
   - t143 SSE 解析单测:多块 + [DONE] + 损坏行容错(损坏块跳过不致命);
   - t144 实网连通(#[ignore] + BOEN_LIVE,1 次调用):网关 stream=true
     分块可达、聚合非空、usage 字段如实记录。

## 三、S3 worker 自主环 v0(编排面增量)

**现状**:worker(member agent)的模型回合由 Butler/会话逐步编排
(M7 打通「worker 可调真实能力」;自主循环一直属编排面增量)。

**本批做**(v0 = 「一个人能自己干完整件事」,可测试优先):

1. 合同追加(Minor):wire 方法 `task.autorun`
   `{task_id, max_turns?}` → `{session_id, turns_used, final_state}`;
   事件 `TaskAutorunStateChanged`(started / turn_completed / finished,
   payload 含 task_id、turn 序号、出口原因)。
2. 语义:校验 task Running → 以 worker(member)agent 开(或复用)专属
   会话 → 循环至多 max_turns(默认 6,预算内):
   - 每轮向会话发系统指令(任务目标 + 已有进展提示 +「完成则调用
     task.report 能力,未完成则继续」),经既有 spawn_turn 全链路
     (授权/审计/预算 max_tool_calls 硬限自动生效);
   - 出口三选一:**完成**(turn 调用了 task.report → Task 按 M5 报告路径
     收束)/ **预算耗尽**(既有 BudgetExceeded → blocked,循环停)/
     **卡住**(连续 2 轮模型输出完全相同 → blocked,reason=stalled);
   - task.pause/cancel 在轮间即时生效;每轮发 TaskAutorunStateChanged。
3. v0 明确不做:多 worker 并行、自动重试策略、跨 task 委派——后续按使用反馈。
4. **验收测试**(m9_autorun_tests,MockConnector 脚本化):
   - t150 三轮后 task.report → completed,turns_used=3,事件序列完整;
   - t151 第 2 轮触发预算硬限 → blocked(budget_exhausted),autorun 收口;
   - t152 连续两轮同输出 → blocked(stalled);
   - t153 autorun 中 task.pause → 轮间退出,任务 paused,无悬挂回合;
   - t154 max_turns 用尽仍未报告 → blocked(reason=max_turns),如实出口。

## 四、门禁与收官

- 全量:validate.py 全绿;cargo fmt --check;clippy --all-targets 零警告;
  cargo test --workspace 全绿(既有 235 + 新增 ~15);
- 实测:BOEN_MODEL_STREAM=1 起服,网页真流式发一句(全批实网调用 ≤ 2,
  含 t144);CLI 场景存档(scenarios/S11 自主环 + S12 流式);
- 文档:perf 若有新计量追加 ID(P-12 回合 delta 开销,可选);M9-review 按
  §19 回看(七问 + 条件裁决 + 遗留移交);tag `m9-stage2-batch1`。

## 五、风险与预案

- 网关流式行为差异(SSE 格式/usage 缺失):解析器容错(损坏块跳过),
  usage 零值兜底并留档;若网关完全不支持 stream,如实降级为非流式 + 留档
  (flag 关闭即回旧路径,不阻塞批次)。
- 自主环与既有预算/暂停语义的交互:全部复用既有硬限与状态机边,不新增旁路。
