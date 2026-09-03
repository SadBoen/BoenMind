# ADR-0019: system.exec 内置命令执行工具(审批类)

状态:accepted(2026-09-03,用户令「按常规设计,不用问我,一次性做到日常可用」)

## 背景

BoenMind 出厂零执行工具(ADR-0005/0006 立场:权限以合同显式化),导致对话里的
Agent 无法执行任何宿主命令——用户实测「帮我在 VPS 上跑一条命令」直接不可达,
而 pi(read/bash/edit/write)、Hermes、DSH 等主流 Agent 均出厂自带 shell。
调研结论:跑代码不需要专门工具,通用 shell 即万能执行器;分歧只在权限边界。

## 决策

1. **新增内置能力 `system.exec`**:模型可在对话中发起宿主命令(Windows=
   cmd /C,其余=sh -c);effect=external-side-effect → **每次调用必弹审批卡**
   (与 Claude Code 的「每条命令先确认」常规交互一致,亦满足 Broker 管线);
   批准后执行,拒绝即取消。
2. **形态 = 内置异步能力**:provider id `builtin.async`(以 `.async` 结尾
   → registry 标异步,handle.rs 判定规则扩展),与 MCP 同管线——超时钳制
   (默认 60s,参数可调 ≤300s)、取消、单写者零阻塞、收据轮询 + op_results
   入表(直通修复后轮询路径语义一致)。
3. **边界**:输出合并截断 16K 字符;超时 kill_on_drop;不做 shell 沙箱——
   真沙箱化执行仍归 ADR-0016 的 Skill v0.2 管线(BACKLOG 既有条目);
   context-mode 的受限执行与其互补,不因本能力取消排期。
4. **工作目录**:首版以服务进程 cwd 执行;会话绑定工作区(ADR-0018)的
   cwd 注入对 exec 的消费仍列 BACKLOG(与 Skill v0.2 执行线同批评估)。

## 后果

- 对话 Agent 获得「能动手」的常规体验;审批卡保证人仍在环。
- 高风险命令的最终防线是用户审批而非静态分类——误批准风险由用户承担,
  与主流 Agent 常规一致。
- 模型可见工具清单新增 system.exec(描述:需要用户审批),直通/审批两路
  行为均已有回合级测试覆盖。
