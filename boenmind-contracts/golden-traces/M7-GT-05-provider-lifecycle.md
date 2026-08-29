# M7-GT-05:Provider 接入、进度与崩溃恢复(黄金轨迹)

> 目的:钉住 M7 通过条件的行为面——MCP Provider 可发现、可调用、可报告进度;
> Provider 崩溃不拖垮 Runtime;失败调用不无限等待。JSON 通过标注 schema;
> 状态迁移必须是 core-transitions 的边。
> 场景 A = MCP server(stdio)接入握手 → 首调审批 → 带进度调用成功;
> 场景 B = 子进程崩溃 → unavailable 快速失败 → 重连恢复。
> 上游:ADR-0010、M7 规格 §2-S1~S6;错误码与信封形态沿用 GT-02。

约定:`seq` = event_seq(全局单调);`→` = 迁移;`[S:…]` = 校验所依据的 schema。

---

## 场景 A:MCP server 接入与首次调用

### A0. 安装与握手(前置)

```text
前置:mcp 配置文件显式列出 server "notes"(trust=explicit-config,即用户安装批准,
      M7 规格 S6);Runtime 启动时执行握手:
      initialize(protocolVersion 2024-11-05)→ tools/list 返回 3 工具
      (notes.search / notes.read / notes.write)。
动态注册:能力名 mcp.notes.search / mcp.notes.read / mcp.notes.write;
      manifest 由 annotations 映射——search/read 标注 readOnlyHint → effect=read-only,
      approval=not-required;write 无标注 → effect=reversible-command,
      approval=required(未知风险首调审批,M7.7)。
      scopes=[domain:mcp.notes];input_schema=工具 inputSchema 直通。
注册完成不发独立事件;能力即刻进入发现面(capability list 可见)。
```

### A1. 首调 mcp.notes.write:approval=required → 强制审批

```json
{"v": "0.1", "method": "capability.call",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAM1",
 "idempotency_key": null,
 "params": {"capability": "mcp.notes.write", "args": {"path": "inbox.md", "content": "hi"},
            "idempotency_key": "m7gt05-write-1", "deadline_ms": 30000}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAM1", "ok": false,
 "error": {"code": "approval_required",
           "message": "能力 mcp.notes.write 需要用户审批",
           "retryable": true,
           "retry_after_ms": null,
           "detail_ref": null}}
```

```json
{"event_seq": 1, "type": "approval.requested", "occurred_at": "2026-08-30T10:00:00.200Z",
 "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAM2",
 "payload": {"approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAAM3",
             "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAM2",
             "capability": "mcp.notes.write", "principal": "surface:user",
             "risk_class": "reversible-command", "effective_risk": "reversible-command",
             "input_trust": "trusted", "expires_at": "2026-08-30T10:05:00.200Z"}}
```

```text
事件迁移:operation not_started→running→waiting_approval             [S: core-transitions]
```

### A2. 用户批准(scope=count:5)→ Grant 物化

```json
{"v": "0.1", "method": "approval.respond",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAM4",
 "idempotency_key": null,
 "params": {"approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAAM3", "decision": "approve",
            "scope": "count:5"}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAM4", "ok": true,
 "result": {"approval_id": "appr_01JAAAAAAAAAAAAAAAAAAAAAM3",
            "state": "approved", "grant_id": "grant_01JAAAAAAAAAAAAAAAAAAAAAM5"}}
```

```text
事件 1  approval.resolved {approval_id: "appr_01JAAAAAAAAAAAAAAAAAAAAAM3",
         outcome: "approved", scope: "count:5", grant_id: "grant_01JAAAAAAAAAAAAAAAAAAAAAM5"}
事件 2  grant.created {grant_id: "grant_01JAAAAAAAAAAAAAAAAAAAAAM5",
         approval_id: "appr_01JAAAAAAAAAAAAAAAAAAAAAM3", audience: "surface:user",
         action: "mcp.notes.write", scope: "count:5", delegation_depth: 0,
         parent_hash: <Approval 对象 SHA-256>}
事件迁移:operation waiting_approval→running(guard: approval_granted)    [S: core-transitions]
```

### A3. 重发同参调用:Grant 命中 → 执行,进度经 MCP notifications/progress 转发

```json
{"event_seq": 3, "type": "capability.progress", "occurred_at": "2026-08-30T10:00:01.100Z",
 "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAM6",
 "payload": {"call_id": "call_01JAAAAAAAAAAAAAAAAAAAAAM7",
             "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAM6",
             "capability": "mcp.notes.write", "progress": 1, "total": 2,
             "message": "writing"}}
```

```json
{"event_seq": 4, "type": "capability.invoked", "occurred_at": "2026-08-30T10:00:01.300Z",
 "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAM6",
 "payload": {"call_id": "call_01JAAAAAAAAAAAAAAAAAAAAAM7",
             "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAAM6",
             "capability": "mcp.notes.write", "principal": "surface:user",
             "binding_epoch": 1, "provider_instance_id": "mcp.notes@0.1.0",
             "outcome": "ok", "error_code": null,
             "idempotency_key_hash": "sha256:1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809"}}
```

```text
事件迁移:operation not_started→running→succeeded                    [S: core-transitions]
MCP tools/call 结果经既有 capability 收据路径落 execution receipt 与 outbox;
同 idempotency_key 重放返回原收据(outcome=suppressed 审计),与 M4 语义一致。
```

---

## 场景 B:子进程崩溃、快速失败与恢复

### B1. MCP 子进程崩溃 → provider unavailable(不拖垮 Runtime)

```json
{"event_seq": 5, "type": "provider.health.changed", "occurred_at": "2026-08-30T10:02:00.000Z",
 "payload": {"provider": "mcp.notes", "from": "healthy", "to": "unavailable",
             "reason": "stdio 子进程退出"}}
```

```text
Runtime 主回路不受影响:核心状态、既有收据、Task/Agent 全部照常;
后续对 mcp.notes.* 的调用走熔断快速失败路径(不等待)。
```

### B2. unavailable 期间调用:快速失败(unavailable,不无限等待)

```json
{"v": "0.1", "method": "capability.call",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAM8",
 "idempotency_key": null,
 "params": {"capability": "mcp.notes.read", "args": {"path": "inbox.md"},
            "idempotency_key": null, "deadline_ms": 30000}}
```

```json
{"v": "0.1", "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAM8", "ok": false,
 "error": {"code": "unavailable",
           "message": "provider mcp.notes 当前不可用",
           "retryable": true,
           "retry_after_ms": null,
           "detail_ref": null}}
```

```text
事件迁移:operation not_started→running→failed                      [S: core-transitions]
事件 3  capability.invoked {outcome: "error", error_code: "unavailable",
         capability: "mcp.notes.read", principal: "surface:user", ...}
```

### B3. 下次调用触发重连:握手成功 → 恢复 healthy → 调用成功

```json
{"event_seq": 6, "type": "provider.health.changed", "occurred_at": "2026-08-30T10:03:00.000Z",
 "payload": {"provider": "mcp.notes", "from": "unavailable", "to": "healthy",
             "reason": "重连握手成功"}}
```

```json
{"v": "0.1", "method": "capability.call",
 "request_id": "req_01JAAAAAAAAAAAAAAAAAAAAAM9",
 "idempotency_key": null,
 "params": {"capability": "mcp.notes.read", "args": {"path": "inbox.md"},
            "idempotency_key": null, "deadline_ms": 30000}}
```

```json
{"event_seq": 7, "type": "capability.invoked", "occurred_at": "2026-08-30T10:03:00.400Z",
 "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAANA",
 "payload": {"call_id": "call_01JAAAAAAAAAAAAAAAAAAAAANB",
             "operation_id": "op_01JAAAAAAAAAAAAAAAAAAAAANA",
             "capability": "mcp.notes.read", "principal": "surface:user",
             "binding_epoch": 1, "provider_instance_id": "mcp.notes@0.1.0",
             "outcome": "ok", "error_code": null, "idempotency_key_hash": null}}
```

```text
事件迁移:operation not_started→running→succeeded                    [S: core-transitions]
重连上限(restart_limit,缺省 3)内自动重试;超限保持 unavailable 直至重装。
```

---

## 轨迹不变量覆盖表

```text
场景 A 满足:安装批准(显式配置)、握手与能力发现(工具 → manifest 动态注册)、
            未知风险首调审批、Grant 消费、进度事件(capability.progress)、
            收据归因链(principal/binding_epoch/provider_instance_id)
场景 B 满足:Provider 崩溃不拖垮 Runtime、unavailable 快速失败(不无限等待)、
            重连恢复、重连上限
模型连接器(model.invoke)同构纳入:M7 规格 S1,行为由测试套件断言
            (本轨迹聚焦 MCP 外部 Provider 面,避免与 GT-01 turn 轨迹重复)
```
