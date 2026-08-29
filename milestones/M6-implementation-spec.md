# M6 实现规格 v1.0(实现者自主冻结)

> 第 2 层工件:M6(Team、Delegate 和多 Agent 协作)的技术栈、合同增发清单与
> 任务分解。地位在基线(第 0 层)与合同库(第 1 层)之下;冲突以上两层为准。
> 上游输入:基线 §18-M6(六子项)、§11、ADR-0002(条件 5 余项/reservation
> 悬置)、ADR-0004(三层归属:M5 已闭合条件 6)、M5 回看遗留账本。
> 状态:**v1.0(2026-08-30 实现者自主冻结)**。沿用治理变更:技术规格不送
> 用户评审;开放裁决点按 §9 默认路径执行,记 PENDING 供事后知情。

## 1. 范围与形态裁定

基线 §18-M6 六子项全量。核心形态:**把 M5 的「管家 + 单 Worker」推广为
「多成员团队 + 委派链」**。架构定性(§2.3 六问测试):机制进内核、策略留
外围——成员关系/深度/并发/归因为 L2 规范约束(强制点在内核),编队策略/
成员 prompt/结果呈现为 Coordinator 可替换行为(数据与 Agent 层,不进内核)。
C4 模型零改动(M6 为实现既有拓扑:executor 的 Task/Team 编排 + TaskFlow
视图早有位置)。

| 子项 | M6 交付 | 说明 |
|---|---|---|
| M6.1 Team 定义 | Team = Task 的成员编队(members 多员化 + 子任务树);不设独立 Team 合同对象(ADR-0002:Team 是产品理解,内核中是 Task 成员关系) | task.v0_1 增量:parent_task_id 启用、delegation_depth 字段 |
| M6.2 成员 prompt 和工具授权 | 成员角色声明(name + prompt 模板引用 + allowed capabilities)随 spawn 落为 Grant 集;prompt = Skill(数据,加载不改权限) | 授权签发链 M5 已建,M6 泛化到多成员 |
| M6.3 Agent spawn | task_spawn_member(多 Worker)+ spawn_subtask(子任务委派);per-task principal 命名空间 | 命令为内核 API(编排器/测试面);wire 面随 M8 UI |
| M6.4 delegate 归因链 | task.created 载荷增 parent_task_id;子任务/成员调用与父 Operation/父 Task 归因可上溯 | 归因是审计合同,走事件注册表 |
| M6.5 预算、深度和并发上限 | 委派深度 ≤ 3(根 Task 为 0,子任务 +1);单 Task 成员并发 ≤ 5;子任务预算 ≤ 父包络剩余;authorization ⊆ 父(只减不增) | 深度/并发/子集校验在 spawn 强制点(L2) |
| M6.6 结果收集和报告 | task_collect:聚合成员结果(来源 agent/状态/关联 Operation);成员故障不破坏 Task(替换 spawn) | 结果以 capability.invoked/observation 事实为源,非自述 |

非目标:跨 Task 记忆自动提炼(默认不自动写长期记忆不变)、成员 Agent 的
LLM 自主回合循环(M7 真实 Provider)、Team 持久化 UI(M8)、reservation
(§9 结算延续不做)。

## 2. 技术栈

无新增外部依赖。复用 M1–M5 既有栈(tokio 单写者、SQLite v6→v7 expand、
JSONL 事件、axum、clap)。

## 3. 仓库结构(增量)

```text
runtime/crates/bm-core/src/
  team.rs              # 成员编队:spawn_member/spawn_subtask/collect、深度与
                       # 并发门禁、authorization 子集校验、per-task principal
runtime/crates/bm-persist/src/sqlite_state.rs
                       # v7:tasks.parent_task_id/delegation_depth 列
runtime/crates/bm-testkit/tests/m6_team.rs
boenmind-contracts/    # 增发清单见 §4(Minor,只增)
```

## 4. 合同增发清单(Minor,只增不破)

1. `task/task.v0_1.schema.json`:parent_task_id 由 const null 放宽为
   task 引用(M6 启用,原预留兑现);增可选字段 delegation_depth
   (integer ≥ 0,根 Task = 0)。member 对象不变。
2. `registry/runtime-events.v0_1.json`:task.created 载荷增
   parent_task_id("id|null";40 → 40 条,载荷键集 +1);新增
   task.member.removed(成员移除/替换事实;41 条)。sync.rs 事件数断言
   40 → 41 同步。
3. `golden-traces/M6-GT-04-team-delegate.md`:场景 A(建根 Task → 多成员
   spawn → 子任务委派 → 结果收集)、场景 B(成员故障 → 替换 → Task 不倒);
   validate.py R2–R4 自动覆盖。
4. `m0/perf-baseline.v0_1.md`:无新增指标(P-11 口径覆盖 task.* 折叠)。

M1–M5 冻结字段零改动;上述全部为放宽预留枚举/新增可选字段/新事件/新
轨迹,均为 Minor。

## 5. 关键设计决策

### 5.1 per-task principal 命名空间(M5 遗留承接) **[M6.3]**

- Coordinator/Worker principal 由共享命名空间升级为
  `agent:coord:{task_id}` / `agent:worker:{task_id}`:跨 Task 访问在
  Grant 查表层结构性不命中(默认拒绝),子树裁剪从「构造性」升级为
  「结构性」。intersection_grants 签名泛化(audience 由调用方按 task 定)。
- M5 语义保持:Coordinator Grant parent = Butler bootstrap(上界链),
  Worker Grant parent = Coordinator capability.call Grant。

### 5.2 委派 = 子任务(spawn_subtask) **[M6.3/M6.5]**

- 委派链以子任务表达:child = Task{parent_task_id, delegation_depth =
  parent+1},created_by = 父 Coordinator principal。基线 delegation_depth=0
  (Grant 不可再转授)不破——转授禁止的是**能力授权**;任务分解走子任务,
  每层受深度上限约束。
- 强制点(全部在 spawn,即 L2):①深度 ≤ 3;②child.authorization 的动词
  集 ⊆ parent.authorization(只减不增,资源谓词取交);③child.budget 的
  max_tool_calls ≤ 父包络剩余(used 为共享记账:子任务调用计入父包络);
  ④并发:单 Task 存活成员(spawn 后未终态)≤ 5。
- 子任务完成回报父:child 终态时 observation 记录,父 Task 的 collect 可见。

### 5.3 多成员与故障隔离(M6.3/M6.6) **[M6.3/M6.6]**

- task_spawn_member:再签发一枚 Worker 成员(授权同 §5.1 链),成员并发
  门禁先行。成员故障 = 其调用 error,不迁移 Task 状态;替换 = 再次 spawn。
- worker 调用结果入 World.task_results((agent, operation, capability,
  state, summary)),task_collect 聚合输出——团队结果有**来源**(agent_id)、
  **状态**(operation state)、**关联 Operation**(operation_id),满足基线
  M6 通过条件第 4 条。

### 5.4 前置结算 **[ADR-0002 条件 5 余项/reservation]**

- 跨 Task 上下文传递(条件 5):以 memory 域显式授权为主——子任务声明
  可读父任务 memory:task:<parent_id> 域(经 memory.search 走 Broker);
  不做 Coordinator 全量重建(条件原文禁止)。M5 接口面自此有真实场景。
- reservation:延续不做(M4 §9/M5 §8-8)——M5 实测回执丢失场景由幂等键
  承担;M7 真实负载数据出来后再裁。

## 6. 任务分解与顺序

```text
T0  合同增发:§4 清单(task 放宽/事件载荷 +1/新事件/GT-04;validate.py
    全绿为闸;sync.rs 41 断言)
T1  bm-persist v7:tasks.parent_task_id/delegation_depth 列;TaskCreated
    物化写 parent;task_from_row/合同载荷带 parent/depth
T2  team.rs:per-task principal、intersection_grants 泛化、spawn_member/
    spawn_subtask(四门禁)、collect 聚合;task.create 接线(深度继承/
    authorization 子集/预算子分配)
T3  测试:m6_team.rs(t90-t95)——多成员/隔离/委派链四门禁/故障替换/
    collect 归因;存量同步(GT-03 parent 恒 null 用例切换)
T4  全量回归 + 性能复跑(P-11 口径)→ §19 回看:结算 + AGENTS.md +
    tag m6-team-delegate
```

依赖:T0 → T1 → T2 → T3 → T4。合批:T1+T2 一轮、T3 一轮、T4 一轮
(沿用提速方案)。

## 7. 验收面

| 通过条件(基线) | 载体 |
|---|---|
| 成员权限只减不增 | spawn_subtask authorization 子集校验(动词集 ⊆、谓词取交);Grant 链 parent 哈希 |
| 委派受深度、预算和并发约束 | 四门禁测试:深度 >3 拒、预算超剩余拒、并发 >5 拒、越权动词拒 |
| 成员故障不会破坏整个 Task | t92:worker 连续失败 → Task 保持 running → 替换 spawn → 新成员成功 |
| 团队结果有来源、状态和关联 Operation | t93 collect 聚合:每条结果含 agent_id/state/operation_id |

## 8. 合同解读条款(实现期裁决,回看复核)

1. **Team 对象不独立设合同**(§1):Task.members + 子任务树即 Team;
   独立 Team 对象若 M7/M8 出现真实需求再以 Minor 增发。
2. **委派 = 子任务**(§5.2):Grant delegation_depth 恒 0 语义不变;
   深度上限约束的是任务分解层级。成员「替换」= 新 spawn + 旧成员事实
   留痕(member.removed),墓碑语义与 Task 一致。
3. **task.created 载荷 +1 键**(§4-2):事件注册表载荷键集 Minor 追加,
   镜像断言同步;旧轨迹(GT-03)无该键仍合法(envelope 不校验载荷键)。
4. **spawn/collect 为内核 API**(§1):wire 面随 M8 审批/UI 一起定
   (编排命令的 Surface 形态属产品体验)。
5. **并发上限口径**(§5.2):按 Task 存活成员数(worker 角色计)≤ 5,
   合同默认;成员级 max_concurrent_tools 随 M7 真实并发负载定标。

## 9. 裁决定案(2026-08-30,原开放项)

- **reservation**:延续不做(幂等键承担回执丢失判定;M5 实测支撑),
  M7 真实负载后复核。
- **跨 Task 上下文(ADR-0002 条件 5 余项)**:以 memory 域显式授权为主
  (子任务声明可读父域,经 Broker memory.search),不做全量重建;
  条件自本回看闭合。
- 无需用户预裁决项;产品体验类(spawn/collect 的 UI 形态)记 PENDING,
  默认路径继续。
