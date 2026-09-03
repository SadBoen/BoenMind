# 未结事项总台账(BACKLOG)

> 定位:全仓「还欠什么」的**唯一入口**(ADR-0015 确立)。
> **维护口径(2026-09-02 用户裁决更新)**:已闭合条目**移出台账不再留行**,闭合史可溯 git 提交史与 `milestones/HISTORY.md`;台账只留未结项,每项须有来源与状态;裁决「不做」的进 §7 备忘防重开。
> 状态口径:OPEN=未动工 / DEFERRED=用户拍板后置 / INPROGRESS=进行中。

## 1. 新工进行与待审批次

| 条目 | 范围与口径 | 状态 |
|---|---|---|
| 直通工具对话链路断(实测 P1) | 2026-09-03 VPS 实测(bm.sadinsun.top v0.0.2):聊天内只读直通工具(system.echo/counter.bump)必现 60s「工具执行超时」→ 回合失败。根因=turn.rs 工具轮对直通调用只轮询 op_results,而同步直通结果内联在收据里、从不入该表(仅异步回单 turn.rs:1118 与审批重放 handlers.rs:641 两路写入;GetOpResult 只读 runtime.rs:705)。v0.0.2 与当前 HEAD 同病;审批类工具全链路实测正常。修向=收据 state=succeeded 时直取内联 result(或 capability_call_inner 同步入表)+补对话内直通工具 E2E(现测试全走 capability_call_inner 直调,恰绕过此断点) | OPEN(待排期) |
| 模型调用 30 秒硬顶(实测 P1) | 同轮实测:DEFAULT_TURN_TIMEOUT_SECS=30(runtime.rs:40,两服务端二进制写死不可配)作每次模型调用 deadline;实测 mimo-v2.5 常规调用 12~29s,超 30s 即整回合失败(err internal/unavailable),用户自己 00:12 的纯聊天回合也中招。修向=默认值上调(≥120s)或出配置(env/设置页) | OPEN(待排期) |
| 失败回合后 agent 卡死(实测 P2) | 同轮实测:回合失败后 agent 停留 Failed,同会话再发消息必 500「agent 不在可接单状态」,UI 无恢复路径只能新建对话。修向=失败落定回 Idle 或 send_input 对 Failed 自愈 | OPEN(待排期) |
| 模型自编工具结果(mimo 质量备忘) | 同轮实测:问 counter.bump 时模型未发起调用直接编造「bumped successfully」(/admin/context 证实 0 工具轮);对话区无 [调用] 标记即可辨真伪,强提示词可压不断根;随模型侧观察,不立项 | OPEN(记录在案) |
| Skill v0.2 第二步(scripts 执行面) | 第一步(合同 Minor: version + references)与 ADR-0016(Broker 七步管线覆盖脚本设计)已闭合交付;**第二步**:等待用户审阅确认 ADR-0016 后接入 wasmtime 执行引擎写代码 | OPEN(待 ADR-0016 确认后动工) |
| W8 遗留:能力执行 cwd 注入 | ADR-0018 只做到回合 system prompt 注入;MCP/context-mode 等需要 cwd 的能力执行面尚未消费会话绑定工作区(该插件默认也未启用);与 Skill v0.2 执行线同批评估,继续经 Broker 管线、不新增特权通道 | OPEN(依 ADR-0016/0017 排期) |
| W8 遗留:workspace_id 跨重启持久 | 现为进程内会话作用域(ADR-0018 决策 3:Web 会话指针重启即失效,持久化无用户可见收益);待会话列表真数据(W 后续)一并评估是否随会话恢复 | OPEN |

## 2. 流程收尾

| 条目 | 来源 | 状态 |
|---|---|---|
| F-09 deepwiki S1/S2/S6/S7/S10 逐条裁决;S5 口径偏宽复核(t119b≠quarantined 分表) | AUDIT F-09 + FULL-REVIEW §2.4 | OPEN(随下一里程碑回看) |
| W4 验收记录回填(按规格 §4 验收门补实测证据与截图) | W4-implementation-spec | OPEN |

## 3. 技术债(缓办,均不影响日常使用)

### 3.1 掉链项(前序回看承诺「随某里程碑补」未接住;来源 FULL-REVIEW §4)

| 条目 | 来源 | 状态 |
|---|---|---|
| WAL checkpoint 策略定标 | M2-review §6-3 → M3-review §6-5 | OPEN |
| bus.resumed 发射点 | M4-review §6-2 | OPEN |
| CLI capability list 命令 | M4-review §6-2 | OPEN |
| Orchestrator 单进程等价映射复测(独立进程形态) | M5-review §6-1 | OPEN |
| Task 级停滞窗口/并发上限配置字段 | M6-review §6-1(M5 部分闭合) | OPEN |
| 成员级 max_concurrent_tools 定标 | M6-review §6-1 | OPEN |
| 多代 stdio 进度聚合 | M7-review §6-5 | OPEN |
| capability 操作不落 operations 表复核 | M4-review §6-4 | OPEN |
| P-06 RSS 独立采样接入 perf 套件 | M1 §6-2 → M2 → M3 §6-4 后无接续 | OPEN |

### 3.2 后端与前端重构债

| 条目 | 说明 | 状态 |
|---|---|---|
| F-05 | 200+ 行函数重构债(与 L-01/R-08 同批) | OPEN(缓办) |
| F-07 | bm-surface-http → bm-persist 直依赖待裁决(收口或留档) | OPEN |
| F-11 | memory_drawer_verdict 硬编码权限规则与 ADR-0006 张力(broker.rs 已补注;合同化重构待排期) | OPEN |
| P3 大文件拆分 | broker.rs(1657 行)/turn.rs(1694)/task_ops.rs(1710)/sqlite_state.rs(1205)/webadmin.rs(1882 行);broker 建议拆法=mod+policy(GrantLedger)/credential/executor/audit | OPEN(缓办) |
| P4 非测试 unwrap 甄别清理 | 全仓约 400 处 unwrap 需区分测试/非测试逐步替换;非测试 panic 10 处均系不变量断言,评估=维持现状 | OPEN(缓办) |
| P5 Capability 抽象演进 | 同步 invoke 无超时护栏(trait 注释已写明选型约束)/错误 String→结构化枚举/统一单 async trait 评估留 M 系列回看 | OPEN(缓办) |
| McpPage.tsx 组件解耦 | 714 行拆子表单/子对话框 | OPEN |
| 前端静态分析 | ESLint + Stylelint 接入 CI | OPEN |
| theme.css !important 收敛 | 玻璃段 4 处(毛玻璃化刻意选型,收敛须换实现手法) | OPEN(低) |
| CustomEvent 类型化 | 5 文件发事件/12 文件监听,可随 ESLint 批次一并 | OPEN(低) |

### 3.3 低优杂项

| 条目 | 说明 | 状态 |
|---|---|---|
| PENPOT-quickstart 过时 | 指向已删除的 runtime/web/tokens.css;已加过时标注,待归档或更新 | OPEN(低) |
| MCP 子进程 stderr 采集 | 现为 `Stdio::inherit()` 直通 server.log(W2 刻意诊断选型);后续可管道采集入插件页 | OPEN(低) |
| allowed_capabilities 措辞澄清 | 合同已注明「仅提示面数据」;角色编辑 UI 宜加一句「不构成权限控制」防误解 | OPEN(低) |

## 4. 用户拍板后置(DEFERRED,裁决记录见 milestones/PENDING.md)

| 条目 | 来源 | 状态 |
|---|---|---|
| S4 draining 两步摘除实测 | M7 §6-1 → M8 §6-4 → M9 §6 | DEFERRED(留档后置) |
| lease 通道真实吞吐实测 | M4 §6-1 → M8 §6-4 | DEFERRED(留档后置) |
| 桌面安装包(`cargo tauri build` 出 .exe) | M3 §6-1 → M8 §6-1(D-M8-3) | DEFERRED(搁置不排期,骨架在 shell/tauri) |
| memory 条目级删除所有权 | M9-review §6-2 | DEFERRED(演进项) |

## 5. 候选队列(用户提过、未排期)

| 条目 | 范围与口径 | 状态 |
|---|---|---|
| context-mode Rust 后续安全执行面 | 将当前可选宿主执行替换/增强为真正的 wasmtime/WASI 或操作系统级沙箱；补齐上游客户端 hooks、skills、网络抓取的 BoenMind 适配（首版仅迁移 MCP 核心） | OPEN(依 ADR-0017 与 ADR-0016 后续排期) |

- 使用反馈轮:当前阶段,按「先真实使用一周」节奏收集反馈;
- App 产品面完整设计(manifest ui_panels 自动导航/数据域隔离/生命周期):远期,音乐播放器 App 先行探路。

## 6. 已闭合备忘(留出处,勿重开)

- **内置能力全量 MCP 化 = 用户裁决不搞(2026-09-02)**;复核意见=同步 trait 快路径系刻意选型(零进程开销),全量子进程化对单用户场景属倒退;热插拔诉求由 §1「MCP 完整热插拔」承接。
- 合同 Minor 三笔:tools maxItems 0→16 / finish_reason 加 tool_calls / invoke_response 加 tool_calls(commit 40988a7,W4)。
- PENDING.md 的 11 条 D-M 系列 user 裁决全部闭合(见 milestones/PENDING.md 表)。
- S5(S4 部分实现)、S9(verification 三分法,M5/M7)裁决见 FULL-REVIEW §2.4。
- 其余已交付/已修复条目(审计 F-01~F-04/F-10、W4b 三件套、上下文透视面板、前端治理两轮、api_dsh 移除、release 线重写、坏 MCP 条目容错、冒烟套件等)已按 2026-09-02 维护口径移出台账,可溯 git 史与 HISTORY.md。
