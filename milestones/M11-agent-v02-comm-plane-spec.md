# M11 规格:Agent v0.2 通信面(批次 1:共享面)

> 决策文档 = `adr/ADR-0031-agent-v02-comm-plane.md`。状态:**已批准动工**(O1=直通+审计,2026-09-10)。日期 2026-09-10。
> 临时工件:收官回看后删除(ADR-0027)。

## 0. 批次切分(ADR-0031 决策 3)

| 批次 | 内容 | 时点 |
|---|---|---|
| 批次 1(本规格) | Task 作用域共享面(公告栏):发布/查询发现 | **已落地**(见 §4 验收) |
| 批次 2 | 点名消息 + 成员身份面(含 #31 max_concurrent_tools) | 另行排批;动工前核成员真并发成熟度(O2) |
| 远程网格 | 跨机传输 | 阶段二后续,另行 ADR |

## 1. 批次 1 能力面(内核内置,全走 Broker)

- `task.share.publish` {title, content}:把一条发现贴上本 Task 公告栏。作用域 = 调用方所属 task:<id>,跨 Task 结构性不命中。refs 字段暂缓——发现正文即 content,合同最小面;批 2 随消息信封一并评估。
- `task.share.list` {since_seq?}:按序读取本 Task 公告栏。
- **权限形态(O1 已裁决:直通+审计,对齐 fs.*/ADR-0002 既有三层,零新裁决步)**:trusted 主体走 Broker 步 6 内建直通;worker/coord 走 Task 授权 Grant(步 4 批量预授权,ADR-0002 裁决 4);无 Grant 的 untrusted 调用 publish 升级审批(Reversible 生效)、list 默认拒绝(ADR-0006)。审计 = capability.invoked(succeeded)+share.published 双落盘。
- **合同归类(O4 已定稿)**:归 ADR-0002 协调动词族 scopes=["domain:task"],manifest approval=not-required;不走 ADR-0020 内置冻结清单例外位。task_id 自 principal 结构推导(args 不可指定,防伪逃逸)。
- 合同增事件 `share.published`(Minor):{task_id, principal, title, content};注册表 + EventType 键表同步,47 条(sync 测试更新)。内存投影沿 emit 钩子增量维护 + 启动重放重建(增量=重建有测试)。

## 2. 批次 1 呈现面

- 内核测试夹具验收(runtime/tests.rs m11_share_tests 四门)+ share.rs 投影单测三门;webapp 呈现不在批次 1(O5)。

## 3. 批次 2 机制预留(本期只登记,不实现)

- 消息信封草案:addressed 消息 {from_member, to_member, kind, body, task_scope};与 share.published 同族不同型。
- worker_call wire 增成员归属段(合同 Minor);principal → `agent:worker:{task_id}:{member_id}`(段格式规格冻结时定)。
- #31:max_concurrent_tools 计数点 = worker_call 入口成员在途计数;预算账本按成员分账。
- 前置核查(O2):M9 worker 自主环 v0 已落;批次 2 规格冻结前核实成员调用是否已真并发,据此排批。

## 4. 验收门(批次 1,全绿)

1. 跨成员可见:A publish → B list 可见(Coordinator 同理,principal 同族);
2. 跨 Task 隔离:T1 发布,T2 结构性不可见;args 指定 task_id 不改归属(防伪);
3. Broker 审计:capability.invoked + share.published 双落盘;
4. 越权:非 Task 域主体 ValidationFailed;无 Grant publish 升级审批、list 默认拒绝;
5. 重启恢复:增量投影 == 事件重放重建(测试锁死);
6. 门禁:validate.py 全绿 + 全仓 465 测试绿 + clippy 零警告;
7. 内核夹具可用性验收完成;webapp 呈现不在本批(O5)。

## 5. 开放点汇总

- **O1** ✅ 已裁决(2026-09-10):直通+审计,走既有 Broker 三层(直通/Grant/审批),零新裁决步;
- **O2** 批次 2 时点:成员真并发成熟度核实后排批;
- **O3** 公告栏保留期:事件溯源永存,活跃投影只挂活跃 Task,随 Task 终态失效,不设独立清理;
- **O4** ✅ 已定稿(2026-09-10):协调动词族,domain:task,不占 ADR-0020 例外位;
- **O5** webapp 呈现:批次 1 不做,随批次 2 / W 序列另行;
- **O6**(新增,随批注记)对话工具目录现对无 Task 主体也展示 task.share.*(描述已注明"由调用主体自动归属"),按主体过滤目录留批 2 一并评估。
