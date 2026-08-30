# M8-GT-06:独立 Judge 评估报告(黄金轨迹)

> 目的:钉住 M8.7 的行为面——事件日志区间 → 独立评估器 → 确定性报告。
> 上游:evaluation/evaluation-report.v0_1;M8 规格 S5;t116/t117。
> 场景 A = 正常长任务区间评估(全过);场景 B = 故障区间(未匹配
> intent + 双终态)判 fail。报告体以 text 块呈现(派生工件,不进
> 事件日志/信封;落库经 evaluation_reports 表)。

约定:`seq` = event_seq(全局单调);`→` = 迁移;`[S:…]` = 校验所依据的 schema。

---

## 场景 A:正常区间评估(全部通过)

```text
输入:事件区间 [1,4]
  seq=1  runtime.started            {pid, version, started_at}
  seq=2  capability.invoked         {capability: "mcp.wiki.page.write",
                                     outcome: "intent", operation_id: op_a, ...}
  seq=3  capability.invoked         {capability: "mcp.wiki.page.write",
                                     outcome: "ok",      operation_id: op_a, ...}
  seq=4  model.invocation.completed {latency_ms: 1800, usage_in: 100, ...}
  outbox: op_a → published(副作用对账行)
```

```text
报告(evaluation-report.v0_1 形态;judge_version 0.1.0):
  report_id:    rep_<ulid26(from_seq 确定性派生)>
  range:        {from_seq: 1, to_seq: 4}
  checks:
    seq.contiguous        pass  "n=4 from=1 to=4 gaps=0"
    event.registry_keys   pass  "n=4 全部命中注册表键集"
    inv.single_terminal   pass  "ops=1 multi_terminal=0"
    receipt.side_effect   pass  "intents=1 unmatched=0"
    latency.bucket        pass  "n=1 p50=1800ms max=1800ms(门 30s)"
  summary:      {passed: 5, failed: 0, skipped: 0}
  generated_at: seq=4 的 occurred_at(确定性;非墙钟)
```

```text
不变量:同输入两次评估,报告逐字节一致(t117);
        报告过 [S: evaluation/evaluation-report.v0_1];
        落库 round-trip(evaluation_reports 表,v8)读回一致。
```

---

## 场景 B:故障区间(检查项判 fail)

```text
输入:事件区间 [1,3]
  seq=1  capability.invoked  {outcome: "intent", operation_id: op_b, ...}
         ——无任何完成事件、无 outbox 行
  seq=2  operation.state.changed {operation_id: op_b, from: running, to: failed}
  seq=3  operation.state.changed {operation_id: op_b, from: failed,  to: succeeded}
         ——同一 operation 两次终态
```

```text
报告:
  receipt.side_effect   fail  "intents=1 unmatched=1"
  inv.single_terminal   fail  "ops=1 multi_terminal=1"
  summary: {passed: 1, failed: 2, skipped: 2}
不变量:故障必须显性落入报告(summary.failed > 0),不得静默;
        verdict ∈ {pass, fail, skipped};evidence 只引 seq/计数,零原文
        (INV-5:合同结构阻止自由文本字段)。
```

---

## 轨迹不变量覆盖表

```text
场景 A 满足:长任务可回放可评估(基线 M8 通过条件第二句)、
            副作用收据检查(第三句)、Judge 确定性(同输入恒同报告)
场景 B 满足:故障显性化、报告字段封闭(脱敏纪律的合同面)
独立保证:Judge 只读事件日志与 outbox,不依赖运行时内存态(bm-judge crate)
```
