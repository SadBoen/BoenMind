# M1-GT-01：单 Agent 回合（黄金轨迹）

> 目的：给实现者一条逐字节可对照的端到端路径。本文件中每个 JSON 必须通过其标注的
> schema；每个状态迁移必须能在 core-transitions 中找到对应边；每个步骤标注其满足的
> 不变量（INV-*，见 invariants/M1-invariants.md）。
> 验证方式：回放器按本轨迹驱动 Runtime，逐条比对事件序列与 payload 结构。
> 场景 A = 正常回合；场景 B = 模型链超时到失败。

约定：`seq` = event_seq（全局单调）；`→` = 迁移；`[S:…]` = 校验所依据的 schema。

---

## 场景 A：正常完成一个回合

### A0. Runtime 启动

```text
[进程内] runtime.start()
事件 1  [S: envelope#/event_envelope]
```

```json
{"event_seq": 1, "type": "runtime.started", "occurred_at": "2026-08-29T09:30:00.100Z",
 "payload": {"pid": 43121, "version": "0.1.0-m1", "started_at": "2026-08-29T09:30:00.098Z"}}
```

### A1. 创建会话与 Agent

```json
{"v": "0.1", "method": "session.create",
 "request_id": "req_01J9Z8G3K2X7M4Q6B8WD5RNYVT",
 "idempotency_key": null,
 "params": {"agent": {"name": "assistant",
                      "model_chain": ["zhipu.glm-4-flash", "openai.gpt-4o-mini"],
                      "budget": {"max_tokens": 50000, "max_turns": 10}}}}
```

```json
{"v": "0.1", "request_id": "req_01J9Z8G3K2X7M4Q6B8WD5RNYVT", "ok": true,
 "result": {"session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
            "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
            "created_at": "2026-08-29T09:30:00.220Z",
            "resume_cursor": {"event_seq": 3}}}
```

```text
事件 2  session.created  {session_id, agent_id}                        [INV-8]
事件 3  agent.created   {agent_id, session_id, model_chain[2]}         [S: envelope]
事件迁移：session created→active；agent created→starting→running       [S: core-transitions]
预算强制点①：回合未开始，预估通过（max_tokens=50000）                  [INV-7]
```

### A2. 发起回合（产生执行收据）

```json
{"v": "0.1", "method": "agent.send_input",
 "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
 "idempotency_key": null,
 "params": {"session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
            "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
            "content": "用一句话解释什么是幂等性",
            "input_trust": "trusted"}}
```

```json
{"v": "0.1", "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK", "ok": true,
 "result": {"operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
            "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
            "principal": "user", "task_type": "agent.turn",
            "state": "running",
            "created_at": "2026-08-29T09:30:05.012Z",
            "completed_at": null,
            "action_summary": "Agent 回合：解释幂等性",
            "result_reference": null, "error": null}}
```

```text
事件迁移：operation not_started→running；agent running→waiting_model
事件 4  agent.turn.started   {operation_id, turn_index: 1}             [INV-1]
事件 5  agent.waiting_model  {operation_id, model_id: "zhipu.glm-4-flash"}
```

### A3. 模型调用（Runtime 内部，按 model/connector 合同）

```text
[内部] connector.invoke(attempt=1)   [S: model/connector#/definitions/invoke_request]
请求要点：model_id=zhipu.glm-4-flash；tools=[]；secret_ref="secret:model/zhipu"；
budget_ctx.remaining_tokens=50000；deadline="2026-08-29T09:30:35.012Z"
响应要点：ok=true；usage={tokens_in: 412, tokens_out: 58}；finish_reason="stop"；
latency_ms=1873；stream_interrupted=false                        [INV-4][INV-5]
```

```text
事件 6  model.invocation.completed {attempt: 1, usage_in: 412, usage_out: 58,
                                    latency_ms: 1873, stream_interrupted: false,
                                    content: "…回答正文…(M8.1 起携带,截断 16KB)",
                                    content_truncated: false}
事件迁移：agent waiting_model→running；operation running→succeeded
事件 7  operation.state.changed {from: "running", to: "succeeded", reason_code: "result_recorded"}
事件 8  agent.completed {turn_index: 1}
预算强制点③：记账 {used_tokens: 470, limit_tokens: 50000, ratio: 0.0094} < 0.8，无 warning
```

### A4. 查询收据

```json
{"v": "0.1", "method": "operations.get",
 "request_id": "req_01J9Z8G6CQX7M4Q6B8WD5RY4NJ",
 "idempotency_key": null,
 "params": {"operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM"}}
```

```json
{"v": "0.1", "request_id": "req_01J9Z8G6CQX7M4Q6B8WD5RY4NJ", "ok": true,
 "result": {"operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
            "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
            "principal": "user", "task_type": "agent.turn",
            "state": "succeeded",
            "created_at": "2026-08-29T09:30:05.012Z",
            "completed_at": "2026-08-29T09:30:07.110Z",
            "action_summary": "已回答幂等性问题（412 入 / 58 出 token）",
            "result_reference": {"kind": "execution_log", "ref": "log:op_01J9Z8G56BX7M4Q6B8WD5RV6QM"},
            "error": null}}
```

### A5. Execution Log 落盘内容（可回放断言）           [S: logs/execution-log-entry]

```json
{"log_seq": 1, "ts": "2026-08-29T09:30:05.012Z", "kind": "agent.turn",
 "session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
 "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
 "operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
 "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
 "state": "running", "secret_scan": "passed",
 "detail": {"turn_index": 1, "input_digest": "sha256:9f2c…", "input_bytes": 42}}
```

```json
{"log_seq": 2, "ts": "2026-08-29T09:30:06.890Z", "kind": "model.invocation",
 "session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
 "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
 "operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
 "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
 "state": "waiting_model", "secret_scan": "passed",
 "detail": {"model_id": "zhipu.glm-4-flash", "attempt": 1,
            "usage": {"tokens_in": 412, "tokens_out": 58},
            "latency_ms": 1873, "stream_interrupted": false}}
```

```json
{"log_seq": 3, "ts": "2026-08-29T09:30:07.108Z", "kind": "budget.check",
 "session_id": "sess_01J9Z8G4A1X7M4Q6B8WD5RQ2WX",
 "agent_id": "agent_01J9Z8G4A1X7M4Q6B8WD5RS3ZP",
 "operation_id": "op_01J9Z8G56BX7M4Q6B8WD5RV6QM",
 "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
 "state": "running", "secret_scan": "passed",
 "detail": {"scope": "agent", "used_tokens": 470, "limit_tokens": 50000, "ratio": 0.0094}}
```

### A6. 关闭会话并停止 Runtime

```text
[会话关闭不取消任何已完成的操作；若回合仍在进行，结果等效于 detach（INV-6）]
session.close → 事件 9  session.closed {reason: "user_request"}
runtime.stop   → 事件 10 runtime.stopping → 事件 11 runtime.stopped {uptime_ms: 8210}
```

---

## 场景 B：模型链超时 → 回合失败（简版）

前置：同 A0/A1，预算 max_tokens=50000。send_input 后模型调用两次尝试全部超时。

```text
[内部] connector.invoke(attempt=1) → ok=false, error_code="timeout", retryable=true
[内部] connector.invoke(attempt=2) → ok=false, error_code="timeout", retryable=true
       降级链已尽（attempt ≤ 3 为合同上限，本链配置 max_attempts=2）
```

```text
事件 6'  model.invocation.failed {model_id: "zhipu.glm-4-flash", attempt: 1, error_code: "timeout"}
事件 7'  model.invocation.failed {model_id: "openai.gpt-4o-mini", attempt: 2, error_code: "timeout"}
事件迁移：agent waiting_model→failed；operation running→failed
         （guard: error_terminal_and_no_external_effect_possible ——
           模型调用无外部副作用，故 failed 合法；若存在外部副作用则必须走 outcome_unknown）
事件 8'  agent.failed {operation_id, error_code: "timeout"}
```

```json
{"v": "0.1", "request_id": "req_01J9Z8G56BX7M4Q6B8WD5RT8HK",
 "ok": false,
 "error": {"code": "timeout",
           "message": "模型降级链耗尽：2 次尝试均超时",
           "retryable": false,
           "retry_after_ms": null,
           "detail_ref": null}}
```

```text
CLI（M3 起）按映射退出码 7；此刻（M1）测试进程内断言 error.code == "timeout"
关键对照：本场景不出现 outcome_unknown —— 无外部副作用的失败必须落在 failed
```

---

## 轨迹不变量覆盖表

```text
场景 A 满足：INV-1 INV-2 INV-3 INV-4 INV-5 INV-6 INV-7 INV-8 INV-9 INV-12
场景 B 满足：INV-2 INV-3 INV-9 INV-10 INV-11 INV-12
两条场景合起来演示了 failed 与 outcome_unknown 的分界（基线 9.5 的核心语义）
```
