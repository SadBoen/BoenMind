# M5 实现规格 v1.0(实现者自主冻结)

> 第 2 层工件:M5(Butler、Task 和长期监护)的技术栈、合同增发清单与任务分解。
> 地位在基线(第 0 层)与合同库(第 1 层)之下;冲突以上两层为准。
> 上游输入:基线 §18-M5(七子项)、§9.7/§10/§11/§20、ADR-0002(条件 2/5 余项
> 与预算二分)、ADR-0004(条件 6 与 D-M2-2)、M4 回看遗留账本(forever 收紧、
> T6c 收紧、复核项)、M4 规格结构范本。
> 状态:**v1.0(2026-08-29 实现者自主冻结)**。沿 M4 治理变更:技术规格不送
> 用户评审;开放裁决点按 §9 默认路径执行,全部记入 PENDING 供事后知情。

## 1. 范围与形态裁定

基线 §18-M5 七子项全量。核心形态变化:**系统从「单 Agent 闭环 + 受管辖调用
面」长出第一套持久编排对象**——Task(L2 规范状态)、Butler(内置协调 App)、
Coordinator/Worker(受限协调 Agent 与成员 Agent)、Watchdog/Observation
(长期监护闭环)。C4 模型(ButlerPaths/TaskFlow 视图)已含全部组件,本里程碑
为**实现既有拓扑,非架构变更**。多成员 Team/深度并发约束归 M6;真实外部
Provider 归 M7。

| 子项 | M5 交付 | 说明 |
|---|---|---|
| M5.1 Butler 作为内置 App | Butler 以普通 App 身份注册;协调权物化为 bootstrap Grant 集(审计可溯、可撤销);领域权默认拒绝有结构断言 | ADR-0002:仅持协调权;无内核特权(§5.3) |
| M5.2 Task 创建和生命周期 | Task 规范对象归 L2(SQLite v4)+ task 状态机(7 态)+ task_epoch 写入门禁 + task.* 事件族 | ADR-0004 三层归属;task:<id> scope 自此启用(§5.1/§5.4) |
| M5.3 Coordinator Agent | 三方交集物化为 task:<id> Grant;safe/mutation 二分;子树裁剪 Broker 强制;成员授权签发链(parent_grant_hash 启用) | ADR-0002 条件 2 余项闭合(§5.4) |
| M5.4 Task Board Projection | 投影自事件日志重建(event_seq 绑定,确定性);Wire task.list/get + CLI task 组 | ADR-0004:投影可弃可重建;P-11 定标(§5.2) |
| M5.5 watch / pause / resume / stop | task 级生命周期命令(wire 六方法);agent 状态机增 paused 态;跨重启恢复 | 触发者之一 = 用户显式 resume(§5.2) |
| M5.6 Watchdog 和 Observation Log | 监护检测面(基线 §20 八项)→ 8 态分类;停滞窗口与编排重启触发者(前置结算);G4 守护 | ADR-0004 条件 6 兑现(§5.2/§5.6) |
| M5.7 记忆作用域与 memory.* | memory.write/search/delete 三能力经 Broker;四作用域即权限边界;FTS5 检索;不自动写长期记忆 | ADR-0002 条件 5 余项闭合(§5.8) |

非目标:多成员 Team/编队与 delegate 归因链(M6.1/6.3/6.4)、深度与并发上限
(M6.5,spawn 单成员闭环除外)、真实 Provider/MCP(M7)、向量检索(接口预留)、
用户删除权全量与备份(M8)、审批 UI 形态(M8)、独立进程 Orchestrator(M7+
复测)、预算 reservation(M4 §9 决定延续,见 §9)。

## 2. 技术栈

无新增外部依赖。复用 M1–M4 既有栈:tokio 单写者核心循环、SQLite(bm-persist,
expand-contract v3→v4,FTS5 为 SQLite 内建模块,rusqlite bundled 构建已含,
若构建特征缺失则启用同一依赖的 feature)、JSONL 事件日志、axum Wire 端点、
clap CLI。Watchdog 为核心循环内的节流扫描任务(时间经 MockClock 注入),不引
入独立线程/进程;Butler/Coordinator/Worker 均为同进程内 App/Agent 对象
(三权分立与 L2 边界不因角色增多而破——审批、预算、审计仍经同一 Broker 与
单写者循环)。

## 3. 仓库结构(增量)

```text
runtime/crates/bm-core/src/
  task.rs              # Task 规范对象、task 状态机推进、task_epoch 门禁接线
  butler.rs            # Butler App 注册、bootstrap 协调权 Grant 集、task.create 入口
  coordinator.rs       # Coordinator:三方交集物化、safe/mutation 二分、成员授权签发
  budget.rs            # Task/Agent 两级账本、包络子分配、扩容受控变更、Broker 执行点
  watchdog.rs          # 监护扫描:停滞/重复/持续错误检测、8 态分类、重启触发事实事件
  observation.rs       # verification 钩子消费、Observation Log 记录、完成判定门禁
  memory.rs            # memory.* 能力实现(FTS5 检索、作用域边界、级联失效最小面)
runtime/crates/bm-providers/src/
  builtin.rs           # 增 memory.* 三能力 manifest(system.* 五能力不变)
  mock_drift.rs        # 监护注入 mock:空转/重复/停滞/声称完成但未生效(GT-03 场景 B/C)
runtime/crates/bm-persist/src/
  sqlite_state.rs      # v4 expand:tasks / task_budget_ledger / observations /
                       # memories 表 + T6c 收紧(grant 计数消费、幂等收据落表)
runtime/crates/bm-wire/ # task.* 六方法 + events.poll task_id 过滤
runtime/crates/bm-cli   # task create/list/show/pause/resume/stop 命令组
boenmind-contracts/     # 增发清单见 §4(Minor,只增)
```

## 4. 合同增发清单(Minor,只增不破)

1. `task/task.v0_1.schema.json`:Task 规范对象——task_id、title、goal(脱敏
   摘要)、state、created_by、task_epoch、authorization(Task 授权:协调动词
   白名单 + 资源谓词,三方交集的 Task 分量载体)、budget 包络(Budget 对象
   键值对)、deadline、members[]、parent_task_id(恒 null,M6 预留)、
   created_at/updated_at。
2. `registry/runtime-events.v0_1.json` 增发 8 项(32 → 40;sync.rs 事件数
   断言同步):task.created / task.state.changed(from/to/reason)/
   task.member.added / task.budget.increased(用户批准扩容的事实)/
   task.stalled / task.repeating / watchdog.reorchestration.triggered(事实
   事件:监护已触发编排重启,形态守 G2 边界)/ observation.recorded。
   memory 调用复用 capability.invoked,零新增。
3. `wire/task.v0_1.schema.json`:task.create / task.list / task.get /
   task.pause / task.resume / task.stop 六方法 params/result;watch 观察复用
   events.poll,增发可选 task_id 过滤参数(wire/session.v0_1 只增字段)。
4. `wire/envelope.v0_1.schema.json`:method 枚举 +6;error_code 零新增
   (budget_exceeded/outcome_unknown 已有;注册表 available_since=M5 备注同步)。
5. `state-machines/core-transitions.v0_1.json`:machines 增 task(7 态:
   created/running/paused/blocked/completed/failed/cancelled;边集见 §5.1);
   agent 状态机增 paused 态与四边(running→paused、paused→running、
   paused→stopping、paused→cancelled;补齐 C4 executor 文字与合同的既有落差)。
6. `memory/memory-entry.v0_1.schema.json`:记忆条目——scope
   (memory:app:<app>|task:<id>|agent:<id>|user)、content 或 content_ref、
   source 标注(trust 分级复用 §4.5)、content_hash、created_at、
   correction_of(用户纠正覆盖载体,可空)。
7. `logs/observation-log-entry.v0_1.schema.json`:Observation Log 条目——
   声称摘要(claim_ref)、观察证据(证据事件 event_seq/收据引用)、结论态
   (基线 §20 八态)、verdict(verified/unverified/failed)、关联
   task_id/operation_id、timestamp。四类记录合同的第二块(M1 已有
   execution-log-entry)。
8. `golden-traces/M5-GT-03-task-lifecycle.md`:GT-03 场景 A(主链路:建
   Task→Coordinator 规划→spawn Worker→副作用调用→声称完成→verification
   核验→completed)与场景 B(监护链路:Worker 空转/停滞→Watchdog 检测→
   编排重启或 blocked→用户 resume→恢复)。validate.py R2–R4 轨迹遍历自动覆盖。
9. `m0/perf-baseline.v0_1.md` 增 P-11(Task Board 投影重建延迟)定标骨架,
   数值 T9 回填(ADR-0004 未决分歧 4 的首个定量回应)。

M1–M4 已冻结字段与事件零改动;capability/manifest 的 verification 钩子
(query/expect/within_ms)M4 已结构化预留,M5 启用消费,零合同改动;
capability/grant 的 task:<id> scope 枚举值 M4 已合法保留,启用产生,零改动。
上述全部为新增文件/新增枚举值/新增状态与边,均为 Minor。

## 5. 关键设计决策

### 5.1 Task 规范对象与状态机(L2 唯一持有)**[ADR-0004 三层归属]**

- Task 规范状态、生命周期、成员关系、预算与截止时间唯一由 L2 持有(SQLite
  v4 tasks 表 + task.* 事件族);Butler 内存与任何 Task Board 仅为投影。
  写入门禁复用 M2 epoch 基建:task_epoch 递增 + CAS 条件写入 + 命令租约,
  过期 epoch 命令 Stale 拒绝并留审计事件(M2 已有机制,挂接 Task 对象域)。
- task 状态机(7 态):created→running(task_started);running⇄paused
  (task_paused/task_resumed);running→blocked(budget_exhausted |
  stall_hard_limit | outcome_unknown_pending);blocked→running
  (user_resolved);running/paused→completed(**guard verified_completion**
  ——无 Observation 核验不得完成,§5.7);running/paused→failed
  (verified_failure);三态→cancelled(task_cancelled)。
- waiting_approval 不入 task 状态机(它是 Operation/Agent 级);Task 层以
  监护态 waiting_approval 呈现于投影,状态机保持 running。
- 成员关系:task.member.added 为持久事实;成员删除以 L2 墓碑为准。

### 5.2 编排重启触发者与停滞窗口(前置结算 1)**[ADR-0004 条件 6 / D-M2-2]**

- **触发者恰为两类**:①用户显式 resume(任意 Surface 的 task.resume);
  ②Watchdog 自动触发——停滞判定成立后,Watchdog 持久发布事实事件
  watchdog.reorchestration.triggered,由编排器(Butler/Coordinator)消费后
  从最近一致的持久状态与决策事件重新推理;Watchdog 与 Runtime 监督层均不
  推断编排下一步(「仅监督,不推断」边界不变,M2 语义延续)。
- **停滞窗口数值(合同默认,Task 可配置)**:
  - 停滞判定:无进展信号(无新事件、无心跳更新、无 Operation 状态变化)
    持续超 stalled_after = 15 分钟 → 监护态 stalled + 触发②;
  - 重复判定:连续 repeat_threshold = 3 次同工具 + 同参数哈希 + 同错误
    (或同结果)→ 监护态 repeating;
  - 硬顶:自最近进展起累计超 stall_hard_limit = 24 小时不再自动重启,
    Task 转 blocked(budget_exhausted 同路)等待用户裁定,防无限空转烧预算;
  - waiting_approval 态豁免自动重启(等的是人,不是机器);
  - Watchdog 扫描周期 watchdog_tick = 60 秒。
  测试经 MockClock 注入短窗口(秒级),数值仅合同默认。产品数值属体验,
  大白话记 PENDING D-M5-1 供用户随口改判,机制不阻塞。
- 基线 §10.3 已按本节补入 ADR-0004 条件 6 的定义(「条件要求在基线补充
  定义」的兑现动作);M5 回看时 ADR-0004 条件 6 正式闭合。

### 5.3 Butler 内置 App 与协调权物化 **[M5.1;ADR-0002 要点 1]**

- Butler 以普通 App 身份注册(App Registry),不持任何内核特权;其全部
  调用与其他调用方同构走 Broker,守护测试断言无旁路(G3 同款逆断言)。
- 协调权物化:系统引导期将 §10.1 协调动词清单(task.create/task.cancel/
  agent.spawn/agent.pause/agent.resume/agent.stop/agent.watch/team.create/
  capability.discover/event.subscribe/task.collect)签发为 butler principal
  的 bootstrap Grant 集(issued_by=runtime_bootstrap,scope=forever,审计
  可溯);**用户可撤销**——撤销后 Butler 仅剩只读查询,重授走审批。
  领域动词(mail.read 等)不在清单,默认拒绝有结构断言(权限矩阵测试)。
- task.create 请求携带 Task 授权声明(§4-1 authorization 字段),作为
  三方交集的 Task 分量;Butler 不得签发超出自身 Grant 上界的授权。

### 5.4 Coordinator 与成员授权链 **[M5.3;ADR-0002 条件 2 余项]**

- Coordinator 为受限 Agent 类型:权限 = Butler 可授协调权 ∩ Task 授权 ∩
  用户授权,默认拒绝、Task 结束即失效。三方交集物化为 task:<id> scope 的
  Grant(M4 预留枚举自此启用;M4 解读条款 4 的 validation_failed 仅保留给
  引用不存在 Task 的情形)。
- 协调权二分:safe_coordination(只读查询/状态查询/结果收集,默认继承)
  与 mutation_coordination(cancel/pause/stop/spawn/team.create,须在 Task
  授权中显式列出)。子树裁剪在 Broker 调用点强制:协调动词作用于子树外
  目标一律 permission_denied(决策矩阵测试覆盖)。
- 成员角色授权由 Coordinator 签发,parent_grant_hash 链回溯至 Coordinator
  自身 Grant,逐级不得超过上界;delegation_depth 恒 0 语义 = Agent 签发的
  Grant 不可再转授(M4 冻结字段启用链,零合同改动)。
- M5 单成员 Worker 闭环(Notes 演示域):Coordinator spawn 一个 Worker、
  签发 task:<id> Grant、收集结果——多成员编队/深度/并发约束归 M6。
- 双路径自 M5 双开(直接 Capability / Domain Worker):统一合同 M4 已冻结
  (Grant/幂等/脱敏/收据),收据增来源标注;「同一能力双路径收据合同一致」
  测试进 CI——ADR-0002 条件 4 的双开禁止自此有合同前提地解除。

### 5.5 Task 预算包络(前置结算 2)**[ADR-0002 要点 5;§9.7]**

- Agent/Task 两级账本:成员用量逐笔记账并同步累计至所属 Task 包络;
  软限 80% → budget.warning,硬限 → Agent 暂停 + Task blocked
  (budget_exhausted)请求用户裁定。
- 包络内子分配允许、扩容禁止:Coordinator 可在包络内向成员子分配(逐笔记
  账),不可扩容;扩容仅用户批准(task.budget_increase → Approval → 批准
  后包络更新 + task.budget.increased 事实事件)。Broker 为唯一执行点:
  「工具调用前」强制点收编进 Broker 决策段,守护测试断言绕过 Broker 无
  预算执行出口。
- T6c 收紧兑现(M4 遗留):count 类 Grant 消费余量与幂等收据仓自内存态
  落入 SQLite v4(grant 计数消费列 + 幂等收据表),重启不再回满。

### 5.6 Watchdog 与长期监护 **[M5.6;基线 §20]**

- 检测面(基线 §20 八项):状态变化核验、重复动作、无进展、完成后仍执行、
  声称成功无可核验结果、副作用结果未知、超预算/截止/并发、Provider 不可用
  仍持续等待——M5 全部落检测,其中副作用未知/Provider 等待复用 M2/M4 既有
  信号(outbox pending、binding unavailable)。
- 监护 8 态分类(progressing/completed/failed/stalled/repeating/
  waiting_approval/outcome_unknown/interrupted)随扫描产出,写入
  Observation Log 与事件;监护态非 task 状态机态,投影呈现。
- **G4 守护(新增,常驻 CI)**:监督/监护层类型面无编排命令出口——
  Watchdog 产出仅限事实事件与 Observation 记录,不出现命令形状
  (G1 命令语义检测同款断言反向覆盖 watchdog 产物);配合 M2「仅监督不
  推断」边界,防隐形第二协调器。

### 5.7 Observation 核验与完成判定 **[基线通过条件第 4 条]**

- Command 类能力的 verification 钩子(query/expect/within_ms)在执行收据
  后由 Observation 消费:外部系统查询/文件状态/Provider 收据/确定性断言
  优先于模型自述;核验结果写 observation.recorded 与 Observation Log
  (四类记录合同的第二块落地)。
- 完成判定门禁:Task completed 必须 guard verified_completion——声称完成
  但 verification 失败/无 verification 且无收据 → 结论 unverified,Task
  转 outcome_unknown 挂起(blocked 路)或由用户裁定,禁止自动标成功。
  GT-03 场景 C 注入「声称完成但未生效」证伪。
- 独立 Judge(M8.7 起)接口预留不实现;Observation 判定 M5 全确定性。

### 5.8 memory.* 与作用域边界 **[M5.7;ADR-0002 条件 5 余项]**

- memory.write / memory.search / memory.delete 三能力以内置 Provider 注册
  (manifest 走 M4 合同,risk=read-only/low-risk 按 manifest 声明),全部
  经 Broker,作用域即权限边界:memory:app:<app> / memory:task:<id> /
  memory:agent:<id> / memory:user(写入需显式授权,走审批)。
- 检索 FTS5(SQLite 内建);向量引擎可替换,接口预留不承诺。
- 默认不自动写长期记忆;Task 结束时的提炼由所属 App 显式声明;用户纠正
  优先级最高(覆盖而非追加,correction_of 字段);来源被删除时记忆级联
  失效(M5 最小面:scope 内删除落墓碑,回放不长回;全量删除权归 M8)。
- Memory Service(跨 Task 上下文传递)接口面闭合:memory:task:<id> 归档
  策略 + memory:user 显式授权;真实跨 Task 场景随 M6 团队语境复用。

## 6. 任务分解与顺序

```text
T0  合同增发:§4 清单 1–9(GT-03 骨架、sync.rs 镜像更新、validate.py 全绿
    为闸)
T1  bm-core task.rs + SQLite v4:Task 对象、task 状态机 7 态 11 边、
    task_epoch 门禁接线、task.* 事件族、T6c 两表落地
T2  Wire task.* 六方法 + events.poll task_id 过滤 + CLI task 组 +
    Task Board 投影重建(event_seq 绑定、重建确定性测试、P-11 骨架)
T3  butler.rs:Butler App 注册、bootstrap 协调权 Grant 集、领域权默认拒绝
    权限矩阵、task.create 全链路、双路径收据来源标注启用
T4  coordinator.rs:三方交集物化(task:<id> Grant 启用)、safe/mutation
    二分、子树裁剪 Broker 强制、成员授权签发链(parent 哈希)、决策矩阵
    测试随行
T5  Worker 协调链闭环:单成员 spawn、Notes 演示域、协调动词全链路经
    Broker、子树外默认拒绝、GT-03 场景 A 回放
T6  budget.rs:两级账本、包络子分配、扩容受控变更(用户批准)、Broker
    预算执行点收编、软硬限两态、T6c 消费切换
T7  watchdog.rs:八项检测面、8 态分类、停滞窗口/硬顶/waiting_approval
    豁免(MockClock 注入)、编排重启触发者两类、G4 守护、GT-03 场景 B
T8  observation.rs + memory.rs:verification 消费、完成判定门禁(GT-03
    场景 C 注入)、Observation Log 合同落地、memory.* 三能力与四作用域、
    FTS5 检索、纠正覆盖与级联墓碑
T9  全量回归(134 项存量全绿)+ 性能回填(P-11 + P-01..P-10 复跑劣化
    < 25%)+ GT-03 三场景回放
T10 §19 回看:前置结算条件逐条闭合(ADR-0004 条件 6;ADR-0002 条件 2/5;
    M4 遗留 forever 收紧/T6c 复核)+ S1–S10 相关项裁决 + AGENTS.md 进度 +
    tag m5-butler-task
```

依赖:T0 → T1 → T2 → T3 → T4 → T5 → T6 → T7 → T8 → T9 → T10。
T1 起每任务带新测试先行(沿用 M2–M4 测试先行纪律;M4 测试先行共抓出
5 个真 bug,纪律延续)。

## 7. 验收面

基线 M5 通过条件:Butler 只有协调权限;Task 状态不依赖 Butler 内存;长期
任务能发现无进展、重复动作和持续错误;Agent 声称完成的结果能够被实际观察
和核验。载体映射:

| 通过条件 | 载体 |
|---|---|
| Butler 只有协调权限 | T3 权限矩阵:协调动词清单恰为 §10.1 集、领域动词默认拒绝、无旁路(G3 逆断言)、bootstrap Grant 可撤销 |
| Task 状态不依赖 Butler 内存 | T2/T3:清空 Butler 内存态与投影缓存后 task.list 行为逐字节一致;Runtime 重启后 Task 恢复(M2 底座)+ 投影重建确定性;损坏投影缓存无行为差异(等价混沌,单进程映射见 §8-2) |
| 发现无进展/重复/持续错误 | T7 注入测试:空转→repeating(3 次)、停滞→stalled(15min 默认)、硬顶→blocked(24h)、waiting_approval 豁免 |
| 声称完成可核验 | T8:verification 失败不得 completed(GT-03 场景 C);unverified → outcome_unknown 等用户;收据/查询优先于自述 |

前置结算与遗留条件落点(M5 回看逐条裁决闭合):

| 条件 | 来源 | 落点 |
|---|---|---|
| 编排重启触发者 + 停滞窗口上限 | ADR-0004 条件 6 / PENDING D-M2-2 | §5.2;T7;基线 §10.3 已补定义;T10 闭合 |
| 预算包络二分(两级账本/子分配/扩容仅用户/Broker 唯一执行点) | ADR-0002 要点 5 / M4 §9 | §5.5;T6 |
| 协调动词子树裁剪 | ADR-0002 条件 2 余项 | §5.4;T4 |
| Memory Service 接口 | ADR-0002 条件 5 余项 | §5.8;T8(接口面闭合) |
| 双路径统一合同启用 | ADR-0002 条件 4 | §5.4;T3/T5(一致性测试进 CI) |
| forever scope 收紧 | M4-review §6.4 | §8-5;T3 |
| T6c 收紧(count 余量/幂等收据落盘) | M4-review §6.3 | §5.5;T1/T6 |
| 投影重建延迟定量(ADR-0004 未决分歧 4) | 首个数值 | P-11;T9 回填 |

## 8. 合同解读条款(实现期裁决,回看复核)

1. **停滞窗口数值**(§5.2):15 分钟 / 3 次 / 60 秒 / 24 小时为合同默认,
   Task 可配置;数值属产品体验,大白话记 PENDING D-M5-1 供用户随口改判,
   机制先行不阻塞。
2. **单进程形态的「Orchestrator 崩溃」等价映射**(§7):沿用 M2 先例——
   清空编排器内存态 + Runtime 进程重启恢复 + 投影重建确定性 + 损坏投影
   缓存无行为差异;真实独立进程形态随 M7 Provider 外置 / M8 部署定标复测。
3. **task:<id> scope 启用**(§5.4)= M4 解读条款 4 的兑现;validation_failed
   仅保留给引用不存在 Task 的情形;M4 决策矩阵中 task scope 用例同步切换。
4. **delegation_depth 恒 0 的链式语义**(§5.4):任何 Agent 签发的 Grant
   不可再转授;parent_grant_hash 链保证审计可上溯,逐级不超上界;字段值
   仍恒 0,M4 冻结合同零改动。
5. **forever 收紧**(M4 遗留):high-risk-command 能力的 Grant scope 恒
   once(与恒审批语义一致);external-side-effect 默认 ttl,forever 须审批
   卡片显式选择并全量审计;read-only/low-risk 不限。审批交互形态归 M8。
6. **watchdog.reorchestration.triggered 是事实事件**(§5.2/§5.6):陈述
   「监护已触发重启」这一已发生事实,非请求性命令;G4 守护反向固化
   (Watchdog 产物无命令形状),G2 边界不破。
7. **memory FTS = 阶段一可替换实现**(§5.8);级联失效 M5 做 scope 内
   墓碑最小面;用户删除权全量(单 Task/App 域/全局 + 备份加密)归 M8。
8. **预算 reservation 延续不做**(M4 §9 决定):子分配 = 记账分配,回执
   丢失场景由幂等键承担恢复期判定;是否引入预留随 M7/M8 真实负载数据裁决。

## 9. 裁决定案(2026-08-29,原开放项与默认路径)

- **D-M2-2 结算**(ADR-0004 条件 6):触发者 = 用户显式 resume ∪ Watchdog
  自动(事实事件驱动);停滞窗口默认 15 分钟、硬顶 24 小时、重复 3 次、
  扫描 60 秒;waiting_approval 豁免。基线 §10.3 已补定义(条件兑现动作);
  PENDING 记大白话供追认/改判,数值变更走 Task 配置不破合同。M5 回看时
  条件 6 闭合。
- **ADR-0002 预算包络**:按 §5.5 落地,回看时与条件 2/5 一并结算;不实现
  reservation(§8-8)。
- **Task 授权载体**:Task 创建时声明 authorization(协调动词白名单 +
  资源谓词),为三方交集的 Task 分量;用户授权分量 = 审批产生的 Grant
  (M4 机制复用)。
- **Butler bootstrap Grant 可撤销**:撤销后仅剩只读;重授走审批。撤销不
  影响 Task 存在性(L2 持有)。
- **Watchdog 实现形态**:核心循环内节流扫描(MockClock 注入),不新增
  线程/进程;扫描开销随 P-11 一并观测。
- 无需用户预裁决项;产品体验类问题(停滞窗口数值、暂停/恢复交互形态等)
  实现中记 PENDING,默认路径继续。
