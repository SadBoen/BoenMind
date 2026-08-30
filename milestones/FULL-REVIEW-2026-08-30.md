# 全面回看——M1-M9 整体(2026-08-30)

- 对象:阶段一全部(M0-M8)+ 阶段二第一批(M9)交付后的整体状态
- 方式:四道门禁实测 + 三路并行审查(架构复盘 / 代码质量横切 / 遗留台账盘点)
- 结论:**带条件通过(passed_with_conditions)**;条件见 §7
- 锚点:9194ac0(第四轮评审修复后)

## §1 健康基线(门禁 A/B/E/I)

```text
validate.py:  R1-R4 全绿(22 文件 / 45 事件 × 方法枚举 × 迁移边 41 次零非法)
tests:        260 passed / 0 failed(全量,本日实测)
clippy:       --all-targets 零警告;fmt --check 干净
性能(I 门):  引用记录⑦(2026-08-30 第四轮评审时复跑,P-01..P-05/07/08 门内,
              默认配置下热路径与 M0.6 基线无劣化);P-06 RSS 仍随真实使用采样
代码规模:     107 个 .rs / 34,118 行(不含编译产物)
```

## §2 架构复盘(门禁 G)

### 2.1 架构红线四查

| 红线 | 结论 |
|---|---|
| 绕过 Broker 的旁路 | **未发现走通路径**。16 个 RPC 方法全经 RuntimeHandle;模型调用 spawn 前 broker.decide(turn.rs:40-90);MCP 异步执行先 broker.prepare(turn.rs:1044-1115) |
| 新的事实源 | **未发现**。rusqlite 仅 bm-persist 一处写方;TaskBoard 纯投影(task.rs:244-302);审批超时=denied 单点 |
| 重复逻辑 | 双日志(event_log/exec_log)为基线设计但**追加/轮换机制两处独立实现**(exec_log.rs 脱离 bm-persist);无同一状态迁移两份实现 |
| 不合理耦合 | bm-surface-http 直依赖 bm-persist(SSE 只读,lib.rs:27);bm-providers 依赖 bm-core 而非 bm-contract;core→surface 反向依赖未发现 |

### 2.2 C4 模型与实现漂移(最大架构面问题)

模型画了但代码不存在(12 项):Tauri Chat / Voice / Notification / Mail(仅 mock)/
Stock / Music App、Audio / File / Timer Provider、L0 Supervisor 等——多数属
「模型超前、阶段兑现」合法状态,但 **ADR-0011 App=MCP 形态未回写模型**(Wiki 实为
python MCP server,模型仍画独立 Rust 进程;Market App 无模型元素而模型画 Stock)
已构成程序性漂移(硬纪律 2:先改模型再改文字)。代码有但模型没画(7 项):
bm-judge、bm-runtime 组装根、/admin/cli 端点、watchdog、butler/team/task 进内核等。

**处置建议**:列阶段二开工前专项——模型回写一次(含超前容器标注阶段归属),
不随本回看自动改(取舍需裁决:超前容器保留标阶段 vs 删除)。

### 2.3 ADR 兑现度

| ADR | 状态 |
|---|---|
| 0001/0002/0004/0010/0011 | **成立**(三权分立/Butler 协调权/Task 归 L2/网关信任边界/App=MCP 均有代码实证) |
| 0005 | 大体成立;butler/coordinator/team/memory 进 bm-core 与「内核最小机制」张力(M6-review §5-G 已自认),与 R-11 条件清账同源 |
| 0006 | 成立;**边缘案例**:memory_drawer_verdict 为硬编码常量规则(broker.rs:473-500,M9 §4 已留档偏差),与「权力以合同显式化」存在口径张力,宜发 ADR 补记 |
| 0003/0007 | 未到期(L0 不存在;主密钥暂 env 明文,条件未闭合,随阶段二) |
| 0008 | 部分成立:模型漂移(§2.2);S1-S10 索引滞后(adr/README、architecture/README 仍写「全部 proposed」) |
| 0009 | 部分成立:Web v1 已交付;Tauri 壳仅骨架(D-M8-3 待用户裁决);TLS 依赖反代留档 |

### 2.4 deepwiki S1-S10 状态总表(散在 M4-M9 回看,此处首次汇总)

| 编号 | 一句话 | 状态 |
|---|---|---|
| S1 restart 类型/退出宽限期 | proposed 未裁决 | M4-review:95 后无接续 |
| S2 升级迁移回放入验收 | proposed 未裁决 | 同上 |
| S3 停滞检测 | **部分采纳**(watchdog 落地;generation 面因无 L0 未落) | M5/M6-review |
| S4 draining 两步化 | 部分实现未实测(D-M8-1①) | M7→M8→M9 移交在档 |
| S5 manifest 前置校验/quarantined | **已裁决部分采纳并兑现**(M4 register + M8 t119b);**存疑**:t119b 实为坏库隔离重建,与「quarantined 分表」原文非同一机制,裁决口径偏宽 | M8-review:82 |
| S6 懒启动 activationEvents | proposed 未裁决 | M4-review:95 |
| S7 rest_for_one 级联重启 | proposed 未裁决(注意 M8-review:94 的 "S7" 是同名异物) | M4-review:95 |
| S8 Wire API 按方向拆分 | 部分实践未正式裁决 | M4-review:91 |
| S9 verification 三分法 | **已裁决闭合**(部分采纳,M5 落地 M7 确认) | M7-review:102 |
| S10 Patch 级维护窗口 | proposed 未裁决 | M4-review:95 |

## §3 代码质量横切

**panic 面(生产代码)**:unwrap()=0,todo!/unimplemented!=0,expect()≈176
(77 处锁中毒 + 其余带中文不变量注释)。**P1 一处**:exec_log.rs:125-127/172 的
I/O expect——磁盘满/权限/文件被占用(Windows 备份拷贝窗口)可 panic 整个
runtime,属外部条件可触发。其余为启动 fail-fast、自写行解析(有 open_resilient
兜底)、逻辑卫兵,均可留。

**大文件**:broker.rs(1657)生产段仅 758 行、无超长函数,**不需拆**;
task_ops.rs(1709)/turn.rs(1560)各含 2-3 个 200+ 行函数(handle_turn_event 274、
capability_call_inner 271、handle_task_create 206 等),autorun 子系统 ~250 行
可独立成文件——**重构债,登记不立即动**(R-08/L-01 同批权衡)。

**吞错误**:投影写普遍 `let _ =`(turn.rs:718 save_idem_receipt 最关键——静默失败
将使重启后幂等抑制失效,副作用可能重放;事件日志先行使其大体无害但**全仓库无
失败诊断痕迹**)。store.rs:184,421,428,530 meta 损坏静默按 0。

**技术债标记**:TODO/FIXME/HACK 全仓零命中,纪律到位。

**测试**:271 个测试属性;负路径文化突出(broker 5×3 决策矩阵/升级矩阵/注入
套件;错误码断言 ApprovalRequired×29 等)。**无 happy-path-only 模块**。缺口
(P2):autorun send_failed 分支(task_ops.rs:1698-1704)、exec_log 复扫降格分支
(exec_log.rs:111-118)无测试。

## §4 遗留台账盘点

M1-M9 各 review §6 展开到最小粒度共 **72 条**:

```text
已闭合 31 条(43%)——主闭合波次:M2 混沌/性能、M7 收编模型调用、
              M8 Judge/取消/级联删除、M9 了结 M8 三项
部分闭合 6 条——Web 观感、approval.list UI、预算 reservation、审批交互形态、
              删除权全量(备份加密/全局删除缺)、Team 富交互
悬置 35 条——其中约 8 条属「写明随某里程碑却未接住」的掉链项:
              WAL checkpoint 定标(M2-3/M3-5)、bus.resumed 发射点(M4-2a)、
              CLI capability list(M4-2c)、Orchestrator 等价复测(M5-1)、
              Task 级配置字段(M6-1c)、max_concurrent_tools(M6-1d)、
              多代进度聚合(M7-5)、capability 操作留档复核(M4-4a)
```

**需用户拍板的独立决策 7 项**(互有绑定,大白话裁决单见 PENDING.md):
D-M2-1/D-M2-2 追认(执行已闭,追认未收口)、D-M5-1 停滞窗口数值、
D-M2-3 GT-01 Patch、D-M3-1 web 观感、D-M3-2/D-M8-3 桌面包三选一、
远程 MCP 启动时机(方向已拍板)。

**审计台账另册**:A-06~A-09 四条待批处理 + R-14 六子项待逐条复核 + 演进/条件
清账登记项——不在 72 条内,属下轮批处理队列。

**台账卫生问题**(本回看发现):PENDING 条目状态滞后(D-M2-1/D-M2-2 执行已闭
未收口、D-M3-1 表述过时);AGENTS.md 文件地图仍写 ADR-0001..0009(实为 0011);
M8 交接清单漏接 8 条掉链项。**状态索引随本回看修正,追认类不改判、仍待用户。**

## §5 本轮新发现问题登记(F 系,已入审计台账)

| 编号 | 级别 | 内容 | 处置 |
|---|---|---|---|
| F-01 | P1 | exec_log I/O expect 可被外部条件触发 panic | 登记待批(修复小,可随下轮批处理) |
| F-02 | P2 | 投影写静默失败无诊断(save_idem_receipt 幂等抑制失效风险) | 登记(加失败日志钩子) |
| F-03 | P2 | glm_http SECRET_BRIDGE 全局静态无人接线(死代码/潜在旁路面) | 登记(删或接线,二选一) |
| F-04 | P2 | store.rs 位点 meta 损坏静默按 0 | 登记 |
| F-05 | P2 | turn.rs/task_ops.rs 重构债(200+ 行函数 ×5、重复守卫 ×6、budget 解析 ×3) | 登记缓办 |
| F-06 | 架构 | C4 模型漂移清单(§2.2) | 登记待裁决(阶段二开工前回写) |
| F-07 | 架构 | bm-surface-http→bm-persist 直依赖(只读穿透) | 登记(裁决收口或留档) |
| F-08 | 架构 | /admin/cli spawn+令牌注入权力面未入 C4 | 随 F-06 一并回写 |
| F-09 | 流程 | S1/S2/S6/S7/S10 五条长期 proposed 无人裁决;S5 裁决口径偏宽 | 随下一里程碑回看逐条裁决 |
| F-10 | P2 | autorun send_failed / exec_log 降格占位分支无测试 | 登记随批处理补 |
| F-11 | 治理 | memory_drawer_verdict 与 ADR-0006 口径张力 | 候选:发 ADR-0012 补记 |

## §6 回看七问(基线 §19)

1. **新增能力是否解决了目标问题?** 是。M1-M9 主线(最小闭环→持久化→Surface→
   权限→Butler/Task→Team→Provider/MCP→真实 App→自主环/流式/记忆授权)逐级
   可用,31/72 遗留闭合、四个 P0 修复均带复现验收。
2. **旧能力是否仍然可用?** 是。260 测试含全量回归,默认配置 golden trace 零变化
   (M9-review §5-3 为最近一次证明)。
3. **崩溃/断线/重复执行会怎样?** 恢复路径有 M2 四混沌验收 + R-03 水位修复 +
   t161 取消×撤审防护;薄弱面:投影写静默失败(F-02)与位点 meta 按 0(F-04)
   在极端窗口下行为不透明。
4. **日志能否解释每一步?** 事件面能;Execution Log 的 I/O panic 面(F-01)是
   唯一实质缺口。
5. **结果是否被实际观察核验?** 是。t144 实网流式 + 真实浏览器手测(M9)、
   t116 实网 6 回合(M8)为最近实证;审计纪律「不许说满话」持续执行。
6. **合同与状态模型是否稳定?** 是。v1.0 冻结至今只增不破(2 事件 ×1 方法 ×
   载荷键,均 Minor);迁移表单一来源。
7. **继续推进还是退回?** **继续**,带 §7 条件。架构红线主体干净,无需退回;
   但模型回写(F-06)不宜再拖过下一批。

## §7 裁决与下一步

**passed_with_conditions**,条件三件:

1. **F-01(exec_log panic 面)随下一轮批处理修复**——唯一 P1 级代码风险。
2. **C4 模型回写(F-06/F-08)列阶段二下一批开工前置**——按 ADR-0008 程序纪律;
   回写取舍(超前容器标注 vs 删除)届时随规格裁决。
3. **7 项用户拍板项维持登记**,真实使用一周的反馈仍是下一批排序的第一输入。

下一步不变(与 M9-review §6 一致):真实使用攒手感 → 远程 MCP(用户已拍板
下一批)→ 自主环真工具闭环 / web 版调整。本回看的 F 系登记不改变该排序,
仅作为下轮批处理与开工前置的输入。

## §8 用户拍板结果(2026-08-30,大白话四问;§4 的 7 项待拍板全部落定)

| 事项 | 裁决 |
|---|---|
| 下一批(远程 MCP)时机 | **先真实使用一周再开工**(维持既定策略) |
| 桌面安装包(D-M8-3/D-M3-2) | **以后再说,骨架保留**(搁置不排期,非取消) |
| 看护闹钟数值(D-M5-1) | **15 分钟 / 3 次 / 24 小时维持**,定案 |
| 三笔欠签字项 | **全部追认**:D-M2-1(混沌四项映射)、D-M2-2(编排重启)、
D-M2-3(GT-01 示例修正——已当场执行:GT-01:87 改 `secret:model.zhipu`,
validate 全绿 + 260 测试复核通过) |

D-M3-1(web 观感)随桌面包裁决一并维持搁置;PENDING.md 各条目已按上表
收口。待拍板队列至此清零,悬置项仅剩执行类(批处理队列与里程碑候选)。
