# M11 规格:Agent v0.2 通信面(批次 1:共享面)

> 决策文档 = `adr/ADR-0031-agent-v02-comm-plane.md`。状态:**待用户批准**(规格过目后才动工)。日期 2026-09-10。
> 临时工件:收官回看后删除(ADR-0027)。

## 0. 批次切分(ADR-0031 决策 3)

| 批次 | 内容 | 时点 |
|---|---|---|
| 批次 1(本规格) | Task 作用域共享面(公告栏):发布/查询发现 | 待批准后动工 |
| 批次 2 | 点名消息 + 成员身份面(含 #31 max_concurrent_tools) | 另行排批;动工前核成员真并发成熟度(O2) |
| 远程网格 | 跨机传输 | 阶段二后续,另行 ADR |

## 1. 批次 1 能力面(内核内置,全走 Broker)

- `task.share.publish` {title, content, refs?}:把一条发现贴上本 Task 公告栏。作用域 = 调用方所属 task:<id>,跨 Task 结构性不命中。
- `task.share.list` {since_seq?}:按序读取本 Task 公告栏。
- 权限形态(开放点 O1,**建议:直通类+审计**,对齐 fs.* 先例):发布/查询均为本 Task 内存投影上的低风险操作,不落敏感域、不出沙箱;默认拒绝语义不变,未授权主体照常 Denied。
- 合同归类(开放点 O4,建议):两能力归 ADR-0002 协调动词族 safe_coordination 类(list 纯查询可默认继承;publish 为本 Task 投影写,随 Task 授权列出),不走 ADR-0020 内置冻结清单的 fs./system. 例外位。规格冻结时定稿。
- 合同增事件 `share.published`(Minor,只增):envelope 载 {task_id, principal, title, content, refs};events.jsonl 落盘,内存投影沿 TaskBoard 材料化路径重建(重启可重放)。错误码如需新增与注册表同步(CI 比对)。

## 2. 批次 1 呈现面

- CLI/夹具验收为主(bm-cli task 命令族或集成测试夹具);webapp 呈现不在批次 1(开放点 O5:Task 视图 UI 现缺,随批次 2 或 W 序列另行立项,避免本批膨胀)。

## 3. 批次 2 机制预留(本期只登记,不实现)

- 消息信封草案:addressed 消息 {from_member, to_member, kind, body, task_scope};与 share.published 同族不同型。
- worker_call wire 增成员归属段(合同 Minor);principal → `agent:worker:{task_id}:{member_id}`(段格式规格冻结时定)。
- #31:max_concurrent_tools 计数点 = worker_call 入口成员在途计数;预算账本按成员分账。
- 前置核查(O2):M9 worker 自主环 v0 已落;批次 2 规格冻结前核实成员调用是否已真并发,据此排批。

## 4. 验收门(批次 1)

1. 跨成员可见:A 成员 publish,B 成员 list 可见,Coordinator 同可见;
2. 跨 Task 隔离:task1 发布,task2 list 结构性不可见(测试锁死);
3. Broker 审计:publish/list 均有审计记录(principal/能力/参数);
4. 越权:非本 Task 主体调用 → Denied(默认拒绝语义不变);
5. 重启恢复:发布后重启,投影由事件重放重建,list 一致;
6. 门禁:validate.py 全绿 + 全仓测试绿 + clippy 零警告;
7. CLI/夹具可用性验收;webapp 呈现不在本批(O5)。

## 5. 开放点汇总

- **O1** publish 权限形态:直通+审计(建议)vs 审批类——影响发布延迟,批准规格时一并定;
- **O2** 批次 2 时点:成员真并发成熟度核实后排批;
- **O3** 公告栏保留期:事件溯源永存,活跃投影只挂活跃 Task,随 Task 终态失效,不设独立清理;
- **O4** task.share.* 合同归类:协调动词族(ADR-0002 safe_coordination,建议),规格冻结定稿;
- **O5** webapp 呈现:批次 1 不做,随批次 2 / W 序列另行。
