# M6-GT-04:Team 委派与结果收集(黄金轨迹)

> 目的:给实现者一条多成员团队与委派链的逐字节可对照路径。JSON 必须通过
> 标注 schema;状态迁移必须是 core-transitions 的边(含 M5 task 状态机)。
> 场景 A = 建根 Task → 多成员 spawn → 子任务委派(深度/预算/子集门禁)→
> 结果收集;场景 B = 成员故障 → 替换 spawn → Task 不倒。
> 上游:基线 §18-M6 六子项、ADR-0002 条件 5 余项、M6 规格 §5。

约定:`seq` = event_seq(全局单调);`→` = 迁移;`[S:…]` = 校验所依据的 schema。

---

## 场景 A:根 Task、多成员与子任务委派

### A0. Runtime 启动(前置:system.* 与 memory.* 能力已注册;bootstrap Grant 已物化)

```json
{"event_seq": 1, "type": "runtime.started", "occurred_at": "2026-08-30T09:00:00.100Z",
 "payload": {"pid": 45100, "version": "0.1.0-m6", "started_at": "2026-08-30T09:00:00.098Z"}}
```

### A1. 建根 Task(授权声明含两个能力资源)

```json
{"v": "0.1", "method": "task.create",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAD1",
 "idempotency_key": null,
 "params": {"title": "周报汇总", "goal": "汇总本周笔记与邮件摘要",
            "authorization": [{"verb": "capability.call", "klass": "mutation",
                               "resources": [{"capability": "system.notes.write"},
                                             {"capability": "system.mail.mock_send"}]},
                              {"verb": "agent.spawn", "klass": "mutation"}],
            "budget": {"max_tokens": 1000000, "max_turns": 1000, "max_tool_calls": 50},
            "deadline": null}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAD1", "ok": true,
 "result": {"task_id": "task_01JAAAAAAAAAAAAAAAAAAAAAD2",
            "state": "running", "created_at": "2026-08-30T09:00:01.000Z"}}
```

```text
事件:task.created {task_id, title, created_by, parent_task_id: null}
迁移:task created→running(task_started)                        [S: core-transitions]
协调链自举(M5 语义,per-task principal):Coordinator Grant ×3 +
Worker Grant ×2(notes.write/mail.mock_send 各一枚,scope=task:<id>,
parent 哈希链可上溯);成员事实 coordinator + worker
```

### A2. 追加第二名 Worker(并发门禁内)

```text
[内部] task_spawn_member(task_01JAAAAAAAAAAAAAAAAAAAAAD2)
事件:task.member.added {task_id, agent_id: <worker#2>, role: "worker",
         grant_id: <worker grant>}
并发口径:存活 worker = 2 ≤ 5(M6.5 门禁通过)
```

### A3. 委派子任务(深度 1,授权子集)

```text
[内部] spawn_subtask(parent=task_01JAAAAAAAAAAAAAAAAAAAAAD2,
                     title="邮件摘要",
                     authorization=[capability.call × {system.mail.mock_send}],
                     budget={max_tool_calls: 10})
门禁:深度 0+1=1 ≤ 3 ✓;动词子集 ⊆ 父 ✓;资源谓词取交 ✓;
      max_tool_calls 10 ≤ 父剩余 48 ✓
事件 1  task.created {task_id: task_01JAAAAAAAAAAAAAAAAAAAAAD3,
         title: "邮件摘要", created_by: "agent:coord:task_01JAAAAAAAAAAAAAAAAAAAAAD2",
         parent_task_id: task_01JAAAAAAAAAAAAAAAAAAAAAD2}
事件 2  task.state.changed {task_id: D3, from: created, to: running,
         reason_code: task_started, task_epoch: 1}
迁移:task created→running                                       [S: core-transitions]
子任务 Coordinator/Worker Grant:parent 哈希链回溯至父 Worker Grant
(授权链跨层级可上溯;delegation_depth 恒 0 语义不变——转授禁止的是
能力授权,任务分解走子任务)
```

### A4. 结果收集(来源/状态/关联 Operation)

```text
[内部] task_collect(task_01JAAAAAAAAAAAAAAAAAAAAAD2)
Worker 调用 ×2:system.notes.write → ok(op_…E5);子任务完成回报
聚合输出:每条结果含 agent_id(agent:worker:task_…D2)/ state(succeeded)/
operation_id(op_…E5)/ capability / action_summary —— 来源、状态、
关联 Operation 三要素齐备(基线 M6 通过条件第 4 条)
```

---

## 场景 B:成员故障与替换

```text
前置:根 Task「清理缓存」task_01JAAAAAAAAAAAAAAAAAAAAAF1(running,1 Worker)
B1. Worker 连续失败(Provider error):capability.invoked outcome=error ×2
    ——Task 保持 running(成员故障不迁移 Task 状态;watchdog 重复检测照常)
B2. 替换:task_spawn_member → 新 Worker 成员事实(member.added;
    旧成员留痕 task.member.removed {reason: "replaced"})
    新成员调用成功 → Task 继续
B3. 对照:存活 worker 计数含替换前后(≤ 5);故障成员的 Grant 随 Task
    终态统一撤销(Task 结束即失效,M5 语义延续)
```

---

## 轨迹不变量覆盖表

```text
场景 A 满足:Team = 成员编队 + 子任务树(不设独立 Team 合同,规格 §8-1)、
            委派四门禁(深度/子集/预算/并发)、授权链跨层级可上溯、
            收集三要素(来源/状态/关联 Operation)、parent_task_id 归因
场景 B 满足:成员故障不迁移 Task 状态、替换 spawn、并发门禁口径、
            member.removed 留痕
两场景合并演示基线 M6 通过条件第 1/2/3/4 条的轨迹面
```
