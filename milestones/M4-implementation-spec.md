# M4 实现规格 v1.0(实现者自主冻结)

> 第 2 层工件:M4(Capability、Broker、权限和审批)的技术栈、合同增发清单与
> 任务分解。地位在基线(第 0 层)与合同库(第 1 层)之下;冲突以上两层为准。
> 上游输入:基线 §18-M4(八子项)、ADR-0001(七条件)、ADR-0002(六条件)、
> `M4-adr-settlement.md`(11 条硬约束,本规格 §7 逐条转写)、M1/M2/M3 回看
> 遗留账本、m0 威胁模型与 PI 用例集。
> 状态:**v1.0(2026-08-29 实现者自主冻结)**。沿 M3 治理变更:技术规格不送
> 用户评审;开放裁决点按 §9 默认路径执行,全部记入 PENDING 供事后知情。

## 1. 范围与形态裁定

基线 §18-M4 八子项全量。核心形态变化:**Runtime 内出现第一套受管辖的跨域
调用面**——Capability Registry(谁提供什么)+ Capability Broker(能不能调
以及调谁,唯一入口)+ 审批/授权持久对象;Model 调用路径保持 M1 端口形态
不变(豁免裁决见 §5.8)。M4 无 Task/Team 对象(基线 M5/M6),无真实外部
Provider(基线 M7);用内置演示能力集实证全链路,验收不含真实副作用。

| 子项 | M4 交付 | 说明 |
|---|---|---|
| M4.1 Capability Registry | manifest 注册 + Provider Binding 两层(持久逻辑目录 + 可丢失运行时缓存)+ binding_epoch | 见 §5.2;增发 capability.* 合同(Minor) |
| M4.2 Capability Broker | 策略预编译 O(1) 查表 + 七步管线逻辑拆分 + 统一审计 | 硬约束 1;三证伪测试进验收 |
| M4.3 输入输出 Schema 校验 | args 过 manifest input_schema / 结果过 output_schema,违者 validation_failed | manifest 承载 schema,决策点校验 |
| M4.4 身份与权限交集 | principal 授权(Grant 集)∩ manifest scopes,默认拒绝 | ADR-0006:权限以合同显式化;M4 交集的 Task 分量尚不存在,授权来源 = 用户批准(§5.4) |
| M4.5 Approval 持久化和恢复 | Approval 持久对象(SQLite)+ 重启恢复 + waiting_approval Operation 状态 | 审批中断后可恢复(基线通过条件) |
| M4.6 审计事件和调用归因链 | capability.invoked/denied 事件携带 principal/request_id/operation_id/grant_id/binding_epoch | 审计可追溯调用者、Task(M4 恒空)、Operation |
| M4.7 审批状态机与授权范围 | requested→waiting_user→approved/denied/expired/withdrawn;scope = once/count/ttl/forever | task:<id> 随 M5;M4 产生含 task 的 scope = validation_failed |
| M4.8 input_trust 门控 | untrusted 驱动 → reversible+ 100% 升级审批;越权 100% 拒绝;量化 CI 门槛 | 硬约束 9;PI 用例 A2/A3 自 M4 全面生效 |

非目标:Butler/Coordinator/Task(M5)、子树过滤(M5)、真实 Provider/MCP/收据
实证(M7,复测项见 settlement 0001-4/5)、模型调用经 Broker(§5.8 豁免,M7 复议)、
预算子分配(M5,预算强制点 M1 已有)。

## 2. 技术栈

无新增依赖。全部复用 M1–M3 既有栈:tokio 单写者核心循环、SQLite(bm-persist,
expand-contract v2→v3)、JSONL 事件日志、axum Wire 端点、clap CLI。Broker 为
bm-core 内的同步决策结构(查表是纯函数,无 IO),不引入独立线程/进程——
三权分立是同进程协议角色分立(ADR-0001 共识第 1 条)。

## 3. 仓库结构(增量)

```text
runtime/crates/bm-core/src/
  capability.rs        # CapabilityManifest / RiskClass / principal / trust 类型
  registry.rs          # Capability Registry:manifest 表 + Binding 两层 + epoch
  broker.rs            # Broker:策略查表编译、七步逻辑拆分、凭证签发、审计事件
  approval.rs          # Approval / Grant 持久对象与状态机推进(事件经核心循环落盘)
runtime/crates/bm-providers/src/
  builtin.rs           # 内置演示能力 Provider(system.* 五能力,见 §5.6)
  mock_side_effect.rs  # 副作用 mock:外部收据/幂等查询/崩溃点注入(outbox 验收)
runtime/crates/bm-persist/src/
  sqlite_state.rs      # v3 expand:approvals / grants / capabilities / outbox 表
boenmind-contracts/    # 增发清单见 §4(Minor,只增)
runtime/crates/bm-cli  # approval 命令组兑现;capability list/call 增发
```

## 4. 合同增发清单(Minor,只增不破)

1. `capability/manifest.v0_1.schema.json`:Capability Manifest(基线 §5.2
   全字段:capability/provider/version/input_schema/output_schema/effect/
   idempotent/cancellable/timeout_ms/approval 必填;scopes/verification/undo/
   retry/deprecated_by 可选)+ 增列 `mutation_class: safe|mutation`
   (硬约束 8;effect=read-only 派生 safe、其余 mutation,显式声明可覆盖;
   M5 协调动词过滤消费此字段)。
2. `capability/grant.v0_1.schema.json`:Grant 下限字段集(硬约束 8/10):
   grant_id、audience、action、resource 谓词(精确 capability 名 + 可选
   args 等值谓词字典;空谓词 = 全参授权)、scope(once/count:<n>/ttl:<dur>/
   forever/task:<id>)、delegation_depth(恒 0)、expires_at、
   revocation_version、parent_grant_hash、issued_by、created_at。
3. `capability/approval.v0_1.schema.json`:Approval 持久对象:approval_id、
   capability、args 摘要(脱敏)、principal、risk_class、effective_risk、
   input_trust、state、scope 选项、requested_at/expires_at/resolved_at、
   grant_id(批准后回填)。
4. `capability/lease.v0_1.schema.json`:数据面通道凭证(硬约束 4):
   lease_id、binding_epoch、policy_version、operation_id、deadline、
   provider_instance_id、byte_budget。
5. `wire/capability.v0_1.schema.json`:`capability.call`(params:
   capability/args/idempotency_key?/deadline_ms?;result:operation 收据)、
   `approval.list` / `approval.respond`(approve+scope 选择 / deny)三方法
   的 params/result。
6. `wire/envelope.v0_1.schema.json`:method 枚举增 3 项;error_code 枚举
   增 permission_denied / approval_required / approval_denied /
   idempotency_conflict 四码(注册表 available_since=M4 已备;envelope 与
   注册表同步,R6/sync.rs 镜像断言同步更新为「M1∪M4 码集」)。
7. `registry/runtime-events.v0_1.json` 增发:approval.requested /
   approval.resolved / approval.expired / grant.created / grant.revoked /
   capability.invoked(含 outcome: ok|error|suppressed,幂等抑制审计载体)/
   capability.denied / provider.binding.changed / bus.degraded / bus.resumed。
   (22 → 32;sync.rs 事件数断言同步。)
8. `state-machines/core-transitions.v0_1.json`:operation 状态机增
   waiting_approval 状态与三条边:running→waiting_approval(guard
   approval_pending)、waiting_approval→running(guard approval_granted)、
   waiting_approval→cancelled(guard approval_denied_or_expired_or_withdrawn)。
   agent 状态机 waiting_approval 不在本期(工具调用循环 M5 随行;
   m1_subset_note 的兑现顺序记入 §8 解读条款)。
9. `golden-traces/M2-GT-02-capability-approval.md`:GT-02 两场景
   (预授权直通 / untrusted 升级审批→批准→执行→审计)。validate.py R2–R4
   从硬编码 GT-01 改为遍历 golden-traces/*.md(泛化,不改判定规则)。
10. `m0/perf-baseline.v0_1.md` 增 P-09(Broker 授权开销)/ P-10(队头阻塞)
    定标骨架,数值 T9 回填。

M1–M3 已冻结字段与事件零改动;上述全部为新增文件/新增枚举值/新增状态与边,
均为 Minor。

## 5. 关键设计决策

### 5.1 Broker 策略查表(O(1))与三证伪测试 **[硬约束 1]**

- 决策结构:`PolicyTable: (principal_kind, capability) → Decision`,
  Decision ∈ {deny, allow{grant_id}, require_approval{reason}}。由持久态
  (manifest 表 + Grant 表)编译,Grant 增/消费/撤销时增量重编译;读取路径
  纯 HashMap 查找 + 常量级凭证校验(过期/计数/版本),禁止逐条策略求值。
  七步管线(身份→权限→scope→参数校验→绑定→执行→审计)实现为同一函数的
  逻辑分段,非运行时串行管线。
- 三证伪测试(ADR-0001 条件 1,超标即回炉实现而非裁决):
  ① p99:P-09 定标——授权决策(不含 Provider 执行)test build p99 < 200 µs,
  release build p99 < 10 µs;
  ② 队头阻塞:注入 50 ms 慢 Provider 在途调用,并发 100 次无关授权调用的
  p99 劣化 < 2× 空闲基线(P-10);
  ③ 故障半径:Broker 决策路径注入 panic → L0 兜底重启(generation 不变),
  恢复后授权行为一致、审计无缺口;期间不得出现特权降级通道。
- 默认拒绝:查表未命中 = permission_denied,无审批出口(审批不能补授权,
  ADR-0006);unknown capability 与 no_grant 在 capability.denied 的
  reason_code 中区分,便于审计分类。

### 5.2 binding_epoch + provider_instance_id(授权-执行-审计三方一致) **[硬约束 2]**

- Registry 持久逻辑目录:capability → provider/version/status;运行时缓存:
  capability → 实例句柄 + 健康位(可丢失,重启重建)。每次 binding 生命周期
  事件(handshake/热替换/unavailable→恢复)epoch 单调 +1,per-capability
  计数,持久于 capabilities 表,重启不回退。
- Broker 在授权决策点固化 (binding_epoch, provider_instance_id) 进调用凭证
  与审计事件;内置 Provider 执行入口校验凭证 epoch 与自身当前 epoch 一致,
  不匹配即拒(执行前)或按 manifest retry 重试。
- 测试:授权签发后强制切 binding(epoch bump),断言在途执行被拒、审计记录
  归属仍是签发时 epoch/instance(Runtime generation 变更不影响在途归属,
  ADR-0001 条件 2 原文)。

### 5.3 数据面 lease 与四项准入测试 **[硬约束 4]**

- lease 凭证结构见 §4-4;Broker 签发 = 查表 allow 后的追加产物;通道准入 =
  凭证校验(epoch/policy_version/operation_id/decline 过期)。M4 数据面通道
  为进程内 mock 通道(字节流经 lease 准入计时),不落盘、不占 L2 单写者;
  通道字节数/阶段/收据摘要/错误回写审计事件。
- 四测试(ADR-0001 条件 4):①无 lease 建通道被拒;②epoch 切换不影响已授权
  通道审计归属;③通道吞吐不被 L2 持久写 p99 牵制(mock 通道计时断言,M7
  真实 Provider 复测);④崩溃注入下外部提交与收据摘要经 outbox 最终对账。

### 5.4 Grant / Approval 对象模型与 input_trust 门控 **[硬约束 8/9/10]**

- **Approval** = 用户裁决载体(§9.6 状态机,持久于 SQLite v3)。等待审批的
  Operation 进 waiting_approval;超时 → expired(等价 denied,禁止超时默认
  同意);denied → operation cancelled。审批卡片由 Broker 生成结构化摘要,
  untrusted 原文仅作带标注引用。
- **Grant** = Broker 记账载体,由 approved Approval 物化(issued_by=用户
  审批;parent_grant_hash = Approval 对象内容哈希)。M4 单路径期字段全量
  落地但 parent 链不启用(Coordinator 签发随 M5);直接 Capability 与
  Domain Agent 双路径的统一 Grant/幂等/脱敏/收据合同 = 本清单 §4-2/3,
  单路径期仅定义不启用(硬约束 10,双开禁止由「Domain Agent 不存在」结构保证)。
- **scope 语义**:once = Grant 首次消费即失效;count:<n> = Grant 内计数;
  ttl:<duration> = expires_at;forever = 不过期(审计可溯);task:<id> M4
  校验层拒绝产生。scope 生效域 = audience×action×resource 谓词。
- **input_trust 门控**(调用上下文必带,来源规则固定不可自报):
  - Wire Surface 直调 → trusted(用户显式操作,PI-01 语义);
  - 内部调用方(测试 harness/未来 Agent 路径)经 CapabilityContext 构造,
    trust 随内容来源链传递,类型上无从「声明降级」——untrusted 输入标注为
    trusted 视为编程错误,由构造 API 签名排除,守护测试覆盖;
  - effective_risk = manifest.effect 按 §5.3 风险序上提一级
    (read-only→low-risk-command→reversible-command→external-side-effect→
    high-risk-command)当 input_trust=untrusted;
  - effective_risk ∈ {reversible-command, external-side-effect,
    high-risk-command} → 强制 require_approval(即使 manifest.approval=
    not-required,ADR-0002 条件 3 的 100% 升级);read-only/low-risk 不升级。
- **量化 CI 门槛**(硬约束 9):决策矩阵测试 = 5 风险 × 3 trust × 有/无
  Grant(~30 格全断言)+ PI-02/03/06/07/12 跑 Broker 决策层:untrusted→
  reversible+ 升级率 100%、越权拒绝率 100%,以硬断言进 CI,非报告指标。
  PI 用例集 A2/A3 自本里程碑起全面生效(A1/A4 M1 已生效)。

### 5.5 事务性 outbox 与副作用合同 **[硬约束 5]**

- 保持 M2 写序(事件日志追加 → SQLite 提交,单写者)不变;outbox 语义由
  **intent/结果事件对 + 副作用前门禁**承担:external-side-effect 类调用,
  Broker 先持久 capability.invoked(kind=intent 含 idempotency_key 哈希),
  事件落盘后方允许 Provider 执行(副作用前门禁);执行结果以结果事件落盘。
  SQLite v3 增 outbox 表(side-effect 类 operation 的
  intent→pending→published→verified 状态),驱动恢复期对账。
- 恢复:存在 intent 无结果 → outcome_unknown 路径(§13.3)→ Provider 按
  operation_id/idempotency_key 幂等查询:确认未执行可重试、已执行取收据、
  无法确认请求用户裁定。禁止盲目重试(Broker 通用规则,不猜测外部结果)。
- 可证伪指标进回归:audit_gap(外部提交成功而审计/事实缺失)= 0;
  duplicate_side_effect(恢复后重复副作用)= 0。mock 副作用 Provider 提供
  「外部已提交、收据回写前崩溃」注入点验证两指标。

### 5.6 内置演示能力集(system.* Provider)

覆盖五风险等级 × 幂等性 × 审批矩阵,纯内存/本地 SQLite,无真实外部副作用:

```text
system.echo            read-only            幂等,恒放行(有 Grant 或 trusted 直通策略)
system.counter.bump    low-risk-command     幂等键可选;不升级审批
system.notes.write     reversible-command   undo=system.notes.delete 声明;trust 敏感
system.mail.mock_send  external-side-effect 幂等键必备;outbox/收据/幂等查询实证载体
system.danger.purge    high-risk-command    恒 approval_required(与 trust 无关)
```

所有能力注册时声明 manifest(input/output schema、retry、verification),
M4.3 的出入参校验与 §5.1 查表以其为测试面。真实 Provider(M7)替换实现,
合同与审计形态不变。

### 5.7 Bus 加固:降级语义、事件校验、守护三件套 **[硬约束 3/6/7]**

- **降级语义**(两分):A 分发层暂停(慢消费者/订阅断开)——broadcast 满即
  背压丢订阅者侧,核心循环永不阻塞,状态提交照常,补发由 resume cursor
  承担(现状结构成立,补守护测试固化);B 持久写路径故障——Runtime 进
  degraded:授权决策用内存查表缓存(「可丢失运行时缓存」定义内的合法缓存,
  恢复后以持久态重建校验),副作用类调用一律拒绝(安全侧),read-only 可答,
  恢复后发 bus.resumed 并审计 degraded 窗口。瞬态(数据面)与持久写路径
  物理隔离由 lease 通道不落盘结构保证(§5.3)。
- **事件 schema 校验加固**:持久化前逐事件校验(类型 ⊆ 注册表 + payload 过
  注册表 payload 形状);命令语义检测 = 拒绝携带 `requested_action` /
  `instruction` 字段形状的事件持久化并告警(store.write.rejected)。
- **伪造/乱序注入测试**:非白名单生产者(绕过核心循环直接写持久日志)在
  API 层不可达(单写者结构)+ 持久层打开写入钩子注入伪造/乱序事件 →
  回放检出差异或校验拒绝(ADR-0001 条件 3:投影可重建或异常可检出)。
- **架构守护三件套**(硬约束 7,常驻 CI):
  G1 Bus 不得当 RPC:事件流中不得出现携带命令语义形状的事件(持久前拒绝);
  G2 审批/Task 命令不入事件流:approval.respond 等命令走 Wire/Broker,
  事件流中只出现已裁决事实(approval.resolved),不出现请求性事件;
  G3 混层缓存超界即失败:清空全部内存缓存后,授权行为与缓存命中时逐字节
  一致(缓存可丢失性的机器断言)。
  L0 兜底边界:Broker 故障仅由 supervisor 重启承担,守护测试断言不存在
  旁路授权通道(G3 的逆断言:绕过 Broker 的调用入口在类型层不存在)。

### 5.8 模型调用豁免(解读条款,回看复核) **[裁决]**

Agent 回合内的模型调用保持 M1 端口直调形态,不经 Broker:模型连接器与回合
执行器同为 L2 内建模块(基线 §5.4 明文允许),二者之间无 App 数据域边界,
不构成「跨域调用」;回合入口 agent.send_input 已承载身份与审计。豁免范围
在架构守护测试中显式登记(白名单至 M7);M7 Provider 外置时模型连接器随
真实 Provider 一并纳入 Broker 与 binding_epoch,豁免撤销。风险留档:
若回看认定「§7 七类调用方」含 Agent→模型,则 M4 内补迁移(结构已备:
connector.invoke 换 broker.call 形态,预计一个任务量)。

### 5.9 幂等抑制审计证明 **[硬约束 11]**

对同一 Grant 下同 idempotency_key + 同参数哈希的第二次等价 external-side-
effect 请求:不重复执行,返回原收据(result_reference 一致),并产生
capability.invoked(outcome=suppressed)审计事件。测试断言:两次请求的
执行副作用恰一次,第二次被抑制可从审计日志单独证明(ADR-0002 条件 6)。

## 6. 任务分解与顺序

```text
T0  合同增发:§4 清单 1–8(GT-02 骨架、validate.py 泛化、sync.rs 镜像更新;
    validate.py 全绿为闸)
T1  bm-core capability.rs + registry.rs:manifest 注册、Binding 两层、
    binding_epoch 持久计数、机器可读发现面(capability list)
T2  bm-core broker.rs:PolicyTable 编译与 O(1) 查表、七步逻辑拆分、凭证
    签发、审计事件、lease 结构与通道准入(决策矩阵测试随行)
T3  approval.rs + SQLite v3(approvals/grants/capabilities/outbox 四表,
    expand-contract 迁移):审批状态机、waiting_approval 三边、重启恢复、
    Grant 物化与消费/撤销、CLI approval list/approve/deny
T4  input_trust 门控:CapabilityContext、effective_risk 上提、量化 CI 门槛
    (30 格矩阵 + PI 五用例决策层断言)、A2/A3 生效
T5  内置 Provider(system.* 五能力 + mock_side_effect)+ Wire 三方法
    (capability.call / approval.list / approval.respond)+ CLI capability
    call;e2e:预授权直通与审批升级全链路(GT-02 回放)
T6  outbox 对账:副作用前门禁、intent/结果事件对、恢复三路(未执行/已执行/
    未知)、幂等抑制审计测试、audit_gap/duplicate 双零指标注入测试
T7  Bus 加固:持久前事件校验 + 命令语义拒绝、伪造/乱序注入、降级 A/B 两态、
    bus.degraded/resumed 事件
T8  守护与证伪:G1/G2/G3 三件套、binding 切换三方一致、lease 四测试、
    P-09/P-10 三证伪测试、Broker panic 半径测试
T9  全量回归(74 项存量全绿)+ 性能回填(P-09/P-10 + P-01/02/03 复跑劣化
    < 25%)+ GT-02 双场景回放
T10 §19 回看:11 硬约束逐条结算 + settlement 部分落地项复核 + S1–S10 相关
    项裁决 + AGENTS.md 进度 + tag m4-capability-broker
```

依赖:T0 → T1 → T2 → T3 → T4 → T5 → T6 → T7 → T8 → T9 → T10。
T2 起每任务带新测试先行(沿用 M2/M3 测试先行纪律)。

## 7. 验收面

基线 M4 通过条件:所有能力调用经过 Broker;GUI、CLI 和 Agent 不能绕过权限;
高风险能力需要审批;审批中断后可以恢复;审计记录能追溯调用者、Task 和
Operation(M4 Task 恒空,字段在场)。承载:统一入口结构(G3 逆断言)+
决策矩阵 + waiting_approval 恢复测试 + capability.invoked 归因字段断言。

11 条硬约束逐条转写对照(settlement → 本规格):

| # | 硬约束 | 落点 |
|---|---|---|
| 1 | Broker O(1) 查表 + 三证伪 | §5.1;T2/T8;P-09/P-10 |
| 2 | binding_epoch + instance 三方一致 | §5.2;T1/T8 |
| 3 | 事件校验加固 + 伪造/乱序注入 | §5.7;T7 |
| 4 | lease 四测试 + 吞吐解耦 | §5.3;T2/T8(M7 复测留档) |
| 5 | 事务性 outbox + 指标上限 | §5.5;T6;双零指标 |
| 6 | Bus 暂停降级 + 物理隔离 | §5.7;T7 |
| 7 | 守护三件套进 CI | §5.7;T8 |
| 8 | manifest 分级 + Grant 下限集 | §4-1/2;§5.4;T0/T3 |
| 9 | input_trust 100%/100% CI 门槛 | §5.4;T4 |
| 10 | 双路径统一合同(定义不启用) | §5.4;T0/T3 |
| 11 | 幂等抑制审计证明 | §5.9;T6 |

## 8. 合同解读条款(实现期裁决,回看复核)

1. **模型调用豁免**(§5.8):L2 内建模块间调用非跨域;M7 外置时撤销豁免。
2. **outbox = intent/结果事件对 + 前门禁**(§5.5):不改变 M2 事件先写事实源
   语义;「本地状态提交→审计→事实追加」顺序映射为「intent 事件持久先于
   外部副作用,结果事件随后」,审计与事实同日志同序,等价满足 ADR-0001
   条件 5 的绑定要求。
3. **mutation_class 派生**(§4-1):effect→safe/mutation 默认映射,显式
   声明可覆盖;消费方 M5。
4. **task:<id> scope M4 拒绝产生**(§5.4):校验层 validation_failed;
   M5 随 Task 对象启用,枚举值本期合法保留。
5. **agent.waiting_approval 不在本期**(§4-8):审批等待由 operation 状态
   承载;agent 侧状态随 M5 工具调用循环增发(m1_subset_note 兑现顺序)。
6. **forever scope 收紧策略留 M5**:M4 接受 forever 批准并全量审计;
   是否按风险等级限制 forever 的可选性,随 Butler 审批 UI 定。

## 9. 裁决定案(2026-08-29,原开放项)

- envelope 字段边界(settlement 备注「未决分歧」):M4 不裁剪 §7 关联字段——
  capability 调用凭证全字段落地(idempotency_key 对副作用类必备、其余可选),
  复杂度问题留给真实负载(M7/M8)回看时以数据裁决。
- 委托凭证字段超集(ADR-0002 未决分歧):按「下限清单 + 可扩展」冻结 §4-2
  字段集;超集演进走 Minor 追加,不破坏只增纪律。
- 预算预留深度(ADR-0002 未决分歧):M4 不实现 reservation(无 Task 包络
  对象),维持「包络内子分配」留 M5;幂等键承担回执丢失场景的恢复期判定。
- input_trust 声明面:Wire 层不暴露 trust 参数(§5.4 来源规则),客户端
  无法自报信任级别。
- 无需用户预裁决项;产品体验类问题(审批交互形态等)实现中记 PENDING。
