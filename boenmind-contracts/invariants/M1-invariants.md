# M1 不变量清单（断言级）

> 每条不变量 = 一条可测试断言。`检查方式`给出建议的测试形态；测试套件中以 INV-id
> 命名对应用例（CI 规则 R5）。来源列指向架构基线章节，语义冲突时以基线为准。

| ID | 不变量 | 检查方式 | 来源 |
|---|---|---|---|
| INV-1 | 一次 `agent.send_input` 恰好产生一个 `operation_id`；该 operation 在事件日志中恰好出现一次终态 | 属性测试（任意输入序列） | 9.5 |
| INV-2 | `operation.state.changed` 的每条记录的 `(from, to)` 必须是迁移表中的一条边；不存在表外迁移 | 回放断言 | 9.5 |
| INV-3 | `event_seq` 在单次运行内严格递增且连续（1,2,3,…无空洞）；乱序投递不改变按 seq 排序后的投影 | 回放 + 重复投递测试 | 8.3 |
| INV-4 | 每次 `connector.invoke`（无论成败）恰好产生一条 `model.invocation.completed` 或 `model.invocation.failed` 事件与一条对应 Execution Log 条目 | 属性测试 | 5.4 |
| INV-5 | 凭据明文不出现在任何事件、执行日志、错误信封、收据中：对全量日志执行 secret 值 grep，命中数必须为 0 | 泄漏扫描测试 | 4.6 |
| INV-6 | `session.close` / surface 断开不改变进行中 Operation 的状态；恢复（resume）后该 Operation 仍可查询且状态一致 | 断开恢复测试 | 7/14.2 |
| INV-7 | 预算强制点：`remaining_tokens` 不足的回合不得发起模型调用；`ratio ≥ 0.8` 必须出现 `budget.warning`；超限必须出现 `budget.exceeded` 且回合不发起 | 边界值测试 | 9.7 |
| INV-8 | 会话与 Agent 创建后，`session.created`、`agent.created` 事件的 seq 必须小于该 Agent 首个回合所有事件的 seq（因果序） | 回放断言 | 8.3 |
| INV-9 | 收据（receipt）查询是幂等的：任意时刻多次 `operations.get` 返回相同结果（终态后） | 幂等测试 | 9.5 |
| INV-10 | `outcome_unknown` 只能由迁移表 guard 允许的恢复路径结束；普通重试逻辑（同参数自动重发）不得把它当作 `failed` | 状态机 fuzz 测试 | 9.5/20 |
| INV-11 | 无外部副作用的失败必须落在 `failed`，不得落在 `outcome_unknown`；有外部副作用可能且结果未知的超时/崩溃必须落在 `outcome_unknown`，不得自动重放 | 状态机 fuzz + 场景 B 对照 | 13.3 |
| INV-12 | 显式取消语义：只有 `agent.cancel` 或用户裁定能把运行中回合引向 `cancelled`；`session.close`、`runtime.stop`（排空路径）不得产生 `cancelled` 终态 | 取消矩阵测试 | 3/13.4 |

## 使用说明

```text
上述 12 条在 M1 通过条件（基线 M1）之外，构成更细的验收面：
基线 M1 通过条件 = "界面断开不损坏 Session；输入、输出、工具调用和错误
都能关联到 Session、Agent 和 Operation"，由 INV-1/2/6/8 承载。
```
