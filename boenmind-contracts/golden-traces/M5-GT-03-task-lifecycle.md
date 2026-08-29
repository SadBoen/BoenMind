# M5-GT-03:Task 生命周期与长期监护(黄金轨迹)

> 目的:给实现者一条 Butler/Task/Coordinator/Watchdog 全链路的逐字节可对照路径。
> JSON 必须通过标注 schema;状态迁移必须是 core-transitions 的边(含 M5 增发的
> task 状态机与 agent.paused 边)。验证方式:回放器按本轨迹驱动 Runtime,逐条
> 比对事件序列与 payload 结构。
> 场景 A = 主链路(建 Task→单 Worker 执行→声称完成→verification 核验→completed);
> 场景 B = 监护链路(重复动作→停滞→Watchdog 触发编排重启→硬顶 blocked→用户
> resume→核验完成)。「声称完成但未生效」的 unverified 注入由测试套件承载
> (完成判定门禁,M5 规格 §5.7),不在本轨迹。
> 上游:ADR-0002 条件 2/5、ADR-0004 条件 6、M5 规格 §5.1–§5.8。

约定:`seq` = event_seq(全局单调);`→` = 迁移;`[S:…]` = 校验所依据的 schema。

---

## 场景 A:主链路——Task 创建到核验完成

### A0. Runtime 启动(前置:system.* 能力已注册;Butler 协调权 bootstrap Grant 已物化)

```json
{"event_seq": 1, "type": "runtime.started", "occurred_at": "2026-08-29T11:00:00.100Z",
 "payload": {"pid": 44210, "version": "0.1.0-m5", "started_at": "2026-08-29T11:00:00.098Z"}}
```

### A1. 用户经 Surface 建单(Butler 为 Task 授权声明的检查者,非特权通道)

```json
{"v": "0.1", "method": "task.create",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAB1",
 "idempotency_key": null,
 "params": {"title": "整理读书笔记", "goal": "把 inbox 笔记归档到 notes 并复核",
            "authorization": [{"verb": "task.collect", "klass": "safe"},
                              {"verb": "agent.spawn", "klass": "mutation"},
                              {"verb": "agent.stop", "klass": "mutation"}],
            "budget": {"max_tokens": 100000, "max_tool_calls": 50},
            "deadline": null}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAB1", "ok": true,
 "result": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2",
            "state": "created", "created_at": "2026-08-29T11:00:01.000Z"}}
```

```json
{"event_seq": 2, "type": "task.created", "occurred_at": "2026-08-29T11:00:01.000Z",
 "payload": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2",
             "title": "整理读书笔记", "created_by": "butler:system"}}
```

```text
事件 3  task.state.changed {task_id, from: "created", to: "running",
         reason_code: "task_started", task_epoch: 1}
迁移:task created→running                                          [S: core-transitions]
Task 规范状态落 L2(tasks 表 + 事件);Butler 内存仅为投影(ADR-0004)
```

### A2. Butler 创建 Coordinator,Coordinator 签发成员授权并 spawn 单 Worker

```text
[内部] Coordinator Agent 创建(Task 授权内 spawn:agent.spawn ∈ 白名单,mutation 类)
决策:三方交集物化 —— Coordinator 持 task:<id> scope Grant(父 = Butler bootstrap
      Grant,parent 哈希链可上溯,delegation_depth=0 不可再转授,ADR-0002 §11.3)
事件 4  task.member.added {task_id, agent_id: "agent_01JAAAAAAAAAAAAAAAAAAAAAB3",
         role: "coordinator", grant_id: "grant_01JAAAAAAAAAAAAAAAAAAAAAB4"}
事件 5  task.member.added {task_id, agent_id: "agent_01JAAAAAAAAAAAAAAAAAAAAAB5",
         role: "worker", grant_id: "grant_01JAAAAAAAAAAAAAAAAAAAAAB6"}
迁移:agent created→starting→running                                [S: core-transitions]
```

### A3. Worker 执行 reversible 能力(task scope 预授权直通)

```json
{"v": "0.1", "method": "capability.call",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAB7",
 "idempotency_key": null,
 "params": {"capability": "system.notes.write", "args": {"path": "notes/2026/archived.md", "content": "归档摘要"},
            "idempotency_key": "task:task_01JAAAAAAAAAAAAAAAAAAAAAB2:step:1", "deadline_ms": 5000}}
```

```json
{"event_seq": 6, "type": "capability.invoked", "occurred_at": "2026-08-29T11:00:03.200Z",
 "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAB8",
 "payload": {"call_id": "call_01JAAAAAAAAAAAAAAAAAAAAAB9",
             "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAB8",
             "capability": "system.notes.write", "principal": "agent:worker",
             "binding_epoch": 1, "provider_instance_id": "system.notes@0.1.0",
             "outcome": "ok", "error_code": null,
             "idempotency_key_hash": "sha256:1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809"}}
```

```text
迁移:operation not_started→running→succeeded                        [S: core-transitions]
对照:M4 决策矩阵中 task scope = validation_failed 的用例自 M5 起切换为
      直通(M5 规格 §8-3); grant 不存在/引用不存在 Task 时仍 validation_failed
```

### A4. Worker 声称完成 → Observation 核验 → Task 完成(完成判定门禁)

```text
[内部] Worker 声称:归档完成。Observation 消费 manifest verification 钩子
      (query: system.notes.read path=notes/2026/archived.md;expect: 内容存在)
核验证据:read-back 状态断言(确定性断言优先于模型自述,基线 §20)
```

```json
{"log_seq": 1, "task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2",
 "agent_id": "agent_01JAAAAAAAAAAAAAAAAAAAAAB5",
 "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAB8",
 "claim_summary": "Worker 声称归档完成",
 "evidence": [{"kind": "receipt", "ref": "op_01JAAAAAAAAAAAAAAAAAAAAAB8"},
              {"kind": "state_check", "ref": "notes/2026/archived.md exists"}],
 "verdict": "verified", "guard_state": "completed",
 "observed_at": "2026-08-29T11:00:04.000Z"}
```

```json
{"event_seq": 7, "type": "observation.recorded", "occurred_at": "2026-08-29T11:00:04.000Z",
 "payload": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAB2", "log_seq": 1,
             "verdict": "verified", "guard_state": "completed"}}
```

```text
事件 8  task.state.changed {task_id, from: "running", to: "completed",
         reason_code: "verified_completion", task_epoch: 1}
迁移:task running→completed(guard: verified_completion)            [S: core-transitions]
反例留档:verdict=unverified 时 completed 边非法,Task 只能转 blocked
          (outcome_unknown_pending)等用户裁定(场景 B 硬顶同路)
```

---

## 场景 B:监护链路——重复、停滞、编排重启、硬顶与用户恢复

前置:同 A0;新 Task「周报汇总」task_01JAAAAAAAAAAAAAAAAAAAAAC1 已
created→running,单 Worker 在册(过程同 A1–A2,事件 seq 从 20 起)。

### B1. 重复动作检测(连续 3 次同工具+同参数哈希+同错误)

```json
{"event_seq": 26, "type": "task.repeating", "occurred_at": "2026-08-29T11:10:00.000Z",
 "payload": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAC1",
             "agent_id": "agent_01JAAAAAAAAAAAAAAAAAAAAAC2",
             "capability": "system.notes.write", "repeat_count": 3}}
```

```text
监护:repeat_threshold=3(连续同 capability+同参数哈希+同错误)→ guard_state=repeating
Watchdog 只记事实与观测,不推断编排下一步(ADR-0004 条件 6;G4 守护面)
Coordinator 消费 repeating 观测后按自身策略决策(本场景:调整参数后继续)
```

### B2. 停滞检测 → Watchdog 触发编排重启(触发者之二)

```json
{"event_seq": 30, "type": "task.stalled", "occurred_at": "2026-08-29T11:25:00.000Z",
 "payload": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAC1",
             "stalled_ms": 900000, "last_progress_seq": 29}}
```

```json
{"event_seq": 31, "type": "watchdog.reorchestration.triggered", "occurred_at": "2026-08-29T11:25:00.050Z",
 "payload": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAC1",
             "trigger": "watchdog", "reason": "stalled_after=15m(默认窗口)"}}
```

```text
监护:无进展信号持续 15 分钟 → guard_state=stalled;Watchdog 发事实事件触发
编排重启——编排器消费后从最近一致持久状态重新推理(不重放 LLM 推理过程);
Runtime 监督层不自行动步。事件为事实(watchdog 已触发),非请求性命令
(G2 边界形态;watchdog.reorchestration.triggered 载荷无 requested_action 类字段)
对照:waiting_approval 态豁免自动重启(等的是人,不是机器)
```

### B3. 用户暂停与恢复(触发者之一;paused 双向边)

```json
{"v": "0.1", "method": "task.pause",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAC3",
 "idempotency_key": null,
 "params": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAC1", "reason": "我先看看"}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAC3", "ok": true,
 "result": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAC1", "state": "paused"}}
```

```text
事件 32  task.state.changed {task_id, from: "running", to: "paused",
          reason_code: "task_paused", task_epoch: 1}
迁移:task running→paused;成员级联 agent running→paused            [S: core-transitions]
```

```json
{"v": "0.1", "method": "task.resume",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAC4",
 "idempotency_key": null,
 "params": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAC1", "note": "继续,换个来源"}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAC4", "ok": true,
 "result": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAC1", "state": "running"}}
```

```text
事件 33  task.state.changed {task_id, from: "paused", to: "running",
          reason_code: "task_resumed", task_epoch: 1}
迁移:task paused→running;成员级联 agent paused→running             [S: core-transitions]
编排重启触发者之一 = 用户显式 resume(ADR-0004 条件 6/基线 §10.3)
```

### B4. 硬顶:停滞累计超 24h → blocked(不再自动重启)

```json
{"event_seq": 40, "type": "task.state.changed", "occurred_at": "2026-08-30T11:30:00.000Z",
 "payload": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAC1",
             "from": "running", "to": "blocked",
             "reason_code": "stall_hard_limit", "task_epoch": 1}}
```

```text
迁移:task running→blocked(guard: stall_hard_limit)                [S: core-transitions]
硬顶后不再自动重启(防无限空转烧预算);blocked 入口三因:
budget_exhausted / stall_hard_limit / outcome_unknown_pending
自动重启出口已封——仅 user_resolved 可回到 running(迁移表无其他 blocked→running 边)
```

### B5. 用户裁定恢复 → 核验完成

```text
用户经 Surface 对 blocked 提供裁定(补充来源说明)→ user_resolved
事件 41  task.state.changed {task_id, from: "blocked", to: "running",
          reason_code: "user_resolved", task_epoch: 1}
迁移:task blocked→running                                          [S: core-transitions]
(后续执行与核验同 A4:verification 通过 → running→completed,略)
```

---

## 轨迹不变量覆盖表

```text
场景 A 满足:Task 规范状态归 L2(task.created/state.changed 落事件)、
            task:<id> 授权直通(三方交集物化)、单 Worker 成员链
            (coordinator+worker)、声称完成必经 verification 核验才可
            completed(完成判定门禁)、Observation Log 对照记录落地
场景 B 满足:重复动作检测(repeating)、停滞检测与编排重启触发者之二
            (watchdog 事实事件)、用户暂停/恢复双向边与触发者之一
            (task.resume)、硬顶 blocked 且自动出口封死、user_resolved
            恢复、waiting_approval 豁免留档
两场景合并演示基线 M5 通过条件第 2/3/4 条的轨迹面
(第 1 条「Butler 只有协调权限」由权限矩阵测试承载,GT-02 已证审批面)
```
