# M5 里程碑回看记录(基线 §19 门)

> 定稿:2026-08-30 回看(骨架由子代理预起草,全部 ⏳ 已回填)。
> 静态内容来源:`milestones/M5-implementation-spec.md` §4/§7/§8 与
> ADR-0002/0004「条件与验收」。

## Evaluation Record

```text
milestone_id:         M5(Butler、Task 和长期监护)
build_or_commit_id: 29e7ffa(前置结算)→ 9a5917c(T0)→ cc7c7af(T1)→ a240c47(T2)
                      → a8394cc(T3)→ 9bc6f29(T4+T5)→ 1f5346f(T6+T7)→ cc116d2(T8)
test_run_id: cargo test --workspace(2026-08-30,本机)
                      = 188 passed / 0 failed(M1 50 → M4 134 → M5 188;
                      增量:e2e t50-t88、watchdog 单测 5、coordinator 3、butler 4)
log_range:            task.* 事件族首度入流(32→40):task.created/task.state.changed
                      (from/to/reason)/task.member.added/task.budget.increased/
                      task.stalled/task.repeating/watchdog.reorchestration.
                      triggered(事实事件,守 G2 边界)/observation.recorded;
                      memory 调用复用 capability.invoked,零新增;GT-03 三场景
                      回放(A 主链路/B 监护链路/C 声称完成未生效)由 e2e 承载
deterministic_checks: validate.py 全绿(合同库 20 → 25 份工件:task/wire-task/
                      memory-entry/observation-log 四份新合同 + P-11 行,全 Minor);
                      信封/事件/收据 schema 全校验;task 状态机 7 态 12 边逐条镜像;
                      GT-01/02/03 轨迹遍历(validate.py R2–R4 含 task 机)
failure_tests:        t50 表外拒绝与终态迁出拒绝、t61 撤销后建单拒+不复活、
                      t62 领域动词上界拒、t72 未授权能力 100% 升级、t73 task-scope
                      引用不存在 Task 拒、t80 预算硬限 blocked、t84 重复 3 次检测、
                      t86 无证据声称 blocked(禁止自动标成功)、t88 非法 scope 拒、
                      GT-01 伪造行拒开(存量)
replay_result:        GT-01 双场景绿(启动期 12 条 bootstrap Grant 事件按类型过滤,
                      INV-3 连续性不受影响);GT-02 回归绿;GT-03 场景 A/B 由
                      e2e t50/t71/t85/t86/t82 承载;Task Board 投影重建确定性
                      (t56:两次重建逐字节一致 + 与 L2 一致 + 位点=日志末尾)
llm_evaluation:       不适用(M8.7 起;独立 Judge 接口预留不实现,规格 §5.7)
known_failures:       见 §6 条件与遗留
architecture_changes: 合同 Minor 增发九项(规格 §4):task/task.v0_1(Task 规范
                      对象)+ runtime-events +8(32→40,sync.rs 事件数断言同步)
                      + wire/task 六方法 + events.poll task_id 过滤(session.
                      v0_1 只增字段)+ envelope method +6/error_code 零新增 +
                      core-transitions(task 7 态 11 边;agent 增 paused 四边)+
                      memory/memory-entry.v0_1 + logs/observation-log-entry.v0_1
                      + GT-03 + perf-baseline P-11 骨架;M4 冻结面零改动启用:
                      verification 钩子(query/expect/within_ms)消费、task:<id>
                      scope 产生、parent_grant_hash 链(delegation_depth 恒 0)
acceptance_decision:  passed_with_conditions(条件见 §6)
reviewed_at:          2026-08-30
```

## §5 逐门记录

(验收对象/预期载体抄自规格 §7 载体映射与 §6 任务分解;实证结论 T10 回填。)

- **A 功能测试**:M5.1 Butler 内置 App → T3(注册、bootstrap 协调权 Grant 集、
  task.create 全链路、双路径收据来源标注);「Butler 只有协调权限」载体 =
  T3 权限矩阵(协调动词清单恰为 §10.1 集、领域动词默认拒绝、无旁路 G3 逆
  断言、bootstrap Grant 可撤销)。M5.2 Task 生命周期 → T1/T2(7 态 11 边、
  task_epoch 门禁、task.* 事件族、SQLite v4);「Task 状态不依赖 Butler 内存」
  载体 = T2/T3(清空 Butler 内存态与投影缓存后 task.list 行为逐字节一致;
  Runtime 重启后 Task 恢复 + 投影重建确定性;损坏投影缓存无行为差异)。
  M5.3 Coordinator → T4(三方交集 task:<id> Grant、safe/mutation 二分、
  子树裁剪 Broker 强制、成员授权签发链);M5.4 投影 → T2(event_seq 绑定、
  重建确定性);M5.5 watch/pause/resume/stop → T2(六方法、paused 四边、跨
  重启恢复);M5.7 memory.* → T8(三能力、四作用域、FTS5、纠正覆盖、级联
  墓碑)。实证:t85 verified 完成/t86 无证据声称 blocked+用户恢复/t87 memory 生命周期(写入/检索/审批删除/级联/纠正)/t88 非法 scope 拒绝;memory:user 显式授权执行面随 M7(PENDING D-M5-2)。
- **B 回归测试**:M1–M4 存量 134 项全绿(T9);GT-01/GT-02 回放在场;
  P-01..P-10 复跑劣化 < 25%。实证:perf-baseline 记录④——P-11 首填 release p95≈0.95ms(1 万事件,门 1s);P-02/03/04/05/07/08 门内;P-01 触门(+49%/+53%)判解释留档(首启 12 条 bootstrap Grant 持久化的有意成本,双值一致排除噪声,绝对值 <0.21ms),不回炉。
- **C 故障测试**:「发现无进展/重复/持续错误」载体(规格 §7)= T7 注入测试:
  空转→repeating(3 次)、停滞→stalled(15min 默认)、硬顶→blocked(24h)、
  waiting_approval 豁免;mock_drift 四类注入(空转/重复/停滞/声称完成但未
  生效);预算硬限 → Agent 暂停 + Task blocked(budget_exhausted);T6c 落盘
  重启不再回满;过期 task_epoch 命令 Stale 拒绝留审计。实证:t51 Task 状态/epoch 跨重启不回退、t52 count 余量不复活、t53 幂等收据跨重启抑制、t60-t61 撤销持久不复活、butler/task 单测门禁(Stale/门禁拒绝)。
- **D 日志回放**:GT-03 场景 A(建 Task→Coordinator 规划→spawn Worker→
  副作用调用→声称完成→verification 核验→completed)/B(Worker 空转/停滞→
  Watchdog 检测→编排重启或 blocked→用户 resume→恢复)/C(声称完成但未生效
  证伪)逐事件形态可回放;task.* 八事件 + observation.recorded 归因链;
  validate.py R2–R4 轨迹遍历。实证:t56 两次重建逐字节一致 + 与 L2 状态一致 + 位点=日志末尾;t55 events.poll task_id 过滤;损坏投影缓存无行为差异由 T1/T2 混沌等价映射承载。
- **E 确定性评估**:「声称完成可核验」载体(规格 §7)= T8:verification
  失败不得 completed(GT-03 场景 C)、unverified → outcome_unknown 等用户、
  收据/查询优先于自述;子树裁剪决策矩阵(T4 随行);双路径收据合同一致
  测试进 CI(T3/T5)。实证:capability_call_inner 双路径统一执行体,收据/事件 principal=来源标注(surface/worker 同构);t71 worker Grant 直通+t87 memory 经同一管道;PendingCapabilityCall 带身份,审批重放按原 principal 归因。
- **F LLM 评估**:不适用(M8.7 起)。
- **G 架构复盘**:实现既有拓扑,非架构变更(C4 ButlerPaths/TaskFlow 已含
  全部组件);Task 规范状态 L2 唯一持有、Butler 内存与 Task Board 仅投影;
  Watchdog「仅监督,不推断」+ G4 守护(产物无命令形状);Broker 为预算
  唯一执行点;Butler 无内核特权、协调权可撤销。实证:t60 12 动词物化+跨重启幂等、t61 撤销后建单拒(permission_denied)且不复活、t62 领域动词上界拒、t70 协调链授权链(parent_hash 可上溯)、t81 扩容仅用户面生效。
- **H 验收裁决**:passed_with_conditions(前置结算八项全闭合见下表;条件与遗留见 §6)。
- **I 性能冒烟**:P-01..P-10 复跑(门:劣化 < 25%);P-11 首填(Task Board
  投影重建延迟,ADR-0004 未决分歧 4 的首个定量回应,数值 T9 回填);
  Watchdog 扫描开销随 P-11 一并观测(规格 §9)。实证:t82 停滞→事实事件→硬顶 blocked(15min/24h/同episode不重复通告)、t83 审批豁免、t84 重复 3 次、G4 事实形状断言(单测+e2e);24h 常量笔误被测试先行抓住修正。

## M5 前置结算条件逐条闭合表(settlement → 闭合状态)

| 条件 | 来源 | 落点 | 闭合状态 |
|---|---|---|---|
| | ✅ 闭合:触发者两类(用户 resume ∪ Watchdog 事实事件);15min/24h/审批豁免;t82 实证 |
| | ✅ 闭合:软限 80% 告警+硬限 blocked(budget_exhausted);扩容仅用户(t81);reservation 延续不做随真实负载裁决 |
| | ✅ 闭合:M5 单 Task 命名空间构造性裁剪 + 终态失效;多 Team 隔离随 M6 per-task principal |
| | ✅ 接口面闭合:memory:task:<id> 承载任务上下文;真实跨 Task 场景随 M6 |
| | ✅ 闭合:统一执行体 + principal 来源标注 + 重放归因;ADR-0002 条件 4 双开解除 |
| | ✅ 闭合:high-risk 恒 once、external 默认 ttl、forever 须审批卡片显式选择 |
| | ✅ 闭合:t52/t53 跨重启实证(count 余量不回满、抑制不重放副作用) |
| | ✅ 首填:release p95≈0.95ms(1 万事件,门 1s,余量 ~1000×) |

**裁决(T10):八项前置条件全部闭合——ADR-0004 条件 6 闭合;ADR-0002
条件 2/5 余项闭合、条件 4 双开解除,对外口径升级为「成立」(条件 1/3 已于
M4 实证)。预期(规格 §5.2/§5.4):8 项闭合后 ADR-0004 条件 6 正式
闭合;ADR-0002 条件 2/5 余项闭合、条件 4 的双开禁止有合同前提地解除;
M4 遗留 forever 收紧/T6c 复核了结。**

## S1–S10 相关项裁决(deepwiki-validation 修订建议)

- M4 回看承接(静态):S5 注册期前置校验已部分采纳(quarantined 分表随
  M7);S4「摘除→排空→终止」完整两步随 M7 真实进程;S8 Wire 分方向留
  M8 发行前评估;S9 verification 三分法的 Liveness/Readiness/Startup 动作
  映射随 M7 Provider 生命周期;S1/S2/S3/S6/S7/S10 属 M7/M8 范围。
- 本回看候选相关项(是否纳入裁决由 T10 定):S9 的 verification 钩子
  M5 启用消费于完成判定(规格 §5.7);S3 停滞检测思想与 Watchdog 停滞
  判定的关系(generation 升级停滞检测仍属 M8)。
- 本回看逐条裁决结果:S9 部分采纳(verification 钩子消费落地,Liveness/Readiness 映射随 M7);S3 方向已实践(watchdog 检测面,完整对照随 M6);其余维持 proposed。

## §6 条件与遗留

1. **单进程 Orchestrator 等价映射复测**(规格 §8-2):M5 验收 = 清空编排器
   内存态 + Runtime 重启恢复 + 投影重建确定性 + 损坏投影缓存无行为差异;
   真实独立进程形态随 M7 Provider 外置 / M8 部署定标复测。
2. **M7 项**:真实外部 Provider/MCP(非目标);预算 reservation 是否引入随
   M7/M8 真实负载数据裁决(规格 §8-8)。
3. **M8 项**:独立 Judge(M8.7 起,接口预留不实现);审批交互形态(forever
   显式选择,规格 §8-5);用户删除权全量(单 Task/App 域/全局 + 备份加密,
   规格 §8-7);向量检索引擎替换(接口预留,规格 §5.8)。
4. **M6 承接项**:多成员 Team/编队与 delegate 归因链(M6.1/6.3/6.4);深度
   与并发上限(M6.5,M5 仅单成员 spawn 闭环);Memory Service 真实跨 Task
   场景复用(规格 §5.8)。
5. **PENDING D-M5-1**(规格 §5.2/§8-1/§9):停滞窗口数值(15min/3 次/60s/
   24h)大白话追认或改判;数值属产品体验,Task 可配置,变更不破合同。
6. **解读条款回看复核**(规格 §8):§8-3(task:<id> scope 启用与 M4 决策
   矩阵切换)、§8-4(delegation_depth 链式语义)、§8-6(事实事件 G2 边界)
   等逐条复核。
7. **CI 三平台确认**:推送后矩阵全绿(188 项;R5/R6 镜像断言在列):推送 cc116d2 触发,本地全绿为门(⏳ 矩阵结果随 CI 运行确认)

## §7 回看七问(基线)

1. 解决目标问题?(M5 通过条件四条:Butler 只有协调权限/Task 状态不依赖
   Butler 内存/发现无进展、重复动作和持续错误/声称完成的结果可实际观察
   核验)——是:无 verified 核验不得 completed,unverified 一律 blocked 等裁定(t85/t86)。
2. 旧能力可用?是——M1–M4 存量全绿;合同全 Minor 增发零破坏;GT-01/02 回放绿(启动期系统事实按类型过滤,INV-3 不破)。
3. 崩溃/断线/重复执行?(编排重启触发者恰两类;task_epoch Stale 拒绝;
   T6c 落盘重启不回满;幂等键续跑)——是:t51 状态/epoch 不回退、t52 余量不复活、t53 抑制不重放、blocked 无自动出口、watchdog 同episode不重复通告。
4. 日志能解释每一步?(task.* 八事件 + watchdog 事实事件 +
   observation.recorded;归因链含 task_id/operation_id)——能:状态迁移带具体 reason_code,授权链 parent_hash 可上溯 bootstrap,watchdog 事实事件可区分触发者。
5. 结果被实际核验?(verification 收据/查询优先于模型自述;完成判定门禁
   guard verified_completion)——是:t85/t86 门禁双向实证。
6. 合同与状态模型稳定?(全 Minor 增发;sync.rs 事件数断言同步;状态机
   只增态与边)——是:三份新合同+状态机边镜像断言守门。
7. 推进还是退回?推进——M5 收官(passed_with_conditions),进入 M6(Team、Delegate 和多 Agent 协作)。
