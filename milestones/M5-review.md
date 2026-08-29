# M5 里程碑回看记录(基线 §19 门)

> 骨架:T10 收官时回填,当前占位值以 ⏳ 标注;静态内容(预期事件族、合同增发清单、
> 载体映射、前置结算落点)抄自 `milestones/M5-implementation-spec.md` §4/§7/§8 与
> ADR-0002/0004「条件与验收」,动态数值与裁决一律不预填。

## Evaluation Record

```text
milestone_id:         M5(Butler、Task 和长期监护)
build_or_commit_id:   ⏳ T10 回填
test_run_id:          ⏳ T10 回填
log_range:            task.* 事件族首度入流(32→40):task.created/task.state.changed
                      (from/to/reason)/task.member.added/task.budget.increased/
                      task.stalled/task.repeating/watchdog.reorchestration.
                      triggered(事实事件,守 G2 边界)/observation.recorded;
                      memory 调用复用 capability.invoked,零新增;GT-03 三场景
                      回放(A 主链路/B 监护链路/C 声称完成未生效)由 e2e 承载
deterministic_checks: ⏳ T10 回填
failure_tests:        ⏳ T10 回填
replay_result:        ⏳ T10 回填
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
acceptance_decision:  ⏳ T10 回填
reviewed_at:          ⏳ T10 回填
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
  墓碑)。实证:⏳
- **B 回归测试**:M1–M4 存量 134 项全绿(T9);GT-01/GT-02 回放在场;
  P-01..P-10 复跑劣化 < 25%。实证:⏳
- **C 故障测试**:「发现无进展/重复/持续错误」载体(规格 §7)= T7 注入测试:
  空转→repeating(3 次)、停滞→stalled(15min 默认)、硬顶→blocked(24h)、
  waiting_approval 豁免;mock_drift 四类注入(空转/重复/停滞/声称完成但未
  生效);预算硬限 → Agent 暂停 + Task blocked(budget_exhausted);T6c 落盘
  重启不再回满;过期 task_epoch 命令 Stale 拒绝留审计。实证:⏳
- **D 日志回放**:GT-03 场景 A(建 Task→Coordinator 规划→spawn Worker→
  副作用调用→声称完成→verification 核验→completed)/B(Worker 空转/停滞→
  Watchdog 检测→编排重启或 blocked→用户 resume→恢复)/C(声称完成但未生效
  证伪)逐事件形态可回放;task.* 八事件 + observation.recorded 归因链;
  validate.py R2–R4 轨迹遍历。实证:⏳
- **E 确定性评估**:「声称完成可核验」载体(规格 §7)= T8:verification
  失败不得 completed(GT-03 场景 C)、unverified → outcome_unknown 等用户、
  收据/查询优先于自述;子树裁剪决策矩阵(T4 随行);双路径收据合同一致
  测试进 CI(T3/T5)。实证:⏳
- **F LLM 评估**:不适用(M8.7 起)。
- **G 架构复盘**:实现既有拓扑,非架构变更(C4 ButlerPaths/TaskFlow 已含
  全部组件);Task 规范状态 L2 唯一持有、Butler 内存与 Task Board 仅投影;
  Watchdog「仅监督,不推断」+ G4 守护(产物无命令形状);Broker 为预算
  唯一执行点;Butler 无内核特权、协调权可撤销。实证:⏳
- **H 验收裁决**:⏳(前置结算闭合见下表;条件与遗留见 §6)。
- **I 性能冒烟**:P-01..P-10 复跑(门:劣化 < 25%);P-11 首填(Task Board
  投影重建延迟,ADR-0004 未决分歧 4 的首个定量回应,数值 T9 回填);
  Watchdog 扫描开销随 P-11 一并观测(规格 §9)。实证:⏳

## M5 前置结算条件逐条闭合表(settlement → 闭合状态)

| 条件 | 来源 | 落点 | 闭合状态 |
|---|---|---|---|
| 编排重启触发者 + 停滞窗口上限 | ADR-0004 条件 6 / PENDING D-M2-2 | 规格 §5.2;T7;基线 §10.3 已补定义;T10 闭合 | ⏳ |
| 预算包络二分(两级账本/子分配/扩容仅用户/Broker 唯一执行点) | ADR-0002 要点 5 / M4 §9 | 规格 §5.5;T6 | ⏳ |
| 协调动词子树裁剪 | ADR-0002 条件 2 余项 | 规格 §5.4;T4 | ⏳ |
| Memory Service 接口 | ADR-0002 条件 5 余项 | 规格 §5.8;T8(接口面闭合) | ⏳ |
| 双路径统一合同启用 | ADR-0002 条件 4 | 规格 §5.4;T3/T5(一致性测试进 CI) | ⏳ |
| forever scope 收紧 | M4-review §6.4 | 规格 §8-5;T3 | ⏳ |
| T6c 收紧(count 余量/幂等收据落盘) | M4-review §6.3 | 规格 §5.5;T1/T6 | ⏳ |
| 投影重建延迟定量(ADR-0004 未决分歧 4) | 首个数值 | P-11;T9 回填 | ⏳ |

**裁决(T10):⏳——预期(规格 §5.2/§5.4):8 项闭合后 ADR-0004 条件 6 正式
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
- 本回看逐条裁决结果:⏳

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
7. **CI 三平台确认**:推送后矩阵全绿(存量 134 + M5 增量):⏳

## §7 回看七问(基线)

1. 解决目标问题?(M5 通过条件四条:Butler 只有协调权限/Task 状态不依赖
   Butler 内存/发现无进展、重复动作和持续错误/声称完成的结果可实际观察
   核验)——⏳
2. 旧能力可用?(M1–M4 存量 134 项全绿;合同全 Minor 增发零破坏)——⏳
3. 崩溃/断线/重复执行?(编排重启触发者恰两类;task_epoch Stale 拒绝;
   T6c 落盘重启不回满;幂等键续跑)——⏳
4. 日志能解释每一步?(task.* 八事件 + watchdog 事实事件 +
   observation.recorded;归因链含 task_id/operation_id)——⏳
5. 结果被实际核验?(verification 收据/查询优先于模型自述;完成判定门禁
   guard verified_completion)——⏳
6. 合同与状态模型稳定?(全 Minor 增发;sync.rs 事件数断言同步;状态机
   只增态与边)——⏳
7. 推进还是退回?(M5 收官 → M6)——⏳
