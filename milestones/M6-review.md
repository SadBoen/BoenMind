# M6 里程碑回看记录(基线 §19 门)

## Evaluation Record

```text
milestone_id:         M6(Team、Delegate 和多 Agent 协作)
build_or_commit_id:   bbe265d(备忘)→ b3aee3c(T0+T1 合同/持久层)
                      → 2ff1797(T2+T3 team.rs/接线/测试)→ 本提交(T4 回看)
test_run_id:          cargo test --workspace(2026-08-30,本机)
                      = 196 passed / 0 failed(M5 188 → M6 196;增量:
                      m6_team e2e 4、team.rs 单测 4、sync/delegation 用例)
log_range:            task.created 载荷 +parent_task_id(委派归因)、
                      task.member.removed 新事件(40 → 41)、grant.created
                      承载 per-task principal 授权链;GT-04 双场景由
                      m6_team e2e t90-t93 承载
deterministic_checks: validate.py 全绿(合同库 25 份工件;task.v0_1 放宽
                      parent/delegation_depth + GT-04,全 Minor);R2–R4
                      轨迹 4 条遍历;事件 41 断言与镜像同步
failure_tests:        t91 四门禁(深度>3 拒/预算超父剩余拒/授权越界拒/
                      并发>5 拒)、t92 成员故障不迁移 Task 状态+替换留痕、
                      t90 跨 Task principal 结构性隔离、t88(存量)非法 scope
replay_result:        GT-04 场景 A(建根→多成员→子任务委派→收集)与
                      场景 B(故障→替换)由 e2e 承载;GT-01/02/03 回归绿
llm_evaluation:       不适用(M8.7 起)
known_failures:       见 §6 条件与遗留
architecture_changes: 合同 Minor:task.v0_1(parent 放宽 + delegation_depth)
                      + 事件 41 + GT-04;SQLite v7(tasks 两列);C4 模型
                      零改动(实现既有拓扑);无新增 perf 指标(P-11 口径覆盖)
acceptance_decision:  passed_with_conditions(条件见 §6)
reviewed_at:          2026-08-30
```

## §5 逐门记录

- **A 功能测试**:M6.1 Team=成员编队+子任务树(members 多员化、
  parent_task_id 启用)、M6.2 成员授权(spawn 时授权链签发,prompt=Skill
  数据面)、M6.3 spawn(task_spawn_member/subtask,per-task principal)、
  M6.4 delegate 归因(parent_task_id + parent 哈希链 + operation 归因)、
  M6.5 四门禁、M6.6 collect(三要素)——全部有测试实证。
- **B 回归测试**:M1–M5 存量全绿(M5 coordinator 测试按 per-task
  principal 同步更新);GT-01/02/03 回放绿。
- **C 故障测试**:成员故障不迁移 Task 状态(t92)、替换留痕
  (member.removed)、跨 Task 隔离(t90)、门禁拒绝路径(t91)。
- **D 日志回放**:GT-04 双场景逐事件形态可回放;委派链经
  parent_task_id + grant.parent_hash 双链可上溯。
- **E 确定性评估**:授权子集算法(动词集 ⊆ + 谓词取交)单测;深度/
  预算/并发门禁边界单测;Task 合同 delegation_depth ≤3 schema 断言。
- **F LLM 评估**:不适用(M8.7 起)。
- **G 架构复盘**:机制进内核、策略留外围落地——四门禁在 spawn 强制点
  (L2),编队策略无内核代码;Team 不设独立合同对象(ADR-0002 语义:
  Task 成员关系即 Team);per-task principal 使子树裁剪成为结构性保证。
- **H 验收裁决**:passed_with_conditions。
- **I 性能冒烟**:无新增指标;P-01..P-11 存量口径不回退(全量回归绿,
  性能面由 M5 记录④承载)。

## 前置结算与承接项闭合

| 项 | 来源 | 状态 |
|---|---|---|
| ADR-0002 条件 5 余项(跨 Task 上下文真实场景) | ADR-0002 | ✅ 闭合:子任务声明可读父 memory:task:<id> 域(Broker memory.search);不做全量重建(条件原文禁止) |
| reservation 悬置裁决 | ADR-0002 未决分歧 | ✅ 裁定:延续不做(幂等键承担;M7 真实负载后复核) |
| per-task principal(M5 遗留) | M5-review §6 | ✅ 闭合:t90 结构性隔离 |
| Task 级停滞窗口可配置(M5 遗留) | M5-review §6 | 部分:合同默认值已定;Task 级配置字段随 M7(Task 配置面) |
| budget 子分配逐笔归因(M5 遗留) | M5-review §6 | ✅ 闭合:子任务预算门禁 + 各 Task 独立账本行 |

## S1–S10 相关项裁决

- S3(停滞/监护):M5 已实践;M6 无新增面(多成员归属判定随 M7 真实
  Agent 身份)。
- 其余 S 项维持 proposed,随 M7(Provider 进程)/M8(升级与发行)裁决。

## §6 条件与遗留

1. **M7 项**:成员 LLM Agent 自主回合循环(真实 Provider 后 worker 才是
   真正的 Agent);spawn/collect 的 Wire 面与 UI(M8 一起定);Task 级
   停滞窗口/并发上限配置字段;成员级 max_concurrent_tools 定标。
2. **M8 项**:Team Board UI;委派链可视化;删除权与委派产物级联。
3. **口径说明**:存活 worker 计数按 Task.members 现存 worker 角色;
   member.removed 为留痕事实,不回滚授权(Task 终态统一撤销)。
4. **CI 三平台**:推送后矩阵全绿确认(196 项)。

## §7 回看七问(基线)

1. 解决目标问题?是——多成员团队 + 委派链落地,四门禁使「委派受深度、
   预算和并发约束」成为强制点而非约定。
2. 旧能力可用?是——M1–M5 存量全绿;合同全 Minor;M5 测试按 per-task
   principal 同步后语义等价。
3. 崩溃/断线/重复执行?成员故障不迁移 Task 状态(t92);子任务预算
   spawn 时点校验(不做 reservation,记录在案);member.removed 墓碑
   不回滚授权,Task 终态统一撤销。
4. 日志能解释每一步?能——委派链 parent_task_id + grant.parent_hash
   双链可上溯;member.added/removed 留痕;collect 三要素齐备。
5. 结果被实际观察和核验?collect 聚合自 capability 事实流(非成员自述);
   观测/完成判定门禁(M5)对子任务同样生效。
6. 合同与状态模型稳定?是——全 Minor;delegation_depth 上限 3 入合同;
   事件 41 断言守门。
7. 推进还是退回?推进——M6 收官(passed_with_conditions),进入 M7
   (Provider、MCP 和 App 隔离)。
