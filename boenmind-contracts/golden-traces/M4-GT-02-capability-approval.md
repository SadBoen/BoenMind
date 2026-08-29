# M4-GT-02:能力调用与审批(黄金轨迹)

> 目的:给实现者一条 Broker 全链路的逐字节可对照路径。JSON 必须通过标注 schema;
> 状态迁移必须是 core-transitions 的边(含 M4 增发的 waiting_approval 三边)。
> 验证方式:回放器按本轨迹驱动 Runtime,逐条比对事件序列与 payload 结构。
> 场景 A = trusted 直通 + 高风险恒审批被拒;场景 B = untrusted 升级审批后批准执行。
> 上游:ADR-0001/0002 条件、M4 规格 §5.1/§5.4;错误码形态沿用 GT-01。

约定:`seq` = event_seq(全局单调);`→` = 迁移;`[S:…]` = 校验所依据的 schema。

---

## 场景 A:trusted 直通与高风险恒审批

### A0. Runtime 启动(前置:system.* 五能力已注册,Binding epoch=1)

```json
{"event_seq": 1, "type": "runtime.started", "occurred_at": "2026-08-29T10:00:00.100Z",
 "payload": {"pid": 44121, "version": "0.1.0-m4", "started_at": "2026-08-29T10:00:00.098Z"}}
```

### A1. 直通:用户 Surface 直调 read-only 能力(trusted,manifest approval=not-required)

```json
{"v": "0.1", "method": "capability.call",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAA06",
 "idempotency_key": null,
 "params": {"capability": "system.echo", "args": {"message": "ping"},
            "idempotency_key": null, "deadline_ms": 1000}}
```

Broker 决策(查表,规格 §5.1):principal=surface:user × system.echo → 命中
内建直通策略(read-only + trusted + not-required)→ allow,签发凭证
(binding_epoch=1,provider_instance_id="system.echo@0.1.0");审计随之落事件。

```json
{"event_seq": 2, "type": "capability.invoked", "occurred_at": "2026-08-29T10:00:00.180Z",
 "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAA03",
 "payload": {"call_id": "call_01JAAAAAAAAAAAAAAAAAAAAA02",
             "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAA03",
             "capability": "system.echo", "principal": "surface:user",
             "binding_epoch": 1, "provider_instance_id": "system.echo@0.1.0",
             "outcome": "ok", "error_code": null, "idempotency_key_hash": null}}
```

```text
事件迁移:operation not_started→running→succeeded                    [S: core-transitions]
调用方经 operations.get 可查收据(result 形态见 wire/capability#/capability.call/result)
```

### A2. 高风险恒审批:system.danger.purge(high-risk-command,与 trust 无关)

```json
{"v": "0.1", "method": "capability.call",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAA07",
 "idempotency_key": null,
 "params": {"capability": "system.danger.purge", "args": {"target": "notes"},
            "idempotency_key": null, "deadline_ms": 5000}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAA07", "ok": false,
 "error": {"code": "approval_required",
           "message": "能力 system.danger.purge 需要用户审批",
           "retryable": true,
           "retry_after_ms": null,
           "detail_ref": null}}
```

```json
{"event_seq": 3, "type": "approval.requested", "occurred_at": "2026-08-29T10:00:00.220Z",
 "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAA0A",
 "payload": {"approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAA04",
             "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAA0A",
             "capability": "system.danger.purge", "principal": "surface:user",
             "risk_class": "high-risk-command", "effective_risk": "high-risk-command",
             "input_trust": "trusted", "expires_at": "2026-08-29T10:05:00.220Z"}}
```

```text
事件迁移:operation not_started→running→waiting_approval             [S: core-transitions]
错误信封 code=approval_required(M4 起可用,CLI 退出码 4)
```

### A3. 用户经 CLI/GUI 查看并拒绝

```json
{"v": "0.1", "method": "approval.list",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAA08",
 "idempotency_key": null,
 "params": {}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAA08", "ok": true,
 "result": {"approvals": [{"approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAA04",
             "capability": "system.danger.purge", "principal": "surface:user",
             "risk_class": "high-risk-command", "effective_risk": "high-risk-command",
             "input_trust": "trusted", "state": "waiting_user",
             "scope_choices": ["once", "count:5", "ttl:1h"],
             "requested_at": "2026-08-29T10:00:00.220Z",
             "expires_at": "2026-08-29T10:05:00.220Z",
             "resolved_at": null, "grant_id": null}]}}
```

```json
{"v": "0.1", "method": "approval.respond",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAA09",
 "idempotency_key": null,
 "params": {"approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAA04", "decision": "deny"}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAA09", "ok": true,
 "result": {"approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAA04",
            "state": "denied", "grant_id": null}}
```

```text
事件 4  approval.resolved {approval_id, outcome: "denied", scope: null, grant_id: null}
事件迁移:operation waiting_approval→cancelled
         (guard: approval_denied_or_expired_or_withdrawn;denied 等价拒绝,基线 §9.6)
关键对照:审批被拒不产生 Grant;重发同参调用 → 新 Approval(默认拒绝语义保持,PI-07)
```

---

## 场景 B:untrusted 驱动 reversible → 100% 升级审批 → 批准执行

前置:同 A0。内部调用方(未来 Agent 路径的测试替身)以 agent-derived 上下文
转交一段不可信邮件内容驱动的写请求——trust 随内容来源链传递,调用方不可自报
降级(规格 §5.4;Wire 层无 trust 参数面)。

### B1. 内部调用:effective_risk 上提一级 → 强制审批

```text
[内部] broker.call(system.notes.write, args={path:"notes/inbox.md", content:<untrusted 摘要>},
                   principal="agent:note_bot", input_trust="untrusted")
决策:risk_class=reversible-command × untrusted → effective_risk=external-side-effect
      (上提一级,规格 §5.4)→ reversible 及以上 100% 升级(ADR-0002 条件 3)
事件 1' approval.requested {approval_id: appr_01JAAAAAAAAAAAAAAAAAAAAA0B,
         capability: "system.notes.write", principal: "agent:note_bot",
         risk_class: "reversible-command", effective_risk: "external-side-effect",
         input_trust: "untrusted"}
事件迁移:operation not_started→running→waiting_approval
```

### B2. 用户批准(scope=once)→ Grant 物化

```json
{"v": "0.1", "method": "approval.respond",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAA0E",
 "idempotency_key": null,
 "params": {"approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAA0B", "decision": "approve",
            "scope": "once"}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAA0E", "ok": true,
 "result": {"approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAA0B",
            "state": "approved", "grant_id": "grant_01JAAAAAAAAAAAAAAAAAAAAA0C"}}
```

```text
事件 2' approval.resolved {outcome: "approved", scope: "once",
         grant_id: "grant_01JAAAAAAAAAAAAAAAAAAAAA0C"}
事件 3' grant.created {grant_id, approval_id, audience: "agent:note_bot",
         action: "system.notes.write", scope: "once", delegation_depth: 0,
         parent_hash: <Approval 对象 SHA-256>}
事件迁移:operation waiting_approval→running(guard: approval_granted)
```

### B3. 查表命中 Grant → 执行成功

```json
{"event_seq": 5, "type": "capability.invoked", "occurred_at": "2026-08-29T10:02:10.900Z",
 "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAA0F",
 "payload": {"call_id": "call_01JAAAAAAAAAAAAAAAAAAAAA0D",
             "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAA0F",
             "capability": "system.notes.write", "principal": "agent:note_bot",
             "binding_epoch": 1, "provider_instance_id": "system.notes@0.1.0",
             "outcome": "ok", "error_code": null,
             "idempotency_key_hash": "sha256:9b1dec3f2a6c47d5b8e0f1a2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a"}}
```

```text
事件迁移:operation running→succeeded
Grant 消费:scope=once 首次消费即失效;第三次等价请求 → 新 Approval(需再批准)
幂等对照:同 idempotency_key 的重复请求返回原收据并留 outcome=suppressed 审计
          (规格 §5.9,ADR-0002 条件 6;本轨迹不含,由测试套件断言)
```

---

## 轨迹不变量覆盖表

```text
场景 A 满足:统一入口(无 Broker 外调用)、高风险需审批、审批可拒绝、
            waiting_approval 三边、审计归因链(capability.invoked 含
            principal/binding_epoch/provider_instance_id)
场景 B 满足:untrusted→reversible+ 100% 升级审批(ADR-0002 条件 3)、
            Grant 下限字段集物化(ADR-0002 条件 1)、scope=once 消费语义、
            input_trust 不可自报降级
两场景合并演示默认拒绝与审批闭环(基线 M4 通过条件的轨迹面)
```
