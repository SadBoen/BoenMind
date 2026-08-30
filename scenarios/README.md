# 常见用户使用场景(CLI 实测清单)

- 目的:以真实用户视角,经 `boenmind` CLI 走通常见场景;发现的问题
  一律记入 `milestones/AUDIT-2026-08-30.md` 台账,**只记录不修改**。
- 前置:boenmind-server 运行中(本机 7531);`boenmind` CLI 在同目录。
- 用法:可直接在网页终端 `http://127.0.0.1:7531/cli.html` 逐条输入,
  或执行 `scenarios/run_all.sh`。

## 场景总表

| # | 场景 | 命令序列 | 预期 |
|---|---|---|---|
| S1 | 会话生命周期 | `session create --name demo` → `session resume <id>` → `session close <id>` | 三个命令全部退出码 0;resume 报会话状态;close 后再 resume 仍成功(历史不损坏) |
| S2 | 单轮问答(真实模型) | `session create --name qa --model gpt-5.6-luna` → `agent send <sess> <agent> "你好"` → `operations get <op>` | 回合 succeeded;收据含 action_summary |
| S3 | 能力直通(只读) | `capability call system.echo --args "{\"msg\":\"hi\"}"` | succeeded,result 原样回显 |
| S4 | 审批拒绝流 | `capability call system.danger.purge --args "{\"target\":\"notes\"}"` → `approval list` → `approval deny <appr>` → `operations get <op>` | 首调 approval_required;deny 后收据 state=cancelled |
| S5 | 审批批准 + 幂等抑制 | `capability call system.mail.mock_send --args "{\"to\":\"a@x\"}" --idempotency-key m5-1` → `approval approve <appr> --scope once` → 重放同命令同 key → 再重放一次 | 批准后执行成功;同 key 重放返回原收据(suppressed 审计) |
| S6 | 任务生命周期 | `task create "演示" "把话说完"` → `task list` → `task show <id>` → `task pause <id>` → `task resume <id>` → `task stop <id>` | 全部退出码 0;状态迁移 paused→running→stopped |
| S7 | 回合取消 | `agent send <sess> <agent> "长问题" --no-wait` → `agent cancel <sess> <agent> <op>` → `operations get <op>` | 收据 state=cancelled |
| S8 | 事件轮询 | `events poll <sess> --since 0 --limit 20` | 列出该会话事件,含 agent.turn.started 等 |
| S9 | 未知能力默认拒绝 | `capability call system.ghost` | 退出码非 0;permission_denied |
| S10 | 帮助与自描述 | `boenmind --help` | 列出全部子命令组 |

## 实测记录

- 2026-08-30 / 本机 release + 网页 CLI 终端 / S1✓ S2✓ S3✓(无参) S4✓
  S5✗(A-06 阻塞) S6✓ S7✓(受理) S8✓ S9✓ S10✓
- 发现:A-06(参数切分转义)、A-07(无会话速记)、A-08(分页提示)、
  A-09(取消竞速)→ 详见 milestones/AUDIT-2026-08-30.md
