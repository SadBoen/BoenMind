# M2 实现规格(草案 v0.1)

> 第 2 层工件:M2(持久化、事件日志与崩溃恢复)的技术栈、crate 划分与任务分解。
> 地位在基线(第 0 层)与合同库(第 1 层)之下;冲突以上两层为准。
> 上游输入:`milestones/M2-adr-settlement.md`(ADR-0003/0004 结算,含 7 条硬约束)
> 与 `milestones/PENDING.md`(D-M2-1/D-M2-2 默认路径)。
> 状态:**草案**——用户评审冻结为 v1.0 后开工;自主推进期间按 §9 默认路径先行。

## 1. 范围

基线 §18 M2.1–M2.7 全部子项,外加:四项混沌验收(D-M2-1 适配映射)、
S4 崩溃恢复、P-02/04/05/07 性能回填(M1 延期项)。

通过条件(基线):强制终止后可以恢复 Session、Task 和 Operation;已完成操作
不会因重启自动重复;重复投递不会破坏投影;未知副作用不会被当成普通失败。
M2 对象域无 Task(归 M5),Task 相关断言按 D-M2-1 适配映射落在
session/operation/event-log 域,原文四项在 M3/M5 回看补跑。

非目标:CLI Surface(M3)、Task/Team(M5)、Capability/Approval(M4)、
外部 Provider 进程(M7)、generation 升级(阶段二)。

## 2. 技术栈

| 项 | 选择 | 理由 |
|---|---|---|
| 嵌入式存储 | SQLite(rusqlite,bundled feature) | 基线 M2.1 明示 SQLite;bundled 免系统依赖,三平台一致(测试矩阵 P0) |
| 并发模型 | 专用写者线程持有唯一连接 | 与核心循环同构的单写者纪律;写请求经通道串行,杜绝 WAL 下的多写者竞争 |
| 事件日志 | 追加写 JSONL 文件(逐行一条 event_envelope,复用 M1 序列化形态) | 与「SQLite 规范状态」分离的 Append-only 日志(M2.2);文本行可校验、可 diff、可直接过 envelope schema |
| 崩溃一致性 | **先日志后状态**:事件先 append+flush(+fsync 可配)再提交 SQLite 事务,事务内更新 meta.last_applied_seq | 崩溃窗口只会「日志有、状态未及」——恢复时重放尾部即可,永不出现「状态有、日志无」的反向孤儿(规格 §8.1) |
| 快照/压实 | 快照 = SQLite 在 snapshot_seq 的自包含检查点;压实 = 截断事件日志中 seq ≤ snapshot_seq 的前缀 | ADR-0004 条件 2 的强制义务;触发策略:每 N=10_000 条事件或日志超 64MB,先快照成功再截断 |
| 迁移 | PRAGMA user_version 版本化 schema 迁移,expand-contract(只加列不删列) | ADR-0003 条件 2 对偶:数据一致性不押注任何回滚 |

## 3. 仓库结构(增量)

```text
runtime/crates/
  bm-persist/            # 新增:持久层(bm-core 只依赖其端口 trait)
    src/store.rs         #   EventStore 端口:append/replay_since/snapshot/truncate_prefix
    src/sqlite_state.rs  #   规范状态库:sessions/agents/operations/budgets/meta 五表
    src/event_log.rs     #   JSONL 追加日志 + 前缀截断(压实)
    src/recovery.rs      #   启动恢复:load SQLite → tail replay → RecoveryPlan
    src/chaos.rs         #   注入点:在 append/commit 之间崩溃(测试专用)
  bm-core/               # 增量:RuntimeConfig 增加 store: Option<Arc<dyn EventStore>>
    src/runtime.rs       #   核心循环写穿(store 写入失败 = 拒绝命令,不可静默)
    src/resume.rs        #   跨进程 session.resume(M1 D3 边界升级)
  bm-testkit/            # 增量:混沌四项 + 重放确定性 + P-02/04/05/07
```

端口原则延续 M1:核心循环依赖 `EventStore` trait,bm-persist 是可替换实现
(万物皆插件;M2 不做进程外 Provider,进程内 crate 即可)。

## 4. SQLite schema v1(expand-contract 起点)

```sql
meta(key TEXT PRIMARY KEY, value TEXT)            -- schema_version/last_applied_seq/snapshot_seq
sessions(id TEXT PRIMARY KEY, state TEXT, agent_id TEXT, created_at TEXT)
agents(id TEXT PRIMARY KEY, session_id TEXT, name TEXT, model_chain TEXT/*JSON*/,
       state TEXT, budget_max_tokens INTEGER, budget_max_turns INTEGER,
       budget_used_tokens INTEGER, budget_turns_used INTEGER)
operations(id TEXT PRIMARY KEY, session_id TEXT, agent_id TEXT, request_id TEXT,
           state TEXT, turn_index INTEGER, created_at TEXT, completed_at TEXT,
           action_summary TEXT, result_ref TEXT, error_code TEXT, error_message TEXT)
tombstones(kind TEXT, id TEXT, at TEXT)            -- M5 起启用,M2 预留(ADR-0004:删除以墓碑为准)
```

全部主键即 ULID;状态列只存合同状态名;查询不建二级索引(M2 无查询负载,
投影一律走事件重放——ADR-0004 条件 1)。

## 5. 关键设计决策

### 5.1 事件日志与规范状态的写入序 **[裁决]**

写序固定为:① event_log.append+flush → ② SQLite 事务(状态变更 +
meta.last_applied_seq=seq)。恢复算法:打开 SQLite → 读 last_applied_seq →
重放日志中 seq 更大的尾部 → 提交。因此:
- SQLite 是**规范状态的快路径载体**;事件日志是**可重建的事实史**;
- 任何投影(任务板/卡片/索引)只允许从事件日志重建(ADR-0004 条件 1);
- 两者互为完整性校验:启动时断言 `meta.last_applied_seq ≤ log.last_seq`。

### 5.2 跨进程 resume(M1 D3 边界升级) **[裁决]**

M2 起 `session.resume` 支持跨进程:启动恢复(RecoveryPlan)完成后,session/
agent/operation 全部自持久状态就位;resume 返回投影补发事件(since_seq 之后,
自事件日志读取)。M1 的「同进程 resume」读法废止,规格 §8.3 相应标注为
「M1 阶段限定」。

### 5.3 outcome_unknown 处置机制(M2.6) **[裁决]**

M2 对象域无外部副作用,`outcome_unknown` 仍不可达(INV-11 保证);本子项交付
**机制**而非触发场景:
- 恢复流程 RecoveryPlan 三动作:`query`(查外部系统)/`claim`(认领继续)/
  `compensate`(显式补偿),禁止普通重试(ADR-0004 条件 4 / INV-10);
- 用户裁定入口:`operation.settle` 内部命令(非 Wire 方法,M4 起升级为合同方法),
  把 outcome_unknown/interrupted 落向 succeeded/failed/cancelled;
- M2 对象域演练:interrupted 操作(崩溃遗留)按 effect_class=NoEffect 走
  `claim` → 重新驱动回合(无副作用,可安全重跑)→ 终态。

### 5.4 CAS/epoch 写入门禁底座(ADR-0004 条件 3) **[裁决]**

meta 与规范状态写入提供 `compare_and_set(key, expect, new)` 形态;启动恢复
与快照切换必须走 CAS;带过期 epoch(= 过期 expect)的写入拒绝并留审计痕迹
(事件 `store.write.rejected`,见 §6)。task_epoch 完整门禁 M5 启用。

### 5.5 合同增发(只增不破,Minor) **[裁决]**

runtime-events 注册表增发 2 个 M2 事件(新增条目,不改既有条目):
- `runtime.recovered {last_applied_seq, replayed, interrupted_recovered}`——启动恢复完成;
- `store.write.rejected {key, reason}`——CAS 过期/写入门禁拒绝的可审计事件。
同步动作:注册表 JSON 增条 → bm-contract `EventType`/`payload_keys` 同步测试
自动变红 → 补镜像 → validate.py 全绿。envelope 错误码枚举不动。

## 6. 任务分解与顺序

```text
T0  合同增发 2 事件 + bm-contract 镜像同步 + validate.py 全绿        (Minor,先行走红-变绿)
T1  bm-persist 骨架:EventStore 端口 + SQLite schema v1 + JSONL 日志
T2  写穿接线:核心循环 ①→② 写序;store 失败 = 命令失败(不可静默)
T3  启动恢复 RecoveryPlan + 跨进程 resume(M2.7 resume cursor 语义)
T4  混沌①:S4 杀进程恢复——子进程 taskkill /F 后重启,断言状态/位点/无半写
T5  快照与压实(强制触发策略)+ 恢复自快照路径
T6  事件重放器:空库重放至任意 seq;混沌③:同前缀两次重建逐字段一致
T7  outcome_unknown 机制:RecoveryPlan 三动作 + 裁定入口 + interrupted 演练(M2.6)
T8  CAS 门禁 + 混沌④:过期写入拒绝留痕;混沌②:投影类本地库损坏无行为差异
T9  全量回归(50+ 新增测试)+ P-02/04/05/07 回填(1 万条预置事件)
T10 §19 回看 + PENDING D-M2-1 追认转正 + AGENTS.md 进度 + tag m2-persist-recovery
```

依赖:T0 → T1 → T2 → T3 →(T4,T5,T6)→ T7 → T8 → T9 → T10。
每步合并后主干可校验(测试 + clippy -D warnings + validate.py)。

## 7. 验收面

- 基线 M2 通过条件四句(§1)各有测试承载;
- 四项混沌(D-M2-1 映射)= t_chaos_01..04;
- INV 全部保持绿(M1 的 12 条不回归);INV-3 的连续性语义扩展为
  「跨重启仍连续」(日志 seq 不断档);
- 新事件 payload 键集过注册表;事件过 envelope schema;
- P-02/04/05/07 数值回填 perf-baseline(§1.1 追加 M2 记录②)。

## 8. 合同解读条款(实现期裁决,回看复核)

1. **「先日志后状态」的单写者写序**是本规格对 M2.1/M2.2 关系的裁决:
   事件日志为重建事实源,SQLite 为规范状态快路径;基线未规定双写顺序,
   本裁决选择崩溃安全方向,并使两者互为校验。
2. **resume cursor 跨进程化**:M1 规格 §7-D3 的「仅同进程」读法在 M2 升格,
   M1 文本不回改(历史规格不追改),以本条为准。
3. **store.write.rejected 属注册表增发事件**,不进 envelope 错误码
   (它是审计事实,不是调用方错误)。
4. **混沌①的杀进程方式**:Windows `taskkill /F`、POSIX SIGKILL,经子进程
   测试拓扑执行(测试矩阵 S4 的可执行化)。

## 9. 自主推进默认路径(PENDING 未决期间的先行依据)

- D-M2-1(混沌映射)与 D-M2-2(编排重启数值)未追认前,按本文 §1/§5.3/
  §6-T4/T8 默认路径实现;追认后仅更新验收清单措辞,不影响结构。
- 合同增发(T0)属 Minor 且可逆,按规程先行;若用户否决,回退该提交即可
  (注册表增条是纯追加)。
